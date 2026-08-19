//! Tauri command handlers for SSH operations and session management

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{command, AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::command_storage;
use crate::session;
use crate::ssh::{SshConnection, SshInput};
use crate::{AppState, ConnectRequest, ConnectResponse, QuickCommand, SavedSession};

/// Connect to an SSH server
#[command]
pub async fn ssh_connect(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: ConnectRequest,
) -> Result<ConnectResponse, String> {
    let session_id = Uuid::new_v4().to_string();

    // Create output channel for terminal data (tokio async)
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(1024);

    // Create input channel for user input (tokio async for russh)
    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<SshInput>();

    // Store input sender in state (wrapped for async compatibility)
    {
        let mut senders = state.input_senders.write();
        senders.insert(session_id.clone(), input_tx);
    }

    // Create connection object
    let connection = Arc::new(SshConnection::new(
        session_id.clone(),
        request.host.clone(),
        request.port,
        request.username.clone(),
    ));

    // Store connection in state
    {
        let mut connections = state.connections.write();
        connections.insert(session_id.clone(), connection.clone());
    }

    let host = request.host.clone();
    let port = request.port;
    let username = request.username.clone();
    let password = request.password.clone();
    let auth_method = request.auth_method.clone();
    let private_key_name = request.private_key_name.clone();
    let private_key_passphrase = request.private_key_passphrase.clone();
    let connection_for_task = connection.clone();

    // Spawn connection task - using spawn_blocking for ssh2
    let app_handle = app.clone();
    let sid = session_id.clone();

    // Forward output to frontend in async task
    let app_for_output = app_handle.clone();
    let sid_for_output = sid.clone();

    // Setup logging
    let settings = crate::settings::load_settings();
    let mut log_file = None;
    if settings.logging_enabled {
        let log_dir_str = request
            .log_directory
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| settings.log_directory.clone())
            .unwrap_or_else(|| {
                let docs_dir = dirs::document_dir().unwrap_or_default();
                docs_dir
                    .join("SpurXSecureSSH")
                    .join("logs")
                    .to_string_lossy()
                    .to_string()
            });

        let log_dir = std::path::Path::new(&log_dir_str);
        let _ = std::fs::create_dir_all(log_dir);

        let now = chrono::Local::now();
        let date = now.format("%Y%m%d").to_string();
        let time = now.format("%H%M%S").to_string();

        let file_name = settings
            .log_format
            .replace("{session}", &request.host)
            .replace("{date}", &date)
            .replace("{time}", &time);

        let file_path = log_dir.join(&file_name);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .ok();

        if let Some(ref mut f) = file {
            if settings.log_timestamps {
                let start_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                use std::io::Write;
                let _ =
                    f.write_all(format!("\n--- Session Started: {} ---\n", start_time).as_bytes());
            }
        }
        log_file = file;
    }

    tokio::spawn(async move {
        use std::io::Write;
        while let Some(data) = output_rx.recv().await {
            if let Some(ref mut file) = log_file {
                let stripped_data = strip_ansi_escapes::strip(&data);
                let _ = file.write_all(&stripped_data);
            }
            // Emit terminal data to frontend
            let _ = app_for_output.emit(&format!("terminal-data-{}", sid_for_output), data);
        }
    });

    // Spawn async SSH connection in tokio
    let sid_for_ssh = sid.clone();
    let app_for_ssh = app_handle.clone();
    tokio::spawn(async move {
        let mut input_rx = input_rx;
        // Load settings to check debug mode
        let settings = crate::settings::load_settings();
        let debug_mode = settings.debug_mode;

        use crate::ssh::connect_ssh_async;

        match connect_ssh_async(
            &sid_for_ssh,
            &host,
            port,
            &username,
            password.as_deref(),
            &auth_method,
            private_key_name.as_deref(),
            private_key_passphrase.as_deref(),
            output_tx,
            &mut input_rx,
            debug_mode,
            app_for_ssh.clone(),
        ).await {
            Ok(_) => {
                connection_for_task.set_connected(false);
                let _ = app_for_ssh.emit(&format!("session-closed-{}", sid_for_ssh), ());
            }
            Err(e) => {
                eprintln!("SSH error for session {}: {}", sid_for_ssh, e);
                connection_for_task.set_connected(false);
                let _ = app_for_ssh.emit(&format!("session-error-{}", sid_for_ssh), e.clone());
                let _ = app_for_ssh.emit(&format!("session-closed-{}", sid_for_ssh), ());
            }
        }
    });

    Ok(ConnectResponse {
        session_id,
        success: true,
        error: None,
    })
}

