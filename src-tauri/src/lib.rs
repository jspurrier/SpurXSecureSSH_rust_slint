use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub mod ai;
pub mod command_storage;
pub mod commands;
pub mod import_export;
pub mod keys;
pub mod known_hosts;
pub mod session;
pub mod settings;
pub mod sftp;
pub mod ssh;
pub mod ymodem;

/// Application state shared across Tauri commands
pub struct AppState {
    /// Active SSH connections indexed by session ID
    pub connections: RwLock<HashMap<String, Arc<ssh::SshConnection>>>,
    /// Channel senders for terminal input, indexed by session ID (tokio async for russh)
    pub input_senders: RwLock<HashMap<String, tokio::sync::mpsc::UnboundedSender<ssh::SshInput>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            input_senders: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// SSH connection request from frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub log_directory: Option<String>,
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    pub private_key_name: Option<String>,
    pub private_key_passphrase: Option<String>,
}

fn default_auth_method() -> String {
    "password".to_string()
}

/// SSH connection response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectResponse {
    pub session_id: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Saved session configuration (no credentials stored)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub folder: Option<String>,
    pub username: Option<String>,
    pub log_directory: Option<String>,
    #[serde(default)]
    pub use_global_credentials: bool,
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    pub private_key_name: Option<String>,
}

impl SavedSession {
    pub fn new(
        name: String,
        host: String,
        port: u16,
        folder: Option<String>,
        username: Option<String>,
        log_directory: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            host,
            port,
            folder,
            username,
            log_directory,
            use_global_credentials: false,
            auth_method: "password".to_string(),
            private_key_name: None,
        }
    }
}

/// Quick command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    pub command: String,
    pub folder: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub example: Option<String>,
}

/// Get the default application configuration directory.
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

/// Get the standardized application configuration directory.
///
/// - Windows: %APPDATA%/SpurXSecureSSH
/// - Linux/macOS: ~/.spurx-secure-ssh
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::ssh_connect,
            commands::ssh_disconnect,
            commands::ssh_write,
            commands::ssh_resize,
            commands::get_saved_sessions,
            commands::save_session,
            commands::delete_session,
            commands::save_session_password,
            commands::get_session_password,
            commands::delete_session_password,
            commands::save_global_password,
            commands::get_global_password,
            commands::delete_global_password,
            commands::get_quick_commands,
            commands::save_quick_command,
            commands::delete_quick_command,
            commands::ai_chat,
            commands::get_ai_config,
            commands::save_ai_config,
            commands::get_settings,
            commands::save_settings,
            commands::import_sessions,
            commands::export_sessions,
            commands::write_log,
            commands::get_log_directory,
            commands::sftp_connect,
            commands::sftp_disconnect,
            commands::sftp_list,
            commands::sftp_download,
            commands::sftp_upload,
            commands::sftp_mkdir,
            commands::sftp_delete_file,
            commands::sftp_delete_dir,
            commands::sftp_rename,
            commands::get_known_hosts,
            commands::delete_known_host,
            commands::check_latest_version,
            commands::ssh_start_ymodem_send,
            commands::ssh_cancel_ymodem,
            commands::generate_key_pair,
            commands::list_keys,
            commands::delete_key,
            commands::is_master_password_set,
            commands::change_master_password,
            commands::local_list_dir,
            commands::local_get_home_dir,
            commands::local_delete,
            commands::get_storage_info,
            commands::set_storage_location,
            commands::reset_storage_location,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
