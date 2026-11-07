mod admin;
mod auth;
mod boards;
mod comments;
mod config;
mod files;
mod me;
mod search;
mod topics;

use crate::config::{Config, StorageConfig};
use actix_web::{get, web, Error, HttpResponse, Scope};

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
