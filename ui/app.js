const tauriApi = window.__TAURI__;

if (!tauriApi || !tauriApi.core || typeof tauriApi.core.invoke !== "function") {
  document.body.innerHTML = "<main style='padding:24px;font-family:Segoe UI,sans-serif;'>无法连接 Tauri API。请使用桌面客户端启动。</main>";
  throw new Error("Tauri API not available");
}

const invoke = tauriApi.core.invoke;

let initialized = false;
let entries = [];
let selectedKey = "";
let desktopPrefs = {
  always_on_top: false,
  auto_lock_minutes: 5,
  hide_grace_minutes: 1,
  global_toggle_shortcut: "Ctrl+Shift+Q",
  launch_at_startup: false,
};
let capturingShortcut = false;
let sessionDeadlineMs = null;
let busy = false;
let formSnapshot = "";

const authPanel = document.getElementById("auth-panel");
const appPanel = document.getElementById("app-panel");
const sessionPill = document.getElementById("session-pill");
const authTitle = document.getElementById("auth-title");
const authTip = document.getElementById("auth-tip");
const authSubmit = document.getElementById("auth-submit");
const confirmWrap = document.getElementById("confirm-wrap");
const authStatus = document.getElementById("auth-status");
const appStatus = document.getElementById("app-status");
const entryMeta = document.getElementById("entry-meta");

const masterPasswordInput = document.getElementById("master-password");
const confirmPasswordInput = document.getElementById("confirm-password");
const searchInput = document.getElementById("search-input");
const serviceInput = document.getElementById("service-input");
const usernameInput = document.getElementById("username-input");
const passwordInput = document.getElementById("password-input");
const passwordVisibilityBtn = document.getElementById("password-visibility-btn");
const passwordCopyBtn = document.getElementById("password-copy-btn");
const notesInput = document.getElementById("notes-input");
const entryList = document.getElementById("entry-list");
const lengthInput = document.getElementById("length-input");
const typeSelect = document.getElementById("type-select");
const newBtn = document.getElementById("new-btn");
const saveBtn = document.getElementById("save-btn");
const revertBtn = document.getElementById("revert-btn");
const deleteBtn = document.getElementById("delete-btn");
const generateBtn = document.getElementById("generate-btn");
const refreshBtn = document.getElementById("refresh-btn");
const settingsBtn = document.getElementById("settings-btn");
const exportJsonBtn = document.getElementById("export-json-btn");
const importJsonBtn = document.getElementById("import-json-btn");

const settingsModal = document.getElementById("settings-modal");
const settingsStatus = document.getElementById("settings-status");
const autoLockInput = document.getElementById("auto-lock-input");
const hideGraceInput = document.getElementById("hide-grace-input");
const globalShortcutInput = document.getElementById("global-shortcut-input");
const captureShortcutBtn = document.getElementById("capture-shortcut-btn");
const launchAtStartupToggle = document.getElementById("launch-at-startup-toggle");
const alwaysOnTopToggle = document.getElementById("always-on-top-toggle");
const statusTimers = new WeakMap();
const TRANSFER_PATH_CACHE_KEY = "vaultNovaPlainTransferPath";
let transferPathHint = "vault-plain.json";

try {
  const cached = window.localStorage.getItem(TRANSFER_PATH_CACHE_KEY);
  if (cached && cached.trim()) {
    transferPathHint = cached.trim();
  }
} catch (_) {
}

function setStatus(target, message, level = "") {
  target.textContent = message || "";
  target.className = "status" + (level ? ` ${level}` : "");
}

function setTransientStatus(target, message, level = "", timeoutMs = 1500) {
  setStatus(target, message, level);
  const activeTimer = statusTimers.get(target);
  if (activeTimer) {
    clearTimeout(activeTimer);
  }
  if (!message) {
    statusTimers.delete(target);
    return;
  }
  const timer = setTimeout(() => {
    if (target.textContent === message) {
      setStatus(target, "", "");
    }
    statusTimers.delete(target);
  }, timeoutMs);
  statusTimers.set(target, timer);
}