/// Disconnect from an SSH session
#[command]
pub async fn ssh_disconnect(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    // Remove input sender to close the channel
    {
        let mut senders = state.input_senders.write();
        senders.remove(&session_id);
    }

    // Remove connection
    {
        let mut connections = state.connections.write();
        if let Some(conn) = connections.remove(&session_id) {
            conn.set_connected(false);
        }
    }

    Ok(())
}

/// Write data to an SSH session
#[command]
pub async fn ssh_write(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let senders = state.input_senders.read();
    if let Some(sender) = senders.get(&session_id) {
        sender
            .send(SshInput::Data(data))
            .map_err(|e| format!("Failed to send data: {}", e))?;
    }
    Ok(())
}

/// Resize the SSH PTY
#[command]
pub async fn ssh_resize(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let senders = state.input_senders.read();
    if let Some(sender) = senders.get(&session_id) {
        sender
            .send(SshInput::Resize { rows, cols })
            .map_err(|e| format!("Failed to send resize command: {}", e))?;
    }
    Ok(())
}

/// Get list of saved sessions
#[command]
pub async fn get_saved_sessions(password: Option<String>) -> Result<Vec<SavedSession>, String> {
    session::load_sessions_secure(password.as_deref())
}

/// Save a new session
#[command]
pub async fn save_session(session: SavedSession, password: Option<String>) -> Result<(), String> {
    session::save_session_secure(session, password.as_deref())
}

/// Delete a session
#[command]
pub async fn delete_session(session_id: String, password: Option<String>) -> Result<(), String> {
    session::delete_session_secure(&session_id, password.as_deref())
}

/// Save session password securely
#[command]
pub async fn save_session_password(session_id: String, password: String) -> Result<(), String> {
    session::save_password(&session_id, &password)
}

/// Get session password securely
#[command]
pub async fn get_session_password(session_id: String) -> Result<Option<String>, String> {
    session::get_password(&session_id)
}

/// Delete session password securely
#[command]
pub async fn delete_session_password(session_id: String) -> Result<(), String> {
    session::delete_password(&session_id)
}

/// Save global password securely
#[command]
pub async fn save_global_password(password: String) -> Result<(), String> {
    session::save_password("__global__", &password)
}

/// Get global password securely
#[command]
pub async fn get_global_password() -> Result<Option<String>, String> {
    session::get_password("__global__")
}

/// Delete global password securely
#[command]
pub async fn delete_global_password() -> Result<(), String> {
    session::delete_password("__global__")
}

/// Get list of quick commands
#[command]
pub async fn get_quick_commands() -> Result<Vec<QuickCommand>, String> {
    command_storage::load_commands()
}

/// Save a quick command
#[command]
pub async fn save_quick_command(command: QuickCommand) -> Result<(), String> {
    command_storage::save_command(command)
}

/// Delete a quick command
#[command]
pub async fn delete_quick_command(command_id: String) -> Result<(), String> {
    command_storage::delete_command(&command_id)
}

// ============================================
// AI Assistant Commands
// ============================================

use crate::ai::{self, AiConfig, AiMessage};

/// Chat with AI assistant
#[command]
pub async fn ai_chat(provider: String, messages: Vec<AiMessage>) -> Result<String, String> {
    ai::chat(&provider, messages).await
}

/// Get AI configuration
#[command]
pub async fn get_ai_config() -> Result<AiConfig, String> {
    Ok(ai::load_config())
}

