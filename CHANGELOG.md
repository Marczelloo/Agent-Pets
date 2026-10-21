# Changelog

## 0.19.0

### Added

- `/pets` commands in the Claude Code mod control the widget from the prompt, also while a turn runs: `/pets jump` goes to the agent that needs you, `panel` opens the panel on this session, `mute [1h|8|forever]` / `unmute`, `name [text]`, `pin` / `unpin`, `clear` removes inactive pets, and `pet on|off` / `nudges on|off` switch the mod. `/pets help` lists them. They go through a new `POST /v1/actions/claude-mod` (localhost, with the token).
- A Codex account on the free plan shows its limit again: the free plan has a single monthly window instead of 5 h and weekly, and it now gets its own "monthly" row in the panel, a bar on the taskbar, the tooltip and notifications. Switching plans swaps the windows by the newest reading.

### Fixed

- Clicking a pet in the taskbar no longer sometimes opens the panel only for it to vanish at once: the click now counts on release, like Windows flyouts, so letting go of the button no longer hands focus back to the taskbar.
- The panel opens and closes smoothly: it rises and fades in, and fades out before the window hides, with no white or empty frame flashing up first.
- A Codex pet no longer stays in the question pose with its bubble after you answer: work Codex does while it waits for the answer shows as work, and the answer ends the wait.

## 0.18.0

### Added

- Linux support (experimental: Agent Pets is developed and tested day to day on Windows; Linux was tested in virtual machines with Ubuntu, Fedora and Linux Mint before this release): the pets stand in an always-on-top strip along the bottom of the screen on X11/XWayland (GTK + WebKitGTK), at the bottom right, bottom left or in a floating window, with `deb`, `rpm` and AppImage packages, MPRIS media reactions and cross-platform agent integrations. Settings only offer what works on Linux and talk about the system instead of the Windows taskbar. On Wayland the global keyboard shortcuts go through the desktop's GlobalShortcuts portal (the desktop asks to confirm them once), and compositor border/shadow rules (e.g. Hyprland) are a manual step for now (docs/building.md).

### Fixed

- After an update the taskbar and Start show the new app icon right away, instead of the old one Windows kept in its icon cache.
- A session with a very long title no longer stretches its ⋯ menu out of the panel: the menu keeps a sensible width and a long item ends with an ellipsis (the full text shows on hover).

## 0.17.2

### Changed

- A new, simpler app icon: the pet peeking over the edge of the taskbar, readable down to 16 px.

### Fixed

- A pet waking up (and any change between poses) no longer stutters: its breathing and arm swing keep their rhythm instead of jumping while the pose changes.
- Grok's arms look natural: a raised arm bends at the elbow instead of growing long and stiff, and a hand on the hip rests on Grok's own waist instead of sticking out sideways.
- A happy pet's eyes close into arcs as one: Grok and the panda no longer show an open eye and a smile arc at the same time.
- Grok's happy and sleepy eye arcs no longer touch each other inside the visor.

## 0.17.1

### Changed

- Notifications clean themselves up: a limit warning goes after 6 hours and replaces an earlier one for the same agent and window, everything else goes after a day; news about an update waiting to be installed stays.
- The preview pet picker in Look is a grid of equal tiles in balanced rows (5 + 4, or 3 × 3 in a narrow window); the chosen pet is tinted in its colour.

### Fixed

- Limit notifications are no longer squeezed into a narrow column with the title cut off.
- The Statistics window has the same slim, themed scrollbar as the panel.
- The "+N" for extra subagents sits between the parent and its minis, level with them, instead of at the far end of the group where it looked like it belonged to the neighbouring pet.
- A scrollbar appearing in the panel no longer squeezes the cards; it is slimmer and follows the theme.

## 0.17.0

### Added

