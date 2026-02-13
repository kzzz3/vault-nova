use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

use crate::password;
use crate::vault::{self, Entry};

const DEFAULT_SESSION_TTL_SECONDS: u64 = 5 * 60;

pub struct AppCore {
    vault_path: PathBuf,
    session: Mutex<Option<Session>>,
    session_ttl_seconds: Mutex<u64>,
}

struct Session {
    master_password: Zeroizing<String>,
    expires_at: u64,
}

#[derive(Debug, Serialize)]
pub struct AppStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub expires_in_seconds: Option<u64>,
}

impl AppCore {
    pub fn new(vault_path: PathBuf) -> Self {
        Self {
            vault_path,
            session: Mutex::new(None),
            session_ttl_seconds: Mutex::new(DEFAULT_SESSION_TTL_SECONDS),
        }
    }

    pub fn set_session_timeout_minutes(&self, minutes: u64) -> Result<u64> {
        if !(1..=120).contains(&minutes) {
            bail!("timeout minutes must be between 1 and 120")
        }

        let ttl = minutes * 60;
        {
            let mut guard = self
                .session_ttl_seconds
                .lock()
                .map_err(|_| anyhow!("session ttl lock poisoned"))?;
            *guard = ttl;
        }

        let now = current_timestamp()?;
        let mut session_guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("session lock poisoned"))?;
        if let Some(session) = session_guard.as_mut() {
            session.expires_at = now + ttl;
        }

        Ok(minutes)
    }

    pub fn status(&self) -> Result<AppStatus> {
        let initialized = self.vault_path.exists();
        let now = current_timestamp()?;

        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("session lock poisoned"))?;

        let mut unlocked = false;
        let mut expires_in_seconds = None;

        if let Some(session) = guard.as_ref() {
            if session.expires_at > now {
                unlocked = true;
                expires_in_seconds = Some(session.expires_at - now);
            }
        }

        if !unlocked {
            if let Some(mut expired_session) = guard.take() {
                expired_session.master_password.zeroize();
            }
        }

        Ok(AppStatus {
            initialized,
            unlocked,
            expires_in_seconds,
        })
    }

    pub fn setup(&self, master_password: String, confirm_password: String) -> Result<()> {
        if self.vault_path.exists() {
            bail!("vault already exists")
        }

        let master_password = Zeroizing::new(master_password);
        let confirm_password = Zeroizing::new(confirm_password);

        if master_password.trim().is_empty() {
            bail!("master password cannot be empty")
        }
        if master_password.as_str() != confirm_password.as_str() {
            bail!("passwords do not match")
        }

        vault::create_new(&self.vault_path, master_password.as_str())?;
        self.set_session(master_password.as_str().to_string())?;
        Ok(())
    }

    pub fn unlock(&self, master_password: String) -> Result<()> {
        if !self.vault_path.exists() {
            bail!("vault does not exist")
        }

        let master_password = Zeroizing::new(master_password);
        if master_password.is_empty() {
            bail!("master password cannot be empty")
        }

        vault::open(&self.vault_path, master_password.as_str())
            .context("wrong master password or corrupted vault")?;
        self.set_session(master_password.as_str().to_string())?;
        Ok(())
    }

    pub fn lock(&self) -> Result<()> {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("session lock poisoned"))?;
        if let Some(mut session) = guard.take() {
            session.master_password.zeroize();
        }
        Ok(())
    }

    pub fn list_entries(&self) -> Result<Vec<Entry>> {
        self.with_master_password(|master_password| {
            let mut unlocked = vault::open(&self.vault_path, master_password)?;
            unlocked.data.entries.sort_by(|left, right| {
                left.service
                    .cmp(&right.service)
                    .then(left.username.cmp(&right.username))
            });
            Ok(unlocked.data.entries)
        })
    }

    pub fn upsert_entry(
        &self,
        service: String,
        username: String,
        password_text: String,
        notes: Option<String>,
    ) -> Result<()> {
        let service = service.trim().to_string();
        let username = username.trim().to_string();

        if service.is_empty() || username.is_empty() || password_text.is_empty() {
            bail!("service, username and password are required")
        }

        self.with_master_password(|master_password| {
            let mut unlocked = vault::open(&self.vault_path, master_password)?;
            let updated_at = current_timestamp()?;

            if let Some(existing) = unlocked
                .data
                .entries
                .iter_mut()
                .find(|entry| entry.service == service && entry.username == username)
            {
                existing.password = password_text;
                existing.notes = notes;
                existing.updated_at = updated_at;
            } else {
                unlocked.data.entries.push(Entry {
                    service,
                    username,
                    password: password_text,
                    notes,
                    updated_at,
                });
            }

            vault::save(
                &self.vault_path,
                master_password,
                &unlocked.salt,
                &unlocked.data,
            )?;
            Ok(())
        })
    }

    pub fn delete_entry(&self, service: String, username: Option<String>) -> Result<usize> {
        let service = service.trim().to_string();
        if service.is_empty() {
            bail!("service is required")
        }

        self.with_master_password(|master_password| {
            let mut unlocked = vault::open(&self.vault_path, master_password)?;
            let before = unlocked.data.entries.len();

            unlocked.data.entries.retain(|entry| {
                if let Some(ref username) = username {
                    !(entry.service == service && entry.username == *username)
                } else {
                    entry.service != service
                }
            });

            let removed = before.saturating_sub(unlocked.data.entries.len());
            if removed == 0 {
                bail!("no matching entry found")
            }

            vault::save(
                &self.vault_path,
                master_password,
                &unlocked.salt,
                &unlocked.data,
            )?;

            Ok(removed)
        })
    }

    pub fn generate_password(
        &self,
        length: usize,
        include_numbers: bool,
        include_symbols: bool,
    ) -> Result<String> {
        self.ensure_unlocked()?;
        password::generate_password(length, include_numbers, include_symbols)
    }

    fn set_session(&self, master_password: String) -> Result<()> {
        let ttl = self.session_ttl_seconds()?;
        let expires_at = current_timestamp()? + ttl;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("session lock poisoned"))?;

        if let Some(mut old_session) = guard.take() {
            old_session.master_password.zeroize();
        }

        *guard = Some(Session {
            master_password: Zeroizing::new(master_password),
            expires_at,
        });
        Ok(())
    }

    fn ensure_unlocked(&self) -> Result<()> {
        self.with_master_password(|_| Ok(()))
    }

    fn with_master_password<T>(&self, f: impl FnOnce(&str) -> Result<T>) -> Result<T> {
        let now = current_timestamp()?;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("session lock poisoned"))?;

        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow!("vault is locked, please unlock first"))?;

        if session.expires_at <= now {
            if let Some(mut expired) = guard.take() {
                expired.master_password.zeroize();
            }
            bail!("session expired, please unlock again")
        }

        let ttl = self.session_ttl_seconds()?;
        session.expires_at = now + ttl;
        let master_password = session.master_password.clone();
        drop(guard);
        f(master_password.as_str())
    }

    fn session_ttl_seconds(&self) -> Result<u64> {
        let guard = self
            .session_ttl_seconds
            .lock()
            .map_err(|_| anyhow!("session ttl lock poisoned"))?;
        Ok(*guard)
    }
}

