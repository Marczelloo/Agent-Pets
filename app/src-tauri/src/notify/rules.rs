//! When to send a notification (spec 2.4). Pure logic; delivery is in `notify/mod.rs`.
use crate::core::Snapshot;
use pets_core::i18n::{tr, Lang};
use pets_core::model::{Agent, Limit, Session, State, Window};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings { pub needs_you: bool, pub done: bool, pub limits: bool }

impl Default for Settings { fn default() -> Self { Settings { needs_you: true, done: true, limits: true } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind { NeedsYou, Done, Limit }

#[derive(Clone, Debug, PartialEq)]
pub struct Toast { pub kind: ToastKind, pub session_id: Option<String>, pub title: String, pub body: String }

pub struct Rules {
    settings: Settings,
    lang: Lang,
    primed: bool,
    /// Reported `needs_you` episodes and `done` turns.
    sent: HashSet<String>,
    /// Limit windows over 90% (reported, or already over at startup) → that window's `resets_at`.
    limits: HashMap<(Agent, Window), Option<i64>>,
    spend_hit: HashSet<(Agent, Window)>,
    /// Windows already warned about running out early -> that window's `resets_at`.
    early: HashMap<(Agent, Window), i64>,
    /// Local `HH:MM` of a timestamp; tests swap in a UTC one.
    clock: fn(i64) -> String,
}

const NEEDS_AFTER_MS: i64 = 15_000;
/// One more toast for an episode still waiting this long (the first one is easy to miss).
const REMIND_AFTER_MS: i64 = 600_000;
const LONG_TURN_MS: i64 = 120_000;
const LIMIT_PCT: f32 = 90.0;
/// Below this usage, treat the limit window as new (for limits without reset times and fluctuations around 90%).
const LIMIT_REARM_PCT: f32 = 80.0;
/// The same window's reset time fluctuates between readings; a larger shift indicates a new window.
const RESET_JITTER_MS: i64 = 60_000;
/// Early warning: at least this much used, running out within the hour, and at least this long before the reset.
const EARLY_MIN_PCT: f32 = 50.0;
const EARLY_WITHIN_MS: i64 = 3_600_000;
const EARLY_GAP_MS: i64 = 900_000;
/// A reset further than this after running out is named without its time.
const EARLY_FAR_MS: i64 = 12 * 3_600_000;

fn local_clock(ts: i64) -> String { pets_core::mute::clock(ts, pets_core::time::local_midnight) }

fn name(s: &Session, lang: Lang) -> String {
    if !s.title.is_empty() { return s.title.chars().take(60).collect(); }
    s.cwd.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(tr(lang, "Sesja", "Session")).to_string()
}

/// 950, 12,4 tys., 4,8 mln (en: 12.4K, 4.8M), like `formatTokens` in the UI.
fn compact(n: u64, lang: Lang) -> String {
    let (units, dec, space) = match lang { Lang::Pl => (["tys.", "mln", "mld"], ",", " "), Lang::En => (["K", "M", "B"], ".", "") };
    for (base, unit) in [(1e9, units[2]), (1e6, units[1]), (1e3, units[0])] {
        if n as f64 >= base {
            let r = (n as f64 / base * 10.0).round() / 10.0;
            let v = if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}").replace('.', dec) };
            return format!("{v}{space}{unit}");
        }
    }
    n.to_string()
}

/// " · sesja: 2,3 mln tok." / " · 1,42 $": what the session used so far, when the agent reports it.
fn usage_tail(s: &Session, lang: Lang) -> String {
    let Some(u) = s.usage else { return String::new() };
    if u.tokens > 0 { return format!(" · {}", match lang { Lang::Pl => format!("sesja: {} tok.", compact(u.tokens, lang)), Lang::En => format!("session: {} tokens", compact(u.tokens, lang)) }); }
    if u.cost > 0.0 { return format!(" · {}", match lang { Lang::Pl => format!("{:.2} $", u.cost).replace('.', ","), Lang::En => format!("${:.2}", u.cost) }); }
    String::new()
}

fn who(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
        Agent::Opencode => "opencode",
        Agent::Antigravity => "Antigravity",
        Agent::Copilot => "Copilot",
        Agent::Cursor => "Cursor",
        Agent::Grok => "Grok",
        Agent::Zcode => "ZCode",
        Agent::Other => "Agent",
    }
}

