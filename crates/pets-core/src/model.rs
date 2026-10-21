use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Agent { Claude, Codex, Opencode, Antigravity, Copilot, Cursor, Grok, Zcode, Other }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin { Cli, Desktop, Router }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum App { Terminal, ClaudeDesktop, CodexApp, Vscode, T3code, Cursor, Antigravity, Zed, Jetbrains, Zcode, Other }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State { Thinking, Working, NeedsYou, Done, Error, Idle, Sleep, Compacting, Ended }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tool { Edit, Bash, Read, Grep, Web, Agent, Mcp, Other }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress { pub done: u32, pub total: u32 }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context { pub used: u64, pub max: u64 }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct JumpTarget {
    pub pid: Option<u32>,
    pub session_id: String,
    pub cwd: String,
    pub app: Option<App>,
    /// `App::Other` program name (e.g. a Codex `originator` outside the map).
    #[serde(default)]
    pub app_name: Option<String>,
    /// Host program PID (VS Code, t3code…) when different from the agent process.
    #[serde(default)]
    pub host_pid: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub origin: Origin,
    pub title: String,
    pub cwd: String,
    pub state: State,
    pub tool: Option<Tool>,
    pub progress: Option<Progress>,
    pub context: Option<Context>,
    pub started_at: i64,
    pub last_activity: i64,
    /// Time of entering the current state (for minimum state duration and thresholds).
    pub state_since: i64,
    pub turn_started_at: Option<i64>,
    pub jump: JumpTarget,
    #[serde(default)]
    pub router_task: Option<RouterTask>,
    /// Parent session ID; `None` = ordinary session.
    #[serde(default)]
    pub parent: Option<String>,
    /// Only for children (subagents).
    #[serde(default)]
    pub sub: Option<SubInfo>,
    /// Current action text (in memory only), empty outside tool work.
    #[serde(default)]
    pub action: Option<String>,
    /// Question text (in memory only), only in `needs_you`.
    #[serde(default)]
    pub question: Option<String>,
    /// A parent's `needs_you` is its subagent's request: further child work means it has been answered.
    #[serde(skip)]
    pub waits_on_child: bool,
    /// Model ID used by the agent (e.g. `claude-opus-5-5`); the UI computes the display name.
    #[serde(default)]
    pub model: Option<String>,
    /// Display name for an `Other` agent (from the door).
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Session usage from the agent database (since 0.11, opencode only).
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Pinned by the user in the panel (set from `labels`, never by the store).
    #[serde(default)]
    pub pinned: bool,
    /// `title` is the user's own name (set from `labels`); the panel offers to reset it.
    #[serde(default)]
    pub renamed: bool,
}

/// Tokens and cost for one session; `account` = account whose limits apply to it (Claude or ChatGPT subscription).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Usage { pub tokens: u64, pub cost: f64, pub account: Option<Agent> }

/// Agent's daily total (since local midnight) across all its sessions.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct AgentUsage { pub agent: Agent, pub tokens_today: u64, pub cost_today: f64 }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubKind { Claude, Codex, Router, Opencode, Copilot, Cursor }

/// Child description: who started it and why.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SubInfo {
    pub kind: SubKind,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Background task (`requestShape: background`): ends only via `SubagentStop` or after 10 min of quiet.
    #[serde(default)]
    pub background: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Window { FiveHour, Weekly, Spend, Monthly }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Limit {
    pub agent: Agent,
    pub window: Window,
    pub used_pct: f32,
    pub resets_at: Option<i64>,
    /// Time of the last real reading when the value is old (Claude app stopped polling): shown as "as of", never as live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source { Claude, Codex, Router, Opencode, Generic, Copilot, Antigravity, Cursor, Grok, Zcode }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SessionStart, Prompt, ToolStart, ToolEnd, NeedsInput, TurnEnd,
    Error, Compact, SessionEnd, Meta, Limits,
}

