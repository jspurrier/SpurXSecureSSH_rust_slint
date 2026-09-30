use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::sftp::SftpManager;
use crate::ssh;
use crate::terminal::TerminalBuffer;
use crate::tunnels::{TunnelConfig, TunnelManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: String,
    pub name: String,
    pub color_index: usize,
    pub collapsed: bool,
    pub tab_ids: Vec<String>,
}

impl TabGroup {
    pub fn new(name: String, color_index: usize) -> Self {
        Self {
            id: format!("group-{}", Uuid::new_v4()),
            name,
            color_index,
            collapsed: false,
            tab_ids: Vec::new(),
        }
    }
}

pub struct AppState {
    pub connections: RwLock<HashMap<String, Arc<ssh::SshConnection>>>,
    pub input_senders: RwLock<HashMap<String, tokio::sync::mpsc::UnboundedSender<ssh::SshInput>>>,
    pub terminal_buffers: RwLock<HashMap<String, Arc<parking_lot::Mutex<TerminalBuffer>>>>,
    pub tunnel_manager: Arc<TunnelManager>,
    pub sftp_manager: Arc<SftpManager>,
    pub tab_groups: RwLock<HashMap<String, TabGroup>>,
    pub current_terminal_rows: std::sync::atomic::AtomicUsize,
    pub current_terminal_cols: std::sync::atomic::AtomicUsize,
    pub current_terminal_font_size: std::sync::atomic::AtomicU32,
    pub current_terminal_font_family: RwLock<String>,
    pub session_credentials: RwLock<HashMap<String, ConnectRequest>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            input_senders: RwLock::new(HashMap::new()),
            terminal_buffers: RwLock::new(HashMap::new()),
            tunnel_manager: Arc::new(TunnelManager::new()),
            sftp_manager: Arc::new(SftpManager::new()),
            tab_groups: RwLock::new(HashMap::new()),
            current_terminal_rows: std::sync::atomic::AtomicUsize::new(24),
            current_terminal_cols: std::sync::atomic::AtomicUsize::new(80),
            current_terminal_font_size: std::sync::atomic::AtomicU32::new(14),
            current_terminal_font_family: RwLock::new("JetBrains Mono".to_string()),
            session_credentials: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectResponse {
    pub session_id: String,
    pub success: bool,
    pub error: Option<String>,
}

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
    #[serde(default)]
    pub tunnels: Vec<TunnelConfig>,
    #[serde(default)]
    pub auto_start_tunnels: bool,
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
            tunnels: Vec::new(),
            auto_start_tunnels: false,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_group_creation_and_membership() {
        let state = AppState::new();
        let mut group = TabGroup::new("Production".to_string(), 0);
        assert_eq!(group.name, "Production");
        assert_eq!(group.color_index, 0);
        assert!(!group.collapsed);

        group.tab_ids.push("tab-1".to_string());
        group.tab_ids.push("tab-2".to_string());
        assert_eq!(group.tab_ids.len(), 2);

        state.tab_groups.write().insert(group.id.clone(), group);
        assert_eq!(state.tab_groups.read().len(), 1);

        // Remove a tab
        {
            let mut groups = state.tab_groups.write();
            for g in groups.values_mut() {
                g.tab_ids.retain(|id| id != "tab-1");
            }
        }

        let groups = state.tab_groups.read();
        let g = groups.values().next().unwrap();
        assert_eq!(g.tab_ids, vec!["tab-2".to_string()]);
    }
}
