//! Session import/export functionality
//! Supports SecureCRT XML and PuTTY registry formats

use crate::{QuickCommand, SavedSession};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use pbkdf2::pbkdf2_hmac_array;
use sha2::Sha256;

/// Supported import/export formats
#[derive(Debug, Clone, PartialEq)]
pub enum SessionFormat {
    SecureCRT,
    PuTTY,
    SpurXEncrypted,
    SpurXPlaintext,
}

impl std::str::FromStr for SessionFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "securecrt" | "xml" => Ok(SessionFormat::SecureCRT),
            "putty" | "reg" => Ok(SessionFormat::PuTTY),
            "spurx_encrypted" | "spurx" => Ok(SessionFormat::SpurXEncrypted),
            "spurx_plaintext" | "json" => Ok(SessionFormat::SpurXPlaintext),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedSession {
    pub session: SavedSession,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpurxExportPayload {
    #[serde(default)]
    pub sessions: Vec<ExportedSession>,
    #[serde(default)]
    pub commands: Vec<QuickCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPayload {
    pub sessions: Vec<SavedSession>,
    pub commands: Vec<QuickCommand>,
}

// ============================================
// SecureCRT XML Import
// ============================================

/// Parse SecureCRT XML export file
pub fn import_securecrt(path: &Path) -> Result<Vec<SavedSession>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    parse_securecrt_xml(&content)
}

/// Parse SecureCRT XML content
fn parse_securecrt_xml(xml: &str) -> Result<Vec<SavedSession>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut sessions = Vec::new();
    let mut folder_stack: Vec<String> = Vec::new();
    let mut in_sessions = false;
    let mut current_session: Option<SessionBuilder> = None;
    let mut current_element_name = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                if tag_name == "key" {
                    // Get the name attribute
                    if let Some(name_attr) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"name")
                    {
                        let name = String::from_utf8_lossy(&name_attr.value).to_string();

                        if name == "Sessions" {
                            in_sessions = true;
                        } else if in_sessions {
                            // This could be a folder or a session
                            folder_stack.push(name);
                        }
                    }
                } else if in_sessions && (tag_name == "string" || tag_name == "dword") {
                    // Get the name attribute for property tracking
                    if let Some(name_attr) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"name")
                    {
                        current_element_name =
                            String::from_utf8_lossy(&name_attr.value).to_string();
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                if tag_name == "key" && in_sessions && !folder_stack.is_empty() {
                    // Check if we're ending a session
                    if let Some(builder) = current_session.take() {
                        if builder.is_session {
                            // Build folder path from stack (excluding current session name)
                            let folder_path = if folder_stack.len() > 1 {
                                Some(folder_stack[..folder_stack.len() - 1].join("/"))
                            } else {
                                None
                            };

                            if let Some(session) = builder.build(folder_path) {
                                sessions.push(session);
                            }
                        }
                    }
                    folder_stack.pop();
                } else if tag_name == "key" && in_sessions && folder_stack.is_empty() {
                    in_sessions = false;
                }
                current_element_name.clear();
            }
            Ok(Event::Text(ref e)) => {
                if in_sessions && !folder_stack.is_empty() {
                    let text = String::from_utf8_lossy(e.as_ref()).to_string();

                    // Initialize session builder if needed
                    if current_session.is_none() {
                        current_session = Some(SessionBuilder::new(
                            folder_stack.last().cloned().unwrap_or_default(),
                        ));
                    }

                    if let Some(ref mut builder) = current_session {
                        match current_element_name.as_str() {
                            "Is Session" => {
                                builder.is_session = text == "1";
                            }
                            "Hostname" => {
                                builder.hostname = Some(text);
                            }
                            "[SSH2] Port" => {
                                builder.port = text.parse().ok();
                            }
                            "Username" => {
                                if !text.is_empty() {
                                    builder.username = Some(text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {}", e)),
            _ => {}
        }
    }

    Ok(sessions)
}

/// Helper to build sessions during parsing
#[derive(Default)]
struct SessionBuilder {
    name: String,
    hostname: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    is_session: bool,
}

impl SessionBuilder {
    fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    fn build(self, folder: Option<String>) -> Option<SavedSession> {
        let hostname = self.hostname?;

        Some(SavedSession {
            id: Uuid::new_v4().to_string(),
            name: self.name,
            host: hostname,
            port: self.port.unwrap_or(22),
            folder,
            username: self.username,
            log_directory: None,
            use_global_credentials: false,
            auth_method: "password".to_string(),
            private_key_name: None,
            tunnels: Vec::new(),
            auto_start_tunnels: false,
        })
    }
}

// ============================================
// PuTTY Registry Import
// ============================================

/// Parse PuTTY registry export file (.reg)
pub fn import_putty(path: &Path) -> Result<Vec<SavedSession>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    parse_putty_reg(&content)
}

/// Parse PuTTY .reg file content
fn parse_putty_reg(content: &str) -> Result<Vec<SavedSession>, String> {
    let mut sessions = Vec::new();
    let mut current_session: Option<HashMap<String, String>> = None;
    let mut current_name = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Check for session key header
        if line.starts_with('[') && line.ends_with(']') {
            // Save previous session if any
            if let Some(props) = current_session.take() {
                if let Some(session) = build_putty_session(&current_name, &props) {
                    sessions.push(session);
                }
            }

            // Parse new session name from registry path
            let key_path = &line[1..line.len() - 1];
            if key_path.contains(r"PuTTY\Sessions\") {
                if let Some(name) = key_path.split(r"\Sessions\").last() {
                    // URL decode the session name
                    current_name = url_decode(name);
                    current_session = Some(HashMap::new());
                }
            }
        } else if let Some(ref mut props) = current_session {
            // Parse property line: "PropertyName"="value" or "PropertyName"=dword:value
            if let Some((key, value)) = parse_reg_line(line) {
                props.insert(key, value);
            }
        }
    }

    // Don't forget the last session
    if let Some(props) = current_session {
        if let Some(session) = build_putty_session(&current_name, &props) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

/// Parse a single registry line
fn parse_reg_line(line: &str) -> Option<(String, String)> {
    if !line.starts_with('"') {
        return None;
    }

    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    // Extract key name (strip quotes)
    let key = parts[0].trim_matches('"').to_string();

    // Extract value
    let value_str = parts[1].trim();
    let value = if value_str.starts_with('"') {
        // String value
        value_str.trim_matches('"').to_string()
    } else if let Some(hex) = value_str.strip_prefix("dword:") {
        // DWORD value - parse hex
        u32::from_str_radix(hex, 16)
            .map(|n| n.to_string())
            .unwrap_or_default()
    } else {
        value_str.to_string()
    };

    Some((key, value))
}

/// Build a SavedSession from PuTTY properties
fn build_putty_session(name: &str, props: &HashMap<String, String>) -> Option<SavedSession> {
    let hostname = props.get("HostName")?;

    if hostname.is_empty() {
        return None;
    }

    let port = props
        .get("PortNumber")
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);

    let username = props.get("UserName").cloned().filter(|s| !s.is_empty());

    Some(SavedSession {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        host: hostname.clone(),
        port,
        folder: Some("PuTTY Import".to_string()),
        username,
        log_directory: None,
        use_global_credentials: false,
        auth_method: "password".to_string(),
        private_key_name: None,
        tunnels: Vec::new(),
        auto_start_tunnels: false,
    })
}

/// URL decode a string (for PuTTY session names)
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else {
            result.push(c);
        }
    }

    result
}

// ============================================
// Export Functions
// ============================================

/// Export sessions to SecureCRT XML format
pub fn export_securecrt(sessions: &[SavedSession], path: &Path) -> Result<(), String> {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<VanDyke version="3.0">
    <key name="Sessions">
"#,
    );

    // Group sessions by folder
    let mut folders: HashMap<String, Vec<&SavedSession>> = HashMap::new();
    let mut root_sessions: Vec<&SavedSession> = Vec::new();

    for session in sessions {
        if let Some(ref folder) = session.folder {
            folders.entry(folder.clone()).or_default().push(session);
        } else {
            root_sessions.push(session);
        }
    }

    // Write root sessions
    for session in &root_sessions {
        write_securecrt_session(&mut xml, session, 2)?;
    }

    // Write folders with their sessions
    for (folder, folder_sessions) in &folders {
        let parts: Vec<&str> = folder.split('/').collect();
        let indent = 2;

        // Open folder keys
        for (i, part) in parts.iter().enumerate() {
            let spaces = "    ".repeat(indent + i);
            xml.push_str(&format!("{}<key name=\"{}\">\n", spaces, escape_xml(part)));
        }

        // Write sessions in folder
        for session in folder_sessions {
            write_securecrt_session(&mut xml, session, indent + parts.len())?;
        }

        // Close folder keys
        for i in (0..parts.len()).rev() {
            let spaces = "    ".repeat(indent + i);
            xml.push_str(&format!("{}</key>\n", spaces));
        }
    }

    xml.push_str("    </key>\n</VanDyke>\n");

    fs::write(path, xml).map_err(|e| format!("Failed to write file: {}", e))
}