/// Optional data carried by an event. Empty fields do not overwrite session state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct EventData {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub origin: Option<Origin>,
    pub progress: Option<Progress>,
    pub context: Option<Context>,
    pub limits: Vec<Limit>,
    pub pid: Option<u32>,
    pub app: Option<App>,
    pub router_task: Option<RouterTask>,
    pub parent: Option<String>,
    pub sub: Option<SubInfo>,
    /// Action text at `ToolStart` (absent = tool without text).
    pub action: Option<String>,
    /// Question text at `NeedsInput`.
    pub question: Option<String>,
    /// `taskId` of an Agent Router task requested by this session (`codex_delegate`/`continue`/`review` result).
    pub router_link: Option<String>,
    /// `SubagentStop` without `agent_id`: end the newest live child of this session.
    pub sub_end: bool,
    /// Request (`NeedsInput`) reported by a subagent, waiting at the parent.
    pub from_child: bool,
    pub model: Option<String>,
    pub agent_name: Option<String>,
    pub app_name: Option<String>,
    pub host_pid: Option<u32>,
}

/// Agent Router task linked to a Codex thread (`~/.agent-router/status.json`).
/// The UI computes "health" (active, quiet, stuck, blocked) from `last_activity_at` when rendering.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RouterTask {
    pub task_id: String,
    /// `pending`, `running`, `completed`, `failed`, `interrupted`, `quota_exhausted`
    pub status: String,
    pub last_activity_at: Option<i64>,
    pub blocked: bool,
    pub stall_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub source: Source,
    pub session_id: String,
    pub kind: Kind,
    pub tool: Option<Tool>,
    pub ts: i64,
    #[serde(default)]
    pub data: EventData,
}