function normalizeError(error) {
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error.message === "string") {
    return error.message;
  }
  try {
    return JSON.stringify(error);
  } catch (_) {
    return "操作失败";
  }
}

function rememberTransferPath(path) {
  const trimmed = (path || "").trim();
  if (!trimmed) {
    return;
  }

  transferPathHint = trimmed;
  try {
    window.localStorage.setItem(TRANSFER_PATH_CACHE_KEY, trimmed);
  } catch (_) {
  }
}

async function pickTransferPath(kind) {
  const command = kind === "export" ? "pick_export_plain_vault_path" : "pick_import_plain_vault_path";
  const picked = await call(command, {
    payload: {
      hint_path: transferPathHint,
    },
  });

  if (!picked || typeof picked !== "string") {
    return "";
  }

  const normalized = picked.trim();
  if (!normalized) {
    return "";
  }

  return normalized;
}

async function call(command, payload = {}) {
  try {
    return await invoke(command, payload);
  } catch (error) {
    throw new Error(normalizeError(error));
  }
}

function keyOf(entry) {
  return `${entry.service}::${entry.username}`;
}

function selectedEntryFromState() {
  if (!selectedKey) {
    return null;
  }
  return entries.find((entry) => keyOf(entry) === selectedKey) || null;
}

function resetPasswordMask() {
  passwordInput.type = "password";
  passwordVisibilityBtn.textContent = "👁";
  passwordVisibilityBtn.setAttribute("aria-label", "显示密码");
  passwordVisibilityBtn.setAttribute("title", "显示密码");
}

function applyEntryToForm(entry) {
  selectedKey = keyOf(entry);
  serviceInput.value = entry.service;
  usernameInput.value = entry.username;
  passwordInput.value = entry.password;
  notesInput.value = entry.notes || "";
  resetPasswordMask();

  const date = new Date(entry.updated_at * 1000);
  entryMeta.textContent = `上次更新: ${date.toLocaleString()}`;
  syncFormSnapshot();
  updateEditorActions();
}

function currentFormSnapshot() {
  return JSON.stringify({
    service: serviceInput.value,
    username: usernameInput.value,
    password: passwordInput.value,
    notes: notesInput.value,
  });
}

function syncFormSnapshot() {
  formSnapshot = currentFormSnapshot();
}

function hasUnsavedChanges() {
  return currentFormSnapshot() !== formSnapshot;
}

function confirmDiscardChanges() {
  return !hasUnsavedChanges() || window.confirm("当前条目有未保存修改，确定放弃吗？");
}

function setBusy(nextBusy, label = "") {
  busy = nextBusy;
  [saveBtn, deleteBtn, generateBtn, refreshBtn, newBtn, exportJsonBtn, importJsonBtn].forEach((button) => {
    button.disabled = nextBusy || (button === deleteBtn && !selectedEntryFromState());
  });
  if (nextBusy && label) setTransientStatus(appStatus, label, "", 30000);
}

function updateEditorActions() {
  deleteBtn.disabled = busy || !selectedEntryFromState();
  saveBtn.classList.toggle("has-changes", hasUnsavedChanges());
  if (selectedEntryFromState() && hasUnsavedChanges()) {
    entryMeta.textContent = "有未保存修改";
  }
}

function showAuthMode() {
  appPanel.classList.add("hidden");
  authPanel.classList.remove("hidden");
  masterPasswordInput.value = "";
  confirmPasswordInput.value = "";
  selectedKey = "";
  updateEditorActions();

  if (initialized) {
    authTitle.textContent = "解锁保险库";
    authTip.textContent = "输入主密码以访问你的密码库。";
    confirmWrap.classList.add("hidden");
    authSubmit.textContent = "解锁";
  } else {
    authTitle.textContent = "初始化保险库";
    authTip.textContent = "首次使用请设置主密码，之后每次进入都需要主密码。";
    confirmWrap.classList.remove("hidden");
    authSubmit.textContent = "创建并解锁";
  }

  masterPasswordInput.focus();
}

