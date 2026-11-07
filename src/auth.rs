//! Authentication: verifies MediaWiki OAuth 2.0 access tokens (JWT, RS256) stored
//! in the `access_token` cookie.

use crate::config::Config;
use actix_web::{
    dev,
    error::{ErrorInternalServerError, ErrorUnauthorized},
    web::Data,
    Error, FromRequest, HttpRequest,
};
use chrono::prelude::*;
use derive_more::{Display, Error};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Deserializer, Serialize};
use std::future::{ready, Ready};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    aud: String,
    #[serde(deserialize_with = "from_float")]
    iat: DateTime<Utc>,
    #[serde(deserialize_with = "from_float")]
    nbf: DateTime<Utc>,
    #[serde(deserialize_with = "from_float")]
    exp: DateTime<Utc>,
    sub: String,
    #[serde(default)]
    scopes: Vec<String>,
}

fn from_float<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: f64 = Deserialize::deserialize(deserializer)?;
    let timestamp = s as i64;
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .ok_or_else(|| serde::de::Error::custom("timestamp out of range"))
}

/// Extracts the numeric user id from a JWT `sub` claim.
///
/// Accepts plain ids (`"123"`) as well as prefixed forms some Extension:OAuth
/// versions/configurations may produce (e.g. `"mw:CentralAuth::123"`).
pub fn parse_user_id(sub: &str) -> Option<i32> {
    let sub = sub.trim();
    sub.parse::<i32>()
        .ok()
        .or_else(|| sub.rsplit(':').next()?.parse::<i32>().ok())
        .filter(|id| *id > 0)
}

#[derive(Debug, Display, Error, PartialEq)]
enum DecodeError {
    TokenExpired,
    TokenInvalid,
}

/// Verifies access tokens with the wiki's OAuth 2.0 public key
/// (`$wgOAuth2PublicKey`). Created once at startup and shared as app data.
pub struct JwtVerifier {
    key: DecodingKey,
    audience: String,
}

impl JwtVerifier {
    pub fn new(config: &Config) -> Result<Self, String> {
        let path = &config.oauth.public_key_path;
        let pem = std::fs::read(path)
            .map_err(|e| format!("cannot read OAUTH_PUBLIC_KEY_PATH ({}): {}", path, e))?;
        Self::from_pem(&pem, &config.oauth.client_id)
            .map_err(|e| format!("invalid public key in {}: {}", path, e))
    }

    pub fn from_pem(pem: &[u8], audience: &str) -> Result<Self, String> {
        let key = DecodingKey::from_rsa_pem(pem).map_err(|e| e.to_string())?;
        Ok(JwtVerifier {
            key,
            audience: audience.to_owned(),
        })
    }

    fn decode(&self, token: &str) -> Result<Claims, DecodeError> {
        let mut validation = Validation::new(Algorithm::RS256);
        // MediaWiki uses float timestamps, which jsonwebtoken cannot parse; `exp` is
        // checked below instead.
        validation.validate_exp = false;
        validation.required_spec_claims.clear();
        validation.set_audience(&[&self.audience]);
        let token_data = jsonwebtoken::decode::<Claims>(token, &self.key, &validation)
            .map_err(|_| DecodeError::TokenInvalid)?;
        if token_data.claims.exp < Utc::now() {
            Err(DecodeError::TokenExpired)
        } else {
            Ok(token_data.claims)
        }
    }
}

#[derive(Debug)]
pub struct UserInfo {
    pub id: Option<i32>,
    pub token: Option<String>,
}

