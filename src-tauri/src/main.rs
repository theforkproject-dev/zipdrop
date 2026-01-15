// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod processor;
mod uploader;

use config::{
    delete_r2_config, get_demo_output_dir, load_r2_config, load_settings, migrate_keychain_entries,
    save_r2_config, save_settings, AppSettings, R2Config,
};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{
    include_image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, PhysicalPosition,
};
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

/// App state
pub struct AppState {
    pub r2_config: Mutex<Option<R2Config>>,
    pub settings: Mutex<AppSettings>,
}

/// Combined result from processing and uploading
#[derive(Debug, Clone, serde::Serialize)]
pub struct DropResult {
    pub url: String,
    pub local_path: Option<String>,
    pub r2_key: Option<String>,
    pub original_size: u64,
    pub processed_size: u64,
    pub file_type: String,
    pub is_demo: bool,
}

/// Config status for frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigStatus {
    pub is_configured: bool,
    pub demo_mode: bool,
    pub bucket_name: Option<String>,
}

/// Set R2 configuration (saves to Keychain)
#[tauri::command]
fn set_r2_config(state: tauri::State<'_, AppState>, config: R2Config) -> Result<(), String> {
    // Save to secure storage
    save_r2_config(&config)?;

    // Update in-memory state
    let mut r2_config = state.r2_config.lock().map_err(|e| e.to_string())?;
    *r2_config = Some(config);

    // Disable demo mode when R2 is configured
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.demo_mode = false;
    save_settings(&settings)?;

    Ok(())
}

/// Get current config status
#[tauri::command]
fn get_config_status(state: tauri::State<'_, AppState>) -> ConfigStatus {
    let r2_config = state.r2_config.lock().ok();
    let settings = state.settings.lock().ok();

    ConfigStatus {
        is_configured: r2_config.as_ref().and_then(|c| c.as_ref()).is_some(),
        demo_mode: settings.map(|s| s.demo_mode).unwrap_or(true),
        bucket_name: r2_config
            .and_then(|c| c.as_ref().map(|cfg| cfg.bucket_name.clone())),
    }
}

/// Get R2 config (for populating settings form)
#[tauri::command]
fn get_r2_config(state: tauri::State<'_, AppState>) -> Option<R2Config> {
    state.r2_config.lock().ok().and_then(|c| c.clone())
}

/// Enable/disable demo mode
#[tauri::command]
fn set_demo_mode(state: tauri::State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.demo_mode = enabled;
    save_settings(&settings)?;
    Ok(())
}

/// Delete R2 configuration
#[tauri::command]
fn clear_r2_config(state: tauri::State<'_, AppState>) -> Result<(), String> {
    delete_r2_config()?;

    let mut r2_config = state.r2_config.lock().map_err(|e| e.to_string())?;
    *r2_config = None;

    Ok(())
}

/// Copy to clipboard helper - handles errors gracefully
fn copy_text_to_clipboard(text: &str) {
    // Use a small delay to avoid clipboard contention
    std::thread::sleep(std::time::Duration::from_millis(50));
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text);
    }
}

/// Process and upload files - the main workflow
#[tauri::command]
async fn process_and_upload(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<DropResult, String> {
    println!("[zipdrop] process_and_upload called with {} files", paths.len());
    
    // Convert strings to PathBufs
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();

    if path_bufs.is_empty() {
        return Err("No files provided".to_string());
    }

    // Check settings
    let is_demo = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.demo_mode
    };
    
    println!("[zipdrop] demo_mode: {}", is_demo);

    // Get output directory
    let output_dir = if is_demo {
        get_demo_output_dir()?
    } else {
        std::env::temp_dir().join("zipdrop")
    };
    
    println!("[zipdrop] output_dir: {:?}", output_dir);

    // Process files (compress/zip)
    println!("[zipdrop] Starting file processing...");
    let process_result = processor::process_files(path_bufs, &output_dir)?;
    println!("[zipdrop] Processing complete: {:?}", process_result.output_path);

    if is_demo {
        // Demo mode: just return the local path
        let local_path = process_result.output_path.to_string_lossy().to_string();

        // Copy path to clipboard
        copy_text_to_clipboard(&local_path);

        Ok(DropResult {
            url: format!("file://{}", local_path),
            local_path: Some(local_path),
            r2_key: None,
            original_size: process_result.original_size,
            processed_size: process_result.processed_size,
            file_type: process_result.file_type,
            is_demo: true,
        })
    } else {
        // Production mode: upload to R2
        let r2_config = {
            let config_guard = state.r2_config.lock().map_err(|e| e.to_string())?;
            config_guard.clone().ok_or_else(|| {
                "R2 not configured. Please set up your R2 credentials or enable demo mode."
                    .to_string()
            })?
        };

        // Upload to R2
        let upload_result =
            uploader::upload_to_r2(&process_result.output_path, &r2_config).await?;

        // Copy URL to clipboard
        copy_text_to_clipboard(&upload_result.url);

        // Clean up temp file
        let _ = std::fs::remove_file(&process_result.output_path);

        Ok(DropResult {
            url: upload_result.url,
            local_path: None,
            r2_key: Some(upload_result.key),
            original_size: process_result.original_size,
            processed_size: process_result.processed_size,
            file_type: process_result.file_type,
            is_demo: false,
        })
    }
}

