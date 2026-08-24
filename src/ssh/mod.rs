use async_trait::async_trait;
use parking_lot::Mutex;
use russh::{client, ChannelMsg};
use russh_keys::key;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum SshInput {
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    StartYmodemSend { file_path: String },
    CancelYmodem,
}

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
    pub output_tx: mpsc::UnboundedSender<Vec<u8>>,
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
                if let Ok(matched) =
                    russh_keys::check_known_hosts_path(&self.host, 22, server_public_key, &path)
                {
                    verified = matched;
                }
            }
        }

        let key_algo = server_public_key.name();

        if verified {
            let msg = format!("\x1b[32mHost key ({}) verified.\x1b[0m\r\n", key_algo);
            let _ = self.output_tx.send(msg.into_bytes());
        } else {
            let msg = format!(
                "\r\n\x1b[33mWARNING: Host '{}' ({}) not found in known_hosts (or mismatched). Proceeding...\x1b[0m\r\n",
                self.host, key_algo
            );
            let _ = self.output_tx.send(msg.into_bytes());
        }

        Ok(true)
    }
}

pub fn get_russh_config() -> client::Config {
    let preferred = russh::Preferred {
        kex: std::borrow::Cow::Owned(vec![
            russh::kex::CURVE25519,
            russh::kex::CURVE25519_PRE_RFC_8731,
            russh::kex::ECDH_SHA2_NISTP256,
            russh::kex::ECDH_SHA2_NISTP384,
            russh::kex::ECDH_SHA2_NISTP521,
            russh::kex::DH_G16_SHA512,
            russh::kex::DH_G14_SHA256,
            russh::kex::DH_G14_SHA1,
            russh::kex::DH_G1_SHA1,
        ]),
        key: std::borrow::Cow::Owned(vec![
            russh_keys::key::ED25519,
            russh_keys::key::ECDSA_SHA2_NISTP256,
            russh_keys::key::ECDSA_SHA2_NISTP384,
            russh_keys::key::ECDSA_SHA2_NISTP521,
            russh_keys::key::RSA_SHA2_512,
            russh_keys::key::RSA_SHA2_256,
            russh_keys::key::SSH_RSA,
        ]),
        cipher: std::borrow::Cow::Owned(vec![
            russh::cipher::CHACHA20_POLY1305,
            russh::cipher::AES_256_GCM,
            russh::cipher::AES_256_CTR,
            russh::cipher::AES_192_CTR,
            russh::cipher::AES_128_CTR,
            russh::cipher::AES_256_CBC,
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
        window_size: 4 * 1024 * 1024,
        maximum_packet_size: 64 * 1024,
        inactivity_timeout: None,
        keepalive_interval: Some(std::time::Duration::from_secs(15)),
        keepalive_max: 0,
        ..Default::default()
    }
}

struct SessionLogger {
    file: Option<tokio::fs::File>,
    with_timestamps: bool,
    line_buf: Vec<u8>,
}

impl SessionLogger {
    async fn new(session_name: &str, session_log_dir: Option<&str>) -> Self {
        let settings = crate::settings::load_settings();
        let should_log =
            settings.logging_enabled || session_log_dir.map(|d| !d.is_empty()).unwrap_or(false);
        if !should_log {
            return Self {
                file: None,
                with_timestamps: false,
                line_buf: Vec::new(),
            };
        }

        let dir_path = if let Some(dir) = session_log_dir.filter(|d| !d.is_empty()) {
            std::path::PathBuf::from(dir)
        } else if let Some(ref dir) = settings.log_directory.filter(|d| !d.is_empty()) {
            std::path::PathBuf::from(dir)
        } else {
            crate::settings::get_default_log_dir()
        };

        let _ = tokio::fs::create_dir_all(&dir_path).await;

        let now = chrono::Local::now();
        let date_str = now.format("%Y-%m-%d").to_string();
        let time_str = now.format("%H-%M-%S").to_string();
        let sanitized_name =
            session_name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");

        let mut filename = settings.log_format;
        if filename.is_empty() {
            filename = "{session}_{date}_{time}.log".to_string();
        }
        let filename = filename
            .replace("{session}", &sanitized_name)
            .replace("{host}", &sanitized_name)
            .replace("{date}", &date_str)
            .replace("{time}", &time_str);

        let file_path = dir_path.join(filename);
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await
            .ok();

        if let Some(ref mut f) = file {
            use tokio::io::AsyncWriteExt;
            let header = format!(
                "=== Session Log Started: {} (Target: {}) ===\r\n",
                now.format("%Y-%m-%d %H:%M:%S"),
                session_name
            );
            let _ = f.write_all(header.as_bytes()).await;
        }

        Self {
            file,
            with_timestamps: settings.log_timestamps,
            line_buf: Vec::new(),
        }
    }

    async fn write_bytes(&mut self, bytes: &[u8]) {
        if let Some(ref mut file) = self.file {
            use tokio::io::AsyncWriteExt;
            let clean = strip_ansi_escapes::strip(bytes);
            if self.with_timestamps {
                let mut out_buf = Vec::with_capacity(clean.len() + 64);
                for &b in &clean {
                    if self.line_buf.is_empty() {
                        let ts = chrono::Local::now()
                            .format("[%Y-%m-%d %H:%M:%S] ")
                            .to_string();
                        out_buf.extend_from_slice(ts.as_bytes());
                    }
                    self.line_buf.push(b);
                    out_buf.push(b);
                    if b == b'\n' {
                        self.line_buf.clear();
                    }
                }
                let _ = file.write_all(&out_buf).await;
            } else {
                let _ = file.write_all(&clean).await;
            }
        }
    }

    async fn close(&mut self) {
        if let Some(ref mut file) = self.file {
            use tokio::io::AsyncWriteExt;
            let now = chrono::Local::now();
            let footer = format!(
                "\r\n=== Session Log Ended: {} ===\r\n",
                now.format("%Y-%m-%d %H:%M:%S")
            );
            let _ = file.write_all(footer.as_bytes()).await;
            let _ = file.flush().await;
        }
    }
}

pub async fn connect_ssh_async(
    session_id: &str,
    host: &str,
    port: u16,
    username: &str,
    password: Option<&str>,
    auth_method: &str,
    private_key_name: Option<&str>,
    private_key_passphrase: Option<&str>,
    log_directory: Option<String>,
    initial_cols: u16,
    initial_rows: u16,
    output_tx: mpsc::UnboundedSender<Vec<u8>>,
    input_rx: &mut mpsc::UnboundedReceiver<SshInput>,
    debug_mode: bool,
    ymodem_tx: Option<mpsc::UnboundedSender<crate::ymodem::YmodemProgress>>,
) -> Result<(), String> {
    let _ = output_tx.send(format!("\r\nConnecting to {}:{}...\r\n", host, port).into_bytes());

    let config = Arc::new(get_russh_config());

    if debug_mode {
        let _ = output_tx.send(b"Performing SSH handshake...\r\n".to_vec());
    }

    let mut session_opt = None;
    let mut last_err = String::new();

    for attempt in 0..3 {
        let handler = ClientHandler {
            host: host.to_string(),
            output_tx: output_tx.clone(),
        };

        let socket = match tokio::net::TcpStream::connect((host, port)).await {
            Ok(s) => s,
            Err(e) => {
                last_err = format!("Connection to {}:{} failed: {}", host, port, e);
                if attempt < 2 {
                    let delay_ms = 400 * (attempt + 1) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                let err = format!("\r\nConnection failed to {}:{}: {}\r\n", host, port, e);
                let _ = output_tx.send(err.into_bytes());
                return Err(last_err);
            }
        };
        let _ = socket.set_nodelay(true);

        match client::connect_stream(config.clone(), socket, handler).await {
            Ok(s) => {
                session_opt = Some(s);
                break;
            }
            Err(e) => {
                last_err = format!("SSH handshake failed: {}", e);
                if attempt < 2 {
                    let delay_ms = 400 * (attempt + 1) as u64;
                    if debug_mode {
                        let _ = output_tx.send(
                            format!("Handshake interrupted; retrying in {}ms...\r\n", delay_ms)
                                .into_bytes(),
                        );
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                let err = format!("\r\nSSH handshake failed: {}\r\n", e);
                let _ = output_tx.send(err.into_bytes());
                return Err(last_err);
            }
        }
    }

    let mut session = session_opt.ok_or_else(|| last_err)?;

    if debug_mode {
        let _ = output_tx.send(b"Authenticating...\r\n".to_vec());
    }

    let auth_res = if auth_method == "publickey" {
        if let Some(key_name) = private_key_name {
            let key_path = match crate::keys::find_private_key_path(key_name) {
                Some(p) => p,
                None => {
                    let _ = output_tx.send(
                        format!("\r\nPrivate key file not found: {:?}\r\n", key_name).into_bytes(),
                    );
                    return Err("Private key not found".to_string());
                }
            };

            let passphrase_str = private_key_passphrase.filter(|s| !s.is_empty());
            if debug_mode {
                let _ = output_tx.send(b"Loading private key...\r\n".to_vec());
            }

            match russh_keys::load_secret_key(&key_path, passphrase_str) {
                Ok(key_pair) => {
                    if debug_mode {
                        let _ = output_tx.send(b"Authenticating with public key...\r\n".to_vec());
                    }
                    match session
                        .authenticate_publickey(username, Arc::new(key_pair))
                        .await
                    {
                        Ok(res) => res,
                        Err(e) => {
                            let _ = output_tx
                                .send(format!("\r\nPublic key auth error: {}\r\n", e).into_bytes());
                            return Err(format!("Public key auth error: {}", e));
                        }
                    }
                }
                Err(e) => {
                    let _ = output_tx
                        .send(format!("\r\nFailed to load private key: {}\r\n", e).into_bytes());
                    return Err(format!("Failed to load private key: {}", e));
                }
            }
        } else {
            let _ = output_tx
                .send(b"\r\nPublic key authentication requested but no key selected.\r\n".to_vec());
            return Err("Public key authentication requested but no key name provided".to_string());
        }
    } else {
        match session
            .authenticate_password(username, password.unwrap_or(""))
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let _ = output_tx
                    .send(format!("\r\nPassword authentication error: {}\r\n", e).into_bytes());
                return Err(format!("Password authentication failed: {}", e));
            }
        }
    };

    if !auth_res {
        let _ = output_tx.send(
            format!(
                "\r\nAuthentication failed for user '{}' on {}:{}!\r\n",
                username, host, port
            )
            .into_bytes(),
        );
        return Err("Authentication failed".to_string());
    }

    let _ = output_tx.send(b"Authenticated! Opening channel...\r\n".to_vec());

    let mut channel = match session.channel_open_session().await {
        Ok(c) => c,
        Err(e) => {
            let _ = output_tx.send(format!("\r\nFailed to open channel: {}\r\n", e).into_bytes());
            return Err(format!("Failed to open channel: {}", e));
        }
    };

    let pty_cols = initial_cols.max(20) as u32;
    let pty_rows = initial_rows.max(8) as u32;
    if let Err(e) = channel
        .request_pty(true, "xterm-256color", pty_cols, pty_rows, 0, 0, &[])
        .await
    {
        let _ = output_tx.send(format!("\r\nFailed to request PTY: {}\r\n", e).into_bytes());
        return Err(format!("Failed to request PTY: {}", e));
    }

    if let Err(e) = channel.request_shell(true).await {
        let _ = output_tx.send(format!("\r\nFailed to start shell: {}\r\n", e).into_bytes());
        return Err(format!("Failed to start shell: {}", e));
    }

    let _ = output_tx.send(b"Shell ready!\r\n\r\n".to_vec());

    let mut logger = SessionLogger::new(host, log_directory.as_deref()).await;
    let mut ymodem_sender: Option<crate::ymodem::YmodemSender> = None;

    loop {
        tokio::select! {
            msg_res = channel.wait() => {
                match msg_res {
                    Some(ChannelMsg::Data { ref data }) => {
                        logger.write_bytes(data).await;
                        if let Some(ref mut sender) = ymodem_sender {
                            if let Some(response_packet) = sender.handle_data(data) {
                                let _ = channel.data(&*response_packet).await;
                            }
                            if sender.state == crate::ymodem::YmodemState::Finished {
                                let _ = output_tx.send(b"\r\n\x1b[32mYModem transfer completed successfully!\x1b[0m\r\n".to_vec());
                                ymodem_sender = None;
                            } else if sender.state == crate::ymodem::YmodemState::Failed {
                                let err = sender.error_msg.as_deref().unwrap_or("Unknown error");
                                let _ = output_tx.send(format!("\r\n\x1b[31mYModem transfer failed: {}\x1b[0m\r\n", err).into_bytes());
                                ymodem_sender = None;
                            }
                        } else {
                            let _ = output_tx.send(data.to_vec());
                        }
                    }
                    Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        logger.write_bytes(data).await;
                        let _ = output_tx.send(data.to_vec());
                    }
                    Some(ChannelMsg::Close) | None => {
                        let _ = output_tx.send(b"\r\n\x1b[33mConnection closed.\x1b[0m\r\n".to_vec());
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
                            let _ = output_tx.send(b"\r\n\x1b[31mError: Another YModem transfer is already active!\x1b[0m\r\n".to_vec());
                        } else {
                            let _ = output_tx.send(b"\r\n\x1b[36mInitializing YModem transfer...\x1b[0m\r\n".to_vec());
                            match crate::ymodem::YmodemSender::new(&file_path, session_id.to_string(), ymodem_tx.clone()) {
                                Ok(sender) => {
                                    ymodem_sender = Some(sender);
                                    let _ = output_tx.send(b"\x1b[36mYModem sender ready. Waiting for remote to start ('C')...\x1b[0m\r\n".to_vec());
                                }
                                Err(e) => {
                                    let _ = output_tx.send(format!("\r\n\x1b[31mFailed to load file: {}\x1b[0m\r\n", e).into_bytes());
                                }
                            }
                        }
                    }
                    Some(SshInput::CancelYmodem) => {
                        if ymodem_sender.is_some() {
                            ymodem_sender = None;
                            let _ = output_tx.send(b"\r\n\x1b[33mYModem transfer cancelled.\x1b[0m\r\n".to_vec());
                        }
                    }
                    None => break,
                }
            }
        }
    }

    logger.close().await;
    Ok(())
}
