//! Core thread: events from `pets-core` → state snapshot for the UI (`pets://snapshot`).
use pets_core::dismiss::{self, Dismissed};
use pets_core::i18n::{tr, Lang};
use pets_core::ingest::StateBoard;
use pets_core::labels::{self, Labels};
use pets_core::mod_state::{self, ModPrefs};
use pets_core::model::{Limit, Session};
use pets_core::replay::Replay;
use pets_core::runtime::{Runtime, RuntimeConfig};
use pets_core::store::{Store, Timing};
use pets_core::time::now_ms;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub limits: Vec<Limit>,
    /// Daily totals for agents with their own database (opencode, spec 0.11 §4.2).
    pub agent_usage: Vec<pets_core::model::AgentUsage>,
    /// Windows that run out before they reset at the current pace.
    #[serde(default)]
    pub forecasts: Vec<pets_core::pace::Forecast>,
    /// Core clock (ms); differs from wall clock in replay mode.
    pub now: i64,
}

pub type Shared = Arc<Mutex<Snapshot>>;

/// Snapshot without manually hidden sessions (those active since being hidden return), with the user's names and pins applied.
/// A child (subagent) is visible only with a visible parent: hiding the parent hides its children.
pub fn snapshot_of(store: &Store, now: i64, hidden: &mut Dismissed, labels: &mut Labels) -> Snapshot {
    let sessions = with_parents(labels.apply(hidden.filter(store.sessions().into_iter().cloned().collect()), now));
    Snapshot { sessions, limits: store.limits().to_vec(), agent_usage: store.agent_usage().to_vec(), forecasts: Vec::new(), now }
}

fn with_parents(mut v: Vec<Session>) -> Vec<Session> {
    loop {
        let ids: std::collections::HashSet<String> = v.iter().map(|s| s.id.clone()).collect();
        let before = v.len();
        v.retain(|s| s.parent.as_ref().map(|p| ids.contains(p)).unwrap_or(true));
        if v.len() == before { return v; }
    }
}

/// Board for the Claude mod: the same visible sessions and limits as the widget, reduced to the safe fields.
fn board_of(store: &Store, now: i64, hidden: &mut Dismissed, labels: &mut Labels, prefs: ModPrefs) -> Vec<u8> {
    let snap = snapshot_of(store, now, hidden, labels);
    mod_state::render(&snap.sessions, &snap.limits, prefs, env!("CARGO_PKG_VERSION"))
}

fn publish_board(board: &StateBoard, store: &Store, now: i64, hidden: &mut Dismissed, labels: &mut Labels, prefs: ModPrefs) {
    let bytes = board_of(store, now, hidden, labels, prefs);
    // a poisoned lock still holds a whole board: the next write replaces it
    *board.write().unwrap_or_else(|e| e.into_inner()) = bytes;
}

/// "Remove inactive": hides visible sessions in idle states and returns their IDs (for "Undo").
/// A child is not hidden separately: it disappears with its parent or on its own when finished. A pinned session stays.
pub fn dismiss_inactive(store: &Store, hidden: &mut Dismissed, labels: &mut Labels, now: i64) -> Vec<String> {
    let ids: Vec<String> = labels.apply(hidden.filter(store.sessions().into_iter().cloned().collect()), now)
        .into_iter().filter(|s| s.parent.is_none() && !s.pinned && dismiss::inactive(s)).map(|s| s.id).collect();
    hidden.dismiss(&ids, now);
    ids
}

pub enum Mode { Live, Replay { path: PathBuf, speed: f64 } }

