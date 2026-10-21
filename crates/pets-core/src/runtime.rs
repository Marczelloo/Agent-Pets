//! Live data core: hook ingestion, file watching, state restoration at startup, and the clock.
use crate::claude::{self, hook::{HookState, TaskTracker}, HookEnvelope};
use crate::endpoint::Endpoint;
use crate::ingest::{Incoming, Ingest, StateBoard};
use crate::model::Event;
use crate::pid;
use crate::rehydrate::{keep_for_rehydration, recent_files};
use crate::store::{Store, Timing};
use crate::watch::{watch, Sources};
use anyhow::Context as _;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

pub struct RuntimeConfig {
    pub home: PathBuf,
    pub endpoint_path: PathBuf,
    /// Optional recording of normalized events (JSONL), as with `pets-cli run --record`.
    pub record: Option<File>,
    /// Claude app `plan-usage-history.json` files (account limits without CLI).
    pub claude_usage_files: Vec<PathBuf>,
    /// Public Agent Router task status file (`~/.agent-router/status.json`).
    pub router_status: Option<PathBuf>,
    /// Apps with pets (settings).
    pub apps: crate::settings::Apps,
    /// Language of action and question text.
    pub lang: crate::i18n::Lang,
    /// Where the Claude mod's commands go (`/v1/actions/claude-mod`); `None` leaves the route out.
    pub desk: Option<crate::mod_action::Desk>,
}

impl RuntimeConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(RuntimeConfig {
            home: dirs::home_dir().context("home directory not found")?,
            endpoint_path: Endpoint::default_path(),
            record: None,
            claude_usage_files: claude::desktop_usage::candidate_files(
                &dirs::data_local_dir().unwrap_or_default(), &dirs::config_dir().unwrap_or_default()),
            router_status: dirs::home_dir().map(|h| h.join(".agent-router").join(crate::router::FILE)),
            apps: crate::settings::Apps::default(),
            lang: crate::i18n::system(),
            desk: None,
        })
    }
}

pub struct Runtime {
    store: Store,
    sources: Sources,
    tasks: TaskTracker,
    hook_state: HookState,
    lang: crate::i18n::Lang,
    /// Router tasks → sessions that requested them (IDs only, `~/.agent-pets/links.json`).
    links: crate::links::Links,
    links_path: PathBuf,
    record: Option<File>,
    hooks: Receiver<Incoming>,
    files: Option<Receiver<PathBuf>>,
    usage: claude::desktop_usage::Poller,
    router: Option<crate::router::Poller>,
    apps: crate::settings::Apps,
    last_seen: std::collections::BTreeMap<&'static str, i64>,
    _ingest: Ingest,
    /// Routes enabled in settings (door, Copilot, Antigravity), shared with the endpoint thread.
    doors: std::sync::Arc<crate::ingest::Doors>,
    /// Body of `GET /v1/state`, shared with the endpoint thread.
    board: StateBoard,
    /// Host program for an opencode session, resolved once per session (one process snapshot).
    opencode_hosts: std::collections::HashMap<String, Option<crate::host::Host>>,
    /// Home directory (opencode database).
    #[cfg_attr(not(feature = "opencode-db"), allow(dead_code))]
    home: PathBuf,
    /// Last opencode usage read and request for an immediate read (turn end).
    #[cfg_attr(not(feature = "opencode-db"), allow(dead_code))]
    usage_at: Option<i64>,
    usage_due: bool,
    _watcher: Option<notify::RecommendedWatcher>,
}

impl Runtime {
    pub fn start(cfg: RuntimeConfig) -> anyhow::Result<Runtime> {
        Self::start_with_watch(cfg, true)
    }

    fn start_with_watch(cfg: RuntimeConfig, watch_files: bool) -> anyhow::Result<Runtime> {
        let links_path = crate::links::path(&cfg.home);
        let mut links = crate::links::Links::load(&links_path);
        links.prune(crate::time::now_ms());
        let (tx, hooks) = channel();
        let doors = std::sync::Arc::new(crate::ingest::Doors::new(&cfg.apps));
        let board: StateBoard = std::sync::Arc::new(std::sync::RwLock::new(crate::mod_state::empty(env!("CARGO_PKG_VERSION"))));
        let ingest = Ingest::start_with_desk(Endpoint::new_token(), tx, doors.clone(), board.clone(), cfg.desk)?;
        ingest.endpoint().write(&cfg.endpoint_path).context("writing endpoint.json")?;
        let mut rt = Runtime {
            store: Store::new(Timing::default()), sources: Sources::new(), tasks: TaskTracker::default(),
            hook_state: HookState::default(), lang: cfg.lang, links, links_path,
            record: cfg.record, hooks, files: None, usage: claude::desktop_usage::Poller::new(cfg.claude_usage_files),
            router: cfg.router_status.map(crate::router::Poller::new), apps: cfg.apps, last_seen: Default::default(),
            _ingest: ingest, doors, board, opencode_hosts: Default::default(), _watcher: None,
            home: cfg.home.clone(), usage_at: None, usage_due: false,
        };
        rt.sources.lang = rt.lang;
        let roots = [cfg.home.join(".claude").join("projects"), cfg.home.join(".codex").join("sessions")];
        // Restore state: live Claude sessions from the registry, then their transcripts and Codex rollouts.
        let live: Vec<_> = claude::registry::read_registry(&cfg.home.join(".claude").join("sessions"))
            .into_iter().filter(|s| pid::is_alive(s.pid)).collect();
        for s in &live { rt.apply(s.to_event()); }
        let live_ids = live.iter().map(|s| s.session_id.clone()).collect();
        let recent: Vec<PathBuf> = roots.iter().flat_map(|r| recent_files(r, Duration::from_secs(1800))).collect();
        for f in keep_for_rehydration(&recent, &live_ids) { rt.poll_file(&f); }
        rt.end_answered_subagents(crate::time::now_ms());
        let first = rt.usage.poll(crate::time::now_ms());
        rt.desktop_usage(first);
        rt.poll_router(crate::time::now_ms());
        // one tick before the first snapshot: old threads from recently touched files disappear before being shown
        rt.store.tick(crate::time::now_ms(), &pid::is_alive);
        if watch_files {
            let (ftx, files) = channel();
            rt._watcher = Some(watch(&roots, ftx)?);
            rt.files = Some(files);
        }
        Ok(rt)
    }

    pub fn store(&self) -> &Store { &self.store }

    /// The board served by `GET /v1/state`: the same allocation the endpoint thread reads.
    pub fn state_board(&self) -> StateBoard { self.board.clone() }

    /// Event from outside the core (e.g. Claude account limits fetched by the app). Return whether state changed.
    pub fn apply_external(&mut self, e: Event) -> bool { self.apply(e) }

    /// Antigravity limits read by the app from its local server; `Stale` (Antigravity closed)
    /// removes them immediately because waiting until reset would show old numbers for hours.
    pub fn antigravity_usage(&mut self, u: crate::adapters::antigravity_usage::Usage) -> bool {
        use crate::adapters::antigravity_usage::Usage;
        match u {
            Usage::Limits(e) => self.apply(e),
            Usage::Stale => self.store.retain_limits(|l| l.agent != crate::model::Agent::Antigravity),
        }
    }

