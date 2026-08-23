use serde::Serialize;
use std::fs;
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

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read known_hosts: {}", e))?;
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[0].to_string();
            let key_type = parts[1].to_string();
            let raw_key = format!("{} {}", parts[1], parts[2]);
            let fingerprint = match ssh_key::PublicKey::from_openssh(&raw_key) {
                Ok(pk) => pk.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
                Err(_) => "SHA256:...".to_string(),
            };
            entries.push(KnownHostEntry {
                name,
                key_type,
                fingerprint,
            });
        }
    }

    Ok(entries)
}

pub fn delete_known_host(host_name: &str) -> Result<(), String> {
    let path = get_known_hosts_path().ok_or("Could not determine home directory")?;
    if !path.exists() {
        return Err("known_hosts file not found".to_string());
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read known_hosts: {}", e))?;
    let mut new_lines = Vec::new();
    let mut found = false;

    for line in content.lines() {
        let is_match = {
            let parts: Vec<&str> = line.split_whitespace().collect();
            !parts.is_empty()
                && (parts[0] == host_name || parts[0].starts_with(&format!("[{}]:", host_name)))
        };

        if is_match {
            found = true;
        } else {
            new_lines.push(line);
        }
    }

    if !found {
        return Err(format!("Host '{}' not found in known_hosts", host_name));
    }

    fs::write(&path, new_lines.join("\n") + "\n")
        .map_err(|e| format!("Failed to write known_hosts: {}", e))?;

    Ok(())
}

pub fn clear_all_known_hosts() -> Result<(), String> {
    let path = get_known_hosts_path().ok_or("Could not determine home directory")?;
    if path.exists() {
        fs::write(&path, "").map_err(|e| format!("Failed to clear known_hosts: {}", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_hosts_path_resolution() {
        let path = get_known_hosts_path();
        assert!(path.is_some());
    }
}
