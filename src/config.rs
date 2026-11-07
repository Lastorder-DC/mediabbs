//! Application configuration, read once from environment variables (and `.env`)
//! at startup. Invalid configuration aborts startup with a descriptive message
//! instead of panicking later in a request handler.
//!
//! See `.env.example` for every variable and its default.

use crate::connection_info::{ForwardedHeader, TrustProxy};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    /// Apply pending migrations when the server starts.
    pub migrate_on_start: bool,
    /// Empty means "allow any origin" (not recommended in production).
    pub cors_allowed_origins: Vec<String>,
    pub trust_proxy: TrustProxy,
    pub forwarded_header: ForwardedHeader,
    pub sentry_dsn: Option<String>,
    pub mw: MwConfig,
    pub oauth: OauthConfig,
    pub admin: AdminConfig,
    pub cookie: CookieConfig,
    pub site: SiteConfig,
    pub upload: UploadConfig,
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
pub struct UploadConfig {
    pub enabled: bool,
    pub require_login: bool,
    /// Maximum decoded file size in bytes.
    pub max_bytes: usize,
    /// Lower-case extensions without dot.
    pub allowed_extensions: Vec<String>,
    /// URL prefix for stored files, ending with `/` (empty when uploads are disabled).
    pub public_url: String,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageConfig {
    Disabled,
    S3 {
        region: String,
        /// S3-compatible endpoint (MinIO, R2, ...). `None` = AWS.
        endpoint: Option<String>,
        bucket: String,
        /// Prepended to object keys, e.g. `bbs/`.
        key_prefix: String,
    },
    Local {
        dir: String,
    },
}

#[derive(Debug, Clone)]
pub struct SiteConfig {
    /// `None` = derive from the wiki's sitename.
    pub name: Option<String>,
    /// UI language/date locale (`SITE_LOCALE`); `None` lets the client pick
    /// (wiki language, then the browser).
    pub locale: Option<String>,
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
    /// Open live-update streams allowed at once (0 turns live updates off;
    /// clients then poll).
    pub live_updates_max_streams: usize,
    /// Topics, comments and uploads per client address per minute (0 = no limit).
    pub posts_per_minute: usize,
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
        // Treat empty values as unset; strip one layer of surrounding quotes.
        let get = |key: &str| get(key).map(|v| unquote(&v)).filter(|v| !v.is_empty());
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
        let migrate_on_start = bool_or("MIGRATE_ON_START", false)?;
        let cors_allowed_origins = get_any(&["CORS_ALLOWED_ORIGINS", "CORS"])
            .map(|v| split_list(&v, &[',', '|']))
            .unwrap_or_default();
        let trust_proxy = TrustProxy::parse(get("TRUST_PROXY").as_deref())
            .map_err(|e| format!("TRUST_PROXY: {}", e))?;
        let forwarded_header = ForwardedHeader::parse(get("FORWARDED_HEADER").as_deref())
            .map_err(|e| format!("FORWARDED_HEADER: {}", e))?;
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

