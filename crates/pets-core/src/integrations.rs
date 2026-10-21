//! Apps with pets: detection, status, enabling, disabling, and complete removal.
//! A future program needs a new `AppId` variant plus a core adapter.
use crate::i18n::{tr, Lang};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Why an integration change did not happen: a deliberate no-op or a genuine failure.
#[derive(Debug, Clone, PartialEq)]
pub enum Error { LeftAlone(String), Failed(String) }

impl Error {
    pub fn left_alone(&self) -> bool { matches!(self, Self::LeftAlone(_)) }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::LeftAlone(message) | Self::Failed(message) => f.write_str(message) }
    }
}

impl From<String> for Error { fn from(message: String) -> Self { Self::Failed(message) } }
impl From<&str> for Error { fn from(message: &str) -> Self { Self::Failed(message.into()) } }
impl From<Error> for String { fn from(error: Error) -> Self { error.to_string() } }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppId { ClaudeCode, Codex, AgentRouter, Opencode, Copilot, Antigravity, Cursor, Grok, Zcode }

impl AppId {
    pub const ALL: [AppId; 9] = [AppId::ClaudeCode, AppId::Codex, AppId::AgentRouter, AppId::Opencode, AppId::Copilot, AppId::Antigravity,
        AppId::Cursor, AppId::Grok, AppId::Zcode];
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Detected { pub found: bool, pub path: Option<String>, pub note: Option<String> }

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status { pub installed: bool, pub detail: String }

pub fn claude_settings(home: &Path) -> PathBuf { home.join(".claude").join("settings.json") }
pub fn pets_dir(home: &Path) -> PathBuf { home.join(".agent-pets") }
/// Hook binary name: `hook.exe` on Windows, `hook` elsewhere.
#[cfg(windows)]
pub const HOOK_FILE: &str = "hook.exe";
#[cfg(not(windows))]
pub const HOOK_FILE: &str = "hook";
pub fn installed_hook(home: &Path) -> PathBuf { pets_dir(home).join(HOOK_FILE) }
fn statusline_original(home: &Path) -> PathBuf { pets_dir(home).join("statusline-original.json") }

/// First line of our plugin: a file without it is not ours and is left untouched.
pub const PLUGIN_MARK: &str = "// agent-pets plugin v1";
pub const OPENCODE_PLUGIN: &str = include_str!("../assets/opencode-plugin.js");
fn opencode_dir(home: &Path) -> PathBuf { home.join(".config").join("opencode") }
/// opencode 1.18 loads `{plugin,plugins}/*.{ts,js}` from its config directory (spike S1).
pub fn opencode_plugin(home: &Path) -> PathBuf { opencode_dir(home).join("plugins").join("agent-pets.js") }

enum PluginFile { Missing, Ours(String), Foreign }

fn plugin_file(home: &Path) -> PluginFile {
    match std::fs::read_to_string(opencode_plugin(home)) {
        Ok(t) if t.starts_with(PLUGIN_MARK) => PluginFile::Ours(t),
        Ok(_) => PluginFile::Foreign,
        // (a file where the `plugins` folder should be reads as NotADirectory on Linux, NotFound on Windows)
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => PluginFile::Missing,
        // file exists but cannot be read (e.g. not UTF-8): treat it as someone else's
        Err(_) => PluginFile::Foreign,
    }
}

fn foreign_plugin(lang: Lang) -> String {
    tr(lang, "Plik agent-pets.js w plugins/ opencode nie jest nasz, nie nadpisuję.",
        "agent-pets.js in the opencode plugins/ folder is not ours; not overwriting it.").into()
}

fn enable_opencode(home: &Path, lang: Lang) -> Result<String, Error> {
    if !opencode_dir(home).is_dir() { return Err(Error::LeftAlone(detect(AppId::Opencode, home, lang).note.unwrap_or_default())); }
    let f = opencode_plugin(home);
    match plugin_file(home) {
        PluginFile::Foreign => return Err(Error::LeftAlone(foreign_plugin(lang))),
        PluginFile::Ours(t) if t == OPENCODE_PLUGIN => {}
        _ => {
            let dir = f.parent().expect("plugins/");
            let err = |e: std::io::Error| format!("{} {}: {e}", tr(lang, "Nie mogę zapisać", "Cannot write"), f.display());
            std::fs::create_dir_all(dir).map_err(err)?;
            // atomic write: opencode never reads half a file
            let tmp = dir.join(".agent-pets.js.tmp");
            std::fs::write(&tmp, OPENCODE_PLUGIN).map_err(err)?;
            std::fs::rename(&tmp, &f).map_err(|e| { let _ = std::fs::remove_file(&tmp); err(e) })?;
        }
    }
    Ok(tr(lang, "Plugin opencode zainstalowany. Uruchom ponownie otwarte sesje opencode.",
        "opencode plugin installed. Restart open opencode sessions.").into())
}

fn disable_opencode(home: &Path, lang: Lang) -> Result<String, Error> {
    match plugin_file(home) {
        PluginFile::Ours(_) => {
            std::fs::remove_file(opencode_plugin(home)).map_err(|e| e.to_string())?;
            Ok(tr(lang, "Plugin opencode usunięty.", "opencode plugin removed.").into())
        }
        _ => Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()),
    }
}

/// The Claude Code mod (`claude-plugin/`) is placed as a skills-dir plugin: Claude Code lists it as `agent-pets@skills-dir`.
pub fn claude_plugin_dir(home: &Path) -> PathBuf { home.join(".claude").join("skills").join("agent-pets") }

macro_rules! plugin_file {
    ($rel:literal) => { ($rel, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude-plugin/", $rel)) as &[u8]) };
}
/// Every file of the mod except tests and what the engine generates (`tsconfig.json`, `.claude-plugin/types/`);
/// a test walks `claude-plugin/` so a new file cannot be forgotten. `plugin.json` goes last: the mod's duplicate
/// guard treats the folder as present once it exists, so it must never exist next to a half-written `hooks/`.
const PLUGIN_FILES: &[(&str, &[u8])] = &[
    plugin_file!("hooks/hooks.json"), plugin_file!("hooks/bridge.ts"), plugin_file!("hooks/commands.ts"), plugin_file!("hooks/nudge.ts"), plugin_file!("hooks/pane.tsx"),
    plugin_file!("hooks/pet.tsx"), plugin_file!("hooks/register.tsx"), plugin_file!("hooks/report.ts"), plugin_file!("hooks/sprites.ts"),
    plugin_file!("types/index.d.ts"), plugin_file!(".claude-plugin/plugin.json"),
];
/// Written by the Claude Code engine into the plugin folder; never ours to delete.
fn engine_generated(rel: &str) -> bool { rel == "tsconfig.json" || rel.starts_with(".claude-plugin/types/") }

enum PluginDir { Missing, Ours, Foreign }

/// A symlink, or on Windows any reparse point (junction): never followed, so nothing outside our folder is touched.
fn is_link(m: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    let reparse = { use std::os::windows::fs::MetadataExt; m.file_attributes() & 0x400 != 0 };
    #[cfg(not(windows))]
    let reparse = false;
    m.file_type().is_symlink() || reparse
}
fn path_is_link(p: &Path) -> bool { std::fs::symlink_metadata(p).map(|m| is_link(&m)).unwrap_or(false) }

fn plugin_dir_state(home: &Path) -> PluginDir {
    // a link to someone's checkout is not our install, even when its plugin.json names agent-pets
    if path_is_link(&claude_plugin_dir(home)) { return PluginDir::Foreign; }
    match std::fs::read(claude_plugin_dir(home).join(".claude-plugin").join("plugin.json")) {
        Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
            Ok(v) if v.get("name").and_then(|n| n.as_str()) == Some("agent-pets") => PluginDir::Ours,
            _ => PluginDir::Foreign,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PluginDir::Missing,
        Err(_) => PluginDir::Foreign,
    }
}

fn foreign_plugin_dir(lang: Lang) -> String {
    tr(lang, "Folder ~/.claude/skills/agent-pets nie jest nasz, nie nadpisuję go.",
        "~/.claude/skills/agent-pets is not ours; not overwriting it.").into()
}

/// Files under `dir` that are not part of the mod; `rel` is the `/` path of `dir` inside the plugin folder.
/// A link inside the folder is skipped: not descended into and not deleted.
fn strays(dir: &Path, rel: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        let Ok(m) = e.metadata() else { continue };
        if is_link(&m) { continue; }
        if m.is_dir() { strays(&e.path(), &r, out); }
        else if !engine_generated(&r) && !PLUGIN_FILES.iter().any(|(p, _)| *p == r) { out.push(e.path()); }
    }
}

/// Puts the mod in `~/.claude/skills/agent-pets`: files are rewritten only when they differ, and an old file of ours
/// that this version no longer ships is removed. A folder whose `plugin.json` names another plugin is left alone.
pub fn place_plugin(home: &Path, lang: Lang) -> Result<(), Error> {
    let dir = claude_plugin_dir(home);
    let ours = match plugin_dir_state(home) { PluginDir::Foreign => return Err(Error::LeftAlone(foreign_plugin_dir(lang))), PluginDir::Ours => true, PluginDir::Missing => false };
    for (rel, bytes) in PLUGIN_FILES {
        let f = dir.join(rel);
        // a subfolder that is a link would send the write outside our folder
        if f.parent().is_some_and(path_is_link) { return Err(Error::LeftAlone(foreign_plugin_dir(lang))); }
        if std::fs::read(&f).ok().as_deref() == Some(*bytes) { continue; }
        let err = |e: std::io::Error| format!("{} {}: {e}", tr(lang, "Nie mogę zapisać", "Cannot write"), f.display());
        std::fs::create_dir_all(f.parent().expect("plugin file has a folder")).map_err(err)?;
        // atomic write: the engine never reads half a file
        let tmp = f.with_extension("agent-pets-tmp");
        std::fs::write(&tmp, bytes).map_err(err)?;
        std::fs::rename(&tmp, &f).map_err(|e| { let _ = std::fs::remove_file(&tmp); err(e) })?;
    }
    if ours {
        let mut old = Vec::new();
        strays(&dir, "", &mut old);
        for f in old { let _ = std::fs::remove_file(f); }
    }
    Ok(())
}

/// Removes the mod folder, but only when it is ours.
pub fn remove_plugin(home: &Path) -> Result<(), Error> {
    match plugin_dir_state(home) {
        PluginDir::Ours => std::fs::remove_dir_all(claude_plugin_dir(home)).map_err(|e| e.to_string().into()),
        _ => Ok(()),
    }
}

/// Whether an embedded file is missing or differs. A folder that is not ours is none of our business: not a repair.
pub fn plugin_needs_repair(home: &Path) -> bool {
    if matches!(plugin_dir_state(home), PluginDir::Foreign) { return false; }
    let dir = claude_plugin_dir(home);
    PLUGIN_FILES.iter().any(|(rel, bytes)| std::fs::read(dir.join(rel)).ok().as_deref() != Some(*bytes))
}

/// Hook command for cmd and bash: forward slashes (bash does not consume `\`), quotes for any character outside the safe
/// set (space, apostrophe, `&`, parentheses…). An ordinary path stays unquoted, so it also works in PowerShell.
pub fn hook_command(hook: &Path, agent: &str, event: &str) -> String {
    let p = hook.to_string_lossy().replace('\\', "/");
    let safe = p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | ':' | '_' | '-' | '~'));
    let p = if safe { p } else { format!("\"{p}\"") };
    format!("{p} --agent {agent} --event {event}")
}

/// Hook command for PowerShell (Copilot `powershell` key): call operator and single quotes, with `'` doubled.
pub fn hook_command_ps(hook: &Path, agent: &str, event: &str) -> String {
    let p = hook.to_string_lossy().replace('\\', "/").replace('\'', "''");
    format!("& '{p}' --agent {agent} --event {event}")
}

/// Copilot events: PascalCase for those also known to VS Code, camelCase for CLI-only events (spec 0.11 §3.1).
pub const COPILOT_EVENTS: [&str; 12] = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "SubagentStart",
    "SubagentStop", "Stop", "sessionEnd", "notification", "errorOccurred", "postToolUseFailure", "permissionRequest"];
/// Without `PreToolUse`: it is a permission gate where every response makes a decision (`{}` denies, `ask` forces a prompt).
pub const ANTIGRAVITY_EVENTS: [&str; 3] = ["PreInvocation", "PostToolUse", "Stop"];
/// Our hook key in Antigravity `hooks.json`.
const ANTIGRAVITY_KEY: &str = "agent-pets";

