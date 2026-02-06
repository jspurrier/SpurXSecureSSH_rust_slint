//! Application settings management
//! Stores settings as JSON in user's config directory

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // Terminal settings
    pub font_family: String,
    pub font_size: u32,
    pub cursor_style: String, // "block" | "underline" | "bar"
    pub cursor_blink: bool,
    pub scrollback_lines: u32,

    // Appearance
    pub theme: String, // "dark" | "light"
    pub color_scheme: String,

    // Connection defaults
    pub default_port: u16,
    pub connection_timeout: u32,
    pub keepalive_interval: u32,

    // Behavior
    pub confirm_on_close: bool,
    pub save_window_size: bool,
    pub restore_sessions: bool,

    // Paths
    pub download_directory: Option<String>,
    pub log_directory: Option<String>,

    // Logging
    pub logging_enabled: bool,
    pub log_format: String,
    pub log_timestamps: bool,
    pub debug_mode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            // Terminal
            font_family: "Cascadia Code, JetBrains Mono, Fira Code, Consolas, monospace"
                .to_string(),
            font_size: 14,
            cursor_style: "block".to_string(),
            cursor_blink: true,
            scrollback_lines: 10000,

            // Appearance
            theme: "dark".to_string(),
            color_scheme: "GitHub Dark".to_string(),

            // Connection
            default_port: 22,
            connection_timeout: 30,
            keepalive_interval: 60,

            // Behavior
            confirm_on_close: true,
            save_window_size: true,
            restore_sessions: false,

            // Paths
            download_directory: None,
            log_directory: None,

            // Logging
            logging_enabled: false,
            log_format: "{session}_{date}_{time}.log".to_string(),
            log_timestamps: true,
            debug_mode: false,
        }
    }
}

/// Get the settings file path
/// Get the settings file path
fn get_settings_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();

    // Ensure directory exists
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(app_dir.join("settings.json"))
}

/// Load settings from disk
pub fn load_settings() -> AppSettings {
    match get_settings_path() {
        Ok(path) => {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                    Err(_) => AppSettings::default(),
                }
            } else {
                AppSettings::default()
            }
        }
        Err(_) => AppSettings::default(),
    }
}

/// Save settings to disk
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_path()?;

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write settings file: {}", e))?;

    Ok(())
}