function showAppMode() {
  authPanel.classList.add("hidden");
  appPanel.classList.remove("hidden");
}

function clearForm() {
  serviceInput.value = "";
  usernameInput.value = "";
  passwordInput.value = "";
  notesInput.value = "";
  selectedKey = "";
  resetPasswordMask();
  entryMeta.textContent = "新建条目";
  syncFormSnapshot();
  updateEditorActions();
}

function renderEntries() {
  const q = searchInput.value.trim().toLowerCase();
  const filtered = entries.filter((entry) => {
    if (!q) {
      return true;
    }
    return entry.service.toLowerCase().includes(q) || entry.username.toLowerCase().includes(q);
  });

  entryList.innerHTML = "";
  if (!filtered.length) {
    entryList.innerHTML = '<p class="meta">没有匹配条目</p>';
    return;
  }

  filtered.forEach((entry) => {
    const item = document.createElement("article");
    item.className = "entry-item";
    if (selectedKey === keyOf(entry)) {
      item.classList.add("active");
    }

    const service = document.createElement("p");
    service.className = "entry-service";
    service.textContent = entry.service;

    const username = document.createElement("p");
    username.className = "entry-user";
    username.textContent = entry.username;

    item.appendChild(service);
    item.appendChild(username);
    item.addEventListener("click", () => {
      if (!confirmDiscardChanges()) return;
      applyEntryToForm(entry);
      renderEntries();
    });

    entryList.appendChild(item);
  });
}

function updateSessionPill(status) {
  if (!status.unlocked) {
    sessionPill.textContent = initialized ? "未解锁" : "待初始化";
    sessionDeadlineMs = null;
    return;
  }

  if (typeof status.expires_in_seconds === "number") {
    const mins = Math.max(1, Math.ceil(status.expires_in_seconds / 60));
    sessionPill.textContent = `已解锁 · 剩余约 ${mins} 分钟`;
    sessionDeadlineMs = Date.now() + status.expires_in_seconds * 1000;
  } else {
    sessionPill.textContent = "已解锁";
    sessionDeadlineMs = null;
  }
}

async function refreshStatus() {
  const status = await call("get_app_status");
  initialized = !!status.initialized;
  updateSessionPill(status);
  return status;
}

async function refreshDesktopPrefs() {
  const prefs = await call("get_desktop_preferences");
  desktopPrefs = Object.assign({}, desktopPrefs, prefs || {});

  autoLockInput.value = String(desktopPrefs.auto_lock_minutes || 5);
  hideGraceInput.value = String(desktopPrefs.hide_grace_minutes ?? 1);
  globalShortcutInput.value = desktopPrefs.global_toggle_shortcut || "Ctrl+Shift+Q";
  launchAtStartupToggle.checked = !!desktopPrefs.launch_at_startup;
  alwaysOnTopToggle.checked = !!desktopPrefs.always_on_top;

  if (desktopPrefs.global_shortcut_registered === false && desktopPrefs.global_shortcut_error) {
    setStatus(settingsStatus, desktopPrefs.global_shortcut_error, "error");
  }
  return desktopPrefs;
}

function openSettingsModal() {
  settingsModal.classList.remove("hidden");
  setStatus(settingsStatus, "", "");
}

function closeSettingsModal() {
  settingsModal.classList.add("hidden");
  capturingShortcut = false;
  captureShortcutBtn.textContent = "按键录制";
}

function eventToShortcut(event) {
  const ignored = ["Control", "Shift", "Alt", "Meta"];
  if (ignored.includes(event.key)) {
    return "";
  }

  const parts = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");

  if (!parts.length) {
    return "";
  }

  const key = event.key.length === 1 ? event.key.toUpperCase() : event.key;
  parts.push(key);
  return parts.join("+");
}

