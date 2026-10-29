//! Updates from GitHub releases (`tauri-plugin-updater`): check 15 s after launch and every 6 h,
//! notification or installation at a quiet moment (`pets_core::update_gate`). The plugin verifies the signature.
use pets_core::i18n::{tr, Lang};
use pets_core::settings::Updates;
use pets_core::update_gate::{busy, Calm};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const ENDPOINT: &str = "https://github.com/Marczelloo/agent-pets/releases/latest/download/latest.json";
const FIRST_CHECK: Duration = Duration::from_secs(15);
const EVERY: Duration = Duration::from_secs(6 * 3600);
/// After a network failure (often right after waking a laptop) check again sooner than `EVERY`.
const RETRY: Duration = Duration::from_secs(15 * 60);
const GATE_TICK: Duration = Duration::from_secs(5);

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    Checking,
    Latest,
    Available { version: String, notes: Option<String> },
    Downloading { version: String, pct: Option<u8> },
    /// Downloaded and verified, waiting for a quiet moment (automatic mode).
    Ready { version: String },
    /// `verify`: update problem (signature, installation), also visible in the panel;
    /// otherwise a manual-check error, visible only in settings.
    Error { message: String, verify: bool },
}

/// "Notify" mode reports each version once (even across app restarts).
pub fn should_notify(mode: Updates, notified: Option<&str>, version: &str) -> bool {
    mode == Updates::Notify && notified != Some(version)
}

/// Version for the "Updated to …" toast after launch; first launch has no toast at all.
pub fn updated_toast(seen: Option<&str>, current: &str) -> Option<String> {
    seen.filter(|s| *s != current).map(|_| current.to_string())
}

/// Network issue (offline, interrupted download): only log it and retry on schedule.
pub fn is_network(e: &tauri_plugin_updater::Error) -> bool {
    matches!(e, tauri_plugin_updater::Error::Network(_) | tauri_plugin_updater::Error::Reqwest(_))
}

/// Wait before the next scheduled check.
pub fn next_check(offline: bool) -> Duration { if offline { RETRY } else { EVERY } }

pub fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    total.filter(|t| *t > 0).map(|t| ((done.min(t) * 100) / t) as u8)
}

/// Found update and, after download, its verified installer.
type Found = Option<(Update, Option<Vec<u8>>)>;

#[derive(Default)]
pub struct Updater {
    status: Mutex<Option<UpdateStatus>>,
    found: Mutex<Found>,
    /// One check or download at a time.
    working: AtomicBool,
    /// The last check or download failed on the network.
    offline: AtomicBool,
}

/// Release `working` on scope exit.
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }

impl Updater {
    pub fn status(&self) -> UpdateStatus { self.status.lock().unwrap().clone().unwrap_or(UpdateStatus::Idle) }
    fn take(&self) -> Option<Busy<'_>> { (!self.working.swap(true, Ordering::AcqRel)).then_some(Busy(&self.working)) }
    /// Wait (up to 10 min) for a check or download to finish: a second "Install" does not report an error.
    async fn acquire(&self) -> Option<Busy<'_>> {
        for _ in 0..3000 {
            if let Some(b) = self.take() { return Some(b); }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        None
    }
    fn found(&self) -> Found { self.found.lock().unwrap().clone() }
    fn downloaded(&self) -> bool { matches!(&*self.found.lock().unwrap(), Some((_, Some(_)))) }
}

fn set(app: &AppHandle, s: UpdateStatus) {
    *app.state::<Updater>().status.lock().unwrap() = Some(s.clone());
    let _ = app.emit("pets://update", s);
}

fn lang(app: &AppHandle) -> Lang { app.state::<crate::settings::SettingsState>().lang() }
fn mode(app: &AppHandle) -> Updates { app.state::<crate::settings::SettingsState>().get().updates }

fn verify_failed(app: &AppHandle) -> String {
    tr(lang(app), "Nie udało się zweryfikować aktualizacji", "Could not verify the update").into()
}

/// `latest.json` URL. `AGENT_PETS_UPDATE_URL` overrides it only in development or test builds
/// (`--features update-test`, docs/building.md); release builds always query GitHub. The signature is always verified.
fn endpoint_from(env: Option<String>, allow_override: bool) -> String {
    env.filter(|_| allow_override).unwrap_or_else(|| ENDPOINT.into())
}