/// Save AI configuration
#[command]
pub async fn save_ai_config(config: AiConfig) -> Result<(), String> {
    ai::save_config(&config)
}

// ============================================
// Settings Commands
// ============================================

use crate::settings::{self, AppSettings};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCheckResponse {
    pub update_available: bool,
    pub latest_version: String,
    pub current_version: String,
    pub url: String,
    pub error: Option<String>,
}

/// Check for latest version on GitHub
#[command]
pub async fn check_latest_version(app: AppHandle) -> Result<VersionCheckResponse, String> {
    let current_version = app.package_info().version.to_string();
    let url = "https://api.github.com/repos/jspurrier/SpurXSecureSSH_rust_Public/tags";

    let client = reqwest::Client::builder()
        .user_agent("spurx-secure-ssh")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch tags: {}", e))?;

    if !response.status().is_success() {
        return Ok(VersionCheckResponse {
            update_available: false,
            latest_version: "".to_string(),
            current_version,
            url: "".to_string(),
            error: Some(format!("GitHub API returned error: {}", response.status())),
        });
    }

    let tags: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let mut max_version = semver::Version::new(0, 0, 0);
    let mut max_tag_name = "".to_string();

    for tag in tags {
        if let Some(name) = tag.get("name").and_then(|n| n.as_str()) {
            // Strip 'v' prefix if present
            let clean_name = name.strip_prefix('v').unwrap_or(name);
            if let Ok(version) = semver::Version::parse(clean_name) {
                if version > max_version {
                    max_version = version;
                    max_tag_name = name.to_string();
                }
            }
        }
    }

    let current_semver = semver::Version::parse(&current_version)
        .map_err(|e| format!("Failed to parse current version: {}", e))?;

    let update_available = max_version > current_semver;
    let release_url = if update_available {
        format!(
            "https://github.com/jspurrier/SpurXSecureSSH_rust_Public/releases/tag/{}",
            max_tag_name
        )
    } else {
        "".to_string()
    };

    Ok(VersionCheckResponse {
        update_available,
        latest_version: max_version.to_string(),
        current_version,
        url: release_url,
        error: None,
    })
}

/// Get application settings
#[command]
pub async fn get_settings() -> Result<AppSettings, String> {
    Ok(settings::load_settings())
}

/// Save application settings
#[command]
pub async fn save_settings(settings: AppSettings) -> Result<(), String> {
    settings::save_settings(&settings)
}

use crate::known_hosts::{self, KnownHostEntry};

/// Get known hosts
#[command]
pub async fn get_known_hosts() -> Result<Vec<KnownHostEntry>, String> {
    known_hosts::get_known_hosts()
}

/// Delete a known host
#[command]
pub async fn delete_known_host(host: String) -> Result<(), String> {
    known_hosts::delete_known_host(&host)
}

// ============================================
// Import/Export Commands
// ============================================

use crate::import_export;

/// Import sessions and commands from a file
#[command]
pub async fn import_sessions(
    path: String,
    format: String,
    password: Option<String>,
) -> Result<import_export::ImportPayload, String> {
    import_export::import_sessions(&path, &format, password)
}

/// Export sessions and commands to a file
#[command]
pub async fn export_sessions(
    path: String,
    format: String,
    sessions: Vec<SavedSession>,
    commands: Vec<crate::QuickCommand>,
    password: Option<String>,
) -> Result<(), String> {
    import_export::export_sessions(&path, &format, &sessions, &commands, password)
}

// ============================================
// Logging Commands
// ============================================

use std::fs::OpenOptions;
use std::io::Write;

/// Write data to a log file (appends to existing file)
#[command]
pub async fn write_log(
    file_path: String,
    data: String,
    with_timestamp: bool,
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;

    let content = if with_timestamp {
        let now = chrono::Local::now();
        format!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), data)
    } else {
        data
    };

    file.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write to log file: {}", e))?;

    Ok(())
}