/// Write a single session in SecureCRT format
fn write_securecrt_session(
    xml: &mut String,
    session: &SavedSession,
    indent: usize,
) -> Result<(), String> {
    let spaces = "    ".repeat(indent);
    let inner = "    ".repeat(indent + 1);

    xml.push_str(&format!(
        "{}<key name=\"{}\">\n",
        spaces,
        escape_xml(&session.name)
    ));
    xml.push_str(&format!("{}<dword name=\"Is Session\">1</dword>\n", inner));
    xml.push_str(&format!(
        "{}<string name=\"Protocol Name\">SSH2</string>\n",
        inner
    ));
    xml.push_str(&format!(
        "{}<string name=\"Hostname\">{}</string>\n",
        inner,
        escape_xml(&session.host)
    ));
    xml.push_str(&format!(
        "{}<dword name=\"[SSH2] Port\">{}</dword>\n",
        inner, session.port
    ));

    if let Some(ref username) = session.username {
        xml.push_str(&format!(
            "{}<string name=\"Username\">{}</string>\n",
            inner,
            escape_xml(username)
        ));
    }

    xml.push_str(&format!("{}</key>\n", spaces));

    Ok(())
}

/// Export sessions to PuTTY registry format
pub fn export_putty(sessions: &[SavedSession], path: &Path) -> Result<(), String> {
    let mut reg = String::from("Windows Registry Editor Version 5.00\n\n");

    for session in sessions {
        let encoded_name = url_encode(&session.name);

        reg.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\{}]\n",
            encoded_name
        ));
        reg.push_str(&format!("\"HostName\"=\"{}\"\n", session.host));
        reg.push_str(&format!("\"PortNumber\"=dword:{:08x}\n", session.port));
        reg.push_str("\"Protocol\"=\"ssh\"\n");

        if let Some(ref username) = session.username {
            reg.push_str(&format!("\"UserName\"=\"{}\"\n", username));
        }

        reg.push('\n');
    }

    fs::write(path, reg).map_err(|e| format!("Failed to write file: {}", e))
}