fn endpoint() -> String {
    endpoint_from(std::env::var("AGENT_PETS_UPDATE_URL").ok(), cfg!(any(debug_assertions, feature = "update-test")))
}

async fn find(app: &AppHandle) -> Result<Option<Update>, (String, bool)> {
    let url = endpoint().parse().map_err(|e| (format!("{e}"), false))?;
    let up = app.updater_builder().endpoints(vec![url]).map_err(|e| (e.to_string(), false))?.build().map_err(|e| (e.to_string(), false))?;
    match up.check().await {
        Ok(u) => Ok(u),
        // Linux: releases carry no updater package for it yet, so a missing target means up to date
        Err(tauri_plugin_updater::Error::TargetNotFound(_) | tauri_plugin_updater::Error::TargetsNotFound(_)) if cfg!(target_os = "linux") => Ok(None),
        Err(e) => Err((e.to_string(), is_network(&e))),
    }
}

/// Check the version. `manual`: "Check now" button (result always visible, works even when `off`).
pub async fn check(app: &AppHandle, manual: bool) -> UpdateStatus {
    let u = app.state::<Updater>();
    {
        let Some(_busy) = u.take() else { return u.status() };
        if manual { set(app, UpdateStatus::Checking); }
        let r = find(app).await;
        u.offline.store(matches!(r, Err((_, true))), Ordering::Release);
        match r {
            Ok(None) => set(app, UpdateStatus::Latest),
            Ok(Some(up)) => {
                let version = up.version.clone();
                if matches!(u.found(), Some((f, Some(_))) if f.version == version) {
                    set(app, UpdateStatus::Ready { version });
                    return u.status();
                }
                set(app, UpdateStatus::Available { version: version.clone(), notes: up.body.clone() });
                *u.found.lock().unwrap() = Some((up.clone(), None));
                let home = app.state::<crate::settings::SettingsState>().home.clone();
                let mut st = crate::appstate::load(&home);
                if should_notify(mode(app), st.notified_version.as_deref(), &version) {
                    announce(app, &version, up.body.as_deref());
                    st.notified_version = Some(version);
                    let _ = crate::appstate::save(&home, &st);
                }
            }
            Err((e, network)) => {
                pets_core::app_log!("update check: {e}");
                if manual { set(app, UpdateStatus::Error { message: tr(lang(app), "Błąd sprawdzania aktualizacji", "Could not check for updates").into(), verify: false }); }
                else if !network { crate::problems::report(app, "update", tr(lang(app), "Aktualizacja nie powiodła się", "Update failed"), &e); }
            }
        }
    }
    if mode(app) == Updates::Auto && matches!(u.status(), UpdateStatus::Available { .. }) { download(app).await; }
    u.status()
}

/// Download and verify the found version's installer; keep it in memory as `Ready`.
async fn download(app: &AppHandle) -> bool {
    let u = app.state::<Updater>();
    let Some(_busy) = u.acquire().await else { return u.downloaded() };
    let Some((up, None)) = u.found() else { return u.downloaded() };
    let version = up.version.clone();
    set(app, UpdateStatus::Downloading { version: version.clone(), pct: Some(0) });
    let (a, v) = (app.clone(), version.clone());
    let (mut got, mut last) = (0u64, Some(0u8));
    let r = up.download(move |chunk, total| {
        got += chunk as u64;
        let p = percent(got, total);
        if p != last { last = p; set(&a, UpdateStatus::Downloading { version: v.clone(), pct: p }); }
    }, || {}).await;
    match r {
        Ok(bytes) => {
            *u.found.lock().unwrap() = Some((up, Some(bytes)));
            set(app, UpdateStatus::Ready { version });
            true
        }
        Err(e) if is_network(&e) => {
            // interrupted download is not a bad signature: the version remains pending; retry on schedule
            pets_core::app_log!("update download (network): {e}");
            u.offline.store(true, Ordering::Release);
            set(app, UpdateStatus::Available { version: up.version.clone(), notes: up.body.clone() });
            false
        }
        Err(e) => {
            pets_core::app_log!("update download: {e}");
            *u.found.lock().unwrap() = None;
            set(app, UpdateStatus::Error { message: verify_failed(app), verify: true });
            crate::problems::report(app, "update", tr(lang(app), "Aktualizacja nie powiodła się", "Update failed"), &e.to_string());
            false
        }
    }
}