    /// Enable and disable apps live: sessions and limits of disabled apps disappear immediately. Return whether state changed.
    pub fn set_apps(&mut self, apps: crate::settings::Apps) -> bool {
        if apps == self.apps { return false; }
        if apps.agent_router && !self.apps.agent_router {
            if let Some(p) = self.router.as_mut() { p.reset(); }
        }
        self.apps = apps;
        self.doors.set(&apps);
        let removed = self.store.retain_sessions(|s| session_on(apps, s))
            | (!apps.opencode && self.store.set_agent_usage(Vec::new()));
        self.prune_opencode_hosts();
        removed | self.store.retain_limits(|l| agent_on(apps, l.agent))
    }

    /// Language of new action and question text (settings change).
    pub fn set_lang(&mut self, lang: crate::i18n::Lang) { self.lang = lang; self.sources.lang = lang; }

    /// Last event time from each source (diagnostics).
    pub fn last_seen(&self) -> std::collections::BTreeMap<&'static str, i64> { self.last_seen.clone() }

    /// Process pending events and advance the clock. Return `true` when state changed.
    pub fn step(&mut self, now: i64) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.hooks.try_recv() {
            changed |= match msg {
                Incoming::ClaudeHook(env) => self.on_hook(env),
                Incoming::ClaudeMod(p) => self.on_mod(p),
                Incoming::Generic(e) => self.apply(e),
                Incoming::Opencode(v) => self.on_opencode(&v),
                Incoming::Copilot(env) => self.apps.copilot
                    && crate::adapters::copilot::events(&env, self.lang).into_iter().fold(false, |c, e| self.apply(e) | c),
                Incoming::Antigravity(env) => self.apps.antigravity
                    && crate::adapters::antigravity::events(&env, self.lang).into_iter().fold(false, |c, e| self.apply(e) | c),
                Incoming::Cursor(env) => self.apps.cursor
                    && crate::adapters::cursor::events(&env, self.lang).into_iter().fold(false, |c, e| self.apply(e) | c),
                Incoming::Grok(env) => self.apps.grok
                    && crate::adapters::grok::events(&env, self.lang).into_iter().fold(false, |c, e| self.apply(e) | c),
                Incoming::Zcode(env) => self.apps.zcode
                    && crate::adapters::zcode::events(&env, self.lang).into_iter().fold(false, |c, e| self.apply(e) | c),
            };
        }
        let paths: Vec<PathBuf> = self.files.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for p in paths { changed |= self.poll_file(&p); }
        changed |= self.poll_router(now);
        changed |= self.poll_opencode_usage(now);
        let usage = self.usage.poll(now);
        changed |= self.desktop_usage(usage);
        changed |= !self.store.tick(now, &pid::is_alive).is_empty();
        self.prune_opencode_hosts();
        changed
    }

    fn prune_opencode_hosts(&mut self) {
        self.opencode_hosts.retain(|id, _| self.store.session(id).is_some());
    }

    /// Claude app limits. An older "as of" reading is replaced by the next report, so a window that ran out disappears.
    fn desktop_usage(&mut self, u: Option<claude::desktop_usage::Usage>) -> bool {
        use claude::desktop_usage::Usage;
        use crate::model::Agent;
        match u {
            Some(Usage::Limits(e)) => {
                let dropped = self.store.retain_limits(|l| l.agent != Agent::Claude || l.stale_since.is_none());
                self.apply(e) | dropped
            }
            Some(Usage::Stale) => self.store.drop_limits_without_reset(Agent::Claude),
            None => false,
        }
    }

    fn poll_router(&mut self, now: i64) -> bool {
        if !self.apps.agent_router { return false; }
        let events = self.router.as_mut().map(|p| p.poll(now)).unwrap_or_default();
        events.into_iter().fold(false, |c, e| self.apply(e) | c)
    }

    fn apply(&mut self, mut e: Event) -> bool {
        if !event_on(self.apps, &e) {
            // a disabled app's session is removed, but account limits (e.g. Codex from a router thread) may remain
            let limits: Vec<_> = e.data.limits.iter().filter(|l| agent_on(self.apps, l.agent)).copied().collect();
            if limits.is_empty() { return false; }
            let mut only = Event::new(e.source, e.session_id.clone(), crate::model::Kind::Limits, e.ts);
            only.data.limits = limits;
            e = only;
        }
        let key = source_key(&e);
        let seen = self.last_seen.entry(key).or_insert(e.ts);
        *seen = (*seen).max(e.ts);
        self.as_child_of_caller(&mut e);
        if let Some(f) = &mut self.record {
            // action and question text (file names, commands) is never written to disk
            let mut clean = e.clone();
            clean.data.action = None;
            clean.data.question = None;
            let _ = writeln!(f, "{}", serde_json::to_string(&clean).unwrap_or_default());
        }
        let mut changed = !self.store.apply(&e).is_empty();
        if let Some(task) = e.data.router_link.clone() {
            // requested by a subagent (or Codex child thread): attach as a child of the main session, since grandchildren are not drawn
            let owner = e.data.parent.clone().or_else(|| self.store.session(&e.session_id).and_then(|s| s.parent.clone()))
                .unwrap_or_else(|| e.session_id.clone());
            changed |= self.link(&task, &owner, e.ts);
        }
        changed
    }

    /// A router task requested by a tracked session is its child (the store orphans it when the parent is absent).
    fn as_child_of_caller(&self, e: &mut Event) {
        let Some(task) = e.data.router_task.as_ref() else { return };
        let Some(parent) = self.links.parent_of(&task.task_id) else { return };
        e.data.parent = Some(parent.to_string());
        e.data.sub = Some(crate::model::SubInfo {
            kind: crate::model::SubKind::Router, agent_type: None, description: e.data.title.clone(), background: false,
        });
    }

    /// Save the link and immediately attach an already visible task to its parent.
    fn link(&mut self, task: &str, parent: &str, ts: i64) -> bool {
        self.links.link(task, parent, ts);
        let _ = self.links.save_if_dirty(&self.links_path);
        let known: Vec<(String, String, i64)> = self.store.sessions().iter()
            .filter(|s| s.router_task.as_ref().map(|r| r.task_id.as_str()) == Some(task) && s.parent.as_deref() != Some(parent))
            .map(|s| (s.id.clone(), s.title.clone(), s.last_activity)).collect();
        let mut changed = false;
        for (id, title, last) in known {
            // time before last activity: attaching changes only data, not state or activity
            let mut m = Event::new(crate::model::Source::Router, id, crate::model::Kind::Meta, last - 1);
            m.data.parent = Some(parent.to_string());
            m.data.sub = Some(crate::model::SubInfo {
                kind: crate::model::SubKind::Router, agent_type: None, description: Some(title).filter(|t| !t.is_empty()), background: false,
            });
            changed |= !self.store.apply(&m).is_empty();
        }
        changed
    }

    fn poll_file(&mut self, p: &Path) -> bool {
        use crate::watch::{kind_of, FileKind};
        match kind_of(p) {
            Some(FileKind::ClaudeTranscript | FileKind::ClaudeSubagent) if !self.apps.claude_code => return false,
            // Codex rollouts also carry router threads
            Some(FileKind::CodexRollout) if !self.apps.codex && !self.apps.agent_router => return false,
            _ => {}
        }
        let mut changed = false;
        if self.sources.track(p, true) { for e in self.sources.poll(p) { changed |= self.apply(e); } }
        changed
    }

    /// After restoration: a subagent whose file ends in a recent response has finished (hooks are not replayed).
    fn end_answered_subagents(&mut self, now: i64) {
        for (id, ts) in self.sources.finished_subagents() {
            if now - ts < 30_000 || self.store.session(&id).map(|s| s.state == crate::model::State::Ended).unwrap_or(true) { continue; }
            let mut e = Event::new(crate::model::Source::Claude, id.clone(), crate::model::Kind::SessionEnd, ts);
            e.data.parent = id.split('/').next().map(String::from);
            self.apply(e);
        }
    }

    fn on_opencode(&mut self, env: &serde_json::Value) -> bool {
        if !self.apps.opencode { return false; }
        let mut changed = false;
        for mut e in crate::adapters::opencode::events(env, self.lang) {
            // new (or returning) session: host program from the process tree above opencode
            if self.store.session(&e.session_id).is_none() {
                let pid = e.data.pid;
                let host = self.opencode_hosts.entry(e.session_id.clone())
                    .or_insert_with(|| pid.and_then(|p| crate::host::host_of(&crate::host::ProcTable::snapshot(), p))).clone();
                if let Some(h) = host {
                    e.data.app = Some(h.app);
                    e.data.app_name = h.name;
                    e.data.host_pid = Some(h.pid);
                }
            }
            if e.kind == crate::model::Kind::SessionEnd { self.opencode_hosts.remove(&e.session_id); }
            // turn end: new tokens in the database; read them at the next step
            if e.kind == crate::model::Kind::TurnEnd { self.usage_due = true; }
            changed |= self.apply(e);
        }
        changed
    }

    /// opencode usage from its database (spec 0.11 §4.2–4.3): only with the integration enabled and a live opencode pet,
    /// every 30 s or immediately after turn end.
    #[cfg(feature = "opencode-db")]
    fn poll_opencode_usage(&mut self, now: i64) -> bool {
        use crate::opencode_db as db;
        if !self.apps.opencode { return false; }
        let ids: Vec<String> = self.store.sessions().iter().filter(|s| s.agent == crate::model::Agent::Opencode).map(|s| s.id.clone()).collect();
        if ids.is_empty() { return false; }
        if !self.usage_due && self.usage_at.is_some_and(|t| now - t < 30_000) { return false; }
        self.usage_due = false;
        self.usage_at = Some(now);
        let Some(c) = db::open(&db::db_path(&self.home)) else { return false };
        let raw: Vec<&str> = ids.iter().map(|i| i.strip_prefix("opencode:").unwrap_or(i)).collect();
        let mut changed = false;
        for u in db::session_usage(&c, &raw) {
            let usage = crate::model::Usage { tokens: u.tokens, cost: u.cost, account: db::account_for(&u) };
            changed |= self.store.set_usage(&format!("opencode:{}", u.id), Some(usage));
        }
        let (tokens_today, cost_today) = db::today(&c, crate::time::local_midnight(now));
        changed | self.store.set_agent_usage(vec![crate::model::AgentUsage { agent: crate::model::Agent::Opencode, tokens_today, cost_today }])
    }

    #[cfg(not(feature = "opencode-db"))]
    fn poll_opencode_usage(&mut self, _now: i64) -> bool { false }

    /// Payload from the Claude Code mod. It only enriches sessions that hooks already created (a mod payload never
    /// makes a pet); account limits are the exception because they belong to no session.
    fn on_mod(&mut self, p: claude::plugin::ModPayload) -> bool {
        if !self.apps.claude_code { return false; }
        let update = claude::plugin::to_update(&p);
        let mut changed = false;
        for e in update.events {
            if e.kind == crate::model::Kind::Limits || self.store.session(&e.session_id).is_some() {
                changed |= self.apply(e);
            }
        }
        if let Some(cost) = update.cost {
            let usage = crate::model::Usage { tokens: 0, cost, account: Some(crate::model::Agent::Claude) };
            changed |= self.store.set_usage(&p.session_id, Some(usage));
        }
        changed
    }

    fn on_hook(&mut self, env: HookEnvelope) -> bool {
        if !self.apps.claude_code { return false; }
        let mut changed = false;
        if let Some(tp) = claude::hook::transcript_path(&env) { changed |= self.poll_file(&tp); }
        // subagent lines before its `SubagentStop`: late ones will not revive a finished child
        if let Some(sp) = claude::hook::subagent_transcript_path(&env) { changed |= self.poll_file(&sp); }
        for e in self.hook_state.events(&env, self.lang) { changed |= self.apply(e); }
        if let Some(e) = self.tasks.observe(&env) { changed |= self.apply(e); }
        changed
    }
}

