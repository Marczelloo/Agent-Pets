use std::io::Read;
use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use crate::claude::{plugin::ModPayload, HookEnvelope};
use crate::endpoint::Endpoint;
use crate::mod_action::{Desk, ModReply};

#[allow(clippy::large_enum_variant)] // short-lived values passed by move; boxing would only add noise
pub enum Incoming {
    ClaudeHook(HookEnvelope),
    /// Payload from the Claude Code mod (`claude::plugin::to_update`).
    ClaudeMod(ModPayload),
    /// Event from the door, already validated (`adapters::generic::to_event`).
    Generic(crate::model::Event),
    /// koperta pluginu opencode (`adapters::opencode::events`)
    Opencode(serde_json::Value),
    /// `hook.exe --agent copilot` (`adapters::copilot::events`)
    Copilot(crate::adapters::AgentEnvelope),
    /// `hook.exe --agent antigravity` (`adapters::antigravity::events`)
    Antigravity(crate::adapters::AgentEnvelope),
    /// `hook.exe --agent cursor` (`adapters::cursor::events`)
    Cursor(crate::adapters::AgentEnvelope),
    /// `hook.exe --agent grok` (`adapters::grok::events`)
    Grok(crate::adapters::AgentEnvelope),
    /// `hook.exe --agent zcode` (`adapters::zcode::events`)
    Zcode(crate::adapters::AgentEnvelope),
}

/// Routes enabled live from settings; a closed route appears absent (404).
#[derive(Debug, Default)]
pub struct Doors {
    pub generic: AtomicBool, pub copilot: AtomicBool, pub antigravity: AtomicBool,
    pub cursor: AtomicBool, pub grok: AtomicBool, pub zcode: AtomicBool,
}

impl Doors {
    pub fn new(apps: &crate::settings::Apps) -> Doors {
        let d = Doors::default();
        d.set(apps);
        d
    }
    pub fn set(&self, apps: &crate::settings::Apps) {
        self.generic.store(apps.generic, Ordering::Relaxed);
        self.copilot.store(apps.copilot, Ordering::Relaxed);
        self.antigravity.store(apps.antigravity, Ordering::Relaxed);
        self.cursor.store(apps.cursor, Ordering::Relaxed);
        self.grok.store(apps.grok, Ordering::Relaxed);
        self.zcode.store(apps.zcode, Ordering::Relaxed);
    }
}

/// JSON served by `GET /v1/state`; the runtime keeps it current, the endpoint thread only reads it.
pub type StateBoard = Arc<RwLock<Vec<u8>>>;

pub struct Ingest {
    endpoint: Endpoint,
    server: Arc<tiny_http::Server>,
    handle: Option<JoinHandle<()>>,
    stopped: std::sync::mpsc::Receiver<()>,
}

const MAX_BODY: u64 = 1 << 20;

impl Ingest {
    /// `doors`: which settings-controlled routes are open (door, Copilot, Antigravity); changed live.
    /// `board`: bytes returned by `GET /v1/state` (read on each request).
    pub fn start(token: String, tx: Sender<Incoming>, doors: Arc<Doors>, board: StateBoard) -> anyhow::Result<Ingest> {
        Self::start_with_desk(token, tx, doors, board, None)
    }

    /// `desk`: where `POST /v1/actions/claude-mod` hands the mod's commands; without one the route is absent.
    pub fn start_with_desk(token: String, tx: Sender<Incoming>, doors: Arc<Doors>, board: StateBoard, desk: Option<Desk>) -> anyhow::Result<Ingest> {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!(e))?);
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| anyhow::anyhow!("no port"))?;
        let endpoint = Endpoint { port, token: token.clone() };
        let srv = server.clone();
        let (stopped_tx, stopped) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let expected = format!("Bearer {token}");
            for mut req in srv.incoming_requests() {
                let (status, body) = handle_request(&mut req, &expected, &tx, &doors, &board, desk.as_ref());
                let _ = match body {
                    Some(b) => {
                        let json = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header");
                        req.respond(tiny_http::Response::from_data(b).with_status_code(status).with_header(json))
                    }
                    None => req.respond(tiny_http::Response::empty(status)),
                };
            }
            let _ = stopped_tx.send(());
        });
        Ok(Ingest { endpoint, server, handle: Some(handle), stopped })
    }

    pub fn endpoint(&self) -> &Endpoint { &self.endpoint }

    pub fn stop(self) {}
}

impl Drop for Ingest {
    fn drop(&mut self) {
        self.server.unblock();
        if self.stopped.recv_timeout(std::time::Duration::from_secs(10)).is_ok() {
            if let Some(h) = self.handle.take() { let _ = h.join(); }
        }
    }
}

