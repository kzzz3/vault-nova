#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

const DEFAULT_VAULT_PATH: &str = "vault.json";

fn main() {
    if let Err(error) = vault_nova::desktop::run(PathBuf::from(DEFAULT_VAULT_PATH)) {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
