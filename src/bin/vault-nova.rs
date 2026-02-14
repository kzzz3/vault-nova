#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

const DEFAULT_VAULT_PATH: &str = "vault.json";

fn main() {
    let vault_path = resolve_vault_path(PathBuf::from(DEFAULT_VAULT_PATH));
    if let Err(error) = vault_nova::desktop::run(vault_path) {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn resolve_vault_path(default_relative_path: PathBuf) -> PathBuf {
    if default_relative_path.is_absolute() {
        return default_relative_path;
    }

    let cwd_candidate = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(&default_relative_path));
    let exe_candidate = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join(&default_relative_path))
    });
    let app_data_candidate =
        app_data_dir().map(|dir| dir.join("Vault Nova").join(&default_relative_path));

    if let Some(path) = cwd_candidate.as_ref().filter(|path| path.exists()) {
        return path.clone();
    }

    if let Some(path) = exe_candidate.as_ref().filter(|path| path.exists()) {
        return path.clone();
    }

    if let Some(path) = app_data_candidate {
        return path;
    }

    exe_candidate
        .or(cwd_candidate)
        .unwrap_or(default_relative_path)
}

#[cfg(target_os = "windows")]
fn app_data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(target_os = "windows"))]
fn app_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}
