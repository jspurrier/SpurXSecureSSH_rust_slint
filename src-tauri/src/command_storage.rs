//! Command storage and management
//! Stores quick commands as JSON in user's config directory

use std::fs;
use std::path::PathBuf;

use crate::QuickCommand;

/// Get the commands file path
/// Get the commands file path
fn get_commands_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();

    // Ensure directory exists
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(app_dir.join("commands.json"))
}

/// Load all saved commands from disk
pub fn load_commands() -> Result<Vec<QuickCommand>, String> {
    let path = get_commands_path()?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read commands file: {}", e))?;

    let commands: Vec<QuickCommand> =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse commands: {}", e))?;

    Ok(commands)
}

/// Save all commands to disk
fn save_all_commands(commands: &[QuickCommand]) -> Result<(), String> {
    let path = get_commands_path()?;

    let content = serde_json::to_string_pretty(commands)
        .map_err(|e| format!("Failed to serialize commands: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write commands file: {}", e))?;

    Ok(())
}

/// Save a single command (adds or updates)
pub fn save_command(command: QuickCommand) -> Result<(), String> {
    let mut commands = load_commands()?;

    // Check if command already exists
    if let Some(existing) = commands.iter_mut().find(|c| c.id == command.id) {
        *existing = command;
    } else {
        commands.push(command);
    }

    save_all_commands(&commands)
}

/// Delete a command by ID
pub fn delete_command(command_id: &str) -> Result<(), String> {
    let mut commands = load_commands()?;
    commands.retain(|c| c.id != command_id);
    save_all_commands(&commands)
}
