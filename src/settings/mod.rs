use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // OS Platform tracking (e.g. "linux", "windows", "macos")
    #[serde(default)]
    pub os: Option<String>,

    // Terminal settings
    pub font_family: String,
    pub font_size: u32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub scrollback_lines: u32,

    // Appearance
    pub theme: String,
    pub color_scheme: String,
    #[serde(default = "default_accent")]
    pub accent_color: String,

    // Connection defaults
    pub default_port: u16,
    pub connection_timeout: u32,
    pub keepalive_interval: u32,
    pub global_username: Option<String>,

    // Behavior
    pub confirm_on_close: bool,
    pub save_window_size: bool,
    pub restore_sessions: bool,
    #[serde(default)]
    pub remember_expanded_folders: bool,
    #[serde(default)]
    pub expanded_folders: Vec<String>,
    #[serde(default)]
    pub single_click_launch: bool,

    // Paths
    pub download_directory: Option<String>,
    pub log_directory: Option<String>,

    // Logging
    pub logging_enabled: bool,
    pub log_format: String,
    pub log_timestamps: bool,
    pub debug_mode: bool,
}

fn default_accent() -> String {
    "cyan".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            os: Some(std::env::consts::OS.to_string()),
            font_family: "JetBrains Mono".to_string(),
            font_size: 14,
            cursor_style: "block".to_string(),
            cursor_blink: true,
            scrollback_lines: 10000,
            theme: "dark".to_string(),
            color_scheme: "GitHub Dark".to_string(),
            accent_color: "cyan".to_string(),
            default_port: 22,
            connection_timeout: 30,
            keepalive_interval: 60,
            global_username: None,
            confirm_on_close: true,
            save_window_size: true,
            restore_sessions: false,
            remember_expanded_folders: false,
            expanded_folders: Vec::new(),
            single_click_launch: false,
            download_directory: None,
            log_directory: None,
            logging_enabled: false,
            log_format: "{session}_{date}_{time}.log".to_string(),
            log_timestamps: true,
            debug_mode: false,
        }
    }
}

fn get_settings_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    Ok(app_dir.join("settings.json"))
}

pub fn get_default_log_dir() -> PathBuf {
    let base_docs = dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")));

    if let Some(docs) = base_docs {
        docs.join("SpurXSecureSSH").join("logs")
    } else {
        crate::get_app_config_dir().join("logs")
    }
}

pub fn is_valid_path_for_current_os(path_str: &str) -> bool {
    let path_str = path_str.trim();
    if path_str.is_empty() {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        if path_str.starts_with('/') {
            return false;
        }
        if !path_str.contains(':') && !path_str.starts_with(r"\\") {
            return false;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if (path_str.len() >= 2 && path_str.chars().nth(1) == Some(':'))
            || path_str.starts_with(r"\\")
        {
            return false;
        }
        if !path_str.starts_with('/') && !path_str.starts_with('~') {
            return false;
        }
    }

    true
}

pub fn load_settings() -> AppSettings {
    let mut settings = match get_settings_path() {
        Ok(path) => {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        serde_json::from_str::<AppSettings>(&content).unwrap_or_default()
                    }
                    Err(_) => AppSettings::default(),
                }
            } else {
                AppSettings::default()
            }
        }
        Err(_) => AppSettings::default(),
    };

    let current_os = std::env::consts::OS.to_string();
    let os_changed = settings
        .os
        .as_ref()
        .map(|o| o != &current_os)
        .unwrap_or(false);

    if os_changed {
        settings.os = Some(current_os.clone());
        if let Some(ref log_dir) = settings.log_directory {
            if !is_valid_path_for_current_os(log_dir) {
                settings.log_directory = None;
            }
        }
        if let Some(ref dl_dir) = settings.download_directory {
            if !is_valid_path_for_current_os(dl_dir) {
                settings.download_directory = None;
            }
        }
    } else {
        if settings.os.is_none() {
            settings.os = Some(current_os);
        }
        if let Some(ref log_dir) = settings.log_directory {
            if !is_valid_path_for_current_os(log_dir) {
                settings.log_directory = None;
            }
        }
    }

    if settings.font_family.contains(',') || settings.font_family.trim().is_empty() {
        settings.font_family = "JetBrains Mono".to_string();
    }

    settings
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_path()?;
    let mut settings_to_save = settings.clone();
    settings_to_save.os = Some(std::env::consts::OS.to_string());
    let content = serde_json::to_string_pretty(&settings_to_save)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write settings file: {}", e))?;
    Ok(())
}