/// Get the default log directory
#[command]
pub async fn get_log_directory() -> Result<String, String> {
    let docs_dir =
        dirs::document_dir().ok_or_else(|| "Could not find documents directory".to_string())?;

    let log_dir = docs_dir.join("SpurXSecureSSH").join("logs");

    // Ensure directory exists
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log directory: {}", e))?;
    }

    Ok(log_dir.to_string_lossy().to_string())
}

// ============================================
// SFTP Commands
// ============================================

use crate::sftp::{SftpFileEntry, SFTP_MANAGER};

/// Connect SFTP session
#[command]
pub async fn sftp_connect(
    session_id: String,
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    auth_method: String,
    private_key_name: Option<String>,
    private_key_passphrase: Option<String>,
) -> Result<(), String> {
    SFTP_MANAGER
        .connect(
            &session_id,
            &host,
            port,
            &username,
            password.as_deref(),
            &auth_method,
            private_key_name.as_deref(),
            private_key_passphrase.as_deref(),
        )
        .await
}

/// Disconnect SFTP session
#[command]
pub async fn sftp_disconnect(session_id: String) -> Result<(), String> {
    SFTP_MANAGER.disconnect(&session_id).await;
    Ok(())
}

/// List directory contents
#[command]
pub async fn sftp_list(session_id: String, path: String) -> Result<Vec<SftpFileEntry>, String> {
    SFTP_MANAGER.list_dir(&session_id, &path).await
}

/// Download file from remote to local
#[command]
pub async fn sftp_download(
    app: AppHandle,
    session_id: String,
    remote_path: String,
    local_path: String,
) -> Result<u64, String> {
    SFTP_MANAGER
        .download(&session_id, &remote_path, &local_path, app)
        .await
}

/// Upload file from local to remote
#[command]
pub async fn sftp_upload(
    app: AppHandle,
    session_id: String,
    local_path: String,
    remote_path: String,
) -> Result<u64, String> {
    SFTP_MANAGER
        .upload(&session_id, &local_path, &remote_path, app)
        .await
}

/// Create directory
#[command]
pub async fn sftp_mkdir(session_id: String, path: String) -> Result<(), String> {
    SFTP_MANAGER.mkdir(&session_id, &path).await
}

/// Delete file
#[command]
pub async fn sftp_delete_file(session_id: String, path: String) -> Result<(), String> {
    SFTP_MANAGER.delete_file(&session_id, &path).await
}

/// Delete directory
#[command]
pub async fn sftp_delete_dir(session_id: String, path: String) -> Result<(), String> {
    SFTP_MANAGER.delete_dir(&session_id, &path).await
}

/// Rename file or directory
#[command]
pub async fn sftp_rename(
    session_id: String,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    SFTP_MANAGER
        .rename(&session_id, &old_path, &new_path)
        .await
}

// ============================================
// YModem Commands
// ============================================

/// Start YModem file transfer
#[command]
pub async fn ssh_start_ymodem_send(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    file_path: String,
) -> Result<(), String> {
    let senders = state.input_senders.read();
    if let Some(sender) = senders.get(&session_id) {
        sender
            .send(SshInput::StartYmodemSend { file_path })
            .map_err(|e| format!("Failed to start YModem: {}", e))?;
    } else {
        return Err("Session sender not found".to_string());
    }
    Ok(())
}

/// Cancel active YModem file transfer
#[command]
pub async fn ssh_cancel_ymodem(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let senders = state.input_senders.read();
    if let Some(sender) = senders.get(&session_id) {
        sender
            .send(SshInput::CancelYmodem)
            .map_err(|e| format!("Failed to cancel YModem: {}", e))?;
    } else {
        return Err("Session sender not found".to_string());
    }
    Ok(())
}

// ============================================
// Key Manager Commands
// ============================================

use crate::keys::{self, KeyInfo};

/// Generate a new SSH Key Pair
#[command]
pub async fn generate_key_pair(
    name: String,
    algorithm: String,
    passphrase: Option<String>,
) -> Result<KeyInfo, String> {
    keys::generate_key_pair(&name, &algorithm, passphrase)
}