/// Install the found version (download first if needed). The app exits and the installer restarts it.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let u = app.state::<Updater>();
    if u.found().is_none() { check(app, true).await; }
    if !u.downloaded() && !download(app).await { return Err(verify_failed(app)); }
    let Some((up, Some(bytes))) = u.found() else { return Err(verify_failed(app)) };
    up.install(bytes).map_err(|e| {
        pets_core::app_log!("update install: {e}");
        // no longer "ready": the quiet gate will not retry every 2 min; retry after the next check
        *u.found.lock().unwrap() = None;
        let msg = tr(lang(app), "Nie udało się zainstalować aktualizacji", "Could not install the update").to_string();
        set(app, UpdateStatus::Error { message: msg.clone(), verify: true });
        msg
    })
}

fn announce(app: &AppHandle, version: &str, notes: Option<&str>) {
    let l = lang(app);
    let title = format!("{} {version}", tr(l, "Dostępna wersja", "Version available:"));
    let body = notes.and_then(|n| n.lines().map(str::trim).find(|x| !x.is_empty())).unwrap_or("").to_string();
    let a = app.clone();
    crate::notify::show_update(app, crate::notify::center::Kind::Update, &title, &body, Some(tr(l, "Zainstaluj", "Install")), move |action| {
        if action.as_deref() == Some("install") {
            let a2 = a.clone();
            tauri::async_runtime::spawn(async move { let _ = install(&a2).await; });
        } else {
            crate::panel::open(&a, None);
        }
    });
}

/// Mode change in settings: switching to "automatic" downloads a pending version immediately,
/// rather than waiting for the next check several hours later.
pub fn mode_changed(app: &AppHandle, mode: Updates) {
    if mode != Updates::Auto || !matches!(app.state::<Updater>().status(), UpdateStatus::Available { .. }) { return; }
    let a = app.clone();
    tauri::async_runtime::spawn(async move { download(&a).await; });
}

/// Whether the user has the panel or settings open (do not interrupt with installation).
/// Runs on the scheduler thread, so the GTK read is routed through the main thread.
fn ui_open(app: &AppHandle) -> bool {
    let app = app.clone();
    crate::shell::on_main(move || {
        ["panel", "settings"].iter().any(|l| app.get_webview_window(l).and_then(|w| w.is_visible().ok()).unwrap_or(false))
    }).unwrap_or(false)
}

/// Scheduler thread: post-update toast, first check after 15 s, then every 6 h (15 min after a network failure); in mode
/// automatic mode, check the quiet-moment gate for a downloaded version every 5 s.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        greet_after_update(&app);
        std::thread::sleep(FIRST_CHECK);
        let mut next = Instant::now();
        let mut calm = Calm::default();
        loop {
            if Instant::now() >= next {
                if mode(&app) != Updates::Off { tauri::async_runtime::block_on(check(&app, false)); }
                next = Instant::now() + next_check(app.state::<Updater>().offline.load(Ordering::Acquire));
            }
            let ready = matches!(app.state::<Updater>().status(), UpdateStatus::Ready { .. });
            if ready && mode(&app) == Updates::Auto {
                let states: Vec<_> = app.state::<crate::core::Shared>().lock().unwrap().sessions.iter().map(|s| s.state).collect();
                let b = busy(&states, crate::shell::fullscreen_app(), ui_open(&app));
                if calm.observe(b, pets_core::time::now_ms()) {
                    if let Err(e) = tauri::async_runtime::block_on(install(&app)) {
                        if !matches!(app.state::<Updater>().status(), UpdateStatus::Available { .. }) {
                            crate::problems::report(&app, "update", tr(lang(&app), "Aktualizacja nie powiodła się", "Update failed"), &e);
                        }
                    }
                    calm = Calm::default();
                }
            } else {
                calm = Calm::default();
            }
            std::thread::sleep(GATE_TICK);
        }
    });
}