- Background integration repair and update failures appear in notifications and open Diagnostics for a copyable report.
- Weekly stats compare this calendar week with last week, with an optional Monday recap notification.
- Export settings to a dated JSON file and import them on another PC; installed agent integrations, Claude token consent and notification mute state stay local.
- Two global shortcuts: Ctrl+Alt+Shift+J jumps to the agent that needs you (the one waiting longest, else the latest one that finished in the last 30 minutes, else opens the panel) and Ctrl+Alt+Shift+K shows or hides the panel. Change or turn them off in Settings → General.
- Rename a session or pin it from the card's ⋯ menu. The name shows in the panel, tooltips, toasts and the Claude mod; pinned sessions sit right after the ones waiting for you and survive "Clear inactive".
- The Limits tab says when a limit will hit 100% at the current pace, and a toast warns once when a 5h or weekly limit will run out within the hour, well before it resets (needs the limit notifications on).
- A toast when a limit that went past 90% resets (needs the limit notifications on). It also comes when the reading has gone stale or the usage simply drops.
- Mute notifications for an hour, until 8:00 or until you turn them back on: from the tray menu or Settings → Notifications. Windows toasts stay quiet, the list in the panel keeps filling and says until when.
- A second toast when an agent has been waiting for you for 10 minutes. The done toast says how long the turn took and how many tokens it used.
- Settings → Notifications: switch the toast sound off; a question waiting for you has its own sound.
- Claude's spend limit shows up next to the 5h and weekly limits (Limits tab, tooltips, `/pets`) when Claude Code reports one. It can read above 100%, and a toast says when it is reached.

### Changed

- Releases are built and drafted on GitHub Actions from a version tag.
- The stage draws fewer frames when nothing moves: 20 fps for resting pets, 10 when they all sleep, 4 for an empty stage. Anything that changes brings back the full rate for a few seconds.

### Fixed

- Pressing Esc in Claude Code, or a failed turn, ends its subagents' pets right away instead of leaving them to time out. Background subagents keep running.
- Agent Router tasks link to their session whatever name the router's MCP server is registered under.
- Tokens, keys and passwords in commands are masked (•••) in bubbles, tooltips and the panel.
- Speech bubbles no longer watch the cursor while none is shown.
- A session running in a Windows Terminal window that was started from an IDE is named after that terminal and jumps to its window, not to the IDE.
- The copied resume command uses `Set-Location -LiteralPath '…'`, so folders with spaces, quotes, `$` or `&` paste safely into PowerShell.
- Text reported through the door (`hook.exe report`) drops direction marks and control characters, so it cannot disguise itself, and a pid it reports can only bring forward a terminal or IDE window.
- The statistics window computes its view off the main thread, so a long history no longer stalls the app while it opens.
- Sessions started in a temporary folder no longer show up as projects in the statistics rankings; their usage still counts in the totals.
- The session cost in the panel follows the UI language (0,42 $ in Polish, $0.42 in English).
- Update news in the panel's notifications looks like the other entries and stays on top. Only the latest one is kept, and "Updated to version" no longer shows an Install button; Install appears only while that version still waits to be installed.

## 0.16.1

### Changed

- The terminal pet and the nudges are switched in Settings → Apps, under Claude Code mod, instead of in `/pets`. The terminal pet is now off by default.
- Session cards in the panel no longer repeat the limit bars; they stay on the Limits tab.
- CI fails when the mod does not validate or its tests fail.

## 0.16.0

### Added

- **Claude Code mod.** A plugin for Claude Code's new mod API, installed by the app into `~/.claude/skills/agent-pets` and available from the repo marketplace (`/plugin marketplace add Marczelloo/agent-pets`, then `/plugin install agent-pets@agent-pets`).
  - Live Claude 5-hour and weekly limits with exact reset times after every turn, in terminal and Claude app sessions, without reading your login.
  - `/pets`: a pane with the sessions and limits of every agent, and switches for the pet and the nudges.
  - Nudges when another agent needs you or fails.
  - A pixel Clawd with a status line above the prompt.
  - Context, cost and model per session, and an interrupted, refused or failed turn ends the pet's animation at once.
- **Claude Code mod** switch in Settings → Apps (on by default; takes effect in new Claude Code sessions).
- `POST /v1/events/claude-mod` and `GET /v1/state` on the local connection (localhost only, with the token).
- Seven more Claude Code hook events: `PermissionRequest`, `PostToolUseFailure`, `SubagentStart`, `StopFailure`, `PostCompact` and `Elicitation`/`ElicitationResult`. Existing installs are repaired on update.

### Changed

- Claude limits come first from the mod, then from opt-in plan polling (skipped while the mod reports), then from the Claude app's samples.
- Permission prompts show their bubble immediately, and a failed tool no longer leaves the pet "working".
- The release script and CI validate and test the mod; its version must equal the app's.

### Fixed

- The pet no longer keeps "thinking" after a finished turn when a later measure arrived before the Stop hook.
- An error calms down to idle after two minutes, like a finished turn, instead of staying until the session ends.
- Every card's ⋯ menu in the panel can remove the pet, not only cards of finished sessions.

### Removed

- The statusline pass-through and its switch. On the first start of 0.16 the app restores your own statusline if the pass-through was installed. `pets-cli uninstall-statusline` remains for restoring one by hand.