/// List all saved SSH Key Pairs
#[command]
pub async fn list_keys() -> Result<Vec<KeyInfo>, String> {
    keys::list_keys()
}

/// Delete an SSH Key Pair
#[command]
pub async fn delete_key(name: String) -> Result<(), String> {
    keys::delete_key(&name)
}

// ============================================
// Master Password Commands
// ============================================

/// Check if master password lock is active
#[command]
pub async fn is_master_password_set() -> Result<bool, String> {
    session::is_master_password_set()
}

/// Enable, disable, or change the master password session lock
#[command]
pub async fn change_master_password(
    old_password: Option<String>,
    new_password: Option<String>,
) -> Result<(), String> {
    session::change_master_password(old_password.as_deref(), new_password.as_deref())
}

// ============================================
// Local Filesystem Commands
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// List contents of a local directory
#[command]
pub async fn local_list_dir(path: String) -> Result<Vec<LocalFileEntry>, String> {
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        return Err("Path does not exist".to_string());
    }

    let entries = std::fs::read_dir(&path_buf)
        .map_err(|e| format!("Failed to read directory: {}", e))?;

    let mut list = Vec::new();
    for entry in entries {
        if let Ok(entry) = entry {
            let metadata = entry.metadata().ok();
            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path().to_string_lossy().to_string();
            list.push(LocalFileEntry {
                name,
                path: entry_path,
                is_dir,
                size,
            });
        }
    }
    Ok(list)
}

/// Get local user's home directory path
#[command]
pub async fn local_get_home_dir() -> Result<String, String> {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "Could not find home directory".to_string())
}

/// Delete a local file or directory
#[command]
pub async fn local_delete(path: String) -> Result<(), String> {
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        return Err("Path does not exist".to_string());
    }

    if path_buf.is_dir() {
        std::fs::remove_dir_all(path_buf)
            .map_err(|e| format!("Failed to delete local directory: {}", e))?;
    } else {
        std::fs::remove_file(path_buf)
            .map_err(|e| format!("Failed to delete local file: {}", e))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub default_path: String,
    pub current_path: String,
    pub is_custom: bool,
}

/// Query storage paths and redirection status
#[command]
pub async fn get_storage_info() -> Result<StorageInfo, String> {
    let default_path = crate::get_default_config_dir();
    let current_path = crate::get_app_config_dir();
    let is_custom = default_path != current_path;

    Ok(StorageInfo {
        default_path: default_path.to_string_lossy().to_string(),
        current_path: current_path.to_string_lossy().to_string(),
        is_custom,
    })
}

