//! App settings: state loaded from `~/.agent-pets/settings.json`, commands for the settings window
//! and wizard, a single settings window, and `hook.exe` selection for Claude Code hook installation.
use pets_core::i18n::{self, Lang};
use pets_core::integrations::{self, AppId, Detected, Status};
use pets_core::settings::{self as core_settings, Settings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub struct SettingsState {
    pub home: PathBuf,
    pub path: PathBuf,
    pub current: RwLock<Settings>,
    pub first_run: Mutex<bool>,
    pub load_error: Mutex<Option<String>>,
}

impl SettingsState {
    pub fn load(home: PathBuf) -> SettingsState {
        let path = core_settings::path(&home);
        let l = core_settings::load(&path);
        SettingsState { home, path, current: RwLock::new(l.settings), first_run: Mutex::new(l.first_run), load_error: Mutex::new(l.error) }
    }
    pub fn get(&self) -> Settings { self.current.read().unwrap().clone() }
    /// Language of Rust-provided text: the setting or Windows language.
    pub fn lang(&self) -> Lang { i18n::current(self.get().language) }
    /// View for the UI; `system` is the Windows language (UI uses it to resolve `auto` after later changes).
    pub fn view(&self, system: Lang) -> SettingsView {
        let settings = self.get();
        let lang = i18n::resolve(settings.language, system == Lang::Pl);
        SettingsView { settings, first_run: *self.first_run.lock().unwrap(), load_error: self.load_error.lock().unwrap().clone(), lang, system_lang: system }
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

#[derive(Serialize, Clone, Debug)]
pub struct SettingsView { pub settings: Settings, pub first_run: bool, pub load_error: Option<String>, pub lang: Lang, pub system_lang: Lang }

#[derive(Serialize, Clone, Debug)]
pub struct AppRow { pub id: AppId, pub detected: Detected, pub status: Status, pub enabled: bool }

/// Report for the Diagnostics tab. No tokens, content, or session titles.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Diagnostics {
    pub version: String,
    pub endpoint_port: Option<u16>,
    pub settings_path: String,
    pub settings_error: Option<String>,
    pub hook_exe: Option<String>,
    /// Whether the startup entry is actually in the registry.
    pub autostart_registered: bool,
    pub last_seen: BTreeMap<String, i64>,
    pub apps: Vec<(AppId, bool, String)>,
    /// Statistics scan state (without project names or paths).
    pub stats_files: usize,
    pub stats_scanned_bytes: u64,
    pub stats_total_bytes: u64,
    /// App log file and its last lines (errors and lifecycle only, home folder as `~`).
    pub log_path: String,
    pub log_tail: Vec<String>,
}

/// Time of the latest event from each source, updated by the core (`core::live`).
#[derive(Default)]
pub struct LastSeen(pub Mutex<BTreeMap<String, i64>>);

fn app_on(s: &Settings, id: AppId) -> bool {
    match id {
        AppId::ClaudeCode => s.apps.claude_code,
        AppId::Codex => s.apps.codex,
        AppId::AgentRouter => s.apps.agent_router,
        AppId::Opencode => s.apps.opencode,
        AppId::Copilot => s.apps.copilot,
        AppId::Antigravity => s.apps.antigravity,
        AppId::Cursor => s.apps.cursor,
        AppId::Grok => s.apps.grok,
        AppId::Zcode => s.apps.zcode,
    }
}

fn set_app(s: &mut Settings, id: AppId, on: bool) {
    match id {
        AppId::ClaudeCode => s.apps.claude_code = on,
        AppId::Codex => s.apps.codex = on,
        AppId::AgentRouter => s.apps.agent_router = on,
        AppId::Opencode => s.apps.opencode = on,
        AppId::Copilot => s.apps.copilot = on,
        AppId::Antigravity => s.apps.antigravity = on,
        AppId::Cursor => s.apps.cursor = on,
        AppId::Grok => s.apps.grok = on,
        AppId::Zcode => s.apps.zcode = on,
    }
}

/// First existing hook candidate: installed resources, program directory, release build next to debug.
pub fn pick_hook(candidates: &[PathBuf]) -> Option<PathBuf> { candidates.iter().find(|p| p.is_file()).cloned() }

/// Hook file name (`hook.exe` on Windows, `hook` elsewhere).
pub fn hook_file() -> &'static str { pets_core::integrations::HOOK_FILE }

pub fn hook_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let hook = hook_file();
    let mut out = Vec::new();
    if let Ok(r) = app.path().resource_dir() { out.push(r.join("resources").join(hook)); out.push(r.join(hook)); }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        out.push(dir.join(hook));
        // `target/debug/agent-pets` in development builds; hook built in release mode
        if let Some(target) = dir.parent() { out.push(target.join("release").join(hook)); }
    }
    out
}

