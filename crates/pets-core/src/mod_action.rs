//! Commands from the Claude Code mod (`/pets jump`, `/pets mute 1h`, …), sent to
//! `POST /v1/actions/claude-mod`. The endpoint thread parses them and hands each one with a reply
//! channel to the app, which carries it out and answers with a [`ModReply`].
use crate::model::Agent;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Sender;

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ModAction {
    /// Jump to the agent that needs you, other than the session asking.
    Jump { session_id: String },
    /// Open the panel on the asking session.
    Panel { session_id: String },
    /// `off`, `hour`, `morning` or `forever` (`mute::MuteChoice::parse`).
    Mute { choice: String },
    /// `None` or an empty name clears it.
    Rename { session_id: String, #[serde(default)] name: Option<String> },
    Pin { session_id: String, pinned: bool },
    /// Remove the pets of finished and idle sessions ("Clear inactive").
    Clear,
    /// The mod's two switches from Settings → Apps; a missing one stays as it is.
    Prefs { #[serde(default)] pet: Option<bool>, #[serde(default)] nudges: Option<bool> },
}

#[derive(Deserialize)]
struct Envelope {
    v: u32,
    #[serde(flatten)]
    action: ModAction,
}

/// A command body (`{"v":1,"action":"jump",…}`), `None` when it is not one this version knows.
pub fn parse(body: &[u8]) -> Option<ModAction> {
    serde_json::from_slice::<Envelope>(body).ok().filter(|e| e.v == 1).map(|e| e.action)
}

#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct ModReply {
    pub ok: bool,
    /// Session jumped to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Agent>,
    /// Muted until (ms; `i64::MAX` for "until turned back on").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Pets removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// `not_found`, `bad_choice`, … for the mod to put into words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ModReply {
    pub fn done() -> ModReply { ModReply { ok: true, ..Default::default() } }
    pub fn failed(error: &str) -> ModReply { ModReply { ok: false, error: Some(error.into()), ..Default::default() } }
}

/// Where the endpoint hands commands; no desk means the route is not there (404).
pub type Desk = Sender<(ModAction, Sender<ModReply>)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_commands_of_version_one() {
        assert_eq!(parse(br#"{"v":1,"action":"jump","session_id":"s"}"#), Some(ModAction::Jump { session_id: "s".into() }));
        assert_eq!(parse(br#"{"v":1,"action":"clear"}"#), Some(ModAction::Clear));
        assert_eq!(parse(br#"{"v":1,"action":"rename","session_id":"s"}"#), Some(ModAction::Rename { session_id: "s".into(), name: None }));
        assert_eq!(parse(br#"{"v":1,"action":"prefs","pet":true}"#), Some(ModAction::Prefs { pet: Some(true), nudges: None }));
        assert_eq!(parse(br#"{"v":2,"action":"clear"}"#), None, "unknown version");
        assert_eq!(parse(br#"{"action":"clear"}"#), None, "missing version");
        assert_eq!(parse(br#"{"v":1,"action":"format_disk"}"#), None);
        assert_eq!(parse(br#"{"v":1,"action":"pin","session_id":"s"}"#), None, "missing field");
    }

    #[test]
    fn replies_leave_out_what_they_do_not_say() {
        assert_eq!(serde_json::to_string(&ModReply::done()).unwrap(), r#"{"ok":true}"#);
        assert_eq!(serde_json::to_string(&ModReply::failed("not_found")).unwrap(), r#"{"ok":false,"error":"not_found"}"#);
        let r = ModReply { title: Some("web".into()), agent: Some(Agent::Codex), ..ModReply::done() };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"ok":true,"title":"web","agent":"codex"}"#);
    }
}
