# Vault Nova

Vault Nova is a local-first password manager written in Rust.

## Features

- Encrypted vault storage using `AES-256-GCM`
- Master-key derivation via `Argon2id`
- Tauri desktop UI (not browser-hosted)
- Unlock gate: access requires master password
- Session timeout with explicit lock action
- System tray integration for fast summon/hide
- Settings modal for always-on-top, auto-lock minutes, global hotkey, and launch-at-startup
- CLI for init/add/get/list/delete/generate
- Built-in random password generator

## Quick Start

```bash
cargo run
```

The desktop app opens as a native window (`vault-nova` binary).

On first launch, set your master password.

The app can also be summoned from the tray icon quickly.

You can also initialize from CLI first:

```bash
cargo run --bin vault-cli -- init
```

## CLI Commands

```bash
cargo run --bin vault-cli -- --help
```

Common examples:

```bash
cargo run --bin vault-cli -- add github alice
cargo run --bin vault-cli -- get github --username alice --show-password
cargo run --bin vault-cli -- list
cargo run --bin vault-cli -- delete github --username alice
cargo run --bin vault-cli -- generate --length 24
```

## Build and Package

Release binaries:

```bash
cargo build --release
```

- Desktop GUI binary (no console window in release): `target/release/vault-nova.exe`
- CLI binary: `target/release/vault-cli.exe`

Build Windows installer package (Tauri bundle):

```bash
cargo tauri build
```

Installer artifacts are generated under `target/release/bundle/` (for example `msi` or `nsis`, depending on your setup).

## Desktop Shortcuts

- `Ctrl + S`: save current entry in app
- `Ctrl + Shift + Q` (default): global hide/summon shortcut, configurable in Settings

If the configured global shortcut conflicts with another app, settings will show an error hint.

## Tray Behavior

- Left-click tray icon toggles show/hide
- Closing or minimizing the window hides to tray; reopening within 1 minute skips re-login
- Tray menu supports lock and quit

Desktop data files (default):

```text
vault.json
setting.json
```

## Tests

```bash
cargo test
```

Current tests cover password generation, vault encrypt/decrypt behavior, unlock flow, CRUD requirements, and wrong-password handling.
