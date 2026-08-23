use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use ssh_key::{Algorithm, EcdsaCurve, LineEnding, PrivateKey, PublicKey};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyInfo {
    pub name: String,
    pub algorithm: String,
    pub public_key: String,
    pub fingerprint: String,
    pub path: String,
}

pub fn get_app_keys_dir() -> PathBuf {
    let app_dir = crate::get_app_config_dir();
    app_dir.join("keys")
}

pub fn get_default_ssh_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");
        if let Ok(_) = fs::create_dir_all(&ssh_dir) {
            return ssh_dir;
        }
    }
    get_app_keys_dir()
}

pub fn get_keys_dir() -> PathBuf {
    get_default_ssh_dir()
}

pub fn get_default_os_ssh_path() -> String {
    get_default_ssh_dir().to_string_lossy().to_string()
}

pub fn find_private_key_path(name_or_path: &str) -> Option<PathBuf> {
    let trimmed = name_or_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }
    let in_default_ssh = get_default_ssh_dir().join(trimmed);
    if in_default_ssh.exists() {
        return Some(in_default_ssh);
    }
    let in_app = get_app_keys_dir().join(trimmed);
    if in_app.exists() {
        return Some(in_app);
    }
    if p.exists() {
        return Some(p.to_path_buf());
    }
    None
}

pub fn generate_key_pair(
    name: &str,
    algorithm: &str,
    passphrase: Option<String>,
    storage_dir: Option<&str>,
) -> Result<KeyInfo, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Key name cannot be empty".to_string());
    }

    // Sanitize filename
    if trimmed_name.contains('/') || trimmed_name.contains('\\') || trimmed_name.contains("..") {
        return Err("Key name contains invalid characters".to_string());
    }

    let target_dir = match storage_dir {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => get_default_ssh_dir(),
    };

    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create storage directory: {}", e))?;

    let priv_path = target_dir.join(trimmed_name);
    let pub_path = target_dir.join(format!("{}.pub", trimmed_name));

    if priv_path.exists() || pub_path.exists() {
        return Err(format!(
            "A key named '{}' already exists in {}",
            trimmed_name,
            target_dir.display()
        ));
    }

    let mut rng = OsRng;
    let private_key = match algorithm.to_lowercase().as_str() {
        "ed25519" => PrivateKey::random(&mut rng, Algorithm::Ed25519)
            .map_err(|e| format!("Failed to generate ED25519 key: {}", e))?,
        "rsa-2048" => {
            let rsa_key = ssh_key::private::RsaKeypair::random(&mut rng, 2048)
                .map_err(|e| format!("Failed to generate RSA-2048 key: {}", e))?;
            PrivateKey::from(rsa_key)
        }
        "rsa-4096" => {
            let rsa_key = ssh_key::private::RsaKeypair::random(&mut rng, 4096)
                .map_err(|e| format!("Failed to generate RSA-4096 key: {}", e))?;
            PrivateKey::from(rsa_key)
        }
        "ecdsa-p256" => PrivateKey::random(
            &mut rng,
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256,
            },
        )
        .map_err(|e| format!("Failed to generate ECDSA P-256 key: {}", e))?,
        "ecdsa-p384" => PrivateKey::random(
            &mut rng,
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP384,
            },
        )
        .map_err(|e| format!("Failed to generate ECDSA P-384 key: {}", e))?,
        "ecdsa-p521" => PrivateKey::random(
            &mut rng,
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP521,
            },
        )
        .map_err(|e| format!("Failed to generate ECDSA P-521 key: {}", e))?,
        _ => return Err(format!("Unsupported algorithm: {}", algorithm)),
    };

    let public_key = private_key.public_key();
    let pub_openssh = public_key
        .to_openssh()
        .map_err(|e| format!("Failed to format public key: {}", e))?;

    let fingerprint = public_key.fingerprint(Default::default()).to_string();

    if let Some(ref pwd) = passphrase {
        if !pwd.is_empty() {
            let encrypted = private_key
                .encrypt(&mut rng, pwd.as_bytes())
                .map_err(|e| format!("Failed to encrypt private key: {}", e))?;
            encrypted
                .write_openssh_file(&priv_path, LineEnding::LF)
                .map_err(|e| format!("Failed to write private key file: {}", e))?;
        } else {
            private_key
                .write_openssh_file(&priv_path, LineEnding::LF)
                .map_err(|e| format!("Failed to write private key file: {}", e))?;
        }
    } else {
        private_key
            .write_openssh_file(&priv_path, LineEnding::LF)
            .map_err(|e| format!("Failed to write private key file: {}", e))?;
    }

    fs::write(&pub_path, &pub_openssh)
        .map_err(|e| format!("Failed to write public key file: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&priv_path, fs::Permissions::from_mode(0o600));
        let _ = fs::set_permissions(&pub_path, fs::Permissions::from_mode(0o644));
    }

    let algo_display = format_algo_name(algorithm, public_key);

    Ok(KeyInfo {
        name: trimmed_name.to_string(),
        algorithm: algo_display,
        public_key: pub_openssh,
        fingerprint,
        path: priv_path.to_string_lossy().to_string(),
    })
}

