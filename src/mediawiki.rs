//! HTTP client for the MediaWiki instance the board is attached to.
//!
//! All wiki URLs come from [`MwConfig`]; nothing here is specific to a wiki.

use crate::auth::Profile;
use crate::config::{Config, MwConfig};
use actix_web::{http::StatusCode, web::Bytes};
use anyhow::{anyhow, Result};
use awc::Client;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// The wiki rejected the access token (revoked, expired on the wiki side, ...).
/// Turned into `401 TokenInvalid` by [`crate::custom_error::CustomError`].
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("the wiki rejected the access token")]
pub struct WikiAuthError;

const SITEINFO_TTL: Duration = Duration::from_secs(60 * 60);
const SITEINFO_RETRY: Duration = Duration::from_secs(60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, Debug)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Serialize, Debug)]
struct TokenRequest<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<&'a str>,
}

/// Response of `rest.php/oauth2/resource/profile`.
///
/// `realname` and `email` are only returned when the OAuth consumer was granted
/// access to private information, and some fields may be missing depending on the
/// Extension:OAuth version, so everything except `sub`/`username` is optional.
#[derive(Debug, PartialEq, Deserialize)]
struct MwProfileResponse {
    #[serde(deserialize_with = "int_or_string")]
    sub: i32,
    username: String,
    #[serde(default)]
    editcount: Option<i32>,
    #[serde(default)]
    confirmed_email: bool,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    registered: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    rights: Vec<String>,
    #[serde(default)]
    grants: Vec<String>,
    #[serde(default)]
    realname: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Accepts `123`, `"123"` and prefixed strings such as `"mw:CentralAuth::123"`.
fn int_or_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntOrString {
        Int(i32),
        Str(String),
    }
    match IntOrString::deserialize(deserializer)? {
        IntOrString::Int(i) => Ok(i),
        IntOrString::Str(s) => crate::auth::parse_user_id(&s)
            .ok_or_else(|| de::Error::custom(format!("invalid user id: {}", s))),
    }
}

#[derive(Debug, Deserialize)]
struct MwBlocksResponse {
    query: MwBlocksResponseQuery,
}

#[derive(Debug, Deserialize)]
struct MwBlocksResponseQuery {
    blocks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MwSiteInfoResponse {
    query: MwSiteInfoQuery,
}

#[derive(Debug, Deserialize)]
struct MwSiteInfoQuery {
    general: MwSiteInfoGeneral,
    #[serde(default)]
    namespaces: HashMap<String, MwNamespace>,
}

#[derive(Debug, Deserialize)]
struct MwSiteInfoGeneral {
    sitename: String,
    server: String,
    articlepath: String,
    #[serde(default)]
    scriptpath: String,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MwNamespace {
    id: i32,
    #[serde(default)]
    content: bool,
}

/// Wiki metadata discovered through `action=query&meta=siteinfo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub site_name: String,
    /// Absolute server URL, e.g. `https://wiki.example.org`.
    pub server: String,
    /// Absolute article URL template, e.g. `https://wiki.example.org/wiki/$1`.
    pub article_path: String,
    pub script_path: String,
    /// Absolute logo URL.
    pub logo: Option<String>,
    pub lang: Option<String>,
    /// All namespace ids (>= 0), sorted.
    pub namespaces: Vec<i32>,
    /// Namespaces flagged as content namespaces, sorted.
    pub content_namespaces: Vec<i32>,
}

impl SiteInfo {
    fn from_response(resp: MwSiteInfoResponse, base_url: &str) -> Self {
        let general = resp.query.general;
        let scheme = if base_url.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        // `server` may be protocol-relative (`//wiki.example.org`).
        let server = absolute_url(scheme, base_url, &general.server)
            .trim_end_matches('/')
            .to_owned();
        let mut namespaces: Vec<i32> = resp
            .query
            .namespaces
            .values()
            .map(|ns| ns.id)
            .filter(|id| *id >= 0)
            .collect();
        namespaces.sort_unstable();
        let mut content_namespaces: Vec<i32> = resp
            .query
            .namespaces
            .values()
            .filter(|ns| ns.content)
            .map(|ns| ns.id)
            .collect();
        content_namespaces.sort_unstable();
        SiteInfo {
            site_name: general.sitename,
            article_path: absolute_url(scheme, &server, &general.articlepath),
            script_path: general.scriptpath,
            logo: general
                .logo
                .filter(|l| !l.is_empty())
                .map(|l| absolute_url(scheme, &server, &l)),
            lang: general.lang,
            server,
            namespaces,
            content_namespaces,
        }
    }

