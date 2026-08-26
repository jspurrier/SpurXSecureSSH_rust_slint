//! SSH Tunnels and Port Forwarding module
//! Manages Local Port Forwarding (e.g. localhost:8080 -> remote_host:80) via russh direct-tcpip

use parking_lot::RwLock;
use russh::client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::ssh::ClientHandler;
use crate::ConnectRequest;

/// Configuration for an SSH Tunnel / Port Forward
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub id: String,
    pub name: String,
    pub session_id: Option<String>,
    #[serde(default = "default_tunnel_type")]
    pub tunnel_type: String, // "local" (Local -L), "remote" (Remote -R), "dynamic" (Dynamic -D)
    #[serde(default = "default_local_host")]
    pub local_host: String, // e.g. "127.0.0.1" or "0.0.0.0"
    pub local_port: u16, // e.g. 8080
    #[serde(default = "default_remote_host")]
    pub remote_host: String, // e.g. "10.0.0.1" or "192.168.1.50"
    pub remote_port: u16, // e.g. 80
    #[serde(default)]
    pub auto_start: bool,
}

fn default_tunnel_type() -> String {
    "local".to_string()
}

fn default_local_host() -> String {
    "127.0.0.1".to_string()
}

fn default_remote_host() -> String {
    "127.0.0.1".to_string()
}

/// Real-time status of an SSH Tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelStatus {
    pub id: String,
    pub running: bool,
    pub error: Option<String>,
    pub active_connections: u32,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub local_address: String,
    pub remote_target: String,
}

/// Running tunnel instance handles
struct RunningTunnel {
    config: TunnelConfig,
    cancel_tx: broadcast::Sender<()>,
    active_connections: Arc<AtomicU32>,
    bytes_rx: Arc<AtomicU64>,
    bytes_tx: Arc<AtomicU64>,
    error: Arc<parking_lot::Mutex<Option<String>>>,
}