fn format_algo_name(algo_hint: &str, pub_key: &PublicKey) -> String {
    match pub_key.key_data() {
        ssh_key::public::KeyData::Ed25519(_) => "ED25519".to_string(),
        ssh_key::public::KeyData::Rsa(_) => {
            if algo_hint.contains("4096") {
                "RSA-4096".to_string()
            } else if algo_hint.contains("2048") {
                "RSA-2048".to_string()
            } else {
                "RSA".to_string()
            }
        }
        ssh_key::public::KeyData::Ecdsa(curve) => match curve {
            ssh_key::public::EcdsaPublicKey::NistP256(_) => "ECDSA P-256".to_string(),
            ssh_key::public::EcdsaPublicKey::NistP384(_) => "ECDSA P-384".to_string(),
            ssh_key::public::EcdsaPublicKey::NistP521(_) => "ECDSA P-521".to_string(),
        },
        _ => algo_hint.to_uppercase(),
    }
}

pub fn list_keys() -> Result<Vec<KeyInfo>, String> {
    let mut list = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    // Check default SSH directory (~/.ssh) and app keys directory
    let dirs_to_scan = vec![get_default_ssh_dir(), get_app_keys_dir()];

    for dir in dirs_to_scan {
        if !dir.exists() {
            continue;
        }

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "pub" {
                            let priv_candidate = path.with_extension("");
                            let path_key = priv_candidate.to_string_lossy().to_string();
                            if seen_paths.contains(&path_key) {
                                continue;
                            }
                            seen_paths.insert(path_key.clone());

                            if let Ok(pub_content) = fs::read_to_string(&path) {
                                let key_name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("")
                                    .to_string();

                                if let Ok(pub_key) = PublicKey::from_openssh(&pub_content) {
                                    let algo_display = format_algo_name("", &pub_key);
                                    let fingerprint =
                                        pub_key.fingerprint(Default::default()).to_string();

                                    list.push(KeyInfo {
                                        name: key_name,
                                        algorithm: algo_display,
                                        public_key: pub_content.trim().to_string(),
                                        fingerprint,
                                        path: priv_candidate.to_string_lossy().to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(list)
}

pub fn delete_key(path_or_name: &str) -> Result<(), String> {
    let target = Path::new(path_or_name);
    let (priv_path, pub_path) = if target.is_absolute() {
        let priv_p = target.to_path_buf();
        let pub_p = if priv_p.extension().map_or(false, |e| e == "pub") {
            priv_p.clone()
        } else {
            PathBuf::from(format!("{}.pub", priv_p.display()))
        };
        (priv_p, pub_p)
    } else {
        let in_ssh = get_default_ssh_dir().join(path_or_name);
        if in_ssh.exists()
            || get_default_ssh_dir()
                .join(format!("{}.pub", path_or_name))
                .exists()
        {
            (
                in_ssh.clone(),
                get_default_ssh_dir().join(format!("{}.pub", path_or_name)),
            )
        } else {
            let in_app = get_app_keys_dir().join(path_or_name);
            (
                in_app.clone(),
                get_app_keys_dir().join(format!("{}.pub", path_or_name)),
            )
        }
    };

    if priv_path.exists() {
        let _ = fs::remove_file(priv_path);
    }
    if pub_path.exists() {
        let _ = fs::remove_file(pub_path);
    }

    Ok(())
}