    /// Default namespaces for the "recent changes" panel: content namespaces plus
    /// Project, Template, Help and Category (when they exist).
    pub fn default_document_namespaces(&self) -> Vec<i32> {
        let mut result: Vec<i32> = self
            .namespaces
            .iter()
            .copied()
            .filter(|id| self.content_namespaces.contains(id) || [4, 10, 12, 14].contains(id))
            .collect();
        if result.is_empty() {
            result.push(0);
        }
        result
    }

    /// Default namespaces for the "recent discussions" panel: every talk namespace.
    pub fn default_discussion_namespaces(&self) -> Vec<i32> {
        self.namespaces
            .iter()
            .copied()
            .filter(|id| id % 2 == 1)
            .collect()
    }
}

/// Resolves a possibly relative / protocol-relative URL against `server`.
fn absolute_url(scheme: &str, server: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_owned()
    } else if url.starts_with("//") {
        format!("{}:{}", scheme, url)
    } else if url.starts_with('/') {
        format!("{}{}", server.trim_end_matches('/'), url)
    } else {
        format!("{}/{}", server.trim_end_matches('/'), url)
    }
}

struct SiteInfoCache {
    fetched_at: Instant,
    info: Option<SiteInfo>,
}

pub struct MwClient {
    config: Arc<Config>,
    siteinfo: RwLock<Option<SiteInfoCache>>,
}

impl MwClient {
    pub fn new(config: Arc<Config>) -> Self {
        MwClient {
            config,
            siteinfo: RwLock::new(None),
        }
    }

    fn mw(&self) -> &MwConfig {
        &self.config.mw
    }

    fn client(&self) -> Client {
        Client::builder()
            .add_default_header(("User-Agent", self.mw().user_agent.as_str()))
            .timeout(HTTP_TIMEOUT)
            .finish()
    }

    async fn request_token(&self, request: &TokenRequest<'_>) -> Result<Option<TokenPair>> {
        let mut res = self
            .client()
            .post(self.mw().token_url())
            .insert_header(("Accept", "application/json"))
            .send_form(request)
            .await
            .map_err(|e| anyhow!("token request failed: {}", e))?;
        if res.status() != StatusCode::OK {
            log::warn!("token request rejected by wiki: {}", res.status());
            return Ok(None);
        }
        let body: Bytes = res.body().limit(MAX_RESPONSE_BYTES).await?;
        Ok(Some(serde_json::from_slice(&body)?))
    }

    /// Exchanges an authorization code. `Ok(None)` when the wiki rejects it.
    pub async fn exchange_code(&self, code: &str) -> Result<Option<TokenPair>> {
        self.request_token(&TokenRequest {
            grant_type: "authorization_code",
            client_id: &self.config.oauth.client_id,
            client_secret: &self.config.oauth.client_secret,
            code: Some(code),
            refresh_token: None,
        })
        .await
    }

