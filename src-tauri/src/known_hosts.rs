use serde::Serialize;
use ssh2::Session;
use std::path::PathBuf;

#[derive(Debug, Serialize, Clone)]
pub struct KnownHostEntry {
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
}

fn get_known_hosts_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

pub fn get_known_hosts() -> Result<Vec<KnownHostEntry>, String> {
    let path = get_known_hosts_path().ok_or("Could not determine home directory")?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    // Initialize a session just to access KnownHosts parsing logic
    let sess = Session::new().map_err(|e| format!("Failed to create SSH session: {}", e))?;
    let mut known_hosts = sess
        .known_hosts()
        .map_err(|e| format!("Failed to init known hosts: {}", e))?;

    known_hosts
        .read_file(&path, ssh2::KnownHostFileKind::OpenSSH)
        .map_err(|e| format!("Failed to read known_hosts file: {}", e))?;

    let mut entries = Vec::new();

    // Iterate over hosts
    // ssh2 crate known_hosts().hosts() returns Result<Vec<Host>, Error>
    let hosts = known_hosts
        .hosts()
        .map_err(|e| format!("Failed to get hosts list: {}", e))?;

    for host in hosts {
        let name = host.name().unwrap_or("Unknown").to_string();
        let key_type = "Unknown".to_string();

        entries.push(KnownHostEntry {
            name,
            key_type,
            fingerprint: "SHA256:...".to_string(),
        });
    }

    Ok(entries)
}

pub fn delete_known_host(host_name: &str) -> Result<(), String> {
    let path = get_known_hosts_path().ok_or("Could not determine home directory")?;

    // Use ssh2 to remove
    let sess = Session::new().map_err(|e| format!("Failed to create session: {}", e))?;
    let mut known_hosts = sess
        .known_hosts()
        .map_err(|e| format!("Failed to init known hosts: {}", e))?;

    known_hosts
        .read_file(&path, ssh2::KnownHostFileKind::OpenSSH)
        .map_err(|e| format!("Failed to read known_hosts: {}", e))?;

    // Get hosts list
    let hosts = known_hosts
        .hosts()
        .map_err(|e| format!("Failed to get hosts: {}", e))?;

    // Find hosts to remove
    // We cannot remove while iterating if we are iterating the struct itself,
    // but here `hosts` is a Vec<Host> copy.
    let mut found = false;

    for host in hosts {
        if let Some(name) = host.name() {
            // Check if name matches or if it matches [name]:port
            if name == host_name || name.starts_with(&format!("[{}]:", host_name)) {
                known_hosts
                    .remove(&host)
                    .map_err(|e| format!("Failed to remove host: {}", e))?;
                found = true;
            }
        }
    }

    if !found {
        return Err(format!("Host '{}' not found in known_hosts", host_name));
    }

    // Write back to file
    known_hosts
        .write_file(&path, ssh2::KnownHostFileKind::OpenSSH)
        .map_err(|e| format!("Failed to write known_hosts: {}", e))?;

    Ok(())
}