fn greet_after_update(app: &AppHandle) {
    let home = app.state::<crate::settings::SettingsState>().home.clone();
    let mut st = crate::appstate::load(&home);
    let current = app.package_info().version.to_string();
    if let Some(v) = updated_toast(st.seen_version.as_deref(), &current) {
        let title = format!("{} {v}", tr(lang(app), "Zaktualizowano do wersji", "Updated to version"));
        crate::notify::show_update(app, crate::notify::center::Kind::Updated, &title, "", None, |_| {});
    }
    if st.seen_version.as_deref() != Some(current.as_str()) {
        st.seen_version = Some(current);
        let _ = crate::appstate::save(&home, &st);
    }
}

#[tauri::command]
pub fn update_status(u: tauri::State<Updater>) -> UpdateStatus { u.status() }

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus { check(&app, true).await }

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> { install(&app).await }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_mode_announces_each_version_once() {
        assert!(should_notify(Updates::Notify, None, "0.7.1"));
        assert!(should_notify(Updates::Notify, Some("0.7.0"), "0.7.1"));
        assert!(!should_notify(Updates::Notify, Some("0.7.1"), "0.7.1"));
        assert!(!should_notify(Updates::Auto, None, "0.7.1"));
        assert!(!should_notify(Updates::Off, None, "0.7.1"));
    }

    #[test]
    fn the_updated_toast_shows_after_a_version_change_but_not_on_the_first_run() {
        assert_eq!(updated_toast(Some("0.7.0"), "0.7.1"), Some("0.7.1".to_string()));
        assert_eq!(updated_toast(Some("0.7.1"), "0.7.1"), None);
        assert_eq!(updated_toast(None, "0.7.1"), None);
    }

    #[test]
    fn status_serializes_for_the_ui() {
        let v = serde_json::to_value(UpdateStatus::Available { version: "0.7.1".into(), notes: None }).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "available", "version": "0.7.1", "notes": null }));
        assert_eq!(serde_json::to_value(UpdateStatus::Latest).unwrap(), serde_json::json!({ "state": "latest" }));
        let d = serde_json::to_value(UpdateStatus::Downloading { version: "0.7.1".into(), pct: Some(40) }).unwrap();
        assert_eq!(d["pct"], 40);
    }

    #[test]
    fn the_test_address_is_honoured_only_in_test_builds() {
        assert_eq!(endpoint_from(Some("http://127.0.0.1:8765/latest.json".into()), true), "http://127.0.0.1:8765/latest.json");
        assert_eq!(endpoint_from(Some("http://127.0.0.1:8765/latest.json".into()), false), ENDPOINT);
        assert_eq!(endpoint_from(None, true), ENDPOINT);
    }

    #[test]
    fn errors_say_whether_they_are_about_the_update_itself() {
        let v = serde_json::to_value(UpdateStatus::Error { message: "x".into(), verify: true }).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "error", "message": "x", "verify": true }));
    }

    #[test]
    fn a_second_install_waits_for_the_running_download_instead_of_failing() {
        let u = std::sync::Arc::new(Updater::default());
        let busy = u.take().unwrap();
        let u2 = u.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            done_tx.send(tauri::async_runtime::block_on(async { u2.acquire().await.is_some() })).unwrap();
        });
        ready_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(done_rx.try_recv().is_err());
        drop(busy);
        assert!(done_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap());
        waiter.join().unwrap();
    }

    #[test]
    fn a_network_failure_retries_sooner() {
        assert_eq!(next_check(false), EVERY);
        assert!(next_check(true) < EVERY);
    }

    #[test]
    fn network_trouble_is_not_reported_as_a_bad_signature() {
        assert!(is_network(&tauri_plugin_updater::Error::Network("timeout".into())));
        assert!(!is_network(&tauri_plugin_updater::Error::ReleaseNotFound));
        assert!(!is_network(&tauri_plugin_updater::Error::TargetNotFound("windows-x86_64".into())));
    }

    #[test]
    fn download_percent_is_bounded_and_unknown_without_a_length() {
        assert_eq!(percent(50, Some(200)), Some(25));
        assert_eq!(percent(500, Some(200)), Some(100));
        assert_eq!(percent(10, None), None);
        assert_eq!(percent(10, Some(0)), None);
    }
}
