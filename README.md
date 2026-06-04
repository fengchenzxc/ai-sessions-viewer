<div align="center">

# Claude Session Viewer

**English** · [中文](README.zh-CN.md) · [日本語](README.ja.md) · [CHANGELOG](CHANGELOG.md)

<p align="center">A native desktop browser for <strong>Claude Code</strong>, <strong>Codex</strong>, and <strong>Gemini CLI</strong> — read, search, and manage local session transcripts from all three in one place.</p>

<p align="center">
<strong>Faithful replay</strong> — thinking chains, tool-call pairings, structured diffs, inline screenshots.<br/>
<strong>Fast search</strong> — cross-project global hit (<strong>⌘⇧F</strong>) jumps to the exact message; one-click resume in Terminal.<br/>
<strong>Deep stats</strong> — aggregate token spend and cost; slice by project, model, or tool.<br/>
<strong>Read-only safety</strong> — original JSONL is never touched; delete is a move to shared trash, never <code>rm</code>.<br/>
<strong>Flexible export</strong> — single session or batches to offline-readable Markdown or HTML.
</p>

[![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=fff)](https://tauri.app)
[![Vue 3](https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=fff)](https://vuejs.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<br />

<img src="docs/screenshots/cover.png" alt="Claude Session Viewer — unified browser for Claude Code, Codex and Gemini CLI sessions" width="820" />

</div>

---

## Why

Claude Code, Codex, and Gemini CLI each write their session JSONL files to disk, but in different layouts and through different CLIs. None ships a built-in browser. This app gives you a single timeline across all three:

| Agent | Path | Grouping |
| --- | --- | --- |
| Claude | `~/.claude/projects/<dir>/<sessionId>.jsonl` | by project directory |
| Codex | `~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl` | by the `cwd` recorded inside each file |
| Gemini | `~/.gemini/tmp/<slug>/chats/session-*.jsonl` | by `slug`; cwd read from the `.project_root` sibling |

The app is **read-only** against the originals — deletion is a soft move into `~/.claude/.session-viewer-trash/`, never `rm`.

## Changes in this fork

This repository is maintained at [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer) and keeps the same Tauri + Vue + Rust architecture. Compared with the original project, it adds:

- **Codex desktop parity** — Codex sessions are cross-checked against Codex app metadata, including app-list rank, first-page position, archived status, and internal guardian / auto-review subthreads.
- **Codex filters and labels** — settings can independently show or hide Codex internal review sessions and archived sessions; the session list marks them as review or archived sessions.
- **Terminal preference** — session resume can open in Warp, Terminal.app, or iTerm2. The app now launches temporary executable scripts instead of AppleScript typing, so no Accessibility permission is required.
- **Safer temporary scripts** — terminal resume scripts are hidden `.tmpXXXXXX` files created inside the project directory, run `codex resume <session_id>`, and self-clean after launch with a backend cleanup fallback.
- **Codex-style themes and font sizing** — added Codex light and Dracula dark themes, macOS font smoothing, 14px UI baseline, and 12px code / diff baseline.
- **Stats and parser hardening** — expanded Codex/Gemini parsing, pricing/model normalization, tool/activity classification, cancellation behavior, and regression tests.
- **Repository migration** — release/update links point to [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer), with GitHub Actions builds publishing installers from tags.

## Features

- 🗂 **Unified project view** — group sessions by working directory across both CLIs
- 💬 **Faithful chat replay** — text, thinking blocks, tool calls, structured diffs, inline images
- 🔎 **In-session search with scope** — search across the whole conversation or scope to user messages, agent replies (incl. edits), or tool noise; prev / next jump + match counter
- 🌐 **Global search (⌘⇧F / Ctrl+Shift+F)** — Algolia-style overlay over the current agent, scoped to session titles and your own messages; click a hit to jump straight to that message with a flash highlight; recent queries with single-item removal
- 🔃 **Session list search & sort** — keyword search runs on the Rust side, matching session titles and your message text (cancellable mid-typing); sort by recency / size / message count, or show only ones with an ID
- 🪗 **Collapse / expand all tool calls** — one click to hide tool-call clutter and focus on the conversation
- 📤 **Export session** — save a single session to Markdown or HTML (native Save-As, offline-renderable HTML with inlined avatars / styles)
- 🧰 **Multi-select & batch ops** — pick sessions in bulk to move them to the trash or export them into a single `export-YYYYMMDD-HHMMSS-{md,html}/` folder
- 🔄 **Resume or start fresh** — open Terminal in a project to resume an existing session (`claude --resume <id>` / `codex resume <id>`) or start a brand-new one
- 📡 **Live tail** — opened session auto-refreshes as the CLI appends new messages; an "● Live" indicator shows the watcher is active, and a "N new ↓" pill surfaces additions when you've scrolled up
- 🗑 **Shared trash** — soft-delete, preview a deleted session's transcript, restore one or many (multi-select); survives across both agents
- 🏠 **Welcome screen** — recently opened projects per agent with one-click reopen + per-entry removal
- 📌 **Pin / sink projects** — color-coded pins on the sidebar; sunk projects go to the bottom
- ✏️ **Rename sessions** — your new title syncs back to the CLI, so `claude` / `codex` resume pickers show it too
- 🌗 **Light / dark / system theme** — Codex-inspired neutral palette with brand-color accents
- 🌐 **i18n with auto-detect** — English / 简体中文 / 繁體中文 / 日本語; first launch matches the OS language, falls back to English
- ⚡️ **Custom tooltip & agent brand icons** — no out-of-place native chrome
- 🖼 **Image lightbox** for screenshots embedded in transcripts

## Install

### Pre-built

Grab the latest installer from [Releases](https://github.com/fengchenzxc/ai-sessions-viewer/releases):

| Platform | File |
| --- | --- |
| macOS (Apple Silicon + Intel) | `ai-sessions-viewer_<ver>_universal.dmg` |
| Windows x64 | `ai-sessions-viewer_<ver>_x64-setup.exe` |
| Linux x86_64 (Debian/Ubuntu) | `ai-sessions-viewer_<ver>_amd64.deb` |
| Linux x86_64 (portable) | `ai-sessions-viewer_<ver>_amd64.AppImage` |

On macOS the `.app` is **ad-hoc signed but not notarized**, so first launch may show *"Apple cannot verify…"*. Two ways past it:

- Right-click the app in Finder → **Open** → confirm in the dialog (one-time).
- Or strip the quarantine attribute in Terminal:
  ```bash
  sudo xattr -dr com.apple.quarantine /Applications/ai-sessions-viewer.app
  ```

On Linux the `.AppImage` is portable — `chmod +x` and run. The `.deb` installs with:
```bash
sudo apt install ./ai-sessions-viewer_<ver>_amd64.deb
```

### Build from source

Prereqs: **Node 20+**, **Rust stable**, plus the platform-specific toolchain:
- **macOS** — Xcode CLT.
- **Windows** — MSVC + WebView2.
- **Linux** — `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `libxdo-dev`, `patchelf` (on Debian/Ubuntu: `sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libxdo-dev patchelf`).

```bash
git clone https://github.com/fengchenzxc/ai-sessions-viewer.git
cd ai-sessions-viewer
npm install
npm run tauri dev          # dev shell
npm run tauri build        # bundle .app / .dmg / .msi / .deb / .AppImage
```

`npm run build` is the typecheck step (`vue-tsc --noEmit` + Vite build). Unit tests live under `test/` on Vitest — `npm test` for watch mode, `npm run test:run` for a single CI run, `npm run test:coverage` for a v8 coverage report.

## Usage

1. **Switch agent** — segmented control at the top of the sidebar (Claude 🟠 / Codex 🟢)
2. **Pick a project** — sidebar lists every working directory; right-click for pin / sink / rename
3. **Open a session** — center column renders messages + tool calls grouped by call → result
4. **Resume** — toolbar ▶ button opens Terminal with the right CLI
5. **Export** — chat toolbar ⬇ saves a single session as Markdown / HTML; multi-select sessions in the list, then the topbar ⬇ exports them all to an `export-YYYYMMDD-HHMMSS-{md,html}/` folder
6. **Delete / restore** — toolbar 🗑 soft-deletes; trash icon in the topbar restores

## Partial screenshots

<table>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/cover.png" alt="Main 3-pane view with sidebar, sessions, and chat" />
      <p align="center"><em>Main view — sidebar, sessions, chat with one-click export</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/chat.png" alt="Faithful chat replay with thinking, tools, and structured diffs" />
      <p align="center"><em>Faithful chat replay — thinking, tool calls, structured diffs, live tail</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/search.png" alt="Global search overlay" />
      <p align="center"><em>Global search (⌘⇧F) jumps straight to the message</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/stats.png" alt="Token & cost analytics dashboard" />
      <p align="center"><em>Token &amp; cost analytics by project, model, tool</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/export.png" alt="Exported HTML preview opened in browser" />
      <p align="center"><em>Exported HTML — fully offline, opens in any browser</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/trash.png" alt="Shared trash with restore" />
      <p align="center"><em>Shared trash — soft-delete with one-click restore</em></p>
    </td>
  </tr>
</table>

## Tech stack

- **Frontend** — Vue 3 + Vite + Tailwind CSS v4 (CSS-variable design tokens)
- **Backend** — Rust + Tauri 2; each agent's JSONL parsing is isolated behind a `SessionSource` trait under `src-tauri/src/agents/<agent>.rs`
- **JSONL parsing** — all on the Rust side; the frontend never touches the disk
- **Icons** — [iconify](https://iconify.design) (`lucide`, `material-icon-theme`, `arcticons`) inlined at build time
- **No store** — state lives in `App.vue` refs; `localStorage` only for lang / theme / pin prefs

See [`CLAUDE.md`](CLAUDE.md) for architecture notes aimed at contributors and [`docs/release-ci.md`](docs/release-ci.md) for the release pipeline.

## Contributing

PRs welcome. Please use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, ...) — `release-please` consumes them to auto-bump versions and update [`CHANGELOG.md`](CHANGELOG.md).

## License

[MIT](LICENSE) © jerrywu001

> Friend link: [linux.do](https://linux.do/)