/// Copy text to clipboard
#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;
    clipboard
        .set_text(&text)
        .map_err(|e| format!("Failed to copy: {}", e))?;
    Ok(())
}

/// Open a path in Finder
#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .args(["-R", &path])
        .spawn()
        .map_err(|e| format!("Failed to open Finder: {}", e))?;
    Ok(())
}

/// Open a URL in the default browser
#[tauri::command]
fn open_in_browser(url: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}

/// Delete an object from R2 (fire-and-forget, errors are logged but not returned)
#[tauri::command]
async fn delete_from_r2(state: tauri::State<'_, AppState>, key: String) -> Result<(), String> {
    let r2_config = {
        let config_guard = state.r2_config.lock().map_err(|e| e.to_string())?;
        config_guard.clone().ok_or_else(|| "R2 not configured".to_string())?
    };

    uploader::delete_from_r2(&key, &r2_config).await
}

/// Validate R2 credentials before saving
#[tauri::command]
async fn validate_r2_config(config: R2Config) -> Result<(), String> {
    uploader::validate_r2_credentials(&config).await
}

fn main() {
    // Migrate old keychain entries (one-time cleanup)
    migrate_keychain_entries();
    
    // Load persisted config on startup
    let r2_config = load_r2_config().ok().flatten();
    let settings = load_settings().unwrap_or_else(|_| AppSettings {
        demo_mode: true, // Default to demo mode
        demo_output_dir: None,
    });

    tauri::Builder::default()
        .manage(AppState {
            r2_config: Mutex::new(r2_config),
            settings: Mutex::new(settings),
        })
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();

            // Apply native macOS vibrancy effect
            #[cfg(target_os = "macos")]
            apply_vibrancy(&window, NSVisualEffectMaterial::Menu, None, Some(12.0))
                .expect("Failed to apply vibrancy");

            // Build tray menu
            let version = app.package_info().version.to_string();
            let version_item = MenuItem::with_id(app, "version", format!("Version {}", version), false, None::<&str>)?;
            let check_updates = MenuItem::with_id(app, "check_updates", "Check for Updates...", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit ZipDrop", true, Some("CmdOrCtrl+Q"))?;
            let tray_menu = Menu::with_items(app, &[&version_item, &check_updates, &separator, &quit_item])?;

            // Build tray icon with custom icon
            let tray_icon = include_image!("icons/tray-icon.png");
            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let tray_pos = rect.position.to_physical::<i32>(1.0);
                            let tray_size = rect.size.to_physical::<u32>(1.0);

                            if let Ok(window_size) = window.outer_size() {
                                let window_width = window_size.width as i32;
                                let x = tray_pos.x - (window_width / 2)
                                    + (tray_size.width as i32 / 2);
                                let y = tray_pos.y + tray_size.height as i32 + 4;
                                let _ = window.set_position(PhysicalPosition::new(x, y));
                            }

                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => app.exit(0),
                        "check_updates" => {
                            let _ = std::process::Command::new("open")
                                .arg("https://github.com/theforkproject-dev/zipdrop/releases")
                                .spawn();
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            Ok(())
        })
        // Note: Auto-hide on blur disabled to allow drag-and-drop from Finder
        // Users can click the tray icon again or press Escape to close
        .invoke_handler(tauri::generate_handler![
            set_r2_config,
            get_r2_config,
            get_config_status,
            set_demo_mode,
            clear_r2_config,
            process_and_upload,
            copy_to_clipboard,
            reveal_in_finder,
            open_in_browser,
            delete_from_r2,
            validate_r2_config
        ])
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
