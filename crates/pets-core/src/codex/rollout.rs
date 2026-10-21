use serde_json::{json, Value};
use crate::action::{action_text, question_text};
use crate::claude::hook::find_task_id;
use crate::i18n::{tr, Lang};
use crate::model::*;
use crate::time::rfc3339_ms;
use crate::tools::from_codex;

/// Text value of a `key:` field in JS code (`command: "…"`, `cmd: '…'`), with escape sequences decoded.
pub fn js_string_field(src: &str, keys: &[&str]) -> Option<String> {
    for key in keys {
        let mut from = 0;
        while let Some(i) = src[from..].find(key) {
            let at = from + i;
            from = at + key.len();
            let before_ok = src[..at].chars().next_back().map(|c| !(c.is_alphanumeric() || c == '_')).unwrap_or(true);
            let rest = src[from..].trim_start();
            let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix("\":")).map(str::trim_start);
            let Some(rest) = rest.filter(|_| before_ok) else { continue };
            let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else { continue };
            let mut out = String::new();
            let mut chars = rest[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some(o) => out.push(o),
                        None => break,
                    },
                    c if c == q => return Some(out),
                    c => out.push(c),
                }
            }
        }
    }
    None
}

const SHELLS: [&str; 4] = ["shell_command", "exec_command", "shell", "local_shell"];

