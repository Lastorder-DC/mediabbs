use crate::auth::RefreshToken;
use crate::config::{Config, CookieConfig, SameSite};
use crate::custom_error::CustomError;
use crate::mediawiki::{MwClient, TokenPair};
use actix_web::cookie::time::Duration;
use actix_web::{
    cookie::{self, Cookie},
    post,
    web::{self, Data},
    HttpResponse, Scope,
};
use actix_web_validator::Json;
use anyhow::anyhow;
use validator::Validate;

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    code: String,
}

fn build_cookie<'a>(
    config: &CookieConfig,
    name: &'a str,
    value: &'a str,
    max_age: Duration,
) -> Cookie<'a> {
    let mut builder = Cookie::build(name, value)
        .http_only(true)
        .secure(config.secure)
        .same_site(match config.same_site {
            SameSite::Strict => cookie::SameSite::Strict,
            SameSite::Lax => cookie::SameSite::Lax,
            SameSite::None => cookie::SameSite::None,
        })
        .max_age(max_age)
        .path("/");
    if let Some(domain) = &config.domain {
        builder = builder.domain(domain.clone());
    }
    builder.finish()
}

fn token_response(
    config: &CookieConfig,
    tokens: Option<&TokenPair>,
) -> Result<HttpResponse, CustomError> {
    let (access_value, refresh_value, max_age) = match tokens {
        Some(t) => (
            t.access_token.as_str(),
            t.refresh_token.as_str(),
            Duration::days(config.max_age_days),
        ),
        None => ("", "", Duration::ZERO),
    };
    let mut response = HttpResponse::NoContent().finish();
    response
        .add_cookie(&build_cookie(config, "access_token", access_value, max_age))
        .map_err(|e| anyhow!(format!("{}", e)))?;
    response
        .add_cookie(&build_cookie(
            config,
            "refresh_token",
            refresh_value,
            max_age,
        ))
        .map_err(|e| anyhow!(format!("{}", e)))?;
    Ok(response)
}

#[post("/login")]
async fn login(
    config: Data<Config>,
    mw: Data<MwClient>,
    Json(LoginRequest { code }): Json<LoginRequest>,
) -> Result<HttpResponse, CustomError> {
    match mw.exchange_code(&code).await? {
        Some(tokens) => token_response(&config.cookie, Some(&tokens)),
        None => Ok(HttpResponse::Unauthorized().body("LoginFailed")),
    }
}

#[post("/logout")]
async fn logout(config: Data<Config>) -> Result<HttpResponse, CustomError> {
    token_response(&config.cookie, None)
}

#[post("/refresh")]
async fn refresh(
    config: Data<Config>,
    mw: Data<MwClient>,
    RefreshToken { refresh_token }: RefreshToken,
) -> Result<HttpResponse, CustomError> {
    match mw.refresh(&refresh_token).await? {
        Some(tokens) => token_response(&config.cookie, Some(&tokens)),
        None => Ok(HttpResponse::Unauthorized().finish()),
    }
}

pub fn scope() -> Scope {
    web::scope("/auth")
        .service(login)
        .service(logout)
        .service(refresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cookie() {
        let config = CookieConfig {
            domain: Some("example.org".into()),
            secure: true,
            same_site: SameSite::None,
            max_age_days: 7,
        };
        let c = build_cookie(&config, "access_token", "abc", Duration::days(7)).to_string();
        assert!(c.starts_with("access_token=abc"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("Secure"));
        assert!(c.contains("SameSite=None"));
        assert!(c.contains("Domain=example.org"));
        assert!(c.contains("Path=/"));
        assert!(c.contains("Max-Age=604800"));
    }
}