pub const CURSOR_EVENTS: [&str; 10] = ["sessionStart", "sessionEnd", "beforeSubmitPrompt", "preToolUse", "postToolUse",
    "postToolUseFailure", "subagentStart", "subagentStop", "preCompact", "stop"];
pub const GROK_EVENTS: [&str; 11] = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "PostToolUseFailure", "Notification", "Stop", "StopFailure", "StopCancelled", "PreCompact"];
pub const ZCODE_EVENTS: [&str; 7] = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse",
    "PostToolUseFailure", "Stop"];

pub fn copilot_hooks(home: &Path) -> PathBuf { home.join(".copilot").join("hooks").join("agent-pets.json") }
pub fn antigravity_hooks(home: &Path) -> PathBuf { home.join(".gemini").join("config").join("hooks.json") }
pub fn cursor_hooks_path(home: &Path) -> PathBuf { home.join(".cursor").join("hooks.json") }
pub fn grok_hooks(home: &Path) -> PathBuf { home.join(".grok").join("hooks").join("agent-pets.json") }
/// ZCode data directory: `ZCODE_DATA_BASE_DIR` if set, otherwise `~/.zcode`.
pub fn zcode_dir(home: &Path, base: Option<PathBuf>) -> PathBuf { base.unwrap_or_else(|| home.join(".zcode")) }
pub fn zcode_config(home: &Path, base: Option<PathBuf>) -> PathBuf { zcode_dir(home, base).join("cli").join("config.json") }
fn zcode_base() -> Option<PathBuf> { std::env::var_os("ZCODE_DATA_BASE_DIR").filter(|v| !v.is_empty()).map(PathBuf::from) }

/// All commands in the JSON tree (`command` and `powershell` fields).
fn commands(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(o) => for (k, x) in o {
            match (k.as_str(), x.as_str()) {
                ("command" | "powershell", Some(c)) => out.push(c.to_string()),
                _ => commands(x, out),
            }
        },
        serde_json::Value::Array(a) => for x in a { commands(x, out) },
        _ => {}
    }
}

/// Whether this is our entry: it has commands and each invokes our `hook.exe` for this agent.
fn ours(v: &serde_json::Value, agent: &str) -> bool {
    let mut c = Vec::new();
    commands(v, &mut c);
    let mark = format!("--agent {agent} --event ");
    !c.is_empty() && c.iter().all(|x| x.contains(&mark))
}

fn copilot_json(hook: &Path) -> serde_json::Value {
    let hooks: serde_json::Map<String, serde_json::Value> = COPILOT_EVENTS.iter().map(|ev| (ev.to_string(), serde_json::json!([{
        "type": "command", "command": hook_command(hook, "copilot", ev), "powershell": hook_command_ps(hook, "copilot", ev), "timeoutSec": 5,
    }]))).collect();
    serde_json::json!({ "version": 1, "hooks": hooks })
}

enum HookFile { Missing, Ours, Foreign }

/// Hook file that belongs entirely to us (Copilot, Grok).
fn own_file(f: &Path, agent: &str) -> HookFile {
    match std::fs::read(f) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HookFile::Missing,
        Ok(b) if serde_json::from_slice::<serde_json::Value>(&b).is_ok_and(|v| ours(&v, agent)) => HookFile::Ours,
        _ => HookFile::Foreign,
    }
}

fn copilot_file(home: &Path) -> HookFile { own_file(&copilot_hooks(home), "copilot") }

/// Write our file only when its contents change; atomically, so the agent never reads half a file.
fn write_own_file(f: &Path, text: &str, lang: Lang) -> Result<(), Error> {
    if std::fs::read_to_string(f).ok().as_deref() == Some(text) { return Ok(()); }
    let dir = f.parent().expect("hooks/");
    let err = |e: std::io::Error| format!("{} {}: {e}", tr(lang, "Nie mogę zapisać", "Cannot write"), f.display());
    std::fs::create_dir_all(dir).map_err(err)?;
    let tmp = dir.join(".agent-pets.json.tmp");
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(&tmp, f).map_err(|e| { let _ = std::fs::remove_file(&tmp); err(e).into() })
}

fn foreign_copilot(lang: Lang) -> String {
    tr(lang, "Plik agent-pets.json w ~/.copilot/hooks nie jest nasz, nie nadpisuję.",
        "agent-pets.json in ~/.copilot/hooks is not ours; not overwriting it.").into()
}

fn enable_copilot(home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if !home.join(".copilot").is_dir() { return Err(Error::LeftAlone(detect(AppId::Copilot, home, lang).note.unwrap_or_default())); }
    if let HookFile::Foreign = copilot_file(home) { return Err(Error::LeftAlone(foreign_copilot(lang))); }
    let hook = place_hook(home, hook_src, lang)?;
    let text = serde_json::to_string_pretty(&copilot_json(&hook)).map_err(|e| e.to_string())?;
    write_own_file(&copilot_hooks(home), &text, lang)?;
    Ok(tr(lang, "Hooki Copilota zapisane. Uruchom ponownie otwarte sesje Copilota.",
        "Copilot hooks written. Restart open Copilot sessions.").into())
}

fn disable_copilot(home: &Path, lang: Lang) -> Result<String, Error> {
    match copilot_file(home) {
        HookFile::Ours => {
            std::fs::remove_file(copilot_hooks(home)).map_err(|e| e.to_string())?;
            Ok(tr(lang, "Hooki Copilota usunięte.", "Copilot hooks removed.").into())
        }
        _ => Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()),
    }
}

fn antigravity_entry(hook: &Path) -> serde_json::Value {
    let cmd = |ev: &str| serde_json::json!({"type": "command", "command": hook_command(hook, "antigravity", ev), "timeout": 5});
    let groups: serde_json::Map<String, serde_json::Value> = ANTIGRAVITY_EVENTS.iter().map(|ev| {
        // only tool events have groups with `matcher`; the rest are direct command lists (Antigravity Hooks docs)
        let g = if ev.ends_with("ToolUse") { serde_json::json!({"matcher": "*", "hooks": [cmd(ev)]}) } else { cmd(ev) };
        (ev.to_string(), serde_json::json!([g]))
    }).collect();
    serde_json::Value::Object(groups)
}

/// Antigravity hook file: `None` = no file; error = invalid JSON, non-object root, or our key belongs to someone else.
fn read_antigravity(home: &Path, lang: Lang) -> Result<Option<serde_json::Value>, Error> {
    let f = antigravity_hooks(home);
    let v: serde_json::Value = match std::fs::read(&f) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string().into()),
        Ok(b) => crate::hooks_install::parse_config(&b).map_err(|e| format!("{} {} ({e})", f.display(), tr(lang, "jest uszkodzony", "is damaged")))?,
    };
    if !v.is_object() {
        return Err(Error::LeftAlone(format!("{} {}", f.display(), tr(lang, "ma nieoczekiwany format, nie zmieniam go.", "has an unexpected format; not changing it."))));
    }
    if v.get(ANTIGRAVITY_KEY).is_some_and(|k| !ours(k, "antigravity")) {
        return Err(Error::LeftAlone(tr(lang, "Klucz agent-pets w ~/.gemini/config/hooks.json nie jest nasz, nie zmieniam go.",
            "The agent-pets key in ~/.gemini/config/hooks.json is not ours; not changing it.").into()));
    }
    Ok(Some(v))
}

fn enable_antigravity(home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if !home.join(".gemini").is_dir() { return Err(Error::LeftAlone(detect(AppId::Antigravity, home, lang).note.unwrap_or_default())); }
    let current = read_antigravity(home, lang)?;
    let hook = place_hook(home, hook_src, lang)?;
    let entry = antigravity_entry(&hook);
    let ours_now = current.as_ref().and_then(|v| v.get(ANTIGRAVITY_KEY));
    let done = tr(lang, "Hooki Antigravity zapisane (kopia: hooks.json.agent-pets.bak). Potrzebna wersja Antigravity z hookami.",
        "Antigravity hooks written (backup: hooks.json.agent-pets.bak). Needs an Antigravity version with hooks.");
    // repair on every startup: if unchanged, write nothing (leave user file and backup untouched)
    if ours_now == Some(&entry) { return Ok(done.into()); }
    // back up only the file before our first edit; changing our entry (e.g. new path) does not overwrite it
    crate::hooks_install::edit_file_opts(&antigravity_hooks(home), ours_now.is_none(), |v| { v[ANTIGRAVITY_KEY] = entry; })
        .map_err(|e| format!("{} {}: {e}", tr(lang, "Nie udało się zapisać hooków w", "Could not write the hooks to"), antigravity_hooks(home).display()))?;
    Ok(done.into())
}

fn disable_antigravity(home: &Path, lang: Lang) -> Result<String, Error> {
    let nothing = || Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into());
    let Some(v) = read_antigravity(home, lang)? else { return nothing() };
    if v.get(ANTIGRAVITY_KEY).is_none() { return nothing(); }
    let f = antigravity_hooks(home);
    let mut empty = false;
    // no new backup: keep the file from before our first edit as the backup
    crate::hooks_install::edit_file_opts(&f, false, |v| {
        if let Some(o) = v.as_object_mut() { o.remove(ANTIGRAVITY_KEY); empty = o.is_empty(); }
    }).map_err(|e| e.to_string())?;
    // file with only our key: nothing remains after disabling
    if empty { std::fs::remove_file(&f).map_err(|e| e.to_string())?; }
    Ok(tr(lang, "Hooki Antigravity usunięte.", "Antigravity hooks removed.").into())
}

/// File with `hooks` map {event: [entries]} (Cursor, ZCode). `None` = no file; error = invalid JSON or unexpected shape,
/// then change nothing.
fn read_hook_map(f: &Path, lang: Lang) -> Result<Option<serde_json::Value>, Error> {
    let unusual = || format!("{} {}, {}", tr(lang, "Nietypowy plik", "Unusual file"), f.display(),
        tr(lang, "nic nie zmieniam.", "leaving it unchanged."));
    let v: serde_json::Value = match std::fs::read(f) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string().into()),
        Ok(b) => crate::hooks_install::parse_config(&b).map_err(|_| Error::LeftAlone(unusual()))?,
    };
    let shape = v.is_object() && match v.get("hooks") {
        None => true,
        Some(h) => h.as_object().is_some_and(|m| m.values().all(|l| l.is_array())),
    };
    if shape { Ok(Some(v)) } else { Err(Error::LeftAlone(unusual())) }
}

/// Remove our entries from all `hooks` lists; lists emptied by this are removed. Return whether anything was removed.
fn strip_hooks(v: &mut serde_json::Value, is_ours: &dyn Fn(&serde_json::Value) -> bool) -> bool {
    let Some(hooks) = v.get_mut("hooks").and_then(|h| h.as_object_mut()) else { return false };
    let mut removed = false;
    hooks.retain(|_, list| {
        let Some(a) = list.as_array_mut() else { return true };
        let n = a.len();
        a.retain(|e| !is_ours(e));
        removed |= a.len() != n;
        !(n > 0 && a.is_empty())
    });
    removed
}

/// Append our entry to each event list; others' entries stay before ours. Write nothing if unchanged,
/// and create the backup (`.agent-pets.bak`) only from a file without our entries, preserving the pre-edit file.
fn join_hooks(f: &Path, current: Option<&serde_json::Value>, mut v: serde_json::Value, events: &[&str],
              entry: impl Fn(&str) -> serde_json::Value, is_ours: &dyn Fn(&serde_json::Value) -> bool, lang: Lang) -> Result<(), Error> {
    let had_ours = merge_hooks(&mut v, events, entry, is_ours);
    write_hooks(f, current, v, had_ours, lang)
}

/// Place our entries at the end of `hooks` lists (old ones removed). Return whether any old ones existed.
fn merge_hooks(v: &mut serde_json::Value, events: &[&str], entry: impl Fn(&str) -> serde_json::Value,
               is_ours: &dyn Fn(&serde_json::Value) -> bool) -> bool {
    let had_ours = strip_hooks(v, is_ours);
    let hooks = v.as_object_mut().expect("sprawdzone").entry("hooks").or_insert_with(|| serde_json::json!({}));
    for ev in events {
        hooks.as_object_mut().expect("sprawdzone").entry(ev.to_string()).or_insert_with(|| serde_json::json!([]))
            .as_array_mut().expect("sprawdzone").push(entry(ev));
    }
    had_ours
}

fn write_hooks(f: &Path, current: Option<&serde_json::Value>, v: serde_json::Value, had_ours: bool, lang: Lang) -> Result<(), Error> {
    if current == Some(&v) { return Ok(()); }
    crate::hooks_install::edit_file_opts(f, !had_ours, |x| *x = v)
        .map_err(|e| format!("{} {}: {e}", tr(lang, "Nie udało się zapisać hooków w", "Could not write the hooks to"), f.display()).into())
}