fn agent_on(apps: crate::settings::Apps, agent: crate::model::Agent) -> bool {
    match agent {
        crate::model::Agent::Claude => apps.claude_code,
        // router requests tasks on the same Codex account
        crate::model::Agent::Codex => apps.codex || apps.agent_router,
        crate::model::Agent::Opencode => apps.opencode,
        crate::model::Agent::Other => apps.generic,
        crate::model::Agent::Copilot => apps.copilot,
        crate::model::Agent::Antigravity => apps.antigravity,
        crate::model::Agent::Cursor => apps.cursor,
        crate::model::Agent::Grok => apps.grok,
        crate::model::Agent::Zcode => apps.zcode,
    }
}

fn session_on(apps: crate::settings::Apps, s: &crate::model::Session) -> bool {
    match (s.origin, s.agent) {
        (crate::model::Origin::Router, _) => apps.agent_router,
        (_, crate::model::Agent::Claude) => apps.claude_code,
        (_, crate::model::Agent::Codex) => apps.codex,
        (_, a) => agent_on(apps, a),
    }
}

fn event_on(apps: crate::settings::Apps, e: &Event) -> bool {
    use crate::model::{Origin, Source};
    if e.source == Source::Router || e.data.origin == Some(Origin::Router) { return apps.agent_router; }
    match e.source {
        Source::Claude => apps.claude_code,
        Source::Codex => apps.codex,
        Source::Router => apps.agent_router,
        Source::Opencode => apps.opencode,
        Source::Generic => apps.generic,
        Source::Copilot => apps.copilot,
        Source::Antigravity => apps.antigravity,
        Source::Cursor => apps.cursor,
        Source::Grok => apps.grok,
        Source::Zcode => apps.zcode,
    }
}