        // --- uploads ---
        let upload_enabled = bool_or("UPLOAD_ENABLED", true)?;
        let storage = if !upload_enabled {
            StorageConfig::Disabled
        } else {
            match get("STORAGE_BACKEND")
                .map(|v| v.to_ascii_lowercase())
                .as_deref()
            {
                None | Some("s3") => {
                    let endpoint = get("S3_ENDPOINT").map(|e| e.trim_end_matches('/').to_owned());
                    let region = match (get("S3_REGION"), &endpoint) {
                        (Some(r), _) => r,
                        // Region is only used for request signing with custom endpoints.
                        (None, Some(_)) => "us-east-1".to_owned(),
                        (None, None) => {
                            return Err("S3_REGION is not set (or set UPLOAD_ENABLED=false)".into())
                        }
                    };
                    let bucket = get("S3_BUCKET")
                        .ok_or("S3_BUCKET is not set (or set UPLOAD_ENABLED=false)")?;
                    let key_prefix = match get("S3_KEY_PREFIX") {
                        None => "bbs/".to_owned(),
                        Some(p) if p == "/" => String::new(),
                        Some(p) => format!("{}/", p.trim_matches('/')),
                    };
                    StorageConfig::S3 {
                        region,
                        endpoint,
                        bucket,
                        key_prefix,
                    }
                }
                Some("local") => StorageConfig::Local {
                    dir: get("LOCAL_UPLOAD_DIR").unwrap_or_else(|| "./uploads".into()),
                },
                Some(_) => return Err("STORAGE_BACKEND must be s3 or local".into()),
            }
        };
        let public_url = match (get("UPLOAD_PUBLIC_URL"), &storage) {
            (_, StorageConfig::Disabled) => String::new(),
            (Some(url), _) => format!("{}/", url.trim_end_matches('/')),
            (
                None,
                StorageConfig::S3 {
                    endpoint: None,
                    region,
                    bucket,
                    ..
                },
            ) => format!("https://{}.s3.{}.amazonaws.com/", bucket, region),
            (None, _) => {
                return Err(
                    "UPLOAD_PUBLIC_URL is required for local storage and custom S3 endpoints"
                        .into(),
                )
            }
        };
        let upload = UploadConfig {
            enabled: upload_enabled,
            require_login: bool_or("UPLOAD_REQUIRE_LOGIN", true)?,
            max_bytes: match get("UPLOAD_MAX_BYTES") {
                None => 10 * 1024 * 1024,
                Some(v) => v
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("UPLOAD_MAX_BYTES must be a positive integer")?,
            },
            allowed_extensions: get("UPLOAD_ALLOWED_EXTENSIONS")
                .map(|v| {
                    split_list(&v, &[','])
                        .into_iter()
                        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                        .collect()
                })
                .unwrap_or_else(|| {
                    ["png", "jpg", "jpeg", "gif", "webp"]
                        .iter()
                        .map(|e| e.to_string())
                        .collect()
                }),
            public_url,
            storage,
        };
        // Types a browser can run script in. S3/CDN serve them as-is (the local
        // backend adds a sandbox CSP, but refuse them everywhere).
        const ACTIVE_TYPES: &[&str] = &[
            "svg", "svgz", "html", "htm", "xhtml", "xht", "xml", "xsl", "js", "mjs", "css", "pdf",
            "swf",
        ];
        if let Some(ext) = upload
            .allowed_extensions
            .iter()
            .find(|e| ACTIVE_TYPES.contains(&e.as_str()))
        {
            return Err(format!(
                "UPLOAD_ALLOWED_EXTENSIONS: '{}' files can carry scripts and cannot be allowed",
                ext
            ));
        }

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
            locale: get("SITE_LOCALE"),
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
            live_updates_max_streams: match get("LIVE_UPDATES_MAX_STREAMS") {
                None => 1000,
                Some(v) => v
                    .parse()
                    .map_err(|_| "LIVE_UPDATES_MAX_STREAMS must be a non-negative integer")?,
            },
            posts_per_minute: match get("POSTS_PER_MINUTE") {
                None => 10,
                Some(v) => v
                    .parse()
                    .map_err(|_| "POSTS_PER_MINUTE must be a non-negative integer")?,
            },
        };

        Ok(Config {
            bind_address,
            database_url,
            migrate_on_start,
            cors_allowed_origins,
            trust_proxy,
            forwarded_header,
            sentry_dsn,
            mw,
            oauth,
            admin,
            cookie,
            site,
            upload,
        })
    }
}

