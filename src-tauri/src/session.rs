//! Session storage and management
//! Stores sessions as JSON in user's config directory

use std::fs;
use std::path::PathBuf;

use crate::SavedSession;

/// Get the sessions file path
/// Get the sessions file path
fn get_sessions_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();

    // Ensure directory exists
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(app_dir.join("sessions.json"))
}

/// Load all saved sessions from disk
pub fn load_sessions() -> Result<Vec<SavedSession>, String> {
    let path = get_sessions_path()?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read sessions file: {}", e))?;

    let sessions: Vec<SavedSession> =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse sessions: {}", e))?;

    Ok(sessions)
}

/// Save all sessions to disk
fn save_all_sessions(sessions: &[SavedSession]) -> Result<(), String> {
    let path = get_sessions_path()?;

    let content = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed to serialize sessions: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write sessions file: {}", e))?;

    Ok(())
}

/// Save a single session (adds or updates)
pub fn save_session(session: SavedSession) -> Result<(), String> {
    let mut sessions = load_sessions()?;

    // Check if session already exists
    if let Some(existing) = sessions.iter_mut().find(|s| s.id == session.id) {
        *existing = session;
    } else {
        sessions.push(session);
    }

    save_all_sessions(&sessions)
}

/// Delete a session by ID
pub fn delete_session(session_id: &str) -> Result<(), String> {
    let mut sessions = load_sessions()?;
    sessions.retain(|s| s.id != session_id);
    save_all_sessions(&sessions)?;

    // Also delete password from keyring
    let _ = delete_password(session_id);
    Ok(())
}

/// Save password to system keyring
pub fn save_password(id: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("SpurXSecureSSH", id).map_err(|e| e.to_string())?;
    entry.set_password(password).map_err(|e| e.to_string())
}

/// Get password from system keyring
pub fn get_password(id: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new("SpurXSecureSSH", id).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(pwd) => Ok(Some(pwd)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Delete password from system keyring
pub fn delete_password(id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("SpurXSecureSSH", id).map_err(|e| e.to_string())?;
    match entry.delete_password() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