/// Theme setting as the native window theme (title bar, and WebView2's `prefers-color-scheme` before the page applies its own);
/// `None` follows Windows.
pub fn window_theme(t: core_settings::Theme) -> Option<tauri::Theme> {
    match t {
        core_settings::Theme::System => None,
        core_settings::Theme::Light => Some(tauri::Theme::Light),
        core_settings::Theme::Dark => Some(tauri::Theme::Dark),
    }
}

/// Save and broadcast settings; `apply_effects` attaches effects (core, notifications, startup).
fn store(app: &AppHandle, new: Settings) -> Result<(), String> {
    let st = app.state::<SettingsState>();
    core_settings::save(&st.path, &new).map_err(|e| format!("{} {}: {e}", i18n::tr(st.lang(), "Nie udało się zapisać", "Could not save"), st.path.display()))?;
    let old = std::mem::replace(&mut *st.current.write().unwrap(), new.clone());
    *st.load_error.lock().unwrap() = None;
    if old.theme != new.theme { app.set_theme(window_theme(new.theme)); }
    crate::apply_effects(app, &old, &new);
    let _ = app.emit("pets://settings", &new);
    Ok(())
}

/// Settings change from Rust (e.g. position after moving the stage): save, broadcast, and apply effects as from the window.
pub fn update(app: &AppHandle, f: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut s = app.state::<SettingsState>().get();
    f(&mut s);
    store(app, s)
}

/// Settings from the window, excluding the app list (only integrations change it, to preserve installed hooks).
/// Exception: the open gate has no installation, so its toggle comes from the window.
pub fn merge_user_settings(current: &Settings, incoming: Settings) -> Settings {
    let apps = core_settings::Apps { generic: incoming.apps.generic, ..current.apps };
    Settings { apps, ..incoming }
}

#[tauri::command]
pub fn settings_get(state: tauri::State<SettingsState>) -> SettingsView {
    state.view(i18n::system())
}

#[tauri::command]
pub fn settings_set(app: AppHandle, settings: Settings) -> Result<(), String> {
    let merged = merge_user_settings(&app.state::<SettingsState>().get(), settings);
    store(&app, merged)
}

const MAX_BACKUP_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
struct Backup<'a> { agent_pets_settings: u8, version: &'a str, settings: &'a Settings }

#[derive(Deserialize)]
struct BackupInput { agent_pets_settings: u8, settings: serde_json::Value }

fn backup_json(settings: &Settings, version: &str) -> Result<Vec<u8>, serde_json::Error> {
    let mut portable = settings.clone();
    portable.notifications.muted_until = None;
    serde_json::to_vec_pretty(&Backup { agent_pets_settings: 1, version, settings: &portable })
}

fn backup_name(date: &str, exists: impl Fn(&str) -> bool) -> String {
    let base = format!("agent-pets-settings-{date}");
    let mut name = format!("{base}.json");
    let mut n = 2;
    while exists(&name) { name = format!("{base}-{n}.json"); n += 1; }
    name
}

#[cfg(windows)]
fn local_date() -> String {
    unsafe extern "system" { fn GetLocalTime(now: *mut windows::Win32::Foundation::SYSTEMTIME); }
    let mut now = windows::Win32::Foundation::SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut now); }
    format!("{:04}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay)
}

#[cfg(not(windows))]
fn local_date() -> String { pets_core::time::rfc3339(pets_core::time::now_ms())[..10].to_string() }

fn backup_dir(app: &AppHandle) -> PathBuf {
    app.path().download_dir().unwrap_or_else(|_| app.state::<SettingsState>().home.clone())
}