impl Event {
    pub fn new(source: Source, session_id: impl Into<String>, kind: Kind, ts: i64) -> Self {
        Event { source, session_id: session_id.into(), kind, tool: None, ts, data: EventData::default() }
    }
    pub fn agent(&self) -> Agent {
        match self.source {
            Source::Claude => Agent::Claude,
            Source::Codex | Source::Router => Agent::Codex,
            Source::Opencode => Agent::Opencode,
            Source::Generic => Agent::Other,
            Source::Copilot => Agent::Copilot,
            Source::Antigravity => Agent::Antigravity,
            Source::Cursor => Agent::Cursor,
            Source::Grok => Agent::Grok,
            Source::Zcode => Agent::Zcode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_0_7_session_loads_without_the_new_fields() {
        let v = serde_json::json!({"id": "s", "agent": "claude", "origin": "cli", "title": "", "cwd": "", "state": "thinking",
            "tool": null, "progress": null, "context": null, "started_at": 1, "last_activity": 2, "state_since": 2,
            "turn_started_at": null, "jump": {"pid": null, "session_id": "s", "cwd": "", "app": null}});
        let s: Session = serde_json::from_value(v).unwrap();
        assert_eq!((s.parent, s.sub, s.action, s.question), (None, None, None, None));
    }

    #[test]
    fn a_0_10_session_has_no_usage() {
        let v = serde_json::json!({"id": "s", "agent": "opencode", "origin": "cli", "title": "", "cwd": "", "state": "thinking",
            "tool": null, "progress": null, "context": null, "started_at": 1, "last_activity": 2, "state_since": 2,
            "turn_started_at": null, "jump": {"pid": null, "session_id": "s", "cwd": "", "app": null}, "model": "gpt-6-sol"});
        let s: Session = serde_json::from_value(v).unwrap();
        assert_eq!(s.usage, None);
        let u = Usage { tokens: 3, cost: 0.25, account: Some(Agent::Claude) };
        assert_eq!(serde_json::to_value(u).unwrap(), serde_json::json!({"tokens": 3, "cost": 0.25, "account": "claude"}));
    }

    #[test]
    fn a_0_9_session_has_no_model_agent_name_or_host() {
        let v = serde_json::json!({"id": "s", "agent": "codex", "origin": "cli", "title": "", "cwd": "", "state": "thinking",
            "tool": null, "progress": null, "context": null, "started_at": 1, "last_activity": 2, "state_since": 2,
            "turn_started_at": null, "jump": {"pid": null, "session_id": "s", "cwd": "", "app": "vscode"}});
        let s: Session = serde_json::from_value(v).unwrap();
        assert_eq!((s.model, s.agent_name, s.jump.app_name, s.jump.host_pid), (None, None, None, None));
    }

    #[test]
    fn new_variants_round_trip_in_snake_case() {
        for (a, j) in [(Agent::Opencode, "opencode"), (Agent::Antigravity, "antigravity"), (Agent::Copilot, "copilot"),
                       (Agent::Cursor, "cursor"), (Agent::Grok, "grok"), (Agent::Other, "other")] {
            assert_eq!(serde_json::to_value(a).unwrap(), j);
            assert_eq!(serde_json::from_value::<Agent>(j.into()).unwrap(), a);
        }
        for (a, j) in [(App::T3code, "t3code"), (App::Cursor, "cursor"), (App::Antigravity, "antigravity"),
                       (App::Zed, "zed"), (App::Jetbrains, "jetbrains"), (App::Other, "other")] {
            assert_eq!(serde_json::to_value(a).unwrap(), j);
            assert_eq!(serde_json::from_value::<App>(j.into()).unwrap(), a);
        }
        for (x, j) in [(Source::Opencode, "opencode"), (Source::Generic, "generic")] {
            assert_eq!(serde_json::to_value(x).unwrap(), j);
            assert_eq!(serde_json::from_value::<Source>(j.into()).unwrap(), x);
        }
    }

    #[test]
    fn opencode_and_door_events_belong_to_their_agents() {
        assert_eq!(Event::new(Source::Opencode, "opencode:s", Kind::Prompt, 1).agent(), Agent::Opencode);
        assert_eq!(Event::new(Source::Generic, "generic:kilo:s", Kind::Prompt, 1).agent(), Agent::Other);
        assert_eq!(Event::new(Source::Router, "r", Kind::Prompt, 1).agent(), Agent::Codex);
    }

    #[test]
    fn copilot_and_antigravity_events_belong_to_their_agents() {
        assert_eq!(Event::new(Source::Copilot, "copilot:s", Kind::Prompt, 1).agent(), Agent::Copilot);
        assert_eq!(Event::new(Source::Antigravity, "antigravity:s", Kind::Prompt, 1).agent(), Agent::Antigravity);
        for (x, j) in [(Source::Copilot, "copilot"), (Source::Antigravity, "antigravity")] {
            assert_eq!(serde_json::to_value(x).unwrap(), j);
        }
        assert_eq!(serde_json::to_value(SubKind::Copilot).unwrap(), "copilot");
    }

    #[test]
    fn cursor_grok_and_zcode_events_belong_to_their_agents() {
        for (x, a, j) in [(Source::Cursor, Agent::Cursor, "cursor"), (Source::Grok, Agent::Grok, "grok"), (Source::Zcode, Agent::Zcode, "zcode")] {
            assert_eq!(Event::new(x, "s", Kind::Prompt, 1).agent(), a);
            assert_eq!(serde_json::to_value(x).unwrap(), j);
            assert_eq!(serde_json::to_value(a).unwrap(), j);
            assert_eq!(serde_json::from_value::<Agent>(j.into()).unwrap(), a);
        }
        assert_eq!(serde_json::to_value(SubKind::Cursor).unwrap(), "cursor");
        assert_eq!(serde_json::from_value::<App>("zcode".into()).unwrap(), App::Zcode);
    }

    #[test]
    fn sub_info_is_snake_case() {
        let i = SubInfo { kind: SubKind::Router, agent_type: None, description: Some("Tytuł".into()), background: true };
        let v = serde_json::to_value(&i).unwrap();
        assert_eq!(v["kind"], "router");
        assert_eq!(serde_json::from_value::<SubInfo>(v).unwrap(), i);
    }
}
