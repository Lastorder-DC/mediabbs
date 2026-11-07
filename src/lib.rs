#[macro_use]
extern crate serde;
pub mod auth;
pub mod cli;
pub mod config;
pub mod connection_info;
pub mod custom_error;
pub mod db;
pub mod mediawiki;
pub mod models;
pub mod routes;
pub mod schema;
pub mod storage;
use actix_cors::Cors;
use actix_web::{
    middleware::{DefaultHeaders, Logger},
    web, App, HttpServer,
};
use std::sync::Arc;

fn exit_with_config_error(message: String) -> ! {
    eprintln!("Configuration error: {}", message);
    std::process::exit(1);
}

pub async fn run() -> std::io::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("actix_web=info,mediabbs=info"),
    )
    .init();

    let config = Arc::new(config::Config::from_env().unwrap_or_else(|e| exit_with_config_error(e)));
    let jwt = web::Data::new(
        auth::JwtVerifier::new(&config).unwrap_or_else(|e| exit_with_config_error(e)),
    );
    let mw = web::Data::new(mediawiki::MwClient::new(config.clone()));
    let storage = web::Data::new(
        storage::Storage::new(config.clone()).unwrap_or_else(|e| exit_with_config_error(e)),
    );
    let config_data = web::Data::from(config.clone());

    // Sentry is optional: SENTRY_DSN (or legacy SENTRY_URL). Unset/empty disables it.
    let _guard = sentry::init((config.sentry_dsn.clone(), {
        let mut options = sentry::ClientOptions::default();
        options.auto_session_tracking = true;
        options.session_mode = sentry::SessionMode::Request;
        options.release = sentry::release_name!();
        options
    }));
    let pool = db::create_connection_pool(&config.database_url);
    if config.migrate_on_start {
        let mut conn = pool.get().expect("cannot get a database connection");
        db::run_migrations(&mut conn, true).expect("failed to run migrations");
    }

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
            .wrap(DefaultHeaders::new().add(("Access-Control-Allow-Credentials", "true")))
            .app_data(web::Data::new(pool.clone()))
            .app_data(config_data.clone())
            .app_data(jwt.clone())
            .app_data(mw.clone())
            .app_data(storage.clone())
            .app_data(actix_web_validator::JsonConfig::default().limit((1024 * 1024)))
            .service(web::scope("/v1").service(routes::scope(&config)))
            .service(routes::scope(&config))
    })
    .bind(bind_address)?
    .run()
    .await
}
