mod antigravity_usage;
mod appstate;
mod bubbles;
mod core;
mod hotkeys;
#[cfg(target_os = "linux")]
mod hotkeys_portal;
mod jump;
mod media;
mod mod_actions;
mod notify;
mod panel;
mod problems;
mod settings;
mod shell;

/// Startup helpers for `main` (single-threaded environment access); see `shell::taskbar_linux`.
#[cfg(target_os = "linux")]
pub use shell::init_hyprland_socket;
mod stats;
mod system;
mod tooltip;
mod tray;
mod updater;
mod usage;
mod version;

use tauri::{Manager, RunEvent};

#[tauri::command]
fn snapshot(state: tauri::State<core::Shared>) -> core::Snapshot {
    state.lock().unwrap().clone()
}

#[tauri::command]
fn stage_hello(app: tauri::AppHandle, shell: tauri::State<shell::Shell>, tip: tauri::State<tooltip::Tooltip>) {
    tip.hide(&app);
    shell.hello();
}

#[tauri::command]
fn stage_set_width(width: f64, shell: tauri::State<shell::Shell>) { shell.set_width(width); }

/// Latest stage layout (mode, note about left-side icons) for a settings window opened later.
#[tauri::command]
fn stage_layout(shell: tauri::State<shell::Shell>) -> Option<shell::Layout> { shell.layout() }

/// Monitors available in the "Taskbar" tab.
#[tauri::command]
fn monitors_list() -> Vec<shell::placement::MonitorInfo> { shell::monitors() }

/// Floating window: clicks over empty space pass through to windows beneath it.
#[tauri::command]
fn stage_passthrough(on: bool, shell: tauri::State<shell::Shell>) { shell.passthrough(on); }

/// "Move" (stage menu, "Taskbar" tab): drag the stage in the taskbar; Enter or clicking outside saves, Esc cancels.
#[tauri::command]
fn stage_move(shell: tauri::State<shell::Shell>) { shell.start_move(); }

/// "Jump" to a session. Read the Claude registry now because desktop session `hostSessionId` is absent from the snapshot.
#[tauri::command]
fn jump(app: tauri::AppHandle, session_id: String) -> jump::JumpResult {
    let r = jump_to(&app, &session_id);
    if !r.needs_attention() { panel::hide(&app); }
    r
}

/// Shared by the panel command and the "Jump" button in a toast.
pub fn jump_to(app: &tauri::AppHandle, session_id: &str) -> jump::JumpResult {
    let snap = app.state::<core::Shared>().lock().unwrap().clone();
    let lang = app.state::<settings::SettingsState>().lang();
    let Some(s) = snap.sessions.iter().find(|s| s.id == session_id) else {
        return jump::JumpResult { method: "none".into(), detail: pets_core::i18n::tr(lang, "Sesja już nie istnieje", "The session no longer exists").into() };
    };
    let home = settings::home();
    let reg = jump::registry::find(&home, session_id);
    jump::exec::run(&jump::plan(&jump::Target::from(s, reg.as_ref())), lang)
}

/// Send a hide command to the core thread and wait (up to 2 s) for hidden IDs.
fn ask_core(app: &tauri::AppHandle, make: impl FnOnce(std::sync::mpsc::Sender<Vec<String>>) -> core::CoreMsg) -> Vec<String> {
    let tx = app.state::<core::Control>().0.lock().unwrap().clone();
    core::ask(&tx, make)
}

pub fn dismiss(app: &tauri::AppHandle, ids: Vec<String>) -> Vec<String> { ask_core(app, |tx| core::CoreMsg::Dismiss(ids, tx)) }
pub fn dismiss_inactive(app: &tauri::AppHandle) -> Vec<String> { ask_core(app, core::CoreMsg::DismissInactive) }

/// Hide sessions (✕ in panel, "Remove from taskbar"); they return on new activity.
/// `async`: waits for the core cycle (up to 250 ms), so it does not block the main thread.
#[tauri::command(async)]
fn session_dismiss(app: tauri::AppHandle, ids: Vec<String>) -> Vec<String> { dismiss(&app, ids) }

