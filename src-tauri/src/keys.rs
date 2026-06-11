use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use ssh_key::{PrivateKey, LineEnding, Algorithm};
use rand::rngs::OsRng;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyInfo {
    pub name: String,
    pub algorithm: String,
    pub public_key: String,
    pub fingerprint: String,
}

pub fn get_keys_dir() -> PathBuf {
    let app_dir = crate::get_app_config_dir();
    app_dir.join("keys")
}

pub fn generate_key_pair(name: &str, algorithm: &str, passphrase: Option<String>) -> Result<KeyInfo, String> {
    let keys_dir = get_keys_dir();
    fs::create_dir_all(&keys_dir).map_err(|e| format!("Failed to create keys directory: {}", e))?;

    let priv_path = keys_dir.join(name);
    let pub_path = keys_dir.join(format!("{}.pub", name));

    if priv_path.exists() || pub_path.exists() {
        return Err("A key with this name already exists".to_string());
    }

    let mut rng = OsRng;
    let private_key = match algorithm {
        "ed25519" => {
            PrivateKey::random(&mut rng, Algorithm::Ed25519)
                .map_err(|e| format!("Failed to generate key: {}", e))?
        }
        "rsa-2048" => {
            let rsa_key = ssh_key::private::RsaKeypair::random(&mut rng, 2048)
                .map_err(|e| format!("Failed to generate RSA key: {}", e))?;
            PrivateKey::from(rsa_key)
        }
        "rsa-4096" => {
            let rsa_key = ssh_key::private::RsaKeypair::random(&mut rng, 4096)
                .map_err(|e| format!("Failed to generate RSA key: {}", e))?;
            PrivateKey::from(rsa_key)
        }
        _ => return Err(format!("Unsupported algorithm: {}", algorithm)),
    };

    let public_key = private_key.public_key();
    let pub_openssh = public_key.to_openssh()
        .map_err(|e| format!("Failed to format public key: {}", e))?;

    let fingerprint = public_key.fingerprint(Default::default()).to_string();

    // Write private key
    if let Some(ref pwd) = passphrase {
        if !pwd.is_empty() {
            let encrypted = private_key.encrypt(&mut rng, pwd.as_bytes())
                .map_err(|e| format!("Failed to encrypt private key: {}", e))?;
            encrypted.write_openssh_file(&priv_path, LineEnding::LF)
                .map_err(|e| format!("Failed to write private key file: {}", e))?;
        } else {
            private_key.write_openssh_file(&priv_path, LineEnding::LF)
                .map_err(|e| format!("Failed to write private key file: {}", e))?;
        }
    } else {
        private_key.write_openssh_file(&priv_path, LineEnding::LF)
            .map_err(|e| format!("Failed to write private key file: {}", e))?;
    }

    // Write public key
    fs::write(&pub_path, &pub_openssh)
        .map_err(|e| format!("Failed to write public key file: {}", e))?;

    Ok(KeyInfo {
        name: name.to_string(),
        algorithm: algorithm.to_string(),
        public_key: pub_openssh,
        fingerprint,
    })
}

pub fn list_keys() -> Result<Vec<KeyInfo>, String> {
    let keys_dir = get_keys_dir();
    if !keys_dir.exists() {
        return Ok(Vec::new());
    }

    let mut list = Vec::new();
    let entries = fs::read_dir(keys_dir).map_err(|e| format!("Failed to read keys directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "pub" {
                    let pub_content = fs::read_to_string(&path)
                        .map_err(|e| format!("Failed to read public key: {}", e))?;
                    
                    let key_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();

                    // Parse public key to get fingerprint and algorithm
                    if let Ok(pub_key) = ssh_key::PublicKey::from_openssh(&pub_content) {
                        let algorithm = match pub_key.key_data() {
                            ssh_key::public::KeyData::Ed25519(_) => "ed25519",
                            ssh_key::public::KeyData::Rsa(_) => "rsa",
                            _ => "unknown",
                        }.to_string();

                        let fingerprint = pub_key.fingerprint(Default::default()).to_string();

                        list.push(KeyInfo {
                            name: key_name,
                            algorithm,
                            public_key: pub_content,
                            fingerprint,
                        });
                    }
                }
            }
        }
    }

    Ok(list)
}

pub fn delete_key(name: &str) -> Result<(), String> {
    let keys_dir = get_keys_dir();
    let priv_path = keys_dir.join(name);
    let pub_path = keys_dir.join(format!("{}.pub", name));

    if priv_path.exists() {
        fs::remove_file(priv_path).map_err(|e| format!("Failed to delete private key: {}", e))?;
    }
    if pub_path.exists() {
        fs::remove_file(pub_path).map_err(|e| format!("Failed to delete public key: {}", e))?;
    }

    Ok(())
}
