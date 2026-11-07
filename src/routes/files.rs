use crate::auth::UserInfo;
use crate::config::Config;
use crate::connection_info::ConnectionInfo;
use crate::custom_error::CustomError;
use crate::mediawiki::MwClient;
use crate::rate_limit::{too_many, PostLimiter};
use crate::storage::{content_matches_extension, is_valid_stored_name, Storage, StoredFile};
use actix_web::{
    get,
    http::header,
    post,
    web::{self, Data, Path},
    HttpRequest, HttpResponse, Scope,
};
use base64::Engine as _;
use futures_util::StreamExt;
use validator::Validate;

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct PostFileRequest {
    #[validate(length(min = 1, max = 255))]
    filename: String,
    /// Base64-encoded file content.
    content: String,
}

/// Lower-case extension of `filename`, if it is in the allow list.
fn allowed_extension(filename: &str, allowed: &[String]) -> Option<String> {
    let ext = std::path::Path::new(filename)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    if allowed.contains(&ext) {
        Some(ext)
    } else {
        None
    }
}

/// The largest request body: base64 is ~4/3 of the file, plus room for the JSON.
fn body_limit(config: &Config) -> usize {
    config.upload.max_bytes / 3 * 4 + 64 * 1024
}

#[post("")]
#[allow(clippy::too_many_arguments)] // actix extractors
async fn post_files(
    config: Data<Config>,
    mw: Data<MwClient>,
    storage: Data<Storage>,
    limiter: Data<PostLimiter>,
    ConnectionInfo { ip }: ConnectionInfo,
    UserInfo { token, .. }: UserInfo,
    request: HttpRequest,
    mut payload: web::Payload,
) -> Result<HttpResponse, CustomError> {
    let upload = &config.upload;
    if !upload.enabled {
        return Ok(HttpResponse::NotFound().body("Uploads are disabled"));
    }
    // JSON only: other content types can be sent cross-site without a CORS preflight.
    let is_json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json"));
    if !is_json {
        return Ok(HttpResponse::UnsupportedMediaType().body("Expected JSON"));
    }
    if let Err(wait) = limiter.check(ip) {
        return Ok(too_many(wait));
    }

    // Who is uploading, before reading a body of up to several megabytes.
    match token {
        Some(token) => {
            if mw.profile(&token).await?.blocked {
                return Ok(HttpResponse::Forbidden().body("You are blocked"));
            }
        }
        None if upload.require_login => {
            return Ok(HttpResponse::Unauthorized().body("LoginRequired"));
        }
        None => {
            if mw.is_ip_blocked(&ip).await? {
                return Ok(HttpResponse::Forbidden().body("You are blocked"));
            }
        }
    }

    let limit = body_limit(&config);
    let mut body = web::BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let Ok(chunk) = chunk else {
            return Ok(HttpResponse::BadRequest().body("Invalid request body"));
        };
        if body.len() + chunk.len() > limit {
            return Ok(HttpResponse::PayloadTooLarge().body("File is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    let PostFileRequest { filename, content } =
        match serde_json::from_slice::<PostFileRequest>(&body) {
            Ok(req) if req.validate().is_ok() => req,
            _ => return Ok(HttpResponse::BadRequest().body("Invalid request body")),
        };
    drop(body);

    let ext = match allowed_extension(&filename, &upload.allowed_extensions) {
        Some(ext) => ext,
        None => {
            return Ok(HttpResponse::BadRequest().body(format!(
                "File type is not allowed (allowed: {})",
                upload.allowed_extensions.join(", ")
            )))
        }
    };
    let bytes = match base64::engine::general_purpose::STANDARD.decode(content.trim()) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(HttpResponse::BadRequest().body("Invalid base64 content")),
    };
    if bytes.is_empty() {
        return Ok(HttpResponse::BadRequest().body("File is empty"));
    }
    if bytes.len() > upload.max_bytes {
        return Ok(HttpResponse::PayloadTooLarge().body("File is too large"));
    }
    if !content_matches_extension(&bytes, &ext) {
        return Ok(HttpResponse::BadRequest().body("File content does not match its extension"));
    }

    let stored: StoredFile = storage.save(bytes, &ext).await?;
    Ok(HttpResponse::Ok().json(stored))
}

/// Serves files of the local storage backend. Only registered when
/// `STORAGE_BACKEND=local`.
#[get("{filename}")]
async fn get_upload(
    storage: Data<Storage>,
    filename: Path<String>,
) -> Result<HttpResponse, CustomError> {
    let filename = filename.into_inner();
    let dir = match storage.local_dir() {
        Some(dir) => dir,
        None => return Ok(HttpResponse::NotFound().finish()),
    };
    if !is_valid_stored_name(&filename) {
        return Ok(HttpResponse::NotFound().finish());
    }
    let path = dir.join(&filename);
    let bytes = match web::block(move || std::fs::read(path)).await? {
        Ok(bytes) => bytes,
        Err(_) => return Ok(HttpResponse::NotFound().finish()),
    };
    let ext = filename.rsplit('.').next().unwrap_or_default();
    let mime = mime_guess::from_ext(ext).first_or_octet_stream();
    Ok(HttpResponse::Ok()
        .content_type(mime.to_string())
        .insert_header(("Cache-Control", "public, max-age=31536000, immutable"))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Content-Security-Policy", "default-src 'none'; sandbox"))
        .body(bytes))
}

pub fn scope(_config: &Config) -> Scope {
    web::scope("/files").service(post_files)
}

pub fn uploads_scope() -> Scope {
    web::scope("/uploads").service(get_upload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test, App};
    use std::{collections::HashMap, sync::Arc};

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

    fn test_config(dir: &std::path::Path, require_login: bool) -> Arc<Config> {
        let map: HashMap<&str, String> = [
            ("BIND_ADDRESS", "127.0.0.1:0".to_owned()),
            ("DATABASE_URL", "mysql://unused".to_owned()),
            ("MW_BASE_URL", "https://wiki.invalid".to_owned()),
            ("OAUTH_CLIENT_ID", "id".to_owned()),
            ("OAUTH_CLIENT_SECRET", "secret".to_owned()),
            ("CHECK_IP_BLOCKS", "false".to_owned()),
            ("STORAGE_BACKEND", "local".to_owned()),
            ("LOCAL_UPLOAD_DIR", dir.to_string_lossy().into_owned()),
            (
                "UPLOAD_PUBLIC_URL",
                "https://api.example/uploads/".to_owned(),
            ),
            ("UPLOAD_MAX_BYTES", "64".to_owned()),
            ("UPLOAD_REQUIRE_LOGIN", require_login.to_string()),
        ]
        .iter()
        .cloned()
        .collect();
        Arc::new(Config::from_lookup(|k| map.get(k).cloned()).unwrap())
    }

    fn post(filename: &str, bytes: &[u8]) -> test::TestRequest {
        test::TestRequest::post()
            .uri("/files")
            .peer_addr("203.0.113.5:1234".parse().unwrap())
            .set_json(serde_json::json!({
                "filename": filename,
                "content": base64::engine::general_purpose::STANDARD.encode(bytes),
            }))
    }

    async fn run_upload_flow(dir: std::path::PathBuf) {
        let config = test_config(&dir, false);
        let app = test::init_service(
            App::new()
                .app_data(Data::from(config.clone()))
                .app_data(Data::new(MwClient::new(config.clone())))
                .app_data(Data::new(Storage::new(config.clone()).unwrap()))
                .app_data(Data::new(PostLimiter::new(0)))
                .service(scope(&config))
                .service(uploads_scope()),
        )
        .await;

        let res = test::call_service(&app, post("x.html", PNG).to_request()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = test::call_service(&app, post("x.png", b"<script>").to_request()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = test::call_service(&app, post("x.png", &[0x89u8; 100]).to_request()).await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let res = test::call_service(&app, post("Photo.PNG", PNG).to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        let stored: serde_json::Value = test::read_body_json(res).await;
        let path = stored["path"].as_str().unwrap().to_owned();
        assert!(path.ends_with(".png"));
        assert_eq!(
            stored["url"].as_str().unwrap(),
            format!("https://api.example/uploads/{}", path)
        );

        let req = test::TestRequest::get()
            .uri(&format!("/uploads/{}", path))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("content-type").unwrap(), "image/png");
        assert_eq!(
            res.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(test::read_body(res).await, PNG);

        let req = test::TestRequest::get()
            .uri("/uploads/..%2Fsecret.png")
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // Anonymous uploads rejected when login is required
        let config = test_config(&dir, true);
        let app = test::init_service(
            App::new()
                .app_data(Data::from(config.clone()))
                .app_data(Data::new(MwClient::new(config.clone())))
                .app_data(Data::new(Storage::new(config.clone()).unwrap()))
                .app_data(Data::new(PostLimiter::new(2)))
                .service(scope(&config)),
        )
        .await;
        let res = test::call_service(&app, post("x.png", PNG).to_request()).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(test::read_body(res).await, "LoginRequired");
        // Refused before the body is read, whatever its size.
        let big = vec![0x89u8; 4096];
        let res = test::call_service(&app, post("x.png", &big).to_request()).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = test::call_service(&app, post("x.png", PNG).to_request()).await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        // No CORS-simple content types (cross-site forms).
        let req = test::TestRequest::post()
            .uri("/files")
            .peer_addr("203.0.113.6:1234".parse().unwrap())
            .insert_header(("content-type", "text/plain"))
            .set_payload(r#"{"filename":"x.png","content":""}"#)
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[actix_web::test]
    async fn test_upload_flow() {
        let dir = std::env::temp_dir().join(format!("mediabbs-files-{}", nanoid::nanoid!()));
        run_upload_flow(dir.clone()).await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[actix_web::test]
    async fn test_allowed_extension() {
        let allowed = vec!["png".to_owned(), "jpg".to_owned()];
        assert_eq!(allowed_extension("a.PNG", &allowed), Some("png".into()));
        assert_eq!(
            allowed_extension("photo.final.jpg", &allowed),
            Some("jpg".into())
        );
        assert_eq!(allowed_extension("a.html", &allowed), None);
        assert_eq!(allowed_extension("png", &allowed), None);
        assert_eq!(allowed_extension("a.png.html", &allowed), None);
    }
}
