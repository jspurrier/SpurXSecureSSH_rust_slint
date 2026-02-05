use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub mod ai;
pub mod command_storage;
pub mod commands;
pub mod import_export;
pub mod known_hosts;
pub mod session;
pub mod settings;
pub mod sftp;
pub mod ssh;

/// Application state shared across Tauri commands
pub struct AppState {
    /// Active SSH connections indexed by session ID
    pub connections: RwLock<HashMap<String, Arc<ssh::SshConnection>>>,
    /// Channel senders for terminal input, indexed by session ID (std::sync for blocking ssh2)
    pub input_senders: RwLock<HashMap<String, std::sync::mpsc::Sender<Vec<u8>>>>,
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
    pub password: String,
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
}

impl SavedSession {
    pub fn new(
        name: String,
        host: String,
        port: u16,
        folder: Option<String>,
        username: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            host,
            port,
            folder,
            username,
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

/// Get the standardized application configuration directory.
///
/// - Windows: %APPDATA%/SpurXSecureSSH
/// - Linux/macOS: ~/.spurx-secure-ssh
pub fn get_app_config_dir() -> std::path::PathBuf {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::ssh_connect,
            commands::ssh_disconnect,
            commands::ssh_write,
            commands::get_saved_sessions,
            commands::save_session,
            commands::delete_session,
            commands::save_session_password,
            commands::get_session_password,
            commands::delete_session_password,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
