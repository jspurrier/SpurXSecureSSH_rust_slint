use crate::QuickCommand;
use std::fs;
use std::path::PathBuf;

fn get_commands_path() -> Result<PathBuf, String> {
    let app_dir = crate::get_app_config_dir();
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    Ok(app_dir.join("commands.json"))
}

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

fn save_all_commands(commands: &[QuickCommand]) -> Result<(), String> {
    let path = get_commands_path()?;
    let content = serde_json::to_string_pretty(commands)
        .map_err(|e| format!("Failed to serialize commands: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write commands file: {}", e))?;
    Ok(())
}

pub fn save_command(command: QuickCommand) -> Result<(), String> {
    let mut commands = load_commands()?;
    if let Some(existing) = commands.iter_mut().find(|c| c.id == command.id) {
        *existing = command;
    } else {
        commands.push(command);
    }
    save_all_commands(&commands)
}

pub fn delete_command(command_id: &str) -> Result<(), String> {
    let mut commands = load_commands()?;
    commands.retain(|c| c.id != command_id);
    save_all_commands(&commands)
}

pub fn rename_folder(old_path: &str, new_path: &str) -> Result<(), String> {
    let mut commands = load_commands()?;
    let old_prefix = format!("{}/", old_path);
    for c in &mut commands {
        if c.folder == old_path {
            c.folder = new_path.to_string();
        } else if c.folder.starts_with(&old_prefix) {
            let suffix = &c.folder[old_path.len()..];
            c.folder = format!("{}{}", new_path, suffix);
        }
    }
    save_all_commands(&commands)
}

pub fn delete_folder(folder_path: &str) -> Result<(), String> {
    let mut commands = load_commands()?;
    let prefix = format!("{}/", folder_path);
    commands.retain(|c| c.folder != folder_path && !c.folder.starts_with(&prefix));
    save_all_commands(&commands)
}
