//! Session storage and management
//! Stores sessions as JSON in user's config directory

use std::fs;
use std::path::PathBuf;

use crate::SavedSession;

/// Get the sessions file path
fn get_sessions_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "Could not find config directory".to_string())?;
    
    let app_dir = config_dir.join("SpurXSecureSSH");
    
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
    
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read sessions file: {}", e))?;
    
    let sessions: Vec<SavedSession> = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse sessions: {}", e))?;
    
    Ok(sessions)
}

/// Save all sessions to disk
fn save_all_sessions(sessions: &[SavedSession]) -> Result<(), String> {
    let path = get_sessions_path()?;
    
    let content = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed to serialize sessions: {}", e))?;
    
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write sessions file: {}", e))?;
    
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
    save_all_sessions(&sessions)
}
