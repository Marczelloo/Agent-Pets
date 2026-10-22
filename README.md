<p align="center">
  <img src="docs/images/banner.png" alt="Agent Pets banner: the Agent Pets wordmark with all nine pets perched on its letters: the Grok robot waves hi, the ZCode panda wears headphones, the Cursor block claps, Kodek, a blob and Clawd sit on the letters, the Copilot pilot sends hearts, the Android robot cheers and the opencode cyclops throws a paper plane; under it the tagline &quot;Your coding agents, alive on the Windows 11 taskbar&quot; and a Windows 11 taskbar" width="100%">
</p>

<p align="center">
  <a href="https://github.com/Marczelloo/agent-pets/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/Marczelloo/agent-pets?include_prereleases&color=D97757"></a>
  <img alt="Windows 11" src="https://img.shields.io/badge/Windows-11-5DCAA5">
  <img alt="Linux: experimental" src="https://img.shields.io/badge/Linux-experimental-E8B04B">
  <a href="LICENSE"><img alt="License: GPL-3.0-or-later" src="https://img.shields.io/badge/license-GPL--3.0-8C887E"></a>
</p>

# Agent Pets

Animated pets in the Windows 11 taskbar that show what your coding agents are doing. Every session of **Claude Code**, **Codex**, **opencode**, **GitHub Copilot**, **Antigravity**, **Cursor**, **Grok Build** or **ZCode** gets its own pet: it codes at a desk, types commands, reads files, catches web pages with a butterfly net, waves when it needs you and naps when idle. Progress, context and rate limits sit right next to it. There is also an [experimental Linux version](#linux-experimental).

<p align="center">
  <img src="docs/images/pets.gif" alt="Animated gallery of all pets: Clawd editing, Kodek browsing, the opencode cyclops running a command, the Copilot pilot reading, the Android robot thinking, the Cursor block searching, the Grok robot delegating, the ZCode panda done and a blob pet that needs you" width="900">
</p>

<p align="center">
  <img src="docs/images/taskbar.png" alt="Five pets in the taskbar: a knocked-out Codex robot after an error, a Codex robot presenting a web page, Clawd waving because it needs you, a Codex robot with the Agent Router badge, Clawd typing at a desk; a +2 badge and rate-limit bars" width="900">
</p>

## Features

- **One pet per session**, posed by what the agent does: thinking, editing, running a command, reading, searching, browsing, delegating, waiting for you, done, error, asleep.
- **Limits at a glance.** 5-hour and weekly bars for Claude, Codex and Antigravity, task progress under each pet, a "+N" badge when the taskbar runs out of room. Claude limits come live from the Claude Code mod, with exact reset times; opt-in plan polling and the Claude app's own samples fill the gaps. See [Agents](docs/agents.md#claude-rate-limits).
- **A mod inside Claude Code.** Live limits, a `/pets` pane with every agent's sessions, `/pets jump`, `mute`, `name`, `pin` and `clear` to control the widget from the prompt, nudges when another agent waits or fails, and a pixel Clawd above the prompt. On by default, and installable from the marketplace without the widget (`/plugin marketplace add Marczelloo/agent-pets`). See [Agents](docs/agents.md#the-claude-code-mod).
- **One click back to the session.** The panel lists every session; **Open** brings back the Claude or Codex app, the editor, the terminal, or resumes the session in a new one.
- **Speech bubbles and subagents.** See the question an agent asks or the command it runs; subagents show up as mini pets next to their parent.
- **Seven looks, two ways to move.** Sticker, Sketch, Clean, Pixel art, Neon, Ink and Pastel, plus a Dynamic mode with anime-inspired scenes.
- **Statistics for fun.** A podium of your projects, token and time counters, an agent race, an activity calendar and badges.
- **Your layout.** Next to the tray, on the left, anywhere you drag them or in a floating window; any monitor, any size.
- **Private and light.** Everything is read from local files and hooks; drawing pauses under full-screen apps and slows down on battery.

More in [docs/features.md](docs/features.md).

### Every state

<p align="center">
  <img src="docs/images/states.gif" alt="Clawd in twelve states: thinking, editing, running a command, reading, searching, browsing, delegating, needs you, done, error, compacting and asleep" width="900">
</p>

### Seven looks

<p align="center">
  <img src="docs/images/styles.gif" alt="Kodek thinking and Clawd editing in the seven looks: Sticker, Sketch, Clean, Pixel art, Neon, Ink and Pastel" width="900">
</p>

### Dynamic motion

<p align="center">
  <img src="docs/images/dynamic.gif" alt="Dynamic motion scenes: a punch barrage, ninja hand seals, a detective with a magnifier, shadow thinking, a thunder dash, a summoning seal and Hollow Purple while compacting" width="900">
</p>

## Install

1. Download `Agent.Pets_<version>_x64-setup.exe` from [Releases](https://github.com/Marczelloo/agent-pets/releases/latest) and run it. It installs for your user, without administrator rights. Code signing is being set up (see [Code signing policy](#code-signing-policy)); until then SmartScreen may ask: **More info → Run anyway**.
2. The first-run wizard finds your agents and connects them, and lets you pick notifications, autostart and a look.
3. Restart open agent sessions so they pick up the hooks.

Change anything later in **Settings** (right-click the tray icon, or ⚙ in the panel). Updates are signed and install from GitHub; uninstall from Windows Settings → Apps, which also removes the hooks.

### Linux (experimental)

> **Linux support is experimental.** Agent Pets is developed and tested day to day on Windows 11, and Windows is its main platform. The Linux version was tested before the release, in virtual machines with Ubuntu (GNOME), Fedora (KDE) and Linux Mint (Cinnamon), but not every change gets checked there. Expect rough edges, and please [report what breaks](https://github.com/Marczelloo/agent-pets/issues/new).

`.deb` (Ubuntu, Mint, Debian), `.rpm` (Fedora, openSUSE) and `.AppImage` packages are on the same [Releases](https://github.com/Marczelloo/agent-pets/releases/latest) page. The pets sit in a strip along the bottom of the screen (above the bottom panel when there is one, rather than inside it), at the bottom right or bottom left (Settings → Screen), or in a floating window, on X11 and on Wayland (through XWayland). For now: no auto-update (install the new package), the tray icon opens a menu on any click (its first item shows the panel) and on stock GNOME needs the AppIndicator extension (otherwise launch the app again to open Settings), and on GNOME/KDE Wayland the floating window catches clicks over its empty space. On Wayland the global keyboard shortcuts go through the desktop's GlobalShortcuts portal (KDE Plasma, GNOME 48 and newer), which asks you to confirm them once; on a desktop without it they may only fire while an Agent Pets window is active. If the pets show up blank or leave trails, start the app with `WEBKIT_DISABLE_DMABUF_RENDERER=1` (or `=0`: by default the DMABUF renderer is turned off only on the NVIDIA driver).

## Supported agents

| Agent | Pet | Status | Limits |
|---|---|---|---|
| Claude Code | Clawd | ✅ core | 5h and weekly |
| Codex, incl. [Agent Router](https://github.com/Marczelloo/agent-router-mcp) tasks | Kodek | ✅ core | 5h and weekly |
| opencode | a near-black cyclops | ✅ core | tokens, cost, account bars |
| GitHub Copilot | a pilot in a leather helmet | 🧪 experimental | – |
| Antigravity | the Android robot | 🧪 experimental | Gemini 5h and weekly |
| Cursor | a dark faceted block | 🧪 experimental | – |
| Grok Build | a white robot with a visor | 🧪 experimental | – |
| ZCode | a panda with a “Z” headband | 🧪 experimental | – |
| Anything else | a blob with its first letter | ✅ [the door](docs/door.md) | – |

Claude Code, Codex and opencode get the most care and testing; the experimental ones work through their hooks and were not checked in every detail against a live session. Setup details, what each integration writes and how to undo it: [docs/agents.md](docs/agents.md).

| Panel | First-run wizard | Settings |
|---|---|---|
| <img src="docs/images/panel.png" alt="Panel with the session list: a main session with its subagents and two more sessions" width="260"> | <img src="docs/images/wizard.png" alt="Wizard step: pet appearance, motion and a gallery of seven styles" width="400"> | <img src="docs/images/settings.png" alt="Settings window, Look tab with the live preview and all scenes" width="400"> |

## Privacy

Session data never leaves your computer: states come from local hooks and files, and only titles, progress and counters are kept, never message content. The only network calls are the update check on GitHub (can be turned off) and, if you allow it, fetching your Claude plan limits from `api.anthropic.com`. Details in [docs/privacy.md](docs/privacy.md).

## Code signing policy

From 0.17 on, release installers are built from this repository by the [release workflow](.github/workflows/release.yml) on GitHub Actions, not on a personal computer. Code signing of `Agent.Pets_<version>_x64-setup.exe`, `agent-pets.exe` and `hook.exe` is being requested from [SignPath Foundation](https://signpath.org); once it is granted, this section will say: free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- Committers and reviewers: [Marczelloo](https://github.com/Marczelloo)
- Approvers: [Marczelloo](https://github.com/Marczelloo)

Every release is also signed for the in-app updater, which installs only an update with a valid signature. Releases after 0.19.0 list the SHA-256 of every file in `SHA256SUMS.txt` and carry a [build provenance attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds): `gh attestation verify <file> --repo Marczelloo/agent-pets` confirms the file was built by the release workflow from this repository. [Privacy](docs/privacy.md) lists every network call the app makes; it sends no other data anywhere.

## Documentation

- [Features](docs/features.md): panel, notifications, bubbles, subagents, statistics, layout
- [Agents](docs/agents.md): setup and behaviour of every integration, rate limits
- [Door for other agents](docs/door.md): give any tool a pet with one HTTP call
- [Privacy](docs/privacy.md): what is read, sent and stored
- [Building](docs/building.md): build from source, `pets-cli`, architecture, releases
- [Contributing](CONTRIBUTING.md)

## License

Agent Pets © 2026 [Marczelloo](https://github.com/Marczelloo), licensed under the [GNU General Public License v3.0 or later](LICENSE). You may use, study, change and share it, also commercially; if you share a changed version, it must stay open source under the same license. Versions up to 0.12.2 were released under the MIT License; see [NOTICE](NOTICE) for earlier terms, contributors, trademarks and third-party artwork.

The Antigravity pet is based on the Android robot, which is reproduced or modified from work created and shared by Google and used according to terms described in the [Creative Commons 3.0 Attribution License](https://creativecommons.org/licenses/by/3.0/).

Agent Pets is not affiliated with Anthropic, OpenAI, GitHub, Google, Cursor, xAI, Z.ai or opencode.