#[tauri::command]
pub fn settings_export(app: AppHandle) -> Result<String, String> {
    let st = app.state::<SettingsState>();
    let lang = st.lang();
    let dir = backup_dir(&app);
    let bytes = backup_json(&st.get(), env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    let date = local_date();
    for _ in 0..1000 {
        let name = backup_name(&date, |name| dir.join(name).exists());
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(e) = file.write_all(&bytes) { let _ = std::fs::remove_file(&path); return Err(e.to_string()); }
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", i18n::tr(lang, "Nie udało się wyeksportować ustawień", "Could not export settings"))),
        }
    }
    Err(i18n::tr(lang, "Nie udało się wyeksportować ustawień", "Could not export settings").into())
}

fn reveal_allowed(path: &Path, dir: &Path, home: &Path) -> bool {
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) { return false; }
    let Ok(file) = path.canonicalize() else { return false; };
    [dir, home].into_iter().filter_map(|p| p.canonicalize().ok()).any(|root| file.starts_with(root))
}

#[tauri::command]
pub fn settings_reveal(app: AppHandle, path: String) -> Result<(), String> {
    let st = app.state::<SettingsState>();
    let lang = st.lang();
    let file = Path::new(&path);
    if !file.is_file() || !reveal_allowed(file, &backup_dir(&app), &st.home) {
        return Err(i18n::tr(lang, "Nieprawidłowa ścieżka pliku", "Invalid file path").into());
    }
    Command::new("explorer.exe").arg(format!("/select,{}", file.display())).spawn()
        .map(|_| ()).map_err(|e| e.to_string())
}

fn import_settings(text: &str, current: &Settings, lang: Lang) -> Result<Settings, String> {
    if text.len() > MAX_BACKUP_BYTES { return Err(i18n::tr(lang, "Plik jest za duży.", "The file is too large.").into()); }
    let invalid = || i18n::tr(lang, "To nie jest plik ustawień Agent Pets.", "This is not an Agent Pets settings file.").to_string();
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| invalid())?;
    let object = value.as_object().ok_or_else(invalid)?;
    let inner = if object.contains_key("agent_pets_settings") {
        let backup: BackupInput = serde_json::from_value(value).map_err(|_| invalid())?;
        if backup.agent_pets_settings != 1 || !backup.settings.is_object() { return Err(invalid()); }
        backup.settings
    } else {
        if !object.keys().any(|k| ["version", "apps", "claude_plan_usage", "claude_mod", "claude_mod_pet", "claude_mod_nudges", "notifications", "pets", "power_saving", "autostart", "language", "theme", "updates", "stage", "hotkeys"].contains(&k.as_str())) { return Err(invalid()); }
        value
    };
    let mut incoming = core_settings::from_json(&serde_json::to_vec(&inner).map_err(|_| invalid())?).map_err(|_| invalid())?;
    incoming.apps = current.apps;
    incoming.claude_mod = current.claude_mod;
    incoming.claude_plan_usage = current.claude_plan_usage;
    incoming.notifications.muted_until = current.notifications.muted_until;
    Ok(incoming)
}

#[tauri::command]
pub fn settings_import(app: AppHandle, text: String) -> Result<(), String> {
    let st = app.state::<SettingsState>();
    let current = st.get();
    let imported = import_settings(&text, &current, st.lang())?;
    store(&app, imported)
}

/// The new deadline (`None` = unmuted).
pub(crate) fn mute(app: &AppHandle, choice: pets_core::mute::MuteChoice) -> Result<Option<i64>, String> {
    let until = pets_core::mute::mute_until(choice, pets_core::time::now_ms(), pets_core::time::local_midnight);
    update(app, |s| s.notifications.muted_until = until).map(|()| until)
}

/// Tray menu: mute (or unmute) toasts, saved and broadcast like a change from the settings window so every view follows.
pub fn set_mute(app: &AppHandle, choice: pets_core::mute::MuteChoice) {
    if let Err(e) = mute(app, choice) { pets_core::app_log!("mute: {e}"); }
}

/// `choice`: `off`, `hour`, `morning` (next 8:00) or `forever`; the new deadline arrives in `pets://settings`.
#[tauri::command]
pub fn notifications_mute(app: AppHandle, choice: String) -> Result<(), String> {
    mute(&app, pets_core::mute::MuteChoice::parse(&choice).ok_or_else(|| format!("unknown mute choice: {choice}"))?).map(|_| ())
}