    /// Refreshes an access token. `Ok(None)` when the wiki rejects it.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Option<TokenPair>> {
        self.request_token(&TokenRequest {
            grant_type: "refresh_token",
            client_id: &self.config.oauth.client_id,
            client_secret: &self.config.oauth.client_secret,
            code: None,
            refresh_token: Some(refresh_token),
        })
        .await
    }

    pub async fn profile(&self, token: &str) -> Result<Profile> {
        let mut res = self
            .client()
            .get(self.mw().profile_url())
            .insert_header(("Accept", "application/json"))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .send()
            .await
            .map_err(|e| anyhow!("profile request failed: {}", e))?;
        if res.status() == StatusCode::UNAUTHORIZED || res.status() == StatusCode::FORBIDDEN {
            return Err(WikiAuthError.into());
        }
        if !res.status().is_success() {
            return Err(anyhow!("profile request returned {}", res.status()));
        }
        // MediaWiki may send an incorrect content-type header, so parse the body directly.
        let body: Bytes = res.body().limit(MAX_RESPONSE_BYTES).await?;
        let data: MwProfileResponse = serde_json::from_slice(&body)?;
        Ok(self.make_profile(data))
    }

    fn make_profile(&self, data: MwProfileResponse) -> Profile {
        let is_admin = self.config.admin.is_admin(&data.groups, &data.rights);
        Profile {
            id: data.sub,
            username: data.username,
            confirmed_email: data.confirmed_email,
            blocked: data.blocked,
            groups: data.groups,
            rights: data.rights,
            email: data.email.unwrap_or_default(),
            is_admin,
        }
    }

    /// Whether an IP address (or a range containing it) is blocked on the wiki.
    /// Always `false` when `CHECK_IP_BLOCKS=false`.
    pub async fn is_ip_blocked(&self, ip: &IpAddr) -> Result<bool> {
        if !self.mw().check_ip_blocks {
            return Ok(false);
        }
        let ip = ip.to_string();
        let mut res = self
            .client()
            .get(&self.mw().api_url)
            .query(&[
                ("action", "query"),
                ("list", "blocks"),
                ("bkip", ip.as_str()),
                ("bklimit", "1"),
                ("format", "json"),
            ])?
            .insert_header(("Accept", "application/json"))
            .send()
            .await
            .map_err(|e| anyhow!("block check failed: {}", e))?;
        let body: Bytes = res.body().limit(MAX_RESPONSE_BYTES).await?;
        let data: MwBlocksResponse = serde_json::from_slice(&body)?;
        Ok(!data.query.blocks.is_empty())
    }

    /// Cached wiki metadata. `None` when the wiki could not be reached; retried
    /// after a short delay.
    pub async fn siteinfo(&self) -> Option<SiteInfo> {
        if let Ok(guard) = self.siteinfo.read() {
            if let Some(cache) = guard.as_ref() {
                let ttl = if cache.info.is_some() {
                    SITEINFO_TTL
                } else {
                    SITEINFO_RETRY
                };
                if cache.fetched_at.elapsed() < ttl {
                    return cache.info.clone();
                }
            }
        }
        let info = match self.fetch_siteinfo().await {
            Ok(info) => Some(info),
            Err(e) => {
                log::warn!("failed to fetch siteinfo: {}", e);
                // Keep serving stale data if we have it.
                self.siteinfo
                    .read()
                    .ok()
                    .and_then(|g| g.as_ref().and_then(|c| c.info.clone()))
            }
        };
        if let Ok(mut guard) = self.siteinfo.write() {
            *guard = Some(SiteInfoCache {
                fetched_at: Instant::now(),
                info: info.clone(),
            });
        }
        info
    }

    async fn fetch_siteinfo(&self) -> Result<SiteInfo> {
        let mut res = self
            .client()
            .get(&self.mw().api_url)
            .query(&[
                ("action", "query"),
                ("meta", "siteinfo"),
                ("siprop", "general|namespaces"),
                ("format", "json"),
                ("formatversion", "2"),
            ])?
            .insert_header(("Accept", "application/json"))
            .send()
            .await
            .map_err(|e| anyhow!("siteinfo request failed: {}", e))?;
        let body: Bytes = res.body().limit(MAX_RESPONSE_BYTES).await?;
        let data: MwSiteInfoResponse = serde_json::from_slice(&body)?;
        Ok(SiteInfo::from_response(data, &self.mw().base_url))
    }

    /// Article URL template (`$1` = title): `MW_ARTICLE_PATH`, else siteinfo, else
    /// `index.php?title=$1`.
    pub fn article_path(&self, siteinfo: Option<&SiteInfo>) -> String {
        if let Some(p) = &self.mw().article_path {
            return p.clone();
        }
        match siteinfo {
            Some(info) => info.article_path.clone(),
            None => self.mw().fallback_article_path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_without_private_info() {
        let body = r#"{"sub":3,"username":"Foo","editcount":10,"confirmed_email":false,
            "blocked":false,"registered":"20210101000000","groups":["*","user"],
            "rights":["read"],"grants":["mwoauth-authonly"]}"#;
        let data: MwProfileResponse = serde_json::from_str(body).expect("must parse");
        assert_eq!(data.email, None);
        assert_eq!(data.realname, None);
        assert_eq!(data.groups, vec!["*", "user"]);
    }

    #[test]
    fn test_profile_sub_as_string() {
        let body = r#"{"sub":"mw:CentralAuth::5","username":"Foo"}"#;
        let data: MwProfileResponse = serde_json::from_str(body).expect("must parse");
        assert_eq!(data.sub, 5);
        let body = r#"{"sub":"abc","username":"Foo"}"#;
        assert!(serde_json::from_str::<MwProfileResponse>(body).is_err());
    }

    #[test]
    fn test_blocks_response() {
        let body = r#"{"batchcomplete":true,"query":{"blocks":[{"id":12}]}}"#;
        let data: MwBlocksResponse = serde_json::from_str(body).unwrap();
        assert_eq!(data.query.blocks.len(), 1);
    }

    #[test]
    fn test_absolute_url() {
        assert_eq!(
            absolute_url("https", "https://w.example", "//w.example"),
            "https://w.example"
        );
        assert_eq!(
            absolute_url("https", "https://w.example", "/wiki/$1"),
            "https://w.example/wiki/$1"
        );
        assert_eq!(
            absolute_url("https", "https://w.example/", "img/logo.png"),
            "https://w.example/img/logo.png"
        );
        assert_eq!(
            absolute_url("https", "https://w.example", "http://cdn.example/l.png"),
            "http://cdn.example/l.png"
        );
    }

    #[test]
    fn test_siteinfo_parse() {
        let body = r#"{"batchcomplete":true,"query":{"general":{
            "sitename":"Example Wiki","server":"//wiki.example.org",
            "articlepath":"/wiki/$1","scriptpath":"/w",
            "logo":"/w/resources/assets/wiki.png","lang":"en"},
          "namespaces":{
            "-1":{"id":-1,"name":"Special","content":false},
            "0":{"id":0,"name":"","content":true},
            "1":{"id":1,"name":"Talk","content":false},
            "2":{"id":2,"name":"User","content":false},
            "3":{"id":3,"name":"User talk","content":false},
            "4":{"id":4,"name":"Project","content":false},
            "6":{"id":6,"name":"File","content":false},
            "14":{"id":14,"name":"Category","content":false},
            "3000":{"id":3000,"name":"Custom","content":true},
            "3001":{"id":3001,"name":"Custom talk","content":false}}}}"#;
        let resp: MwSiteInfoResponse = serde_json::from_str(body).unwrap();
        let info = SiteInfo::from_response(resp, "https://wiki.example.org");
        assert_eq!(info.site_name, "Example Wiki");
        assert_eq!(info.server, "https://wiki.example.org");
        assert_eq!(info.article_path, "https://wiki.example.org/wiki/$1");
        assert_eq!(
            info.logo.as_deref(),
            Some("https://wiki.example.org/w/resources/assets/wiki.png")
        );
        assert_eq!(info.namespaces, vec![0, 1, 2, 3, 4, 6, 14, 3000, 3001]);
        assert_eq!(info.content_namespaces, vec![0, 3000]);
        assert_eq!(info.default_document_namespaces(), vec![0, 4, 14, 3000]);
        assert_eq!(info.default_discussion_namespaces(), vec![1, 3, 3001]);
    }
}
