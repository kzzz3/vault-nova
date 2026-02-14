use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutoStartManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::app_core::{AppCore, AppStatus};
use crate::desktop_prefs::{self, DesktopPreferences};
use crate::vault::Entry;

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "vault_nova_tray";

const MENU_TOGGLE: &str = "tray_toggle";
const MENU_LOCK: &str = "tray_lock";
const MENU_QUIT: &str = "tray_quit";

pub struct DesktopShellState {
    prefs_path: PathBuf,
    prefs: Mutex<DesktopPreferences>,
    registered_shortcut: Mutex<Option<String>>,
    shortcut_error: Mutex<Option<String>>,
    hide_grace_deadline: Mutex<Option<u64>>,
}

impl DesktopShellState {
    pub fn new(vault_path: &Path) -> Self {
        let prefs_path = preferences_path(vault_path);
        let prefs = if prefs_path.exists() {
            desktop_prefs::load(&prefs_path).unwrap_or_default()
        } else {
            DesktopPreferences::default()
        };

        Self {
            prefs_path,
            prefs: Mutex::new(prefs),
            registered_shortcut: Mutex::new(None),
            shortcut_error: Mutex::new(None),
            hide_grace_deadline: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Result<DesktopPreferences> {
        let guard = self
            .prefs
            .lock()
            .map_err(|_| anyhow!("desktop preferences lock poisoned"))?;
        Ok(guard.clone())
    }

    pub fn update(
        &self,
        updater: impl FnOnce(&mut DesktopPreferences),
    ) -> Result<DesktopPreferences> {
        let mut guard = self
            .prefs
            .lock()
            .map_err(|_| anyhow!("desktop preferences lock poisoned"))?;

        updater(&mut guard);
        desktop_prefs::save(&self.prefs_path, &guard)?;
        Ok(guard.clone())
    }

    pub fn set_shortcut_state(
        &self,
        registered_shortcut: Option<String>,
        shortcut_error: Option<String>,
    ) -> Result<()> {
        {
            let mut registered_guard = self
                .registered_shortcut
                .lock()
                .map_err(|_| anyhow!("registered shortcut lock poisoned"))?;
            *registered_guard = registered_shortcut;
        }
        {
            let mut error_guard = self
                .shortcut_error
                .lock()
                .map_err(|_| anyhow!("shortcut error lock poisoned"))?;
            *error_guard = shortcut_error;
        }
        Ok(())
    }

    pub fn shortcut_feedback(&self) -> Result<(bool, Option<String>)> {
        let is_registered = self
            .registered_shortcut
            .lock()
            .map_err(|_| anyhow!("registered shortcut lock poisoned"))?
            .is_some();

        let error = self
            .shortcut_error
            .lock()
            .map_err(|_| anyhow!("shortcut error lock poisoned"))?
            .clone();

        Ok((is_registered, error))
    }

    pub fn schedule_hide_grace_lock(&self, seconds: u64) -> Result<()> {
        let now = current_timestamp()?;
        let mut guard = self
            .hide_grace_deadline
            .lock()
            .map_err(|_| anyhow!("hide grace lock poisoned"))?;
        *guard = Some(now + seconds);
        Ok(())
    }

    pub fn clear_hide_grace_lock(&self) -> Result<()> {
        let mut guard = self
            .hide_grace_deadline
            .lock()
            .map_err(|_| anyhow!("hide grace lock poisoned"))?;
        *guard = None;
        Ok(())
    }

    pub fn should_lock_after_hide_grace(&self) -> Result<bool> {
        let now = current_timestamp()?;
        let mut guard = self
            .hide_grace_deadline
            .lock()
            .map_err(|_| anyhow!("hide grace lock poisoned"))?;

        match *guard {
            Some(deadline) if now >= deadline => {
                *guard = None;
                Ok(true)
            }
            Some(_) => Ok(false),
            None => Ok(false),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EntriesPayload {
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertEntryPayload {
    service: String,
    username: String,
    password: String,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteEntryPayload {
    service: String,
    username: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeleteResult {
    removed: usize,
}

#[derive(Debug, Serialize)]
pub struct PasswordPayload {
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct VaultTransferPayload {
    file_path: String,
}

#[derive(Debug, Serialize)]
pub struct VaultTransferResult {
    file_path: String,
    entries: usize,
}

#[derive(Debug, Serialize)]
pub struct DesktopPreferencesPayload {
    always_on_top: bool,
    auto_lock_minutes: u64,
    hide_grace_minutes: u64,
    global_toggle_shortcut: String,
    global_shortcut_registered: bool,
    global_shortcut_error: Option<String>,
    launch_at_startup: bool,
}

impl DesktopPreferencesPayload {
    fn from_prefs(
        value: &DesktopPreferences,
        global_shortcut_registered: bool,
        global_shortcut_error: Option<String>,
    ) -> Self {
        Self {
            always_on_top: value.always_on_top,
            auto_lock_minutes: value.auto_lock_minutes,
            hide_grace_minutes: value.hide_grace_minutes,
            global_toggle_shortcut: value.global_toggle_shortcut.clone(),
            global_shortcut_registered,
            global_shortcut_error,
            launch_at_startup: value.launch_at_startup,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetDesktopPreferencesPayload {
    always_on_top: Option<bool>,
    auto_lock_minutes: Option<u64>,
    hide_grace_minutes: Option<u64>,
    global_toggle_shortcut: Option<String>,
    launch_at_startup: Option<bool>,
}

#[tauri::command]
pub fn get_app_status(state: tauri::State<'_, AppCore>) -> Result<AppStatus, String> {
    state.status().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn setup_vault(
    state: tauri::State<'_, AppCore>,
    master_password: String,
    confirm_password: String,
) -> Result<(), String> {
    state
        .setup(master_password, confirm_password)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn unlock_vault(
    state: tauri::State<'_, AppCore>,
    master_password: String,
) -> Result<(), String> {
    state
        .unlock(master_password)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_entries(state: tauri::State<'_, AppCore>) -> Result<EntriesPayload, String> {
    let entries = state.list_entries().map_err(|error| error.to_string())?;
    Ok(EntriesPayload { entries })
}

#[tauri::command]
pub fn upsert_entry(
    state: tauri::State<'_, AppCore>,
    payload: UpsertEntryPayload,
) -> Result<(), String> {
    state
        .upsert_entry(
            payload.service,
            payload.username,
            payload.password,
            payload.notes,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_entry(
    state: tauri::State<'_, AppCore>,
    payload: DeleteEntryPayload,
) -> Result<DeleteResult, String> {
    let removed = state
        .delete_entry(payload.service, payload.username)
        .map_err(|error| error.to_string())?;
    Ok(DeleteResult { removed })
}

#[tauri::command]
pub fn generate_password(
    state: tauri::State<'_, AppCore>,
    length: usize,
    include_numbers: bool,
    include_symbols: bool,
) -> Result<PasswordPayload, String> {
    let password = state
        .generate_password(length, include_numbers, include_symbols)
        .map_err(|error| error.to_string())?;
    Ok(PasswordPayload { password })
}

#[tauri::command]
pub fn export_plain_vault(
    state: tauri::State<'_, AppCore>,
    payload: VaultTransferPayload,
) -> Result<VaultTransferResult, String> {
    let path = payload.file_path.trim();
    if path.is_empty() {
        return Err("导出路径不能为空".to_string());
    }

    let (resolved_path, entries) = state
        .export_plain_json(PathBuf::from(path))
        .map_err(|error| error.to_string())?;

    Ok(VaultTransferResult {
        file_path: resolved_path.display().to_string(),
        entries,
    })
}

#[tauri::command]
pub fn import_plain_vault(
    state: tauri::State<'_, AppCore>,
    payload: VaultTransferPayload,
) -> Result<VaultTransferResult, String> {
    let path = payload.file_path.trim();
    if path.is_empty() {
        return Err("导入路径不能为空".to_string());
    }

    let (resolved_path, entries) = state
        .import_plain_json(PathBuf::from(path))
        .map_err(|error| error.to_string())?;

    Ok(VaultTransferResult {
        file_path: resolved_path.display().to_string(),
        entries,
    })
}

#[tauri::command]
pub fn get_desktop_preferences(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<DesktopPreferencesPayload, String> {
    let prefs = state.snapshot().map_err(|error| error.to_string())?;
    let (registered, error) = state
        .shortcut_feedback()
        .map_err(|error| error.to_string())?;
    Ok(DesktopPreferencesPayload::from_prefs(
        &prefs, registered, error,
    ))
}

#[tauri::command]
pub fn set_desktop_preferences(
    app: AppHandle,
    state: tauri::State<'_, DesktopShellState>,
    payload: SetDesktopPreferencesPayload,
) -> Result<DesktopPreferencesPayload, String> {
    let current = state.snapshot().map_err(|error| error.to_string())?;
    let next_auto_lock_minutes = payload
        .auto_lock_minutes
        .unwrap_or(current.auto_lock_minutes);
    let next_hide_grace_minutes = payload
        .hide_grace_minutes
        .unwrap_or(current.hide_grace_minutes);

    if !(1..=120).contains(&next_auto_lock_minutes) {
        return Err("自动锁定时间需在 1 到 120 分钟".to_string());
    }
    if next_hide_grace_minutes > 120 {
        return Err("免登录时间需在 0 到 120 分钟".to_string());
    }
    if next_hide_grace_minutes > next_auto_lock_minutes {
        return Err("免登录时间不能超过会话超时时间".to_string());
    }

    if let Some(shortcut) = payload.global_toggle_shortcut.as_ref() {
        update_global_shortcut(&app, &state, shortcut).map_err(|error| error.to_string())?;
    }

    let prefs = state
        .update(|preferences| {
            if let Some(always_on_top) = payload.always_on_top {
                preferences.always_on_top = always_on_top;
            }
            if let Some(auto_lock_minutes) = payload.auto_lock_minutes {
                preferences.auto_lock_minutes = auto_lock_minutes;
            }
            if let Some(hide_grace_minutes) = payload.hide_grace_minutes {
                preferences.hide_grace_minutes = hide_grace_minutes;
            }
            if let Some(shortcut) = payload.global_toggle_shortcut.as_ref() {
                preferences.global_toggle_shortcut = shortcut.trim().to_string();
            }
            if let Some(launch_at_startup) = payload.launch_at_startup {
                preferences.launch_at_startup = launch_at_startup;
            }
        })
        .map_err(|error| error.to_string())?;

    if let Some(auto_lock_minutes) = payload.auto_lock_minutes {
        let app_core = app.state::<AppCore>();
        app_core
            .set_session_timeout_minutes(auto_lock_minutes)
            .map_err(|error| error.to_string())?;
    }

    if let Some(launch_at_startup) = payload.launch_at_startup {
        let is_enabled = app
            .autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())?;

        if launch_at_startup && !is_enabled {
            app.autolaunch()
                .enable()
                .map_err(|error| error.to_string())?;
        } else if !launch_at_startup && is_enabled {
            app.autolaunch()
                .disable()
                .map_err(|error| error.to_string())?;
        }
    }

    apply_preferences_to_main_window(&app, &prefs).map_err(|error| error.to_string())?;
    let (registered, error) = state
        .shortcut_feedback()
        .map_err(|error| error.to_string())?;
    Ok(DesktopPreferencesPayload::from_prefs(
        &prefs, registered, error,
    ))
}

pub fn run(vault_path: PathBuf) -> Result<()> {
    let shell_state = DesktopShellState::new(&vault_path);

    let shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = toggle_main_visibility(app);
            }
        })
        .build();

    tauri::Builder::default()
        .manage(AppCore::new(vault_path))
        .manage(shell_state)
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(shortcut_plugin)
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = summon_main_window(app);
        }))
        .setup(|app| {
            let handle = app.handle().clone();
            setup_tray(&handle).map_err(|error| anyhow!(error))?;

            let state = handle.state::<DesktopShellState>();
            let prefs = state.snapshot().map_err(|error| anyhow!(error))?;
            let app_core = handle.state::<AppCore>();
            app_core
                .set_session_timeout_minutes(prefs.auto_lock_minutes)
                .map_err(|error| anyhow!(error))?;
            if prefs.launch_at_startup {
                let _ = handle.autolaunch().enable();
            } else {
                let _ = handle.autolaunch().disable();
            }
            if let Err(error) =
                update_global_shortcut(&handle, &state, &prefs.global_toggle_shortcut)
            {
                let message = format!("{error}");
                let _ = state.set_shortcut_state(None, Some(message.clone()));
                eprintln!(
                    "Failed to register global shortcut '{}': {message}",
                    prefs.global_toggle_shortcut
                );
            }
            apply_preferences_to_main_window(&handle, &prefs).map_err(|error| anyhow!(error))?;
            let window = main_window(&handle).map_err(|error| anyhow!(error))?;
            window.center().map_err(|error| anyhow!(error))?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if let Err(error) = handle_menu_action(app, id) {
                eprintln!("Tray action failed ({id}): {error:#}");
            }
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let state = window.state::<DesktopShellState>();
                let _ = schedule_hide_grace_from_prefs(&state);
                let _ = window.hide();
            }
            WindowEvent::Resized(_) => {
                if window.is_minimized().unwrap_or(false) {
                    let state = window.state::<DesktopShellState>();
                    let _ = schedule_hide_grace_from_prefs(&state);
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            setup_vault,
            unlock_vault,
            list_entries,
            upsert_entry,
            delete_entry,
            generate_password,
            export_plain_vault,
            import_plain_vault,
            get_desktop_preferences,
            set_desktop_preferences,
        ])
        .run(tauri::generate_context!())
        .map_err(|error| anyhow!("failed to run desktop app: {error}"))?;

    Ok(())
}

fn preferences_path(vault_path: &Path) -> PathBuf {
    let prefs_file_name = "setting.json";
    match vault_path.parent() {
        Some(parent) => parent.join(prefs_file_name),
        None => PathBuf::from(prefs_file_name),
    }
}

fn setup_tray(app: &AppHandle) -> Result<()> {
    let toggle = MenuItem::with_id(app, MENU_TOGGLE, "唤起 / 隐藏", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, MENU_LOCK, "锁定密码库", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出 Vault Nova", true, None::<&str>)?;

    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&toggle, &lock, &separator, &quit])?;

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Vault Nova")
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = toggle_main_visibility(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    tray_builder.build(app)?;
    Ok(())
}

fn handle_menu_action(app: &AppHandle, action_id: &str) -> Result<()> {
    match action_id {
        MENU_TOGGLE => {
            let _ = toggle_main_visibility(app)?;
        }
        MENU_LOCK => {
            let app_core = app.state::<AppCore>();
            app_core.lock()?;
            let state = app.state::<DesktopShellState>();
            let _ = state.clear_hide_grace_lock();
            summon_main_window(app)?;
        }
        MENU_QUIT => {
            app.exit(0);
        }
        _ => {}
    }

    Ok(())
}

fn apply_preferences_to_main_window(app: &AppHandle, prefs: &DesktopPreferences) -> Result<()> {
    let window = main_window(app)?;
    window
        .set_always_on_top(prefs.always_on_top)
        .context("failed to apply always-on-top preference")?;
    Ok(())
}

fn summon_main_window(app: &AppHandle) -> Result<()> {
    let state = app.state::<DesktopShellState>();
    if state.should_lock_after_hide_grace()? {
        let app_core = app.state::<AppCore>();
        app_core.lock()?;
    }

    let window = main_window(app)?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().context("failed to unminimize window")?;
    }

    let prefs = state.snapshot()?;
    let _ = state.clear_hide_grace_lock();

    window.show().context("failed to show main window")?;
    window
        .set_always_on_top(prefs.always_on_top)
        .context("failed to set always-on-top")?;
    window.set_focus().context("failed to focus main window")?;
    Ok(())
}

fn toggle_main_visibility(app: &AppHandle) -> Result<bool> {
    let window = main_window(app)?;
    if window
        .is_visible()
        .context("failed to inspect visibility")?
    {
        let state = app.state::<DesktopShellState>();
        let _ = schedule_hide_grace_from_prefs(&state);
        window.hide().context("failed to hide main window")?;
        Ok(false)
    } else {
        summon_main_window(app)?;
        Ok(true)
    }
}

fn update_global_shortcut(
    app: &AppHandle,
    state: &DesktopShellState,
    shortcut: &str,
) -> Result<()> {
    let normalized = shortcut.trim();
    if normalized.is_empty() {
        let _ = state.set_shortcut_state(None, Some("global shortcut cannot be empty".to_string()));
        return Err(anyhow!("global shortcut cannot be empty"));
    }

    let guard = state
        .registered_shortcut
        .lock()
        .map_err(|_| anyhow!("registered shortcut lock poisoned"))?;
    let current = guard.clone();
    drop(guard);

    if let Some(existing) = current.as_ref() {
        if existing == normalized {
            let _ = state.set_shortcut_state(Some(existing.clone()), None);
            return Ok(());
        }

        let _ = app.global_shortcut().unregister(existing.as_str());
    }

    if let Err(error) = app.global_shortcut().register(normalized) {
        let _ = state.set_shortcut_state(
            None,
            Some(format!(
                "快捷键 '{normalized}' 注册失败，可能与系统或其他软件冲突"
            )),
        );
        return Err(anyhow!(
            "failed to register shortcut '{normalized}': {error}"
        ));
    }

    let mut guard = state
        .registered_shortcut
        .lock()
        .map_err(|_| anyhow!("registered shortcut lock poisoned"))?;
    *guard = Some(normalized.to_string());
    drop(guard);
    let _ = state.set_shortcut_state(Some(normalized.to_string()), None);
    Ok(())
}

fn main_window(app: &AppHandle) -> Result<WebviewWindow> {
    app.get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| anyhow!("main window '{MAIN_WINDOW_LABEL}' was not found"))
}

fn schedule_hide_grace_from_prefs(state: &DesktopShellState) -> Result<()> {
    let prefs = state.snapshot()?;
    let minutes = prefs
        .hide_grace_minutes
        .min(prefs.auto_lock_minutes)
        .min(120);
    let seconds = minutes.saturating_mul(60);
    state.schedule_hide_grace_lock(seconds)
}

fn current_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?
        .as_secs())
}
