//! `GET /config`: public, non-secret settings for mediabbs-client, so the client
//! build does not need to know anything about the wiki it is attached to.

use crate::config::Config;
use crate::db::DbPool;
use crate::mediawiki::MwClient;
use crate::models::SiteSetting;
use actix_web::{get, web, web::block, web::Data, HttpResponse, Scope};

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicConfig<'a> {
    /// Board name shown in the UI. `null` when neither SITE_NAME nor siteinfo is available.
    site_name: Option<String>,
    locale: Option<&'a str>,
    default_board: Option<&'a str>,
    wiki: PublicWikiConfig,
    oauth: PublicOauthConfig<'a>,
    nav_links: &'a serde_json::Value,
    recent_changes: PublicRecentChanges,
    site_notice: PublicSiteNotice<'a>,
    upload: PublicUpload<'a>,
    /// Menus saved from the admin UI (`null`: config.js / skin defaults apply).
    menus: Option<serde_json::Value>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicUpload<'a> {
    enabled: bool,
    require_login: bool,
    max_bytes: usize,
    allowed_extensions: &'a [String],
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicWikiConfig {
    base_url: String,
    api_url: String,
    /// Absolute article URL template, `$1` is replaced with the (encoded) title.
    article_path: String,
    site_name: Option<String>,
    logo: Option<String>,
    lang: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicOauthConfig<'a> {
    client_id: &'a str,
    authorize_url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicRecentChanges {
    enabled: bool,
    document_namespaces: Option<Vec<i32>>,
    discussion_namespaces: Option<Vec<i32>>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicSiteNotice<'a> {
    enabled: bool,
    page: &'a str,
}

#[get("")]
async fn get_config(
    config: Data<Config>,
    mw: Data<MwClient>,
    pool: Option<Data<DbPool>>,
) -> HttpResponse {
    let siteinfo = mw.siteinfo().await;
    let menus = match pool {
        Some(pool) => load_menus(pool).await,
        None => None,
    };
    let site = &config.site;
    let public = PublicConfig {
        site_name: site
            .name
            .clone()
            .or_else(|| siteinfo.as_ref().map(|i| i.site_name.clone())),
        locale: site.locale.as_deref(),
        default_board: site.default_board.as_deref(),
        wiki: PublicWikiConfig {
            base_url: config.mw.base_url.clone(),
            api_url: config.mw.public_api_url.clone(),
            article_path: mw.article_path(siteinfo.as_ref()),
            site_name: siteinfo.as_ref().map(|i| i.site_name.clone()),
            logo: siteinfo.as_ref().and_then(|i| i.logo.clone()),
            lang: siteinfo.as_ref().and_then(|i| i.lang.clone()),
        },
        oauth: PublicOauthConfig {
            client_id: &config.oauth.client_id,
            authorize_url: config.mw.authorize_url(),
        },
        nav_links: &site.nav_links,
        recent_changes: PublicRecentChanges {
            enabled: site.recent_changes_enabled,
            document_namespaces: site
                .document_namespaces
                .clone()
                .or_else(|| siteinfo.as_ref().map(|i| i.default_document_namespaces())),
            discussion_namespaces: site
                .discussion_namespaces
                .clone()
                .or_else(|| siteinfo.as_ref().map(|i| i.default_discussion_namespaces())),
        },
        site_notice: PublicSiteNotice {
            enabled: site.sitenotice_enabled,
            page: &site.sitenotice_page,
        },
        upload: PublicUpload {
            enabled: config.upload.enabled,
            require_login: config.upload.require_login,
            max_bytes: config.upload.max_bytes,
            allowed_extensions: &config.upload.allowed_extensions,
        },
        menus,
    };
    // Don't let browsers cache a degraded response for long.
    let max_age = if siteinfo.is_some() { 300 } else { 30 };
    HttpResponse::Ok()
        .insert_header(("Cache-Control", format!("max-age={}", max_age)))
        .json(public)
}

/// Stored menus; `None` when unset or unreadable (logged).
async fn load_menus(pool: Data<DbPool>) -> Option<serde_json::Value> {
    let res = block(move || -> anyhow::Result<Option<serde_json::Value>> {
        let mut conn = pool.get()?;
        match SiteSetting::get(&mut conn, SiteSetting::MENUS)? {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    })
    .await;
    match res {
        Ok(Ok(menus)) => menus,
        Ok(Err(e)) => {
            log::error!("cannot load menus: {}", e);
            None
        }
        Err(e) => {
            log::error!("cannot load menus: {}", e);
            None
        }
    }
}

pub fn scope() -> Scope {
    web::scope("/config").service(get_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use std::sync::Arc;

    /// Live test against a real wiki (network required):
    /// `TEST_MW_BASE_URL=https://www.mediawiki.org TEST_MW_SCRIPT_PATH=/w \
    ///  cargo test --lib live_ -- --ignored --nocapture`
    #[actix_web::test]
    #[ignore]
    async fn live_get_config() {
        live_get_config_inner().await;
    }

    async fn live_get_config_inner() {
        let base = std::env::var("TEST_MW_BASE_URL").expect("TEST_MW_BASE_URL");
        let script = std::env::var("TEST_MW_SCRIPT_PATH").unwrap_or_default();
        let config = Arc::new(
            Config::from_lookup(|k| match k {
                "BIND_ADDRESS" => Some("127.0.0.1:0".into()),
                "DATABASE_URL" => Some("mysql://unused".into()),
                "MW_BASE_URL" => Some(base.clone()),
                "MW_SCRIPT_PATH" => Some(script.clone()),
                "OAUTH_CLIENT_ID" => Some("test-client".into()),
                "OAUTH_CLIENT_SECRET" => Some("unused".into()),
                "UPLOAD_ENABLED" => Some("false".into()),
                _ => None,
            })
            .unwrap(),
        );
        let mw = Data::new(MwClient::new(config.clone()));
        let app = test::init_service(
            App::new()
                .app_data(Data::from(config.clone()))
                .app_data(mw.clone())
                .service(scope()),
        )
        .await;
        let req = test::TestRequest::get().uri("/config").to_request();
        let body: serde_json::Value = test::call_and_read_body_json(&app, req).await;
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
        assert!(body["siteName"].is_string(), "siteinfo should be reachable");
        assert!(body["wiki"]["articlePath"].as_str().unwrap().contains("$1"));
        assert_eq!(body["oauth"]["clientId"], "test-client");
        assert_eq!(body["upload"]["enabled"], false);

        let blocked = mw
            .is_ip_blocked(&"127.0.0.1".parse().unwrap())
            .await
            .expect("block check should succeed");
        println!("127.0.0.1 blocked: {}", blocked);
    }
}