async function loadEntries() {
  const data = await call("list_entries");
  entries = data.entries || [];
  entries.sort((a, b) => a.service.localeCompare(b.service) || a.username.localeCompare(b.username));

  if (selectedKey && !selectedEntryFromState()) {
    clearForm();
  }

  renderEntries();
  updateEditorActions();
}

async function onAuthSubmit(event) {
  event.preventDefault();
  setStatus(authStatus, "", "");

  const masterPassword = masterPasswordInput.value;
  if (!masterPassword) {
    setStatus(authStatus, "主密码不能为空", "error");
    return;
  }

  try {
    if (initialized) {
      await call("unlock_vault", { masterPassword });
    } else {
      await call("setup_vault", {
        masterPassword,
        confirmPassword: confirmPasswordInput.value,
      });
      initialized = true;
    }

    showAppMode();
    clearForm();
    await loadEntries();
    await refreshDesktopPrefs();
    await refreshStatus();
    setStatus(appStatus, "保险库已解锁", "ok");
  } catch (error) {
    setStatus(authStatus, error.message, "error");
  }
}

async function saveEntry() {
  if (busy) return;
  const payload = {
    service: serviceInput.value.trim(),
    username: usernameInput.value.trim(),
    password: passwordInput.value,
    notes: notesInput.value.trim() || null,
  };

  if (!payload.service || !payload.username || !payload.password) {
    setStatus(appStatus, "服务名、用户名和密码为必填项", "error");
    return;
  }

  setBusy(true, "正在保存...");
  try {
    await call("upsert_entry", { payload });
    await loadEntries();
    clearForm();
    renderEntries();
    setStatus(appStatus, "条目已保存，已切换到新建", "ok");
  } catch (error) {
    setStatus(appStatus, error.message, "error");
    if (error.message.includes("locked") || error.message.includes("expired")) {
      showAuthMode();
    }
  } finally {
    setBusy(false);
  }
}

async function deleteEntry() {
  const selectedEntry = selectedEntryFromState();

  if (!selectedEntry) {
    setStatus(appStatus, "删除前请先选择一个条目", "error");
    return;
  }

  if (!window.confirm(`确定删除“${selectedEntry.service} / ${selectedEntry.username}”吗？此操作不可撤销。`)) return;

  setBusy(true, "正在删除...");
  try {
    await call("delete_entry", {
      payload: {
        service: selectedEntry.service,
        username: selectedEntry.username,
      },
    });
    await loadEntries();
    clearForm();
    renderEntries();
    setStatus(appStatus, "条目已删除", "ok");
  } catch (error) {
    setStatus(appStatus, error.message, "error");
  } finally {
    setBusy(false);
  }
}

function revertChanges() {
  const selectedEntry = selectedEntryFromState();
  if (selectedEntry) {
    applyEntryToForm(selectedEntry);
    renderEntries();
    setTransientStatus(appStatus, "已撤销修改", "ok", 1500);
    return;
  }

  clearForm();
  renderEntries();
  setTransientStatus(appStatus, "已清空未保存内容", "ok", 1500);
}

async function generatePassword() {
  if (busy) return;
  const length = Number(lengthInput.value || 16);
  if (!Number.isFinite(length) || length < 8 || length > 128) {
    setStatus(appStatus, "长度必须在 8 到 128 之间", "error");
    return;
  }

  const type = typeSelect.value;
  const includeNumbers = type !== "letters_only";
  const includeSymbols = type === "letters_numbers_symbols";

  setBusy(true, "正在生成密码...");
  try {
    const data = await call("generate_password", {
      length,
      includeNumbers,
      includeSymbols,
    });
    passwordInput.value = data.password;
    setStatus(appStatus, "已生成新密码", "ok");
  } catch (error) {
    setStatus(appStatus, error.message, "error");
  } finally {
    setBusy(false);
  }
}

