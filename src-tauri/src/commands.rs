//! Tauri command handlers for SSH operations and session management

use std::sync::Arc;
use tauri::{command, AppHandle, State, Emitter};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{AppState, ConnectRequest, ConnectResponse, SavedSession, QuickCommand};
use crate::ssh::{SshConnection, connect_ssh_blocking};
use crate::session;
use crate::command_storage;

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
    
    // Create input channel for user input (std sync for blocking ssh2)
    let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    
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
    tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            // Emit terminal data to frontend
            let _ = app_for_output.emit(&format!("terminal-data-{}", sid_for_output), data);
        }
    });
    
    // Spawn blocking SSH connection in a separate thread
    let sid_for_ssh = sid.clone();
    std::thread::spawn(move || {
        let mut input_rx = input_rx;
        
        match connect_ssh_blocking(&host, port, &username, &password, output_tx, &mut input_rx) {
            Ok(_) => {
                connection_for_task.set_connected(false);
                // Can't easily emit from std thread, connection is closed
            }
            Err(e) => {
                eprintln!("SSH error for session {}: {}", sid_for_ssh, e);
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
        sender.send(data)
            .map_err(|e| format!("Failed to send data: {}", e))?;
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