/// Whether every event has exactly our current entry.
fn has_hooks(v: &serde_json::Value, events: &[&str], entry: impl Fn(&str) -> serde_json::Value) -> bool {
    events.iter().all(|ev| v["hooks"][*ev].as_array().is_some_and(|a| a.contains(&entry(ev))))
}

/// Cursor 3.22 runs hooks through PowerShell: ordinary paths keep the form verified live, while quoted paths
/// (space, apostrophe…) get the call operator because `"…" --agent` is a PowerShell syntax error.
fn cursor_entry(hook: &Path, ev: &str) -> serde_json::Value {
    let cmd = hook_command(hook, "cursor", ev);
    let cmd = if cmd.starts_with('"') { hook_command_ps(hook, "cursor", ev) } else { cmd };
    serde_json::json!({"command": cmd, "timeout": 5})
}
fn is_cursor(e: &serde_json::Value) -> bool { ours(e, "cursor") }

fn enable_cursor(home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if !home.join(".cursor").is_dir() { return Err(Error::LeftAlone(detect(AppId::Cursor, home, lang).note.unwrap_or_default())); }
    let f = cursor_hooks_path(home);
    let current = read_hook_map(&f, lang)?;
    let hook = place_hook(home, hook_src, lang)?;
    let mut v = current.clone().unwrap_or_else(|| serde_json::json!({}));
    if v.get("version").is_none() { v["version"] = serde_json::json!(1); }
    join_hooks(&f, current.as_ref(), v, &CURSOR_EVENTS, |ev| cursor_entry(&hook, ev), &is_cursor, lang)?;
    Ok(tr(lang, "Hooki Cursora zapisane (kopia: hooks.json.agent-pets.bak). Uruchom ponownie Cursor.",
        "Cursor hooks written (backup: hooks.json.agent-pets.bak). Restart Cursor.").into())
}

fn disable_cursor(home: &Path, lang: Lang) -> Result<String, Error> {
    let f = cursor_hooks_path(home);
    let Some(mut v) = read_hook_map(&f, lang)? else { return Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()) };
    if !strip_hooks(&mut v, &is_cursor) { return Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()); }
    let empty = v.get("hooks").and_then(|h| h.as_object()).is_none_or(|h| h.is_empty());
    // without a backup, the file was ours from the start: nothing remains after disabling
    if empty && !f.with_file_name("hooks.json.agent-pets.bak").exists() {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
    } else {
        crate::hooks_install::edit_file_opts(&f, false, |x| *x = v).map_err(|e| e.to_string())?;
    }
    Ok(tr(lang, "Hooki Cursora usunięte.", "Cursor hooks removed.").into())
}

fn grok_json(hook: &Path) -> serde_json::Value {
    let hooks: serde_json::Map<String, serde_json::Value> = GROK_EVENTS.iter().map(|ev| {
        // Grok Build runs the command through PowerShell on Windows (`-Command`): `"path" --agent` is a parser error there
        let cmd = hook_command(hook, "grok", ev);
        let cmd = if cmd.starts_with('"') { hook_command_ps(hook, "grok", ev) } else { cmd };
        let mut g = serde_json::json!({"hooks": [{"type": "command", "command": cmd, "timeout": 5}]});
        // `matcher` only for tool events, as in Claude Code
        if ev.contains("ToolUse") { g["matcher"] = serde_json::json!("*"); }
        (ev.to_string(), serde_json::json!([g]))
    }).collect();
    serde_json::json!({ "hooks": hooks })
}

fn grok_text(hook: &Path) -> String { serde_json::to_string_pretty(&grok_json(hook)).expect("json") }

fn foreign_grok(lang: Lang) -> String {
    tr(lang, "Plik agent-pets.json w ~/.grok/hooks nie jest nasz, nie nadpisuję.",
        "agent-pets.json in ~/.grok/hooks is not ours; not overwriting it.").into()
}

fn enable_grok(home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if !home.join(".grok").is_dir() { return Err(Error::LeftAlone(detect(AppId::Grok, home, lang).note.unwrap_or_default())); }
    if let HookFile::Foreign = own_file(&grok_hooks(home), "grok") { return Err(Error::LeftAlone(foreign_grok(lang))); }
    let hook = place_hook(home, hook_src, lang)?;
    write_own_file(&grok_hooks(home), &grok_text(&hook), lang)?;
    Ok(tr(lang, "Hooki Groka zapisane. Uruchom ponownie otwarte sesje Grok Build.",
        "Grok hooks written. Restart open Grok Build sessions.").into())
}

fn disable_grok(home: &Path, lang: Lang) -> Result<String, Error> {
    match own_file(&grok_hooks(home), "grok") {
        HookFile::Ours => {
            std::fs::remove_file(grok_hooks(home)).map_err(|e| e.to_string())?;
            Ok(tr(lang, "Hooki Groka usunięte.", "Grok hooks removed.").into())
        }
        _ => Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()),
    }
}

/// ZCode process hook: direct path (no shell, so no quotes) and separate arguments.
fn zcode_entry(hook: &Path, ev: &str) -> serde_json::Value {
    serde_json::json!({"matcher": "*", "hooks": [{"type": "process", "command": hook.to_string_lossy(),
        "args": ["--agent", "zcode", "--event", ev], "timeoutMs": 5000}]})
}

fn is_zcode(e: &serde_json::Value) -> bool {
    e["hooks"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|h| {
        let args = h["args"].as_array();
        let has = |w: &str| args.is_some_and(|a| a.iter().any(|x| x.as_str() == Some(w)));
        let cmd = h["command"].as_str().unwrap_or("").to_lowercase().replace('\\', "/");
        has("--agent") && has("zcode") && (cmd.ends_with(".agent-pets/hook.exe") || cmd.ends_with(".agent-pets/hook"))
    }))
}

/// ZCode config: events live in `hooks.events`, alongside only `enabled`, `timeoutMs`, and `maxOutputBytes`; ZCode rejects
/// the whole file for any other key in `hooks` and runs hooks only with `hooks.enabled: true` (schema from the ZCode
/// bundle, verified live). Unexpected shape is an error; change nothing then.
fn read_zcode(f: &Path, lang: Lang) -> Result<Option<serde_json::Value>, Error> {
    let unusual = || format!("{} {}, {}", tr(lang, "Nietypowy plik", "Unusual file"), f.display(),
        tr(lang, "nic nie zmieniam.", "leaving it unchanged."));
    let v: serde_json::Value = match std::fs::read(f) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string().into()),
        Ok(b) => crate::hooks_install::parse_config(&b).map_err(|_| Error::LeftAlone(unusual()))?,
    };
    let shape = v.is_object() && match v.get("hooks") {
        None => true,
        Some(h) => h.as_object().is_some_and(|m| {
            m.keys().all(|k| matches!(k.as_str(), "enabled" | "timeoutMs" | "maxOutputBytes" | "events"))
                && m.get("events").is_none_or(|e| e.as_object().is_some_and(|e| e.values().all(|l| l.is_array())))
        }),
    };
    if shape { Ok(Some(v)) } else { Err(Error::LeftAlone(unusual())) }
}

/// `hooks.events` as `{"hooks": …}` for shared helpers.
fn zcode_view(v: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"hooks": v["hooks"].get("events").cloned().unwrap_or_else(|| serde_json::json!({}))})
}

fn enable_zcode(home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if !zcode_dir(home, zcode_base()).is_dir() { return Err(Error::LeftAlone(detect(AppId::Zcode, home, lang).note.unwrap_or_default())); }
    let f = zcode_config(home, zcode_base());
    let current = read_zcode(&f, lang)?;
    // hooks disabled by the user: respect that decision; do not enable them
    if current.as_ref().is_some_and(|v| v["hooks"]["enabled"] == false) {
        return Err(Error::LeftAlone(tr(lang, "Hooki w ZCode są wyłączone (hooks.enabled: false w ~/.zcode/cli/config.json), nic nie zmieniam.",
            "Hooks are turned off in ZCode (hooks.enabled: false in ~/.zcode/cli/config.json); leaving it unchanged.").into()));
    }
    let hook = place_hook(home, hook_src, lang)?;
    let mut v = current.clone().unwrap_or_else(|| serde_json::json!({}));
    let mut view = zcode_view(&v);
    let had_ours = merge_hooks(&mut view, &ZCODE_EVENTS, |ev| zcode_entry(&hook, ev), &is_zcode);
    v["hooks"]["enabled"] = serde_json::json!(true);
    v["hooks"]["events"] = view["hooks"].take();
    write_hooks(&f, current.as_ref(), v, had_ours, lang)?;
    Ok(tr(lang, "Hooki ZCode zapisane (kopia: config.json.agent-pets.bak). Uruchom ponownie ZCode.",
        "ZCode hooks written (backup: config.json.agent-pets.bak). Restart ZCode.").into())
}

fn disable_zcode(home: &Path, lang: Lang) -> Result<String, Error> {
    let f = zcode_config(home, zcode_base());
    let Some(mut v) = read_zcode(&f, lang)? else { return Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()) };
    let mut view = zcode_view(&v);
    if !strip_hooks(&mut view, &is_zcode) { return Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into()); }
    // Keep `enabled` only if present before our first edit (backup); without a backup, the file was ours
    let theirs_on = std::fs::read(f.with_file_name("config.json.agent-pets.bak")).ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|b| b["hooks"]["enabled"] == true);
    let h = v["hooks"].as_object_mut().expect("sprawdzone");
    match view["hooks"].take() {
        e if e.as_object().is_some_and(|e| e.is_empty()) => { h.remove("events"); }
        e => { h.insert("events".into(), e); }
    }
    if !theirs_on && h.get("enabled") == Some(&serde_json::json!(true)) { h.remove("enabled"); }
    if h.is_empty() { v.as_object_mut().expect("sprawdzone").remove("hooks"); }
    crate::hooks_install::edit_file_opts(&f, false, |x| *x = v).map_err(|e| e.to_string())?;
    Ok(tr(lang, "Hooki ZCode usunięte.", "ZCode hooks removed.").into())
}

fn home_folder(id: AppId) -> &'static str {
    match id {
        AppId::ClaudeCode => ".claude", AppId::Codex => ".codex", AppId::AgentRouter => ".agent-router",
        AppId::Opencode => ".config/opencode", AppId::Copilot => ".copilot", AppId::Antigravity => ".gemini",
        AppId::Cursor => ".cursor", AppId::Grok => ".grok", AppId::Zcode => ".zcode",
    }
}

/// Second location used to detect the app (Cursor also stores data in `%APPDATA%\Cursor`).
fn other_folder(id: AppId) -> Option<&'static str> {
    match id { AppId::Cursor => Some("AppData/Roaming/Cursor"), _ => None }
}

/// Detect an app by its directory in the user's home: it creates one on first launch.
pub fn detect(id: AppId, home: &Path, lang: Lang) -> Detected {
    let base = if id == AppId::Zcode { zcode_base() } else { None };
    let dir = base.into_iter().chain(std::iter::once(home_folder(id)).chain(other_folder(id)).map(|f| home.join(f))).find(|d| d.is_dir());
    if let Some(dir) = dir {
        return Detected { found: true, path: Some(dir.to_string_lossy().into_owned()), note: None };
    }
    let note = match id {
        AppId::ClaudeCode => tr(lang, "Nie znaleziono ~/.claude. Uruchom Claude Code raz, potem włącz tutaj.",
            "~/.claude not found. Run Claude Code once, then turn it on here."),
        AppId::Codex => tr(lang, "Nie znaleziono ~/.codex. Uruchom Codex raz, potem włącz tutaj.",
            "~/.codex not found. Run Codex once, then turn it on here."),
        AppId::AgentRouter => tr(lang, "Nie znaleziono ~/.agent-router (serwer MCP Agent Router).",
            "~/.agent-router not found (Agent Router MCP server)."),
        AppId::Opencode => tr(lang, "Nie znaleziono ~/.config/opencode. Uruchom opencode raz, potem włącz tutaj.",
            "~/.config/opencode not found. Run opencode once, then turn it on here."),
        AppId::Copilot => tr(lang, "Nie znaleziono ~/.copilot. Uruchom Copilot raz, potem włącz tutaj.",
            "~/.copilot not found. Run Copilot once, then turn it on here."),
        AppId::Antigravity => tr(lang, "Nie znaleziono ~/.gemini. Uruchom Antigravity raz, potem włącz tutaj.",
            "~/.gemini not found. Run Antigravity once, then turn it on here."),
        AppId::Cursor => tr(lang, "Nie znaleziono ~/.cursor. Uruchom Cursor raz, potem włącz tutaj.",
            "~/.cursor not found. Run Cursor once, then turn it on here."),
        AppId::Grok => tr(lang, "Nie znaleziono ~/.grok. Uruchom Grok Build raz, potem włącz tutaj.",
            "~/.grok not found. Run Grok Build once, then turn it on here."),
        AppId::Zcode => tr(lang, "Nie znaleziono ~/.zcode. Uruchom ZCode raz, potem włącz tutaj.",
            "~/.zcode not found. Run ZCode once, then turn it on here."),
    };
    Detected { found: false, path: None, note: Some(note.into()) }
}