fn copy_dir_all(src: impl AsRef<std::path::Path>, dst: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Set a new storage location and optionally migrate configuration files
#[command]
pub async fn set_storage_location(new_path: String, move_files: bool) -> Result<(), String> {
    let new_path_buf = std::path::PathBuf::from(&new_path);
    if !new_path_buf.is_absolute() {
        return Err("The storage path must be an absolute path".to_string());
    }

    let default_dir = crate::get_default_config_dir();
    let current_dir = crate::get_app_config_dir();

    // Ensure the new storage directory exists
    if !new_path_buf.exists() {
        std::fs::create_dir_all(&new_path_buf)
            .map_err(|e| format!("Failed to create new directory: {}", e))?;
    }

    if move_files && current_dir != new_path_buf {
        // Move files
        let files_to_copy = ["sessions.json", "settings.json", "commands.json", "ai_config.json"];
        for file in &files_to_copy {
            let src_file = current_dir.join(file);
            if src_file.exists() && src_file.is_file() {
                std::fs::copy(&src_file, new_path_buf.join(file))
                    .map_err(|e| format!("Failed to copy file {}: {}", file, e))?;
            }
        }

        let src_keys = current_dir.join("keys");
        if src_keys.exists() && src_keys.is_dir() {
            copy_dir_all(&src_keys, new_path_buf.join("keys"))
                .map_err(|e| format!("Failed to copy keys directory: {}", e))?;
        }
    }

    // Write pointer file in the default directory
    let pointer_file = default_dir.join("storage_path.txt");
    std::fs::create_dir_all(&default_dir)
        .map_err(|e| format!("Failed to create default config directory: {}", e))?;
    std::fs::write(&pointer_file, &new_path)
        .map_err(|e| format!("Failed to write storage pointer: {}", e))?;

    Ok(())
}

/// Reset storage location to default path
#[command]
pub async fn reset_storage_location() -> Result<(), String> {
    let default_dir = crate::get_default_config_dir();
    let pointer_file = default_dir.join("storage_path.txt");

    if pointer_file.exists() {
        std::fs::remove_file(pointer_file)
            .map_err(|e| format!("Failed to delete storage pointer: {}", e))?;
    }

    Ok(())
}

// ============================================
// SSH TUNNEL / PORT FORWARDING COMMANDS
// ============================================

use crate::tunnels::{TunnelConfig, TunnelStatus, TUNNEL_MANAGER};

/// Start an individual SSH tunnel
#[command]
pub async fn start_tunnel(
    tunnel: TunnelConfig,
    connection: ConnectRequest,
) -> Result<(), String> {
    TUNNEL_MANAGER.start_tunnel(tunnel, connection).await
}

/// Stop an individual SSH tunnel
#[command]
pub async fn stop_tunnel(tunnel_id: String) -> Result<(), String> {
    TUNNEL_MANAGER.stop_tunnel(&tunnel_id).await;
    Ok(())
}

/// Stop all tunnels across all sessions
#[command]
pub async fn stop_all_tunnels() -> Result<(), String> {
    TUNNEL_MANAGER.stop_all_tunnels().await;
    Ok(())
}

/// Get status of all active running tunnels
#[command]
pub async fn get_active_tunnels() -> Result<Vec<TunnelStatus>, String> {
    Ok(TUNNEL_MANAGER.get_statuses().await)
}

/// Start all configured tunnels for a given saved session
#[command]
pub async fn start_session_tunnels(
    session_id: String,
    password: Option<String>,
) -> Result<usize, String> {
    let sessions = session::load_sessions_secure(password.as_deref())?;
    let target_session = sessions
        .iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "Session not found".to_string())?;

    if target_session.tunnels.is_empty() {
        return Ok(0);
    }

    // Resolve password if needed
    let session_pwd = if target_session.use_global_credentials {
        session::get_password("__global__").ok().flatten()
    } else {
        session::get_password(&target_session.id).ok().flatten()
    };

    // Resolve private key passphrase if needed
    let key_passphrase = if target_session.auth_method == "publickey" {
        if let Some(ref key_name) = target_session.private_key_name {
            session::get_password(&format!("key:{}", key_name)).ok().flatten()
        } else {
            None
        }
    } else {
        None
    };

    let connect_req = ConnectRequest {
        host: target_session.host.clone(),
        port: target_session.port,
        username: target_session.username.clone().unwrap_or_default(),
        password: session_pwd,
        log_directory: target_session.log_directory.clone(),
        auth_method: target_session.auth_method.clone(),
        private_key_name: target_session.private_key_name.clone(),
        private_key_passphrase: key_passphrase,
    };

    let mut started_count = 0;
    for tunnel in &target_session.tunnels {
        let mut tunnel_cfg = tunnel.clone();
        tunnel_cfg.session_id = Some(session_id.clone());
        if let Err(e) = TUNNEL_MANAGER.start_tunnel(tunnel_cfg, connect_req.clone()).await {
            eprintln!("Failed to start tunnel '{}': {}", tunnel.name, e);
        } else {
            started_count += 1;
        }
    }

    Ok(started_count)
}

/// Stop all tunnels for a given saved session
#[command]
pub async fn stop_session_tunnels(session_id: String) -> Result<(), String> {
    TUNNEL_MANAGER.stop_session_tunnels(&session_id).await;
    Ok(())
}

/// Open a URL in the default web browser (e.g. for HTTP/HTTPS tunnels)
#[command]
pub async fn open_tunnel_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| format!("Failed to open browser URL {}: {}", url, e))
}