async function exportPlainVault() {
  const filePath = await pickTransferPath("export");
  if (!filePath) {
    return;
  }

  try {
    const result = await call("export_plain_vault", { payload: { file_path: filePath } });
    rememberTransferPath(filePath);
    setTransientStatus(appStatus, `已导出 ${result.entries} 条到 ${result.file_path}`, "ok", 2200);
  } catch (error) {
    setStatus(appStatus, error.message, "error");
    if (error.message.includes("locked") || error.message.includes("expired")) {
      showAuthMode();
    }
  }
}

async function importPlainVault() {
  const filePath = await pickTransferPath("import");
  if (!filePath) {
    return;
  }

  const confirmed = window.confirm("导入会覆盖当前金库所有条目，确定继续吗？");
  if (!confirmed) {
    return;
  }

  try {
    const result = await call("import_plain_vault", { payload: { file_path: filePath } });
    rememberTransferPath(filePath);
    clearForm();
    await loadEntries();
    renderEntries();
    setTransientStatus(appStatus, `已导入 ${result.entries} 条，来源 ${result.file_path}`, "ok", 2200);
  } catch (error) {
    setStatus(appStatus, error.message, "error");
    if (error.message.includes("locked") || error.message.includes("expired")) {
      showAuthMode();
    }
  }
}

async function saveSettings() {
  setStatus(settingsStatus, "", "");

  const autoLockMinutes = Number(autoLockInput.value || 5);
  const hideGraceMinutes = Number(hideGraceInput.value || 0);
  const globalToggleShortcut = (globalShortcutInput.value || "").trim();
  if (!Number.isFinite(autoLockMinutes) || autoLockMinutes < 1 || autoLockMinutes > 120) {
    setStatus(settingsStatus, "自动锁定时间需在 1 到 120 分钟", "error");
    return;
  }
  if (!Number.isFinite(hideGraceMinutes) || hideGraceMinutes < 0 || hideGraceMinutes > 120) {
    setStatus(settingsStatus, "免登录时间需在 0 到 120 分钟", "error");
    return;
  }
  if (hideGraceMinutes > autoLockMinutes) {
    setStatus(settingsStatus, "免登录时间不能超过会话超时时间", "error");
    return;
  }
  if (!globalToggleShortcut) {
    setStatus(settingsStatus, "全局快捷键不能为空", "error");
    return;
  }

  try {
    const prefs = await call("set_desktop_preferences", {
      payload: {
        always_on_top: !!alwaysOnTopToggle.checked,
        auto_lock_minutes: autoLockMinutes,
        hide_grace_minutes: hideGraceMinutes,
        global_toggle_shortcut: globalToggleShortcut,
        launch_at_startup: !!launchAtStartupToggle.checked,
      },
    });
    desktopPrefs = Object.assign({}, desktopPrefs, prefs);
    if (prefs.global_shortcut_registered === false && prefs.global_shortcut_error) {
      setTransientStatus(appStatus, `设置已保存，但${prefs.global_shortcut_error}`, "error", 1500);
    } else {
      setTransientStatus(appStatus, "设置已保存", "ok", 1500);
    }
    closeSettingsModal();
  } catch (error) {
    setStatus(settingsStatus, error.message, "error");
  }
}

async function boot() {
  try {
    await refreshDesktopPrefs();
    const status = await refreshStatus();
    if (status.unlocked) {
      showAppMode();
      clearForm();
      await loadEntries();
      setStatus(appStatus, "会话仍有效，已自动进入", "ok");
    } else {
      showAuthMode();
    }
  } catch (error) {
    showAuthMode();
    setStatus(authStatus, error.message, "error");
  }
}

document.getElementById("auth-form").addEventListener("submit", onAuthSubmit);

saveBtn.addEventListener("click", saveEntry);
newBtn.addEventListener("click", () => {
  if (!confirmDiscardChanges()) return;
  clearForm();
  renderEntries();
  setTransientStatus(appStatus, "已切换为新建模式", "ok", 1500);
});
revertBtn.addEventListener("click", revertChanges);
deleteBtn.addEventListener("click", deleteEntry);
generateBtn.addEventListener("click", generatePassword);