/// URL encode a string (for PuTTY session names)
fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            result.push(c);
        } else {
            result.push_str(&format!("%{:02X}", c as u32));
        }
    }
    result
}

/// Escape special XML characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ============================================
// SpurX Native Formats (Encrypted and Plaintext)
// ============================================

/// Derive AES key from password
fn derive_key_password(password: &str, salt: &[u8]) -> Key<Aes256Gcm> {
    let key = pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), salt, 100_000);
    *Key::<Aes256Gcm>::from_slice(&key)
}

/// Export to SpurX Plaintext format
pub fn export_spurx_plaintext(
    sessions: &[SavedSession],
    commands: &[QuickCommand],
    path: &Path,
) -> Result<(), String> {
    let mut payload_sessions = Vec::new();
    for session in sessions {
        let password = crate::session::get_password(&session.id).unwrap_or(None);
        payload_sessions.push(ExportedSession {
            session: session.clone(),
            password,
        });
    }

    let payload = SpurxExportPayload {
        sessions: payload_sessions,
        commands: commands.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Serialization error: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed to write file: {}", e))
}

/// Import from SpurX Plaintext format
pub fn import_spurx_plaintext(path: &Path) -> Result<ImportPayload, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let payload: SpurxExportPayload =
        serde_json::from_str(&content).map_err(|e| format!("Parse error: {}", e))?;

    let mut sessions = Vec::with_capacity(payload.sessions.len());
    for entry in payload.sessions {
        let mut session = entry.session;
        session.id = Uuid::new_v4().to_string();
        if let Some(pwd) = entry.password {
            let _ = crate::session::save_password(&session.id, &pwd);
        }
        sessions.push(session);
    }

    let mut commands = Vec::with_capacity(payload.commands.len());
    for mut cmd in payload.commands {
        cmd.id = Uuid::new_v4().to_string();
        commands.push(cmd);
    }

    Ok(ImportPayload { sessions, commands })
}

