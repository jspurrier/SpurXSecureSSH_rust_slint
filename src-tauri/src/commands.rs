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
            &host,
            port,
            &username,
            &password,
            output_tx,
            &mut input_rx,
            debug_mode,
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
pub async fn get_saved_sessions() -> Result<Vec<SavedSession>, String> {
    session::load_sessions()
}

/// Save a new session
#[command]
pub async fn save_session(session: SavedSession) -> Result<(), String> {
    session::save_session(session)
}

/// Delete a session
#[command]
pub async fn delete_session(session_id: String) -> Result<(), String> {
    session::delete_session(&session_id)
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
    password: String,
) -> Result<(), String> {
    SFTP_MANAGER
        .connect(&session_id, &host, port, &username, &password)
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
    session_id: String,
    remote_path: String,
    local_path: String,
) -> Result<u64, String> {
    SFTP_MANAGER
        .download(&session_id, &remote_path, &local_path)
        .await
}

/// Upload file from local to remote
#[command]
pub async fn sftp_upload(
    session_id: String,
    local_path: String,
    remote_path: String,
) -> Result<u64, String> {
    SFTP_MANAGER
        .upload(&session_id, &local_path, &remote_path)
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