/// Global manager for SSH Tunnels
pub struct TunnelManager {
    /// Active running tunnels by tunnel ID
    tunnels: RwLock<HashMap<String, Arc<RunningTunnel>>>,
    /// Shared SSH sessions keyed by session identifier or host:port:user
    ssh_sessions: RwLock<HashMap<String, Arc<client::Handle<ClientHandler>>>>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            tunnels: RwLock::new(HashMap::new()),
            ssh_sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Helper to get or establish an SSH session handle for a connection request
    async fn get_or_create_ssh_session(
        &self,
        key: &str,
        req: &ConnectRequest,
    ) -> Result<Arc<client::Handle<ClientHandler>>, String> {
        // Fast path: check under read lock
        {
            let sessions = self.ssh_sessions.read();
            if let Some(handle_arc) = sessions.get(key) {
                if !handle_arc.is_closed() {
                    return Ok(handle_arc.clone());
                }
            }
        }

        // Connect new SSH session without holding any lock
        let config = Arc::new(crate::ssh::get_russh_config());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = ClientHandler {
            host: req.host.clone(),
            output_tx: tx,
        };

        let connect_fut = client::connect(config, (req.host.as_str(), req.port), handler);
        let mut session = match tokio::time::timeout(std::time::Duration::from_secs(10), connect_fut).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                return Err(format!(
                    "SSH tunnel connect to {}:{} failed: {}",
                    req.host, req.port, e
                ))
            }
            Err(_) => {
                return Err(format!(
                    "SSH tunnel connect to {}:{} timed out after 10s",
                    req.host, req.port
                ))
            }
        };

        let auth_fut = async {
            if req.auth_method == "publickey" {
                if let Some(ref key_name) = req.private_key_name {
                    let key_path = match crate::keys::find_private_key_path(key_name) {
                        Some(p) => p,
                        None => return Err(format!("Private key '{}' not found", key_name)),
                    };

                    let passphrase_str = req
                        .private_key_passphrase
                        .as_deref()
                        .filter(|s| !s.is_empty());
                    let key_pair = russh_keys::load_secret_key(&key_path, passphrase_str)
                        .map_err(|e| format!("Failed to load private key for tunnel: {}", e))?;

                    session
                        .authenticate_publickey(&req.username, Arc::new(key_pair))
                        .await
                        .map_err(|e| format!("Tunnel publickey auth failed: {}", e))
                } else {
                    Err("Publickey auth selected but no key name provided".to_string())
                }
            } else {
                session
                    .authenticate_password(&req.username, req.password.as_deref().unwrap_or(""))
                    .await
                    .map_err(|e| format!("Tunnel password auth failed: {}", e))
            }
        };

        let auth_res = match tokio::time::timeout(std::time::Duration::from_secs(10), auth_fut).await {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("Tunnel SSH authentication timed out after 10s".to_string()),
        };

        if !auth_res {
            return Err("Tunnel SSH authentication failed: Invalid credentials".to_string());
        }

        let handle_arc = Arc::new(session);
        {
            let mut sessions = self.ssh_sessions.write();
            sessions.insert(key.to_string(), handle_arc.clone());
        }
        Ok(handle_arc)
    }

    /// Start a tunnel instance
    pub async fn start_tunnel(
        &self,
        config: TunnelConfig,
        req: ConnectRequest,
    ) -> Result<(), String> {
        // If already running, stop it first
        self.stop_tunnel(&config.id);

        let session_key = if let Some(ref sid) = config.session_id {
            format!("session:{}", sid)
        } else {
            format!("host:{}:{}:{}", req.host, req.port, req.username)
        };

        let session_arc = self.get_or_create_ssh_session(&session_key, &req).await?;

        // Bind local TCP listener
        let bind_addr = format!("{}:{}", config.local_host, config.local_port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| format!("Failed to bind local port {}: {}", bind_addr, e))?;

        let (cancel_tx, _) = broadcast::channel(1);
        let active_connections = Arc::new(AtomicU32::new(0));
        let bytes_rx = Arc::new(AtomicU64::new(0));
        let bytes_tx = Arc::new(AtomicU64::new(0));
        let error = Arc::new(parking_lot::Mutex::new(None));

        let running_tunnel = Arc::new(RunningTunnel {
            config: config.clone(),
            cancel_tx: cancel_tx.clone(),
            active_connections: active_connections.clone(),
            bytes_rx: bytes_rx.clone(),
            bytes_tx: bytes_tx.clone(),
            error: error.clone(),
        });

        {
            let mut tunnels = self.tunnels.write();
            tunnels.insert(config.id.clone(), running_tunnel);
        }

        // Spawn background listener task
        let tunnel_id = config.id.clone();
        let remote_host = config.remote_host.clone();
        let remote_port = config.remote_port;
        let mut cancel_rx = cancel_tx.subscribe();
        let error_for_task = error.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((mut local_stream, peer_addr)) => {
                                let session_handle = session_arc.clone();
                                let remote_h = remote_host.clone();
                                let active_conn = active_connections.clone();
                                let rx_counter = bytes_rx.clone();
                                let tx_counter = bytes_tx.clone();
                                let tid = tunnel_id.clone();

                                tokio::spawn(async move {
                                    active_conn.fetch_add(1, Ordering::SeqCst);

                                    let open_channel_fut = async {
                                        if session_handle.is_closed() {
                                            Err(russh::Error::SendError)
                                        } else {
                                            session_handle.channel_open_direct_tcpip(
                                                remote_h,
                                                remote_port as u32,
                                                peer_addr.ip().to_string(),
                                                peer_addr.port() as u32,
                                            ).await
                                        }
                                    };

                                    let open_channel_res = tokio::time::timeout(
                                        std::time::Duration::from_secs(10),
                                        open_channel_fut,
                                    ).await;

                                    match open_channel_res {
                                        Ok(Ok(channel)) => {
                                            let mut channel_stream = channel.into_stream();
                                            if let Ok((from_client, from_remote)) = tokio::io::copy_bidirectional(&mut local_stream, &mut channel_stream).await {
                                                tx_counter.fetch_add(from_client, Ordering::Relaxed);
                                                rx_counter.fetch_add(from_remote, Ordering::Relaxed);
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            eprintln!("Failed to open direct-tcpip channel for tunnel {}: {:?}", tid, e);
                                        }
                                        Err(_) => {
                                            eprintln!("Direct-tcpip channel open timed out for tunnel {}", tid);
                                        }
                                    }

                                    active_conn.fetch_sub(1, Ordering::SeqCst);
                                });
                            }
                            Err(e) => {
                                eprintln!("Tunnel accept error on {}: {:?}", bind_addr, e);
                                *error_for_task.lock() = Some(format!("Accept error: {}", e));
                                break;
                            }
                        }
                    }
                    _ = cancel_rx.recv() => {
                        // Received cancel signal
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop a running tunnel (synchronous and instant)
    pub fn stop_tunnel(&self, tunnel_id: &str) {
        let mut tunnels = self.tunnels.write();
        if let Some(tunnel) = tunnels.remove(tunnel_id) {
            let _ = tunnel.cancel_tx.send(());
        }
    }

    /// Stop all running tunnels for a specific session ID
    pub fn stop_session_tunnels(&self, session_id: &str) {
        let mut to_remove = Vec::new();
        {
            let tunnels = self.tunnels.read();
            for (id, t) in tunnels.iter() {
                if t.config.session_id.as_deref() == Some(session_id) {
                    to_remove.push(id.clone());
                }
            }
        }
        for id in to_remove {
            self.stop_tunnel(&id);
        }

        // Clean up cached SSH session for this session
        let session_key = format!("session:{}", session_id);
        let mut sessions = self.ssh_sessions.write();
        sessions.remove(&session_key);
    }

    /// Stop all tunnels across all sessions
    pub fn stop_all_tunnels(&self) {
        let mut tunnels = self.tunnels.write();
        for (_, t) in tunnels.drain() {
            let _ = t.cancel_tx.send(());
        }
        let mut sessions = self.ssh_sessions.write();
        sessions.clear();
    }

    /// Check if a tunnel is actively running
    pub fn is_running(&self, tunnel_id: &str) -> bool {
        let tunnels = self.tunnels.read();
        tunnels.contains_key(tunnel_id)
    }

    /// Get real-time status of all running tunnels (instant, non-blocking)
    pub fn get_statuses(&self) -> Vec<TunnelStatus> {
        let tunnels = self.tunnels.read();
        tunnels
            .iter()
            .map(|(id, t)| TunnelStatus {
                id: id.clone(),
                running: true,
                error: t.error.lock().clone(),
                active_connections: t.active_connections.load(Ordering::Relaxed),
                bytes_rx: t.bytes_rx.load(Ordering::Relaxed),
                bytes_tx: t.bytes_tx.load(Ordering::Relaxed),
                local_address: format!("{}:{}", t.config.local_host, t.config.local_port),
                remote_target: format!("{}:{}", t.config.remote_host, t.config.remote_port),
            })
            .collect()
    }
}

