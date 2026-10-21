use pets_core::model::*;
use pets_core::store::Store;
use serde::Serialize;

fn name<T: Serialize>(v: T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

pub fn render(store: &Store, now: i64) -> String {
    let mut out = String::from("Agent Pets · data core (Ctrl+C to quit)\n\n");
    out += &format!("{:<7} {:<8} {:<16} {:>7} {:>8} {:>7}  {}\n", "agent", "source", "state", "progress", "context", "quiet", "title");
    for x in store.sessions() {
        let state = match x.tool { Some(t) => format!("{}:{}", name(x.state), name(t)), None => name(x.state) };
        let progress = x.progress.map(|p| format!("{}/{}", p.done, p.total)).unwrap_or_else(|| "-".into());
        let ctx = x.context.map(|c| format!("{}%", c.used * 100 / c.max.max(1))).unwrap_or_else(|| "-".into());
        let quiet = format!("{}s", (now - x.last_activity).max(0) / 1000);
        let title = if x.title.is_empty() { cut(&x.cwd, 50) } else { cut(&x.title, 50) };
        out += &format!("{:<7} {:<8} {:<16} {:>7} {:>8} {:>7}  {}\n",
            name(x.agent), name(x.origin), state, progress, ctx, quiet, title);
    }
    out += "\nLimits:\n";
    for l in store.limits() {
        let w = match l.window { Window::FiveHour => "5h", Window::Weekly => "week", Window::Spend => "spend", Window::Monthly => "month" };
        let reset = l.resets_at.map(|r| format!(" (resets in {} min)", ((r - now) / 60_000).max(0))).unwrap_or_default();
        out += &format!("  {} {}: {:.0}%{}\n", name(l.agent), w, l.used_pct, reset);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pets_core::store::Timing;

    #[test]
    fn renders_sessions_and_limits() {
        let mut s = Store::new(Timing::default());
        let mut e = Event::new(Source::Claude, "s1", Kind::ToolStart, 0);
        e.tool = Some(Tool::Bash);
        e.data.title = Some("Taskbar widget".into());
        e.data.progress = Some(Progress { done: 2, total: 5 });
        e.data.context = Some(Context { used: 50_000, max: 200_000 });
        s.apply(&e);
        let mut l = Event::new(Source::Codex, "c", Kind::Limits, 0);
        l.data.limits = vec![Limit { agent: Agent::Codex, window: Window::FiveHour, used_pct: 42.0, resets_at: Some(3_600_000), stale_since: None }];
        s.apply(&l);
        let out = render(&s, 10_000);
        assert!(out.contains("Taskbar widget"));
        assert!(out.contains("working:bash"));
        assert!(out.contains("2/5"));
        assert!(out.contains("25%"));
        assert!(out.contains("codex 5h: 42%"));
    }
}