#[tauri::command]
pub fn integrations_list(state: tauri::State<SettingsState>) -> Vec<AppRow> {
    let (s, lang) = (state.get(), state.lang());
    AppId::ALL.iter().map(|id| AppRow {
        id: *id, detected: integrations::detect(*id, &state.home, lang), status: integrations::status(*id, &state.home, lang), enabled: app_on(&s, *id),
    }).collect()
}

fn switch(app: &AppHandle, s: &mut Settings, id: AppId, on: bool) -> Result<String, String> {
    let home = app.state::<SettingsState>().home.clone();
    let lang = i18n::current(s.language);
    let mut msg = if on { integrations::enable(id, &home, pick_hook(&hook_candidates(app)).as_deref(), lang).map_err(String::from)? } else { integrations::disable(id, &home, lang).map_err(String::from)? };
    // the mod is extra: the classic hooks stay installed when it cannot be placed, the reason goes into the message
    if on && id == AppId::ClaudeCode && s.claude_mod {
        if let Err(e) = integrations::place_plugin(&home, lang) { msg = format!("{msg} {e}"); }
    }
    set_app(s, id, on);
    Ok(msg)
}

#[tauri::command]
pub fn integration_set(app: AppHandle, id: AppId, on: bool) -> Result<String, String> {
    let mut s = app.state::<SettingsState>().get();
    let msg = switch(&app, &mut s, id, on)?;
    store(&app, s)?;
    Ok(msg)
}

/// Switch for the Claude Code mod: saves the flag, then places or removes the plugin (only while Claude Code is enabled;
/// otherwise the next enabling does it).
#[tauri::command]
pub fn claude_mod_set(app: AppHandle, on: bool) -> Result<String, String> {
    let st = app.state::<SettingsState>();
    let mut s = st.get();
    let (lang, home) = (i18n::current(s.language), st.home.clone());
    s.claude_mod = on;
    let claude = s.apps.claude_code;
    store(&app, s)?;
    if !claude {
        return Ok(if on { i18n::tr(lang, "Zapisano. Mod zainstaluje się po włączeniu Claude Code.", "Saved. The mod is installed when Claude Code is turned on.") }
                  else { i18n::tr(lang, "Zapisano.", "Saved.") }.into());
    }
    if on {
        integrations::place_plugin(&home, lang).map_err(|e| format!("{} {e}", i18n::tr(lang, "Mod nie został zainstalowany.", "The mod was not installed.")))?;
        Ok(i18n::tr(lang, "Mod zainstalowany. Zadziała w nowych sesjach Claude Code.", "Mod installed. It works in new Claude Code sessions.").into())
    } else {
        integrations::remove_plugin(&home).map_err(|e| format!("{} {e}", i18n::tr(lang, "Mod nie został usunięty.", "The mod was not removed.")))?;
        Ok(i18n::tr(lang, "Mod usunięty.", "Mod removed.").into())
    }
}

/// Finish the wizard: save settings and enable or disable integrations. Return messages for the result screen.
#[tauri::command]
pub fn wizard_finish(app: AppHandle, settings: Settings) -> Vec<String> {
    let mut s = settings;
    let mut out = Vec::new();
    for id in AppId::ALL {
        let on = app_on(&s, id);
        match switch(&app, &mut s, id, on) {
            Ok(m) => out.push(m),
            Err(e) => { set_app(&mut s, id, false); out.push(e); }
        }
    }
    let autostart = s.autostart;
    match store(&app, s) {
        Ok(()) => { *app.state::<SettingsState>().first_run.lock().unwrap() = false; }
        Err(e) => out.push(e),
    }
    crate::sync_autostart(autostart);
    out
}