impl Mode {
    pub fn from_env() -> Mode {
        match std::env::var_os("AGENT_PETS_REPLAY") {
            Some(p) => Mode::Replay {
                path: p.into(),
                speed: std::env::var("AGENT_PETS_REPLAY_SPEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            },
            None => Mode::Live,
        }
    }
}

/// Commands for the core thread: app list from settings, manual session hiding, own names and pins.
/// Hiding responds with a list of hidden IDs (for "Undo").
pub enum CoreMsg {
    Apps(pets_core::settings::Apps),
    Lang(Lang),
    /// The mod's switches from settings; they ride the board.
    ModPrefs(ModPrefs),
    Dismiss(Vec<String>, Sender<Vec<String>>),
    DismissInactive(Sender<Vec<String>>),
    Undismiss(Vec<String>),
    /// Own name for a session (`None` or empty = back to the automatic one).
    Rename(String, Option<String>),
    Pin(String, bool),
}

pub struct Control(pub std::sync::Mutex<Sender<CoreMsg>>);

/// Command with a response (hidden IDs). A non-listening core (replay mode) responds immediately with an empty list.
pub fn ask(control: &Sender<CoreMsg>, make: impl FnOnce(Sender<Vec<String>>) -> CoreMsg) -> Vec<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    if control.send(make(tx)).is_err() { return Vec::new(); }
    rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
}

/// `snaps`: each published snapshot also goes to this channel (notifications).
pub fn spawn(app: AppHandle, shared: Shared, mode: Mode, snaps: Option<Sender<Snapshot>>,
             msgs: std::sync::mpsc::Receiver<CoreMsg>) {
    std::thread::spawn(move || {
        let pace = Mutex::new(pets_core::pace::Pace::default());
        let publish = |store: &Store, now: i64, hidden: &mut Dismissed, labels: &mut Labels| {
            let mut s = snapshot_of(store, now, hidden, labels);
            {
                let mut pace = pace.lock().unwrap();
                pace.observe(&s.limits, now);
                s.forecasts = pace.forecasts(&s.limits, now);
            }
            *shared.lock().unwrap() = s.clone();
            if let Some(tx) = &snaps { let _ = tx.send(s.clone()); }
            let _ = app.emit("pets://snapshot", s);
        };
        let result = match mode {
            Mode::Live => live(&app, msgs, &publish),
            // replay does not handle commands: a closed channel responds immediately instead of waiting 2 s
            Mode::Replay { path, speed } => { drop(msgs); replay(&path, speed, &publish) }
        };
        if let Err(e) = result {
            pets_core::app_log!("data core stopped: {e:#}");
            // release builds have no console: otherwise the user would see only an empty taskbar area
            {
                use tauri::Manager;
                let lang = pets_core::i18n::current(app.state::<crate::settings::SettingsState>().get().language);
                crate::tray::set_status(&app, &failure_text(&e, lang));
            }
        }
    });
}

/// Tray icon tooltip on core failure; Windows truncates it to 127 characters.
pub fn failure_text(e: &anyhow::Error, lang: Lang) -> String {
    let head = tr(lang, "Agent Pets: rdzeń danych nie działa (", "Agent Pets: data core stopped (");
    let room = 127 - head.chars().count() - 1;
    let msg = format!("{e:#}");
    let msg = if msg.chars().count() > room { format!("{}…", msg.chars().take(room - 1).collect::<String>()) } else { msg };
    format!("{head}{msg})")
}

fn live(app: &AppHandle, msgs: std::sync::mpsc::Receiver<CoreMsg>, publish: &dyn Fn(&Store, i64, &mut Dismissed, &mut Labels))
    -> anyhow::Result<()> {
    use tauri::Manager;
    let hidden_path = dismiss::path(&app.state::<crate::settings::SettingsState>().home);
    let mut hidden = Dismissed::load(&hidden_path);
    hidden.prune(now_ms());
    let labels_path = labels::path(&app.state::<crate::settings::SettingsState>().home);
    let mut labels = Labels::load(&labels_path);
    labels.prune(now_ms());
    let mut cfg = RuntimeConfig::from_env()?;
    cfg.apps = app.state::<crate::settings::SettingsState>().get().apps;
    cfg.lang = pets_core::i18n::current(app.state::<crate::settings::SettingsState>().get().language);
    let mut prefs = crate::mod_prefs_of(&app.state::<crate::settings::SettingsState>().get());
    cfg.desk = Some(crate::mod_actions::desk(app));
    let mut rt = Runtime::start(cfg)?;
    let board = rt.state_board();
    // claude account limits from the Anthropic server (exact reset times, independent of CLI sessions), only with consent
    let (usage_tx, usage) = std::sync::mpsc::channel();
    let a = app.clone();
    // quiet while the Claude mod reports the same limits from inside Claude Code
    let has_token = crate::usage::spawn(usage_tx, move || {
        a.state::<crate::settings::SettingsState>().get().claude_plan_usage
            && !crate::usage::mod_reading_recent(&a.state::<crate::settings::LastSeen>().0.lock().unwrap(), now_ms())
    });
    // antigravity limits from its local server when Antigravity is enabled in settings
    let (ag_tx, ag_usage) = std::sync::mpsc::channel();
    let a = app.clone();
    crate::antigravity_usage::spawn(ag_tx, move || a.state::<crate::settings::SettingsState>().get().apps.antigravity);
    // The first snapshot briefly waits for server limits: notification rules then treat an existing limit
    // above 90% as preexisting at launch rather than new.
    if has_token {
        if let Ok(e) = usage.recv_timeout(Duration::from_secs(3)) { rt.apply_external(e); }
    }
    publish(rt.store(), now_ms(), &mut hidden, &mut labels);
    publish_board(&board, rt.store(), now_ms(), &mut hidden, &mut labels, prefs);
    loop {
        let now = now_ms();
        let mut changed = false;
        for m in msgs.try_iter() {
            match m {
                CoreMsg::Apps(a) => changed |= rt.set_apps(a),
                CoreMsg::Lang(l) => rt.set_lang(l),
                CoreMsg::ModPrefs(p) => { prefs = p; changed = true; }
                CoreMsg::Dismiss(ids, reply) => { hidden.dismiss(&ids, now); let _ = reply.send(ids); changed = true; }
                CoreMsg::DismissInactive(reply) => { let _ = reply.send(dismiss_inactive(rt.store(), &mut hidden, &mut labels, now)); changed = true; }
                CoreMsg::Undismiss(ids) => { hidden.undismiss(&ids); changed = true; }
                CoreMsg::Rename(id, name) => { labels.rename(&id, name, now); changed = true; }
                CoreMsg::Pin(id, pinned) => { labels.pin(&id, pinned, now); changed = true; }
            }
        }
        changed |= rt.step(now);
        for e in usage.try_iter() { changed |= rt.apply_external(e); }
        for u in ag_usage.try_iter() { changed |= rt.antigravity_usage(u); }
        if changed {
            publish(rt.store(), now, &mut hidden, &mut labels);
            publish_board(&board, rt.store(), now, &mut hidden, &mut labels, prefs);
            if let Err(e) = hidden.save_if_dirty(&hidden_path) { pets_core::app_log!("{}: {e}", hidden_path.display()); }
            if let Err(e) = labels.save_if_dirty(&labels_path) { pets_core::app_log!("{}: {e}", labels_path.display()); }
        }
        // every pass, not only on change: a repeated mod reading with the same limits must keep oauth polling quiet
        *app.state::<crate::settings::LastSeen>().0.lock().unwrap() =
            rt.last_seen().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Demo recordings are for preview: nothing is hidden or renamed in them.
fn replay(path: &Path, speed: f64, publish: &dyn Fn(&Store, i64, &mut Dismissed, &mut Labels)) -> anyhow::Result<()> {
    let mut none = Dismissed::default();
    let mut plain = Labels::default();
    let mut rp = Replay::load(path, speed)?;
    let mut store = Store::new(Timing::default());
    while let Some(d) = rp.next_delay_ms() {
        std::thread::sleep(Duration::from_millis(d));
        if let Some(clock) = rp.apply_next(&mut store) { publish(&store, clock, &mut none, &mut plain); }
    }
    // After the recording, the clock continues in real time so thresholds still work (done → idle, etc.).
    let end = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = rp.clock() + end.elapsed().as_millis() as i64;
        if !store.tick(now, &|_| true).is_empty() { publish(&store, now, &mut none, &mut plain); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pets_core::model::*;
    use pets_core::replay::Replay;
    use pets_core::store::{Store, Timing};
    use pets_core::dismiss::Dismissed;
    use pets_core::labels::Labels;
    use std::path::Path;

    #[test]
    fn snapshot_serializes_like_the_ts_types() {
        let mut store = Store::new(Timing::default());
        let mut e = Event::new(Source::Claude, "s1", Kind::ToolStart, 1_000);
        e.tool = Some(Tool::Edit);
        e.data.progress = Some(Progress { done: 1, total: 4 });
        store.apply(&e);
        let v = serde_json::to_value(snapshot_of(&store, 2_000, &mut Dismissed::default(), &mut Labels::default())).unwrap();
        assert_eq!(v["now"], 2_000);
        let s = &v["sessions"][0];
        assert_eq!(s["state"], "working");
        assert_eq!(s["tool"], "edit");
        assert_eq!(s["progress"]["total"], 4);
        assert_eq!(s["started_at"], 1_000);
        assert!(s["context"].is_null());
        assert!(s["router_task"].is_null());
        assert!(v["limits"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_dismissed_session_leaves_the_snapshot_and_returns_on_new_activity() {
        let mut store = Store::new(Timing::default());
        store.apply(&Event::new(Source::Claude, "s1", Kind::Prompt, 1_000));
        store.apply(&Event::new(Source::Claude, "s2", Kind::Prompt, 1_000));
        let mut hidden = Dismissed::default();
        hidden.dismiss(&["s1".into()], 1_500);
        let ids = |s: &Snapshot| s.sessions.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&snapshot_of(&store, 1_600, &mut hidden, &mut Labels::default())), vec!["s2"]);
        store.apply(&Event::new(Source::Claude, "s1", Kind::NeedsInput, 2_000));
        assert_eq!(ids(&snapshot_of(&store, 2_100, &mut hidden, &mut Labels::default())).len(), 2);
    }

    #[test]
    fn the_mod_board_lists_only_visible_sessions_and_only_safe_fields() {
        let mut store = Store::new(Timing::default());
        for id in ["s1", "s2"] {
            let mut e = Event::new(Source::Claude, id, Kind::Prompt, 1_000);
            e.data.pid = Some(424_242);
            store.apply(&e);
        }
        child(&mut store, "s1/a", "s1", 1_000);
        let mut hidden = Dismissed::default();
        hidden.dismiss(&["s1".into()], 1_500);
        let bytes = board_of(&store, 1_600, &mut hidden, &mut Labels::default(), ModPrefs::default());
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ids: Vec<&str> = v["sessions"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["s2"], "a dismissed session and its child stay off the board");
        assert!(!String::from_utf8(bytes).unwrap().contains("424242"));
        let on = board_of(&store, 1_600, &mut hidden, &mut Labels::default(), ModPrefs { pet: true, nudges: false });
        let v: serde_json::Value = serde_json::from_slice(&on).unwrap();
        assert_eq!(v["prefs"], serde_json::json!({"pet": true, "nudges": false}));
    }

    #[test]
    fn publishing_the_board_replaces_what_the_endpoint_serves() {
        let board: StateBoard = Arc::new(std::sync::RwLock::new(mod_state::empty("0.0.0")));
        let mut store = Store::new(Timing::default());
        store.apply(&Event::new(Source::Claude, "s1", Kind::Prompt, 1_000));
        publish_board(&board, &store, 1_100, &mut Dismissed::default(), &mut Labels::default(), ModPrefs { pet: true, nudges: true });
        let v: serde_json::Value = serde_json::from_slice(&board.read().unwrap()).unwrap();
        assert_eq!(v["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(v["app_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["prefs"], serde_json::json!({"pet": true, "nudges": true}));
        publish_board(&board, &store, 1_200, &mut Dismissed::default(), &mut Labels::default(), ModPrefs { pet: false, nudges: true });
        let v: serde_json::Value = serde_json::from_slice(&board.read().unwrap()).unwrap();
        assert_eq!(v["prefs"]["pet"], false, "a republish carries the new switches");
    }

    #[test]
    fn remove_inactive_hides_idle_done_and_ended_but_keeps_asking_and_failing_sessions() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/many-sessions.jsonl");
        let mut rp = Replay::load(Path::new(path), 1000.0).unwrap();
        let mut store = Store::new(Timing::default());
        while rp.apply_next(&mut store).is_some() {}
        let mut hidden = Dismissed::default();
        let gone = dismiss_inactive(&store, &mut hidden, &mut Labels::default(), rp.clock());
        assert!(gone.contains(&"d7".to_string()), "{gone:?}");
        assert!(!gone.contains(&"d5".to_string()) && !gone.contains(&"d6".to_string()));
        let left: Vec<_> = snapshot_of(&store, rp.clock(), &mut hidden, &mut Labels::default()).sessions.into_iter().map(|s| s.id).collect();
        assert!(left.contains(&"d5".to_string()) && left.contains(&"d6".to_string()) && !left.contains(&"d7".to_string()));
    }

    #[test]
    fn the_snapshot_and_the_board_carry_the_users_names_and_pins() {
        let mut store = Store::new(Timing::default());
        let mut e = Event::new(Source::Claude, "s1", Kind::Prompt, 1_000);
        e.data.title = Some("auto title".into());
        store.apply(&e);
        store.apply(&Event::new(Source::Claude, "s2", Kind::Prompt, 1_000));
        let mut labels = Labels::default();
        labels.rename("s1", Some("My name".into()), 1_000);
        labels.pin("s2", true, 1_000);
        let snap = snapshot_of(&store, 1_100, &mut Dismissed::default(), &mut labels);
        let by = |id: &str| snap.sessions.iter().find(|s| s.id == id).unwrap().clone();
        assert_eq!((by("s1").title.as_str(), by("s1").pinned), ("My name", false));
        assert!(by("s2").pinned);
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["sessions"].as_array().unwrap().iter().filter(|s| s["pinned"] == true).count(), 1);
        let board: serde_json::Value = serde_json::from_slice(&board_of(&store, 1_100, &mut Dismissed::default(), &mut labels, ModPrefs::default())).unwrap();
        assert!(board["sessions"].as_array().unwrap().iter().any(|s| s["title"] == "My name"));
    }

    #[test]
    fn remove_inactive_keeps_a_pinned_session() {
        let mut store = Store::new(Timing::default());
        store.apply(&Event::new(Source::Claude, "keep", Kind::Prompt, 1_000));
        store.apply(&Event::new(Source::Claude, "drop", Kind::Prompt, 1_000));
        store.apply(&Event::new(Source::Claude, "keep", Kind::TurnEnd, 2_000));
        store.apply(&Event::new(Source::Claude, "drop", Kind::TurnEnd, 2_000));
        let mut labels = Labels::default();
        labels.pin("keep", true, 2_000);
        let mut hidden = Dismissed::default();
        let gone = dismiss_inactive(&store, &mut hidden, &mut labels, 100_000_000);
        assert_eq!(gone, vec!["drop".to_string()]);
    }

    #[test]
    fn asking_a_core_that_does_not_listen_answers_at_once() {
        let (tx, rx) = std::sync::mpsc::channel::<CoreMsg>();
        drop(rx); // replay mode does not read commands
        assert!(ask(&tx, |r| CoreMsg::Dismiss(vec!["a".into()], r)).is_empty());
    }

    #[test]
    fn asking_the_core_returns_its_answer() {
        let (tx, rx) = std::sync::mpsc::channel::<CoreMsg>();
        std::thread::spawn(move || if let Ok(CoreMsg::Dismiss(ids, r)) = rx.recv() { let _ = r.send(ids); });
        assert_eq!(ask(&tx, |r| CoreMsg::Dismiss(vec!["a".into()], r)), vec!["a".to_string()]);
    }

    #[test]
    fn failure_text_names_the_problem_and_fits_the_tray_tooltip() {
        let short = failure_text(&anyhow::anyhow!("home directory missing"), Lang::Pl);
        assert_eq!(short, "Agent Pets: rdzeń danych nie działa (home directory missing)");
        let long = failure_text(&anyhow::anyhow!("{}", "ż".repeat(300)), Lang::Pl);
        assert!(long.chars().count() <= 127, "{}", long.chars().count());
        assert!(long.ends_with("…)"));
    }

    #[test]
    fn failure_text_in_english() {
        assert_eq!(failure_text(&anyhow::anyhow!("no home"), Lang::En), "Agent Pets: data core stopped (no home)");
    }

    #[test]
    fn demo_recording_yields_seven_sessions_and_limits() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/many-sessions.jsonl");
        let mut rp = Replay::load(Path::new(path), 1000.0).unwrap();
        let mut store = Store::new(Timing::default());
        while rp.apply_next(&mut store).is_some() {}
        let snap = snapshot_of(&store, rp.clock(), &mut Dismissed::default(), &mut Labels::default());
        assert_eq!(snap.sessions.len(), 7);
        assert_eq!(snap.limits.len(), 4);
        let state = |id: &str| store.session(id).map(|s| s.state);
        assert_eq!(state("d5"), Some(State::NeedsYou));
        assert_eq!(state("d6"), Some(State::Error));
        assert_eq!(state("d7"), Some(State::Done));
    }

    fn child(store: &mut Store, id: &str, parent: &str, ts: i64) {
        let mut e = Event::new(Source::Claude, id, Kind::Prompt, ts);
        e.data.parent = Some(parent.into());
        e.data.sub = Some(SubInfo { kind: SubKind::Claude, agent_type: None, description: None, background: false });
        store.apply(&e);
    }

    #[test]
    fn hiding_a_parent_hides_its_children() {
        let mut store = Store::new(Timing::default());
        store.apply(&Event::new(Source::Claude, "p", Kind::Prompt, 1_000));
        store.apply(&Event::new(Source::Claude, "q", Kind::Prompt, 1_000));
        child(&mut store, "p/a", "p", 1_000);
        child(&mut store, "q/b", "q", 1_000);
        let mut hidden = Dismissed::default();
        hidden.dismiss(&["p".into()], 1_500);
        let ids: Vec<String> = snapshot_of(&store, 1_600, &mut hidden, &mut Labels::default()).sessions.into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["q".to_string(), "q/b".to_string()]);
    }

    #[test]
    fn remove_inactive_never_picks_a_child_on_its_own() {
        let mut store = Store::new(Timing::default());
        store.apply(&Event::new(Source::Claude, "p", Kind::Prompt, 1_000));
        child(&mut store, "p/a", "p", 1_000);
        store.apply(&{ let mut e = Event::new(Source::Claude, "p/a", Kind::TurnEnd, 2_000); e.data.parent = Some("p".into()); e });
        let gone = dismiss_inactive(&store, &mut Dismissed::default(), &mut Labels::default(), 3_000);
        assert!(gone.is_empty(), "{gone:?}");
    }
}
