# Privacy

Agent Pets reads what your agents do from local files and hooks, and keeps it on your computer.

## Network

Agent Pets makes only these connections to the internet:

- **Update check.** It downloads `latest.json` and, when you install an update, the installer from this repository's GitHub releases. Nothing is sent. Turn it off in Settings → General.
- **Claude plan limits** (opt-in, off by default). Every 5 minutes it asks `api.anthropic.com` for your plan usage with the login Claude Code keeps on your disk. The token is read for each request, sent only to Anthropic and never stored or logged. See [Claude rate limits](agents.md#claude-rate-limits). The Claude Code mod needs none of this: it gets your limits from inside Claude Code.

It never asks any other agent's servers for anything. With Antigravity on, it asks Antigravity's own local server on `127.0.0.1` for the Gemini limits; how Antigravity gets those numbers is up to Antigravity.

## Local connection

The app listens only on `127.0.0.1` and requires a random token, stored with its port in `~/.agent-pets/endpoint.json`. The hooks, the opencode plugin and the [door](door.md) send over it only session states, tool kinds, short action texts (a file name, a command, a search pattern), the question an agent asks, and model names. Never prompts, answers, tool results or file contents. Text from the door is treated as untrusted, cut to size and shown as plain text.

### The Claude Code mod

The [mod](agents.md#the-claude-code-mod) inside Claude Code talks to the app over three routes, all on `127.0.0.1` and both needing the Bearer token from `~/.agent-pets/endpoint.json`:

- **`POST /v1/events/claude-mod`** receives from the mod the session id, working folder, model, context size, cost, how a turn ended and the plan limits (percent used and reset time). No prompts, answers or file contents. A body over 1 MiB is refused, percentages are clamped to 0-100, and an unknown session never creates a pet.
- **`GET /v1/state`** gives the mod the board for `/pets` and the nudges: the visible sessions (id, agent, state, title, question, working folder and start time) and the limits, plus the mod's two switches (pet and nudges, as plain booleans). Hidden and dismissed sessions are left out. It has no other paths than the working folder, and no process ids, tokens or other settings.
- **`POST /v1/actions/claude-mod`** carries the `/pets` commands (jump, open the panel, mute, name, pin, clear inactive, the pet and nudges switches) and answers with the outcome: for a jump the target's title and agent, for a mute the time it ends, for a clear the number of pets removed. Only these commands exist; the app checks that a named session is one it shows.

The mod sends only to `127.0.0.1`, keeps the endpoint token in memory and never writes it to disk. It never reads `~/.claude/.credentials.json`; it gets your limits from Claude Code itself.

## What is read

- **Claude transcripts:** only titles, task progress and token counters. Never message content.
- **Codex session files:** states, tools, context, limits and token counters.
- **opencode's database:** read-only, numbers and names only. Never messages or login data. Not opened with opencode off.
- **Agent Router's status file:** a short title and health per task.

## What is stored

- **In memory only:** the file names, commands, search patterns and questions shown in bubbles, tooltips and the panel. They are never written to disk, logs, the diagnostics report or a `pets-cli --record` file.
- **`~/.agent-pets/stats.json`:** numbers only (tokens, work time, questions, tool calls per hour and model), project folder names, model names and the paths of transcripts already read. Never message content, commands, file names or session titles.
- **`~/.agent-pets/links.json`:** which Agent Router task belongs to which session (ids only), kept for 7 days.
- **`~/.agent-pets/agent-pets.log`:** app errors and a line each time the app starts, with your home folder written as `~`. Never session titles, prompts, commands or tokens. It rotates at 512 KB, keeping one older copy (`agent-pets.log.1`); its last 40 lines are in the diagnostics report.
- **Settings** and backups of the agent config files Agent Pets edited.

The diagnostics report is only copied to your clipboard. **Report a problem** (Settings → Diagnostics, or the tray menu) opens a new GitHub issue in your browser; nothing is sent until you paste the report and submit the form yourself.

Uninstalling removes the hooks, the autostart entry and the files in `~/.agent-pets`; tick "delete app data" to remove your settings as well.