enum Route { Claude, ClaudeMod, Opencode, Generic, Copilot, Antigravity, Cursor, Grok, Zcode }

fn authorized(req: &tiny_http::Request, expected: &str) -> bool {
    let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string());
    auth.as_deref() == Some(expected)
}

fn handle_request(req: &mut tiny_http::Request, expected: &str, tx: &Sender<Incoming>, doors: &Doors, board: &StateBoard, desk: Option<&Desk>) -> (u16, Option<Vec<u8>>) {
    if *req.method() == tiny_http::Method::Get && req.url() == "/v1/state" {
        if !authorized(req, expected) { return (401, None); }
        return (200, Some(board.read().unwrap_or_else(|e| e.into_inner()).clone()));
    }
    if *req.method() == tiny_http::Method::Post && req.url() == "/v1/actions/claude-mod" {
        return match desk { Some(d) => handle_action(req, expected, d), None => (404, None) };
    }
    (handle_post(req, expected, tx, doors), None)
}

/// How long a command may take before the mod hears 503; the app answers at once and does slow work (a jump) afterwards.
const ACTION_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

fn handle_action(req: &mut tiny_http::Request, expected: &str, desk: &Desk) -> (u16, Option<Vec<u8>>) {
    if !authorized(req, expected) { return (401, None); }
    let body = match read_body(req) { Ok(b) => b, Err(code) => return (code, None) };
    let Some(action) = crate::mod_action::parse(&body) else { return (400, None) };
    let (reply_tx, reply) = std::sync::mpsc::channel::<ModReply>();
    if desk.send((action, reply_tx)).is_err() { return (503, None); }
    match reply.recv_timeout(ACTION_WAIT) {
        Ok(r) => (200, serde_json::to_vec(&r).ok()),
        Err(_) => (503, None),
    }
}

fn read_body(req: &mut tiny_http::Request) -> Result<Vec<u8>, u16> {
    if req.body_length().map(|l| l as u64 > MAX_BODY).unwrap_or(false) { return Err(413); }
    let mut body = Vec::new();
    if req.as_reader().take(MAX_BODY + 1).read_to_end(&mut body).is_err() { return Err(400); }
    if body.len() as u64 > MAX_BODY { return Err(413); }
    Ok(body)
}