passwordVisibilityBtn.addEventListener("click", () => {
  const isHidden = passwordInput.type === "password";
  passwordInput.type = isHidden ? "text" : "password";
  passwordVisibilityBtn.textContent = isHidden ? "🙈" : "👁";
  passwordVisibilityBtn.setAttribute("aria-label", isHidden ? "隐藏密码" : "显示密码");
  passwordVisibilityBtn.setAttribute("title", isHidden ? "隐藏密码" : "显示密码");
});

refreshBtn.addEventListener("click", async () => {
  if (!confirmDiscardChanges()) return;
  if (busy) return;
  clearForm();
  renderEntries();
  setBusy(true, "正在刷新...");
  try {
    await loadEntries();
    setTransientStatus(appStatus, "条目已刷新，右侧已清空", "ok", 1500);
  } catch (error) {
    setStatus(appStatus, error.message, "error");
  } finally {
    setBusy(false);
  }
});

passwordCopyBtn.addEventListener("click", async () => {
  const password = passwordInput.value;
  if (!password) {
    setStatus(appStatus, "当前没有可复制的密码", "error");
    return;
  }
  try {
    await navigator.clipboard.writeText(password);
    setTransientStatus(appStatus, "密码已复制到剪贴板（请及时清除）", "ok", 2200);
  } catch (_) {
    setStatus(appStatus, "复制失败，请检查系统剪贴板权限", "error");
  }
});

settingsBtn.addEventListener("click", openSettingsModal);
exportJsonBtn.addEventListener("click", exportPlainVault);
importJsonBtn.addEventListener("click", importPlainVault);
document.getElementById("save-settings-btn").addEventListener("click", saveSettings);
document.getElementById("cancel-settings-btn").addEventListener("click", closeSettingsModal);
captureShortcutBtn.addEventListener("click", () => {
  capturingShortcut = !capturingShortcut;
  captureShortcutBtn.textContent = capturingShortcut ? "按下快捷键..." : "按键录制";
  setStatus(settingsStatus, capturingShortcut ? "请按下组合键（例如 Ctrl+Alt+V）" : "", "");
});

settingsModal.addEventListener("click", (event) => {
  if (event.target === settingsModal) {
    closeSettingsModal();
  }
});

searchInput.addEventListener("input", renderEntries);

[serviceInput, usernameInput, passwordInput, notesInput].forEach((input) => {
  input.addEventListener("input", updateEditorActions);
});

document.addEventListener("keydown", (event) => {
  if (capturingShortcut && !settingsModal.classList.contains("hidden")) {
    event.preventDefault();
    if (event.key === "Escape") {
      capturingShortcut = false;
      captureShortcutBtn.textContent = "按键录制";
      setStatus(settingsStatus, "已取消按键录制", "");
      return;
    }
    const shortcut = eventToShortcut(event);
    if (shortcut) {
      globalShortcutInput.value = shortcut;
      capturingShortcut = false;
      captureShortcutBtn.textContent = "按键录制";
      setStatus(settingsStatus, `已录制: ${shortcut}`, "ok");
    }
    return;
  }

  if (event.key === "Escape" && !settingsModal.classList.contains("hidden")) {
    closeSettingsModal();
    return;
  }

  if (event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey) {
    const key = event.key.toLowerCase();
    if (key === "s") {
      event.preventDefault();
      saveEntry();
    }
  }
});

setInterval(async () => {
  try {
    const status = await refreshStatus();
    if (!status.unlocked && !appPanel.classList.contains("hidden")) {
      showAuthMode();
    }
  } catch (_) {
  }
}, 5000);

setInterval(() => {
  if (!appPanel.classList.contains("hidden") && sessionDeadlineMs && Date.now() >= sessionDeadlineMs) {
    showAuthMode();
    sessionDeadlineMs = null;
  }
}, 1000);

window.addEventListener("focus", async () => {
  try {
    const status = await refreshStatus();
    if (!status.unlocked && !appPanel.classList.contains("hidden")) {
      showAuthMode();
    }
  } catch (_) {
  }
});

boot();
