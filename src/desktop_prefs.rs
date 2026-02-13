use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DesktopPreferences {
    pub always_on_top: bool,
    pub auto_lock_minutes: u64,
    pub global_toggle_shortcut: String,
    pub launch_at_startup: bool,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            always_on_top: true,
            auto_lock_minutes: 5,
            global_toggle_shortcut: "Ctrl+Shift+Q".to_string(),
            launch_at_startup: false,
        }
    }
}

pub fn load(path: &Path) -> Result<DesktopPreferences> {
    if !path.exists() {
        return Ok(DesktopPreferences::default());
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read desktop preferences at {}", path.display()))?;
    let prefs = serde_json::from_slice::<DesktopPreferences>(&bytes)
        .with_context(|| format!("failed to parse desktop preferences at {}", path.display()))?;
    Ok(prefs)
}

pub fn save(path: &Path, preferences: &DesktopPreferences) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create preferences directory {}",
                    parent.display()
                )
            })?;
        }
    }

    let data = serde_json::to_vec_pretty(preferences).context("failed to serialize preferences")?;
    fs::write(path, data)
        .with_context(|| format!("failed to write desktop preferences at {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{DesktopPreferences, load, save};

    #[test]
    fn load_defaults_when_file_missing() {
        let dir = tempdir().expect("temp dir should be created");
        let path = dir.path().join("prefs.json");

        let prefs = load(&path).expect("load should succeed");
        assert_eq!(prefs, DesktopPreferences::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().expect("temp dir should be created");
        let path = dir.path().join("prefs.json");

        let prefs = DesktopPreferences {
            always_on_top: true,
            auto_lock_minutes: 15,
            global_toggle_shortcut: "Ctrl+Alt+V".to_string(),
            launch_at_startup: true,
        };

        save(&path, &prefs).expect("save should succeed");
        let loaded = load(&path).expect("load should succeed");
        assert_eq!(prefs, loaded);
    }
}
