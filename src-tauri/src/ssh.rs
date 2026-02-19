//! SSH connection management module
//! Handles SSH connections using the ssh2 library with PTY support

use parking_lot::Mutex;
use ssh2::{PtyModeOpcode, PtyModes, Session};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use tokio::sync::mpsc;

/// SSH input events from frontend
#[derive(Debug, Clone)]
pub enum SshInput {
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
}

/// SSH connection handler using ssh2 (libssh2)
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

/// Connect to SSH server and handle I/O
pub fn connect_ssh_blocking(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    output_tx: mpsc::Sender<Vec<u8>>,
    input_rx: &mut std::sync::mpsc::Receiver<SshInput>,
    debug_mode: bool,
) -> Result<(), String> {
    // Send connection status
    let _ =
        futures::executor::block_on(output_tx.send(
            format!("\r\n\x1b[36mConnecting to {}:{}...\x1b[0m\r\n", host, port).into_bytes(),
        ));

    // Create TCP connection
    let addr = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("TCP connection failed: {}", e))?;

    // Set initial timeout for handshake/auth (10 seconds)
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;

    let tcp_config_clone = tcp
        .try_clone()
        .map_err(|e| format!("Failed to clone TCP stream: {}", e))?;

    // Create SSH session
    let mut sess = Session::new().map_err(|e| format!("Failed to create SSH session: {}", e))?;

    // Enable trace logging for debugging (prints to stderr)
    sess.set_compress(true);
    // Note: ssh2-rs trace output goes to stderr by default
    // We can try to enable it if the feature is available or just rely on our own logs
    // sess.trace(ssh2::TraceFlags::all());

    sess.set_tcp_stream(tcp);

    // Perform SSH handshake
    if debug_mode {
        let _ = futures::executor::block_on(
            output_tx.send(b"\x1b[36mPerforming SSH handshake...\x1b[0m\r\n".to_vec()),
        );
    }

    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;

    // Host Key Verification
    {
        let mut known_hosts = sess
            .known_hosts()
            .map_err(|e| format!("Failed to init known hosts: {}", e))?;

        // Try to load user's known_hosts file
        let known_hosts_path = dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"));
        if let Some(ref path) = known_hosts_path {
            if path.exists() {
                let _ = known_hosts.read_file(path, ssh2::KnownHostFileKind::OpenSSH);
            }
        }

        let (key, _key_type) = sess.host_key().ok_or("Failed to get host key")?;

        match known_hosts.check(host, key) {
            ssh2::CheckResult::Match => {
                let _ = futures::executor::block_on(
                    output_tx.send(b"\x1b[32mHost key verified.\x1b[0m\r\n".to_vec()),
                );
            }
            ssh2::CheckResult::NotFound => {
                let fingerprint = sess
                    .host_key_hash(ssh2::HashType::Sha1)
                    .map(|h| h.iter().map(|b| format!("{:02x}", b)).collect::<String>());

                let msg = format!(
                    "\r\n\x1b[33mWARNING: Host '{}' not found in known_hosts.\x1b[0m\r\n",
                    host
                );
                let _ = futures::executor::block_on(output_tx.send(msg.into_bytes()));

                if let Some(fp) = fingerprint {
                    let msg = format!("\x1b[33mSHA1 Fingerprint: {}\x1b[0m\r\n", fp);
                    let _ = futures::executor::block_on(output_tx.send(msg.into_bytes()));
                }
            }
            ssh2::CheckResult::Mismatch => {
                let _ = futures::executor::block_on(
                    output_tx.send(
                        b"\r\n\x1b[41;37m WARNING: POSSIBLE DNS SPOOFING DETECTED! \x1b[0m\r\n"
                            .to_vec(),
                    ),
                );
                let _ = futures::executor::block_on(
                    output_tx.send(b"\x1b[31mThe host key for this server does not match the key in known_hosts.\x1b[0m\r\n".to_vec())
                );
                return Err(
                    "Host key verification failed: Mismatch (Possible Man-In-The-Middle attack)"
                        .to_string(),
                );
            }
            ssh2::CheckResult::Failure => {
                return Err("Host key verification failed: Internal error".to_string());
            }
        }
    }

    if debug_mode {
        let _ = futures::executor::block_on(
            output_tx.send(b"\x1b[36mAuthenticating...\x1b[0m\r\n".to_vec()),
        );
    }

    // Try Password Authentication first
    let mut auth_success = false;

    // Check if we can use password auth
    let auth_methods = sess.auth_methods(username).unwrap_or_default();
    if debug_mode {
        let _ = futures::executor::block_on(
            output_tx.send(
                format!(
                    "\x1b[30;1mDebug: Supported auth methods: {}\x1b[0m\r\n",
                    auth_methods
                )
                .into_bytes(),
            ),
        );
    }

    if auth_methods.contains("password") {
        match sess.userauth_password(username, password) {
            Ok(_) => auth_success = true,
            Err(e) => {
                let _ =
                    futures::executor::block_on(output_tx.send(
                        format!("\x1b[33mPassword auth failed: {}\x1b[0m\r\n", e).into_bytes(),
                    ));
            }
        }
    }

    // fallback to keyboard-interactive if password failed or not supported
    if !auth_success && auth_methods.contains("keyboard-interactive") {
        if debug_mode {
            let _ = futures::executor::block_on(
                output_tx.send(b"\x1b[36mTrying keyboard-interactive auth...\x1b[0m\r\n".to_vec()),
            );
        }

        struct Prompter {
            password: String,
        }

        impl ssh2::KeyboardInteractivePrompt for Prompter {
            fn prompt(
                &mut self,
                _username: &str,
                _instructions: &str,
                prompts: &[ssh2::Prompt],
            ) -> Vec<String> {
                prompts.iter().map(|_| self.password.clone()).collect()
            }
        }

        let mut prompter = Prompter {
            password: password.to_string(),
        };

        match sess.userauth_keyboard_interactive(username, &mut prompter) {
            Ok(_) => {} // Success handled by sess.authenticated() check
            Err(e) => {
                let _ = futures::executor::block_on(
                    output_tx.send(
                        format!("\x1b[33mKeyboard-interactive auth failed: {}\x1b[0m\r\n", e)
                            .into_bytes(),
                    ),
                );
            }
        }
    }

    if !sess.authenticated() {
        let _ = futures::executor::block_on(
            output_tx.send(b"\x1b[31mAuthentication failed (all methods)!\x1b[0m\r\n".to_vec()),
        );
        return Err("Authentication failed".to_string());
    }

    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mAuthenticated! Opening channel...\x1b[0m\r\n".to_vec()),
    );

    // Open channel
    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("Failed to open channel: {}", e))?;

    // Request PTY
    channel
        .request_pty(
            "xterm-256color",
            Some(build_standard_pty_modes()),
            Some((80, 24, 0, 0)),
        )
        .map_err(|e| format!("Failed to request PTY: {}", e))?;

    // Start shell
    channel
        .shell()
        .map_err(|e| format!("Failed to start shell: {}", e))?;

    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mShell ready!\x1b[0m\r\n\r\n".to_vec()),
    );

    // Set channel to non-blocking
    // Restore short timeout for interactive loop to trigger WouldBlock and allow polling
    let _ = tcp_config_clone.set_read_timeout(Some(std::time::Duration::from_millis(100)));
    drop(tcp_config_clone);
    sess.set_blocking(false);

    let mut buf = [0u8; 4096];

    // Main I/O loop
    loop {
        // Read from SSH channel (stdout)
        match channel.read(&mut buf) {
            Ok(0) => {
                // EOF - connection closed
                let _ = futures::executor::block_on(
                    output_tx.send(b"\r\n\x1b[33mConnection closed.\x1b[0m\r\n".to_vec()),
                );
                break;
            }
            Ok(n) => {
                let _ = futures::executor::block_on(output_tx.send(buf[..n].to_vec()));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, continue
            }
            Err(e) => {
                let _ = futures::executor::block_on(
                    output_tx
                        .send(format!("\r\n\x1b[31mRead error: {}\x1b[0m\r\n", e).into_bytes()),
                );
                break;
            }
        }

        // Read from SSH channel (stderr)
        match channel.stderr().read(&mut buf) {
            Ok(n) if n > 0 => {
                let _ = futures::executor::block_on(output_tx.send(buf[..n].to_vec()));
            }
            _ => {}
        }

        // Check for input from user (non-blocking)
        match input_rx.try_recv() {
            Ok(input) => match input {
                SshInput::Data(data) => {
                    if let Err(e) = channel.write_all(&data) {
                        let _ = futures::executor::block_on(output_tx.send(
                            format!("\r\n\x1b[31mWrite error: {}\x1b[0m\r\n", e).into_bytes(),
                        ));
                        break;
                    }
                    let _ = channel.flush();
                }
                SshInput::Resize { rows, cols } => {
                    if let Err(e) = channel.request_pty_size(cols.into(), rows.into(), None, None) {
                        if debug_mode {
                            let _ = futures::executor::block_on(output_tx.send(
                                format!("\r\n\x1b[33mResize failed: {}\x1b[0m\r\n", e).into_bytes(),
                            ));
                        }
                    }
                }
            },
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // No input, continue
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Input channel closed
                break;
            }
        }

        // Check if channel is closed
        if channel.eof() {
            let _ = futures::executor::block_on(
                output_tx
                    .send(b"\r\n\x1b[33mConnection closed by remote host.\x1b[0m\r\n".to_vec()),
            );
            break;
        }

        // Smart sleep - only sleep if we didn't do any work
        // This dramatically improves throughput
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let _ = channel.close();
    let _ = channel.wait_close();

    Ok(())
}

/// Build standard PTY modes (RFC 4254)
/// Configures Echo, Canonical mode, Signals, etc. to ensure consistent behavior.
fn build_standard_pty_modes() -> PtyModes {
    let mut modes = PtyModes::new();

    // Standard modes
    modes.set_boolean(PtyModeOpcode::ECHO, true);
    modes.set_boolean(PtyModeOpcode::ECHOE, true);
    modes.set_boolean(PtyModeOpcode::ECHOK, true);
    modes.set_boolean(PtyModeOpcode::ICANON, true);
    modes.set_boolean(PtyModeOpcode::ISIG, true);
    modes.set_boolean(PtyModeOpcode::IEXTEN, true);
    modes.set_boolean(PtyModeOpcode::OPOST, true);
    modes.set_boolean(PtyModeOpcode::ONLCR, true);

    modes
}
