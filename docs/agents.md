# Agents

Turn each agent on or off in **Settings → Apps** (or in the first-run wizard). Restart open sessions of that agent afterwards so it picks up the change. Every integration writes only its own entries or files, keeps a backup when it edits a file you own, and removes exactly what it added when you turn it off or uninstall.

Claude Code, Codex and opencode are the core agents: they get the most care and testing, and only they have usage and statistics. GitHub Copilot, Antigravity, Cursor, Grok Build and ZCode are *experimental*: they work through their hooks on a best-effort basis, and their pets follow each agent's hook documentation, but not every part was checked against a live session.

- [Claude Code](#claude-code)
- [Codex](#codex) and [Agent Router](#agent-router-tasks)
- [opencode](#opencode)
- [GitHub Copilot](#github-copilot)
- [Antigravity](#antigravity)
- [Cursor](#cursor), [Grok Build](#grok-build), [ZCode](#zcode)
- [Why there is no Claude ghost](#why-there-is-no-claude-ghost)
- Anything else: [the door](door.md)

## Claude Code

The wizard installs hooks in `~/.claude/settings.json` and keeps a backup as `settings.json.agent-pets.bak`. Each hook runs `~/.agent-pets/hook.exe`, a tiny client that forwards the hook payload to the app on `127.0.0.1` and always exits with code 0 within about 0.3 s, even when Agent Pets is closed. It never blocks or slows Claude down. Titles, context and task progress come from Claude's transcripts. To install the hooks without the wizard, see [Building](building.md#connect-claude-code-without-the-wizard).

### The Claude Code mod

Claude Code can load small plugins called mods. Agent Pets ships one and, with Claude Code on, places it in `~/.claude/skills/agent-pets` (Claude Code lists it as `agent-pets@skills-dir`). It adds what hooks cannot see and draws a little UI inside Claude Code. It is an enhancer: the hooks stay the source of each session's state, so with the mod off or broken everything the hooks do keeps working.

- **Live limits.** After every turn the mod sends the 5-hour and weekly limits of your plan, with exact reset times, plus the context size, the cost and the model. This works in the terminal and in sessions hosted by the Claude app, and it never reads your login.
- **`/pets`.** Opens a read-only pane with every visible session of every agent (state, title, the question when one waits), the limit bars of each agent and a line saying whether the pet and the nudges are on (they are switched in Settings → Apps, under Claude Code mod). Esc closes it. Without the widget it shows the current session and its limits, with the line "Agent Pets widget not running".
- **`/pets <command>`.** Controls the widget from Claude Code, also while a turn is running: `jump` (go to the agent that needs you, other than this session; else the one that finished last, within 30 minutes; else the panel), `panel` (the panel on this session), `mute [1h|8|forever]` and `unmute`, `name [text]` (name this session; without text, back to the automatic name), `pin` and `unpin`, `clear` (Clear inactive), `pet on|off` and `nudges on|off`. `/pets help` lists them.
- **Nudges.** On by default; switch them in Settings → Apps. A toast when another session (never the current one) needs you or fails, once per question, and a "N waiting" note in the status line while anything waits.
- **Pet above the prompt.** Off by default; switch it in Settings → Apps. A pixel Clawd with a one-line status that follows the session: idle, thinking, working, waiting for you, done, error and asleep. It needs a terminal at least 60 columns wide and draws nothing in the Claude app, VS Code or `claude -p`.
- **Faster states.** A turn you interrupt, a refusal or a failed turn ends the pet's animation at once, without reading the transcript.

Turn it off or on with **Claude Code mod** in Settings → Apps, under Claude Code (on by default). The change takes effect in new Claude Code sessions. Turning Claude Code off or uninstalling Agent Pets removes the mod too; the app only touches the folder when it is its own.

The mod is early-access technology. A Claude Code update that breaks it can take away the extras, never the hooks.

**Without the widget, or on another system.** The same mod works from the repo's marketplace:

```
/plugin marketplace add Marczelloo/agent-pets
/plugin install agent-pets@agent-pets
```

You get `/pets` for the current session. The pet and the nudges are switched in the app's settings, so without the app the pet stays off. If the app's copy is installed as well, the mod's duplicate guard keeps only one active, so you still get one pet and one stream of reports.

### Claude rate limits

There are three sources, tried in this order; the newest reading wins.

- **The mod** (live, from inside Claude Code). Needs no setup and no sign-in beyond the one Claude Code already has. It updates after every turn in terminal sessions and in sessions hosted by the Claude app, as long as the session started after the mod was installed.
- **Your Claude plan** (opt-in, Settings → Limits). For when no Claude Code session runs. Every 5 minutes the app asks `https://api.anthropic.com/api/oauth/usage` for your plan usage, the same request `/usage` in Claude Code makes, with the login Claude Code keeps in `~/.claude/.credentials.json`. It gives exact percentages and reset times. It needs the Claude CLI installed and signed in (`claude auth login` once). The token is read for each request, sent only to `api.anthropic.com` and never stored or logged. The request is skipped while the mod has reported limits in the last 10 minutes.
- **The Claude app.** It saves your plan usage every 5 to 15 minutes in its data folder; Agent Pets reads the latest sample while the app runs. This source has no reset times, and it works without any Claude Code login. The app pauses polling while the PC is idle or after a long time without interaction, so the last sample can be many hours old and nothing Agent Pets can do refreshes it. Agent Pets keeps showing the last sample with an "as of N h ago" marker (a 5-hour window is dropped after 5 hours, the weekly one after 7 days) and the tooltip and panel point to the CLI sign-in.

The statusline pass-through of earlier versions is gone: the mod replaced it. On the first start of 0.16 the app gives you back your own statusline if the pass-through was installed. `pets-cli uninstall-statusline` still restores it by hand.

## Codex

No setup: its session files in `~/.codex/sessions` are read directly. They give states, tools, context and the 5-hour and weekly limits. Codex subagent threads show as mini pets next to their parent.

### Agent Router tasks

[Agent Router](https://github.com/Marczelloo/agent-router-mcp) lets Claude Code hand tasks to Codex, so its tasks show up as Codex pets. With a router version that writes `~/.agent-router/status.json`, the pet also gets:

- the task's title;
- a small router badge;
- the task's health in the tooltip and panel: active, quiet (no events for 30 s), stalled or blocked;
- an error state when the task failed or ran out of quota.

A task delegated by a Claude Code session or a Codex thread belongs to that session and shows as its mini pet. The link (task id → session id, nothing else) is kept in `~/.agent-pets/links.json` for 7 days. The status file holds only a short title per task, never the task text, diffs or commands.

## opencode

Agent Pets writes one plugin file, `~/.config/opencode/plugins/agent-pets.js`, marked as ours on its first line. Turning opencode off deletes it; a file with that name that is not ours is never touched.

The plugin sends the session state, the kind of tool, a short action text (file name, command, search pattern or host), the text of a permission request or question, and the model name to `127.0.0.1`. It never sends messages, answers or file contents. Each call gives up after 300 ms and never throws, so opencode works the same when Agent Pets is closed.

### opencode usage

With opencode on, Agent Pets reads opencode's database, `~/.local/share/opencode/opencode.db`, read-only:

- **On the card in the panel:** tokens of the session and of today, and the cost when above zero.
- **Account bars:** when the session runs on a Claude or ChatGPT subscription through opencode, the card shows that account's 5-hour and weekly bars. Other providers have no bars: that would need a request to their servers with your login, which Agent Pets never makes.
- **Statistics:** opencode sessions join Claude and Codex in the statistics window.

Only the `session`, `message` and `part` tables are read, and from them only numbers, model and provider names, project folders and tool names; never message text or login data. With opencode off the database is not opened.

## GitHub Copilot

*Experimental.* Agent Pets writes its own file, `~/.copilot/hooks/agent-pets.json`, read by Copilot CLI and by the agent in VS Code; JetBrains IDEs that run their Copilot agent on Copilot CLI read it too. Turning Copilot off deletes the file; other files in `hooks/` are never touched.

The hook sends the session state, the kind of tool, a short action text, the text of a permission request and the model name. It never sends prompts, answers, tool results or file contents, prints nothing and always exits 0 within 300 ms, so it never changes what Copilot decides. Subagents show as mini pets.

## Antigravity

*Experimental.* Needs an Antigravity version with hooks (Antigravity 2.0, the CLI, or a recent IDE). Agent Pets adds one entry, `agent-pets`, to `~/.gemini/config/hooks.json` after backing the file up (`hooks.json.agent-pets.bak`); your own hooks stay as they are.

The pet thinks, works, finishes and shows errors, with the model name. Antigravity's hooks do not report when it waits for your approval, so this pet never shows "needs you". Agent Pets does not hook `PreToolUse`, because in Antigravity that hook is a permission gate where every answer decides something; the pet shows a tool once it has run, and your approval settings stay as they are.

**Gemini limits.** While Antigravity is running, its pet brings the 5-hour and weekly Gemini bars, with reset times in the panel. Once a minute Agent Pets asks Antigravity's own local server on `127.0.0.1` for the numbers Antigravity shows in its quota view. That server needs the token Antigravity passes on its command line; Agent Pets reads it from there each time, sends it only to that local server and never stores or logs it. Only the Gemini pool is shown. The Antigravity CLI keeps its token to itself, so with only the CLI open there are no bars. When you close Antigravity the bars go away.

The Android robot pet and the Gemini limits were contributed by [al3ksh](https://github.com/al3ksh).

## Cursor

*Experimental, off until you turn it on.* Agent Pets adds its entries to `~/.cursor/hooks.json`, used by the agent in the Cursor editor and by `cursor-agent` in a terminal, after backing the file up (`hooks.json.agent-pets.bak`). Restart Cursor afterwards.

The pet follows prompts, tool calls, subagents, compacting, the end of a turn and errors, with the model name. Cursor's hooks do not report permission prompts, so this pet never shows "needs you". The hook answers Cursor with an empty decision and never changes what it does. It never sends your prompt, attachments, e-mail address, tool results or file contents. Cursor's cloud agents do not run hooks on your computer, so they get no pet.

## Grok Build

*Experimental, off until you turn it on.* Agent Pets writes its own file, `~/.grok/hooks/agent-pets.json`; turning Grok Build off deletes it.

The pet thinks, works, waits for you on permission prompts, finishes, shows errors and compacts. Tool names are matched by keyword, so an unknown tool shows as plain work. Grok Build runs in a terminal, so **Open** brings that terminal forward.

## ZCode

*Experimental, off until you turn it on.* Agent Pets adds its entries under `hooks.events` in `~/.zcode/cli/config.json` (or under `ZCODE_DATA_BASE_DIR`), after backing the file up (`config.json.agent-pets.bak`). ZCode runs hooks only with `hooks.enabled: true`, so Agent Pets sets it; if you turned hooks off yourself, it leaves the file alone and tells you so.

The pet thinks, works, waits for you on permission requests and finishes. ZCode's hooks do not report errors or the end of a session, so after an error the pet goes idle after a quiet spell.

## Why there is no Claude ghost

Cursor, Grok Build and ZCode also run the Claude Code hooks from `~/.claude/settings.json`, and Grok also runs Cursor's hook file. Without care, every Cursor session would show up as a second, fake Claude pet. `hook.exe` checks who really called it and sends nothing when the caller is not the agent the hook was written for. With a new integration off you see no pet for that agent at all; with it on you see exactly one.
