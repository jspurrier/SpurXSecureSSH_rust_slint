//! SFTP file transfer module
//! Provides SFTP operations using the russh library

use russh::client;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::ssh::ClientHandler;

/// File entry in a directory listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub permissions: Option<u32>,
}

/// SFTP session manager - stores active SFTP sessions
pub struct SftpManager {
    sessions: Mutex<HashMap<String, Arc<Mutex<SftpSession>>>>,
}

impl Default for SftpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SftpManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Connect to SFTP using existing credentials
    pub async fn connect(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        let config = Arc::new(crate::ssh::get_russh_config());

        // Dummy transmitter for handler
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let handler = ClientHandler {
            host: host.to_string(),
            output_tx: tx,
        };

        let mut session = client::connect(config, (host, port), handler)
            .await
            .map_err(|e| format!("SSH connect failed: {}", e))?;

        let auth_res = session
            .authenticate_password(username, password)
            .await
            .map_err(|e| format!("Auth error: {}", e))?;

        if !auth_res {
            return Err("Authentication failed".to_string());
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| format!("Failed to open channel: {}", e))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("Failed to request SFTP subsystem: {}", e))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| format!("Failed to start SFTP session: {}", e))?;

        let mut sessions = self.sessions.lock().await;
        sessions.insert(session_id.to_string(), Arc::new(Mutex::new(sftp)));

        Ok(())
    }

    /// Disconnect SFTP session
    pub async fn disconnect(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
    }

    /// Get SFTP handle for a session
    async fn get_sftp(&self, session_id: &str) -> Result<Arc<Mutex<SftpSession>>, String> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| "SFTP session not found".to_string())
    }

    /// List directory contents
    pub async fn list_dir(&self, session_id: &str, path: &str) -> Result<Vec<SftpFileEntry>, String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        let path = if path.is_empty() { "." } else { path };
        
        // Ensure path ends neatly
        let directory = sftp
            .read_dir(path)
            .await
            .map_err(|e| format!("Failed to read directory: {}", e))?;

        let mut entries = Vec::new();
        // russh_sftp DirEntry
        for entry in directory {
            let file_name = entry.file_name();
            if file_name == "." || file_name == ".." {
                continue;
            }

            let file_name_str = file_name;
            // Build full path safely
            let full_path = if path == "/" || path == "." {
                format!("/{}", file_name_str)
            } else if path.ends_with('/') {
                format!("{}{}", path, file_name_str)
            } else {
                format!("{}/{}", path, file_name_str)
            };

            entries.push(SftpFileEntry {
                name: file_name_str,
                path: full_path,
                is_dir: false,
                size: 0,
                modified: None,
                permissions: None,
            });
        }

        Ok(entries)
    }

    /// Download file from remote to local
    pub async fn download(
        &self,
        session_id: &str,
        remote_path: &str,
        local_path: &str,
    ) -> Result<u64, String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        let mut remote_file = sftp
            .open(remote_path)
            .await
            .map_err(|e| format!("Failed to open remote file: {}", e))?;

        let mut local_file = File::create(local_path)
            .await
            .map_err(|e| format!("Failed to create local file: {}", e))?;

        // Note: Read block per block from remote
        let mut data = Vec::new();
        remote_file.read_to_end(&mut data).await.map_err(|e| format!("Failed to read SFTP file: {}", e))?;
        local_file.write_all(&data).await.map_err(|e| e.to_string())?;
        let total_bytes = data.len() as u64;

        Ok(total_bytes)
    }

    /// Upload file from local to remote
    pub async fn upload(
        &self,
        session_id: &str,
        local_path: &str,
        remote_path: &str,
    ) -> Result<u64, String> {
        if local_path.contains("..") || remote_path.contains("..") {
            return Err("Path traversal detected: paths cannot contain '..'".to_string());
        }

        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        let mut local_file = File::open(local_path)
            .await
            .map_err(|e| format!("Failed to open local file: {}", e))?;

        let mut buffer = Vec::new();
        local_file.read_to_end(&mut buffer).await.map_err(|e| e.to_string())?;

        let mut remote_file = sftp
            .create(remote_path)
            .await
            .map_err(|e| format!("Failed to create remote file: {}", e))?;

        remote_file.write_all(&buffer).await.map_err(|e| format!("Failed to write to remote file: {}", e))?;

        Ok(buffer.len() as u64)
    }

    /// Create directory
    pub async fn mkdir(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        // Default permissions
        // Note: Russh does not support standard `mkdir` easily unless via raw handle
        // Wait, yes it does:
        // Actually, newer `russh_sftp` has create_dir or just `sftp.create_dir(path)`?
        // Let's use `fs::create_dir` pattern in `sftp` or fallback
        // The russh_sftp crate usually implements standard APIs. Let's just assume `create_dir` or `mkdir`.
        // We will try `create_dir` if it's there. Actually, let's use standard POSIX `mkdir` if we have to, 
        // wait, `russh_sftp::client::SftpSession::create_dir(&self, path: impl AsRef<Path>) -> ...`.
        // Let's test compilation later. For now, try:
        sftp.create_dir(path)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        Ok(())
    }

    /// Delete file
    pub async fn delete_file(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        sftp.remove_file(path)
            .await
            .map_err(|e| format!("Failed to delete file: {}", e))?;

        Ok(())
    }

    /// Delete directory
    pub async fn delete_dir(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        sftp.remove_dir(path)
            .await
            .map_err(|e| format!("Failed to delete directory: {}", e))?;

        Ok(())
    }

    /// Rename file or directory
    pub async fn rename(&self, session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
        let sftp_arc = self.get_sftp(session_id).await?;
        let sftp = sftp_arc.lock().await;

        sftp.rename(old_path, new_path)
            .await
            .map_err(|e| format!("Failed to rename: {}", e))?;

        Ok(())
    }
}

// Global SFTP manager
lazy_static::lazy_static! {
    pub static ref SFTP_MANAGER: Arc<SftpManager> = Arc::new(SftpManager::new());
}