pub fn get_available_system_fonts() -> Vec<String> {
    let mut font_list = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Primary bundled font always top of list
    let primary = "JetBrains Mono";
    font_list.push(primary.to_string());
    seen.insert(primary.to_string());

    // Curated popular terminal and SSH client fonts (SecureCRT, Windows Terminal, Warp)
    let curated = [
        "Lucida Console",
        "Consolas",
        "Cascadia Code",
        "Cascadia Mono",
        "Hack",
        "Fira Code",
        "Menlo",
        "Monaco",
        "SF Mono",
        "Liberation Mono",
        "DejaVu Sans Mono",
        "Ubuntu Mono",
        "Source Code Pro",
        "Inconsolata",
        "Courier New",
        "PT Mono",
        "IBM Plex Mono",
        "Monospace",
    ];
    for f in curated {
        if seen.insert(f.to_string()) {
            font_list.push(f.to_string());
        }
    }

    let mut discovered = std::collections::BTreeSet::new();

    // Linux font discovery via fc-list or font paths
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fc-list")
            .arg(":spacing=mono")
            .arg("family")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    for family in line.split(',') {
                        let name = family.trim();
                        if !name.is_empty() && !name.starts_with('.') {
                            discovered.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // macOS font discovery
    #[cfg(target_os = "macos")]
    {
        let font_dirs = ["/System/Library/Fonts", "/Library/Fonts"];
        for dir in font_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ["ttf", "otf", "ttc", "dfont"].contains(&ext.to_lowercase().as_str()) {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                if !stem.starts_with('.') {
                                    discovered.insert(stem.replace('-', " ").replace('_', " "));
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            let user_fonts = home.join("Library").join("Fonts");
            if let Ok(entries) = fs::read_dir(user_fonts) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if !stem.starts_with('.') {
                            discovered.insert(stem.replace('-', " ").replace('_', " "));
                        }
                    }
                }
            }
        }
    }

    // Windows font discovery
    #[cfg(target_os = "windows")]
    {
        let win_fonts = PathBuf::from(r"C:\Windows\Fonts");
        if let Ok(entries) = fs::read_dir(win_fonts) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if !stem.starts_with('.') {
                        discovered.insert(stem.replace('-', " ").replace('_', " "));
                    }
                }
            }
        }
    }

    for f in discovered {
        if seen.insert(f.clone()) {
            font_list.push(f);
        }
    }

    font_list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_available_system_fonts() {
        let fonts = get_available_system_fonts();
        assert!(!fonts.is_empty());
        assert!(fonts.iter().any(|f| f == "Cascadia Code" || f == "JetBrains Mono" || f == "Monospace"));
    }

    #[test]
    fn test_default_log_dir_location() {
        let log_dir = get_default_log_dir();
        assert!(
            log_dir.ends_with("SpurXSecureSSH/logs")
                || log_dir.ends_with("SpurXSecureSSH\\logs")
                || log_dir.ends_with("logs")
        );
    }

    #[test]
    fn test_path_os_validation() {
        #[cfg(target_os = "linux")]
        {
            assert!(is_valid_path_for_current_os("/home/john/Documents/logs"));
            assert!(!is_valid_path_for_current_os(
                r"C:\Users\john\Documents\logs"
            ));
        }

        #[cfg(target_os = "windows")]
        {
            assert!(is_valid_path_for_current_os(
                r"C:\Users\john\Documents\logs"
            ));
            assert!(!is_valid_path_for_current_os("/home/john/Documents/logs"));
        }
    }
}