/// Names of tools called in `exec` tool JS code, e.g. `tools.exec_command(`.
pub fn js_tool_calls(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("tools.") {
        rest = &rest[i + 6..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let after = rest[name.len()..].trim_start();
        if !name.is_empty() && after.starts_with('(') { out.push(name); }
    }
    out
}

pub(crate) const ASK: [&str; 3] = ["request_user_input", "request_user_input_async", "request_permissions"];
/// A question that does not block the tool: returns immediately, with the answer arriving as a new turn.
const ASYNC_ASK: &str = "request_user_input_async";

/// Identify a limit window by `window_minutes`; `primary`/`secondary` have no fixed meaning.
fn limits_from(rl: &Value) -> Vec<Limit> {
    ["primary", "secondary"].iter().filter_map(|k| {
        let w = rl.get(*k)?;
        // the free plan has a single monthly window (28–31 days) instead of 5 h and weekly
        let window = match w.get("window_minutes")?.as_u64()? { 300 => Window::FiveHour, 10080 => Window::Weekly, 40_320..=44_640 => Window::Monthly, _ => return None };
        Some(Limit {
            agent: Agent::Codex,
            window,
            used_pct: w.get("used_percent")?.as_f64()? as f32,
            resets_at: w.get("resets_at").and_then(|v| v.as_i64()).map(|s| s * 1000),
            stale_since: None,
        })
    }).collect()
}

/// Program from `session_meta.originator` (Codex app-server client name). Spike S2: t3code is `t3code_desktop`.
fn host(originator: &str, source: Option<&str>) -> (Origin, Option<App>, Option<String>) {
    let o = originator.trim().to_ascii_lowercase();
    if o == "codex-tui" || o == "codex_exec" || source == Some("cli") { return (Origin::Cli, Some(App::Terminal), None); }
    let app = match o.as_str() {
        // treat empty `originator` (old logs) as before: the Codex app
        "codex desktop" | "" => App::CodexApp,
        "codex_vscode" => App::Vscode,
        "t3code_desktop" => App::T3code,
        // Codex app variants (e.g. `codex_work_desktop`)
        x if x.starts_with("codex") && x.ends_with("desktop") => App::CodexApp,
        _ => return (Origin::Desktop, Some(App::Other), Some(originator.trim().chars().filter(|c| !c.is_control()).take(40).collect())),
    };
    (Origin::Desktop, Some(app), None)
}

pub struct RolloutParser {
    lang: Lang,
    sid: Option<String>,
    parent: Option<String>,
    skip: bool,
    router: bool,
    titled: bool,
    /// Question from `request_user_input_async` not answered yet. Codex keeps working meanwhile, so work shows as work,
    /// but a turn that ends before the answer is still waiting for you (the answer arrives as a message or a new turn).
    pending: Option<Option<String>>,
    /// Call of that question: its immediate "accepted" output is not the answer.
    ask_call: Option<String>,
    /// Current file line number.
    line: usize,
    /// Child thread with history copied from parent (`subagent_history_start_ordinal`): own lines from this point.
    skip_until: usize,
    /// Last sent model (`turn_context` arrives every turn).
    model: Option<String>,
}

impl Default for RolloutParser {
    fn default() -> Self { Self::with_lang(crate::i18n::system()) }
}

impl RolloutParser {
    pub fn new() -> Self { Self::default() }

    pub fn with_lang(lang: Lang) -> Self {
        RolloutParser { lang, sid: None, parent: None, skip: false, router: false, titled: false, pending: None, ask_call: None, line: 0, skip_until: 0, model: None }
    }

    fn ev(&self, kind: Kind, ts: i64) -> Option<Event> {
        let src = if self.router { Source::Router } else { Source::Codex };
        let mut e = Event::new(src, self.sid.clone()?, kind, ts);
        // every child event knows its parent: after disappearing, it returns next turn as a child, not a separate pet
        e.data.parent = self.parent.clone();
        Some(e)
    }

    fn one(&self, ts: i64, kind: Kind) -> Vec<Event> {
        self.ev(kind, ts).map(|e| vec![e]).unwrap_or_default()
    }

    fn tool_start(&self, tool: Tool, action: Option<String>, ts: i64) -> Vec<Event> {
        self.ev(Kind::ToolStart, ts).map(|mut e| { e.tool = Some(tool); e.data.action = action; vec![e] }).unwrap_or_default()
    }

    fn ask(&mut self, name: &str, call: Option<&str>, question: Option<String>, ts: i64) -> Vec<Event> {
        let question = if name == "request_permissions" { Some(tr(self.lang, "Zgoda?", "Allow?").to_string()) } else { question };
        if name == ASYNC_ASK {
            self.pending = Some(question.clone());
            self.ask_call = Some(call.unwrap_or("").to_string());
        }
        self.needs_input(question, ts)
    }

    fn needs_input(&self, question: Option<String>, ts: i64) -> Vec<Event> {
        self.ev(Kind::NeedsInput, ts).map(|mut e| { e.data.question = question; vec![e] }).unwrap_or_default()
    }

    /// Action text for a tool called from `exec` tool JS code.
    fn js_action(&self, call: &str, src: &str) -> Option<String> {
        if SHELLS.contains(&call) {
            let cmd = js_string_field(src, &["command", "cmd"])?;
            return action_text("shell_command", &json!({"command": cmd}), self.lang);
        }
        if call == "apply_patch" { return action_text("apply_patch", &Value::String(src.replace("\\n", "\n")), self.lang); }
        None
    }

    /// Ending a child's turn also clears the parent's "delegating" pose.
    fn with_parent_end(&self, mut out: Vec<Event>, ts: i64) -> Vec<Event> {
        if let Some(par) = &self.parent { out.push(Event::new(Source::Codex, par.clone(), Kind::ToolEnd, ts)); }
        out
    }

    pub fn parse_line(&mut self, line: &str) -> Vec<Event> {
        let index = self.line;
        self.line += 1;
        // parent history copied into the child thread file (with its `session_meta`) does not belong to the child
        if index < self.skip_until { return vec![]; }
        let Ok(d) = serde_json::from_str::<Value>(line) else { return vec![] };
        let Some(ts) = d.get("timestamp").and_then(|v| v.as_str()).and_then(rfc3339_ms) else { return vec![] };
        let ty = d.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let null = Value::Null;
        let p = d.get("payload").unwrap_or(&null);
        let pt = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let ps = |k: &str| p.get(k).and_then(|v| v.as_str());

        if ty == "session_meta" {
            // a file has one owner: later metadata is a copy from another thread
            if self.sid.is_some() || self.skip { return vec![]; }
            let id = ps("id").or(ps("session_id")).unwrap_or("").to_string();
            if ps("thread_source") == Some("subagent") {
                let spawn = p.pointer("/source/subagent/thread_spawn");
                let sp = |k: &str| spawn.and_then(|s| s.get(k)).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(String::from);
                self.parent = sp("parent_thread_id");
                let Some(par) = self.parent.clone() else {
                    // skip auxiliary threads without a parent (e.g. guardian)
                    self.skip = true;
                    return vec![];
                };
                self.sid = Some(id);
                self.skip_until = p.get("subagent_history_start_ordinal").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                // child name comes from metadata; the parent's message may be encrypted
                self.titled = true;
                let sub = SubInfo { kind: SubKind::Codex, agent_type: sp("agent_role"), description: sp("agent_nickname"), background: false };
                let mut e = self.ev(Kind::SessionStart, ts).unwrap();
                e.data.cwd = ps("cwd").map(String::from);
                e.data.parent = Some(par.clone());
                e.data.title = sub.description.clone().or_else(|| sub.agent_type.clone());
                e.data.sub = Some(sub);
                let mut d = Event::new(Source::Codex, par, Kind::ToolStart, ts);
                d.tool = Some(Tool::Agent);
                return vec![e, d];
            }
            let originator = ps("originator").unwrap_or("");
            self.router = originator == "agent-router";
            self.sid = Some(id);
            let (origin, app, app_name) = if self.router { (Origin::Router, None, None) } else { host(originator, ps("source")) };
            let mut e = self.ev(Kind::SessionStart, ts).unwrap();
            e.data.cwd = ps("cwd").map(String::from);
            e.data.origin = Some(origin);
            e.data.app = app;
            e.data.app_name = app_name;
            return vec![e];
        }

        if self.skip { return vec![]; }
        if self.sid.is_none() { return vec![]; }
        if self.pending.is_some() {
            let user_message = pt == "user_message" || p.pointer("/item/type").and_then(|v| v.as_str()) == Some("UserMessage");
            match (ty, pt) {
                // the question's own "accepted" output is not the answer
                ("response_item", "custom_tool_call_output") | ("response_item", "function_call_output")
                    if self.ask_call.as_deref().is_some_and(|c| c.is_empty() || ps("call_id") == Some(c)) => {
                    self.ask_call = None;
                    return vec![];
                }
                // the answer: a message during the turn, or a new turn
                ("event_msg", _) if user_message => {
                    (self.pending, self.ask_call) = (None, None);
                    return self.one(ts, Kind::Prompt);
                }
                ("event_msg", "task_started") | ("event_msg", "turn_aborted") => (self.pending, self.ask_call) = (None, None),
                // the turn ended before the answer: still waiting for you
                ("event_msg", "task_complete") => return self.needs_input(self.pending.clone().flatten(), ts),
                _ => {}
            }
        }

        match (ty, pt) {
            ("turn_context", _) => {
                let Some(m) = ps("model").map(|m| m.trim().chars().take(64).collect::<String>()).filter(|m| !m.is_empty()) else { return vec![] };
                if self.model.as_deref() == Some(&m) { return vec![]; }
                self.model = Some(m.clone());
                let mut e = self.ev(Kind::Meta, ts).unwrap();
                e.data.model = Some(m);
                vec![e]
            }
            ("event_msg", "task_started") => self.one(ts, Kind::Prompt),
            ("event_msg", "task_complete") | ("event_msg", "turn_aborted") => self.with_parent_end(self.one(ts, Kind::TurnEnd), ts),
            ("event_msg", "error") | ("event_msg", "stream_error") => self.one(ts, Kind::Error),
            ("event_msg", "token_count") => {
                let mut e = self.ev(Kind::Meta, ts).unwrap();
                if let (Some(used), Some(max)) = (
                    p.pointer("/info/last_token_usage/input_tokens").and_then(|v| v.as_u64()),
                    p.pointer("/info/model_context_window").and_then(|v| v.as_u64()),
                ) { e.data.context = Some(Context { used, max }); }
                if let Some(rl) = p.get("rate_limits") { e.data.limits = limits_from(rl); }
                if e.data.context.is_none() && e.data.limits.is_empty() { vec![] } else { vec![e] }
            }
            ("event_msg", "item_completed") => match p.pointer("/item/type").and_then(|v| v.as_str()) {
                Some("UserMessage") if !self.titled => {
                    let text = p.pointer("/item/content/0/text").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if text.is_empty() { return vec![]; }
                    self.titled = true;
                    let mut e = self.ev(Kind::Meta, ts).unwrap();
                    e.data.title = Some(text.chars().take(80).collect());
                    vec![e]
                }
                Some("ContextCompaction") => self.one(ts, Kind::Compact),
                Some("McpToolCall") if p.pointer("/item/server").and_then(|v| v.as_str())
                    .zip(p.pointer("/item/tool").and_then(|v| v.as_str()))
                    .is_some_and(|(server, tool)| crate::router::linking_tool(&format!("mcp__{server}__{tool}"))) => {
                    let Some(task) = p.pointer("/item/result").and_then(find_task_id) else { return vec![] };
                    let mut e = self.ev(Kind::Meta, ts).unwrap();
                    e.data.router_link = Some(task);
                    vec![e]
                }
                _ => vec![],
            },
            ("compacted", _) => self.one(ts, Kind::Compact),
            ("response_item", "custom_tool_call") => match ps("name") {
                Some("apply_patch") => {
                    let action = action_text("apply_patch", &Value::String(ps("input").unwrap_or("").to_string()), self.lang);
                    self.tool_start(Tool::Edit, action, ts)
                }
                Some("exec") => {
                    let src = ps("input").unwrap_or("");
                    let calls = js_tool_calls(src);
                    if let Some(ask) = calls.iter().find(|c| ASK.contains(&c.as_str())) {
                        let name = if calls.iter().any(|c| c == ASYNC_ASK) { ASYNC_ASK } else { ask.as_str() };
                        let q = js_string_field(src, &["question", "title"])
                            .and_then(|q| question_text(None, None, Some(&json!({"questions": [{"question": q}]})), self.lang));
                        return self.ask(name, ps("call_id"), q, ts);
                    }
                    match calls.iter().find_map(|c| from_codex(c).map(|t| (c, t))) {
                        Some((c, t)) => { let a = self.js_action(c, src); self.tool_start(t, a, ts) }
                        None => vec![],
                    }
                }
                _ => vec![],
            },
            ("response_item", "function_call") => {
                let name = ps("name").unwrap_or("");
                let ns = ps("namespace").unwrap_or("");
                let args: Value = serde_json::from_str(ps("arguments").unwrap_or("{}")).unwrap_or(Value::Null);
                if ASK.contains(&name) {
                    let q = question_text(None, None, Some(&args), self.lang);
                    return self.ask(name, ps("call_id"), q, ts);
                }
                if name == "update_plan" {
                    let Some(plan) = args.get("plan").and_then(|v| v.as_array()) else { return vec![] };
                    let done = plan.iter().filter(|s| s.get("status").and_then(|v| v.as_str()) == Some("completed")).count();
                    let mut e = self.ev(Kind::Meta, ts).unwrap();
                    e.data.progress = Some(Progress { done: done as u32, total: plan.len() as u32 });
                    return vec![e];
                }
                let (tool, action) = if ns.starts_with("mcp__") {
                    (Some(Tool::Mcp), action_text(&format!("{ns}__{name}"), &args, self.lang))
                } else {
                    (from_codex(name), action_text(name, &args, self.lang))
                };
                tool.map(|t| self.tool_start(t, action, ts)).unwrap_or_default()
            }
            ("response_item", "web_search_call") => self.tool_start(Tool::Web, None, ts),
            ("response_item", "custom_tool_call_output") | ("response_item", "function_call_output") => self.one(ts, Kind::ToolEnd),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TS: &str = "2026-09-23T19:25:19.386Z";
    fn l(ty: &str, payload: Value) -> String { json!({"timestamp": TS, "type": ty, "payload": payload}).to_string() }
    fn meta(originator: &str, source: Value, thread_source: Value) -> String {
        l("session_meta", json!({"id": "t1", "cwd": "C:\\p", "originator": originator, "source": source, "thread_source": thread_source}))
    }
    fn started(p: &mut RolloutParser) { p.parse_line(&meta("Codex Desktop", json!("vscode"), json!("user"))); }

    #[test]
    fn desktop_session_meta() {
        let mut p = RolloutParser::new();
        let e = p.parse_line(&meta("Codex Desktop", json!("vscode"), json!("user")));
        assert_eq!((e[0].kind, e[0].source, e[0].session_id.as_str()), (Kind::SessionStart, Source::Codex, "t1"));
        assert_eq!((e[0].data.origin, e[0].data.app), (Some(Origin::Desktop), Some(App::CodexApp)));
        assert_eq!(e[0].data.cwd.as_deref(), Some("C:\\p"));
        assert_eq!(e[0].ts, 1_790_191_519_386);
    }

    #[test]
    fn router_and_cli_origins() {
        let mut p = RolloutParser::new();
        let e = p.parse_line(&meta("agent-router", json!("vscode"), Value::Null));
        assert_eq!((e[0].source, e[0].data.origin), (Source::Router, Some(Origin::Router)));
        let mut p = RolloutParser::new();
        let e = p.parse_line(&meta("codex-tui", json!("cli"), json!("user")));
        assert_eq!((e[0].data.origin, e[0].data.app), (Some(Origin::Cli), Some(App::Terminal)));
    }

    #[test]
    fn originator_names_the_program() {
        let host = |o: &str, src: Value| {
            let mut p = RolloutParser::new();
            let e = p.parse_line(&meta(o, src, json!("user")));
            (e[0].data.origin, e[0].data.app, e[0].data.app_name.clone())
        };
        assert_eq!(host("codex-tui", json!("cli")), (Some(Origin::Cli), Some(App::Terminal), None));
        assert_eq!(host("codex_exec", json!("exec")), (Some(Origin::Cli), Some(App::Terminal), None));
        assert_eq!(host("whatever", json!("cli")), (Some(Origin::Cli), Some(App::Terminal), None));
        assert_eq!(host("Codex Desktop", json!("vscode")), (Some(Origin::Desktop), Some(App::CodexApp), None));
        assert_eq!(host("codex desktop", json!("vscode")), (Some(Origin::Desktop), Some(App::CodexApp), None));
        assert_eq!(host("codex_vscode", json!("vscode")), (Some(Origin::Desktop), Some(App::Vscode), None));
        // Codex app variants seen in real logs
        assert_eq!(host("codex_work_desktop", json!("vscode")), (Some(Origin::Desktop), Some(App::CodexApp), None));
        assert_eq!(host("t3code_desktop", json!("vscode")), (Some(Origin::Desktop), Some(App::T3code), None));
        let long = "x".repeat(60);
        assert_eq!(host("zed_codex", json!("vscode")), (Some(Origin::Desktop), Some(App::Other), Some("zed_codex".into())));
        assert_eq!(host(&long, json!("vscode")).2.map(|n| n.chars().count()), Some(40));
    }

    #[test]
    fn turn_context_names_the_model() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let e = p.parse_line(&l("turn_context", json!({"model": "gpt-6-sol", "cwd": "C:\\p"})));
        assert_eq!((e[0].kind, e[0].data.model.as_deref()), (Kind::Meta, Some("gpt-6-sol")));
        assert!(p.parse_line(&l("turn_context", json!({"model": "gpt-6-sol"}))).is_empty(), "no new event without a model change");
        assert!(p.parse_line(&l("turn_context", json!({}))).is_empty());
    }

    fn child(p: &mut RolloutParser) -> Vec<Event> {
        let src = json!({"subagent": {"thread_spawn": {"parent_thread_id": "parent1", "depth": 1, "agent_nickname": "Newton", "agent_role": "reviewer"}}});
        p.parse_line(&meta("Codex Desktop", src, json!("subagent")))
    }

    #[test]
    fn a_subagent_thread_is_a_child_of_its_parent_and_the_parent_delegates() {
        let mut p = RolloutParser::with_lang(Lang::Pl);
        let e = child(&mut p);
        let start = e.iter().find(|e| e.session_id == "t1").unwrap();
        assert_eq!(start.kind, Kind::SessionStart);
        assert_eq!(start.data.parent.as_deref(), Some("parent1"));
        assert_eq!(start.data.sub, Some(SubInfo { kind: SubKind::Codex, agent_type: Some("reviewer".into()), description: Some("Newton".into()), background: false }));
        assert_eq!(start.data.title.as_deref(), Some("Newton"));
        let par = e.iter().find(|e| e.session_id == "parent1").unwrap();
        assert_eq!((par.kind, par.tool), (Kind::ToolStart, Some(Tool::Agent)));
        let e = p.parse_line(&l("event_msg", json!({"type": "task_started"})));
        assert_eq!((e[0].session_id.as_str(), e[0].kind), ("t1", Kind::Prompt));
        assert_eq!(e[0].data.parent.as_deref(), Some("parent1"), "every child event knows its parent (e.g. after another turn)");
        let patch = "*** Begin Patch\n*** Update File: src/a.rs\n*** End Patch";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "apply_patch", "input": patch})));
        assert_eq!((e[0].session_id.as_str(), e[0].kind, e[0].data.action.as_deref()), ("t1", Kind::ToolStart, Some("Edytuje a.rs")));
        let um = p.parse_line(&l("event_msg", json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": "zadanie od rodzica"}]}})));
        assert!(um.is_empty(), "child name comes from metadata, not the message");
        let e = p.parse_line(&l("event_msg", json!({"type": "task_complete"})));
        let kinds: Vec<(&str, Kind)> = e.iter().map(|e| (e.session_id.as_str(), e.kind)).collect();
        assert_eq!(kinds, vec![("t1", Kind::TurnEnd), ("parent1", Kind::ToolEnd)]);
    }

    #[test]
    fn guardian_subagent_without_parent_is_ignored() {
        let mut p = RolloutParser::new();
        assert!(p.parse_line(&meta("Codex Desktop", json!({"subagent": {"other": "guardian"}}), json!("subagent"))).is_empty());
        assert!(p.parse_line(&l("event_msg", json!({"type": "task_started"}))).is_empty());
    }

    #[test]
    fn turn_lifecycle() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let k = |p: &mut RolloutParser, t: &str| p.parse_line(&l("event_msg", json!({"type": t})))[0].kind;
        assert_eq!(k(&mut p, "task_started"), Kind::Prompt);
        assert_eq!(k(&mut p, "task_complete"), Kind::TurnEnd);
        assert_eq!(k(&mut p, "turn_aborted"), Kind::TurnEnd);
        assert_eq!(k(&mut p, "error"), Kind::Error);
    }

    #[test]
    fn exec_js_input_picks_first_meaningful_tool() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let input = "await tools.update_plan({plan:[]}); const r = await tools.exec_command({cmd:\"ls\"})";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": input})));
        assert_eq!((e[0].kind, e[0].tool), (Kind::ToolStart, Some(Tool::Bash)));
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": "tools.apply_patch(x)"})));
        assert_eq!(e[0].tool, Some(Tool::Edit));
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": "tools.request_user_input({})"})));
        assert_eq!(e[0].kind, Kind::NeedsInput);
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call_output", "output": []})));
        assert_eq!(e[0].kind, Kind::ToolEnd);
    }

    #[test]
    fn function_calls() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let fc = |p: &mut RolloutParser, v: Value| p.parse_line(&l("response_item", v));
        let e = fc(&mut p, json!({"type": "function_call", "name": "js", "namespace": "mcp__node_repl", "arguments": "{}"}));
        assert_eq!(e[0].tool, Some(Tool::Mcp));
        let e = fc(&mut p, json!({"type": "function_call", "name": "run", "namespace": "web", "arguments": "{}"}));
        assert_eq!(e[0].tool, Some(Tool::Web));
        let e = fc(&mut p, json!({"type": "function_call", "name": "request_user_input", "arguments": "{}"}));
        assert_eq!(e[0].kind, Kind::NeedsInput);
        let e = fc(&mut p, json!({"type": "function_call", "name": "update_plan",
            "arguments": "{\"plan\":[{\"step\":\"a\",\"status\":\"completed\"},{\"step\":\"b\",\"status\":\"in_progress\"}]}"}));
        assert_eq!((e[0].kind, e[0].data.progress), (Kind::Meta, Some(Progress { done: 1, total: 2 })));
        assert!(fc(&mut p, json!({"type": "function_call", "name": "sleep", "namespace": "clock", "arguments": "{}"})).is_empty());
    }

    #[test]
    fn async_question_keeps_waiting_until_the_user_answers() {
        // `request_user_input_async` returns "accepted" immediately and the turn ends; the answer arrives as a new turn.
        let mut p = RolloutParser::new();
        started(&mut p);
        let kinds = |e: Vec<Event>| e.into_iter().map(|e| e.kind).collect::<Vec<_>>();
        let e = p.parse_line(&l("response_item", json!({"type": "function_call", "name": "request_user_input_async", "arguments": "{}"})));
        assert_eq!(kinds(e), vec![Kind::NeedsInput]);
        assert!(p.parse_line(&l("response_item", json!({"type": "function_call_output", "output": "{\"accepted\":true}"}))).is_empty());
        assert_eq!(kinds(p.parse_line(&l("event_msg", json!({"type": "task_complete"})))), vec![Kind::NeedsInput], "still waiting for an answer");
        assert_eq!(kinds(p.parse_line(&l("event_msg", json!({"type": "task_started"})))), vec![Kind::Prompt]);
        assert_eq!(kinds(p.parse_line(&l("event_msg", json!({"type": "task_complete"})))), vec![Kind::TurnEnd]);
    }

    #[test]
    fn work_after_an_async_question_shows_as_work_and_the_answer_ends_the_wait() {
        // Codex keeps working in the same turn after `request_user_input_async`; the answer arrives during the turn
        let mut p = RolloutParser::with_lang(Lang::Pl);
        started(&mut p);
        let kinds = |e: Vec<Event>| e.into_iter().map(|e| e.kind).collect::<Vec<_>>();
        let ask = json!({"type": "function_call", "name": "request_user_input_async", "call_id": "q1",
            "arguments": "{\"questions\":[{\"question\":\"Który wariant?\"}]}"});
        assert_eq!(kinds(p.parse_line(&l("response_item", ask))), vec![Kind::NeedsInput]);
        assert!(p.parse_line(&l("response_item", json!({"type": "function_call_output", "call_id": "q1", "output": "{\"accepted\":true}"}))).is_empty(),
            "the immediate acceptance is not an answer");
        let sh = json!({"type": "function_call", "name": "shell", "call_id": "c2", "arguments": "{\"command\":[\"ls\"]}"});
        assert_eq!(kinds(p.parse_line(&l("response_item", sh))), vec![Kind::ToolStart], "independent work shows as work");
        assert_eq!(kinds(p.parse_line(&l("response_item", json!({"type": "function_call_output", "call_id": "c2", "output": "ok"})))), vec![Kind::ToolEnd]);
        let answer = json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": "A"}]}});
        assert_eq!(kinds(p.parse_line(&l("event_msg", answer))), vec![Kind::Prompt], "the answer brings it back to work");
        let patch = json!({"type": "custom_tool_call", "name": "apply_patch", "call_id": "c3", "input": "*** Begin Patch\n*** Update File: a.rs\n*** End Patch"});
        assert_eq!(kinds(p.parse_line(&l("response_item", patch))), vec![Kind::ToolStart]);
        assert_eq!(kinds(p.parse_line(&l("event_msg", json!({"type": "task_complete"})))), vec![Kind::TurnEnd], "answered: the turn ends as done");
    }

    #[test]
    fn a_turn_ending_before_the_answer_asks_again_with_the_question() {
        let mut p = RolloutParser::with_lang(Lang::Pl);
        started(&mut p);
        p.parse_line(&l("response_item", json!({"type": "function_call", "name": "request_user_input_async", "call_id": "q1",
            "arguments": "{\"questions\":[{\"question\":\"Który wariant?\"}]}"})));
        p.parse_line(&l("response_item", json!({"type": "function_call", "name": "shell", "call_id": "c2", "arguments": "{\"command\":[\"ls\"]}"})));
        let e = p.parse_line(&l("event_msg", json!({"type": "task_complete"})));
        assert_eq!((e[0].kind, e[0].data.question.as_deref()), (Kind::NeedsInput, Some("Pytanie: Który wariant?")));
        let e = p.parse_line(&l("event_msg", json!({"type": "user_message", "message": "A"})));
        assert_eq!(e[0].kind, Kind::Prompt, "an answer as a user message ends the wait too");
        assert!(p.parse_line(&l("event_msg", json!({"type": "user_message", "message": "B"}))).is_empty(), "later messages are not answers");
    }

    #[test]
    fn token_count_gives_context_and_limits() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let e = p.parse_line(&l("event_msg", json!({"type": "token_count",
            "info": {"last_token_usage": {"input_tokens": 28370}, "model_context_window": 258400},
            "rate_limits": {"primary": {"used_percent": 12.5, "window_minutes": 300, "resets_at": 1790209519},
                            "secondary": {"used_percent": 6.0, "window_minutes": 10080, "resets_at": 1790711013}}})));
        assert_eq!(e[0].data.context, Some(Context { used: 28370, max: 258400 }));
        assert_eq!(e[0].data.limits, vec![
            Limit { agent: Agent::Codex, window: Window::FiveHour, used_pct: 12.5, resets_at: Some(1_790_209_519_000), stale_since: None },
            Limit { agent: Agent::Codex, window: Window::Weekly, used_pct: 6.0, resets_at: Some(1_790_711_013_000), stale_since: None }]);
    }

    #[test]
    fn primary_window_is_identified_by_minutes_not_name() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let e = p.parse_line(&l("event_msg", json!({"type": "token_count",
            "rate_limits": {"primary": {"used_percent": 71.0, "window_minutes": 10080, "resets_at": 1}}})));
        assert_eq!(e[0].data.limits[0].window, Window::Weekly);
    }

    #[test]
    fn the_free_plan_has_a_monthly_window() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let e = p.parse_line(&l("event_msg", json!({"type": "token_count",
            "rate_limits": {"primary": {"used_percent": 4.0, "window_minutes": 43200, "resets_at": 1}, "secondary": null}})));
        assert_eq!((e[0].data.limits.len(), e[0].data.limits[0].window, e[0].data.limits[0].used_pct), (1, Window::Monthly, 4.0));
    }

    #[test]
    fn user_message_titles_once_and_compaction() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let um = |t: &str| l("event_msg", json!({"type": "item_completed", "item": {"type": "UserMessage", "content": [{"type": "text", "text": t}]}}));
        assert_eq!(p.parse_line(&um("Pierwszy"))[0].data.title.as_deref(), Some("Pierwszy"));
        assert!(p.parse_line(&um("Drugi")).is_empty());
        let e = p.parse_line(&l("event_msg", json!({"type": "item_completed", "item": {"type": "ContextCompaction"}})));
        assert_eq!(e[0].kind, Kind::Compact);
    }

    #[test]
    fn js_tool_call_scanner() {
        assert_eq!(js_tool_calls("a tools.exec_command ({}) tools.x tools.apply_patch("), vec!["exec_command", "apply_patch"]);
    }

    #[test]
    fn real_rollout_fixtures_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for f in rd.flatten() {
            let mut p = RolloutParser::new();
            let text = std::fs::read_to_string(f.path()).unwrap();
            let evs: Vec<Event> = text.lines().flat_map(|x| p.parse_line(x)).collect();
            assert!(evs.iter().any(|e| e.kind == Kind::SessionStart), "missing SessionStart in {:?}", f.path());
            assert!(evs.iter().any(|e| e.kind == Kind::Prompt), "missing Prompt in {:?}", f.path());
        }
    }

    #[test]
    fn shell_calls_carry_the_command() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let e = p.parse_line(&l("response_item", json!({"type": "function_call", "name": "shell",
            "arguments": "{\"command\":[\"bash\",\"-lc\",\"ls -la\"]}"})));
        assert_eq!((e[0].tool, e[0].data.action.as_deref()), (Some(Tool::Bash), Some("ls -la")));
        let js = "const r = await tools.shell_command({ command: \"cargo test -p \\\"core\\\"\", workdir: \"x\" });";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": js})));
        assert_eq!((e[0].tool, e[0].data.action.as_deref()), (Some(Tool::Bash), Some("cargo test -p \"core\"")));
        let js = "await tools.exec_command({cmd: 'git status'})";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": js})));
        assert_eq!(e[0].data.action.as_deref(), Some("git status"));
    }

    #[test]
    fn patches_name_their_files() {
        let mut p = RolloutParser::with_lang(Lang::Pl);
        started(&mut p);
        let patch = "*** Begin Patch\n*** Update File: C:\\p\\a.ts\n@@\n*** Add File: b.ts\n+x\n*** End Patch";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "apply_patch", "input": patch})));
        assert_eq!(e[0].data.action.as_deref(), Some("Edytuje a.ts +1"));
        let js = "await tools.apply_patch(\"*** Begin Patch\\n*** Update File: src/x.rs\\n@@\\n*** End Patch\")";
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": js})));
        assert_eq!((e[0].tool, e[0].data.action.as_deref()), (Some(Tool::Edit), Some("Edytuje x.rs")));
        let e = p.parse_line(&l("response_item", json!({"type": "custom_tool_call", "name": "exec", "input": "await tools.view_image({path: 'a.png'})"})));
        assert_eq!((e[0].tool, e[0].data.action.as_deref()), (Some(Tool::Read), None), "tool without text");
    }

    #[test]
    fn questions_from_codex() {
        let mut p = RolloutParser::with_lang(Lang::Pl);
        started(&mut p);
        let e = p.parse_line(&l("response_item", json!({"type": "function_call", "name": "request_user_input",
            "arguments": "{\"questions\":[{\"title\":\"Który wariant?\"}]}"})));
        assert_eq!((e[0].kind, e[0].data.question.as_deref()), (Kind::NeedsInput, Some("Pytanie: Który wariant?")));
        let e = p.parse_line(&l("response_item", json!({"type": "function_call", "name": "request_permissions", "arguments": "{}"})));
        assert_eq!((e[0].kind, e[0].data.question.as_deref()), (Kind::NeedsInput, Some("Zgoda?")));
    }

    #[test]
    fn a_router_call_from_codex_links_the_task() {
        let mut p = RolloutParser::new();
        started(&mut p);
        let item = json!({"type": "McpToolCall", "server": "agent-router", "tool": "codex_delegate", "arguments": {},
            "result": {"content": [{"type": "text", "text": "{\"status\":\"running\",\"taskId\":\"codex-1\"}"}]}});
        let e = p.parse_line(&l("event_msg", json!({"type": "item_completed", "item": item})));
        assert_eq!((e[0].session_id.as_str(), e[0].kind, e[0].data.router_link.as_deref()), ("t1", Kind::Meta, Some("codex-1")));
        let alternate = json!({"type": "McpToolCall", "server": "router", "tool": "codex_review", "result": {"taskId": "codex-2"}});
        let e = p.parse_line(&l("event_msg", json!({"type": "item_completed", "item": alternate})));
        assert_eq!(e[0].data.router_link.as_deref(), Some("codex-2"));
        let other = json!({"type": "McpToolCall", "server": "cua_repl", "tool": "js", "result": {"content": [{"type": "text", "text": "{\"taskId\":\"x\"}"}]}});
        assert!(p.parse_line(&l("event_msg", json!({"type": "item_completed", "item": other}))).is_empty());
        let e = p.parse_line(&l("response_item", json!({"type": "function_call", "name": "codex_task_status", "namespace": "mcp__agent-router", "arguments": "{}"})));
        assert_eq!((e[0].tool, e[0].data.action.as_deref()), (Some(Tool::Mcp), Some("agent-router: codex_task_status")));
    }

    #[test]
    fn a_forked_subagent_skips_the_copied_parent_history() {
        // Codex Desktop 09-2026: child file starts with its metadata, then a copy of parent metadata and history
        let mut p = RolloutParser::new();
        let meta = l("session_meta", json!({"id": "c1", "session_id": "par", "forked_from_id": "par", "thread_source": "subagent",
            "cwd": "C:\\p", "originator": "Codex Desktop", "subagent_history_start_ordinal": 4,
            "source": {"subagent": {"thread_spawn": {"parent_thread_id": "par", "depth": 1, "agent_nickname": "Newton"}}}}));
        let mut evs = p.parse_line(&meta);
        evs.extend(p.parse_line(&l("session_meta", json!({"id": "par", "cwd": "C:\\p", "originator": "Codex Desktop", "source": "vscode", "thread_source": "user"}))));
        evs.extend(p.parse_line(&l("event_msg", json!({"type": "task_started"}))));
        evs.extend(p.parse_line(&l("event_msg", json!({"type": "task_complete"}))));
        let own = p.parse_line(&l("event_msg", json!({"type": "task_started"})));
        assert!(evs.iter().all(|e| e.session_id == "c1" || (e.session_id == "par" && e.kind == Kind::ToolStart)),
            "copied history does not change the parent: {:?}", evs.iter().map(|e| (&e.session_id, e.kind)).collect::<Vec<_>>());
        assert!(!evs.iter().any(|e| e.session_id == "c1" && e.kind == Kind::TurnEnd), "copied parent turns do not belong to the child");
        assert_eq!((own[0].session_id.as_str(), own[0].kind, own[0].data.parent.as_deref()), ("c1", Kind::Prompt, Some("par")));
    }
}