#[tauri::command]
pub fn diagnostics(app: AppHandle) -> Diagnostics {
    let st = app.state::<SettingsState>();
    let (s, lang) = (st.get(), st.lang());
    let settings_error = st.load_error.lock().unwrap().clone();
    let last_seen = app.state::<LastSeen>().0.lock().unwrap().clone();
    Diagnostics {
        version: app.package_info().version.to_string(),
        endpoint_port: pets_core::endpoint::Endpoint::read(&pets_core::endpoint::Endpoint::default_path()).ok().map(|e| e.port),
        settings_path: st.path.to_string_lossy().into_owned(),
        settings_error,
        hook_exe: Some(integrations::installed_hook(&st.home)).filter(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned()),
        last_seen,
        autostart_registered: crate::system::autostart_at(crate::system::RUN_KEY),
        apps: AppId::ALL.iter().map(|id| (*id, app_on(&s, *id), integrations::status(*id, &st.home, lang).detail)).collect(),
        stats_files: app.try_state::<crate::stats::StatsState>().and_then(|x| x.scanner.try_lock().ok().map(|s| s.book.files.len())).unwrap_or(0),
        stats_scanned_bytes: app.try_state::<crate::stats::StatsState>().map(|x| x.progress.lock().unwrap().scanned).unwrap_or(0),
        stats_total_bytes: app.try_state::<crate::stats::StatsState>().map(|x| x.progress.lock().unwrap().total).unwrap_or(0),
        log_path: pets_core::applog::path(&st.home).to_string_lossy().into_owned(),
        log_tail: pets_core::applog::tail(pets_core::applog::REPORT_LINES),
    }
}

