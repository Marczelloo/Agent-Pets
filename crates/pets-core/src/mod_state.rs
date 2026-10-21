//! State board for the Claude mod: the sessions and limits the widget shows, rendered as the JSON
//! that `GET /v1/state` serves. The board is built from private structs holding only the safe
//! fields, so nothing else (transcript paths, pids, tokens, usage, router tasks) can leak into it.
//! Of the settings it carries only the mod's two switches, as plain booleans.
use crate::model::{Agent, Limit, Session, State, Window};
use serde::Serialize;

#[derive(Serialize)]
struct Board<'a> {
    v: u32,
    app_version: &'a str,
    prefs: ModPrefs,
    sessions: Vec<BoardSession<'a>>,
    limits: Vec<BoardLimit>,
}

/// The mod's two switches, set in the app's settings; the mod reads them off the board.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct ModPrefs {
    /// The pixel pet above the Claude Code prompt.
    pub pet: bool,
    /// Nudges about other agents.
    pub nudges: bool,
}

impl Default for ModPrefs {
    fn default() -> Self { ModPrefs { pet: false, nudges: true } }
}

#[derive(Serialize)]
struct BoardSession<'a> {
    id: &'a str,
    agent: Agent,
    state: &'static str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<&'a str>,
    cwd: &'a str,
    since: i64,
}

#[derive(Serialize)]
struct BoardLimit {
    agent: Agent,
    window: &'static str,
    used_pct: f32,
    resets_at: Option<i64>,
    stale_since: Option<i64>,
}

/// `None` for `Ended`: such a session is not part of the board.
fn state_name(s: State) -> Option<&'static str> {
    Some(match s {
        State::Thinking => "thinking",
        State::Working => "working",
        State::NeedsYou => "needs_input",
        State::Done => "done",
        State::Error => "error",
        State::Idle => "idle",
        State::Sleep => "sleeping",
        State::Compacting => "compacting",
        State::Ended => return None,
    })
}

fn window_name(w: Window) -> &'static str {
    match w { Window::FiveHour => "five_hour", Window::Weekly => "weekly", Window::Spend => "spend", Window::Monthly => "monthly" }
}

pub fn render(sessions: &[Session], limits: &[Limit], prefs: ModPrefs, app_version: &str) -> Vec<u8> {
    let board = Board {
        v: 1,
        app_version,
        prefs,
        sessions: sessions.iter().filter_map(|s| Some(BoardSession {
            id: &s.id,
            agent: s.agent,
            state: state_name(s.state)?,
            title: &s.title,
            question: if s.state == State::NeedsYou { s.question.as_deref() } else { None },
            cwd: &s.cwd,
            since: s.state_since,
        })).collect(),
        limits: limits.iter().map(|l| BoardLimit {
            agent: l.agent,
            window: window_name(l.window),
            used_pct: if l.used_pct.is_nan() { 0.0 } else if l.window == Window::Spend { l.used_pct.max(0.0) } else { l.used_pct.clamp(0.0, 100.0) },
            resets_at: l.resets_at,
            stale_since: l.stale_since,
        }).collect(),
    };
    serde_json::to_vec(&board).expect("the state board serializes")
}