fn limit_toast(l: &Limit, lang: Lang) -> Toast {
    let who = who(l.agent);
    if l.window == Window::Spend && l.used_pct >= 100.0 {
        let (title, body) = match lang {
            Lang::Pl => (format!("{who}: limit wydatków osiągnięty"), format!("Wykorzystano {:.0}% limitu wydatków", l.used_pct)),
            Lang::En => (format!("{who}: spend limit hit"), format!("{:.0}% of the spend limit used", l.used_pct)),
        };
        return Toast { kind: ToastKind::Limit, session_id: None, title, body };
    }
    let (title, body) = match lang {
        Lang::Pl => { let w = match l.window { Window::FiveHour => "5h", Window::Weekly => "tygodniowy", Window::Spend => "wydatków", Window::Monthly => "miesięczny" }; (format!("{who}: limit {w}"), format!("Zużyto {:.0}% limitu {w}", l.used_pct)) }
        Lang::En => { let w = match l.window { Window::FiveHour => "5h", Window::Weekly => "weekly", Window::Spend => "spend", Window::Monthly => "monthly" }; (format!("{who}: {w} limit"), format!("{:.0}% of the {w} limit used", l.used_pct)) }
    };
    Toast { kind: ToastKind::Limit, session_id: None, title, body }
}

/// The window will be used up well before it resets, if the pace holds.
fn limit_early_toast(agent: Agent, window: Window, runs_out_at: i64, resets_at: i64, lang: Lang, clock: fn(i64) -> String) -> Toast {
    let who = who(agent);
    let (out, reset) = (clock(runs_out_at), clock(resets_at));
    // a bare HH:MM for a reset days away (weekly windows) would read as today
    let far = resets_at - runs_out_at > EARLY_FAR_MS;
    let (title, body) = match lang {
        Lang::Pl => { let w = match window { Window::FiveHour => "5h", Window::Weekly => "tygodniowy", Window::Spend => "wydatków", Window::Monthly => "miesięczny" }; (format!("{who}: limit {w} kończy się"), if far { format!("W tym tempie skończy się ok. {out}, na długo przed resetem") } else { format!("W tym tempie skończy się ok. {out}, przed resetem o {reset}") }) }
        Lang::En => { let w = match window { Window::FiveHour => "5h", Window::Weekly => "weekly", Window::Spend => "spend", Window::Monthly => "monthly" }; (format!("{who}: {w} limit running out"), if far { format!("At this pace it runs out around {out}, long before the reset") } else { format!("At this pace it runs out around {out}, before the {reset} reset") }) }
    };
    Toast { kind: ToastKind::Limit, session_id: None, title, body }
}

/// The window that was nearly used up has reset.
fn limit_back_toast(agent: Agent, window: Window, lang: Lang) -> Toast {
    let who = who(agent);
    let (title, body) = match lang {
        Lang::Pl => { let w = match window { Window::FiveHour => "5h", Window::Weekly => "tygodniowy", Window::Spend => "wydatków", Window::Monthly => "miesięczny" }; (format!("{who}: limit {w} odnowiony"), format!("Limit {w} znów jest dostępny")) }
        Lang::En => { let w = match window { Window::FiveHour => "5h", Window::Weekly => "weekly", Window::Spend => "spend", Window::Monthly => "monthly" }; (format!("{who}: {w} limit reset"), format!("The {w} limit is available again")) }
    };
    Toast { kind: ToastKind::Limit, session_id: None, title, body }
}

impl Rules {
    pub fn set_settings(&mut self, settings: Settings) { self.settings = settings; }
    pub fn set_lang(&mut self, lang: Lang) { self.lang = lang; }

    pub fn new(settings: Settings) -> Rules {
        Rules { settings, lang: Lang::Pl, primed: false, sent: HashSet::new(), limits: HashMap::new(), spend_hit: HashSet::new(), early: HashMap::new(), clock: local_clock }
    }

