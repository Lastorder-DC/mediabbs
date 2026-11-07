use crate::auth::UserInfo;
use crate::custom_error::CustomError;
use crate::mediawiki::MwClient;
use actix_web::{get, web, web::Data, HttpResponse, Scope};

#[get("")]
async fn get_me(
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
) -> Result<HttpResponse, CustomError> {
    if let Some(token) = token {
        let profile = mw.profile(&token).await?;
        Ok(HttpResponse::Ok()
            .insert_header(("Cache-Control", "private, max-age=60"))
            .insert_header(("Vary", "Cookie"))
            .json(profile))
    } else {
        Ok(HttpResponse::Unauthorized().body("TokenMissing"))
    }
}

pub fn scope() -> Scope {
    web::scope("/me").service(get_me)
}