/// New bug report on GitHub, the form's version field filled in (`.github/ISSUE_TEMPLATE/bug_report.yml`).
pub fn issue_url(version: &str) -> String {
    let v: String = version.bytes().map(|b| match b {
        b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'.' | b'-' | b'_' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect();
    format!("https://github.com/Marczelloo/agent-pets/issues/new?template=bug_report.yml&version={v}")
}

/// Open the bug form in the default browser (settings → Diagnostics, tray menu).
pub fn report_problem(app: &AppHandle) {
    let url = issue_url(&app.package_info().version.to_string());
    if !crate::jump::exec::open_url(&url) { pets_core::app_log!("report a problem: cannot open the browser"); }
}

#[tauri::command]
pub fn report_problem_open(app: AppHandle) { report_problem(&app); }

/// Settings window title (title bar, taskbar button, Alt+Tab).
pub fn window_title(lang: Lang) -> &'static str { i18n::tr(lang, "Agent Pets: ustawienia", "Agent Pets: settings") }

/// Window size: natural (all content without scrolling), but no larger than the screen work area minus a margin.
pub fn fit_size(desired: (f64, f64), work: Option<(f64, f64)>) -> (f64, f64) {
    const MARGIN: f64 = 32.0;
    match work {
        Some((w, h)) => (desired.0.min(w - MARGIN), desired.1.min(h - MARGIN)),
        None => desired,
    }
}

/// Primary monitor work area in logical pixels (excluding the taskbar).
pub fn work_area(app: &AppHandle) -> Option<(f64, f64)> {
    let m = app.primary_monitor().ok().flatten()?;
    let (s, a) = (m.scale_factor(), m.work_area());
    Some((a.size.width as f64 / s, a.size.height as f64 / s))
}

/// Settings window: 1100×920 fits "Appearance" (7 styles in a row, music toggle) and most tabs without scrolling.
pub const WINDOW: (f64, f64) = (1100.0, 920.0);

/// Open the settings window (or wizard on first launch); subsequent calls only show it.
pub fn open(app: &AppHandle) { open_at(app, None) }

/// Settings window on a specified tab (e.g. "stage" from the stage menu).
pub fn open_tab(app: &AppHandle, tab: &str) { open_at(app, Some(tab)) }

#[tauri::command]
pub fn settings_open_tab(app: AppHandle, tab: String) { crate::panel::hide(&app); open_tab(&app, &tab); }

fn open_at(app: &AppHandle, tab: Option<&str>) {
    if let Some(w) = app.get_webview_window("settings") {
        if let Some(t) = tab { let _ = app.emit_to("settings", "settings://tab", t); }
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // Building the window in a synchronous command or event handler (tray, second instance) deadlocks
    // on Windows (WebviewWindowBuilder documentation), so build it on a separate thread.
    let app = app.clone();
    let tab = tab.map(str::to_string);
    std::thread::spawn(move || {
        let url = tab.map(|t| format!("settings.html#{t}")).unwrap_or_else(|| "settings.html".into());
        let (w, h) = fit_size(WINDOW, work_area(&app));
        let theme = window_theme(app.state::<SettingsState>().get().theme);
        let _ = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App(url.into()))
            .title(window_title(app.state::<SettingsState>().lang())).inner_size(w, h).min_inner_size(620.0, 460.0).theme(theme).center().build();
    });
}

#[tauri::command]
pub fn settings_open(app: AppHandle) { crate::panel::hide(&app); open(&app); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_wraps_settings_and_drops_the_mute_deadline() {
        let mut s = Settings::default();
        s.notifications.muted_until = Some(i64::MAX);
        s.hotkeys.jump = Some("Ctrl+Alt+J".into());
        let v: serde_json::Value = serde_json::from_slice(&backup_json(&s, "0.17.0").unwrap()).unwrap();
        assert_eq!(v["agent_pets_settings"], 1);
        assert_eq!(v["version"], "0.17.0");
        assert!(v["settings"]["notifications"].get("muted_until").is_none());
        assert_eq!(v["settings"]["hotkeys"]["jump"], "Ctrl+Alt+J");
        assert_eq!(s.notifications.muted_until, Some(i64::MAX));
    }

    #[test]
    fn import_keeps_machine_specific_settings_and_accepts_bare_files() {
        let mut current = Settings::default();
        current.apps.codex = false;
        current.claude_mod = false;
        current.claude_plan_usage = true;
        current.notifications.muted_until = Some(123);
        let text = r#"{"agent_pets_settings":1,"version":"0.17.0","settings":{"theme":"dark","apps":{"codex":true},"claude_mod":true,"claude_plan_usage":false,"notifications":{"needs_you":false,"muted_until":999},"hotkeys":{"jump":null},"future_field":17}}"#;
        let s = import_settings(text, &current, Lang::En).unwrap();
        assert_eq!(s.theme, core_settings::Theme::Dark);
        assert!(!s.apps.codex && !s.claude_mod && s.claude_plan_usage);
        assert_eq!(s.notifications.muted_until, Some(123));
        assert!(!s.notifications.needs_you);
        assert_eq!(s.hotkeys.jump, None);
        assert_eq!(s.extra["future_field"], 17);
        let bare = import_settings(r#"{"theme":"light","unknown":true}"#, &current, Lang::En).unwrap();
        assert_eq!(bare.theme, core_settings::Theme::Light);
        assert_eq!(bare.notifications, core_settings::Notifications { muted_until: Some(123), ..core_settings::Notifications::default() });
        assert_eq!(bare.hotkeys, core_settings::Hotkeys::default());
    }

    #[test]
    fn bad_imports_report_localized_errors() {
        let current = Settings::default();
        for bad in ["garbage", "[]", "{}", r#"{"agent_pets_settings":2,"settings":{}}"#, r#"{"agent_pets_settings":1,"settings":[]}"#] {
            assert_eq!(import_settings(bad, &current, Lang::En).unwrap_err(), "This is not an Agent Pets settings file.");
        }
        assert_eq!(import_settings(&"x".repeat(MAX_BACKUP_BYTES + 1), &current, Lang::Pl).unwrap_err(), "Plik jest za duży.");
    }

    #[test]
    fn export_name_never_reuses_an_existing_file() {
        let used = ["agent-pets-settings-2026-10-06.json", "agent-pets-settings-2026-10-06-2.json"];
        assert_eq!(backup_name("2026-10-06", |name| used.contains(&name)), "agent-pets-settings-2026-10-06-3.json");
    }

    #[test]
    fn reveal_only_accepts_json_inside_the_export_roots() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let downloads = home.join("Downloads");
        let outside = d.path().join("elsewhere");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let good = downloads.join("backup.json");
        let bad = outside.join("backup.json");
        std::fs::write(&good, "{}").unwrap();
        std::fs::write(&bad, "{}").unwrap();
        assert!(reveal_allowed(&good, &downloads, &home));
        assert!(!reveal_allowed(&bad, &downloads, &home));
        assert!(!reveal_allowed(&good.with_extension("txt"), &downloads, &home));
    }

    #[test]
    fn the_theme_setting_maps_to_the_native_window_theme() {
        assert_eq!(window_theme(core_settings::Theme::System), None);
        assert_eq!(window_theme(core_settings::Theme::Light), Some(tauri::Theme::Light));
        assert_eq!(window_theme(core_settings::Theme::Dark), Some(tauri::Theme::Dark));
    }

    #[test]
    fn the_view_reports_the_windows_language_not_the_resolved_one() {
        let d = tempfile::tempdir().unwrap();
        let p = core_settings::path(d.path());
        let s = Settings { language: pets_core::settings::Language::En, ..Settings::default() };
        core_settings::save(&p, &s).unwrap();
        let v = SettingsState::load(d.path().to_path_buf()).view(Lang::Pl);
        assert_eq!((v.lang, v.system_lang), (Lang::En, Lang::Pl));
    }

    #[test]
    fn the_window_title_follows_the_language() {
        assert_eq!(window_title(Lang::En), "Agent Pets: settings");
        assert_eq!(window_title(Lang::Pl), "Agent Pets: ustawienia");
    }

    #[test]
    fn settings_from_the_window_never_change_the_apps() {
        // only integration_set / wizard_finish change apps; the UI may have a stale list
        let mut current = Settings::default();
        current.apps.claude_code = false;
        let incoming = Settings { autostart: false, ..Settings::default() };
        let merged = merge_user_settings(&current, incoming);
        assert!(!merged.apps.claude_code);
        assert!(!merged.autostart);
    }

    #[test]
    fn the_claude_mod_switch_from_the_window_is_kept() {
        let incoming = Settings { claude_mod: false, ..Settings::default() };
        assert!(!merge_user_settings(&Settings::default(), incoming).claude_mod);
    }

    #[test]
    fn the_mod_pet_and_nudges_switches_from_the_window_are_kept() {
        let incoming = Settings { claude_mod_pet: true, claude_mod_nudges: false, ..Settings::default() };
        let merged = merge_user_settings(&Settings::default(), incoming);
        assert!(merged.claude_mod_pet && !merged.claude_mod_nudges);
    }

    #[test]
    fn the_door_switch_from_the_window_is_kept() {
        // the open gate has no installation: its window toggle is the only way to close it
        let mut current = Settings::default();
        current.apps.claude_code = false;
        let mut incoming = Settings::default();
        incoming.apps.generic = false;
        let merged = merge_user_settings(&current, incoming);
        assert!(!merged.apps.generic, "gate closed");
        assert!(!merged.apps.claude_code, "other apps unchanged");
    }

    #[test]
    fn hook_comes_from_the_first_place_that_has_it() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, c) = (d.path().join("a").join("hook.exe"), d.path().join("b.exe"), d.path().join("c.exe"));
        std::fs::write(&b, b"x").unwrap();
        std::fs::write(&c, b"y").unwrap();
        assert_eq!(pick_hook(&[a.clone(), b.clone(), c]), Some(b));
        assert_eq!(pick_hook(&[a]), None);
    }

    #[test]
    fn windows_open_at_their_natural_size_but_never_larger_than_the_screen() {
        assert_eq!(fit_size((1100.0, 860.0), Some((2560.0, 1392.0))), (1100.0, 860.0));
        assert_eq!(fit_size((1100.0, 860.0), Some((1366.0, 728.0))), (1100.0, 696.0));
        assert_eq!(fit_size((1100.0, 860.0), Some((1024.0, 600.0))), (992.0, 568.0));
        assert_eq!(fit_size((800.0, 720.0), None), (800.0, 720.0));
    }

    #[test]
    fn a_problem_report_opens_the_bug_form_with_the_version_filled_in() {
        assert_eq!(issue_url("0.13.0"), "https://github.com/Marczelloo/agent-pets/issues/new?template=bug_report.yml&version=0.13.0");
        assert_eq!(issue_url("1.0.0-beta.1+b7"), "https://github.com/Marczelloo/agent-pets/issues/new?template=bug_report.yml&version=1.0.0-beta.1%2Bb7");
        assert!(issue_url("0.1&x=<y>").ends_with("&version=0.1%26x%3D%3Cy%3E"));
    }

    #[test]
    fn diagnostics_carry_no_tokens_or_session_titles() {
        let v = serde_json::to_value(Diagnostics::default()).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert!(keys.iter().all(|k| !k.contains("token") && !k.contains("title")), "{keys:?}");
        assert!(keys.iter().all(|k| !k.contains("action") && !k.contains("question") && !k.contains("session")), "0.8: no action text {keys:?}");
        for k in ["stats_files", "stats_scanned_bytes", "stats_total_bytes"] { assert!(keys.contains(&k), "0.9: statistics scan state {keys:?}"); }
    }
}
