//! The Claude mod's commands (`/pets jump`, `/pets mute 1h`, …), handed over by `POST /v1/actions/claude-mod`.
//! Each one is answered at once; a jump, which can take a while, goes on after the answer.
use crate::{core, hotkeys, panel, settings};
use pets_core::mod_action::{Desk, ModAction, ModReply};
use pets_core::mute::MuteChoice;
use std::sync::mpsc::Sender;
use tauri::{AppHandle, Manager};

/// The desk for `RuntimeConfig::desk`; its clerk thread ends with the endpoint.
pub fn desk(app: &AppHandle) -> Desk {
    let (tx, rx) = std::sync::mpsc::channel::<(ModAction, Sender<ModReply>)>();
    let app = app.clone();
    std::thread::spawn(move || {
        for (action, reply) in rx { let _ = reply.send(carry_out(&app, action)); }
    });
    tx
}

fn carry_out(app: &AppHandle, action: ModAction) -> ModReply {
    let (sessions, now) = { let s = app.state::<core::Shared>(); let g = s.lock().unwrap(); (g.sessions.clone(), g.now) };
    let known = |id: &str| sessions.iter().any(|s| s.id == id);
    let to_core = |msg: core::CoreMsg| { let _ = app.state::<core::Control>().0.lock().unwrap().send(msg); };
    match action {
        ModAction::Jump { session_id } => {
            // the session asking is where the person already is
            let others: Vec<_> = sessions.iter().filter(|s| s.id != session_id).cloned().collect();
            match hotkeys::pick_target(&others, now) {
                hotkeys::Target::Session(id) => {
                    let s = others.iter().find(|s| s.id == id);
                    let reply = ModReply { title: s.map(|s| s.title.clone()), agent: s.map(|s| s.agent), ..ModReply::done() };
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let r = crate::jump_to(&app, &id);
                        if r.needs_attention() { panel::open_with_status(&app, Some(id), r.detail); }
                    });
                    reply
                }
                hotkeys::Target::Panel => { panel::open(app, None); ModReply::done() }
            }
        }
        ModAction::Panel { session_id } => {
            panel::open(app, known(&session_id).then_some(session_id));
            ModReply::done()
        }
        ModAction::Mute { choice } => match MuteChoice::parse(&choice) {
            None => ModReply::failed("bad_choice"),
            Some(c) => match settings::mute(app, c) {
                Ok(until) => ModReply { until, ..ModReply::done() },
                Err(e) => { pets_core::app_log!("mod mute: {e}"); ModReply::failed("save_failed") }
            },
        },
        ModAction::Rename { session_id, name } => {
            if !known(&session_id) { return ModReply::failed("not_found"); }
            let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
            to_core(core::CoreMsg::Rename(session_id, name));
            ModReply::done()
        }
        ModAction::Pin { session_id, pinned } => {
            if !known(&session_id) { return ModReply::failed("not_found"); }
            to_core(core::CoreMsg::Pin(session_id, pinned));
            ModReply::done()
        }
        ModAction::Clear => ModReply { count: Some(crate::dismiss_inactive(app).len()), ..ModReply::done() },
        ModAction::Prefs { pet, nudges } => {
            let saved = settings::update(app, |s| {
                if let Some(p) = pet { s.claude_mod_pet = p; }
                if let Some(n) = nudges { s.claude_mod_nudges = n; }
            });
            match saved {
                Ok(()) => ModReply::done(),
                Err(e) => { pets_core::app_log!("mod prefs: {e}"); ModReply::failed("save_failed") }
            }
        }
    }
}