impl FromRequest for UserInfo {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        let token_cookie = match req.cookie("access_token") {
            Some(cookie) => cookie,
            None => {
                return ready(Ok(Self {
                    id: None,
                    token: None,
                }))
            }
        };
        let verifier = match req.app_data::<Data<JwtVerifier>>() {
            Some(v) => v,
            None => {
                return ready(Err(ErrorInternalServerError(
                    "JwtVerifier is not configured",
                )))
            }
        };
        let token = token_cookie.value();
        ready(match verifier.decode(token) {
            Ok(decoded) => match parse_user_id(&decoded.sub) {
                Some(id) => Ok(Self {
                    id: Some(id),
                    token: Some(token.to_owned()),
                }),
                None => Err(ErrorUnauthorized("TokenInvalid")),
            },
            Err(DecodeError::TokenExpired) => Err(ErrorUnauthorized("TokenExpired")),
            Err(DecodeError::TokenInvalid) => Err(ErrorUnauthorized("TokenInvalid")),
        })
    }
}

pub struct RefreshToken {
    pub refresh_token: String,
}

impl FromRequest for RefreshToken {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        ready(match req.cookie("refresh_token") {
            Some(token_cookie) => Ok(RefreshToken {
                refresh_token: token_cookie.value().to_owned(),
            }),
            None => Err(ErrorUnauthorized("Token is missing")),
        })
    }
}

/// A MediaWiki user, as returned by `GET /me`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: i32,
    pub username: String,
    pub confirmed_email: bool,
    pub blocked: bool,
    pub groups: Vec<String>,
    pub rights: Vec<String>,
    /// Empty when the OAuth consumer has no access to private information.
    pub email: String,
    /// Whether the user may moderate the board (`ADMIN_GROUPS` / `ADMIN_RIGHTS`).
    pub is_admin: bool,
}

impl Profile {
    pub fn is_admin(&self) -> bool {
        self.is_admin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_id() {
        assert_eq!(parse_user_id("123"), Some(123));
        assert_eq!(parse_user_id(" 42 "), Some(42));
        assert_eq!(parse_user_id("mw:CentralAuth::7"), Some(7));
        assert_eq!(parse_user_id("mw:local:wikidb:15"), Some(15));
        assert_eq!(parse_user_id("0"), None);
        assert_eq!(parse_user_id(""), None);
        assert_eq!(parse_user_id("abc"), None);
    }

    #[test]
    fn test_profile_serializes_camel_case() {
        let profile = Profile {
            id: 1,
            username: "Foo".to_owned(),
            confirmed_email: true,
            blocked: false,
            groups: vec![],
            rights: vec![],
            email: String::new(),
            is_admin: true,
        };
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["confirmedEmail"], true);
        assert_eq!(json["isAdmin"], true);
        assert!(json.get("confirmed_email").is_none());
    }

    fn make_token(aud: &str, sub: &str, exp_offset_secs: i64) -> String {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let now = Utc::now().timestamp() as f64;
        let claims = serde_json::json!({
            "aud": aud,
            "jti": "x",
            "iat": now,
            "nbf": now,
            "exp": now + exp_offset_secs as f64,
            "sub": sub,
            "scopes": ["basic"],
        });
        let key = EncodingKey::from_rsa_pem(include_bytes!("testdata/test_private.pem")).unwrap();
        encode(&Header::new(Algorithm::RS256), &claims, &key).unwrap()
    }

    fn verifier() -> JwtVerifier {
        JwtVerifier::from_pem(include_bytes!("testdata/test_public.pem"), "client-id").unwrap()
    }

    #[test]
    fn test_decode_valid() {
        let claims = verifier()
            .decode(&make_token("client-id", "42", 3600))
            .unwrap();
        assert_eq!(parse_user_id(&claims.sub), Some(42));
    }

    #[test]
    fn test_decode_expired() {
        let res = verifier().decode(&make_token("client-id", "42", -3600));
        assert_eq!(res.unwrap_err(), DecodeError::TokenExpired);
    }

    #[test]
    fn test_decode_wrong_audience_or_garbage() {
        let res = verifier().decode(&make_token("other-client", "42", 3600));
        assert_eq!(res.unwrap_err(), DecodeError::TokenInvalid);
        let res = verifier().decode("not.a.jwt");
        assert_eq!(res.unwrap_err(), DecodeError::TokenInvalid);
    }

    #[test]
    fn test_invalid_pem() {
        assert!(JwtVerifier::from_pem(b"not a key", "aud").is_err());
    }
}