/// Trims whitespace and one pair of matching surrounding quotes.
///
/// `.env` files are parsed shell-style (quotes removed, `$VAR` expanded), while
/// `docker run --env-file` keeps values verbatim. Stripping quotes here lets the
/// same file work in both: values containing `"` or `$` (JSON, `/wiki/$1`) can
/// be written as `KEY='value'`.
fn unquote(v: &str) -> String {
    let v = v.trim();
    let bytes = v.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if first == last && (first == b'\'' || first == b'"') {
            return v[1..v.len() - 1].to_owned();
        }
    }
    v.to_owned()
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
            ("S3_REGION", "ap-northeast-2"),
            ("S3_BUCKET", "bucket"),
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
        assert_eq!(c.site.live_updates_max_streams, 1000);
        assert_eq!(c.site.posts_per_minute, 10);
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
            ("UPLOAD_ENABLED", "false"),
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
    fn test_upload() {
        let c = config(&[]).unwrap();
        assert!(c.upload.enabled && c.upload.require_login);
        assert_eq!(c.upload.max_bytes, 10 * 1024 * 1024);
        assert_eq!(
            c.upload.public_url,
            "https://bucket.s3.ap-northeast-2.amazonaws.com/"
        );
        assert_eq!(
            c.upload.storage,
            StorageConfig::S3 {
                region: "ap-northeast-2".into(),
                endpoint: None,
                bucket: "bucket".into(),
                key_prefix: "bbs/".into()
            }
        );
        let c = config(&[
            ("S3_REGION", ""),
            ("S3_ENDPOINT", "https://minio.example:9000/"),
            ("S3_KEY_PREFIX", "/images/"),
            ("UPLOAD_PUBLIC_URL", "https://cdn.example"),
            ("UPLOAD_ALLOWED_EXTENSIONS", ".PNG, jpg"),
        ])
        .unwrap();
        assert_eq!(c.upload.public_url, "https://cdn.example/");
        assert_eq!(c.upload.allowed_extensions, vec!["png", "jpg"]);
        assert_eq!(
            c.upload.storage,
            StorageConfig::S3 {
                region: "us-east-1".into(),
                endpoint: Some("https://minio.example:9000".into()),
                bucket: "bucket".into(),
                key_prefix: "images/".into()
            }
        );
        let c = config(&[
            ("STORAGE_BACKEND", "local"),
            ("UPLOAD_PUBLIC_URL", "https://api.example/uploads/"),
        ])
        .unwrap();
        assert_eq!(
            c.upload.storage,
            StorageConfig::Local {
                dir: "./uploads".into()
            }
        );
        let c = config(&[("UPLOAD_ENABLED", "false"), ("S3_BUCKET", "")]).unwrap();
        assert_eq!(c.upload.storage, StorageConfig::Disabled);
        assert!(config(&[("STORAGE_BACKEND", "local")]).is_err());
        assert!(config(&[("S3_ENDPOINT", "https://m.example")]).is_err());
        assert!(config(&[("S3_BUCKET", "")]).is_err());
        assert!(config(&[("STORAGE_BACKEND", "ftp")]).is_err());
    }

    #[test]
    fn test_unquote() {
        assert_eq!(unquote("  abc "), "abc");
        assert_eq!(unquote("'/wiki/$1'"), "/wiki/$1");
        assert_eq!(unquote("\"x\""), "x");
        assert_eq!(unquote("'mismatched\""), "'mismatched\"");
        assert_eq!(unquote("'"), "'");
        // As passed verbatim by `docker run --env-file`
        let c = config(&[
            ("NAV_LINKS", r#"'[{"label":"a","url":"https://x"}]'"#),
            ("MW_ARTICLE_PATH", "'/wiki/$1'"),
        ])
        .unwrap();
        assert_eq!(c.site.nav_links[0]["label"], "a");
        assert_eq!(
            c.mw.article_path.as_deref(),
            Some("https://wiki.example.org/wiki/$1")
        );
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
        assert!(config(&[("FORWARDED_HEADER", "x-real-ip")]).is_err());
        assert!(config(&[("UPLOAD_ALLOWED_EXTENSIONS", "png,SVG")]).is_err());
        assert!(config(&[("RECENT_CHANGES_NAMESPACES", "0|x")]).is_err());
        assert!(config(&[("OAUTH_CLIENT_SECRET", "")]).is_err());
    }
}
