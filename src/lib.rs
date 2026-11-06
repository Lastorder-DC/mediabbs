#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate diesel;
pub mod auth;
pub mod config;
pub mod connection_info;
pub mod custom_error;
pub mod db;
pub mod mediawiki;
pub mod models;
pub mod routes;
pub mod s3;
pub mod schema;
use actix_cors::Cors;
use actix_web::{
    middleware::{DefaultHeaders, Logger},
    web, App, HttpServer,
};
use dotenv::dotenv;
use std::{env, sync::Arc};

fn exit_with_config_error(message: String) -> ! {
    eprintln!("Configuration error: {}", message);
    std::process::exit(1);
}

pub async fn run() -> std::io::Result<()> {
    dotenv().ok();
    if env::var_os("RUST_LOG").is_none() {
        env::set_var("RUST_LOG", "actix_web=info,librebbs=info");
    }
    env_logger::init();

    let config = Arc::new(config::Config::from_env().unwrap_or_else(|e| exit_with_config_error(e)));
    let jwt = web::Data::new(
        auth::JwtVerifier::new(&config).unwrap_or_else(|e| exit_with_config_error(e)),
    );
    let mw = web::Data::new(mediawiki::MwClient::new(config.clone()));
    let config_data = web::Data::from(config.clone());

    // Sentry is optional: SENTRY_DSN (or legacy SENTRY_URL). Unset/empty disables it.
    let _guard = sentry::init((
        config.sentry_dsn.clone(),
        sentry::ClientOptions {
            auto_session_tracking: true,
            session_mode: sentry::SessionMode::Request,
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));
    env::set_var("RUST_BACKTRACE", "1");
    let pool = db::create_connection_pool(&config.database_url);

    if config.cors_allowed_origins.is_empty() {
        log::warn!("CORS_ALLOWED_ORIGINS is not set: any origin may send credentialed requests");
    }
    log::info!(
        "MediaWiki: {} (api: {})",
        config.mw.base_url,
        config.mw.api_url
    );
    println!("http://{}", config.bind_address);

    let bind_address = config.bind_address.clone();
    HttpServer::new(move || {
        let mut cors = Cors::default();
        if config.cors_allowed_origins.is_empty() {
            cors = cors.allow_any_origin();
        } else {
            for origin in &config.cors_allowed_origins {
                cors = cors.allowed_origin(origin);
            }
        }
        cors = cors.allow_any_method().allow_any_header();

        App::new()
            .wrap(Logger::default())
            .wrap(sentry_actix::Sentry::new())
            .wrap(cors)
            .wrap(DefaultHeaders::new().header("Access-Control-Allow-Credentials", "true"))
            .data(pool.clone())
            .app_data(config_data.clone())
            .app_data(jwt.clone())
            .app_data(mw.clone())
            .app_data(actix_web_validator::JsonConfig::default().limit(1024 * 1024 * 1))
            .service(web::scope("/v1").service(routes::scope()))
            .service(routes::scope())
    })
    .bind(bind_address)?
    .run()
    .await
}
