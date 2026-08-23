use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs;
use std::path::PathBuf;

use crate::SavedSession;

#[derive(Serialize, Deserialize)]
struct EncryptedSessions {
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("Invalid hex length".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("Invalid hex: {}", e)))
        .collect()
}

fn encrypt_data(data: &[u8], password: &str) -> Result<EncryptedSessions, String> {
    let mut rng = rand::thread_rng();
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut salt);
    rng.fill(&mut nonce_bytes);

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 100_000, &mut key);

    let cipher = Aes256Gcm::new(aes_gcm::aead::generic_array::GenericArray::from_slice(&key));
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(EncryptedSessions {
        salt: to_hex(&salt),
        nonce: to_hex(&nonce_bytes),
        ciphertext: to_hex(&ciphertext),
    })
}

fn decrypt_data(enc: &EncryptedSessions, password: &str) -> Result<Vec<u8>, String> {
    let salt = from_hex(&enc.salt)?;
    let nonce_bytes = from_hex(&enc.nonce)?;
    let ciphertext = from_hex(&enc.ciphertext)?;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 100_000, &mut key);

    let cipher = Aes256Gcm::new(aes_gcm::aead::generic_array::GenericArray::from_slice(&key));
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

fn get_sessions_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    Ok(app_dir.join("sessions.json"))
}

pub fn is_master_password_set() -> Result<bool, String> {
    let path = get_sessions_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read sessions: {}", e))?;
    let trimmed = content.trim();
    Ok(trimmed.starts_with("{\"salt\":") || trimmed.starts_with("{\n  \"salt\":"))
}

pub fn load_sessions_secure(password: Option<&str>) -> Result<Vec<SavedSession>, String> {
    let path = get_sessions_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read sessions file: {}", e))?;
    let trimmed = content.trim();
    let is_encrypted = trimmed.starts_with("{\"salt\":") || trimmed.starts_with("{\n  \"salt\":");

    if is_encrypted {
        let pwd = match password {
            Some(p) => p,
            None => return Err("KEY_REQUIRED".to_string()),
        };

        let enc: EncryptedSessions = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed to parse encrypted sessions envelope: {}", e))?;

        let decrypted_bytes =
            decrypt_data(&enc, pwd).map_err(|_| "INVALID_PASSWORD".to_string())?;

        let sessions: Vec<SavedSession> = serde_json::from_slice(&decrypted_bytes)
            .map_err(|e| format!("Failed to parse decrypted sessions: {}", e))?;

        Ok(sessions)
    } else {
        let sessions: Vec<SavedSession> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse plaintext sessions: {}", e))?;
        Ok(sessions)
    }
}

pub fn load_sessions() -> Result<Vec<SavedSession>, String> {
    load_sessions_secure(None)
}

fn save_all_sessions_secure(
    sessions: &[SavedSession],
    password: Option<&str>,
) -> Result<(), String> {
    let path = get_sessions_path()?;
    let serialized = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed to serialize sessions: {}", e))?;

    if let Some(pwd) = password {
        if !pwd.is_empty() {
            let encrypted = encrypt_data(serialized.as_bytes(), pwd)?;
            let content = serde_json::to_string_pretty(&encrypted)
                .map_err(|e| format!("Failed to serialize encrypted envelope: {}", e))?;
            fs::write(&path, content)
                .map_err(|e| format!("Failed to write sessions file: {}", e))?;
            return Ok(());
        }
    }

    fs::write(&path, serialized).map_err(|e| format!("Failed to write sessions file: {}", e))?;
    Ok(())
}

pub fn save_session_secure(session: SavedSession, password: Option<&str>) -> Result<(), String> {
    let mut sessions = load_sessions_secure(password)?;
    if let Some(existing) = sessions.iter_mut().find(|s| s.id == session.id) {
        *existing = session;
    } else {
        sessions.push(session);
    }
    save_all_sessions_secure(&sessions, password)
}

pub fn delete_session_secure(session_id: &str, password: Option<&str>) -> Result<(), String> {
    let mut sessions = load_sessions_secure(password)?;
    sessions.retain(|s| s.id != session_id);
    save_all_sessions_secure(&sessions, password)
}

pub fn save_session(session: SavedSession) -> Result<(), String> {
    save_session_secure(session, None)
}

pub fn delete_session(session_id: &str) -> Result<(), String> {
    delete_session_secure(session_id, None)
}

pub fn change_master_password(
    old_password: Option<&str>,
    new_password: Option<&str>,
) -> Result<(), String> {
    let sessions = load_sessions_secure(old_password)?;
    save_all_sessions_secure(&sessions, new_password)
}

pub fn save_password(id: &str, password: &str) -> Result<(), String> {
    let creds_dir = crate::get_app_config_dir().join(".creds");
    let _ = fs::create_dir_all(&creds_dir);
    let path = creds_dir.join(format!("{}.pwd", id));
    fs::write(path, password).map_err(|e| e.to_string())
}

pub fn get_password(id: &str) -> Result<Option<String>, String> {
    let creds_dir = crate::get_app_config_dir().join(".creds");
    let path = creds_dir.join(format!("{}.pwd", id));
    if path.exists() {
        Ok(fs::read_to_string(path).ok())
    } else {
        Ok(None)
    }
}

pub fn delete_password(id: &str) -> Result<(), String> {
    let creds_dir = crate::get_app_config_dir().join(".creds");
    let path = creds_dir.join(format!("{}.pwd", id));
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub fn rename_folder(old_path: &str, new_path: &str) -> Result<(), String> {
    let mut sessions = load_sessions()?;
    let old_prefix = format!("{}/", old_path);
    for s in &mut sessions {
        if let Some(ref f) = s.folder {
            if f == old_path {
                s.folder = Some(new_path.to_string());
            } else if f.starts_with(&old_prefix) {
                let suffix = &f[old_path.len()..];
                s.folder = Some(format!("{}{}", new_path, suffix));
            }
        }
    }
    save_all_sessions_secure(&sessions, None)
}

pub fn delete_folder(folder_path: &str) -> Result<(), String> {
    let mut sessions = load_sessions()?;
    let prefix = format!("{}/", folder_path);
    sessions.retain(|s| {
        if let Some(ref f) = s.folder {
            f != folder_path && !f.starts_with(&prefix)
        } else {
            true
        }
    });
    save_all_sessions_secure(&sessions, None)
}
