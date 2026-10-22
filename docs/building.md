# Building

- [Requirements](#requirements)
- [Build and test](#build-and-test)
- [Linux](#linux)
- [Run in development](#run-in-development)
- [pets-cli](#pets-cli)
- [Connect Claude Code without the wizard](#connect-claude-code-without-the-wizard)
- [How it works](#how-it-works)
- [Project layout](#project-layout)
- [README media](#readme-media)
- [Releases](#releases)

## Requirements

- Windows 11, or Linux with X11/XWayland (see [Linux](#linux))
- [Rust](https://rustup.rs) 1.93 or newer (MSVC toolchain)
- [Node.js](https://nodejs.org) 22 and [pnpm](https://pnpm.io) 10
- Optional: Python 3, only for the fixture anonymizer in `tools/`

## Build and test

```powershell
git clone https://github.com/Marczelloo/agent-pets.git
cd agent-pets\app
pnpm install
pnpm build:hook      # builds hook.exe into src-tauri/resources; the app bundles it and needs it to compile
pnpm test
cd ..
cargo build --release --workspace
cargo test --workspace
```

To build the installer, run in `app/`:

```powershell
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

It lands in `target\release\bundle\nsis\`. The config override turns off signing the update files, which needs the release key only the maintainer has.

## Linux

The app also builds and runs on Linux (GTK + WebKitGTK through X11/XWayland; `deb`, `rpm` and `AppImage` bundles come from `app/src-tauri/tauri.linux.conf.json`, and the release workflow builds them on Ubuntu 22.04 for an old enough glibc). Build and test like on Windows, minus the `.exe` suffixes; `pnpm tauri build` in `app/` produces the packages in `target/release/bundle/`.

Linux is an experimental platform: development and day-to-day testing happen on Windows 11. CI builds the app and runs the tests on Linux, and before a release the packages are checked by hand in virtual machines (Ubuntu with GNOME, Fedora with KDE, Linux Mint with Cinnamon), but a change is not tried on Linux every time.

The stage is not embedded in a taskbar (X11 has none): it is an always-on-top strip flush against the panel edge of the work area. The app forces `GDK_BACKEND=x11` at startup, because `gtk move()` is ignored on Wayland.

Under XWayland on GNOME or KDE the X server sees the cursor only over the app's own windows, so the floating window there never passes clicks through (it could not tell when the cursor comes back over a pet), and hover ends on X's leave event. X11 key grabs fire there only while one of the app's windows has focus, so on Wayland the global shortcuts go through the `org.freedesktop.portal.GlobalShortcuts` portal (`hotkeys_portal.rs`), falling back to the grabs when the desktop has no such portal. The tray icon needs a StatusNotifier host (stock GNOME: the AppIndicator extension), which hands the app no clicks on the icon itself, so the tray menu starts with "Show panel"; without one the app shows a one-time hint, and launching it again opens the settings.

### To do: window rules for compositors

Compositors draw their own border and shadow around managed windows (the panel, settings), which look wrong around the app's overlays. Users should add rules to disable them. This is a manual step for now - the app should detect the compositor and offer to install them (e.g. into Hyprland's config) in a future release.

Hyprland 0.55+ (Lua config, e.g. omarchy `~/.config/hypr/hyprland.lua`); the window class is `Agent-pets`:

```lua
o.window({ class = "^Agent-pets$" }, { border_size = 0, no_shadow = true })
o.window({ class = "^Agent-pets$", title = "^agent-pets-(stage|bubbles|tooltip)$" }, { no_focus = true, pin = true })
```

`pin` keeps the strip on every workspace, like a real taskbar. For older Hyprland (hyprlang `windowrulev2`) or other compositors, the equivalent is `noborder`, `noshadow`, `nofocus`, and `pin` rules for the same class and titles.

## Run in development

```powershell
cd app
pnpm tauri dev                                                         # live sessions
$env:AGENT_PETS_REPLAY="$PWD\demo\many-sessions.jsonl"; pnpm tauri dev  # a demo recording with 7 sessions
pnpm dev                                                               # browser previews with demo data
```

`pnpm dev` serves previews at `http://localhost:1420/`: `dev.html` (the taskbar stage), `panel.html`, `bubbles.html`, `stats.html` and `settings.html`.

The app and `pets-cli run` cannot run at the same time: both own the local endpoint.

`prototype/index.html` is the original visual prototype of the pets. Open it in a browser; buttons switch states and tools, and the bottom strip shows the real taskbar size.

## pets-cli

`pets-cli` runs the data core in a terminal:

```powershell
target\release\pets-cli.exe run                          # live table of sessions and limits (Ctrl+C to quit)
target\release\pets-cli.exe run --record session.jsonl   # also record normalized events
target\release\pets-cli.exe replay session.jsonl --speed 10
```

To try replay without any agents: `target\release\pets-cli.exe replay crates\pets-cli\tests\data\sample.jsonl --speed 4`.

## Connect Claude Code without the wizard

Copy `hook.exe` somewhere stable first, so rebuilds don't lock the file while hooks run. Use your home directory, not `AppData`: Windows virtualizes `AppData` for apps installed as MSIX packages (such as the Claude desktop app), so a hook started by Claude could see different files there than Agent Pets does.

```powershell
mkdir $HOME\.agent-pets -Force
copy target\release\hook.exe $HOME\.agent-pets\hook.exe
target\release\pets-cli.exe install-hooks $HOME\.agent-pets\hook.exe
target\release\pets-cli.exe uninstall-hooks   # removes only the Agent Pets entries
```

`install-hooks` merges its entries into `~/.claude/settings.json` and keeps a backup as `settings.json.agent-pets.bak`. Claude Code reads hooks when a session starts, so restart running sessions afterwards.

### The Claude Code mod

The mod lives in `claude-plugin/` and is embedded in the app, which places it in `~/.claude/skills/agent-pets` (see [Agents](agents.md#the-claude-code-mod)). Its version must equal the app's: a test in `pets-core` and `scripts/release.ps1` both check it. To work on the mod:

```powershell
cd claude-plugin
claude plugin validate .   # manifest and hooks
claude plugin test .       # the *.test.ts files, no Claude login needed
```

The repo root has a marketplace (`.claude-plugin/marketplace.json`) for manual installs: `claude plugin validate .` checks it. The statusline pass-through of 0.15 is retired; `pets-cli uninstall-statusline` restores a statusline that it replaced.

## How it works

```
Claude Code ──hook.exe──HTTP (127.0.0.1 + token)──┐
opencode ──plugin──HTTP (127.0.0.1 + token)───────┤
Copilot, Antigravity ──hook.exe──HTTP (127.0.0.1)──┤
Cursor, Grok, ZCode ──hook.exe──HTTP (127.0.0.1)───┤
any tool ──hook.exe report / HTTP (door)──────────┤
Codex  ──~/.codex/sessions/**/rollout-*.jsonl──────┼─► adapters ─► state machine ─► taskbar stage
Claude transcripts + ~/.claude/sessions registry ──┤
Agent Router ──~/.agent-router/status.json─────────┘
```

- **`pets-core`** is the library: the data model; the state machine (`thinking`, `working:<tool>`, `needs_you`, `done`, `error`, `idle`, `sleep`, `compacting`, `ended`, with a minimum time per state and inactivity timeouts); the adapters for every agent and the door; the process-tree walk that finds the program hosting an agent; a read-only reader of opencode's database; a file tailer and watcher, the local ingest server and the hooks installer.
- **`hook.exe`** (`pets-hook`) is the hook client. With `report` it is the command-line side of the door; the retired `--agent-pets-statusline` mode only prints your own statusline until the app has restored it.
- **`pets-cli`** runs everything as a terminal app, with record and replay.
- **`app/`** is the Tauri app. The Rust side embeds the stage window in the taskbar (`SetParent` into `Shell_TrayWnd`), measures the free space with UI Automation, follows DPI and Explorer restarts, reads the mouse natively and shows the tooltip window. The TypeScript side draws the pets on a canvas at 30 fps and pauses while the taskbar is hidden or a full-screen app runs. The panel, settings and statistics are React.

## Project layout

```
crates/pets-core    core library: model, state machine, adapters, ingest, watcher, settings, integrations
crates/pets-hook    hook.exe
crates/pets-cli     pets-cli: run, replay, install-hooks, uninstall-hooks, uninstall-statusline
app/                Tauri app: taskbar stage, renderer, skins, tooltip, panel, settings, statistics, installer
prototype/          the original visual prototype of the pets (canvas 2D)
scripts/            release script
claude-plugin/      the Claude Code mod (function-hook plugin, embedded in the app)
.claude-plugin/     marketplace for manual mod installs
tools/              fixture anonymizer, CPU measurement, README media recorder (showcase/), landing page (site/)
site/               the landing page, one self-contained file built by tools/site
docs/               documentation and README images
```

## README media

The GIFs and the banner in `docs/images` are recorded from the real renderer, frame by frame, so every recording comes out the same. It needs ffmpeg on `PATH` and Microsoft Edge.

```powershell
cd app; pnpm dev                    # serves app/showcase.html
cd tools\showcase; pnpm install
node record.mjs                     # all four: pets, states, styles, dynamic
node record.mjs states --zoom=2     # one board, twice as sharp
node record.mjs banner              # docs/images/banner.png, a still at twice the size
node screens.mjs                    # docs/images/{panel,wizard,settings}.png from the demo data (needs the dev server on :1420)
```

`app/showcase.html?mode=gallery|states|styles|dynamic|banner&live=1` shows a board live in the browser.

The banner draws its wordmark in Fredoka and Nunito, the site's fonts, loaded from Google Fonts, so recording it needs a connection. Afterwards make `site/og.png` from it: the banner scaled to 1200 × 400 in the middle of a 1200 × 630 `#FBF1E8` image.

## Releases

Maintainer only. Since 0.7 the app reads `latest.json` from the latest GitHub release and installs only an installer with a valid updater signature. Releases are built by the `release` workflow (`.github/workflows/release.yml`); `scripts/release.ps1` builds the same thing locally when CI is not available. The `ci` workflow only runs the tests.

### Signing key

Updates are signed with a key in `~/.tauri/agent-pets.key` (no password). It is never in the repository. The public key is in `app/src-tauri/tauri.conf.json` under `plugins.updater.pubkey`. Keep a backup of the key: without it, installed copies cannot be updated.

For the workflow, store the key as a repository secret (it reads the file, nothing is printed):

```powershell
Get-Content ~/.tauri/agent-pets.key -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY
```

If the key is lost, generate a new one:

```powershell
pnpm --dir app exec tauri signer generate --ci -w ~/.tauri/agent-pets.key
```

Put the new public key in `tauri.conf.json`, update the secret and ask users to install the next version by hand once.

### Steps

1. Set the same version in `Cargo.toml`, `app/src-tauri/tauri.conf.json`, `app/package.json` and `claude-plugin/.claude-plugin/plugin.json` (`pwsh scripts/version.ps1` checks them; so do the tests in `version::tests` and `integrations::tests`).
2. Move the `## Unreleased` notes in `CHANGELOG.md` under a `## <version>` heading. Its first bullet appears in the "update available" notification.
3. Push the commit, then the tag: `git tag v<version>` and `git push origin v<version>`.
4. The `release` workflow runs the mod checks, builds the app, signs it (see Code signing), signs the update, writes `latest.json` and attaches both files to a **draft** release with the changelog section as its notes. The Linux job adds its packages to the same draft. Both jobs list every file in `SHA256SUMS.txt` and attest where it was built (`gh attestation verify <file> --repo Marczelloo/agent-pets`); the release notes end with how to check a download. Running it by hand (`workflow_dispatch`) only uploads them as a workflow artifact.
5. Check the draft (download and install the installer once), then publish it. The updater reads only published releases, so nothing reaches users before this step.

Locally: write the notes to a file and run `pwsh scripts/release.ps1 -Notes <file>`. It checks the versions, runs `claude plugin validate` and `claude plugin test`, builds the signed installer into `target/release/upload/`, writes `latest.json` and prints the `gh release create` command. Local builds are not code-signed.

### Code signing

The installer, `agent-pets.exe` and `hook.exe` are signed with a free certificate from [SignPath Foundation](https://signpath.org) for open-source projects. Until the project is accepted the workflow skips this step and builds unsigned installers.

Setup, once:

1. Apply at signpath.org for an open-source certificate (the repository must be public with an OSI license, and the README must carry the code signing policy).
2. In SignPath, create the project `agent-pets` with the artifact configurations `binaries` and `installer` from `packaging/signpath/`, and a signing policy `release-signing`. Link the project to the GitHub repository as a trusted build system.
3. In GitHub, add the secret `SIGNPATH_API_TOKEN` (a CI user's token) and the variable `SIGNPATH_ORGANIZATION_ID`. `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_SIGNING_POLICY_SLUG` are variables too, only needed when the names differ from the ones above.

With `SIGNPATH_ORGANIZATION_ID` set, the workflow sends the two executables for signing before they go into the installer, then the installer itself. The updater signature is made last, from the signed installer, so both checks pass. SignPath Foundation may need a manual approval per release; the workflow waits up to an hour for it.

### winget

The package is `Marczelloo.AgentPets`; manifest templates are in `packaging/winget/`.

The first version goes in by hand:

1. Publish a release, then run `pwsh scripts/winget.ps1 -Version <version>`. It downloads the installer, fills in the manifests in `target/release/winget/<version>/` and runs `winget validate`.
2. Fork `microsoft/winget-pkgs`, copy the three files to `manifests/m/Marczelloo/AgentPets/<version>/` and open a pull request. Moderators review the first submission of a package.

After it is merged, set the variable `WINGET_ENABLED` to `true` and the secret `WINGET_TOKEN` (a classic token with the `public_repo` scope, of the account that owns the fork). From then on the `winget` workflow opens the pull request for each published release.

### Testing an update locally

`AGENT_PETS_UPDATE_URL` points the updater at another address, but only in a dev build or a build with the `update-test` feature; a release always asks GitHub, and the signature is always checked. The release build of the updater accepts only `https`, so test builds need an extra config. Never use it for a real release:

```powershell
pnpm --dir app tauri build --features update-test --config '{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}'
```

1. Build and install version N this way, with the signing variables set as in `scripts/release.ps1`.
2. Build version N+1 with `scripts/release.ps1` and serve `target/release/upload` with `python -m http.server 8765 --bind 127.0.0.1`; set `url` in `latest.json` to `http://127.0.0.1:8765/<installer>`.
3. Start version N with `AGENT_PETS_UPDATE_URL=http://127.0.0.1:8765/latest.json` and click **Check now** in Settings → General.
4. Change one character of the signature in `latest.json` and check that the update is rejected.
