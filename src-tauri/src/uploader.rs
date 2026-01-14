use crate::config::R2Config;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::Region;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// Maximum number of retry attempts for transient errors
const MAX_RETRIES: u32 = 3;

/// Initial delay between retries (doubles each attempt)
const INITIAL_RETRY_DELAY_MS: u64 = 1000;

/// Upload result
#[derive(Debug, Clone, serde::Serialize)]
pub struct UploadResult {
    pub url: String,
    pub key: String,
    pub size: u64,
}

/// Check if an error is transient (worth retrying)
fn is_transient_error(error: &str) -> bool {
    let error_lower = error.to_lowercase();
    error_lower.contains("timeout")
        || error_lower.contains("connection")
        || error_lower.contains("temporarily")
        || error_lower.contains("503")
        || error_lower.contains("502")
        || error_lower.contains("504")
        || error_lower.contains("retry")
        || error_lower.contains("network")
}

/// Convert raw S3/R2 errors into user-friendly messages
fn friendly_error(err_str: &str) -> String {
    let err_lower = err_str.to_lowercase();
    
    // Network errors - be specific so user knows it's not credentials
    if err_lower.contains("timeout") || err_lower.contains("timed out") {
        return "Connection timed out - please try again".to_string();
    }
    if err_lower.contains("connection refused") || err_lower.contains("network") {
        return "Connection failed - check your network".to_string();
    }
    
    // Everything else is a credential/config error - keep it simple
    "Invalid R2 credentials".to_string()
}

/// Validate R2 credentials by uploading and deleting a tiny test object
pub async fn validate_r2_credentials(config: &R2Config) -> Result<(), String> {
    // Create R2 credentials
    let credentials = Credentials::new(
        Some(&config.access_key),
        Some(&config.secret_key),
        None,
        None,
        None,
    )
    .map_err(|e| friendly_error(&e.to_string()))?;

    // R2 endpoint
    let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.account_id);
    let region = Region::Custom {
        region: "auto".to_string(),
        endpoint,
    };

    // Create bucket handle
    let bucket = Bucket::new(&config.bucket_name, region, credentials)
        .map_err(|e| friendly_error(&e.to_string()))?
        .with_path_style();

    // Upload a tiny test object - this validates everything:
    // 1. Account ID is valid (endpoint resolves)
    // 2. Credentials are valid (request is signed correctly)
    // 3. Bucket exists and we have write access
    let test_key = ".zipdrop-connection-test";
    let test_data = b"test";
    
    match bucket.put_object(test_key, test_data).await {
        Ok(response) => {
            if response.status_code() == 200 {
                // Success! Clean up the test object
                let _ = bucket.delete_object(test_key).await;
                Ok(())
            } else {
                Err("Invalid R2 credentials".to_string())
            }
        }
        Err(e) => {
            Err(friendly_error(&e.to_string()))
        }
    }
}

/// Delete an object from Cloudflare R2
pub async fn delete_from_r2(key: &str, config: &R2Config) -> Result<(), String> {
    // Create R2 credentials
    let credentials = Credentials::new(
        Some(&config.access_key),
        Some(&config.secret_key),
        None,
        None,
        None,
    )
    .map_err(|e| format!("Failed to create credentials: {}", e))?;

    // R2 endpoint
    let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.account_id);
    let region = Region::Custom {
        region: "auto".to_string(),
        endpoint,
    };

    // Create bucket handle
    let bucket = Bucket::new(&config.bucket_name, region, credentials)
        .map_err(|e| format!("Failed to create bucket: {}", e))?
        .with_path_style();

    // Delete the object
    bucket
        .delete_object(key)
        .await
        .map_err(|e| format!("Failed to delete from R2: {}", e))?;

    Ok(())
}

/// Upload a file to Cloudflare R2 with retry logic
pub async fn upload_to_r2(file_path: &Path, config: &R2Config) -> Result<UploadResult, String> {
    // Read the file
    let file_data =
        fs::read(file_path).map_err(|e| format!("Failed to read file for upload: {}", e))?;
    let file_size = file_data.len() as u64;

    // Generate unique key with original extension
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let unique_id = Uuid::new_v4().to_string()[..8].to_string();
    let original_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");

    // Sanitize filename (remove spaces, special chars)
    let safe_name: String = original_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let key = format!("u/{}_{}.{}", unique_id, safe_name, ext);

    // Determine content type
    let content_type = match ext {
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    };

    // Create R2 credentials
    let credentials = Credentials::new(
        Some(&config.access_key),
        Some(&config.secret_key),
        None,
        None,
        None,
    )
    .map_err(|e| format!("Failed to create credentials: {}", e))?;

    // R2 endpoint
    let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.account_id);
    let region = Region::Custom {
        region: "auto".to_string(),
        endpoint,
    };

    // Create bucket handle
    let bucket = Bucket::new(&config.bucket_name, region, credentials)
        .map_err(|e| format!("Failed to create bucket: {}", e))?
        .with_path_style();

    // Upload with retry logic
    let mut attempts = 0;
    let mut last_error;
    let mut delay = Duration::from_millis(INITIAL_RETRY_DELAY_MS);

    loop {
        attempts += 1;

        match bucket
            .put_object_with_content_type(&key, &file_data, content_type)
            .await
        {
            Ok(response) => {
                if response.status_code() == 200 {
                    // Success!
                    let public_url =
                        format!("{}/{}", config.public_url_base.trim_end_matches('/'), key);

                    return Ok(UploadResult {
                        url: public_url,
                        key,
                        size: file_size,
                    });
                } else {
                    last_error = format!("R2 upload failed with status: {}", response.status_code());

                    // Check if status code is retryable
                    let status = response.status_code();
                    if (status == 502 || status == 503 || status == 504) && attempts < MAX_RETRIES {
                        eprintln!(
                            "Upload attempt {} failed (status {}), retrying in {:?}...",
                            attempts, status, delay
                        );
                        sleep(delay).await;
                        delay *= 2; // Exponential backoff
                        continue;
                    }

                    return Err(last_error);
                }
            }
            Err(e) => {
                last_error = format!("Failed to upload to R2: {}", e);

                // Check if error is transient and worth retrying
                if is_transient_error(&last_error) && attempts < MAX_RETRIES {
                    eprintln!(
                        "Upload attempt {} failed ({}), retrying in {:?}...",
                        attempts,
                        e,
                        delay
                    );
                    sleep(delay).await;
                    delay *= 2; // Exponential backoff
                    continue;
                }

                return Err(last_error);
            }
        }
    }
}