/// "Remove inactive": hide idle, ready, sleeping, and finished sessions.
#[tauri::command(async)]
fn sessions_dismiss_inactive(app: tauri::AppHandle) -> Vec<String> { dismiss_inactive(&app) }

/// Right click on the stage: `target` is a pet ID or nothing (badge, limits, empty background).
#[tauri::command]
fn stage_menu(app: tauri::AppHandle, target: Option<String>, x: f64, y: f64) {
    let t = target.map(shell::menu::Target::Pet).unwrap_or(shell::menu::Target::Other);
    if let Err(e) = shell::menu::show(&app, t, x, y) { pets_core::app_log!("stage menu: {e}"); }
}

/// "Undo" after hiding.
#[tauri::command]
fn session_undismiss(app: tauri::AppHandle, ids: Vec<String>) {
    let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::Undismiss(ids));
}

/// Own name for a session from the ⋯ menu; empty or none goes back to the automatic title.
#[tauri::command]
fn session_rename(app: tauri::AppHandle, id: String, name: Option<String>) {
    let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::Rename(id, name));
}

/// Pin a session to the top of the panel (and keep it through "Remove inactive").
#[tauri::command]
fn session_pin(app: tauri::AppHandle, id: String, pinned: bool) {
    let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::Pin(id, pinned));
}

/// The mod's two switches, as the board carries them.
pub fn mod_prefs_of(s: &pets_core::settings::Settings) -> pets_core::mod_state::ModPrefs {
    pets_core::mod_state::ModPrefs { pet: s.claude_mod_pet, nudges: s.claude_mod_nudges }
}

/// Effects of changing settings. Notifications and Anthropic-limit consent are read continuously in their threads.
pub fn apply_effects(app: &tauri::AppHandle, old: &pets_core::settings::Settings, new: &pets_core::settings::Settings) {
    if old.apps != new.apps { let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::Apps(new.apps)); }
    let (old_prefs, new_prefs) = (mod_prefs_of(old), mod_prefs_of(new));
    if old_prefs != new_prefs { let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::ModPrefs(new_prefs)); }
    // open gate: `hook.exe report` at the fixed path `~/.agent-pets/hook.exe` (spec 8)
    if new.apps.generic && !old.apps.generic {
        let st = app.state::<settings::SettingsState>();
        let _ = pets_core::integrations::place_hook(&st.home, settings::pick_hook(&settings::hook_candidates(app)).as_deref(), st.lang());
    }
    if old.autostart != new.autostart { sync_autostart(new.autostart); }
    if old.stage != new.stage { app.state::<shell::Shell>().settings_changed(); }
    if old.updates != new.updates { updater::mode_changed(app, new.updates); }
    if old.hotkeys != new.hotkeys { hotkeys::apply(app); }
    if old.power_saving != new.power_saving { system::refresh_power(app); }
    if old.notifications.muted_until != new.notifications.muted_until { tray::refresh(app); }
    if old.language != new.language {
        let lang = pets_core::i18n::current(new.language);
        let _ = app.state::<core::Control>().0.lock().unwrap().send(core::CoreMsg::Lang(lang));
        tray::refresh(app);
        if let Some(w) = app.get_webview_window("settings") { let _ = w.set_title(settings::window_title(lang)); }
    }
}

/// Launching again from Start opens settings; Windows autostart (if the app is already running because the user
/// launched it manually) opens nothing.
fn second_launch_opens_settings(args: &[String]) -> bool { !args.iter().any(|a| a == system::AUTOSTART_ARG) }

/// Match the startup entry to the setting (compare with the registry, not previous settings).
/// Development builds do not register themselves (the entry would point to `target\debug`).
pub fn sync_autostart(on: bool) {
    if cfg!(debug_assertions) { return; }
    let Some(cmd) = system::current_autostart_command() else { return };
    if let Some(v) = system::autostart_action(on, system::autostart_value(system::RUN_KEY).as_deref(), &cmd) { let _ = system::set_autostart(v); }
}

