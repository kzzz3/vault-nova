use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, anyhow, bail};
use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::RngCore;
use serde::{Deserialize, Serialize};

const FILE_VERSION: u8 = 1;
const KDF_ALGORITHM: &str = "argon2id";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub service: String,
    pub username: String,
    pub password: String,
    pub notes: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultData {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct UnlockedVault {
    pub salt: Vec<u8>,
    pub data: VaultData,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultFile {
    version: u8,
    kdf: KdfConfig,
    data: EncryptedBlob,
}

#[derive(Debug, Serialize, Deserialize)]
struct KdfConfig {
    algorithm: String,
    salt: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedBlob {
    nonce: String,
    ciphertext: String,
}

pub fn create_new(path: &Path, master_password: &str) -> Result<()> {
    if path.exists() {
        bail!("vault file already exists at {}", path.display())
    }

    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);

    let data = VaultData::default();
    save(path, master_password, &salt, &data)
}

pub fn open(path: &Path, master_password: &str) -> Result<UnlockedVault> {
    let raw = fs::read(path)
        .with_context(|| format!("failed to read vault file at {}", path.display()))?;
    let file: VaultFile = serde_json::from_slice(&raw).context("vault file is not valid JSON")?;

    if file.version != FILE_VERSION {
        bail!("unsupported vault version: {}", file.version)
    }
    if file.kdf.algorithm != KDF_ALGORITHM {
        bail!(
            "unsupported key derivation algorithm: {}",
            file.kdf.algorithm
        )
    }

    let salt = STANDARD
        .decode(file.kdf.salt)
        .context("invalid base64 salt")?;
    if salt.len() != SALT_LEN {
        bail!("vault salt has invalid length")
    }

    let nonce = STANDARD
        .decode(file.data.nonce)
        .context("invalid base64 nonce")?;
    if nonce.len() != NONCE_LEN {
        bail!("vault nonce has invalid length")
    }

    let ciphertext = STANDARD
        .decode(file.data.ciphertext)
        .context("invalid base64 ciphertext")?;

    let key = derive_key(master_password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| anyhow!("failed to create encryption cipher"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("failed to decrypt vault (wrong password or corrupted file)"))?;

    let data = serde_json::from_slice(&plaintext).context("vault content is corrupted")?;
    Ok(UnlockedVault { salt, data })
}

pub fn save(path: &Path, master_password: &str, salt: &[u8], data: &VaultData) -> Result<()> {
    if salt.len() != SALT_LEN {
        bail!("salt must be {SALT_LEN} bytes")
    }

    let key = derive_key(master_password, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| anyhow!("failed to create encryption cipher"))?;

    let plaintext = serde_json::to_vec(data).context("failed to serialize vault content")?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow!("failed to encrypt vault data"))?;

    let file = VaultFile {
        version: FILE_VERSION,
        kdf: KdfConfig {
            algorithm: KDF_ALGORITHM.to_string(),
            salt: STANDARD.encode(salt),
        },
        data: EncryptedBlob {
            nonce: STANDARD.encode(nonce),
            ciphertext: STANDARD.encode(ciphertext),
        },
    };

    let serialized = serde_json::to_vec_pretty(&file).context("failed to serialize vault file")?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create parent directory {}", parent.display())
            })?;
        }
    }
    let temp_path = temporary_path(path);
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| {
                format!(
                    "failed to create temporary vault file at {}",
                    temp_path.display()
                )
            })?;
        file.write_all(&serialized)
            .context("failed to write temporary vault file")?;
        file.sync_all()
            .context("failed to flush temporary vault file")?;
        drop(file);
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to replace vault file at {}", path.display()))?;
        }
        fs::rename(&temp_path, path)
            .with_context(|| format!("failed to install vault file at {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temp = path.to_path_buf();
    let extension = temp
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("vault");
    temp.set_extension(format!("{extension}.tmp-{}", std::process::id()));
    temp
}

fn derive_key(master_password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let mut key = [0u8; KEY_LEN];
    let argon2 = Argon2::default();
    argon2
        .hash_password_into(master_password.as_bytes(), salt, &mut key)
        .map_err(|error| anyhow!("failed to derive encryption key: {error}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{Entry, create_new, open, save};

    #[test]
    fn vault_roundtrip_create_save_open() {
        let dir = tempdir().expect("temp dir should be created");
        let vault_path = dir.path().join("vault.json");

        create_new(&vault_path, "master-pass").expect("vault should be created");

        let mut unlocked = open(&vault_path, "master-pass").expect("vault should open");
        assert_eq!(unlocked.data.entries.len(), 0);

        unlocked.data.entries.push(Entry {
            service: "github".to_string(),
            username: "alice".to_string(),
            password: "secret-123".to_string(),
            notes: Some("2fa enabled".to_string()),
            updated_at: 1,
        });

        save(&vault_path, "master-pass", &unlocked.salt, &unlocked.data)
            .expect("vault should save");

        let unlocked_again = open(&vault_path, "master-pass").expect("vault should reopen");
        assert_eq!(unlocked_again.data.entries.len(), 1);
        assert_eq!(unlocked_again.data.entries[0].service, "github");
        assert_eq!(unlocked_again.data.entries[0].username, "alice");
    }

    #[test]
    fn open_fails_with_wrong_password() {
        let dir = tempdir().expect("temp dir should be created");
        let vault_path = dir.path().join("vault.json");

        create_new(&vault_path, "master-pass").expect("vault should be created");
        let result = open(&vault_path, "wrong-pass");
        assert!(result.is_err());
    }

    #[test]
    fn open_fails_for_corrupted_file() {
        let dir = tempdir().expect("temp dir should be created");
        let vault_path = dir.path().join("vault.json");

        fs::write(&vault_path, b"not-json").expect("corrupted file should be written");
        let result = open(&vault_path, "master-pass");
        assert!(result.is_err());
    }
}
