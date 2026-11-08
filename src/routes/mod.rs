mod admin;
mod auth;
mod boards;
mod comments;
mod config;
mod files;
mod me;
mod search;
mod topics;

use crate::auth::Profile;
use crate::config::{Config, StorageConfig};
use crate::custom_error::CustomError;
use crate::mediawiki::MwClient;
use actix_web::{get, web, Error, HttpResponse, Scope};

/// The logged-in board administrator, or the response to send instead.
async fn require_admin(
    token: Option<String>,
    mw: &MwClient,
) -> Result<Result<Profile, HttpResponse>, CustomError> {
    let Some(token) = token else {
        return Ok(Err(HttpResponse::Unauthorized().body("TokenMissing")));
    };
    let profile = mw.profile(&token).await?;
    if !profile.is_admin() {
        return Ok(Err(HttpResponse::Forbidden().body("Admin only")));
    }
    Ok(Ok(profile))
}

/// For `showHidden=true` (hidden topics and comments, board administrators
/// only): `None` when allowed or not asked for, else the response to send.
async fn check_show_hidden(
    show_hidden: bool,
    token: Option<String>,
    mw: &MwClient,
) -> Result<Option<HttpResponse>, CustomError> {
    if !show_hidden {
        return Ok(None);
    }
    Ok(require_admin(token, mw).await?.err())
}

/// Responses that include hidden content must not be cached anywhere.
fn no_store(mut response: HttpResponse) -> HttpResponse {
    response.headers_mut().insert(
        actix_web::http::header::CACHE_CONTROL,
        actix_web::http::header::HeaderValue::from_static("private, no-store"),
    );
    response
}

#[get("/")]
async fn get() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Ok().body("Hi!"))
}

pub fn scope(config: &Config) -> Scope {
    let scope = web::scope("")
        .service(get)
        .service(auth::scope())
        .service(admin::scope())
        .service(config::scope())
        .service(me::scope())
        .service(files::scope(config))
        .service(boards::scope())
        .service(topics::scope())
        .service(comments::scope())
        .service(search::scope());
    if let StorageConfig::Local { .. } = config.upload.storage {
        scope.service(files::uploads_scope())
    } else {
        scope
    }
}
