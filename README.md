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

The app opens as a desktop window.

On first launch, set your master password.

The app can also be summoned from the tray icon quickly.

You can also initialize from CLI first:

```bash
cargo run -- init
```

## CLI Commands

```bash
cargo run -- --help
```

Common examples:

```bash
cargo run -- add github alice
cargo run -- get github --username alice --show-password
cargo run -- list
cargo run -- delete github --username alice
cargo run -- generate --length 24
cargo run -- desktop
```

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
