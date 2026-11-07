//! File storage for uploaded images: S3 (AWS or S3-compatible such as MinIO/R2)
//! or a local directory served by the backend itself (`GET /uploads/{name}`).

use crate::config::{Config, StorageConfig};
use anyhow::{anyhow, Result};
use nanoid::nanoid;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::OnceCell;

#[derive(Serialize, Debug, PartialEq)]
pub struct StoredFile {
    /// Storage-relative path (S3 object key / local file name).
    pub path: String,
    /// Public URL of the file.
    pub url: String,
}

pub struct Storage {
    config: Arc<Config>,
    s3: OnceCell<aws_sdk_s3::Client>,
}

impl Storage {
    /// Validates the storage configuration (creating the local directory if needed).
    pub fn new(config: Arc<Config>) -> Result<Self, String> {
        match &config.upload.storage {
            StorageConfig::S3 {
                region,
                endpoint: None,
                ..
            } if !is_aws_region(region) => {
                return Err(format!("S3_REGION is not a valid AWS region: {}", region))
            }
            StorageConfig::S3 { region, .. } if region.trim().is_empty() => {
                return Err("S3_REGION must not be empty".into())
            }
            StorageConfig::Local { dir } => {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create LOCAL_UPLOAD_DIR ({}): {}", dir, e))?;
            }
            _ => {}
        }
        Ok(Storage {
            config,
            s3: OnceCell::new(),
        })
    }

    /// S3 client, created on first use. Credentials come from the standard AWS
    /// chain (environment, `~/.aws/credentials`, instance/container role).
    async fn s3_client(&self, region: &str, endpoint: Option<&str>) -> &aws_sdk_s3::Client {
        self.s3
            .get_or_init(|| async {
                let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .region(aws_config::Region::new(region.to_owned()))
                    .load()
                    .await;
                let mut builder = aws_sdk_s3::config::Builder::from(&shared);
                if let Some(endpoint) = endpoint {
                    // S3-compatible storage: path-style URLs, and only send checksums
                    // when required (not every implementation supports the new defaults).
                    builder = builder
                        .endpoint_url(endpoint)
                        .force_path_style(true)
                        .request_checksum_calculation(
                            aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
                        )
                        .response_checksum_validation(
                            aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired,
                        );
                }
                aws_sdk_s3::Client::from_conf(builder.build())
            })
            .await
    }

    pub fn local_dir(&self) -> Option<PathBuf> {
        match &self.config.upload.storage {
            StorageConfig::Local { dir } => Some(PathBuf::from(dir)),
            _ => None,
        }
    }

    /// Stores `bytes` under a new random name with extension `ext` (lower-case, no dot).
    pub async fn save(&self, bytes: Vec<u8>, ext: &str) -> Result<StoredFile> {
        let filename = format!("{}.{}", nanoid!(), ext);
        let path = match &self.config.upload.storage {
            StorageConfig::Disabled => return Err(anyhow!("uploads are disabled")),
            StorageConfig::S3 {
                region,
                endpoint,
                bucket,
                key_prefix,
            } => {
                let key = format!("{}{}", key_prefix, filename);
                let mime = mime_guess::from_ext(ext).first_or_octet_stream();
                self.s3_client(region, endpoint.as_deref())
                    .await
                    .put_object()
                    .bucket(bucket)
                    .key(&key)
                    .body(bytes.into())
                    .content_type(mime.to_string())
                    .cache_control("public, max-age=31536000, immutable")
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow!(
                            "S3 upload failed: {}",
                            aws_sdk_s3::error::DisplayErrorContext(e)
                        )
                    })?;
                key
            }
            StorageConfig::Local { dir } => {
                let target = PathBuf::from(dir).join(&filename);
                actix_web::web::block(move || std::fs::write(target, bytes))
                    .await
                    .map_err(|e| anyhow!("cannot write upload: {}", e))?
                    .map_err(|e| anyhow!("cannot write upload: {}", e))?;
                filename
            }
        };
        Ok(StoredFile {
            url: format!("{}{}", self.config.upload.public_url, path),
            path,
        })
    }
}

/// AWS region names: `us-east-1`, `ap-northeast-2`, `us-gov-west-1`, `cn-north-1`, ...
fn is_aws_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    parts.len() >= 3
        && parts[0].len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
        && parts.last().unwrap().chars().all(|c| c.is_ascii_digit())
        && matches!(
            parts[parts.len() - 2],
            "east"
                | "west"
                | "north"
                | "south"
                | "central"
                | "northeast"
                | "northwest"
                | "southeast"
                | "southwest"
        )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "png" => Some(ImageKind::Png),
            "jpg" | "jpeg" => Some(ImageKind::Jpeg),
            "gif" => Some(ImageKind::Gif),
            "webp" => Some(ImageKind::Webp),
            _ => None,
        }
    }

    /// Detects the image type from its magic bytes.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
            Some(ImageKind::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(ImageKind::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(ImageKind::Gif)
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(ImageKind::Webp)
        } else {
            None
        }
    }
}