// Global Tunnel Manager singleton
lazy_static::lazy_static! {
    pub static ref TUNNEL_MANAGER: Arc<TunnelManager> = Arc::new(TunnelManager::new());
}

pub fn get_tunnels_path() -> Result<std::path::PathBuf, String> {
    let app_dir = crate::get_app_config_dir();
    if !app_dir.exists() {
        let _ = std::fs::create_dir_all(&app_dir);
    }
    Ok(app_dir.join("tunnels.json"))
}

pub fn load_tunnels() -> Vec<TunnelConfig> {
    if let Ok(path) = get_tunnels_path() {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                return serde_json::from_str(&content).unwrap_or_default();
            }
        }
    }
    Vec::new()
}

pub fn save_tunnel(config: TunnelConfig) -> Result<(), String> {
    let path = get_tunnels_path()?;
    let mut tunnels = load_tunnels();
    if let Some(existing) = tunnels.iter_mut().find(|t| t.id == config.id) {
        *existing = config;
    } else {
        tunnels.push(config);
    }
    let content = serde_json::to_string_pretty(&tunnels)
        .map_err(|e| format!("Failed to serialize tunnels: {}", e))?;
    std::fs::write(path, content).map_err(|e| format!("Failed to write tunnels file: {}", e))?;
    Ok(())
}

pub fn delete_tunnel(id: &str) -> Result<(), String> {
    let path = get_tunnels_path()?;
    let mut tunnels = load_tunnels();
    tunnels.retain(|t| t.id != id);
    let content = serde_json::to_string_pretty(&tunnels)
        .map_err(|e| format!("Failed to serialize tunnels: {}", e))?;
    std::fs::write(path, content).map_err(|e| format!("Failed to write tunnels file: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_config_serialization() {
        let config = TunnelConfig {
            id: "tun-123".to_string(),
            name: "Web Forward".to_string(),
            session_id: Some("sess-1".to_string()),
            tunnel_type: "local".to_string(),
            local_host: "127.0.0.1".to_string(),
            local_port: 8080,
            remote_host: "192.168.1.100".to_string(),
            remote_port: 80,
            auto_start: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: TunnelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "tun-123");
        assert_eq!(deserialized.local_port, 8080);
        assert_eq!(deserialized.remote_port, 80);
        assert!(deserialized.auto_start);
    }

    #[test]
    fn test_tunnel_manager_sync_status_operations() {
        let manager = TunnelManager::new();
        assert!(!manager.is_running("nonexistent"));
        let statuses = manager.get_statuses();
        assert!(statuses.is_empty());

        // Test stopping nonexistent tunnel does not panic
        manager.stop_tunnel("nonexistent");
        manager.stop_session_tunnels("session-nonexistent");
        manager.stop_all_tunnels();
    }
}

