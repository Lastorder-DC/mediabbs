use actix_web::{
    client::Client, dev, error::ErrorUnauthorized, web::Bytes, Error, FromRequest, HttpMessage,
    HttpRequest,
};
use anyhow::anyhow;
use chrono::prelude::*;
use derive_more::{Display, Error};
use futures::future::{err, ok, Ready};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use lazy_static::lazy_static;
use serde::{Deserialize, Deserializer, Serialize};
use std::env;

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
    scopes: Vec<String>,
}

fn from_float<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: f64 = Deserialize::deserialize(deserializer)?;
    let timestamp = s as i64;
    let dt = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(timestamp, 0), Utc);
    Ok(dt)
}

#[derive(Debug, Display, Error)]
enum DecodeError {
    TokenExpired,
    TokenInvalid,
}

lazy_static! {
    static ref PUBLIC_KEY: Vec<u8> = std::fs::read("pubkey.pem").expect("public key must exist");
    static ref DECODING_KEY: DecodingKey<'static> =
        DecodingKey::from_rsa_pem(&PUBLIC_KEY).expect("must succeed");
}

fn decode(token: &str) -> Result<Claims, DecodeError> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = false;
    validation.set_audience(&[env::var("OAUTH_CLIENT_ID").expect("OAUTH_CLIENT_ID must be set")]);
    let token_data = jsonwebtoken::decode::<Claims>(&token, &DECODING_KEY, &validation)
        .map_err(|_| DecodeError::TokenInvalid)?;
    if token_data.claims.exp < Utc::now() {
        Err(DecodeError::TokenExpired)
    } else {
        Ok(token_data.claims)
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
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        if let Some(token_cookie) = req.cookie("access_token") {
            let token = token_cookie.value();
            match decode(&token) {
                Ok(decoded) => match decoded.sub.parse::<i32>() {
                    Ok(id) => ok(Self {
                        id: Some(id),
                        token: Some(token.to_owned()),
                    }),
                    Err(_) => err(ErrorUnauthorized("TokenExpired")),
                },
                Err(e) => match e {
                    DecodeError::TokenExpired => err(ErrorUnauthorized("TokenExpired")),
                    DecodeError::TokenInvalid => err(ErrorUnauthorized("TokenInvalid")),
                },
            }
        } else {
            ok(Self {
                id: None,
                token: None,
            })
        }
    }
}

pub struct RefreshToken {
    pub refresh_token: String,
}

impl FromRequest for RefreshToken {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        if let Some(token_cookie) = req.cookie("refresh_token") {
            ok(RefreshToken {
                refresh_token: token_cookie.value().to_owned(),
            })
        } else {
            err(ErrorUnauthorized("Token is missing"))
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: i32,
    pub username: String,
    pub confirmed_email: bool,
    pub blocked: bool,
    pub groups: Vec<String>,
    pub rights: Vec<String>,
    pub email: String,
}

impl Profile {
    pub fn is_admin(&self) -> bool {
        self.groups.contains(&"boardmanager".to_owned())
    }
}

/// Response of MediaWiki `rest.php/oauth2/resource/profile`.
///
/// `realname` and `email` are only returned when the OAuth consumer was granted
/// access to private information, and some fields may be missing depending on the
/// Extension:OAuth version, so everything except `sub`/`username` is optional.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MwProfileResponse {
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

impl Profile {
    pub async fn get(token: &str) -> anyhow::Result<Self> {
        let client = Client::default();
        let mut res = client
            .get("https://librewiki.net/rest.php/oauth2/resource/profile")
            .set_header("Accept", "application/json")
            .set_header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| anyhow!(format!("{}", e)))?;

        // MW gives incorrect content-type header
        let body: Bytes = res.body().await?;
        let data: MwProfileResponse = serde_json::from_slice(&body)?;
        let resp = Profile {
            id: data.sub,
            username: data.username,
            confirmed_email: data.confirmed_email,
            blocked: data.blocked,
            groups: data.groups,
            rights: data.rights,
            // Kept as a string (empty when not granted) for client compatibility.
            email: data.email.unwrap_or_default(),
        };

        Ok(resp)
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
    fn test_profile_serializes_camel_case() {
        let profile = Profile {
            id: 1,
            username: "Foo".to_owned(),
            confirmed_email: true,
            blocked: false,
            groups: vec![],
            rights: vec![],
            email: String::new(),
        };
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["confirmedEmail"], true);
        assert!(json.get("confirmed_email").is_none());
    }
}
