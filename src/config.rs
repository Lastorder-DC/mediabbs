//! Application configuration, read once from environment variables (and `.env`)
//! at startup. Invalid configuration aborts startup with a descriptive message
//! instead of panicking later in a request handler.
//!
//! See `.env.example` for every variable and its default.

use crate::connection_info::TrustProxy;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    /// Empty means "allow any origin" (not recommended in production).
    pub cors_allowed_origins: Vec<String>,
    pub trust_proxy: TrustProxy,
    pub sentry_dsn: Option<String>,
    pub mw: MwConfig,
    pub oauth: OauthConfig,
    pub admin: AdminConfig,
    pub cookie: CookieConfig,
    pub site: SiteConfig,
}

#[derive(Debug, Clone)]
pub struct MwConfig {
    /// Public base URL of the wiki, e.g. `https://wiki.example.org` (no trailing slash).
    pub base_url: String,
    /// `$wgScriptPath`, e.g. `/w` or empty (no trailing slash).
    pub script_path: String,
    /// `api.php` URL used by the backend (may be an internal address).
    pub api_url: String,
    /// `rest.php` URL used by the backend (may be an internal address).
    pub rest_url: String,
    /// Public `api.php` URL, handed to browsers.
    pub public_api_url: String,
    /// Public `rest.php` URL, handed to browsers (OAuth authorize endpoint).
    pub public_rest_url: String,
    /// Absolute article URL template containing `$1`. `None` = discover via siteinfo.
    pub article_path: Option<String>,
    pub check_ip_blocks: bool,
    pub user_agent: String,
}

impl MwConfig {
    pub fn token_url(&self) -> String {
        format!("{}/oauth2/access_token", self.rest_url)
    }
    pub fn profile_url(&self) -> String {
        format!("{}/oauth2/resource/profile", self.rest_url)
    }
    pub fn authorize_url(&self) -> String {
        format!("{}/oauth2/authorize", self.public_rest_url)
    }
    /// Article URL template used when neither `MW_ARTICLE_PATH` nor siteinfo is available.
    pub fn fallback_article_path(&self) -> String {
        format!("{}{}/index.php?title=$1", self.base_url, self.script_path)
    }
}

#[derive(Debug, Clone)]
pub struct OauthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub public_key_path: String,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub groups: Vec<String>,
    pub rights: Vec<String>,
}