fn read_claude_settings(home: &Path, lang: Lang) -> Result<Option<serde_json::Value>, Error> {
    match std::fs::read(claude_settings(home)) {
        Ok(b) => crate::hooks_install::parse_config(&b).map(Some)
            .map_err(|e| match lang {
                Lang::Pl => format!("{} jest uszkodzony ({e})", claude_settings(home).display()),
                Lang::En => format!("{} is damaged ({e})", claude_settings(home).display()),
            }).map_err(Into::into),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string().into()),
    }
}

pub fn status(id: AppId, home: &Path, lang: Lang) -> Status {
    let hooks = |installed: bool| Status { installed, detail: if installed { tr(lang, "Hooki: zainstalowane", "Hooks: installed") }
        else { tr(lang, "Hooki: brak", "Hooks: missing") }.into() };
    if id == AppId::Copilot {
        return match copilot_file(home) {
            HookFile::Ours => hooks(true),
            HookFile::Missing => hooks(false),
            HookFile::Foreign => Status { installed: false, detail: foreign_copilot(lang) },
        };
    }
    if id == AppId::Antigravity {
        return match read_antigravity(home, lang) {
            Ok(v) => hooks(v.is_some_and(|v| v.get(ANTIGRAVITY_KEY).is_some())),
            Err(e) => Status { installed: false, detail: e.into() },
        };
    }
    if id == AppId::Cursor {
        return match read_hook_map(&cursor_hooks_path(home), lang) {
            Ok(v) => hooks(v.is_some_and(|v| has_hooks(&v, &CURSOR_EVENTS, |ev| cursor_entry(&installed_hook(home), ev)))),
            Err(e) => Status { installed: false, detail: e.into() },
        };
    }
    if id == AppId::Zcode {
        return match read_zcode(&zcode_config(home, zcode_base()), lang) {
            Ok(v) => hooks(v.is_some_and(|v| v["hooks"]["enabled"] == true
                && has_hooks(&zcode_view(&v), &ZCODE_EVENTS, |ev| zcode_entry(&installed_hook(home), ev)))),
            Err(e) => Status { installed: false, detail: e.into() },
        };
    }
    if id == AppId::Grok {
        return match own_file(&grok_hooks(home), "grok") {
            HookFile::Ours => hooks(std::fs::read_to_string(grok_hooks(home)).ok() == Some(grok_text(&installed_hook(home)))),
            HookFile::Missing => hooks(false),
            HookFile::Foreign => Status { installed: false, detail: foreign_grok(lang) },
        };
    }
    if id == AppId::Opencode {
        return match plugin_file(home) {
            PluginFile::Ours(_) => Status { installed: true, detail: tr(lang, "Plugin: zainstalowany", "Plugin: installed").into() },
            PluginFile::Missing => Status { installed: false, detail: tr(lang, "Plugin: brak", "Plugin: missing").into() },
            PluginFile::Foreign => Status { installed: false, detail: foreign_plugin(lang) },
        };
    }
    if id != AppId::ClaudeCode { return Status { installed: true, detail: tr(lang, "Nic do instalowania", "Nothing to install").into() }; }
    match read_claude_settings(home, lang) {
        Err(e) => Status { installed: false, detail: e.into() },
        Ok(v) => {
            let total = crate::hooks_install::EVENTS.len();
            match v.as_ref().map(crate::hooks_install::installed_count).unwrap_or(0) {
                n if n == total => Status { installed: true, detail: tr(lang, "Hooki: zainstalowane", "Hooks: installed").into() },
                0 => Status { installed: false, detail: tr(lang, "Hooki: brak", "Hooks: missing").into() },
                n => Status { installed: false, detail: format!("{} ({n}/{total})", tr(lang, "Hooki: niekompletne", "Hooks: incomplete")) },
            }
        }
    }
}

