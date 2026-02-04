//! SFTP file transfer module
//! Provides SFTP operations using the ssh2 library

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ssh2::{Session, Sftp};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;

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
    sessions: HashMap<String, SftpSessionData>,
}

struct SftpSessionData {
    session: Session,
    _tcp: TcpStream, // Keep TCP stream alive
}

impl SftpManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Connect to SFTP using existing credentials
    pub fn connect(
        &mut self,
        session_id: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        // Create new TCP connection for SFTP
        let addr = format!("{}:{}", host, port);
        let tcp = TcpStream::connect(&addr).map_err(|e| format!("TCP connection failed: {}", e))?;

        // Create SSH session
        let mut sess =
            Session::new().map_err(|e| format!("Failed to create SSH session: {}", e))?;

        sess.set_tcp_stream(tcp.try_clone().map_err(|e| e.to_string())?);

        // Perform SSH handshake
        sess.handshake()
            .map_err(|e| format!("SSH handshake failed: {}", e))?;

        // Authenticate
        sess.userauth_password(username, password)
            .map_err(|e| format!("Authentication failed: {}", e))?;

        if !sess.authenticated() {
            return Err("Authentication failed".to_string());
        }

        // Store session
        self.sessions.insert(
            session_id.to_string(),
            SftpSessionData {
                session: sess,
                _tcp: tcp,
            },
        );

        Ok(())
    }

    /// Disconnect SFTP session
    pub fn disconnect(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// Get SFTP handle for a session
    fn get_sftp(&self, session_id: &str) -> Result<Sftp, String> {
        let data = self
            .sessions
            .get(session_id)
            .ok_or_else(|| "SFTP session not found".to_string())?;

        data.session
            .sftp()
            .map_err(|e| format!("Failed to create SFTP channel: {}", e))
    }

    /// List directory contents
    pub fn list_dir(&self, session_id: &str, path: &str) -> Result<Vec<SftpFileEntry>, String> {
        let sftp = self.get_sftp(session_id)?;

        let path = if path.is_empty() { "/" } else { path };
        let _dir = sftp
            .opendir(Path::new(path))
            .map_err(|e| format!("Failed to open directory: {}", e))?;

        let entries: Vec<SftpFileEntry> = sftp
            .readdir(Path::new(path))
            .map_err(|e| format!("Failed to read directory: {}", e))?
            .into_iter()
            .filter_map(|(path_buf, stat)| {
                let name = path_buf.file_name()?.to_string_lossy().to_string();

                // Skip . and ..
                if name == "." || name == ".." {
                    return None;
                }

                Some(SftpFileEntry {
                    name: name.clone(),
                    path: path_buf.to_string_lossy().to_string(),
                    is_dir: stat.is_dir(),
                    size: stat.size.unwrap_or(0),
                    modified: stat.mtime,
                    permissions: stat.perm,
                })
            })
            .collect();

        Ok(entries)
    }

    /// Download file from remote to local
    pub fn download(
        &self,
        session_id: &str,
        remote_path: &str,
        local_path: &str,
    ) -> Result<u64, String> {
        let sftp = self.get_sftp(session_id)?;

        let mut remote_file = sftp
            .open(Path::new(remote_path))
            .map_err(|e| format!("Failed to open remote file: {}", e))?;

        let mut local_file =
            File::create(local_path).map_err(|e| format!("Failed to create local file: {}", e))?;

        let mut buffer = [0u8; 32768];
        let mut total_bytes = 0u64;

        loop {
            let n = remote_file
                .read(&mut buffer)
                .map_err(|e| format!("Failed to read remote file: {}", e))?;

            if n == 0 {
                break;
            }

            local_file
                .write_all(&buffer[..n])
                .map_err(|e| format!("Failed to write local file: {}", e))?;

            total_bytes += n as u64;
        }

        Ok(total_bytes)
    }

    /// Upload file from local to remote
    pub fn upload(
        &self,
        session_id: &str,
        local_path: &str,
        remote_path: &str,
    ) -> Result<u64, String> {
        // Simple traversal check for local path - reject if it contains ".."
        if local_path.contains("..") || remote_path.contains("..") {
            return Err("Path traversal detected: paths cannot contain '..'".to_string());
        }

        let sftp = self.get_sftp(session_id)?;

        let mut local_file =
            File::open(local_path).map_err(|e| format!("Failed to open local file: {}", e))?;

        let mut remote_file = sftp
            .create(Path::new(remote_path))
            .map_err(|e| format!("Failed to create remote file: {}", e))?;

        let mut buffer = [0u8; 32768];
        let mut total_bytes = 0u64;

        loop {
            let n = local_file
                .read(&mut buffer)
                .map_err(|e| format!("Failed to read local file: {}", e))?;

            if n == 0 {
                break;
            }

            remote_file
                .write_all(&buffer[..n])
                .map_err(|e| format!("Failed to write remote file: {}", e))?;

            total_bytes += n as u64;
        }

        Ok(total_bytes)
    }

    /// Create directory
    pub fn mkdir(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp = self.get_sftp(session_id)?;

        sftp.mkdir(Path::new(path), 0o755)
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        Ok(())
    }

    /// Delete file
    pub fn delete_file(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp = self.get_sftp(session_id)?;

        sftp.unlink(Path::new(path))
            .map_err(|e| format!("Failed to delete file: {}", e))?;

        Ok(())
    }

    /// Delete directory
    pub fn delete_dir(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp = self.get_sftp(session_id)?;

        sftp.rmdir(Path::new(path))
            .map_err(|e| format!("Failed to delete directory: {}", e))?;

        Ok(())
    }

    /// Rename file or directory
    pub fn rename(&self, session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
        let sftp = self.get_sftp(session_id)?;

        sftp.rename(Path::new(old_path), Path::new(new_path), None)
            .map_err(|e| format!("Failed to rename: {}", e))?;

        Ok(())
    }

    /// Get file stat
    pub fn stat(&self, session_id: &str, path: &str) -> Result<SftpFileEntry, String> {
        let sftp = self.get_sftp(session_id)?;

        let stat = sftp
            .stat(Path::new(path))
            .map_err(|e| format!("Failed to stat file: {}", e))?;

        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok(SftpFileEntry {
            name,
            path: path.to_string(),
            is_dir: stat.is_dir(),
            size: stat.size.unwrap_or(0),
            modified: stat.mtime,
            permissions: stat.perm,
        })
    }
}

// Global SFTP manager
lazy_static::lazy_static! {
    pub static ref SFTP_MANAGER: Arc<Mutex<SftpManager>> = Arc::new(Mutex::new(SftpManager::new()));
}