    /// First call only remembers state: nothing already present at app startup is reported.
    pub fn observe(&mut self, snap: &Snapshot, now: i64, focused: &dyn Fn(&Session) -> bool) -> Vec<Toast> {
        let mut out = Vec::new();
        let priming = !self.primed;
        self.primed = true;
        // children (subagents, router tasks) send no notifications: approvals still wait with the parent
        for s in snap.sessions.iter().filter(|s| s.parent.is_none()) {
            match s.state {
                State::NeedsYou => {
                    let key = format!("needs:{}:{}", s.id, s.state_since);
                    let remind = format!("remind:{}:{}", s.id, s.state_since);
                    if priming { self.sent.insert(key); self.sent.insert(remind); continue; }
                    let waited = now - s.state_since;
                    if !self.settings.needs_you || focused(s) { continue; }
                    if waited > NEEDS_AFTER_MS && !self.sent.contains(&key) {
                        self.sent.insert(key);
                        out.push(Toast { kind: ToastKind::NeedsYou, session_id: Some(s.id.clone()),
                            title: tr(self.lang, "Agent czeka na Ciebie", "Agent needs you").into(),
                            body: format!("{} {}", name(s, self.lang), tr(self.lang, "czeka na Ciebie", "is waiting for you")) });
                    } else if waited > REMIND_AFTER_MS && self.sent.insert(remind) {
                        let min = waited / 60_000;
                        out.push(Toast { kind: ToastKind::NeedsYou, session_id: Some(s.id.clone()),
                            title: tr(self.lang, "Agent wciąż czeka", "Agent is still waiting").into(),
                            body: match self.lang {
                                Lang::Pl => format!("{} czeka na Ciebie od {min} min", name(s, self.lang)),
                                Lang::En => format!("{} has been waiting for you for {min} min", name(s, self.lang)),
                            } });
                    }
                }
                State::Done => if let Some(t) = s.turn_started_at {
                    let key = format!("done:{}:{t}", s.id);
                    if priming { self.sent.insert(key); continue; }
                    if self.settings.done && s.state_since - t > LONG_TURN_MS && self.sent.insert(key) {
                        out.push(Toast { kind: ToastKind::Done, session_id: Some(s.id.clone()), title: tr(self.lang, "Agent skończył", "Agent finished").into(),
                            body: format!("{} {} {} min{}", name(s, self.lang), tr(self.lang, "skończył po", "finished after"), (s.state_since - t) / 60_000, usage_tail(s, self.lang)) });
                    }
                },
                _ => {}
            }
        }
        // a window over 90% whose reset time has passed is back, even with no fresh reading (the agent may be closed)
        let mut passed: Vec<(Agent, Window)> = self.limits.iter().filter(|(_, r)| r.is_some_and(|r| now >= r)).map(|(k, _)| *k).collect();
        passed.sort_by_key(|(a, w)| (*a as u8, *w as u8));
        for key in passed { self.limit_back(key, priming, &mut out); }
        for l in snap.limits.iter().filter(|l| l.stale_since.is_none()) {
            let key = (l.agent, l.window);
            // usage dropping well below 90% is a new window too (limits without reset times)
            if l.used_pct < LIMIT_REARM_PCT { self.limit_back(key, priming, &mut out); continue; }
            // a reading of a window that has already reset says nothing about the new one
            if l.used_pct < LIMIT_PCT || l.resets_at.is_some_and(|r| now >= r) { continue; }
            let new_window = match (self.limits.get(&key), l.resets_at) {
                (None, _) => true,
                (Some(Some(known)), Some(r)) => (r - known).abs() > RESET_JITTER_MS,
                // first reading with a reset time after one without it (Claude app → Claude Code mod): same window
                (Some(None), Some(r)) => { self.limits.insert(key, Some(r)); false }
                (Some(_), None) => false,
            };
            if !new_window {
                if l.window == Window::Spend && l.used_pct >= 100.0 && self.spend_hit.insert(key) && !priming && self.settings.limits {
                    out.push(limit_toast(l, self.lang));
                }
                continue;
            }
            self.spend_hit.remove(&key);
            self.limits.insert(key, l.resets_at);
            if l.window == Window::Spend && l.used_pct >= 100.0 { self.spend_hit.insert(key); }
            if !priming && self.settings.limits { out.push(limit_toast(l, self.lang)); }
        }
        if !priming && self.settings.limits { self.early_warnings(snap, now, &mut out); }
        out
    }

    /// One toast per window that runs out within the hour, well before its reset; not when the 90% toast already said it.
    fn early_warnings(&mut self, snap: &Snapshot, now: i64, out: &mut Vec<Toast>) {
        for f in &snap.forecasts {
            let key = (f.agent, f.window);
            let Some(l) = snap.limits.iter().find(|l| (l.agent, l.window) == key && l.stale_since.is_none()) else { continue };
            let Some(resets_at) = l.resets_at else { continue };
            if l.used_pct < EARLY_MIN_PCT || f.runs_out_at - now > EARLY_WITHIN_MS || resets_at - f.runs_out_at < EARLY_GAP_MS || self.limits.contains_key(&key) { continue; }
            if self.early.get(&key).is_some_and(|known| (resets_at - known).abs() <= RESET_JITTER_MS) { continue; }
            self.early.insert(key, resets_at);
            out.push(limit_early_toast(f.agent, f.window, f.runs_out_at, resets_at, self.lang, self.clock));
        }
    }