/// Copy the hook to `~/.agent-pets` only if absent or contents differ (e.g. new version).
/// Also for the door: `hook report` has a stable path at `~/.agent-pets/<hook>` (spec 8).
pub fn place_hook(home: &Path, src: Option<&Path>, lang: Lang) -> Result<PathBuf, Error> {
    let dst = installed_hook(home);
    if let Some(src) = src.filter(|s| s.is_file()) {
        let new = std::fs::read(src).map_err(|e| format!("{} {}: {e}", tr(lang, "Nie mogę odczytać", "Cannot read"), src.display()))?;
        if std::fs::read(&dst).ok().as_deref() != Some(new.as_slice()) {
            std::fs::create_dir_all(pets_dir(home)).map_err(|e| e.to_string())?;
            std::fs::write(&dst, new).map_err(|e| format!("{} {}: {e}", tr(lang, "Nie mogę zapisać", "Cannot write"), dst.display()))?;
            #[cfg(unix)]
            std::fs::set_permissions(&dst, std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
    }
    if dst.is_file() {
        Ok(dst)
    } else {
        let msg = match lang {
            Lang::Pl => format!("Brak {HOOK_FILE} w instalacji Agent Pets; zainstaluj aplikację ponownie."),
            Lang::En => format!("{HOOK_FILE} is missing from the Agent Pets install; reinstall the app."),
        };
        Err(msg.into())
    }
}

/// Restores the `statusLine` that was replaced by our former statusline pass-through (limits now come from the
/// Claude mod); the saved original is deleted.
pub fn remove_statusline(home: &Path, lang: Lang) -> Result<String, Error> {
    let settings = claude_settings(home);
    crate::statusline_install::uninstall_file_at(&settings, &statusline_original(home))
        .map_err(|e| format!("{} {}: {e}", tr(lang, "Nie mogę zapisać", "Cannot write"), settings.display()))?;
    let _ = std::fs::remove_file(statusline_original(home));
    Ok(tr(lang, "Statusline wyłączony.", "Statusline disabled.").into())
}

/// Startup migration: settings still pointing at our statusline hook get the user's own statusline back.
/// Idempotent, so no flag is stored. Returns whether it changed anything.
pub fn migrate_statusline(home: &Path) -> bool {
    match read_claude_settings(home, Lang::En) {
        Ok(Some(v)) if crate::statusline_install::is_installed(&v) => remove_statusline(home, Lang::En).is_ok(),
        _ => false,
    }
}

pub fn enable(id: AppId, home: &Path, hook_src: Option<&Path>, lang: Lang) -> Result<String, Error> {
    if id == AppId::Opencode { return enable_opencode(home, lang); }
    if id == AppId::Copilot { return enable_copilot(home, hook_src, lang); }
    if id == AppId::Antigravity { return enable_antigravity(home, hook_src, lang); }
    if id == AppId::Cursor { return enable_cursor(home, hook_src, lang); }
    if id == AppId::Grok { return enable_grok(home, hook_src, lang); }
    if id == AppId::Zcode { return enable_zcode(home, hook_src, lang); }
    if id != AppId::ClaudeCode { return Ok(tr(lang, "Nic do instalowania", "Nothing to install").into()); }
    read_claude_settings(home, lang)?;
    let hook = place_hook(home, hook_src, lang)?;
    crate::hooks_install::install_file(&claude_settings(home), &hook.to_string_lossy())
        .map_err(|e| format!("{} {}: {e}", tr(lang, "Nie udało się zapisać hooków w", "Could not write the hooks to"), claude_settings(home).display()))?;
    Ok(tr(lang, "Hooki zainstalowane. Uruchom ponownie otwarte sesje Claude Code.", "Hooks installed. Restart open Claude Code sessions.").into())
}

pub fn disable(id: AppId, home: &Path, lang: Lang) -> Result<String, Error> {
    let nothing = || Ok(tr(lang, "Nic do usunięcia", "Nothing to remove").into());
    if id == AppId::Opencode { return disable_opencode(home, lang); }
    if id == AppId::Copilot { return disable_copilot(home, lang); }
    if id == AppId::Antigravity { return disable_antigravity(home, lang); }
    if id == AppId::Cursor { return disable_cursor(home, lang); }
    if id == AppId::Grok { return disable_grok(home, lang); }
    if id == AppId::Zcode { return disable_zcode(home, lang); }
    if id != AppId::ClaudeCode { return nothing(); }
    let plugin = remove_plugin(home);
    let Some(v) = read_claude_settings(home, lang)? else { plugin?; return nothing() };
    let settings = claude_settings(home);
    if crate::statusline_install::is_installed(&v) {
        crate::statusline_install::uninstall_file_at(&settings, &statusline_original(home)).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(statusline_original(home));
    }
    crate::hooks_install::uninstall_file(&settings).map_err(|e| e.to_string())?;
    plugin?;
    Ok(tr(lang, "Hooki usunięte z ustawień Claude Code (kopia: settings.json.agent-pets.bak).",
        "Hooks removed from Claude Code settings (backup: settings.json.agent-pets.bak).").into())
}

/// Whether Claude Code needs re-enabling: hooks disappeared (e.g. update via "uninstall, then install")
/// or `hook.exe` in `~/.agent-pets` differs from the installed copy.
pub fn claude_needs_repair(home: &Path, hook_src: Option<&Path>) -> bool {
    if !status(AppId::ClaudeCode, home, Lang::En).installed { return true; }
    let Some(src) = hook_src.and_then(|p| std::fs::read(p).ok()) else { return false };
    std::fs::read(installed_hook(home)).ok().as_deref() != Some(src.as_slice())
}

/// Uninstall: disable all integrations and remove widget files; keep `settings.json` unless `remove_data`.
pub fn uninstall_all(home: &Path, remove_data: bool, lang: Lang) -> Vec<String> {
    let mut steps: Vec<String> = AppId::ALL.iter().map(|id| match disable(*id, home, lang) {
        Ok(m) => format!("{id:?}: {m}"),
        Err(e) => format!("{id:?}: {}: {e}", tr(lang, "błąd", "error")),
    }).collect();
    for f in [installed_hook(home), pets_dir(home).join("endpoint.json")] {
        if std::fs::remove_file(&f).is_ok() { steps.push(format!("{} {}", tr(lang, "Usunięto", "Removed"), f.display())); }
    }
    if remove_data && std::fs::remove_dir_all(pets_dir(home)).is_ok() {
        steps.push(format!("{} {}", tr(lang, "Usunięto", "Removed"), pets_dir(home).display()));
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn home() -> tempfile::TempDir { tempfile::tempdir().unwrap() }
    fn hook_src(dir: &Path) -> PathBuf {
        let p = dir.join("res").join("hook.exe");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"hook-binary-v1").unwrap();
        p
    }
    fn claude_json(h: &Path) -> Value { serde_json::from_slice(&std::fs::read(claude_settings(h)).unwrap()).unwrap() }
    fn our_hooks(v: &Value) -> usize {
        v["hooks"].as_object().map(|o| o.values().flat_map(|g| g.as_array().unwrap().iter())
            .flat_map(|g| g["hooks"].as_array().unwrap().iter())
            .filter(|h| h["command"].as_str().unwrap_or("").ends_with(" --agent-pets")).count()).unwrap_or(0)
    }

    #[test]
    fn detects_apps_by_their_home_folders() {
        let h = home();
        assert!(!detect(AppId::Codex, h.path(), Lang::Pl).found);
        std::fs::create_dir_all(h.path().join(".codex")).unwrap();
        std::fs::create_dir_all(h.path().join(".agent-router")).unwrap();
        let d = detect(AppId::Codex, h.path(), Lang::Pl);
        assert!(d.found && d.path.unwrap().ends_with(".codex"));
        assert!(detect(AppId::AgentRouter, h.path(), Lang::Pl).found);
        assert!(!detect(AppId::ClaudeCode, h.path(), Lang::Pl).found);
    }

    fn oc(h: &Path) -> PathBuf { h.join(".config").join("opencode") }

    #[test]
    fn copilot_and_antigravity_are_detected_by_their_home_folders() {
        let h = home();
        for id in [AppId::Copilot, AppId::Antigravity] { assert!(!detect(id, h.path(), Lang::Pl).found); }
        assert!(detect(AppId::Copilot, h.path(), Lang::Pl).note.unwrap().contains("~/.copilot"));
        assert!(detect(AppId::Antigravity, h.path(), Lang::En).note.unwrap().contains("~/.gemini"));
        std::fs::create_dir_all(h.path().join(".copilot")).unwrap();
        std::fs::create_dir_all(h.path().join(".gemini")).unwrap();
        assert!(detect(AppId::Copilot, h.path(), Lang::Pl).found);
        assert!(detect(AppId::Antigravity, h.path(), Lang::Pl).found);
        assert_eq!(AppId::ALL.len(), 9);
    }

    #[test]
    fn cursor_grok_and_zcode_are_detected_by_their_folders() {
        let h = home();
        for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { assert!(!detect(id, h.path(), Lang::Pl).found, "{id:?}"); }
        assert!(detect(AppId::Cursor, h.path(), Lang::Pl).note.unwrap().contains("~/.cursor"));
        assert!(detect(AppId::Grok, h.path(), Lang::En).note.unwrap().contains("~/.grok"));
        assert!(detect(AppId::Zcode, h.path(), Lang::En).note.unwrap().contains("~/.zcode"));
        std::fs::create_dir_all(h.path().join("AppData").join("Roaming").join("Cursor")).unwrap();
        assert!(detect(AppId::Cursor, h.path(), Lang::Pl).found);
        let h = home();
        for d in [".cursor", ".grok", ".zcode"] { std::fs::create_dir_all(h.path().join(d)).unwrap(); }
        for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { assert!(detect(id, h.path(), Lang::Pl).found, "{id:?}"); }
    }

    #[test]
    fn opencode_is_detected_by_its_config_folder() {
        let h = home();
        let d = detect(AppId::Opencode, h.path(), Lang::Pl);
        assert!(!d.found && d.note.unwrap().contains("opencode"));
        std::fs::create_dir_all(oc(h.path())).unwrap();
        assert!(detect(AppId::Opencode, h.path(), Lang::Pl).found);
    }

    #[test]
    fn missing_install_is_left_alone() {
        let h = home();
        let error = enable(AppId::Opencode, h.path(), None, Lang::En).unwrap_err();
        assert!(error.left_alone());
        assert_eq!(error.to_string(), "~/.config/opencode not found. Run opencode once, then turn it on here.");
    }

    #[test]
    fn an_unwritable_plugin_path_is_a_failure() {
        let h = home();
        std::fs::create_dir_all(oc(h.path())).unwrap();
        std::fs::write(oc(h.path()).join("plugins"), "not a directory").unwrap();
        let error = enable(AppId::Opencode, h.path(), None, Lang::En).unwrap_err();
        assert!(matches!(error, Error::Failed(_)));
    }

    #[test]
    fn the_opencode_plugin_installs_once_and_uninstalls() {
        let h = home();
        std::fs::create_dir_all(oc(h.path())).unwrap();
        assert!(!status(AppId::Opencode, h.path(), Lang::Pl).installed);
        enable(AppId::Opencode, h.path(), None, Lang::Pl).unwrap();
        enable(AppId::Opencode, h.path(), None, Lang::Pl).unwrap();
        let f = opencode_plugin(h.path());
        assert_eq!(f, oc(h.path()).join("plugins").join("agent-pets.js"));
        assert!(std::fs::read_to_string(&f).unwrap().starts_with(PLUGIN_MARK));
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1, "no temporary files");
        assert!(status(AppId::Opencode, h.path(), Lang::Pl).installed);
        disable(AppId::Opencode, h.path(), Lang::Pl).unwrap();
        assert!(!f.exists());
        assert!(!status(AppId::Opencode, h.path(), Lang::Pl).installed);
    }

    fn plugin_json(h: &Path) -> PathBuf { claude_plugin_dir(h).join(".claude-plugin").join("plugin.json") }
    fn put(f: &Path, text: &str) { std::fs::create_dir_all(f.parent().unwrap()).unwrap(); std::fs::write(f, text).unwrap(); }

    #[test]
    fn the_embedded_mod_has_the_version_of_the_app() {
        let (_, json) = PLUGIN_FILES.iter().find(|(rel, _)| *rel == ".claude-plugin/plugin.json").expect("plugin.json is embedded");
        let v: Value = serde_json::from_slice(json).unwrap();
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"), "bump claude-plugin/.claude-plugin/plugin.json together with the app");
    }

    #[test]
    fn the_plugin_folder_is_a_skills_dir_plugin() {
        assert_eq!(claude_plugin_dir(Path::new("h")), Path::new("h").join(".claude").join("skills").join("agent-pets"));
    }

    #[test]
    fn placing_the_plugin_writes_every_embedded_file_and_nothing_twice() {
        let h = home();
        place_plugin(h.path(), Lang::En).unwrap();
        let dir = claude_plugin_dir(h.path());
        for (rel, bytes) in PLUGIN_FILES { assert_eq!(std::fs::read(dir.join(rel)).unwrap(), *bytes, "{rel}"); }
        let v: Value = serde_json::from_slice(&std::fs::read(plugin_json(h.path())).unwrap()).unwrap();
        assert_eq!(v["name"], "agent-pets");
        assert!(!plugin_needs_repair(h.path()));
        // an old modification time shows a rewrite: nothing is written again
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for (rel, _) in PLUGIN_FILES { std::fs::File::options().write(true).open(dir.join(rel)).unwrap().set_modified(old).unwrap(); }
        place_plugin(h.path(), Lang::En).unwrap();
        for (rel, _) in PLUGIN_FILES { assert_eq!(std::fs::metadata(dir.join(rel)).unwrap().modified().unwrap(), old, "{rel} was rewritten"); }
    }

    #[test]
    fn placing_the_plugin_removes_files_we_no_longer_ship_but_not_the_engines() {
        let h = home();
        place_plugin(h.path(), Lang::En).unwrap();
        let dir = claude_plugin_dir(h.path());
        for f in ["hooks/old.tsx", "stray.txt", "extra/deep/x.json"] { put(&dir.join(f), "x"); }
        for f in ["tsconfig.json", ".claude-plugin/types/claude-code/index.d.ts"] { put(&dir.join(f), "engine"); }
        place_plugin(h.path(), Lang::En).unwrap();
        for f in ["hooks/old.tsx", "stray.txt", "extra/deep/x.json"] { assert!(!dir.join(f).exists(), "{f}"); }
        for f in ["tsconfig.json", ".claude-plugin/types/claude-code/index.d.ts"] { assert!(dir.join(f).is_file(), "{f}"); }
        assert!(!plugin_needs_repair(h.path()));
    }

    #[test]
    fn the_plugin_needs_repair_when_a_file_is_missing_or_edited() {
        let h = home();
        assert!(plugin_needs_repair(h.path()), "nothing placed yet");
        place_plugin(h.path(), Lang::En).unwrap();
        let f = claude_plugin_dir(h.path()).join("hooks").join("pet.tsx");
        std::fs::write(&f, "edited").unwrap();
        assert!(plugin_needs_repair(h.path()));
        place_plugin(h.path(), Lang::En).unwrap();
        assert!(!plugin_needs_repair(h.path()));
        std::fs::remove_file(&f).unwrap();
        assert!(plugin_needs_repair(h.path()));
        place_plugin(h.path(), Lang::En).unwrap();
        assert!(!plugin_needs_repair(h.path()) && f.is_file());
    }

    #[test]
    fn a_skills_folder_that_is_not_ours_is_left_alone() {
        for (lang, text) in [(Lang::En, "is not ours"), (Lang::Pl, "nie jest nasz")] {
            for json in [r#"{"name":"someone-else"}"#, "not json", "{}"] {
                let h = home();
                put(&plugin_json(h.path()), json);
                let mine = claude_plugin_dir(h.path()).join("notes.txt");
                put(&mine, "mine");
                let err = place_plugin(h.path(), lang).unwrap_err();
                assert!(err.to_string().contains(text), "{err}");
                assert!(!plugin_needs_repair(h.path()));
                remove_plugin(h.path()).unwrap();
                disable(AppId::ClaudeCode, h.path(), lang).unwrap();
                uninstall_all(h.path(), false, lang);
                assert_eq!(std::fs::read_to_string(plugin_json(h.path())).unwrap(), json);
                assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine");
                assert!(!claude_plugin_dir(h.path()).join("hooks").exists(), "nothing was written");
            }
        }
    }

    /// Directory link at `link` to `target`: a symlink (`junction` false) or a Windows junction. False when it cannot be made here.
    fn make_link(target: &Path, link: &Path, junction: bool) -> bool {
        if junction {
            return cfg!(windows) && std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target)
                .output().map(|o| o.status.success()).unwrap_or(false);
        }
        #[cfg(windows)] { std::os::windows::fs::symlink_dir(target, link).is_ok() }
        #[cfg(unix)] { std::os::unix::fs::symlink(target, link).is_ok() }
        #[cfg(not(any(windows, unix)))] { false }
    }
    /// What a user's checkout of the mod looks like: ours by name, with files the app would call strays.
    fn checkout(root: &Path) -> PathBuf {
        let c = root.join("checkout");
        for (rel, bytes) in PLUGIN_FILES { std::fs::create_dir_all(c.join(rel).parent().unwrap()).unwrap(); std::fs::write(c.join(rel), bytes).unwrap(); }
        put(&c.join("hooks").join("bridge.test.ts"), "test");
        put(&c.join("notes.md"), "notes");
        c
    }
    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(d: &Path, rel: &str, out: &mut Vec<(String, Vec<u8>)>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let r = format!("{rel}/{}", e.file_name().to_string_lossy());
                if e.path().is_dir() { walk(&e.path(), &r, out); } else { out.push((r, std::fs::read(e.path()).unwrap())); }
            }
        }
        let mut v = Vec::new();
        walk(dir, "", &mut v);
        v.sort();
        v
    }

    #[test]
    fn a_link_to_a_checkout_is_never_followed() {
        for junction in [false, true] {
            let h = home();
            let target = checkout(h.path());
            let before = snapshot(&target);
            let link = claude_plugin_dir(h.path());
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            if !make_link(&target, &link, junction) { eprintln!("cannot create a directory link (junction: {junction}) here; skipping"); continue; }
            let err = place_plugin(h.path(), Lang::En).unwrap_err();
            assert!(err.to_string().contains("is not ours"), "{err}");
            assert!(!plugin_needs_repair(h.path()));
            remove_plugin(h.path()).unwrap();
            disable(AppId::ClaudeCode, h.path(), Lang::En).unwrap();
            uninstall_all(h.path(), false, Lang::En);
            assert_eq!(snapshot(&target), before, "junction: {junction}");
            assert!(path_is_link(&link), "the link itself stays");
        }
    }

    #[test]
    fn a_link_inside_our_folder_is_neither_followed_nor_written_through() {
        for junction in [false, true] {
            let h = home();
            place_plugin(h.path(), Lang::En).unwrap();
            let dir = claude_plugin_dir(h.path());
            let target = checkout(h.path());
            let before = snapshot(&target);
            // a stray link to a folder, and `hooks/` itself replaced by a link
            if !make_link(&target, &dir.join("extra"), junction) { eprintln!("cannot create a directory link (junction: {junction}) here; skipping"); continue; }
            place_plugin(h.path(), Lang::En).unwrap();
            assert_eq!(snapshot(&target), before, "stray link, junction: {junction}");
            std::fs::remove_dir_all(dir.join("hooks")).unwrap();
            assert!(make_link(&target.join("hooks"), &dir.join("hooks"), junction));
            let err = place_plugin(h.path(), Lang::En).unwrap_err();
            assert!(err.to_string().contains("is not ours"), "{err}");
            assert_eq!(snapshot(&target), before, "hooks link, junction: {junction}");
        }
    }

    #[test]
    fn removing_the_plugin_deletes_only_our_folder() {
        let h = home();
        remove_plugin(h.path()).unwrap();
        place_plugin(h.path(), Lang::En).unwrap();
        put(&claude_plugin_dir(h.path()).join("tsconfig.json"), "engine");
        remove_plugin(h.path()).unwrap();
        assert!(!claude_plugin_dir(h.path()).exists());
    }

    #[test]
    fn disabling_claude_code_and_uninstalling_remove_the_plugin() {
        let h = home();
        place_plugin(h.path(), Lang::En).unwrap();
        disable(AppId::ClaudeCode, h.path(), Lang::En).unwrap();
        assert!(!claude_plugin_dir(h.path()).exists());
        place_plugin(h.path(), Lang::En).unwrap();
        uninstall_all(h.path(), false, Lang::En);
        assert!(!claude_plugin_dir(h.path()).exists());
        // also with Claude Code settings present
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        std::fs::write(claude_settings(h.path()), "{}").unwrap();
        place_plugin(h.path(), Lang::En).unwrap();
        disable(AppId::ClaudeCode, h.path(), Lang::En).unwrap();
        assert!(!claude_plugin_dir(h.path()).exists());
    }

    #[test]
    fn every_file_of_the_mod_on_disk_is_embedded() {
        fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                if e.path().is_dir() { if r != "node_modules" { walk(&e.path(), &r, out); } } else { out.push(r); }
            }
        }
        let mut found = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("claude-plugin"), "", &mut found);
        let wanted: Vec<&String> = found.iter().filter(|r| !r.ends_with(".test.ts") && !engine_generated(r)).collect();
        for r in wanted { assert!(PLUGIN_FILES.iter().any(|(p, _)| p == r), "{r} is not in PLUGIN_FILES"); }
        for (p, _) in PLUGIN_FILES { assert!(found.iter().any(|r| r == p), "{p} is embedded but not on disk"); }
    }

    #[test]
    fn hook_commands_work_in_cmd_bash_and_powershell_even_with_spaces_and_apostrophes() {
        let p = Path::new(r"C:\Users\ja\.agent-pets\hook.exe");
        assert_eq!(hook_command(p, "copilot", "Stop"), "C:/Users/ja/.agent-pets/hook.exe --agent copilot --event Stop");
        let sp = Path::new(r"C:\Users\Jan Kowalski\.agent-pets\hook.exe");
        assert_eq!(hook_command(sp, "antigravity", "PreToolUse"),
            r#""C:/Users/Jan Kowalski/.agent-pets/hook.exe" --agent antigravity --event PreToolUse"#);
        let ap = Path::new(r"C:\Users\O'Neil\.agent-pets\hook.exe");
        assert_eq!(hook_command(ap, "antigravity", "Stop"), r#""C:/Users/O'Neil/.agent-pets/hook.exe" --agent antigravity --event Stop"#);
        assert!(hook_command(Path::new(r"C:\Users\R&D\hook.exe"), "copilot", "Stop").starts_with('"'));
        // a short 8.3 name (e.g. a temp folder): `~` inside a path is plain text in cmd, bash and PowerShell
        assert_eq!(hook_command(Path::new(r"C:\Users\RUNNER~1\hook.exe"), "cursor", "Stop"), "C:/Users/RUNNER~1/hook.exe --agent cursor --event Stop");
        assert_eq!(hook_command_ps(ap, "copilot", "Stop"), "& 'C:/Users/O''Neil/.agent-pets/hook.exe' --agent copilot --event Stop");
        assert_eq!(hook_command_ps(sp, "copilot", "Stop"), "& 'C:/Users/Jan Kowalski/.agent-pets/hook.exe' --agent copilot --event Stop");
    }

    fn copilot_json(h: &Path) -> Value { serde_json::from_slice(&std::fs::read(copilot_hooks(h)).unwrap()).unwrap() }

    #[test]
    fn the_copilot_hooks_file_installs_once_and_uninstalls() {
        let h = home();
        let src = hook_src(h.path());
        assert!(enable(AppId::Copilot, h.path(), Some(&src), Lang::Pl).is_err(), "missing ~/.copilot");
        std::fs::create_dir_all(h.path().join(".copilot")).unwrap();
        assert!(!status(AppId::Copilot, h.path(), Lang::Pl).installed);
        enable(AppId::Copilot, h.path(), Some(&src), Lang::Pl).unwrap();
        let first = std::fs::read(copilot_hooks(h.path())).unwrap();
        enable(AppId::Copilot, h.path(), Some(&src), Lang::Pl).unwrap();
        assert_eq!(std::fs::read(copilot_hooks(h.path())).unwrap(), first, "enabling again changes nothing");
        assert_eq!(copilot_hooks(h.path()), h.path().join(".copilot").join("hooks").join("agent-pets.json"));
        let v = copilot_json(h.path());
        assert_eq!(v["version"], 1);
        let hooks = v["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), COPILOT_EVENTS.len());
        for ev in COPILOT_EVENTS {
            let entry = &hooks[ev][0];
            assert_eq!(entry["type"], "command");
            assert!(entry["command"].as_str().unwrap().ends_with(&format!("{HOOK_FILE} --agent copilot --event {ev}")), "{entry}");
            assert!(entry["powershell"].as_str().unwrap().starts_with("& '"), "{entry}");
            assert_eq!(entry["timeoutSec"], 5);
        }
        assert!(installed_hook(h.path()).is_file(), "hook at a stable path");
        assert_eq!(std::fs::read_dir(copilot_hooks(h.path()).parent().unwrap()).unwrap().count(), 1, "no temporary files");
        assert!(status(AppId::Copilot, h.path(), Lang::Pl).installed);
        disable(AppId::Copilot, h.path(), Lang::Pl).unwrap();
        assert!(!copilot_hooks(h.path()).exists());
        assert!(!status(AppId::Copilot, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn someone_elses_copilot_agent_pets_json_is_never_touched() {
        let h = home();
        let f = copilot_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let theirs = r#"{"version":1,"hooks":{"Stop":[{"type":"command","command":"my-script.ps1"}]}}"#;
        std::fs::write(&f, theirs).unwrap();
        std::fs::write(f.with_file_name("audit.json"), "{}").unwrap();
        assert!(enable(AppId::Copilot, h.path(), Some(&hook_src(h.path())), Lang::Pl).is_err());
        disable(AppId::Copilot, h.path(), Lang::Pl).unwrap();
        uninstall_all(h.path(), false, Lang::Pl);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), theirs);
        assert!(f.with_file_name("audit.json").exists(), "other files in hooks/ remain");
        let st = status(AppId::Copilot, h.path(), Lang::Pl);
        assert!(!st.installed && st.detail.contains("nie jest nasz"), "{st:?}");
    }

    fn ag_json(h: &Path) -> Value { serde_json::from_slice(&std::fs::read(antigravity_hooks(h)).unwrap()).unwrap() }

    #[test]
    fn antigravity_hooks_join_someone_elses_hooks_and_leave_them_alone() {
        let h = home();
        let src = hook_src(h.path());
        assert!(enable(AppId::Antigravity, h.path(), Some(&src), Lang::Pl).is_err(), "missing ~/.gemini");
        let f = antigravity_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let mine = json!({"PostToolUse": [{"matcher": "run_command", "hooks": [{"type": "command", "command": "./lint.sh", "timeout": 10}]}]});
        std::fs::write(&f, serde_json::to_string(&json!({"my-linter": mine})).unwrap()).unwrap();
        enable(AppId::Antigravity, h.path(), Some(&src), Lang::Pl).unwrap();
        let v = ag_json(h.path());
        assert_eq!(v["my-linter"], mine, "other hook unchanged");
        let ours = v["agent-pets"].as_object().unwrap();
        assert_eq!(ours.len(), ANTIGRAVITY_EVENTS.len());
        for ev in ANTIGRAVITY_EVENTS {
            // tools: {matcher, hooks: [...]} groups; PreInvocation and Stop: commands directly under the event (otherwise
            // Antigravity 2.5 rejects the whole file: "command hook must specify 'command'")
            let group = &ours[ev][0];
            let hook = if ev.ends_with("ToolUse") { &group["hooks"][0] } else { group };
            let cmd = hook["command"].as_str().unwrap_or_else(|| panic!("{ev}: {group}"));
            assert!(cmd.ends_with(&format!("{HOOK_FILE} --agent antigravity --event {ev}")), "{cmd}");
            assert_eq!((hook["type"].as_str(), &hook["timeout"]), (Some("command"), &json!(5)), "{ev}");
            assert_eq!(group.get("matcher").and_then(|m| m.as_str()), if ev.ends_with("ToolUse") { Some("*") } else { None }, "{ev}");
        }
        assert!(f.with_file_name("hooks.json.agent-pets.bak").exists(), "backup before edit");
        assert!(status(AppId::Antigravity, h.path(), Lang::Pl).installed);
        disable(AppId::Antigravity, h.path(), Lang::Pl).unwrap();
        assert_eq!(ag_json(h.path()), json!({"my-linter": mine}));
        assert!(!status(AppId::Antigravity, h.path(), Lang::Pl).installed);
    }

    /// Editors and PowerShell leave a UTF-8 BOM, or an empty file: neither is "damaged". Other keys keep their order.
    #[test]
    fn a_bom_or_an_empty_hooks_file_is_not_damaged_and_key_order_survives() {
        for (name, text) in [("bom", "\u{feff}{\"zeta\": {}, \"alpha\": {}}"), ("empty", ""), ("blank", "  \r\n")] {
            let h = home();
            let f = antigravity_hooks(h.path());
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, text).unwrap();
            enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap_or_else(|e| panic!("{name}: {e}"));
            let keys: Vec<String> = ag_json(h.path()).as_object().unwrap().keys().cloned().collect();
            if name == "bom" { assert_eq!(keys, ["zeta", "alpha", "agent-pets"], "their keys stay in place, ours is appended"); }
            else { assert_eq!(keys, ["agent-pets"], "{name}"); }
            disable(AppId::Antigravity, h.path(), Lang::Pl).unwrap();
        }
        let h = home();
        std::fs::create_dir_all(claude_settings(h.path()).parent().unwrap()).unwrap();
        std::fs::write(claude_settings(h.path()), "\u{feff}{\"theme\": \"dark\", \"a\": 1}").unwrap();
        enable(AppId::ClaudeCode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let keys: Vec<String> = claude_json(h.path()).as_object().unwrap().keys().cloned().collect();
        assert_eq!(&keys[..2], ["theme", "a"], "Claude settings are not re-sorted");
    }

    #[test]
    fn a_repeated_antigravity_enable_leaves_the_file_and_the_first_backup_alone() {
        let h = home();
        let f = antigravity_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let original = "{\n  \"zeta\": {},\n  \"my-linter\": {\"Stop\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"./x.sh\"}]}]}\n}";
        std::fs::write(&f, original).unwrap();
        let bak = f.with_file_name("hooks.json.agent-pets.bak");
        enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let (file, backup) = (std::fs::read(&f).unwrap(), std::fs::read(&bak).unwrap());
        assert_eq!(backup, original.as_bytes(), "backup is the file before our edit");
        // repair at app startup: nothing changed, so write nothing
        enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert_eq!((std::fs::read(&f).unwrap(), std::fs::read(&bak).unwrap()), (file.clone(), backup.clone()));
        // old entry (different hook.exe path, `Stop` in a group as before the fix): update it, preserving the original backup
        let mut v: Value = serde_json::from_slice(&file).unwrap();
        v["agent-pets"]["Stop"] = json!([{"hooks": [{"type": "command", "command": "C:/old/hook.exe --agent antigravity --event Stop", "timeout": 5}]}]);
        std::fs::write(&f, serde_json::to_string(&v).unwrap()).unwrap();
        enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), original.as_bytes());
        assert!(ag_json(h.path())["agent-pets"]["Stop"][0]["command"].as_str().unwrap().contains(".agent-pets"));
    }

    /// In Antigravity, `PreToolUse` is a permission gate with no neutral response: `{}` denies, `ask` forces a prompt. Do not
    /// register it; an entry from 0.11–0.12.0 disappears on first startup (repair replaces our whole key).
    #[test]
    fn antigravity_never_hooks_the_permission_gate() {
        assert!(!ANTIGRAVITY_EVENTS.contains(&"PreToolUse"));
        let h = home();
        let f = antigravity_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let old = json!({"matcher": "*", "hooks": [{"type": "command", "command": "C:/x/hook.exe --agent antigravity --event PreToolUse", "timeout": 5}]});
        std::fs::write(&f, serde_json::to_string(&json!({"agent-pets": {"PreToolUse": [old]}})).unwrap()).unwrap();
        enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let v = ag_json(h.path());
        assert!(v["agent-pets"].get("PreToolUse").is_none(), "{v}");
        assert!(v["agent-pets"]["PostToolUse"].is_array());
    }

    #[test]
    fn antigravity_creates_its_config_folder_and_removes_a_file_left_empty() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".gemini")).unwrap();
        enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert!(ag_json(h.path())["agent-pets"].is_object());
        disable(AppId::Antigravity, h.path(), Lang::Pl).unwrap();
        assert!(!antigravity_hooks(h.path()).exists(), "empty file after removing our key disappears");
        assert!(disable(AppId::Antigravity, h.path(), Lang::Pl).is_ok(), "missing file means nothing to remove");
    }

    #[test]
    fn a_broken_or_foreign_antigravity_file_is_left_as_it_is() {
        let h = home();
        let f = antigravity_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let foreign = r#"{"agent-pets":{"Stop":[{"hooks":[{"type":"command","command":"their.sh"}]}]}}"#;
        for text in ["{not json", "[]", foreign] {
            std::fs::write(&f, text).unwrap();
            assert!(enable(AppId::Antigravity, h.path(), Some(&hook_src(h.path())), Lang::Pl).is_err(), "{text}");
            let _ = disable(AppId::Antigravity, h.path(), Lang::Pl);
            uninstall_all(h.path(), false, Lang::Pl);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), text, "{text}");
            assert!(!status(AppId::Antigravity, h.path(), Lang::Pl).installed, "{text}");
        }
    }

    #[test]
    fn uninstall_removes_copilot_and_antigravity_hooks() {
        let h = home();
        let src = hook_src(h.path());
        std::fs::create_dir_all(h.path().join(".copilot")).unwrap();
        std::fs::create_dir_all(h.path().join(".gemini")).unwrap();
        enable(AppId::Copilot, h.path(), Some(&src), Lang::Pl).unwrap();
        enable(AppId::Antigravity, h.path(), Some(&src), Lang::Pl).unwrap();
        uninstall_all(h.path(), false, Lang::Pl);
        assert!(!copilot_hooks(h.path()).exists() && !antigravity_hooks(h.path()).exists());
    }

    #[test]
    fn someone_elses_agent_pets_js_is_never_touched() {
        let h = home();
        let f = opencode_plugin(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "export const Mine = 1").unwrap();
        assert!(enable(AppId::Opencode, h.path(), None, Lang::Pl).unwrap_err().left_alone());
        disable(AppId::Opencode, h.path(), Lang::Pl).unwrap();
        uninstall_all(h.path(), false, Lang::Pl);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "export const Mine = 1");
        assert!(!status(AppId::Opencode, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn uninstall_removes_the_opencode_plugin() {
        let h = home();
        assert!(enable(AppId::Opencode, h.path(), None, Lang::Pl).is_err(), "do not create config without opencode");
        assert!(!oc(h.path()).exists());
        std::fs::create_dir_all(oc(h.path())).unwrap();
        enable(AppId::Opencode, h.path(), None, Lang::Pl).unwrap();
        uninstall_all(h.path(), false, Lang::Pl);
        assert!(!opencode_plugin(h.path()).exists());
    }

    #[test]
    fn the_plugin_times_out_and_sends_no_message_content() {
        assert!(OPENCODE_PLUGIN.starts_with(PLUGIN_MARK));
        assert!(OPENCODE_PLUGIN.contains("AbortSignal.timeout(300)"));
        assert!(!OPENCODE_PLUGIN.contains("parts"), "message content does not enter the envelope");
    }

    #[test]
    fn enabling_claude_twice_installs_one_set_of_hooks() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        let src = hook_src(h.path());
        assert!(!status(AppId::ClaudeCode, h.path(), Lang::Pl).installed);
        enable(AppId::ClaudeCode, h.path(), Some(&src), Lang::Pl).unwrap();
        let hook = installed_hook(h.path());
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options().write(true).open(&hook).unwrap().set_modified(old).unwrap();
        let first = std::fs::metadata(&hook).unwrap().modified().unwrap();
        enable(AppId::ClaudeCode, h.path(), Some(&src), Lang::Pl).unwrap();
        assert_eq!(our_hooks(&claude_json(h.path())), crate::hooks_install::EVENTS.len());
        assert_eq!(std::fs::metadata(installed_hook(h.path())).unwrap().modified().unwrap(), first, "same hook.exe is not copied again");
        assert!(status(AppId::ClaudeCode, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn enabling_claude_without_any_hook_binary_is_an_error() {
        let h = home();
        assert!(enable(AppId::ClaudeCode, h.path(), None, Lang::Pl).is_err());
        assert!(!claude_settings(h.path()).exists());
    }

    #[test]
    fn claude_needs_repair_when_hooks_vanished_or_the_hook_is_outdated() {
        // an "uninstall, then install" update removes hooks with the old uninstaller
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        let src = hook_src(h.path());
        assert!(claude_needs_repair(h.path(), Some(&src)), "hooks missing");
        enable(AppId::ClaudeCode, h.path(), Some(&src), Lang::Pl).unwrap();
        assert!(!claude_needs_repair(h.path(), Some(&src)));
        assert!(!claude_needs_repair(h.path(), None), "no resource to compare against");
        std::fs::write(&src, b"hook-binary-v2").unwrap();
        assert!(claude_needs_repair(h.path(), Some(&src)), "newer hook.exe in the installation");
    }

    #[test]
    fn disabling_keeps_someone_elses_hooks() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        std::fs::write(claude_settings(h.path()), json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "other.exe"}]}]}}).to_string()).unwrap();
        enable(AppId::ClaudeCode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        disable(AppId::ClaudeCode, h.path(), Lang::Pl).unwrap();
        let v = claude_json(h.path());
        assert_eq!(our_hooks(&v), 0);
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "other.exe");
    }

    #[test]
    fn a_broken_claude_settings_file_is_left_untouched() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        std::fs::write(claude_settings(h.path()), "{bad").unwrap();
        assert!(enable(AppId::ClaudeCode, h.path(), Some(&hook_src(h.path())), Lang::Pl).is_err());
        assert_eq!(std::fs::read_to_string(claude_settings(h.path())).unwrap(), "{bad");
    }

    #[test]
    fn uninstall_restores_the_statusline_and_removes_our_files() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        std::fs::write(claude_settings(h.path()), json!({"statusLine": {"type": "command", "command": "mine.exe"}}).to_string()).unwrap();
        enable(AppId::ClaudeCode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        crate::statusline_install::install_file_at(&claude_settings(h.path()), "h.exe", &statusline_original(h.path())).unwrap();
        std::fs::write(pets_dir(h.path()).join("endpoint.json"), "{}").unwrap();
        std::fs::write(crate::settings::path(h.path()), "{}").unwrap();
        let steps = uninstall_all(h.path(), false, Lang::Pl);
        assert!(!steps.is_empty());
        let v = claude_json(h.path());
        assert_eq!((our_hooks(&v), v["statusLine"]["command"].as_str()), (0, Some("mine.exe")));
        assert!(!installed_hook(h.path()).exists() && !pets_dir(h.path()).join("endpoint.json").exists());
        assert!(crate::settings::path(h.path()).exists(), "settings remain when data is retained");
        uninstall_all(h.path(), true, Lang::Pl);
        assert!(!pets_dir(h.path()).exists());
    }
    #[test]
    fn migrating_the_statusline_restores_the_original_once() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        std::fs::write(claude_settings(h.path()), json!({"statusLine": {"type": "command", "command": "mine.exe"}}).to_string()).unwrap();
        crate::statusline_install::install_file_at(&claude_settings(h.path()), "h.exe", &statusline_original(h.path())).unwrap();
        assert!(crate::statusline_install::is_installed(&claude_json(h.path())) && statusline_original(h.path()).is_file());
        assert!(migrate_statusline(h.path()));
        assert_eq!(claude_json(h.path())["statusLine"]["command"].as_str(), Some("mine.exe"));
        assert!(!statusline_original(h.path()).exists());
        assert!(!migrate_statusline(h.path()), "idempotent");
    }

    #[test]
    fn migrating_without_our_statusline_touches_nothing() {
        let h = home();
        assert!(!migrate_statusline(h.path()), "no settings file");
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        let theirs = json!({"statusLine": {"type": "command", "command": "mine.exe"}}).to_string();
        std::fs::write(claude_settings(h.path()), &theirs).unwrap();
        assert!(!migrate_statusline(h.path()));
        assert_eq!(std::fs::read_to_string(claude_settings(h.path())).unwrap(), theirs);
    }

    #[test]
    fn texts_follow_the_language() {
        let h = home();
        assert!(detect(AppId::Codex, h.path(), Lang::En).note.unwrap().starts_with("~/.codex not found"));
        assert_eq!(status(AppId::Codex, h.path(), Lang::En).detail, "Nothing to install");
        assert_eq!(status(AppId::ClaudeCode, h.path(), Lang::Pl).detail, "Hooki: brak");
        assert_eq!(disable(AppId::Codex, h.path(), Lang::En).unwrap(), "Nothing to remove");
    }

    fn rd(p: &Path) -> Value { serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap() }
    fn dirs(h: &Path) { for d in [".cursor", ".grok", ".zcode"] { std::fs::create_dir_all(h.join(d)).unwrap(); } }

    #[test]
    fn cursor_hooks_need_the_cursor_folder() {
        let h = home();
        let e = enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap_err();
        assert!(e.to_string().contains("~/.cursor"), "{e}");
        assert!(!cursor_hooks_path(h.path()).exists());
    }

    #[test]
    fn cursor_hooks_install_once_and_a_file_we_made_goes_away() {
        let h = home();
        dirs(h.path());
        let f = cursor_hooks_path(h.path());
        assert!(!status(AppId::Cursor, h.path(), Lang::Pl).installed);
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let v = rd(&f);
        assert_eq!(v["version"], json!(1));
        assert_eq!(v["hooks"].as_object().unwrap().len(), CURSOR_EVENTS.len());
        for ev in CURSOR_EVENTS {
            let a = v["hooks"][ev].as_array().unwrap();
            assert_eq!(a.len(), 1, "{ev}");
            assert!(a[0]["command"].as_str().unwrap().ends_with(&format!("{HOOK_FILE} --agent cursor --event {ev}")), "{ev}");
            assert_eq!(a[0]["timeout"], json!(5));
        }
        assert!(!f.with_file_name("hooks.json.agent-pets.bak").exists(), "there was nothing to back up");
        assert!(status(AppId::Cursor, h.path(), Lang::Pl).installed);
        let before = std::fs::read(&f).unwrap();
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), before);
        disable(AppId::Cursor, h.path(), Lang::Pl).unwrap();
        assert!(!f.exists(), "file created by us disappears");
        assert!(!status(AppId::Cursor, h.path(), Lang::Pl).installed);
        assert!(disable(AppId::Cursor, h.path(), Lang::Pl).is_ok());
    }

    #[test]
    fn cursor_hooks_join_someone_elses_hooks_and_leave_them_alone() {
        let h = home();
        dirs(h.path());
        let f = cursor_hooks_path(h.path());
        let original = "{\n  \"version\": 1,\n  \"hooks\": {\"stop\": [{\"command\": \"./audit.sh\"}]}\n}";
        std::fs::write(&f, original).unwrap();
        let bak = f.with_file_name("hooks.json.agent-pets.bak");
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let v = rd(&f);
        assert_eq!(v["hooks"]["stop"][0], json!({"command": "./audit.sh"}), "other entry stays first");
        assert_eq!(v["hooks"]["stop"].as_array().unwrap().len(), 2);
        assert!(v["hooks"]["stop"][1]["command"].as_str().unwrap().contains("--agent cursor --event stop"));
        assert_eq!(std::fs::read(&bak).unwrap(), original.as_bytes());
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert_eq!(rd(&f)["hooks"]["stop"].as_array().unwrap().len(), 2, "no duplicates");
        // old hook.exe path: one entry with the new path, original backup preserved
        let mut v = rd(&f);
        v["hooks"]["stop"][1]["command"] = json!("C:/old/hook.exe --agent cursor --event stop");
        std::fs::write(&f, serde_json::to_string(&v).unwrap()).unwrap();
        assert!(!status(AppId::Cursor, h.path(), Lang::Pl).installed, "old path is not an installation");
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let stop = rd(&f)["hooks"]["stop"].as_array().unwrap().clone();
        assert_eq!(stop.len(), 2);
        assert!(stop[1]["command"].as_str().unwrap().contains(".agent-pets"));
        assert_eq!(std::fs::read(&bak).unwrap(), original.as_bytes());
        disable(AppId::Cursor, h.path(), Lang::Pl).unwrap();
        assert_eq!(rd(&f), json!({"version": 1, "hooks": {"stop": [{"command": "./audit.sh"}]}}));
    }

    #[test]
    fn an_unusual_cursor_file_is_left_as_it_is() {
        let h = home();
        dirs(h.path());
        let f = cursor_hooks_path(h.path());
        for text in ["{not json", "[]", r#"{"hooks":[]}"#, r#"{"hooks":{"stop":{}}}"#] {
            std::fs::write(&f, text).unwrap();
            let e = enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap_err();
            assert!(e.to_string().contains("nic nie zmieniam"), "{text}: {e}");
            let _ = disable(AppId::Cursor, h.path(), Lang::Pl);
            uninstall_all(h.path(), false, Lang::Pl);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), text, "{text}");
            assert!(!status(AppId::Cursor, h.path(), Lang::Pl).installed, "{text}");
        }
        std::fs::write(&f, "[]").unwrap();
        assert!(enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::En).unwrap_err().to_string().contains("leaving it unchanged"));
    }

    #[test]
    fn grok_hooks_install_once_and_uninstall() {
        let h = home();
        let src = hook_src(h.path());
        assert!(enable(AppId::Grok, h.path(), Some(&src), Lang::Pl).is_err(), "missing ~/.grok");
        dirs(h.path());
        enable(AppId::Grok, h.path(), Some(&src), Lang::Pl).unwrap();
        let f = grok_hooks(h.path());
        let v = rd(&f);
        assert_eq!(v["hooks"].as_object().unwrap().len(), GROK_EVENTS.len());
        assert!(GROK_EVENTS.contains(&"StopCancelled"), "interrupted turn");
        for ev in GROK_EVENTS {
            let group = &v["hooks"][ev][0];
            let cmd = &group["hooks"][0];
            assert!(cmd["command"].as_str().unwrap().ends_with(&format!("{HOOK_FILE} --agent grok --event {ev}")), "{ev}");
            assert_eq!((cmd["type"].as_str(), &cmd["timeout"]), (Some("command"), &json!(5)), "{ev}");
            let tool = matches!(ev, "PreToolUse" | "PostToolUse" | "PostToolUseFailure");
            assert_eq!(group.get("matcher").and_then(|m| m.as_str()), tool.then_some("*"), "{ev}");
        }
        assert!(status(AppId::Grok, h.path(), Lang::Pl).installed);
        let before = std::fs::read(&f).unwrap();
        enable(AppId::Grok, h.path(), Some(&src), Lang::Pl).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), before);
        let other = f.with_file_name("other.json");
        std::fs::write(&other, "{}").unwrap();
        disable(AppId::Grok, h.path(), Lang::Pl).unwrap();
        assert!(!f.exists() && other.exists());
        assert!(!status(AppId::Grok, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn someone_elses_grok_agent_pets_json_is_never_touched() {
        let h = home();
        dirs(h.path());
        let f = grok_hooks(h.path());
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let theirs = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"their.sh"}]}]}}"#;
        std::fs::write(&f, theirs).unwrap();
        assert!(enable(AppId::Grok, h.path(), Some(&hook_src(h.path())), Lang::Pl).is_err());
        let _ = disable(AppId::Grok, h.path(), Lang::Pl);
        uninstall_all(h.path(), false, Lang::Pl);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), theirs);
        assert!(!status(AppId::Grok, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn zcode_hooks_join_its_config_and_leave_the_rest_alone() {
        let h = home();
        let src = hook_src(h.path());
        assert!(enable(AppId::Zcode, h.path(), Some(&src), Lang::Pl).unwrap_err().to_string().contains("~/.zcode"));
        let f = zcode_config(h.path(), None);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let theirs = json!({"matcher": "*", "hooks": [{"type": "process", "command": "C:\\tools\\notify.exe", "args": ["done"]}]});
        // ZCode schema (verified in its bundle): events in `hooks.events`, runner only with `hooks.enabled: true`
        let original = json!({"model": "glm-5", "theme": "dark",
            "hooks": {"enabled": true, "timeoutMs": 60000, "events": {"Stop": [theirs.clone()]}}});
        let text = serde_json::to_string_pretty(&original).unwrap();
        std::fs::write(&f, &text).unwrap();
        enable(AppId::Zcode, h.path(), Some(&src), Lang::Pl).unwrap();
        let v = rd(&f);
        assert_eq!((&v["model"], &v["theme"]), (&original["model"], &original["theme"]));
        assert_eq!((&v["hooks"]["enabled"], &v["hooks"]["timeoutMs"]), (&json!(true), &json!(60000)));
        assert_eq!(v["hooks"].as_object().unwrap().len(), 3, "only enabled, timeoutMs, and events");
        assert_eq!(v["hooks"]["events"]["Stop"][0], theirs);
        assert_eq!(v["hooks"]["events"].as_object().unwrap().len(), ZCODE_EVENTS.len());
        let hook = installed_hook(h.path()).to_string_lossy().into_owned();
        for ev in ZCODE_EVENTS {
            let a = v["hooks"]["events"][ev].as_array().unwrap();
            let e = a.last().unwrap();
            assert_eq!(e["matcher"], json!("*"), "{ev}");
            let inner = &e["hooks"][0];
            assert_eq!((inner["type"].as_str(), inner["command"].as_str(), &inner["timeoutMs"]), (Some("process"), Some(hook.as_str()), &json!(5000)));
            assert_eq!(inner["args"], json!(["--agent", "zcode", "--event", ev]));
        }
        assert_eq!(std::fs::read_to_string(f.with_file_name("config.json.agent-pets.bak")).unwrap(), text);
        assert!(status(AppId::Zcode, h.path(), Lang::Pl).installed);
        let before = std::fs::read(&f).unwrap();
        enable(AppId::Zcode, h.path(), Some(&src), Lang::Pl).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), before);
        disable(AppId::Zcode, h.path(), Lang::Pl).unwrap();
        assert_eq!(rd(&f), original);
        assert!(!status(AppId::Zcode, h.path(), Lang::Pl).installed);
    }

    /// Other hooks without `enabled` were disabled and must stay disabled after our edit.
    #[test]
    fn zcode_hooks_switched_on_by_us_are_switched_off_again() {
        let h = home();
        dirs(h.path());
        let f = zcode_config(h.path(), None);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let theirs = json!({"hooks": [{"type": "command", "command": "notify"}]});
        let original = json!({"hooks": {"events": {"Stop": [theirs]}}});
        std::fs::write(&f, serde_json::to_string_pretty(&original).unwrap()).unwrap();
        enable(AppId::Zcode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert_eq!(rd(&f)["hooks"]["enabled"], json!(true));
        assert!(status(AppId::Zcode, h.path(), Lang::Pl).installed);
        disable(AppId::Zcode, h.path(), Lang::Pl).unwrap();
        assert_eq!(rd(&f), original);
    }

    #[test]
    fn zcode_hooks_turned_off_by_the_user_are_left_alone() {
        let h = home();
        dirs(h.path());
        let f = zcode_config(h.path(), None);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let text = r#"{"hooks":{"enabled":false}}"#;
        std::fs::write(&f, text).unwrap();
        let err = enable(AppId::Zcode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap_err();
        assert!(err.left_alone());
        assert!(err.to_string().contains("enabled"), "{err}");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
        assert!(!status(AppId::Zcode, h.path(), Lang::Pl).installed);
    }

    #[test]
    fn zcode_creates_its_cli_folder_and_drops_a_hooks_key_left_empty() {
        let h = home();
        dirs(h.path());
        let f = zcode_config(h.path(), None);
        enable(AppId::Zcode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        assert!(f.is_file());
        let v = rd(&f);
        assert_eq!(v["hooks"]["enabled"], json!(true));
        assert!(ZCODE_EVENTS.iter().all(|ev| v["hooks"]["events"][ev].is_array() && v["hooks"].get(*ev).is_none()));
        disable(AppId::Zcode, h.path(), Lang::Pl).unwrap();
        assert_eq!(rd(&f), json!({}));
        std::fs::write(&f, r#"{"model":"m"}"#).unwrap();
        enable(AppId::Zcode, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        disable(AppId::Zcode, h.path(), Lang::Pl).unwrap();
        assert_eq!(rd(&f), json!({"model": "m"}));
    }

    #[test]
    fn the_zcode_config_follows_its_data_folder() {
        let h = home();
        let base = h.path().join("zc data");
        assert_eq!(zcode_config(h.path(), Some(base.clone())), base.join("cli").join("config.json"));
        assert_eq!(zcode_config(h.path(), None), h.path().join(".zcode").join("cli").join("config.json"));
        assert_eq!(zcode_dir(h.path(), None), h.path().join(".zcode"));
    }

    #[test]
    fn a_broken_zcode_config_is_left_as_it_is() {
        let h = home();
        dirs(h.path());
        let f = zcode_config(h.path(), None);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        // an event directly in `hooks` is an unknown ZCode key, causing rejection of the whole file
        for text in ["{not json", "[]", r#"{"hooks":[]}"#, r#"{"hooks":{"Stop":"x"}}"#, r#"{"hooks":{"Stop":[]}}"#,
                     r#"{"hooks":{"events":[]}}"#, r#"{"hooks":{"events":{"Stop":"x"}}}"#] {
            std::fs::write(&f, text).unwrap();
            assert!(enable(AppId::Zcode, h.path(), Some(&hook_src(h.path())), Lang::Pl).is_err(), "{text}");
            let _ = disable(AppId::Zcode, h.path(), Lang::Pl);
            uninstall_all(h.path(), false, Lang::Pl);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), text, "{text}");
        }
    }

    #[test]
    fn new_hooks_work_from_a_home_with_spaces_and_apostrophes() {
        for name in ["Jan Kowalski", "O'Neil"] {
            let t = home();
            let h = t.path().join(name);
            dirs(&h);
            let src = hook_src(t.path());
            for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { enable(id, &h, Some(&src), Lang::Pl).unwrap(); }
            let hook = installed_hook(&h).to_string_lossy().into_owned();
            #[cfg_attr(not(windows), allow(unused_variables))]
            let c = rd(&cursor_hooks_path(&h))["hooks"]["stop"][0]["command"].as_str().unwrap().to_string();
            let g = rd(&grok_hooks(&h))["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
            // Cursor runs hooks through PowerShell (verified live): `"path" --agent` is a syntax error there
            #[cfg(windows)]
            assert_eq!(ps_program(&c), Some((hook.replace('\\', "/"), 5)), "{c}");
            // Grok Build also runs hooks through PowerShell (docs: `-Command`)
            #[cfg(windows)]
            assert_eq!(ps_program(&g), Some((hook.replace('\\', "/"), 5)), "{g}");
            assert!(g.contains(&hook.replace('\\', "/").replace('\'', "''")), "{g}");
            let z = rd(&zcode_config(&h, None))["hooks"]["events"]["Stop"][0]["hooks"][0].clone();
            assert_eq!(z["command"].as_str(), Some(hook.as_str()));
            assert_eq!(z["args"].as_array().unwrap().len(), 4);
            for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { assert!(status(id, &h, Lang::Pl).installed, "{id:?}"); }
        }
    }

    /// How PowerShell parses a command: (program, invocation element count) or None on syntax error. Parser only; nothing runs.
    #[cfg(windows)]
    fn ps_program(cmd: &str) -> Option<(String, usize)> {
        let script = "$e=$null; $a=[System.Management.Automation.Language.Parser]::ParseInput($env:AP_CMD,[ref]$null,[ref]$e); \
            if ($e.Count) { 'ERR' } else { $c=$a.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true) | Select-Object -First 1; \
            $c.CommandElements[0].Value; $c.CommandElements.Count }";
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("AP_CMD", cmd).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut l = text.lines();
        match (l.next(), l.next()) {
            (Some(p), Some(n)) if p != "ERR" => Some((p.to_string(), n.trim().parse().unwrap())),
            _ => None,
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_plain_cursor_command_reads_the_same_in_powershell() {
        let h = home();
        dirs(h.path());
        enable(AppId::Cursor, h.path(), Some(&hook_src(h.path())), Lang::Pl).unwrap();
        let c = rd(&cursor_hooks_path(h.path()))["hooks"]["stop"][0]["command"].as_str().unwrap().to_string();
        let hook = installed_hook(h.path()).to_string_lossy().replace('\\', "/");
        assert_eq!(ps_program(&c), Some((hook, 5)), "{c}");
        assert_eq!(ps_program("\"C:/a b/hook.exe\" --agent cursor --event stop"), None, "this was the failure");
    }

    #[test]
    fn uninstall_removes_cursor_grok_and_zcode_hooks() {
        let h = home();
        dirs(h.path());
        let src = hook_src(h.path());
        for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { enable(id, h.path(), Some(&src), Lang::Pl).unwrap(); }
        uninstall_all(h.path(), false, Lang::Pl);
        assert!(!cursor_hooks_path(h.path()).exists() && !grok_hooks(h.path()).exists());
        assert_eq!(rd(&zcode_config(h.path(), None)), json!({}));
        for id in [AppId::Cursor, AppId::Grok, AppId::Zcode] { assert!(!status(id, h.path(), Lang::Pl).installed, "{id:?}"); }
    }
}
