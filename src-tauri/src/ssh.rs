//! SSH connection management module
//! Handles SSH connections using the russh library with PTY support

use async_trait::async_trait;
use parking_lot::Mutex;
use russh::{client, ChannelMsg};
use russh_keys::key;
use std::sync::Arc;
use tokio::sync::mpsc;
use tauri::Emitter;

/// SSH input events from frontend
#[derive(Debug, Clone)]
pub enum SshInput {
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    StartYmodemSend { file_path: String },
    CancelYmodem,
}

/// SSH connection handler using russh
pub struct SshConnection {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub connected: Arc<Mutex<bool>>,
}

impl SshConnection {
    pub fn new(session_id: String, host: String, port: u16, username: String) -> Self {
        Self {
            session_id,
            host,
            port,
            username,
            connected: Arc::new(Mutex::new(false)),
        }
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.lock()
    }

    pub fn set_connected(&self, value: bool) {
        *self.connected.lock() = value;
    }
}

pub struct ClientHandler {
    pub host: String,
    pub output_tx: mpsc::Sender<Vec<u8>>,
}

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let mut verified = false;
        if let Some(mut path) = dirs::home_dir() {
            path.push(".ssh");
            path.push("known_hosts");
            if path.exists() {
                if let Ok(matched) = russh_keys::check_known_hosts_path(
                    &self.host,
                    22,
                    server_public_key,
                    &path,
                ) {
                    verified = matched;
                }
            }
        }

        let key_algo = server_public_key.name();

        if verified {
            let msg = format!("\x1b[32mHost key ({}) verified.\x1b[0m\r\n", key_algo);
            let _ = self.output_tx.send(msg.into_bytes()).await;
        } else {
            let msg = format!(
                "\r\n\x1b[33mWARNING: Host '{}' ({}) not found in known_hosts (or mismatched). Proceeding...\x1b[0m\r\n",
                self.host, key_algo
            );
            let _ = self.output_tx.send(msg.into_bytes()).await;
        }

        Ok(true)
    }
}

pub fn get_russh_config() -> client::Config {
    let preferred = russh::Preferred {
        // Modern algorithms first, then legacy fallbacks for old Cisco/network gear
        kex: std::borrow::Cow::Owned(vec![
            russh::kex::CURVE25519,
            russh::kex::CURVE25519_PRE_RFC_8731,
            russh::kex::ECDH_SHA2_NISTP256,
            russh::kex::ECDH_SHA2_NISTP384,
            russh::kex::ECDH_SHA2_NISTP521,
            russh::kex::DH_G16_SHA512,
            russh::kex::DH_G14_SHA256,
            russh::kex::DH_G14_SHA1,  // Legacy: Cisco IOS, older gear
            russh::kex::DH_G1_SHA1,   // Very old legacy
        ]),
        // Include ECDSA (P-256, P-384, P-521), Ed25519, RSA, and legacy host key types for old SSH servers
        key: std::borrow::Cow::Owned(vec![
            russh_keys::key::ED25519,
            russh_keys::key::ECDSA_SHA2_NISTP256,
            russh_keys::key::ECDSA_SHA2_NISTP384,
            russh_keys::key::ECDSA_SHA2_NISTP521,
            russh_keys::key::RSA_SHA2_512,
            russh_keys::key::RSA_SHA2_256,
            russh_keys::key::SSH_RSA,    // Legacy RSA - required for very old Cisco
        ]),
        cipher: std::borrow::Cow::Owned(vec![
            russh::cipher::CHACHA20_POLY1305,
            russh::cipher::AES_256_GCM,
            russh::cipher::AES_256_CTR,
            russh::cipher::AES_192_CTR,
            russh::cipher::AES_128_CTR,
            russh::cipher::AES_256_CBC, // For older gear
            russh::cipher::AES_192_CBC,
            russh::cipher::AES_128_CBC,
        ]),
        mac: std::borrow::Cow::Owned(vec![
            russh::mac::HMAC_SHA512_ETM,
            russh::mac::HMAC_SHA256_ETM,
            russh::mac::HMAC_SHA512,
            russh::mac::HMAC_SHA256,
            russh::mac::HMAC_SHA1_ETM,
            russh::mac::HMAC_SHA1,
        ]),
        ..russh::Preferred::DEFAULT
    };

    client::Config {
        preferred,
        ..Default::default()
    }
}