    /// Forgets a window that was over 90% and says it is back; nothing for a window that never got there.
    fn limit_back(&mut self, key: (Agent, Window), priming: bool, out: &mut Vec<Toast>) {
        self.spend_hit.remove(&key);
        let was = self.limits.remove(&key);
        // a spend limit has no window: usage dropping means the limit was raised, not that a window reset
        let back = if key.1 == Window::Spend { was.flatten().is_some() } else { was.is_some() };
        if back && !priming && self.settings.limits { out.push(limit_back_toast(key.0, key.1, self.lang)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pets_core::model::*;

    fn sess(id: &str, state: State, since: i64, turn: Option<i64>) -> Session {
        Session { id: id.into(), agent: Agent::Claude, origin: Origin::Cli, title: format!("T-{id}"), cwd: String::new(),
            state, tool: None, progress: None, context: None, started_at: 0, last_activity: since, state_since: since,
            turn_started_at: turn, jump: JumpTarget::default(), router_task: None,
            parent: None, sub: None, action: None, question: None, waits_on_child: false, model: None, agent_name: None, usage: None, pinned: false, renamed: false }
    }
    fn snap(sessions: Vec<Session>, limits: Vec<Limit>) -> Snapshot { Snapshot { sessions, limits, ..Snapshot::default() } }
    use pets_core::pace::Forecast;
    fn utc_clock(ts: i64) -> String { pets_core::mute::clock(ts, |t| t - t.rem_euclid(86_400_000)) }
    fn rules() -> Rules { let mut r = Rules::new(ALL); r.clock = utc_clock; r }
    const MIN: i64 = 60_000;
    /// 2026-10-05 00:00 UTC.
    const DAY0: i64 = 20_730 * 86_400_000;
    fn at(h: i64, m: i64) -> i64 { DAY0 + h * 3_600_000 + m * MIN }
    fn lim5(pct: f32, reset: Option<i64>) -> Limit { Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: reset, stale_since: None } }
    fn fc(runs_out_at: i64) -> Forecast { Forecast { agent: Agent::Claude, window: Window::FiveHour, runs_out_at } }
    fn snap_fc(limits: Vec<Limit>, forecasts: Vec<Forecast>) -> Snapshot { Snapshot { limits, forecasts, ..Snapshot::default() } }
    const ALL: Settings = Settings { needs_you: true, done: true, limits: true };

    #[test]
    fn nothing_on_startup_even_if_everything_qualifies() {
        let mut r = Rules::new(ALL);
        let s = snap(vec![sess("a", State::NeedsYou, 0, None), sess("b", State::Done, 200_000, Some(0))],
            vec![Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: 95.0, resets_at: Some(9), stale_since: None }]);
        assert!(r.observe(&s, 300_000, &|_| false).is_empty());
        assert!(r.observe(&s, 301_000, &|_| false).is_empty(), "preexisting episodes do not appear later");
    }

    #[test]
    fn needs_you_after_15_s_unless_focused_and_only_once() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let s = snap(vec![sess("a", State::NeedsYou, 1_000, None)], vec![]);
        assert!(r.observe(&s, 10_000, &|_| false).is_empty());
        assert!(r.observe(&s, 20_000, &|_| true).is_empty(), "session window has focus");
        let t = r.observe(&s, 20_000, &|_| false);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].kind, t[0].session_id.as_deref()), (ToastKind::NeedsYou, Some("a")));
        assert!(r.observe(&s, 30_000, &|_| false).is_empty());
        let again = snap(vec![sess("a", State::NeedsYou, 50_000, None)], vec![]);
        assert_eq!(r.observe(&again, 70_000, &|_| false).len(), 1, "new episode");
    }

    #[test]
    fn a_long_wait_gets_one_reminder() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let s = snap(vec![sess("a", State::NeedsYou, 1_000, None)], vec![]);
        assert_eq!(r.observe(&s, 20_000, &|_| false).len(), 1);
        assert!(r.observe(&s, 300_000, &|_| false).is_empty(), "five minutes: nothing yet");
        assert!(r.observe(&s, 620_000, &|_| true).is_empty(), "the window has focus: no reminder");
        let t = r.observe(&s, 722_000, &|_| false);
        assert_eq!(t.iter().map(|t| (t.title.as_str(), t.body.as_str())).collect::<Vec<_>>(), [("Agent wciąż czeka", "T-a czeka na Ciebie od 12 min")]);
        assert!(r.observe(&s, 2_000_000, &|_| false).is_empty(), "only one reminder per episode");
        r.set_lang(Lang::En);
        let again = snap(vec![sess("a", State::NeedsYou, 3_000_000, None)], vec![]);
        r.observe(&again, 3_020_000, &|_| false);
        assert_eq!(r.observe(&again, 3_700_000, &|_| false)[0].body, "T-a has been waiting for you for 11 min");
    }

    #[test]
    fn no_reminder_for_an_episode_already_waiting_at_startup() {
        let mut r = Rules::new(ALL);
        let s = snap(vec![sess("a", State::NeedsYou, 0, None)], vec![]);
        r.observe(&s, 5_000, &|_| false);
        assert!(r.observe(&s, 900_000, &|_| false).is_empty());
    }

    #[test]
    fn tokens_read_compact_in_both_languages() {
        assert_eq!((compact(950, Lang::Pl), compact(12_400, Lang::Pl), compact(3_000_000, Lang::Pl)), ("950".into(), "12,4 tys.".into(), "3 mln".into()));
        assert_eq!((compact(12_400, Lang::En), compact(1_250_000_000, Lang::En)), ("12.4K".into(), "1.3B".into()));
    }

    #[test]
    fn done_only_after_long_turns() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        assert!(r.observe(&snap(vec![sess("a", State::Done, 60_000, Some(0))], vec![]), 61_000, &|_| false).is_empty());
        let t = r.observe(&snap(vec![sess("b", State::Done, 200_000, Some(0))], vec![]), 201_000, &|_| false);
        assert_eq!((t.len(), t[0].kind), (1, ToastKind::Done));
        assert_eq!(t[0].body, "T-b skończył po 3 min");
        let mut c = sess("c", State::Done, 400_000, Some(0));
        c.usage = Some(Usage { tokens: 2_340_000, cost: 0.0, account: None });
        assert_eq!(r.observe(&snap(vec![c.clone()], vec![]), 401_000, &|_| false)[0].body, "T-c skończył po 6 min · sesja: 2,3 mln tok.");
        let mut d = sess("d", State::Done, 400_000, Some(0));
        d.usage = Some(Usage { tokens: 0, cost: 1.416, account: Some(Agent::Claude) });
        assert_eq!(r.observe(&snap(vec![d], vec![]), 401_000, &|_| false)[0].body, "T-d skończył po 6 min · 1,42 $");
    }

    #[test]
    fn toasts_speak_english_when_asked() {
        let mut r = Rules::new(ALL);
        r.set_lang(Lang::En);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let t = r.observe(&snap(vec![sess("b", State::Done, 200_000, Some(0))], vec![]), 201_000, &|_| false);
        assert_eq!((t[0].title.as_str(), t[0].body.as_str()), ("Agent finished", "T-b finished after 3 min"));
        let l = r.observe(&snap(vec![], vec![Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: 95.0, resets_at: Some(9_000_000), stale_since: None }]), 202_000, &|_| false);
        assert_eq!((l[0].title.as_str(), l[0].body.as_str()), ("Codex: weekly limit", "95% of the weekly limit used"));
    }

    #[test]
    fn limit_once_per_window_until_reset() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = |pct: f32, reset: i64| Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: Some(reset), stale_since: None };
        const R1: i64 = 18_000_000;
        const R2: i64 = 36_000_000;
        assert!(r.observe(&snap(vec![], vec![l(80.0, R1)]), 1, &|_| false).is_empty());
        assert_eq!(r.observe(&snap(vec![], vec![l(91.0, R1)]), 2, &|_| false).len(), 1);
        assert!(r.observe(&snap(vec![], vec![l(95.0, R1)]), 3, &|_| false).is_empty());
        assert_eq!(r.observe(&snap(vec![], vec![l(92.0, R2)]), 4, &|_| false).len(), 1, "new window after reset");
    }

    #[test]
    fn an_old_reading_never_warns() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let old = Limit { agent: Agent::Claude, window: Window::Weekly, used_pct: 97.0, resets_at: None, stale_since: Some(1) };
        assert!(r.observe(&snap(vec![], vec![old]), 1, &|_| false).is_empty());
    }

    #[test]
    fn limit_without_reset_time_rearms_after_usage_drops() {
        // Claude app limits have no reset time: detect a new window when usage drops.
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = |pct: f32| Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: None, stale_since: None };
        assert_eq!(r.observe(&snap(vec![], vec![l(91.0)]), 1, &|_| false).len(), 1);
        assert!(r.observe(&snap(vec![], vec![l(89.0)]), 2, &|_| false).is_empty());
        assert!(r.observe(&snap(vec![], vec![l(92.0)]), 3, &|_| false).is_empty(), "fluctuation around 90% is still the same window");
        let back = r.observe(&snap(vec![], vec![l(3.0)]), 4, &|_| false);
        assert_eq!(back.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Claude: limit 5h odnowiony"], "usage dropped: the window reset");
        assert!(r.observe(&snap(vec![], vec![l(2.0)]), 5, &|_| false).is_empty(), "said once");
        assert_eq!(r.observe(&snap(vec![], vec![l(93.0)]), 6, &|_| false).len(), 1, "again after reset");
    }

    #[test]
    fn learning_the_reset_time_of_the_same_window_does_not_toast_again() {
        // claude app reports 91% without reset, then the Claude Code mod reports 92% with reset: still the same window
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = |pct: f32, reset: Option<i64>| Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: reset, stale_since: None };
        assert_eq!(r.observe(&snap(vec![], vec![l(91.0, None)]), 1, &|_| false).len(), 1);
        assert!(r.observe(&snap(vec![], vec![l(92.0, Some(18_000_000))]), 2, &|_| false).is_empty());
        assert!(r.observe(&snap(vec![], vec![l(93.0, Some(18_030_000))]), 3, &|_| false).is_empty(), "reset time fluctuates by seconds");
        assert_eq!(r.observe(&snap(vec![], vec![l(95.0, Some(36_000_000))]), 4, &|_| false).len(), 1, "next window");
    }

    #[test]
    fn spend_limit_warning_and_hit_use_spend_wording_in_both_languages() {
        for (lang, pct, title, body) in [
            (Lang::Pl, 90.0, "Claude: limit wydatków", "Zużyto 90% limitu wydatków"),
            (Lang::En, 112.0, "Claude: spend limit hit", "112% of the spend limit used"),
        ] {
            let mut r = Rules::new(ALL);
            r.set_lang(lang);
            r.observe(&snap(vec![], vec![]), 0, &|_| false);
            let l = Limit { agent: Agent::Claude, window: Window::Spend, used_pct: pct, resets_at: None, stale_since: None };
            let t = r.observe(&snap(vec![], vec![l]), 1, &|_| false);
            assert_eq!((t[0].title.as_str(), t[0].body.as_str()), (title, body));
            assert!(r.observe(&snap(vec![], vec![Limit { used_pct: 2.0, ..l }]), 2, &|_| false).is_empty(), "no reset toast for a spend limit without a reset time");
        }
        let mut r = Rules::new(ALL);
        r.set_lang(Lang::En);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = Limit { agent: Agent::Claude, window: Window::Spend, used_pct: 91.0, resets_at: None, stale_since: None };
        assert_eq!(r.observe(&snap(vec![], vec![l]), 1, &|_| false)[0].title, "Claude: spend limit");
        assert_eq!(r.observe(&snap(vec![], vec![Limit { used_pct: 112.0, ..l }]), 2, &|_| false)[0].title, "Claude: spend limit hit");
        assert!(r.observe(&snap(vec![], vec![Limit { used_pct: 113.0, ..l }]), 3, &|_| false).is_empty());
    }

    #[test]
    fn a_nearly_used_limit_says_when_its_window_resets() {
        let mut r = Rules::new(ALL);
        r.set_lang(Lang::En);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        const R1: i64 = 18_000_000;
        let l = |pct: f32, reset: i64| Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: Some(reset), stale_since: None };
        assert_eq!(r.observe(&snap(vec![], vec![l(95.0, R1)]), 1, &|_| false).len(), 1);
        assert!(r.observe(&snap(vec![], vec![l(95.0, R1)]), R1 - 1, &|_| false).is_empty());
        // the reset time passes before any new reading: the old reading must not warn again
        let back = r.observe(&snap(vec![], vec![l(95.0, R1)]), R1, &|_| false);
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].kind, back[0].title.as_str(), back[0].body.as_str()),
            (ToastKind::Limit, "Claude: 5h limit reset", "The 5h limit is available again"));
        assert!(r.observe(&snap(vec![], vec![l(95.0, R1)]), R1 + 1_000, &|_| false).is_empty(), "the old window stays quiet");
        assert!(r.observe(&snap(vec![], vec![l(4.0, 2 * R1)]), R1 + 2_000, &|_| false).is_empty(), "the new window starts low");
    }

    #[test]
    fn a_reset_is_said_even_when_the_reading_has_gone_stale() {
        // the agent was closed: its limit stops updating, but the reset time still comes
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let fresh = Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: 96.0, resets_at: Some(50_000), stale_since: None };
        assert_eq!(r.observe(&snap(vec![], vec![fresh]), 1, &|_| false).len(), 1);
        let stale = Limit { stale_since: Some(2), ..fresh };
        assert_eq!(r.observe(&snap(vec![], vec![stale]), 50_000, &|_| false)[0].title, "Codex: limit tygodniowy odnowiony");
    }

    #[test]
    fn no_reset_toast_for_a_limit_that_never_got_near() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = |pct: f32| Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: pct, resets_at: Some(10), stale_since: None };
        assert!(r.observe(&snap(vec![], vec![l(85.0)]), 1, &|_| false).is_empty());
        assert!(r.observe(&snap(vec![], vec![l(1.0)]), 20, &|_| false).is_empty());
    }

    #[test]
    fn a_limit_over_90_at_startup_still_says_when_it_resets_but_not_with_limits_off() {
        let mut r = Rules::new(ALL);
        let l = Limit { agent: Agent::Claude, window: Window::Weekly, used_pct: 99.0, resets_at: Some(1_000), stale_since: None };
        assert!(r.observe(&snap(vec![], vec![l]), 0, &|_| false).is_empty(), "startup is quiet");
        assert_eq!(r.observe(&snap(vec![], vec![]), 1_000, &|_| false).len(), 1);
        let mut off = Rules::new(Settings { needs_you: true, done: true, limits: false });
        off.observe(&snap(vec![], vec![l]), 0, &|_| false);
        assert!(off.observe(&snap(vec![], vec![]), 1_000, &|_| false).is_empty());
    }

    #[test]
    fn settings_changed_later_take_effect() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        r.set_settings(Settings { needs_you: false, done: true, limits: true });
        assert!(r.observe(&snap(vec![sess("a", State::NeedsYou, 0, None)], vec![]), 60_000, &|_| false).is_empty());
        r.set_settings(ALL);
        assert_eq!(r.observe(&snap(vec![sess("b", State::NeedsYou, 0, None)], vec![]), 60_000, &|_| false).len(), 1);
    }

    #[test]
    fn disabled_kinds_stay_silent() {
        let mut r = Rules::new(Settings { needs_you: false, done: true, limits: true });
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        assert!(r.observe(&snap(vec![sess("a", State::NeedsYou, 0, None)], vec![]), 60_000, &|_| false).is_empty());
    }

    #[test]
    fn children_never_toast() {
        let mut r = Rules::new(ALL);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let mut c = sess("c", State::Done, 200_000, Some(0));
        c.parent = Some("p".into());
        let mut n = sess("n", State::NeedsYou, 0, None);
        n.parent = Some("p".into());
        assert!(r.observe(&snap(vec![c, n], vec![]), 300_000, &|_| false).is_empty());
    }

    #[test]
    fn running_out_early_warns_once_with_both_times() {
        let mut r = rules();
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let now = at(14, 0);
        let s = snap_fc(vec![lim5(70.0, Some(at(18, 0)))], vec![fc(at(14, 40))]);
        let t = r.observe(&s, now, &|_| false);
        assert_eq!(t.iter().map(|t| (t.kind, t.title.as_str(), t.body.as_str())).collect::<Vec<_>>(),
            [(ToastKind::Limit, "Claude: limit 5h kończy się", "W tym tempie skończy się ok. 14:40, przed resetem o 18:00")]);
        assert!(r.observe(&s, now + MIN, &|_| false).is_empty(), "once per window");
        let jitter = snap_fc(vec![lim5(72.0, Some(at(18, 0) + 20_000))], vec![fc(at(14, 38))]);
        assert!(r.observe(&jitter, now + 2 * MIN, &|_| false).is_empty(), "the reset time wobbles by seconds");
    }

    #[test]
    fn the_early_warning_speaks_english() {
        let mut r = rules();
        r.set_lang(Lang::En);
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = Limit { agent: Agent::Codex, window: Window::Weekly, ..lim5(60.0, Some(at(20, 0))) };
        let f = Forecast { agent: Agent::Codex, window: Window::Weekly, runs_out_at: at(10, 30) };
        let t = r.observe(&snap_fc(vec![l], vec![f]), at(10, 0), &|_| false);
        assert_eq!((t[0].title.as_str(), t[0].body.as_str()), ("Codex: weekly limit running out", "At this pace it runs out around 10:30, before the 20:00 reset"));
        let mut far = rules();
        far.observe(&snap(vec![], vec![]), 0, &|_| false);
        let l = Limit { agent: Agent::Codex, window: Window::Weekly, ..lim5(60.0, Some(at(72, 0))) };
        let t = far.observe(&snap_fc(vec![l], vec![f]), at(10, 0), &|_| false);
        assert_eq!(t[0].body, "W tym tempie skończy się ok. 10:30, na długo przed resetem", "a reset days away has no bare time");
    }

    #[test]
    fn no_early_warning_at_startup_with_limits_off_or_when_the_conditions_fail() {
        let now = at(14, 0);
        let ok = |l: Limit, f: Forecast| snap_fc(vec![l], vec![f]);
        let mut first = rules();
        assert!(first.observe(&ok(lim5(70.0, Some(at(18, 0))), fc(at(14, 40))), now, &|_| false).is_empty(), "startup is quiet");
        let mut off = Rules::new(Settings { needs_you: true, done: true, limits: false });
        off.clock = utc_clock;
        off.observe(&snap(vec![], vec![]), 0, &|_| false);
        assert!(off.observe(&ok(lim5(70.0, Some(at(18, 0))), fc(at(14, 40))), now, &|_| false).is_empty(), "limits off");
        for (name, l, f) in [
            ("under 50%", lim5(49.0, Some(at(18, 0))), fc(at(14, 40))),
            ("more than an hour away", lim5(70.0, Some(at(18, 0))), fc(at(15, 1))),
            ("the reset comes first, or within 15 min of it", lim5(70.0, Some(at(14, 50))), fc(at(14, 40))),
            ("no reset time", lim5(70.0, None), fc(at(14, 40))),
            ("a stale reading", Limit { stale_since: Some(1), ..lim5(70.0, Some(at(18, 0))) }, fc(at(14, 40))),
        ] {
            let mut r = rules();
            r.observe(&snap(vec![], vec![]), 0, &|_| false);
            assert!(r.observe(&ok(l, f), now, &|_| false).is_empty(), "{name}");
        }
        let mut edge = rules();
        edge.observe(&snap(vec![], vec![]), 0, &|_| false);
        assert_eq!(edge.observe(&ok(lim5(50.0, Some(at(14, 55))), fc(at(14, 40))), now, &|_| false).len(), 1, "exactly 15 min before the reset still warns");
    }

    #[test]
    fn no_early_warning_after_the_90_percent_toast() {
        let mut r = rules();
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        let now = at(14, 0);
        let s = snap_fc(vec![lim5(92.0, Some(at(18, 0)))], vec![fc(at(14, 20))]);
        let t = r.observe(&s, now, &|_| false);
        assert_eq!(t.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Claude: limit 5h"], "only the 90% toast, in the same pass too");
        assert!(r.observe(&s, now + MIN, &|_| false).is_empty());
    }

    #[test]
    fn the_early_warning_is_rearmed_for_a_new_window() {
        let mut r = rules();
        r.observe(&snap(vec![], vec![]), 0, &|_| false);
        assert_eq!(r.observe(&snap_fc(vec![lim5(70.0, Some(at(18, 0)))], vec![fc(at(14, 40))]), at(14, 0), &|_| false).len(), 1);
        assert!(r.observe(&snap_fc(vec![lim5(3.0, Some(at(23, 0)))], vec![]), at(18, 5), &|_| false).is_empty());
        let next = snap_fc(vec![lim5(70.0, Some(at(23, 0)))], vec![fc(at(20, 30))]);
        assert_eq!(r.observe(&next, at(20, 0), &|_| false).len(), 1, "the next window warns again");
    }
}