fn current_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::AppCore;

    fn vault_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("temp dir should be created");
        let path = dir.path().join("vault.json");
        (dir, path)
    }

    #[test]
    fn setup_unlock_and_status_flow() {
        let (_dir, path) = vault_path();
        let app = AppCore::new(path);

        let initial = app.status().expect("status should load");
        assert!(!initial.initialized);
        assert!(!initial.unlocked);

        app.setup("master-pass".to_string(), "master-pass".to_string())
            .expect("setup should work");

        let after_setup = app.status().expect("status should load");
        assert!(after_setup.initialized);
        assert!(after_setup.unlocked);

        app.lock().expect("lock should work");
        let locked = app.status().expect("status should load");
        assert!(!locked.unlocked);

        app.unlock("master-pass".to_string())
            .expect("unlock should work");
        let unlocked = app.status().expect("status should load");
        assert!(unlocked.unlocked);
    }

    #[test]
    fn entry_crud_requires_unlock() {
        let (_dir, path) = vault_path();
        let app = AppCore::new(path);
        app.setup("master-pass".to_string(), "master-pass".to_string())
            .expect("setup should work");

        app.upsert_entry(
            "github".to_string(),
            "alice".to_string(),
            "secret".to_string(),
            Some("with otp".to_string()),
        )
        .expect("upsert should work");

        let entries = app.list_entries().expect("list should work");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].service, "github");

        app.lock().expect("lock should work");
        assert!(app.list_entries().is_err());

        app.unlock("master-pass".to_string())
            .expect("unlock should work");
        let removed = app
            .delete_entry("github".to_string(), Some("alice".to_string()))
            .expect("delete should work");
        assert_eq!(removed, 1);
    }

    #[test]
    fn unlock_fails_for_wrong_password() {
        let (_dir, path) = vault_path();
        let app = AppCore::new(path);
        app.setup("master-pass".to_string(), "master-pass".to_string())
            .expect("setup should work");
        app.lock().expect("lock should work");

        let result = app.unlock("wrong-pass".to_string());
        assert!(result.is_err());
    }
}