fn handle_post(req: &mut tiny_http::Request, expected: &str, tx: &Sender<Incoming>, doors: &Doors) -> u16 {
    if *req.method() != tiny_http::Method::Post { return 404; }
    let route = match req.url() {
        "/v1/events/claude" => Route::Claude,
        // not door-gated: the runtime checks `apps.claude_code`, as for hooks
        "/v1/events/claude-mod" => Route::ClaudeMod,
        "/v1/events/opencode" => Route::Opencode,
        // a disabled door or integration appears as a missing route
        "/v1/events/generic" if doors.generic.load(Ordering::Relaxed) => Route::Generic,
        "/v1/events/copilot" if doors.copilot.load(Ordering::Relaxed) => Route::Copilot,
        "/v1/events/antigravity" if doors.antigravity.load(Ordering::Relaxed) => Route::Antigravity,
        "/v1/events/cursor" if doors.cursor.load(Ordering::Relaxed) => Route::Cursor,
        "/v1/events/grok" if doors.grok.load(Ordering::Relaxed) => Route::Grok,
        "/v1/events/zcode" if doors.zcode.load(Ordering::Relaxed) => Route::Zcode,
        _ => return 404,
    };
    if !authorized(req, expected) { return 401; }
    let body = match read_body(req) { Ok(b) => b, Err(code) => return code };
    let msg = match route {
        Route::ClaudeMod => serde_json::from_slice::<ModPayload>(&body).ok().filter(|p| p.v == 1).map(Incoming::ClaudeMod),
        Route::Claude => serde_json::from_slice::<HookEnvelope>(&body).ok().map(Incoming::ClaudeHook),
        Route::Opencode => serde_json::from_slice::<serde_json::Value>(&body).ok().filter(|v| v.is_object()).map(Incoming::Opencode),
        Route::Generic => serde_json::from_slice::<crate::adapters::generic::GenericEvent>(&body).ok()
            .and_then(|g| crate::adapters::generic::to_event(g, crate::time::now_ms()).ok()).map(Incoming::Generic),
        Route::Copilot => serde_json::from_slice::<crate::adapters::AgentEnvelope>(&body).ok().map(Incoming::Copilot),
        Route::Antigravity => serde_json::from_slice::<crate::adapters::AgentEnvelope>(&body).ok().map(Incoming::Antigravity),
        Route::Cursor => serde_json::from_slice::<crate::adapters::AgentEnvelope>(&body).ok().map(Incoming::Cursor),
        Route::Grok => serde_json::from_slice::<crate::adapters::AgentEnvelope>(&body).ok().map(Incoming::Grok),
        Route::Zcode => serde_json::from_slice::<crate::adapters::AgentEnvelope>(&body).ok().map(Incoming::Zcode),
    };
    match msg {
        Some(m) => { let _ = tx.send(m); 204 }
        None => 400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn post(port: u16, path: &str, token: &str, body: &str) -> u16 {
        match ureq::post(&format!("http://127.0.0.1:{port}{path}"))
            .set("Authorization", &format!("Bearer {token}"))
            .timeout(Duration::from_secs(2))
            .send_string(body) {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(c, _)) => c,
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn accepts_valid_hook_and_rejects_bad_requests() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(true), board()).unwrap();
        let port = ing.endpoint().port;
        let body = r#"{"ts":1,"ppid":5,"payload":{"hook_event_name":"Stop","session_id":"s"}}"#;
        assert_eq!(post(port, "/v1/events/claude", "secret", body), 204);
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            Incoming::ClaudeHook(h) => assert_eq!(h.ppid, Some(5)),
            _ => panic!("wrong route"),
        }
        assert_eq!(post(port, "/v1/events/claude", "wrong", body), 401);
        assert_eq!(post(port, "/nope", "secret", body), 404);
        assert_eq!(post(port, "/v1/events/claude", "secret", "{bad"), 400);
        ing.stop();
    }

    fn board() -> StateBoard { Arc::new(RwLock::new(Vec::new())) }

    fn get(port: u16, path: &str, token: &str) -> (u16, String) {
        match ureq::get(&format!("http://127.0.0.1:{port}{path}"))
            .set("Authorization", &format!("Bearer {token}"))
            .timeout(Duration::from_secs(2))
            .call() {
            Ok(r) => (r.status(), r.into_string().unwrap()),
            Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    }

    fn door(on: bool) -> Arc<Doors> { Arc::new(Doors::new(&crate::settings::Apps { generic: on, ..Default::default() })) }

    #[test]
    fn the_door_accepts_a_valid_event() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(true), board()).unwrap();
        let port = ing.endpoint().port;
        let body = r#"{"agent":"kilo","name":"Kilo CLI","session":"abc","state":"working","tool":"edit"}"#;
        assert_eq!(post(port, "/v1/events/generic", "secret", body), 204);
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            Incoming::Generic(e) => {
                assert_eq!((e.agent(), e.session_id.as_str()), (crate::model::Agent::Other, "generic:kilo:abc"));
                assert_eq!(e.data.agent_name.as_deref(), Some("Kilo CLI"));
            }
            _ => panic!("wrong route"),
        }
        ing.stop();
    }

    #[test]
    fn the_door_rejects_impostors_bad_ids_and_oversized_bodies() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(true), board()).unwrap();
        let port = ing.endpoint().port;
        let ev = |agent: &str, session: &str| format!(r#"{{"agent":"{agent}","session":"{session}","state":"done"}}"#);
        assert_eq!(post(port, "/v1/events/generic", "secret", &ev("claude", "abc")), 400);
        assert_eq!(post(port, "/v1/events/generic", "secret", &ev("Kilo CLI", "abc")), 400);
        assert_eq!(post(port, "/v1/events/generic", "secret", &ev("kilo", "../x")), 400);
        assert_eq!(post(port, "/v1/events/generic", "secret", r#"{"agent":"kilo","session":"a","state":"nope"}"#), 400);
        assert_eq!(post(port, "/v1/events/generic", "wrong", &ev("kilo", "abc")), 401);
        let big = format!(r#"{{"agent":"kilo","session":"abc","state":"done","title":"{}"}}"#, "x".repeat(1 << 20));
        assert_eq!(post(port, "/v1/events/generic", "secret", &big), 413);
        assert!(rx.try_recv().is_err(), "nothing reached the channel");
        ing.stop();
    }

    #[test]
    fn opencode_envelopes_have_their_own_route() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(false), board()).unwrap();
        let port = ing.endpoint().port;
        let body = r#"{"v":1,"event":"session.status","session":"ses_1","status":"busy"}"#;
        assert_eq!(post(port, "/v1/events/opencode", "secret", body), 204);
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            Incoming::Opencode(v) => assert_eq!(v["session"], "ses_1"),
            _ => panic!("wrong route"),
        }
        assert_eq!(post(port, "/v1/events/opencode", "wrong", body), 401);
        assert_eq!(post(port, "/v1/events/opencode", "secret", "[1]"), 400);
        ing.stop();
    }

    #[test]
    fn a_closed_door_is_not_there() {
        let (tx, rx) = channel();
        let d = door(false);
        let ing = Ingest::start("secret".into(), tx, d.clone(), board()).unwrap();
        let port = ing.endpoint().port;
        let body = r#"{"agent":"kilo","session":"abc","state":"done"}"#;
        assert_eq!(post(port, "/v1/events/generic", "secret", body), 404);
        assert!(rx.try_recv().is_err());
        d.generic.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(post(port, "/v1/events/generic", "secret", body), 204, "switch works live");
        ing.stop();
    }

    #[test]
    fn the_statusline_route_is_gone() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(true), board()).unwrap();
        let port = ing.endpoint().port;
        let body = r#"{"ts":1,"payload":{"session_id":"s"}}"#;
        assert_eq!(post(port, "/v1/events/claude-statusline", "secret", body), 404);
        assert!(rx.try_recv().is_err());
        ing.stop();
    }

    #[test]
    fn claude_mod_route_checks_auth_and_version() {
        let (tx, rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(false), board()).unwrap();
        let port = ing.endpoint().port;
        let ok = r#"{"v":1,"kind":"measure","session_id":"s","ts":1}"#;
        assert_eq!(post(port, "/v1/events/claude-mod", "bad", ok), 401);
        assert_eq!(post(port, "/v1/events/claude-mod", "secret", r#"{"v":2,"kind":"measure","session_id":"s","ts":1}"#), 400);
        assert_eq!(post(port, "/v1/events/claude-mod", "secret", r#"{"kind":"measure","session_id":"s","ts":1}"#), 400, "missing version");
        assert_eq!(post(port, "/v1/events/claude-mod", "secret", "not json"), 400);
        assert_eq!(post(port, "/v1/events/claude-mod", "secret", ok), 204);
        assert!(matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Incoming::ClaudeMod(p) if p.session_id == "s"));
        let big = format!(r#"{{"v":1,"kind":"measure","session_id":"s","cwd":"{}"}}"#, "x".repeat(1 << 20));
        assert_eq!(post(port, "/v1/events/claude-mod", "secret", &big), 413);
        assert!(rx.try_recv().is_err(), "only the valid payload reached the channel");
        ing.stop();
    }

    #[test]
    fn action_route_hands_commands_to_the_desk_and_returns_its_reply() {
        let (tx, _rx) = channel();
        let (desk, orders) = channel::<(crate::mod_action::ModAction, Sender<ModReply>)>();
        let ing = Ingest::start_with_desk("secret".into(), tx, door(false), board(), Some(desk)).unwrap();
        let port = ing.endpoint().port;
        let clerk = std::thread::spawn(move || {
            let (action, reply) = orders.recv().unwrap();
            assert_eq!(action, crate::mod_action::ModAction::Clear);
            reply.send(ModReply { count: Some(3), ..ModReply::done() }).unwrap();
        });
        let send = |token: &str, body: &str| match ureq::post(&format!("http://127.0.0.1:{port}/v1/actions/claude-mod"))
            .set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(4)).send_string(body) {
            Ok(r) => (r.status(), r.into_string().unwrap()),
            Err(ureq::Error::Status(c, _)) => (c, String::new()),
            Err(e) => panic!("{e}"),
        };
        assert_eq!(send("bad", r#"{"v":1,"action":"clear"}"#).0, 401);
        assert_eq!(send("secret", r#"{"v":1,"action":"nope"}"#).0, 400);
        assert_eq!(send("secret", r#"{"v":1,"action":"clear"}"#), (200, r#"{"ok":true,"count":3}"#.to_string()));
        clerk.join().unwrap();
        assert_eq!(send("secret", r#"{"v":1,"action":"clear"}"#).0, 503, "nobody at the desk");
        ing.stop();
    }

    #[test]
    fn without_a_desk_the_action_route_is_not_there() {
        let (tx, _rx) = channel();
        let ing = Ingest::start("secret".into(), tx, door(false), board()).unwrap();
        assert_eq!(post(ing.endpoint().port, "/v1/actions/claude-mod", "secret", r#"{"v":1,"action":"clear"}"#), 404);
        ing.stop();
    }

    #[test]
    fn state_route_serves_the_board_to_the_token_holder_only() {
        let (tx, _rx) = channel();
        let b = board();
        let ing = Ingest::start("secret".into(), tx, door(false), b.clone()).unwrap();
        let port = ing.endpoint().port;
        *b.write().unwrap() = br#"{"v":1}"#.to_vec();
        assert_eq!(get(port, "/v1/state", "bad").0, 401);
        assert_eq!(get(port, "/v1/state", "secret"), (200, r#"{"v":1}"#.to_string()));
        assert_eq!(get(port, "/v1/events/claude", "secret").0, 404);
        assert_eq!(get(port, "/nope", "secret").0, 404);
        assert_eq!(post(port, "/v1/state", "secret", "{}"), 404);
        ing.stop();
    }
}