impl AdminConfig {
    pub fn is_admin(&self, groups: &[String], rights: &[String]) -> bool {
        groups.iter().any(|g| self.groups.contains(g))
            || rights.iter().any(|r| self.rights.contains(r))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

#[derive(Debug, Clone)]
pub struct CookieConfig {
    pub domain: Option<String>,
    pub secure: bool,
    pub same_site: SameSite,
    pub max_age_days: i64,
}

#[derive(Debug, Clone)]
pub struct SiteConfig {
    /// `None` = derive from the wiki's sitename.
    pub name: Option<String>,
    pub locale: String,
    /// `None` = first active board.
    pub default_board: Option<String>,
    /// Extra navigation links for the client, as a JSON array (passed through).
    pub nav_links: serde_json::Value,
    pub recent_changes_enabled: bool,
    /// `None` = derive from siteinfo.
    pub document_namespaces: Option<Vec<i32>>,
    /// `None` = derive from siteinfo.
    pub discussion_namespaces: Option<Vec<i32>>,
    pub sitenotice_enabled: bool,
    pub sitenotice_page: String,
    pub boards_cache_seconds: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Builds the configuration from an arbitrary key lookup (used by tests).
    pub fn from_lookup<F>(get: F) -> Result<Self, String>
    where
        F: Fn(&str) -> Option<String>,
    {
        // Treat empty values as unset.
        let get = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let required = |key: &str| get(key).ok_or_else(|| format!("{} is not set", key));
        let get_any = |keys: &[&str]| keys.iter().find_map(|k| get(k));
        let bool_or = |key: &str, default: bool| -> Result<bool, String> {
            match get(key) {
                None => Ok(default),
                Some(v) => parse_bool(&v).ok_or_else(|| format!("{} must be true or false", key)),
            }
        };
        let list = |key: &str| -> Vec<String> {
            get(key).map(|v| split_list(&v, &[','])).unwrap_or_default()
        };

        // --- server ---
        let bind_address = get_any(&["BIND_ADDRESS", "HOST"])
            .ok_or("BIND_ADDRESS is not set (e.g. 127.0.0.1:8080)")?;
        let database_url = required("DATABASE_URL")?;
        let cors_allowed_origins = get_any(&["CORS_ALLOWED_ORIGINS", "CORS"])
            .map(|v| split_list(&v, &[',', '|']))
            .unwrap_or_default();
        let trust_proxy = TrustProxy::parse(get("TRUST_PROXY").as_deref())
            .map_err(|e| format!("TRUST_PROXY: {}", e))?;
        let sentry_dsn = get_any(&["SENTRY_DSN", "SENTRY_URL"]);

        // --- MediaWiki ---
        let base_url = required("MW_BASE_URL")?.trim_end_matches('/').to_owned();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err("MW_BASE_URL must start with http:// or https://".into());
        }
        let script_path = normalize_path(&get("MW_SCRIPT_PATH").unwrap_or_default());
        let public_api_url = format!("{}{}/api.php", base_url, script_path);
        let public_rest_url = format!("{}{}/rest.php", base_url, script_path);
        let api_url = get("MW_API_URL")
            .map(|v| v.trim_end_matches('/').to_owned())
            .unwrap_or_else(|| public_api_url.clone());
        let rest_url = get("MW_REST_URL")
            .map(|v| v.trim_end_matches('/').to_owned())
            .unwrap_or_else(|| public_rest_url.clone());
        let article_path = match get("MW_ARTICLE_PATH") {
            None => None,
            Some(p) if !p.contains("$1") => {
                return Err("MW_ARTICLE_PATH must contain $1 (e.g. /wiki/$1)".into())
            }
            Some(p) if p.starts_with("http://") || p.starts_with("https://") => Some(p),
            Some(p) => Some(format!("{}{}", base_url, normalize_path(&p))),
        };
        let mw = MwConfig {
            base_url,
            script_path,
            api_url,
            rest_url,
            public_api_url,
            public_rest_url,
            article_path,
            check_ip_blocks: bool_or("CHECK_IP_BLOCKS", true)?,
            user_agent: get("MW_USER_AGENT").unwrap_or_else(|| {
                format!(
                    "mediabbs/{} (+https://github.com/Lastorder-DC/mediabbs)",
                    env!("CARGO_PKG_VERSION")
                )
            }),
        };

        // --- OAuth ---
        let oauth = OauthConfig {
            client_id: required("OAUTH_CLIENT_ID")?,
            client_secret: required("OAUTH_CLIENT_SECRET")?,
            public_key_path: get("OAUTH_PUBLIC_KEY_PATH").unwrap_or_else(|| "pubkey.pem".into()),
        };

        // --- admin ---
        let admin = AdminConfig {
            groups: get("ADMIN_GROUPS")
                .map(|v| split_list(&v, &[',']))
                .unwrap_or_else(|| vec!["sysop".to_owned()]),
            rights: list("ADMIN_RIGHTS"),
        };

        // --- cookies ---
        let same_site = match get("COOKIE_SAMESITE")
            .map(|v| v.to_ascii_lowercase())
            .as_deref()
        {
            None | Some("lax") => SameSite::Lax,
            Some("strict") => SameSite::Strict,
            Some("none") => SameSite::None,
            Some(_) => return Err("COOKIE_SAMESITE must be Lax, Strict or None".into()),
        };
        let secure = bool_or("COOKIE_SECURE", true)?;
        if same_site == SameSite::None && !secure {
            return Err("COOKIE_SAMESITE=None requires COOKIE_SECURE=true".into());
        }
        let max_age_days = match get("COOKIE_MAX_AGE_DAYS") {
            None => 28,
            Some(v) => v
                .parse::<i64>()
                .ok()
                .filter(|d| *d > 0)
                .ok_or("COOKIE_MAX_AGE_DAYS must be a positive integer")?,
        };
        let cookie = CookieConfig {
            domain: get("COOKIE_DOMAIN"),
            secure,
            same_site,
            max_age_days,
        };

        // --- site / client ---
        let nav_links = match get("NAV_LINKS") {
            None => serde_json::Value::Array(vec![]),
            Some(v) => {
                let value: serde_json::Value = serde_json::from_str(&v)
                    .map_err(|e| format!("NAV_LINKS is not valid JSON: {}", e))?;
                if !value.is_array() {
                    return Err("NAV_LINKS must be a JSON array".into());
                }
                value
            }
        };
        let namespaces = |key: &str| -> Result<Option<Vec<i32>>, String> {
            match get(key) {
                None => Ok(None),
                Some(v) => split_list(&v, &[',', '|'])
                    .iter()
                    .map(|x| x.parse::<i32>())
                    .collect::<Result<Vec<_>, _>>()
                    .map(Some)
                    .map_err(|_| format!("{} must be a list of namespace numbers", key)),
            }
        };
        let site = SiteConfig {
            name: get("SITE_NAME"),
            locale: get("SITE_LOCALE").unwrap_or_else(|| "ko-KR".into()),
            default_board: get("DEFAULT_BOARD"),
            nav_links,
            recent_changes_enabled: bool_or("RECENT_CHANGES_ENABLED", true)?,
            document_namespaces: namespaces("RECENT_CHANGES_NAMESPACES")?,
            discussion_namespaces: namespaces("RECENT_DISCUSSIONS_NAMESPACES")?,
            sitenotice_enabled: bool_or("SITENOTICE_ENABLED", true)?,
            sitenotice_page: get("SITENOTICE_PAGE")
                .unwrap_or_else(|| "MediaWiki:Sitenotice".into()),
            boards_cache_seconds: match get("BOARDS_CACHE_SECONDS") {
                None => 300,
                Some(v) => v
                    .parse()
                    .map_err(|_| "BOARDS_CACHE_SECONDS must be a non-negative integer")?,
            },
        };

        Ok(Config {
            bind_address,
            database_url,
            cors_allowed_origins,
            trust_proxy,
            sentry_dsn,
            mw,
            oauth,
            admin,
            cookie,
            site,
        })
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn split_list(v: &str, separators: &[char]) -> Vec<String> {
    v.split(|c| separators.contains(&c))
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `""` -> `""`, `"w"` -> `"/w"`, `"/w/"` -> `"/w"`.
fn normalize_path(p: &str) -> String {
    let p = p.trim().trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') {
        p.to_owned()
    } else {
        format!("/{}", p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let mut map: HashMap<String, String> = [
            ("BIND_ADDRESS", "127.0.0.1:8080"),
            ("DATABASE_URL", "mysql://u:p@localhost/db"),
            ("MW_BASE_URL", "https://wiki.example.org/"),
            ("OAUTH_CLIENT_ID", "id"),
            ("OAUTH_CLIENT_SECRET", "secret"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        for (k, v) in pairs {
            map.insert(k.to_string(), v.to_string());
        }
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn test_defaults() {
        let c = config(&[]).unwrap();
        assert_eq!(c.mw.base_url, "https://wiki.example.org");
        assert_eq!(c.mw.api_url, "https://wiki.example.org/api.php");
        assert_eq!(
            c.mw.token_url(),
            "https://wiki.example.org/rest.php/oauth2/access_token"
        );
        assert_eq!(c.mw.article_path, None);
        assert_eq!(
            c.mw.fallback_article_path(),
            "https://wiki.example.org/index.php?title=$1"
        );
        assert!(c.mw.check_ip_blocks);
        assert_eq!(c.admin.groups, vec!["sysop"]);
        assert_eq!(c.cookie.same_site, SameSite::Lax);
        assert!(c.cookie.secure);
        assert_eq!(c.cookie.max_age_days, 28);
        assert_eq!(c.oauth.public_key_path, "pubkey.pem");
        assert!(c.cors_allowed_origins.is_empty());
        assert_eq!(c.sentry_dsn, None);
        assert_eq!(c.site.boards_cache_seconds, 300);
        assert_eq!(c.trust_proxy, TrustProxy::default_loopback());
    }

    #[test]
    fn test_script_path_and_overrides() {
        let c = config(&[
            ("MW_SCRIPT_PATH", "w/"),
            ("MW_REST_URL", "http://mediawiki:80/w/rest.php/"),
            ("MW_ARTICLE_PATH", "/wiki/$1"),
        ])
        .unwrap();
        assert_eq!(c.mw.script_path, "/w");
        assert_eq!(c.mw.api_url, "https://wiki.example.org/w/api.php");
        assert_eq!(
            c.mw.profile_url(),
            "http://mediawiki:80/w/rest.php/oauth2/resource/profile"
        );
        // Browsers always get the public URL
        assert_eq!(
            c.mw.authorize_url(),
            "https://wiki.example.org/w/rest.php/oauth2/authorize"
        );
        assert_eq!(
            c.mw.article_path.as_deref(),
            Some("https://wiki.example.org/wiki/$1")
        );
    }

    #[test]
    fn test_legacy_names() {
        let c = config(&[
            ("HOST", "0.0.0.0:9000"),
            ("CORS", "https://a.example|https://b.example"),
            ("SENTRY_URL", "https://key@sentry.example/1"),
        ]);
        // BIND_ADDRESS from defaults wins over HOST
        assert_eq!(c.unwrap().bind_address, "127.0.0.1:8080");
        let mut map: HashMap<String, String> = HashMap::new();
        for (k, v) in &[
            ("HOST", "0.0.0.0:9000"),
            ("DATABASE_URL", "mysql://x"),
            ("MW_BASE_URL", "https://w.example"),
            ("OAUTH_CLIENT_ID", "id"),
            ("OAUTH_CLIENT_SECRET", "s"),
            ("CORS", "https://a.example|https://b.example"),
            ("SENTRY_URL", "https://key@sentry.example/1"),
        ] {
            map.insert(k.to_string(), v.to_string());
        }
        let c = Config::from_lookup(|k| map.get(k).cloned()).unwrap();
        assert_eq!(c.bind_address, "0.0.0.0:9000");
        assert_eq!(
            c.cors_allowed_origins,
            vec!["https://a.example", "https://b.example"]
        );
        assert_eq!(
            c.sentry_dsn.as_deref(),
            Some("https://key@sentry.example/1")
        );
    }

    #[test]
    fn test_admin_and_lists() {
        let c = config(&[
            ("ADMIN_GROUPS", "sysop, boardmanager"),
            ("ADMIN_RIGHTS", "block"),
            ("RECENT_CHANGES_NAMESPACES", "0|4|10"),
            (
                "NAV_LINKS",
                r#"[{"label":"Maps","url":"https://maps.example"}]"#,
            ),
        ])
        .unwrap();
        assert!(c.admin.is_admin(&["boardmanager".into()], &[]));
        assert!(c.admin.is_admin(&[], &["block".into()]));
        assert!(!c.admin.is_admin(&["user".into()], &["read".into()]));
        assert_eq!(c.site.document_namespaces, Some(vec![0, 4, 10]));
        assert_eq!(c.site.nav_links[0]["label"], "Maps");
    }

    #[test]
    fn test_errors() {
        assert!(config(&[("MW_BASE_URL", "wiki.example.org")]).is_err());
        assert!(config(&[("MW_ARTICLE_PATH", "/wiki/")]).is_err());
        assert!(config(&[("COOKIE_SAMESITE", "None"), ("COOKIE_SECURE", "false")]).is_err());
        assert!(config(&[("COOKIE_SAMESITE", "sometimes")]).is_err());
        assert!(config(&[("CHECK_IP_BLOCKS", "maybe")]).is_err());
        assert!(config(&[("NAV_LINKS", "{}")]).is_err());
        assert!(config(&[("TRUST_PROXY", "not-an-ip")]).is_err());
        assert!(config(&[("RECENT_CHANGES_NAMESPACES", "0|x")]).is_err());
        assert!(config(&[("OAUTH_CLIENT_SECRET", "")]).is_err());
    }
}