/// At startup: a `statusLine` still pointing at our retired pass-through gets the user's own statusline back
/// (limits now come from the Claude mod). Idempotent.
fn migrate_statusline(app: &tauri::AppHandle) {
    let home = app.state::<settings::SettingsState>().home.clone();
    if pets_core::integrations::migrate_statusline(&home) { pets_core::applog::write("restored the user's statusLine in ~/.claude/settings.json"); }
}

/// At startup: enabled Claude Code without hooks or with an old `hook.exe` (after an update) gets them again;
/// enabled opencode gets this version's plugin; the open gate keeps `hook.exe report` at a fixed location.
fn repair_integrations(app: &tauri::AppHandle) {
    use pets_core::integrations::{self, AppId};
    let st = app.state::<settings::SettingsState>();
    let apps = st.get().apps;
    let src = settings::pick_hook(&settings::hook_candidates(app));
    let mut errors = Vec::new();
    if apps.claude_code && integrations::claude_needs_repair(&st.home, src.as_deref()) {
        problems::collect_repair(&mut errors, "Claude Code", integrations::enable(AppId::ClaudeCode, &st.home, src.as_deref(), st.lang()));
    }
    // the mod: placed again after an update (new version) or when files vanished; a folder that is not ours stays
    if apps.claude_code && st.get().claude_mod && integrations::plugin_needs_repair(&st.home) {
        problems::collect_repair(&mut errors, "Claude Code", integrations::place_plugin(&st.home, st.lang()));
    }
    // only our file: another `agent-pets.js` stays, and enable returns an error without changes
    if apps.opencode { problems::collect_repair(&mut errors, "opencode", integrations::enable(AppId::Opencode, &st.home, None, st.lang())); }
    // refresh commands and `hook.exe` after an update; another file or key stays (enable returns an error without changes)
    if apps.copilot { problems::collect_repair(&mut errors, "GitHub Copilot", integrations::enable(AppId::Copilot, &st.home, src.as_deref(), st.lang())); }
    if apps.antigravity { problems::collect_repair(&mut errors, "Antigravity", integrations::enable(AppId::Antigravity, &st.home, src.as_deref(), st.lang())); }
    if apps.cursor { problems::collect_repair(&mut errors, "Cursor", integrations::enable(AppId::Cursor, &st.home, src.as_deref(), st.lang())); }
    if apps.grok { problems::collect_repair(&mut errors, "Grok Build", integrations::enable(AppId::Grok, &st.home, src.as_deref(), st.lang())); }
    if apps.zcode { problems::collect_repair(&mut errors, "ZCode", integrations::enable(AppId::Zcode, &st.home, src.as_deref(), st.lang())); }
    if apps.generic { problems::collect_repair(&mut errors, "Agent Pets", integrations::place_hook(&st.home, src.as_deref(), st.lang())); }
    if !errors.is_empty() { problems::report(app, "repair", problems::repair_title(st.lang()), &problems::repair_body(&errors)); }
}

/// `agent-pets.exe --uninstall-integrations [--remove-data]` (NSIS uninstaller): clean up and exit without windows.
pub fn uninstall_cli(args: &[String]) -> Option<i32> {
    if !args.iter().any(|a| a == "--uninstall-integrations") { return None; }
    let remove_data = args.iter().any(|a| a == "--remove-data");
    let home = settings::home();
    let lang = pets_core::i18n::current(pets_core::settings::load(&pets_core::settings::path(&home)).settings.language);
    for step in pets_core::integrations::uninstall_all(&home, remove_data, lang) { println!("{step}"); }
    let _ = system::set_autostart(false);
    system::remove_aumid();
    Some(0)
}

/// Release builds have no console: errors and panics also go to `~/.agent-pets/agent-pets.log`.
fn start_log() {
    pets_core::applog::init(&settings::home());
    pets_core::app_log!("Agent Pets {} started", env!("CARGO_PKG_VERSION"));
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        pets_core::app_log!("panic{at}: {msg}");
        default(info);
    }));
}

