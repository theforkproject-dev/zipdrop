use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "com.metalayer.zipdrop";

/// R2 configuration - secrets stored in Keychain, non-secrets in file
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct R2Config {
    pub access_key: String,
    pub secret_key: String,
    pub bucket_name: String,
    pub account_id: String,
    pub public_url_base: String,
}

/// Non-secret config stored in file
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct StoredConfig {
    bucket_name: String,
    account_id: String,
    public_url_base: String,
}

/// Secrets stored as single JSON blob in keychain (one prompt instead of two)
#[derive(Debug, Clone, Deserialize, Serialize)]
struct KeychainSecrets {
    access_key: String,
    secret_key: String,
}

/// App settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default = "default_demo_mode")]
    pub demo_mode: bool,
    pub demo_output_dir: Option<String>,
}

fn default_demo_mode() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            demo_mode: true, // Demo mode ON by default
            demo_output_dir: None,
        }
    }
}

fn get_config_dir() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "Could not find config directory".to_string())?
        .join("zipdrop");
    
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;
    
    Ok(config_dir)
}

fn get_config_path() -> Result<PathBuf, String> {
    Ok(get_config_dir()?.join("config.json"))
}

fn get_settings_path() -> Result<PathBuf, String> {
    Ok(get_config_dir()?.join("settings.json"))
}

/// Save R2 config - secrets go to Keychain, rest to file
pub fn save_r2_config(config: &R2Config) -> Result<(), String> {
    println!("[zipdrop] Saving R2 config...");
    
    // Store both secrets as single JSON blob in Keychain (one prompt instead of two)
    let secrets = KeychainSecrets {
        access_key: config.access_key.clone(),
        secret_key: config.secret_key.clone(),
    };
    let secrets_json = serde_json::to_string(&secrets)
        .map_err(|e| format!("Failed to serialize secrets: {}", e))?;
    
    let secrets_entry = Entry::new(SERVICE_NAME, "r2_credentials")
        .map_err(|e| format!("Keychain error creating credentials entry: {}", e))?;
    secrets_entry
        .set_password(&secrets_json)
        .map_err(|e| format!("Failed to store credentials in keychain: {}", e))?;
    println!("[zipdrop] Credentials saved to keychain");

    // Store non-secrets in file
    let stored = StoredConfig {
        bucket_name: config.bucket_name.clone(),
        account_id: config.account_id.clone(),
        public_url_base: config.public_url_base.clone(),
    };

    let config_path = get_config_path()?;
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    fs::write(&config_path, json)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    Ok(())
}

/// Clean up old keychain entries from the two-entry format
/// Only runs once (tracks completion with a marker file)
pub fn migrate_keychain_entries() {
    // Check if migration already completed
    let marker_path = get_config_dir().ok().map(|d| d.join(".migrated_v1"));
    if let Some(ref path) = marker_path {
        if path.exists() {
            return; // Already migrated
        }
    }
    
    println!("[zipdrop] Cleaning up old keychain entries...");
    
    // Delete old separate entries - this may prompt but only once ever
    if let Ok(entry) = Entry::new(SERVICE_NAME, "r2_access_key") {
        match entry.delete_credential() {
            Ok(_) => println!("[zipdrop] Deleted old r2_access_key entry"),
            Err(e) => println!("[zipdrop] r2_access_key: {}", e),
        }
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, "r2_secret_key") {
        match entry.delete_credential() {
            Ok(_) => println!("[zipdrop] Deleted old r2_secret_key entry"),
            Err(e) => println!("[zipdrop] r2_secret_key: {}", e),
        }
    }
    
    // Mark migration as complete
    if let Some(path) = marker_path {
        let _ = fs::write(&path, "1");
    }
    
    println!("[zipdrop] Old keychain cleanup complete");
}

/// Load R2 config - combine Keychain secrets with file config
pub fn load_r2_config() -> Result<Option<R2Config>, String> {
    println!("[zipdrop] Loading R2 config...");
    let config_path = get_config_path()?;
    
    if !config_path.exists() {
        println!("[zipdrop] No config file found at {:?}", config_path);
        return Ok(None);
    }

    // Load non-secrets from file
    let json = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;
    
    let stored: StoredConfig = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse config: {}", e))?;
    println!("[zipdrop] Loaded config file: bucket={}", stored.bucket_name);

    // Load secrets from Keychain (single entry = single prompt)
    let secrets_entry = Entry::new(SERVICE_NAME, "r2_credentials")
        .map_err(|e| format!("Keychain error: {}", e))?;
    
    let secrets_result = secrets_entry.get_password();
    println!("[zipdrop] Credentials from keychain: {:?}", secrets_result.as_ref().map(|_| "****"));

    match secrets_result.ok() {
        Some(secrets_json) if !secrets_json.is_empty() => {
            let secrets: KeychainSecrets = serde_json::from_str(&secrets_json)
                .map_err(|e| format!("Failed to parse keychain secrets: {}", e))?;
            
            if secrets.access_key.is_empty() || secrets.secret_key.is_empty() {
                println!("[zipdrop] Empty credentials in keychain, returning None");
                return Ok(None);
            }
            
            println!("[zipdrop] R2 config loaded successfully");
            Ok(Some(R2Config {
                access_key: secrets.access_key,
                secret_key: secrets.secret_key,
                bucket_name: stored.bucket_name,
                account_id: stored.account_id,
                public_url_base: stored.public_url_base,
            }))
        }
        _ => {
            println!("[zipdrop] Missing keychain credentials, returning None");
            Ok(None)
        }
    }
}

/// Delete R2 config
pub fn delete_r2_config() -> Result<(), String> {
    // Remove from Keychain (new single entry)
    if let Ok(entry) = Entry::new(SERVICE_NAME, "r2_credentials") {
        let _ = entry.delete_credential();
    }
    // Also clean up old separate entries if they exist (migration cleanup)
    if let Ok(entry) = Entry::new(SERVICE_NAME, "r2_access_key") {
        let _ = entry.delete_credential();
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, "r2_secret_key") {
        let _ = entry.delete_credential();
    }

    // Remove config file
    let config_path = get_config_path()?;
    if config_path.exists() {
        fs::remove_file(&config_path)
            .map_err(|e| format!("Failed to delete config: {}", e))?;
    }

    Ok(())
}

/// Save app settings
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let settings_path = get_settings_path()?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}

/// Load app settings
pub fn load_settings() -> Result<AppSettings, String> {
    let settings_path = get_settings_path()?;
    
    if !settings_path.exists() {
        return Ok(AppSettings::default());
    }

    let json = fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    
    serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

/// Get the demo output directory
pub fn get_demo_output_dir() -> Result<PathBuf, String> {
    let downloads = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "Could not find downloads directory".to_string())?;
    
    let demo_dir = downloads.join("ZipDrop");
    fs::create_dir_all(&demo_dir)
        .map_err(|e| format!("Failed to create demo directory: {}", e))?;
    
    Ok(demo_dir)
}
