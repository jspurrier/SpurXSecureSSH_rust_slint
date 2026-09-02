pub mod ai;
pub mod app_state;
pub mod clipboard;
pub mod command_storage;
pub mod import_export;
pub mod keys;
pub mod known_hosts;
pub mod session;
pub mod settings;
pub mod sftp;
pub mod ssh;
pub mod terminal;
pub mod theme;
pub mod tunnels;
pub mod ymodem;

slint::include_modules!();

pub use app_state::{AppState, ConnectRequest, ConnectResponse, QuickCommand, SavedSession};

pub fn get_default_config_dir() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("SpurXSecureSSH")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".spurx-secure-ssh")
    }
}

pub fn get_app_config_dir() -> std::path::PathBuf {
    let default_dir = get_default_config_dir();
    let pointer_file = default_dir.join("storage_path.txt");
    if pointer_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&pointer_file) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let custom_dir = std::path::PathBuf::from(trimmed);
                if custom_dir.is_absolute() {
                    return custom_dir;
                }
            }
        }
    }
    default_dir
}