pub fn run() {
    start_log();
    tauri::Builder::default()
        // must be the first plugin: a second launch opens settings instead of starting another core
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| if second_launch_opens_settings(&args) { settings::open(app) }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        // shortcuts are registered from Rust (`hotkeys`), so no webview permission is granted
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let prefs = settings::SettingsState::load(settings::home());
            let first_run = *prefs.first_run.lock().unwrap();
            app.set_theme(settings::window_theme(prefs.get().theme));
            app.manage(prefs);
            app.manage(settings::LastSeen::default());
            let shared: core::Shared = Default::default();
            app.manage(shared.clone());
            app.manage(shell::Shell::start(app.handle())?);
            app.manage(tooltip::Tooltip::default());
            tooltip::build(app.handle())?;
            app.manage(bubbles::Bubbles::default());
            bubbles::build(app.handle())?;
            app.manage(panel::Panel::default());
            panel::build(app.handle())?;
            if let Err(e) = tray::build(app.handle()) {
                // Linux: the tray needs an appindicator library; the app stays usable without the icon
                if cfg!(target_os = "linux") { pets_core::app_log!("tray icon: {e}"); } else { return Err(e.into()); }
            }
            #[cfg(target_os = "linux")]
            tray::hint_if_hidden(app.handle());
            app.manage(notify::center::Center::load(settings::home()));
            app.manage(problems::Problems::default());
            let snaps = notify::start(app.handle().clone());
            let (core_tx, core_rx) = std::sync::mpsc::channel();
            app.manage(core::Control(std::sync::Mutex::new(core_tx)));
            core::spawn(app.handle().clone(), shared, core::Mode::from_env(), Some(snaps), core_rx);
            app.manage(stats::StatsState::load(&settings::home()));
            stats::spawn(app.handle().clone());
            app.manage(system::Power::default());
            system::watch_power(app.handle().clone());
            app.manage(media::MediaState::default());
            media::watch_media(app.handle().clone());
            app.manage(updater::Updater::default());
            app.manage(hotkeys::HotkeyState::default());
            hotkeys::apply(app.handle());
            app.manage(shell::menu::MenuTarget::default());
            app.on_menu_event(|app, e| shell::menu::on_event(app, e.id().as_ref()));
            updater::start(app.handle().clone());
            migrate_statusline(app.handle());
            if !first_run {
                sync_autostart(app.state::<settings::SettingsState>().get().autostart);
                repair_integrations(app.handle());
            }
            if first_run { settings::open(app.handle()); }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot, stage_hello, stage_set_width, stage_move, stage_menu, stage_passthrough, monitors_list, stage_layout, jump, panel::panel_open, panel::panel_hide,
            tooltip::tooltip_show, tooltip::tooltip_size, tooltip::tooltip_hide,
            bubbles::stage_pets, bubbles::bubbles_place, bubbles::bubbles_hide, bubbles::bubbles_hits,
            settings::settings_get, settings::settings_set, settings::settings_export, settings::settings_import, settings::settings_reveal,
            settings::integrations_list, settings::integration_set, settings::claude_mod_set, settings::notifications_mute,
            settings::wizard_finish, settings::diagnostics, settings::settings_open, settings::settings_open_tab, settings::report_problem_open, system::power_get, media::media_get,
            notify::center::notifications_list, notify::center::notifications_read, notify::center::notification_remove, notify::center::notifications_clear,
            updater::update_status, updater::update_check, updater::update_install, hotkeys::hotkeys_status,
            session_dismiss, sessions_dismiss_inactive, session_undismiss, session_rename, session_pin,
            stats::stats_open, stats::stats_view, stats::stats_progress
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application")
        .run(|app, event| {
            // logged so a missing line before the next "started" tells a crash (or a kill) from a quit
            if let RunEvent::Exit = event { pets_core::app_log!("Agent Pets exiting"); stats::save_now(app); }
            // The stage window dies with the taskbar when Explorer restarts; the app should keep running.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() { api.prevent_exit(); }
            }
        });
}

#[cfg(test)]
mod launch_tests {
    use super::second_launch_opens_settings;

    #[test]
    fn a_second_launch_from_autostart_opens_nothing() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(second_launch_opens_settings(&a(&["agent-pets.exe"])), "Start menu opens settings, as before");
        assert!(!second_launch_opens_settings(&a(&["agent-pets.exe", "--autostart"])));
    }
}