pub fn empty(app_version: &str) -> Vec<u8> { render(&[], &[], ModPrefs::default(), app_version) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn session(id: &str, state: State) -> Session {
        Session {
            id: id.into(), agent: Agent::Codex, origin: Origin::Cli, title: "Fix the build".into(),
            cwd: "C:/work/app".into(), state, tool: Some(Tool::Bash), progress: Some(Progress { done: 1, total: 2 }),
            context: Some(Context { used: 10, max: 100 }), started_at: 1, last_activity: 2, state_since: 1_234,
            turn_started_at: Some(5),
            jump: JumpTarget { pid: Some(424_242), session_id: id.into(), cwd: "C:/work/app".into(),
                app: Some(App::Terminal), app_name: None, host_pid: Some(777_001) },
            router_task: Some(RouterTask { task_id: "task-secret-1".into(), status: "running".into(), ..Default::default() }),
            parent: None, sub: None, action: Some("secret action".into()), question: None, waits_on_child: false,
            model: Some("gpt-6-sol".into()), agent_name: None,
            usage: Some(Usage { tokens: 987_654, cost: 1.5, account: Some(Agent::Claude) }),
            pinned: false, renamed: false,
        }
    }

    fn parse(b: &[u8]) -> serde_json::Value { serde_json::from_slice(b).unwrap() }

    #[test]
    fn a_session_exposes_only_the_safe_fields() {
        let mut s = session("c1", State::Working);
        // A transcript path can reach a session only through the jump target; plant it there.
        s.jump.session_id = "C:/Users/me/.claude/projects/x/TRANSCRIPT-9.jsonl".into();
        let bytes = render(&[s], &[], ModPrefs::default(), "0.16.0");
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("TRANSCRIPT-9"), "{text}");
        assert!(!text.contains("424242") && !text.contains("777001"), "{text}");
        assert!(!text.contains("task-secret-1") && !text.contains("987654") && !text.contains("secret action"), "{text}");
        let v = parse(&bytes);
        assert_eq!((v["v"].as_u64(), v["app_version"].as_str()), (Some(1), Some("0.16.0")));
        let obj = v["sessions"][0].as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["agent", "cwd", "id", "since", "state", "title"]);
        assert_eq!((obj["agent"].as_str(), obj["state"].as_str(), obj["since"].as_i64()), (Some("codex"), Some("working"), Some(1_234)));
    }

    #[test]
    fn a_session_waiting_for_the_user_carries_its_question() {
        let mut s = session("c1", State::NeedsYou);
        s.question = Some("Run the migration?".into());
        let v = parse(&render(&[s], &[], ModPrefs::default(), "0.16.0"));
        let obj = v["sessions"][0].as_object().unwrap();
        assert_eq!(obj["state"], "needs_input");
        assert_eq!(obj["question"], "Run the migration?");
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["agent", "cwd", "id", "question", "since", "state", "title"]);
    }

    #[test]
    fn a_stale_question_is_not_served_outside_needs_input() {
        let mut s = session("c1", State::Working);
        s.question = Some("old question".into());
        let v = parse(&render(&[s], &[], ModPrefs::default(), "0.16.0"));
        assert!(v["sessions"][0].get("question").is_none());
    }

    #[test]
    fn ended_sessions_are_left_out() {
        let v = parse(&render(&[session("a", State::Ended), session("b", State::Idle)], &[], ModPrefs::default(), "0.16.0"));
        let ids: Vec<&str> = v["sessions"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["b"]);
    }

    #[test]
    fn every_live_state_has_its_wire_name() {
        for (st, name) in [(State::Thinking, "thinking"), (State::Working, "working"), (State::NeedsYou, "needs_input"),
            (State::Done, "done"), (State::Error, "error"), (State::Idle, "idle"), (State::Sleep, "sleeping"),
            (State::Compacting, "compacting")] {
            let v = parse(&render(&[session("a", st)], &[], ModPrefs::default(), "0.16.0"));
            assert_eq!(v["sessions"][0]["state"], name);
        }
    }

    #[test]
    fn limits_are_clamped_and_named() {
        let limits = [
            Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: 130.0, resets_at: Some(9_000), stale_since: None },
            Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: -5.0, resets_at: None, stale_since: Some(8_000) },
            Limit { agent: Agent::Claude, window: Window::Spend, used_pct: 112.0, resets_at: None, stale_since: None },
        ];
        let v = parse(&render(&[], &limits, ModPrefs::default(), "0.16.0"));
        let l = v["limits"].as_array().unwrap();
        assert_eq!((l[0]["agent"].as_str(), l[0]["window"].as_str(), l[0]["used_pct"].as_f64()), (Some("claude"), Some("five_hour"), Some(100.0)));
        assert_eq!((l[0]["resets_at"].as_i64(), l[0]["stale_since"].is_null()), (Some(9_000), true));
        assert_eq!((l[1]["window"].as_str(), l[1]["used_pct"].as_f64()), (Some("weekly"), Some(0.0)));
        assert_eq!((l[2]["window"].as_str(), l[2]["used_pct"].as_f64()), (Some("spend"), Some(112.0)));
        let nan = [Limit { agent: Agent::Claude, window: Window::FiveHour, used_pct: f32::NAN, resets_at: None, stale_since: None }];
        let v: serde_json::Value = serde_json::from_slice(&render(&[], &nan, ModPrefs::default(), "0.16.0")).unwrap();
        assert_eq!(v["limits"][0]["used_pct"].as_f64(), Some(0.0));
        assert_eq!((l[1]["resets_at"].is_null(), l[1]["stale_since"].as_i64()), (true, Some(8_000)));
    }

    #[test]
    fn the_empty_board_has_empty_arrays() {
        let v = parse(&empty("0.16.0"));
        assert_eq!(v["app_version"], "0.16.0");
        assert!(v["sessions"].as_array().unwrap().is_empty());
        assert!(v["limits"].as_array().unwrap().is_empty());
        assert_eq!(v["prefs"], serde_json::json!({"pet": false, "nudges": true}));
    }

    #[test]
    fn the_switches_ride_the_board_as_two_booleans() {
        let v = parse(&render(&[], &[], ModPrefs { pet: true, nudges: false }, "0.16.0"));
        assert_eq!(v["prefs"], serde_json::json!({"pet": true, "nudges": false}));
    }
}