/// Checks that the content of a known image extension really is that image type.
/// Unknown extensions (explicitly allowed by the operator) are not checked.
pub fn content_matches_extension(bytes: &[u8], ext: &str) -> bool {
    match ImageKind::from_extension(ext) {
        Some(expected) => ImageKind::sniff(bytes) == Some(expected),
        None => true,
    }
}

/// Whether `name` looks like a file name produced by [`Storage::save`]
/// (nanoid alphabet + one extension), so it is safe to join to a directory.
pub fn is_valid_stored_name(name: &str) -> bool {
    let mut parts = name.splitn(2, '.');
    let stem = parts.next().unwrap_or_default();
    let ext = parts.next().unwrap_or_default();
    !stem.is_empty()
        && stem.len() <= 64
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !ext.is_empty()
        && ext.len() <= 10
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];

    #[test]
    fn test_sniff() {
        assert_eq!(ImageKind::sniff(PNG), Some(ImageKind::Png));
        assert_eq!(
            ImageKind::sniff(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageKind::Jpeg)
        );
        assert_eq!(ImageKind::sniff(b"GIF89a...."), Some(ImageKind::Gif));
        assert_eq!(
            ImageKind::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageKind::Webp)
        );
        assert_eq!(ImageKind::sniff(b"<html>"), None);
        assert!(content_matches_extension(PNG, "png"));
        assert!(!content_matches_extension(b"<script>", "png"));
        assert!(!content_matches_extension(PNG, "jpg"));
        assert!(content_matches_extension(b"%PDF-1.4", "pdf"));
    }

    #[test]
    fn test_valid_stored_name() {
        assert!(is_valid_stored_name("V1StGXR8_Z5jdHi6B-myT.png"));
        assert!(!is_valid_stored_name("../etc/passwd"));
        assert!(!is_valid_stored_name("a/b.png"));
        assert!(!is_valid_stored_name(".png"));
        assert!(!is_valid_stored_name("abc"));
        assert!(!is_valid_stored_name("abc.png.html"));
        assert!(!is_valid_stored_name("abc."));
    }

    fn test_config(pairs: Vec<(&'static str, String)>) -> Arc<Config> {
        let mut map: std::collections::HashMap<&str, String> = [
            ("BIND_ADDRESS", "127.0.0.1:0"),
            ("DATABASE_URL", "mysql://unused"),
            ("MW_BASE_URL", "https://wiki.example.org"),
            ("OAUTH_CLIENT_ID", "id"),
            ("OAUTH_CLIENT_SECRET", "secret"),
        ]
        .iter()
        .map(|(k, v)| (*k, v.to_string()))
        .collect();
        map.extend(pairs);
        Arc::new(Config::from_lookup(|k| map.get(k).cloned()).unwrap())
    }

    #[test]
    fn test_local_save() {
        let dir = std::env::temp_dir().join(format!("mediabbs-test-{}", nanoid!()));
        let config = test_config(vec![
            ("STORAGE_BACKEND", "local".into()),
            ("LOCAL_UPLOAD_DIR", dir.to_string_lossy().into_owned()),
            ("UPLOAD_PUBLIC_URL", "https://api.example/uploads".into()),
        ]);
        let storage = Storage::new(config).unwrap();
        let stored = actix_web::rt::System::new()
            .block_on(async move { storage.save(PNG.to_vec(), "png").await })
            .unwrap();
        assert!(is_valid_stored_name(&stored.path));
        assert_eq!(
            stored.url,
            format!("https://api.example/uploads/{}", stored.path)
        );
        assert_eq!(std::fs::read(dir.join(&stored.path)).unwrap(), PNG);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_aws_region_names() {
        for r in [
            "us-east-1",
            "ap-northeast-2",
            "us-gov-west-1",
            "cn-north-1",
            "eu-central-2",
        ] {
            assert!(is_aws_region(r), "{}", r);
        }
        for r in ["", "auto", "nowhere-1", "us-east", "US-EAST-1"] {
            assert!(!is_aws_region(r), "{}", r);
        }
    }

    #[test]
    fn test_invalid_region() {
        let config = test_config(vec![
            ("S3_REGION", "nowhere-1".into()),
            ("S3_BUCKET", "b".into()),
        ]);
        assert!(Storage::new(config).is_err());
    }

    /// Live S3 upload (needs credentials and S3_* / UPLOAD_* variables in `.env`):
    /// `cargo test --lib live_s3 -- --ignored`
    #[test]
    #[ignore]
    fn live_s3_upload() {
        dotenvy::dotenv().ok();
        let config = Arc::new(Config::from_env().unwrap());
        let storage = Storage::new(config).unwrap();
        let stored = actix_web::rt::System::new()
            .block_on(async move {
                storage
                    .save(include_bytes!("../LICENSE").to_vec(), "txt")
                    .await
            })
            .expect("must succeed");
        println!("{:?}", stored);
    }
}