fn source_key(e: &Event) -> &'static str {
    use crate::model::Source;
    if e.session_id == claude::desktop_usage::SESSION_ID || e.session_id == claude::account_usage::SESSION_ID {
        return "claude_usage";
    }
    if e.session_id == claude::plugin::USAGE_SESSION_ID { return "claude_mod_usage"; }
    if e.session_id == crate::adapters::antigravity_usage::SESSION_ID { return "antigravity_usage"; }
    match e.source {
        Source::Claude => "claude_code",
        Source::Codex => "codex",
        Source::Router => "agent_router",
        Source::Opencode => "opencode",
        Source::Generic => "generic",
        Source::Copilot => "copilot",
        Source::Antigravity => "antigravity",
        Source::Cursor => "cursor",
        Source::Grok => "grok",
        Source::Zcode => "zcode",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::Endpoint;
    use crate::model::{Agent, Origin};
    use std::time::Duration;

    fn home() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for p in [".claude/projects", ".claude/sessions", ".codex/sessions/2026/09/24"] {
            std::fs::create_dir_all(d.path().join(p)).unwrap();
        }
        d
    }

    fn cfg(h: &tempfile::TempDir) -> RuntimeConfig {
        RuntimeConfig { home: h.path().into(), endpoint_path: h.path().join("endpoint.json"), record: None,
            claude_usage_files: vec![h.path().join("Claude").join(claude::desktop_usage::FILE)],
            router_status: Some(h.path().join(".agent-router").join(crate::router::FILE)),
            apps: crate::settings::Apps::default(), lang: crate::i18n::Lang::Pl, desk: None }
    }

    fn start(cfg: RuntimeConfig) -> Runtime { Runtime::start_with_watch(cfg, false).unwrap() }

    #[test]
    fn public_start_and_shutdown_use_only_the_temporary_home() {
        let h = home();
        let rt = Runtime::start(cfg(&h)).unwrap();
        assert!(h.path().join("endpoint.json").exists());
        drop(rt);
    }

    #[test]
    fn claude_desktop_usage_file_reaches_limits() {
        let h = home();
        let now = crate::time::now_ms();
        std::fs::create_dir_all(h.path().join("Claude")).unwrap();
        std::fs::write(h.path().join("Claude").join(claude::desktop_usage::FILE),
            serde_json::json!({"version": 2, "samples": [{"t": now, "org": "o", "u": {"fh": 42, "sd": 7}}]}).to_string()).unwrap();
        // already in the first post-startup snapshot: otherwise notification rules would treat an existing limit as new
        let rt = start(cfg(&h));
        let five = rt.store().limits().iter().find(|l| l.agent == Agent::Claude && l.window == crate::model::Window::FiveHour).copied();
        assert_eq!(five.map(|l| l.used_pct), Some(42.0));
    }

    #[test]
    fn an_old_claude_app_reading_stays_visible_as_of_its_time() {
        let h = home();
        let t = crate::time::now_ms() - 18 * 3_600_000;
        std::fs::create_dir_all(h.path().join("Claude")).unwrap();
        std::fs::write(h.path().join("Claude").join(claude::desktop_usage::FILE),
            serde_json::json!({"version": 2, "samples": [{"t": t, "org": "o", "u": {"fh": 38, "sd": 47}}]}).to_string()).unwrap();
        let rt = start(cfg(&h));
        let l: Vec<_> = rt.store().limits().iter().filter(|l| l.agent == Agent::Claude).map(|l| (l.window, l.used_pct, l.stale_since)).collect();
        assert_eq!(l, vec![(crate::model::Window::Weekly, 47.0, Some(t))], "the 5 h window is long over");
    }

    #[test]
    fn antigravity_limits_arrive_and_go_away_when_the_app_closes() {
        use crate::adapters::antigravity_usage::{from_summary, Usage};
        let h = home();
        let mut c = cfg(&h);
        c.apps.antigravity = true;
        let mut rt = start(c);
        let body = serde_json::json!({"groups": [{"buckets": [
            {"bucketId": "gemini-5h", "remainingFraction": 0.75, "resetTime": crate::time::rfc3339(crate::time::now_ms() + 3_600_000)}]}]});
        assert!(rt.antigravity_usage(Usage::Limits(from_summary(&body, crate::time::now_ms()).unwrap())));
        assert_eq!(rt.store().limits().iter().filter(|l| l.agent == Agent::Antigravity).map(|l| l.used_pct).collect::<Vec<_>>(), vec![25.0]);
        assert!(rt.antigravity_usage(Usage::Stale));
        assert!(rt.store().limits().iter().all(|l| l.agent != Agent::Antigravity));
        assert!(!rt.store().sessions().iter().any(|s| s.agent == Agent::Antigravity), "limits do not create a pet");
    }

    #[test]
    fn antigravity_limits_are_ignored_while_antigravity_is_off() {
        use crate::adapters::antigravity_usage::{from_summary, Usage};
        let h = home();
        let mut c = cfg(&h);
        c.apps.antigravity = false;
        let mut rt = start(c);
        let body = serde_json::json!({"groups": [{"buckets": [{"bucketId": "gemini-5h", "remainingFraction": 0.5}]}]});
        rt.antigravity_usage(Usage::Limits(from_summary(&body, crate::time::now_ms()).unwrap()));
        assert!(rt.store().limits().iter().all(|l| l.agent != Agent::Antigravity));
    }

    /// Recording with times shifted so the last event happened just now.
    fn fresh(path: &str) -> String {
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(path).unwrap().lines()
            .map(|l| serde_json::from_str(l).unwrap()).collect();
        let ts = |v: &serde_json::Value| v["timestamp"].as_str().and_then(crate::time::rfc3339_ms);
        let shift = crate::time::now_ms() - 60_000 - lines.iter().filter_map(ts).max().unwrap();
        lines.into_iter().map(|mut v| {
            if let Some(t) = ts(&v) { v["timestamp"] = crate::time::rfc3339(t + shift).into(); }
            v.to_string() + "\n"
        }).collect()
    }

    #[test]
    fn rehydrates_recent_codex_rollouts() {
        let h = home();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codex/router-task.jsonl");
        // write instead of fs::copy: Windows copying preserves the old modification time,
        // and restore only files changed within the last 30 minutes
        std::fs::write(h.path().join(".codex/sessions/2026/09/24/rollout-test.jsonl"), fresh(fixture)).unwrap();
        let rt = start(cfg(&h));
        assert!(rt.store().sessions().iter().any(|s| s.agent == Agent::Codex && s.origin == Origin::Router));
    }

    fn router_status(h: &tempfile::TempDir, thread: &str, status: &str, updated: i64) {
        let dir = h.path().join(".agent-router");
        std::fs::create_dir_all(&dir).unwrap();
        let t = crate::time::rfc3339(updated);
        std::fs::write(dir.join(crate::router::FILE), serde_json::json!({"version": 1, "updatedAt": t, "stallSeconds": 180,
            "tasks": [{"taskId": "t1", "threadId": thread, "title": "Policz pliki", "status": status,
                       "workingDirectory": "C:/work", "updatedAt": t, "lastActivityAt": t, "blocked": false}]}).to_string()).unwrap();
    }

    #[test]
    fn router_task_joins_its_codex_pet_at_startup() {
        let h = home();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codex/router-task.jsonl");
        std::fs::write(h.path().join(".codex/sessions/2026/09/24/rollout-test.jsonl"), fresh(fixture)).unwrap();
        let thread = "01a04e96-7474-79a1-a173-8c3cc2919eeb";
        router_status(&h, thread, "running", crate::time::now_ms());
        let rt = start(cfg(&h));
        let s = rt.store().session(thread).expect("one session per thread");
        assert_eq!((s.origin, s.title.as_str()), (Origin::Router, "Policz pliki"));
        assert_eq!(s.router_task.as_ref().map(|r| r.task_id.as_str()), Some("t1"));
        assert_eq!(rt.store().sessions().len(), 1, "no second pet for the same task");
    }

    #[test]
    fn an_old_failed_router_task_does_not_appear_at_startup() {
        let h = home();
        router_status(&h, "old-thread", "failed", crate::time::now_ms() - 2 * 3_600_000);
        let rt = start(cfg(&h));
        assert!(rt.store().sessions().is_empty());
    }

    fn event(src: crate::model::Source, id: &str, kind: crate::model::Kind) -> crate::model::Event {
        crate::model::Event::new(src, id, kind, crate::time::now_ms())
    }

    #[test]
    fn a_disabled_app_brings_no_pets() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut c = cfg(&h);
        c.apps.claude_code = false;
        let mut rt = start(c);
        assert!(!rt.apply_external(event(Source::Claude, "c1", Kind::Prompt)));
        assert!(rt.store().session("c1").is_none());
        assert!(rt.apply_external(event(Source::Codex, "x1", Kind::Prompt)));
    }

    #[test]
    fn opencode_and_the_door_follow_their_switches() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut c = cfg(&h);
        c.apps.opencode = true;
        let mut rt = start(c);
        rt.apply_external(event(Source::Claude, "c1", Kind::Prompt));
        rt.apply_external(event(Source::Opencode, "opencode:a", Kind::Prompt));
        rt.apply_external(event(Source::Generic, "generic:kilo:a", Kind::Prompt));
        let apps = crate::settings::Apps { opencode: false, generic: false, ..Default::default() };
        assert!(rt.set_apps(apps));
        let ids: Vec<&str> = rt.store().sessions().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["c1"]);
        assert!(!rt.apply_external(event(Source::Generic, "generic:kilo:b", Kind::Prompt)));
        assert!(!rt.apply_external(event(Source::Opencode, "opencode:b", Kind::Prompt)));
    }

    #[test]
    fn opencode_host_cache_follows_session_removal() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut c = cfg(&h);
        c.apps.opencode = true;
        let mut rt = Runtime::start(c).unwrap();
        rt.apply_external(event(Source::Opencode, "opencode:keep", Kind::Prompt));
        rt.apply_external(event(Source::Opencode, "opencode:gone", Kind::Prompt));
        rt.opencode_hosts.insert("opencode:keep".into(), None);
        rt.opencode_hosts.insert("opencode:gone".into(), None);
        rt.store.retain_sessions(|s| s.id != "opencode:gone");
        rt.step(crate::time::now_ms());
        assert!(rt.opencode_hosts.contains_key("opencode:keep"));
        assert!(!rt.opencode_hosts.contains_key("opencode:gone"));
        rt.set_apps(crate::settings::Apps { opencode: false, ..Default::default() });
        assert!(rt.opencode_hosts.is_empty());
    }

    #[test]
    fn copilot_and_antigravity_follow_their_switches() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut rt = start(cfg(&h));
        assert!(!rt.apply_external(event(Source::Copilot, "copilot:a", Kind::Prompt)));
        assert!(!rt.apply_external(event(Source::Antigravity, "antigravity:a", Kind::Prompt)));
        rt.set_apps(crate::settings::Apps { copilot: true, antigravity: true, ..Default::default() });
        assert!(rt.apply_external(event(Source::Copilot, "copilot:a", Kind::Prompt)));
        assert!(rt.apply_external(event(Source::Antigravity, "antigravity:a", Kind::Prompt)));
        assert_eq!(rt.store().session("copilot:a").map(|s| s.agent), Some(Agent::Copilot));
        assert!(rt.set_apps(crate::settings::Apps { copilot: false, antigravity: true, ..Default::default() }));
        assert!(rt.store().session("copilot:a").is_none());
        assert_eq!(rt.store().session("antigravity:a").map(|s| s.agent), Some(Agent::Antigravity));
    }

    #[test]
    fn copilot_and_antigravity_routes_open_with_their_switches() {
        let h = home();
        let mut rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let send = |agent: &str, token: &str, body: &str| ureq::post(&format!("http://127.0.0.1:{}/v1/events/{agent}", ep.port))
            .set("Authorization", &format!("Bearer {token}")).send_string(body)
            .map(|r| r.status()).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => c, _ => 0 });
        let env = r#"{"ts":1,"ppid":5,"event":"Stop","payload":{"sessionId":"s"}}"#;
        for agent in ["copilot", "antigravity"] { assert_eq!(send(agent, &ep.token, env), 404, "{agent} closed"); }
        rt.set_apps(crate::settings::Apps { copilot: true, antigravity: true, ..Default::default() });
        for agent in ["copilot", "antigravity"] {
            assert_eq!(send(agent, &ep.token, env), 204, "{agent}");
            assert_eq!(send(agent, "wrong", env), 401, "{agent}");
            assert_eq!(send(agent, &ep.token, r#"{"x":1}"#), 400, "{agent}");
        }
    }

    #[test]
    fn cursor_grok_and_zcode_follow_their_switches() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut rt = start(cfg(&h));
        let all = [(Source::Cursor, "cursor:a", Agent::Cursor), (Source::Grok, "grok:a", Agent::Grok), (Source::Zcode, "zcode:a", Agent::Zcode)];
        for (x, id, _) in all { assert!(!rt.apply_external(event(x, id, Kind::Prompt)), "{id} disabled"); }
        rt.set_apps(crate::settings::Apps { cursor: true, grok: true, zcode: true, ..Default::default() });
        for (x, id, a) in all {
            assert!(rt.apply_external(event(x, id, Kind::Prompt)), "{id}");
            assert_eq!(rt.store().session(id).map(|s| s.agent), Some(a));
        }
        assert!(rt.set_apps(crate::settings::Apps { cursor: true, ..Default::default() }));
        assert!(rt.store().session("cursor:a").is_some());
        assert!(rt.store().session("grok:a").is_none() && rt.store().session("zcode:a").is_none());
    }

    #[test]
    fn cursor_grok_and_zcode_routes_open_with_their_switches() {
        let h = home();
        let mut rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let send = |agent: &str, token: &str, body: &str| ureq::post(&format!("http://127.0.0.1:{}/v1/events/{agent}", ep.port))
            .set("Authorization", &format!("Bearer {token}")).send_string(body)
            .map(|r| r.status()).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => c, _ => 0 });
        let env = r#"{"ts":1,"ppid":5,"event":"stop","payload":{"sessionId":"s"}}"#;
        for agent in ["cursor", "grok", "zcode"] { assert_eq!(send(agent, &ep.token, env), 404, "{agent} closed"); }
        rt.set_apps(crate::settings::Apps { cursor: true, grok: true, zcode: true, ..Default::default() });
        for agent in ["cursor", "grok", "zcode"] {
            assert_eq!(send(agent, &ep.token, env), 204, "{agent}");
            assert_eq!(send(agent, "wrong", env), 401, "{agent}");
            assert_eq!(send(agent, &ep.token, r#"{"x":1}"#), 400, "{agent}");
        }
    }

    fn post_agent(ep: &Endpoint, agent: &str, body: &str) -> u16 {
        ureq::post(&format!("http://127.0.0.1:{}/v1/events/{agent}", ep.port))
            .timeout(Duration::from_secs(10)).set("Authorization", &format!("Bearer {}", ep.token)).send_string(body)
            .map(|r| r.status()).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => c, _ => 0 })
    }

    #[test]
    fn a_zcode_hook_brings_a_zcode_pet_only_when_zcode_is_on() {
        for on in [true, false] {
            let h = home();
            let mut c = cfg(&h);
            c.apps.zcode = on;
            let mut rt = start(c);
            let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
            let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": std::process::id(), "event": "PreToolUse",
                "payload": {"sessionId": "zc_1", "toolName": "Bash", "toolInput": {"command": "npm test"}}}).to_string();
            assert_eq!(post_agent(&ep, "zcode", &body), if on { 204 } else { 404 });
            rt.step(crate::time::now_ms());
            let s = rt.store().session("zcode:zc_1");
            assert_eq!(s.map(|s| (s.agent, s.tool)), on.then_some((Agent::Zcode, Some(crate::model::Tool::Bash))), "zcode {on}");
        }
    }

    #[test]
    fn a_grok_hook_brings_a_grok_pet_only_when_grok_is_on() {
        for on in [true, false] {
            let h = home();
            let mut c = cfg(&h);
            c.apps.grok = on;
            let mut rt = start(c);
            let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
            let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": std::process::id(), "event": "PreToolUse",
                "payload": {"sessionId": "grok_1", "toolName": "bash", "toolInput": {"command": "npm test"}}}).to_string();
            assert_eq!(post_agent(&ep, "grok", &body), if on { 204 } else { 404 });
            rt.step(crate::time::now_ms());
            let s = rt.store().session("grok:grok_1");
            assert_eq!(s.map(|s| (s.agent, s.tool)), on.then_some((Agent::Grok, Some(crate::model::Tool::Bash))), "grok {on}");
        }
    }

    #[test]
    fn a_cursor_hook_brings_a_cursor_pet_only_when_cursor_is_on() {
        for on in [true, false] {
            let h = home();
            let mut c = cfg(&h);
            c.apps.cursor = on;
            let mut rt = start(c);
            let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
            let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": std::process::id(), "event": "preToolUse",
                "payload": {"conversation_id": "conv_1", "tool_name": "Shell", "tool_input": {"command": "npm test"}}}).to_string();
            assert_eq!(post_agent(&ep, "cursor", &body), if on { 204 } else { 404 });
            rt.step(crate::time::now_ms());
            let s = rt.store().session("cursor:conv_1");
            assert_eq!(s.map(|s| (s.agent, s.tool)), on.then_some((Agent::Cursor, Some(crate::model::Tool::Bash))), "cursor {on}");
        }
    }

    #[test]
    fn a_copilot_hook_brings_a_copilot_pet() {
        let h = home();
        let mut c = cfg(&h);
        c.apps.copilot = true;
        let mut rt = start(c);
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": std::process::id(), "event": "PreToolUse",
            "payload": {"sessionId": "cop_1", "cwd": "C:/w", "toolName": "bash", "toolArgs": {"command": "cargo test"}}}).to_string();
        assert_eq!(post_agent(&ep, "copilot", &body), 204);
        rt.step(crate::time::now_ms());
        let s = rt.store().session("copilot:cop_1").expect("Copilot session");
        assert_eq!((s.agent, s.jump.pid, s.tool), (Agent::Copilot, Some(std::process::id()), Some(crate::model::Tool::Bash)));
        assert_eq!(s.action.as_deref(), Some("cargo test"));
    }

    #[test]
    fn an_antigravity_hook_brings_an_antigravity_pet() {
        let h = home();
        let mut c = cfg(&h);
        c.apps.antigravity = true;
        let mut rt = start(c);
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": std::process::id(), "event": "PreInvocation",
            "payload": {"conversationId": "d5f1", "workspacePaths": ["C:/w"], "modelName": "gemini-3.5-pro"}}).to_string();
        assert_eq!(post_agent(&ep, "antigravity", &body), 204);
        rt.step(crate::time::now_ms());
        let s = rt.store().session("antigravity:d5f1").expect("Antigravity session");
        assert_eq!((s.agent, s.state, s.model.as_deref()), (Agent::Antigravity, crate::model::State::Thinking, Some("gemini-3.5-pro")));
    }

    #[cfg(feature = "opencode-db")]
    fn opencode_db_in(h: &tempfile::TempDir, now: i64) {
        use crate::opencode_db::fixture::*;
        let p = db(crate::opencode_db::db_path(h.path()).parent().unwrap());
        let w = rusqlite::Connection::open(&p).unwrap();
        session(&w, "ses_1", None, now, 0.0, [100, 20, 5, 1000, 50]);
        message(&w, "m1", "ses_1", now - 1000, reply("openai", now - 1000, [10, 20, 3, 400, 5], 0.0));
    }

    #[cfg(feature = "opencode-db")]
    #[test]
    fn opencode_usage_reaches_its_pet_and_the_day_total() {
        use crate::model::{AgentUsage, Kind, Source, Usage};
        let h = home();
        let now = crate::time::now_ms();
        opencode_db_in(&h, now);
        let mut c = cfg(&h);
        c.apps.opencode = true;
        let mut rt = start(c);
        rt.apply_external(event(Source::Opencode, "opencode:ses_1", Kind::Prompt));
        assert!(rt.step(now));
        let s = rt.store().session("opencode:ses_1").unwrap();
        assert_eq!(s.usage, Some(Usage { tokens: 1175, cost: 0.0, account: Some(Agent::Codex) }));
        assert_eq!(rt.store().agent_usage(), &[AgentUsage { agent: Agent::Opencode, tokens_today: 438, cost_today: 0.0 }]);
        assert!(rt.set_apps(crate::settings::Apps { opencode: false, ..Default::default() }));
        assert!(rt.store().agent_usage().is_empty(), "disabled opencode has no daily total");
    }

    #[cfg(feature = "opencode-db")]
    #[test]
    fn the_opencode_database_is_opened_only_when_opencode_is_on_and_has_a_pet() {
        use crate::model::{Kind, Source};
        let h = home();
        let now = crate::time::now_ms();
        opencode_db_in(&h, now);
        let opens = || crate::opencode_db::OPENS.with(|n| n.get());
        let mut rt = start(cfg(&h));
        rt.apply_external(event(Source::Opencode, "opencode:ses_1", Kind::Prompt));
        let before = opens();
        for i in 0..40 { rt.step(now + i * 1000); }
        assert_eq!(opens(), before, "opencode disabled");
        rt.set_apps(crate::settings::Apps { opencode: true, ..Default::default() });
        for i in 0..40 { rt.step(now + i * 1000); }
        assert_eq!(opens(), before, "no opencode pet");
        rt.apply_external(event(Source::Opencode, "opencode:ses_1", Kind::Prompt));
        for i in 0..40 { rt.step(now + i * 1000); }
        assert_eq!(opens(), before + 2, "immediately, then every 30 s");
    }

    #[test]
    fn a_door_session_starts_working_and_the_switch_closes_the_route() {
        let h = home();
        let mut rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let send = |ep: &Endpoint| ureq::post(&format!("http://127.0.0.1:{}/v1/events/generic", ep.port))
            .set("Authorization", &format!("Bearer {}", ep.token))
            .send_string(r#"{"agent":"kilo","session":"a","state":"working","tool":"bash"}"#)
            .map(|r| r.status()).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => c, _ => 0 });
        assert_eq!(send(&ep), 204);
        rt.step(crate::time::now_ms());
        let s = rt.store().session("generic:kilo:a").expect("door session");
        assert_eq!((s.agent, s.state, s.tool), (Agent::Other, crate::model::State::Working, Some(crate::model::Tool::Bash)));
        rt.set_apps(crate::settings::Apps { generic: false, ..Default::default() });
        assert_eq!(send(&ep), 404);
    }

    #[test]
    fn an_opencode_plugin_envelope_brings_an_opencode_pet() {
        let h = home();
        let mut c = cfg(&h);
        c.apps.opencode = true;
        let mut rt = start(c);
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let body = serde_json::json!({"v": 1, "ts": crate::time::now_ms(), "pid": std::process::id(), "event": "tool.before",
            "session": "ses_1", "cwd": "C:/w", "tool": "bash", "input": {"command": "cargo test"}}).to_string();
        let r = ureq::post(&format!("http://127.0.0.1:{}/v1/events/opencode", ep.port))
            .set("Authorization", &format!("Bearer {}", ep.token)).send_string(&body).unwrap();
        assert_eq!(r.status(), 204);
        rt.step(crate::time::now_ms());
        let s = rt.store().session("opencode:ses_1").expect("opencode session");
        assert_eq!((s.agent, s.jump.pid), (Agent::Opencode, Some(std::process::id())));
        assert_eq!(s.action.as_deref(), Some("cargo test"));
    }

    #[test]
    fn turning_an_app_off_removes_its_pets_and_limits_at_once() {
        use crate::model::{Agent, Kind, Limit, Source, Window};
        let h = home();
        let mut rt = start(cfg(&h));
        rt.apply_external(event(Source::Claude, "c1", Kind::Prompt));
        rt.apply_external(event(Source::Codex, "x1", Kind::Prompt));
        rt.apply_external(event(Source::Router, "r1", Kind::Prompt));
        let mut l = event(Source::Codex, "x1", Kind::Limits);
        l.data.limits = vec![Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: 10.0, resets_at: None, stale_since: None }];
        rt.apply_external(l);
        let apps = crate::settings::Apps { claude_code: true, codex: false, agent_router: false, ..Default::default() };
        assert!(rt.set_apps(apps));
        let ids: Vec<&str> = rt.store().sessions().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["c1"], "router session disappears with the router despite being a Codex thread");
        assert!(rt.store().limits().is_empty());
        assert!(!rt.set_apps(apps), "repeating the same change changes nothing");
        assert!(!rt.apply_external(event(Source::Codex, "x2", Kind::Prompt)));
    }

    #[test]
    fn last_event_per_source_for_diagnostics() {
        use crate::model::{Kind, Source};
        let h = home();
        let mut rt = start(cfg(&h));
        assert!(!rt.last_seen().contains_key("codex"));
        rt.apply_external(event(Source::Codex, "x1", Kind::Prompt));
        assert!(rt.last_seen().contains_key("codex"));
    }

    #[test]
    fn long_dead_rollouts_touched_recently_do_not_appear_at_startup() {
        // the Codex app changes old rollout dates; their events are days old, so the pet must not appear
        let h = home();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codex/router-task.jsonl");
        std::fs::write(h.path().join(".codex/sessions/2026/09/24/rollout-old.jsonl"), std::fs::read(fixture).unwrap()).unwrap();
        let rt = start(cfg(&h));
        assert!(rt.store().sessions().is_empty());
    }

    #[test]
    fn hook_posted_to_endpoint_reaches_store() {
        let h = home();
        let mut rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let body = serde_json::json!({"ts": crate::time::now_ms(), "ppid": null, "payload": {
            "hook_event_name": "UserPromptSubmit", "session_id": "hook-s1", "cwd": "C:\\work\\demo", "prompt": "x"}});
        let status = ureq::post(&format!("http://127.0.0.1:{}/v1/events/claude", ep.port))
            .set("Authorization", &format!("Bearer {}", ep.token))
            .send_string(&body.to_string()).unwrap().status();
        assert_eq!(status, 204);
        rt.step(crate::time::now_ms());
        assert!(rt.store().session("hook-s1").is_some());
    }

    fn mod_payload(json: serde_json::Value) -> crate::claude::plugin::ModPayload { serde_json::from_value(json).unwrap() }

    fn send_mod(rt: &mut Runtime, json: serde_json::Value) -> bool { rt.on_mod(mod_payload(json)) }

    #[test]
    fn mod_limits_from_the_usage_session_apply_without_creating_a_session() {
        let h = home();
        let mut rt = start(cfg(&h));
        let now = crate::time::now_ms();
        let changed = send_mod(&mut rt, serde_json::json!({"v": 1, "kind": "measure", "session_id": "unknown", "ts": now,
            "rate_limits": [{"kind": "five_hour", "percent_used": 23.5}]}));
        assert!(changed);
        let five = rt.store().limits().iter().find(|l| l.agent == Agent::Claude && l.window == crate::model::Window::FiveHour).copied();
        assert_eq!(five.map(|l| l.used_pct), Some(23.5));
        assert!(rt.store().session("unknown").is_none() && rt.store().sessions().is_empty());
        assert!(rt.last_seen().contains_key("claude_mod_usage"));
    }

    #[test]
    fn mod_session_events_for_unknown_sessions_are_dropped() {
        let h = home();
        let mut rt = start(cfg(&h));
        let now = crate::time::now_ms();
        assert!(!send_mod(&mut rt, serde_json::json!({"v": 1, "kind": "measure", "session_id": "ghost", "ts": now,
            "context": {"tokens": 10, "window": 100}, "cost_usd": 1.0})));
        assert!(!send_mod(&mut rt, serde_json::json!({"v": 1, "kind": "turn_end", "session_id": "ghost", "ts": now, "reason": "aborted"})));
        assert!(rt.store().sessions().is_empty());
    }

    #[test]
    fn mod_turn_end_and_cost_reach_a_session_known_from_hooks() {
        use crate::model::{State, Usage};
        let h = home();
        let mut rt = start(cfg(&h));
        let now = crate::time::now_ms();
        rt.apply_external(event(crate::model::Source::Claude, "s1", crate::model::Kind::Prompt));
        assert_eq!(rt.store().session("s1").map(|s| s.state), Some(State::Thinking));
        assert!(send_mod(&mut rt, serde_json::json!({"v": 1, "kind": "turn_end", "session_id": "s1", "ts": now + 1, "reason": "aborted",
            "cost_usd": 1.42})));
        // the store holds a state for a minimum display time; let the clock pass it
        rt.step(now + 10_000);
        let s = rt.store().session("s1").unwrap();
        assert_eq!(s.state, State::Done);
        assert_eq!(s.usage, Some(Usage { tokens: 0, cost: 1.42, account: Some(Agent::Claude) }));
    }

    #[test]
    fn mod_payloads_are_ignored_while_claude_code_is_off() {
        let h = home();
        let mut c = cfg(&h);
        c.apps.claude_code = false;
        let mut rt = start(c);
        assert!(!send_mod(&mut rt, serde_json::json!({"v": 1, "kind": "measure", "session_id": "s1", "ts": crate::time::now_ms(),
            "rate_limits": [{"kind": "five_hour", "percent_used": 10.0}], "cost_usd": 1.0})));
        assert!(rt.store().limits().is_empty());
        assert!(!rt.last_seen().contains_key("claude_mod_usage"));
    }

    #[test]
    fn mod_payload_posted_to_the_endpoint_reaches_the_store() {
        let h = home();
        let mut rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let body = serde_json::json!({"v": 1, "kind": "measure", "session_id": "x", "ts": crate::time::now_ms(),
            "rate_limits": [{"kind": "seven_day", "percent_used": 40.0}]}).to_string();
        assert_eq!(post_agent(&ep, "claude-mod", &body), 204);
        rt.step(crate::time::now_ms());
        assert_eq!(rt.store().limits().first().map(|l| l.used_pct), Some(40.0));
    }

    #[test]
    fn the_state_board_is_the_one_the_endpoint_serves() {
        let h = home();
        let rt = start(cfg(&h));
        let ep = Endpoint::read(&h.path().join("endpoint.json")).unwrap();
        let get = |token: &str| ureq::get(&format!("http://127.0.0.1:{}/v1/state", ep.port))
            .timeout(Duration::from_secs(10)).set("Authorization", &format!("Bearer {token}")).call()
            .map(|r| (r.status(), r.into_string().unwrap())).unwrap_or_else(|e| match e { ureq::Error::Status(c, _) => (c, String::new()), _ => (0, String::new()) });
        let empty = format!(r#"{{"v":1,"app_version":"{}","prefs":{{"pet":false,"nudges":true}},"sessions":[],"limits":[]}}"#, env!("CARGO_PKG_VERSION"));
        assert_eq!(get(&ep.token), (200, empty));
        *rt.state_board().write().unwrap() = br#"{"v":1,"sessions":[1]}"#.to_vec();
        assert_eq!(get(&ep.token).1, r#"{"v":1,"sessions":[1]}"#);
        assert_eq!(get("wrong").0, 401);
    }

    fn delegated(rt: &mut Runtime, parent: &str, task: &str, ts: i64) {
        use crate::model::{Kind, Source};
        rt.apply_external(crate::model::Event::new(Source::Claude, parent, Kind::Prompt, ts));
        let mut post = crate::model::Event::new(Source::Claude, parent, Kind::ToolEnd, ts);
        post.data.router_link = Some(task.into());
        rt.apply_external(post);
    }

    #[test]
    fn a_router_task_delegated_by_a_session_is_its_child() {
        let h = home();
        let mut rt = start(cfg(&h));
        let now = crate::time::now_ms();
        delegated(&mut rt, "p1", "t1", now);
        router_status(&h, "th1", "running", now);
        rt.step(now + 2_000);
        let s = rt.store().session("th1").expect("zadanie routera");
        assert_eq!(s.parent.as_deref(), Some("p1"));
        let sub = s.sub.clone().unwrap();
        assert_eq!((sub.kind, sub.description.as_deref()), (crate::model::SubKind::Router, Some("Policz pliki")));
        let saved = std::fs::read_to_string(crate::links::path(h.path())).unwrap();
        assert!(saved.contains("t1") && saved.contains("p1") && !saved.contains("Policz"), "IDs only: {saved}");
    }

    #[test]
    fn a_link_arriving_after_the_task_attaches_it_at_once() {
        let h = home();
        let now = crate::time::now_ms();
        router_status(&h, "th1", "running", now);
        let mut rt = start(cfg(&h));
        assert_eq!(rt.store().session("th1").map(|s| s.parent.clone()), Some(None), "without a link: ordinary pet as in 0.7");
        delegated(&mut rt, "p1", "t1", now);
        assert_eq!(rt.store().session("th1").and_then(|s| s.parent.clone()).as_deref(), Some("p1"));
    }

    #[test]
    fn links_come_back_after_a_restart() {
        let h = home();
        let now = crate::time::now_ms();
        {
            let mut rt = start(cfg(&h));
            delegated(&mut rt, "p1", "t1", now);
        }
        let mut rt = start(cfg(&h));
        rt.apply_external(crate::model::Event::new(crate::model::Source::Claude, "p1", crate::model::Kind::Prompt, now));
        router_status(&h, "th1", "running", now);
        rt.step(now + 2_000);
        assert_eq!(rt.store().session("th1").and_then(|s| s.parent.clone()).as_deref(), Some("p1"));
    }

    #[test]
    fn the_recording_never_holds_action_or_question_text() {
        use crate::model::{Kind, Source};
        let h = home();
        let rec = h.path().join("rec.jsonl");
        let mut c = cfg(&h);
        c.record = Some(File::create(&rec).unwrap());
        let mut rt = start(c);
        let now = crate::time::now_ms();
        let mut t = crate::model::Event::new(Source::Claude, "s1", Kind::ToolStart, now);
        t.data.action = Some("curl -H \"Authorization: Bearer sekret-XYZ\"".into());
        rt.apply_external(t);
        let mut n = crate::model::Event::new(Source::Claude, "s1", Kind::NeedsInput, now + 1);
        n.data.question = Some("Pytanie: sekret-QQ?".into());
        rt.apply_external(n);
        drop(rt);
        let text = std::fs::read_to_string(&rec).unwrap();
        assert_eq!(text.lines().count(), 2, "events are recorded");
        assert!(!text.contains("sekret"), "{text}");
        assert!(rt_store_kept_text(&h), "text remains in memory");
    }

    fn rt_store_kept_text(h: &tempfile::TempDir) -> bool {
        use crate::model::{Kind, Source};
        let mut rt = start(cfg(h));
        let mut t = crate::model::Event::new(Source::Claude, "s2", Kind::ToolStart, crate::time::now_ms());
        t.data.action = Some("npm test".into());
        rt.apply_external(t);
        rt.store().session("s2").and_then(|s| s.action.clone()).as_deref() == Some("npm test")
    }

    #[test]
    fn a_router_task_delegated_inside_a_subagent_belongs_to_the_top_session() {
        use crate::model::{Event, Kind, Source, SubInfo, SubKind};
        let h = home();
        let mut rt = start(cfg(&h));
        let now = crate::time::now_ms();
        rt.apply_external(Event::new(Source::Claude, "p1", Kind::Prompt, now));
        let mut post = Event::new(Source::Claude, "p1/a", Kind::ToolEnd, now);
        post.data.parent = Some("p1".into());
        post.data.sub = Some(SubInfo { kind: SubKind::Claude, agent_type: None, description: None, background: false });
        post.data.router_link = Some("t1".into());
        rt.apply_external(post);
        router_status(&h, "th1", "running", now);
        rt.step(now + 2_000);
        assert_eq!(rt.store().session("th1").and_then(|s| s.parent.clone()).as_deref(), Some("p1"),
            "grandchildren are not drawn: task attaches to the main session");
    }

    #[test]
    fn a_subagent_that_already_answered_does_not_come_back_after_a_restart() {
        let h = home();
        let now = crate::time::now_ms();
        std::fs::write(h.path().join(".claude/sessions/1.json"), serde_json::json!({"pid": std::process::id(), "sessionId": "p1",
            "cwd": "C:\\w", "startedAt": now - 300_000, "entrypoint": "cli"}).to_string()).unwrap();
        let dir = h.path().join(".claude/projects/proj/p1/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let line = |agent: &str, ty: &str, content: serde_json::Value, ago: i64| serde_json::json!({"isSidechain": true, "agentId": agent,
            "sessionId": "p1", "type": ty, "timestamp": crate::time::rfc3339(now - ago), "message": {"content": content}}).to_string();
        let running = |agent: &str, ago: i64| line(agent, "assistant", serde_json::json!([{"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}]), ago);
        std::fs::write(dir.join("agent-a.jsonl"), [line("a", "user", serde_json::json!("x"), 200_000), running("a", 150_000),
            line("a", "assistant", serde_json::json!([{"type": "text", "text": "gotowe"}]), 120_000)].join("\n") + "\n").unwrap();
        std::fs::write(dir.join("agent-a.meta.json"), r#"{"agentType":"general-purpose","requestShape":"background"}"#).unwrap();
        std::fs::write(dir.join("agent-b.jsonl"), [line("b", "user", serde_json::json!("y"), 60_000), running("b", 5_000)].join("\n") + "\n").unwrap();
        let rt = start(cfg(&h));
        assert!(rt.store().session("p1/a").map(|s| s.state == crate::model::State::Ended).unwrap_or(true), "finished background subagent: {:?}", rt.store().session("p1/a").map(|s| (s.state, s.last_activity, s.parent.clone())));
        assert_eq!(rt.store().session("p1/b").map(|s| s.state), Some(crate::model::State::Working), "working one remains");
    }
}