/// Export to SpurX Encrypted format
pub fn export_spurx_encrypted(
    sessions: &[SavedSession],
    commands: &[QuickCommand],
    path: &Path,
    password: &str,
) -> Result<(), String> {
    let mut payload_sessions = Vec::new();
    for session in sessions {
        let pwd = crate::session::get_password(&session.id).unwrap_or(None);
        payload_sessions.push(ExportedSession {
            session: session.clone(),
            password: pwd,
        });
    }
    let payload = SpurxExportPayload {
        sessions: payload_sessions,
        commands: commands.to_vec(),
    };
    let json = serde_json::to_vec(&payload).map_err(|e| format!("Serialization error: {}", e))?;

    let salt = Aes256Gcm::generate_nonce(&mut OsRng); // Using 12 bytes nonce as salt for simplicity here, or gen 16 bytes
    let derived_key = derive_key_password(password, &salt);
    let cipher = Aes256Gcm::new(&derived_key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message

    let ciphertext = cipher
        .encrypt(&nonce, json.as_ref())
        .map_err(|e| format!("Encryption error: {}", e))?;

    // File format: [salt 12 bytes] + [nonce 12 bytes] + [ciphertext]
    let mut final_data = Vec::new();
    final_data.extend_from_slice(&salt);
    final_data.extend_from_slice(&nonce);
    final_data.extend_from_slice(&ciphertext);

    fs::write(path, final_data).map_err(|e| format!("Failed to write file: {}", e))
}

/// Import from SpurX Encrypted format
pub fn import_spurx_encrypted(path: &Path, password: &str) -> Result<ImportPayload, String> {
    let content = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    if content.len() < 24 {
        return Err("Invalid file format: too short".into());
    }

    let salt = &content[0..12];
    let nonce = Nonce::from_slice(&content[12..24]);
    let ciphertext = &content[24..];

    let derived_key = derive_key_password(password, salt);
    let cipher = Aes256Gcm::new(&derived_key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed. Incorrect password?")?;

    let payload: SpurxExportPayload =
        serde_json::from_slice(&plaintext).map_err(|e| format!("Parse error: {}", e))?;

    let mut sessions = Vec::with_capacity(payload.sessions.len());
    for entry in payload.sessions {
        let mut session = entry.session;
        session.id = Uuid::new_v4().to_string();
        if let Some(pwd) = entry.password {
            let _ = crate::session::save_password(&session.id, &pwd);
        }
        sessions.push(session);
    }

    let mut commands = Vec::with_capacity(payload.commands.len());
    for mut cmd in payload.commands {
        cmd.id = Uuid::new_v4().to_string();
        commands.push(cmd);
    }

    Ok(ImportPayload { sessions, commands })
}

// ============================================
// Public Import/Export Interface
// ============================================

/// Import sessions from a file
pub fn import_sessions(
    path: &str,
    format: &str,
    password: Option<String>,
) -> Result<ImportPayload, String> {
    let path = Path::new(path);
    let format: SessionFormat = format.parse()?;

    match format {
        SessionFormat::SecureCRT => {
            let sessions = import_securecrt(path)?;
            Ok(ImportPayload {
                sessions,
                commands: Vec::new(),
            })
        }
        SessionFormat::PuTTY => {
            let sessions = import_putty(path)?;
            Ok(ImportPayload {
                sessions,
                commands: Vec::new(),
            })
        }
        SessionFormat::SpurXPlaintext => import_spurx_plaintext(path),
        SessionFormat::SpurXEncrypted => {
            let pwd = password.ok_or_else(|| "Password required for decryption".to_string())?;
            import_spurx_encrypted(path, &pwd)
        }
    }
}

/// Export sessions to a file
pub fn export_sessions(
    path: &str,
    format: &str,
    sessions: &[SavedSession],
    commands: &[QuickCommand],
    password: Option<String>,
) -> Result<(), String> {
    let path = Path::new(path);
    let format: SessionFormat = format.parse()?;

    match format {
        SessionFormat::SecureCRT => export_securecrt(sessions, path),
        SessionFormat::PuTTY => export_putty(sessions, path),
        SessionFormat::SpurXPlaintext => export_spurx_plaintext(sessions, commands, path),
        SessionFormat::SpurXEncrypted => {
            let pwd = password.ok_or_else(|| "Password required for encryption".to_string())?;
            export_spurx_encrypted(sessions, commands, path, &pwd)
        }
    }
}
