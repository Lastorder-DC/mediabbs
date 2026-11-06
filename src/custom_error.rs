use actix_web::{dev::HttpResponseBuilder, error, http::header, http::StatusCode, HttpResponse};
use derive_more::{Display, Error};

#[derive(Debug, Display, Error)]
pub struct CustomError {
    pub err: anyhow::Error,
}

impl error::ResponseError for CustomError {
    fn error_response(&self) -> HttpResponse {
        let body = if self.is_wiki_auth_error() {
            "TokenInvalid"
        } else {
            "INTERNAL SERVER ERROR"
        };
        HttpResponseBuilder::new(self.status_code())
            .set_header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(body)
    }
    fn status_code(&self) -> StatusCode {
        if self.is_wiki_auth_error() {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

impl CustomError {
    fn is_wiki_auth_error(&self) -> bool {
        self.err.downcast_ref::<crate::mediawiki::WikiAuthError>().is_some()
    }
}

impl From<r2d2::Error> for CustomError {
    fn from(err: r2d2::Error) -> CustomError {
        CustomError {
            err: anyhow::anyhow!(err),
        }
    }
}

impl From<actix_web::error::BlockingError<anyhow::Error>> for CustomError {
    fn from(err: actix_web::error::BlockingError<anyhow::Error>) -> CustomError {
        CustomError {
            err: anyhow::anyhow!(err),
        }
    }
}

impl From<anyhow::Error> for CustomError {
    fn from(err: anyhow::Error) -> CustomError {
        CustomError { err }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::ResponseError;

    #[test]
    fn test_status_codes() {
        let e: CustomError = anyhow::Error::from(crate::mediawiki::WikiAuthError).into();
        assert_eq!(e.status_code(), StatusCode::UNAUTHORIZED);
        let e: CustomError = anyhow::anyhow!("boom").into();
        assert_eq!(e.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
