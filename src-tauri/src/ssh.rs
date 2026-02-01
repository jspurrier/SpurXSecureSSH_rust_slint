//! SSH connection management module
//! Handles SSH connections using the ssh2 library with PTY support

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use ssh2::Session;
use tokio::sync::mpsc;
use parking_lot::Mutex;

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
    input_rx: &mut std::sync::mpsc::Receiver<Vec<u8>>,
) -> Result<(), String> {
    // Send connection status
    let _ = futures::executor::block_on(
        output_tx.send(format!("\r\n\x1b[36mConnecting to {}:{}...\x1b[0m\r\n", host, port).into_bytes())
    );
    
    // Create TCP connection
    let addr = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| format!("TCP connection failed: {}", e))?;
    
    // Set non-blocking for reads (with timeout)
    tcp.set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;
    
    // Create SSH session
    let mut sess = Session::new()
        .map_err(|e| format!("Failed to create SSH session: {}", e))?;
    
    sess.set_tcp_stream(tcp);
    
    // Perform SSH handshake
    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mPerforming SSH handshake...\x1b[0m\r\n".to_vec())
    );
    
    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;
    
    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mAuthenticating...\x1b[0m\r\n".to_vec())
    );
    
    // Authenticate with password
    sess.userauth_password(username, password)
        .map_err(|e| format!("Authentication failed: {}", e))?;
    
    if !sess.authenticated() {
        let _ = futures::executor::block_on(
            output_tx.send(b"\x1b[31mAuthentication failed!\x1b[0m\r\n".to_vec())
        );
        return Err("Authentication failed".to_string());
    }
    
    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mAuthenticated! Opening channel...\x1b[0m\r\n".to_vec())
    );
    
    // Open channel
    let mut channel = sess.channel_session()
        .map_err(|e| format!("Failed to open channel: {}", e))?;
    
    // Request PTY
    channel.request_pty("xterm-256color", None, Some((80, 24, 0, 0)))
        .map_err(|e| format!("Failed to request PTY: {}", e))?;
    
    // Start shell
    channel.shell()
        .map_err(|e| format!("Failed to start shell: {}", e))?;
    
    let _ = futures::executor::block_on(
        output_tx.send(b"\x1b[36mShell ready!\x1b[0m\r\n\r\n".to_vec())
    );
    
    // Set channel to non-blocking
    sess.set_blocking(false);
    
    let mut buf = [0u8; 4096];
    
    // Main I/O loop
    loop {
        // Read from SSH channel (stdout)
        match channel.read(&mut buf) {
            Ok(0) => {
                // EOF - connection closed
                let _ = futures::executor::block_on(
                    output_tx.send(b"\r\n\x1b[33mConnection closed.\x1b[0m\r\n".to_vec())
                );
                break;
            }
            Ok(n) => {
                let _ = futures::executor::block_on(
                    output_tx.send(buf[..n].to_vec())
                );
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, continue
            }
            Err(e) => {
                let _ = futures::executor::block_on(
                    output_tx.send(format!("\r\n\x1b[31mRead error: {}\x1b[0m\r\n", e).into_bytes())
                );
                break;
            }
        }
        
        // Read from SSH channel (stderr)
        match channel.stderr().read(&mut buf) {
            Ok(n) if n > 0 => {
                let _ = futures::executor::block_on(
                    output_tx.send(buf[..n].to_vec())
                );
            }
            _ => {}
        }
        
        // Check for input from user (non-blocking)
        match input_rx.try_recv() {
            Ok(data) => {
                if let Err(e) = channel.write_all(&data) {
                    let _ = futures::executor::block_on(
                        output_tx.send(format!("\r\n\x1b[31mWrite error: {}\x1b[0m\r\n", e).into_bytes())
                    );
                    break;
                }
                let _ = channel.flush();
            }
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
                output_tx.send(b"\r\n\x1b[33mConnection closed by remote host.\x1b[0m\r\n".to_vec())
            );
            break;
        }
        
        // Small sleep to prevent busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    
    let _ = channel.close();
    let _ = channel.wait_close();
    
    Ok(())
}