/// Connect to SSH server and handle I/O asynchronously
pub async fn connect_ssh_async(
    session_id: &str,
    host: &str,
    port: u16,
    username: &str,
    password: Option<&str>,
    auth_method: &str,
    private_key_name: Option<&str>,
    private_key_passphrase: Option<&str>,
    output_tx: mpsc::Sender<Vec<u8>>,
    input_rx: &mut mpsc::UnboundedReceiver<SshInput>,
    debug_mode: bool,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Send connection status
    let _ = output_tx.send(
        format!("\r\n\x1b[36mConnecting to {}:{}...\x1b[0m\r\n", host, port).into_bytes(),
    ).await;

    let config = Arc::new(get_russh_config());

    let handler = ClientHandler {
        host: host.to_string(),
        output_tx: output_tx.clone(),
    };

    if debug_mode {
        let _ = output_tx.send(b"\x1b[36mPerforming SSH handshake...\x1b[0m\r\n".to_vec()).await;
    }

    let mut session = client::connect(config, (host, port), handler)
        .await
        .map_err(|e| format!("SSH connect failed: {}", e))?;

    if debug_mode {
        let _ = output_tx.send(b"\x1b[36mAuthenticating...\x1b[0m\r\n".to_vec()).await;
    }

    let auth_res = if auth_method == "publickey" {
        if let Some(key_name) = private_key_name {
            let keys_dir = crate::keys::get_keys_dir();
            let key_path = keys_dir.join(key_name);
            if !key_path.exists() {
                let _ = output_tx.send(format!("\x1b[31mPrivate key file not found: {:?}\x1b[0m\r\n", key_path).into_bytes()).await;
                return Err("Private key not found".to_string());
            }

            let passphrase_str = private_key_passphrase.filter(|s| !s.is_empty());
            
            if debug_mode {
                let _ = output_tx.send(b"\x1b[36mLoading private key...\x1b[0m\r\n".to_vec()).await;
            }

            match russh_keys::load_secret_key(&key_path, passphrase_str) {
                Ok(key_pair) => {
                    if debug_mode {
                        let _ = output_tx.send(b"\x1b[36mAuthenticating with public key...\x1b[0m\r\n".to_vec()).await;
                    }
                    session.authenticate_publickey(username, Arc::new(key_pair)).await
                        .map_err(|e| format!("Public key authentication failed: {}", e))?
                }
                Err(e) => {
                    let _ = output_tx.send(format!("\x1b[31mFailed to load private key: {}\x1b[0m\r\n", e).into_bytes()).await;
                    return Err(format!("Failed to load private key: {}", e));
                }
            }
        } else {
            return Err("Public key authentication requested but no key name provided".to_string());
        }
    } else {
        session.authenticate_password(username, password.unwrap_or("")).await
            .map_err(|e| format!("Password authentication failed: {}", e))?
    };

    if !auth_res {
        let _ = output_tx.send(b"\x1b[31mAuthentication failed!\x1b[0m\r\n".to_vec()).await;
        return Err("Authentication failed".to_string());
    }

    let _ = output_tx.send(b"\x1b[36mAuthenticated! Opening channel...\x1b[0m\r\n".to_vec()).await;

    let mut channel = session.channel_open_session().await
        .map_err(|e| format!("Failed to open channel: {}", e))?;

    channel.request_pty(false, "xterm", 80, 24, 0, 0, &[]).await
        .map_err(|e| format!("Failed to request PTY: {}", e))?;

    channel.request_shell(false).await
        .map_err(|e| format!("Failed to start shell: {}", e))?;

    let _ = output_tx.send(b"\x1b[36mShell ready!\x1b[0m\r\n\r\n".to_vec()).await;

    let mut ymodem_sender: Option<crate::ymodem::YmodemSender> = None;

    loop {
        tokio::select! {
            msg_res = channel.wait() => {
                match msg_res {
                    Some(ChannelMsg::Data { ref data }) => {
                        if let Some(ref mut sender) = ymodem_sender {
                            if let Some(response_packet) = sender.handle_data(data) {
                                let _ = channel.data(&*response_packet).await;
                            }
                            if sender.state == crate::ymodem::YmodemState::Finished {
                                let _ = output_tx.send(b"\r\n\x1b[32mYModem transfer completed successfully!\x1b[0m\r\n".to_vec()).await;
                                ymodem_sender = None;
                            } else if sender.state == crate::ymodem::YmodemState::Failed {
                                let err = sender.error_msg.as_deref().unwrap_or("Unknown error");
                                let _ = output_tx.send(format!("\r\n\x1b[31mYModem transfer failed: {}\x1b[0m\r\n", err).into_bytes()).await;
                                ymodem_sender = None;
                            }
                        } else {
                            let _ = output_tx.send(data.to_vec()).await;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        let _ = output_tx.send(data.to_vec()).await;
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        let _ = output_tx.send(b"\r\n\x1b[33mConnection closed.\x1b[0m\r\n".to_vec()).await;
                        break;
                    }
                    _ => {}
                }
            }
            input_res = input_rx.recv() => {
                match input_res {
                    Some(SshInput::Data(data)) => {
                        let _ = channel.data(&*data).await;
                    }
                    Some(SshInput::Resize { rows, cols }) => {
                        let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                    }
                    Some(SshInput::StartYmodemSend { file_path }) => {
                        if ymodem_sender.is_some() {
                            let _ = output_tx.send(b"\r\n\x1b[31mError: Another YModem transfer is already active!\x1b[0m\r\n".to_vec()).await;
                        } else {
                            let _ = output_tx.send(b"\r\n\x1b[36mInitializing YModem transfer...\x1b[0m\r\n".to_vec()).await;
                            match crate::ymodem::YmodemSender::new(&file_path, session_id.to_string(), app_handle.clone()) {
                                Ok(sender) => {
                                    ymodem_sender = Some(sender);
                                    let _ = output_tx.send(b"\x1b[36mYModem sender ready. Waiting for remote to start ('C')...\x1b[0m\r\n".to_vec()).await;
                                }
                                Err(e) => {
                                    let _ = output_tx.send(format!("\r\n\x1b[31mFailed to load file: {}\x1b[0m\r\n", e).into_bytes()).await;
                                    let _ = app_handle.emit(&format!("ymodem-error-{}", session_id), e);
                                }
                            }
                        }
                    }
                    Some(SshInput::CancelYmodem) => {
                        if let Some(ref mut sender) = ymodem_sender {
                            let _ = channel.data(&[0x18u8, 0x18, 0x18, 0x18, 0x18][..]).await;
                            sender.state = crate::ymodem::YmodemState::Failed;
                            let _ = output_tx.send(b"\r\n\x1b[31mYModem transfer cancelled by user.\x1b[0m\r\n".to_vec()).await;
                            let _ = app_handle.emit(&format!("ymodem-error-{}", session_id), "Cancelled by user".to_string());
                            ymodem_sender = None;
                        }
                    }
                    None => {
                        break;
                    }
                }
            }
        }
    }

    let _ = channel.close().await;

    Ok(())
}
