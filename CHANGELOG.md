# Changelog

All notable changes to this project are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); semver via [release-please](https://github.com/googleapis/release-please) from [Conventional Commits](https://www.conventionalcommits.org/).

> ⚙️ This file is currently maintained by hand. From the first `release-please`-driven release onward, it will be regenerated automatically on every merge to `main` — see [`docs/release-ci.md`](docs/release-ci.md#changelog-自动维护).

---

## [v0.1.3] — 2026-06-04

### Changed

- Renamed the app/package identity to `ai-sessions-viewer`, updated release asset names and repository links to `fengchenzxc/ai-sessions-viewer`, and bumped app metadata to `0.1.3`.

## [v0.1.2] — 2026-05-25

### Added

- **Linux build target** — release pipeline now also runs on `ubuntu-22.04` and uploads `*.deb` (Debian/Ubuntu) and `*.AppImage` (portable) alongside the existing macOS `.dmg` / `.app.tar.gz` and Windows `.msi` / `*-setup.exe`. The runner installs `libwebkit2gtk-4.1-dev` + the standard Tauri 2 toolchain. Release notes body and asset-glob updated accordingly. Pinned to `ubuntu-22.04` (not `ubuntu-latest`) so binaries link against an older glibc and run on a wider range of distros. `.rpm` skipped on purpose — `rpmbuild` isn't preinstalled on the runner and AppImage covers RPM-based distros.
- **Animated "scanning" placeholder on the Stats page** — replaced the static bar-chart icon with a four-bar SVG that pulses on staggered delays, plus a trailing dots animation (`.` → `..` → `...`) on the "Discovering sessions" label. Honors `prefers-reduced-motion`.
- **Single-day fallback for the Daily activity chart** — sessions that only span one day used to render as a lonely dot in a vast empty plot. Now they fall through to a centered summary card (date · cost · calls) inside the same block; multi-day data still renders the dual-axis line+bar chart.
- **"Clear" button for the Welcome screen's Recent projects** — small muted action in the section header, removes the current agent's entire recents list (other agents untouched). i18n: English / 简体中文 / 繁體中文 / 日本語.
- **Stats overview dashboard** (`/stats`) — full-app Token usage & cost analytics page reachable from the sidebar topbar and per-session from the ChatTopbar's "Stats" button. Scope (All agents / Claude / Codex / Gemini) and Range (Today / 7d / 30d / All time) pill filters. Streaming partial snapshots: as the Rust worker chews through JSONLs it emits incremental aggregates so the UI fills in card-by-card instead of waiting for the whole scan.
- **Hero KPI cards** — Cost / Calls / Sessions / Cache hit rate as 4 standalone cards with icons (`Wallet` / `Activity` / `MessageCircle` / `Zap`), `font-variant-numeric: tabular-nums`, light-mode elevation + dark-mode borders, hover lift micro-interaction. Tokens-in / out / cached / written rendered below with hairline dividers.
- **Daily activity chart** — dual-axis: soft-grey columns for calls (right axis), brand smooth line + gradient area fill + emphasized points for cost (left axis). Renders via AntV G2 with theme-reactive colors.
- **By Model / By Activity** — horizontal bar charts with a curated 8-color categorical palette (`blue → violet → emerald → amber → pink → teal → indigo → orange`), light/dark variants. Tooltips show `$X.XX (Y.Y%)`.
- **By Project / Top Sessions / By Tool / By Shell / By MCP** — bar-list rows with rank, name, gradient progress bar, value, and meta count. Click a project or session row to jump straight into it.
- **Per-session stats** — entering Stats from the chat topbar locks scope to `session:<agent>:<path>`; daily, top-sessions, by-project panels are hidden in this mode (no meaning for a single file). "Back" button on the stats topbar returns to the original chat.
- **Codex cost & model breakdown** — recognizes the model from `turn_context.payload.model` (the JSONL location updated by recent Codex versions); pricing table covers `gpt-5` / `gpt-5.1` / `gpt-5.3-codex` / `gpt-5.5` / `o3` / `o4-mini` / `gpt-4o` / `gpt-4.1` families.
- **AntV G2 v5** replaces `chart.js` + `vue-chartjs` for all charts; smaller surface, theme-reactive, no canvas re-bind on data changes.
- **Shared `chartPalette.ts`** — single source of truth for chart brand / text-mute / grid / soft-bar / stroke colors and the categorical palette; used by every G2 chart so theme switches re-render all charts consistently.
- **Dashboard-style section cards** — white-on-tray layout (`stats-body` uses `--surface-2`, `stats-block` uses `--surface` with soft shadow in light mode, border-only in dark), card titles get a 3×14 blue→indigo accent stripe and a hairline divider, padding bumped 14→18/20 px for breathing room.
- **Live tail for in-progress sessions** — opening a session now starts a backend `notify` watcher (`watch_session` / `unwatch_session`) on its JSONL. New lines written by the CLI emit `session:append` events; the frontend appends them to the open chat and either auto-scrolls (if you're within 100 px of the bottom) or surfaces a `N new ↓` pill so you can jump down on demand. File truncation / replacement emits `session:reset` (full re-read) and deletion emits `session:gone` (closes the view). Single-subscription model + 200 ms debounce keeps overhead trivial. Read-only sessions in the Trash do not start a watcher. A pulsing `● Live` indicator next to the session ID confirms the watcher is active.

### Changed

- **"Check for updates" wired up to GitHub Releases** — previously a stub that always said "up to date". `api.checkUpdate()` now `fetch`es `/repos/fengchenzxc/ai-sessions-viewer/releases/latest`, strips the leading `v` from `tag_name`, and compares against `app_version` with a small `compareVer` helper. 404 (no releases yet) is treated as up-to-date silently; other HTTP errors throw so the Settings modal surfaces "Update check failed". `UpdateInfo` gains an optional `htmlUrl` for a future "View release" link. The Rust `check_update` stub and unused `UpdateInfo` struct were removed.
- **Sidebar project toggle is now context-aware** — re-clicking the active project while a chat is open closes the chat and returns to the session list (instead of collapsing the project to the welcome screen). A second click — now on the list view — collapses as before. Two-step toggle matches user mental model: "back, then close".
- **`lib::agents` / `lib::stats` are now `pub`** so the `examples/test_dedup.rs` verification binary (which links against the lib crate externally) can drive the dedup pipeline directly. CI's `clippy --all-targets -- -D warnings` exercises this on every PR.
- **Daily activity bucketing fixed** — was bucketing all of a session's cost / calls / tokens into the day of `last_modified` (file mtime), so a Mon→Fri session dumped 5 days of cost on Friday. Now bucketed per-turn by `turn.timestamp_ms`, matching codeburn exactly (verified within 1% on real data).
- **Claude message-id dedup across files** — Claude JSONL records every assistant message across multiple lines (one per content block: thinking / text / tool_use), and resumed / forked / sub-agent sessions re-copy the same `message.id`. Aggregator now keeps a `seen_message_ids: HashSet<String>` and skips repeats; a session whose every call is a duplicate is dropped entirely (mirrors codeburn's `if (session.apiCalls > 0)`). Result: input tokens / cost roughly halved for users with heavy fork / sub-agent usage.
- **Claude sub-agent JSONLs counted in stats** — new `SessionSource::discover_stats_sessions` trait method enumerates `<projects>/<dir>/<sessionId>/subagents/*.jsonl` for Claude (Codex / Gemini keep the default impl). Chat session list is unchanged so sub-agents don't clutter the UI.
- **Codex `cached_input_tokens` semantics** — Codex's `total_token_usage.input_tokens` already includes cached tokens (unlike Claude where `input_tokens` is the new portion only). Aggregating naively double-counted cache reads, inflating `input` by ~8× for cache-heavy usage. Reader now subtracts `cached_input_tokens` so `in` / `cached` columns are disjoint and totals match codeburn.
- **`bar-fill` color** — switched from solid brand (orange-red) to a `blue → indigo` linear gradient (matching the chart palette's primary colors) so the activity / project / top-session / tool / shell / MCP bars stop looking like one giant red wall.

### Fixed

- **Single-session stats stuck on return** — `watch(props.session?.path)` was gated on `if (isSession.value)`, so when leaving session mode the gate flipped to `false` before the callback ran and the backend stream stayed on `session:<…>` scope, leaving the Stats page showing a single session's data even after "Back". Watcher now always calls `refresh()` and picks the global scope when `session` clears.
- **"By model" donut invisible** — legend at `position: 'right'` inside a narrow column starved the donut of width and truncated labels to `GP…`. Replaced with the categorical horizontal-bar chart.

## [v0.1.1] — 2026-05-23

### Changed

- **Release pipeline split into `build` + `publish`** — `tauri-action` no longer creates GitHub releases; a separate `softprops/action-gh-release` job downloads artifacts from the build matrix and publishes one release with `generate_release_notes: true` (auto-fills "What's Changed" + "New Contributors" from PRs / commits since the previous tag). Bundles upload unconditionally with `if-no-files-found: error` so missing artifacts fail fast. Added a `concurrency` group keyed by ref to prevent double tag-push fights.

### Added

- **Three-agent session support** — browse **Claude Code** (`~/.claude/projects/`), **Codex** (`~/.codex/sessions/`), and **Gemini CLI** (`~/.gemini/tmp/`) sessions in one app, normalized into a shared project → sessions → chat view. Claude and Codex group by project directory; Gemini groups by the `slug` directory, with `cwd` read from each slug's sibling `.project_root` file. Agent switch in the sidebar / welcome screen surfaces all three; Trash mixes them with color-coded badges.
- **Empty-state welcome screen** — with no project selected, the main area lists recently opened projects (per agent) for one-click jump-back, an agent switch, and a link to the project repository. Each recent entry can be removed individually via a hover-revealed ×.
- **Project sidebar** with pin / sink / rename and an agent switch (Claude 🟠 / Codex 🟢 / Gemini 🔵) at the top.
- **Chat replay** — text, thinking blocks, tool calls, structured diffs (Claude `structuredPatch`), inline images, sidechain badge. Tool results of non-file-mutating tools (read / search / shell etc.) embed inside the parent tool call's collapsible body; only Write / Edit / MultiEdit / NotebookEdit / `apply_patch` results stay as standalone diff rows so file mutations remain visually distinct.
- **In-session search with scope filter** — search across the whole conversation or scope to user messages, agent replies (incl. file-mutating edits), or tool noise; previous / next jump with a live match counter.
- **Collapse / expand all tool calls** in one click to hide tool clutter and focus on the conversation.
- **Image lightbox** for screenshots embedded in transcripts.
- **Session list keyword search** (Rust-side) — typing in the list toolbar hits a backend `search_sessions` over the current project, matching session titles **and your own message text** (the local array only carries metadata). Cancellable mid-typing in the React-Fiber style: every new keystroke aborts the in-flight scan and only fires a fresh one once input settles.
- **Session list toolbar** — sort by recency / size / message count, filter to sessions that have an ID, and multi-select for batch ops.
- **Global search** (⌘⇧F / Ctrl+Shift+F) — an Algolia-style overlay over the current agent, scoped to **session titles and your own messages** (assistant text, thinking blocks, and tool calls are intentionally excluded — that's where the noise lives). Click a hit to jump straight to the exact matching message with a flash animation. Keyboard-driven (↑↓ to navigate, ↵ to open, Esc to dismiss); recent queries are kept with per-entry removal.
  - **Performance** — rayon-parallel project scan + ASCII fast-path byte filter as a pre-screen + per-file `(path, mtime)` cache of extracted user-text; results capped at 200 server-side / 80 rendered with a "+N more" hint.
  - **Cancellability** — cooperative bail via an `AtomicU64` generation counter on the Rust side; any new request (or an explicit `cancel_search`) makes the running scan stop on the next loop check.
- **Resume or start fresh** — open Terminal in a project to resume an existing session (`claude --resume <id>` / `codex resume <id>` / `gemini …`) or start a brand-new one. Session-id is validated by a strict allowlist before shelling out.
- **New session in terminal** — start a fresh `claude` / `codex` / `gemini` session in a project's directory straight from the session-list header; the header also gains refresh and delete-project actions.
- **Export single session** to Markdown or HTML via native Save-As dialog; HTML inlines avatar SVGs and the full stylesheet so the file renders offline.
- **Batch export / delete** in the session list — toggle multi-select from the list toolbar to move many sessions to Trash in one go, or export them all into a chosen folder as Markdown / HTML (`export-YYYYMMDD-HHMMSS-{md,html}/`).
- **Soft-delete trash** shared across all three agents under `~/.claude/.session-viewer-trash/`; restore puts the JSONL back to its original parent dir; in-chat system-event row surfaces session renames.
- **Trash list improvements** — keyword-highlighted search, click a trashed entry to preview its full transcript, and a hover spotlight matching the session list.
- **Fly animations** — single-session restore arcs back to its project in the sidebar, and deleting a whole project arcs to Trash, mirroring the existing delete-to-trash animation.
- **Native application menu** — full **File / Edit / View / Find / Window / Help** menu on macOS with accelerators (⌘N new session, ⌘B toggle sidebar, ⌘E export, ⌘, settings, ⌘⌃T trash, ⌘F in-session search, ⌘G / ⌘⇧G prev/next match, ⌘⇧F global search). Theme and Language submenus use `CheckMenuItem` and stay in sync with the in-app prefs via a `menu:sync` event bridge.
- **macOS native chrome** — unified topbar (`NSToolbar` `unifiedCompact`), hidden title, drag region.
- **Light / dark / system theme**; reactive i18n in **English / 简体中文 / 繁體中文 / 日本語**, with first-launch auto-detection from the OS language (falls back to English when no locale matches).
- **Custom singleton `v-tooltip` directive** — replaces the native `title=` attribute everywhere; fades in / out with a 250 ms hover delay and flips above when there is no room below.
- **Agent brand icons** next to "Claude" / "Codex" / "Gemini" labels in the chat role tag, dispatched via a global `agentIcons` dictionary (`material-icon-theme:claude`, `arcticons:openai-chatgpt`, `material-icon-theme:gemini-ai`).
- **Vitest test suite** (309 unit tests across logic modules + leaf components, jsdom env) and a GitHub Actions CI workflow (typecheck, unit tests, `cargo clippy` / `cargo test`).

### Changed

- Toast notifications now appear top-center instead of bottom.
- Projects whose working directory no longer exists show a **"Directory missing"** tag; actions that depend on that directory (resume, new session, refresh) are hidden for them — in both the session list and the sidebar context menu.
- Clicking the already-selected project deselects it (toggle), returning to the welcome screen.
- The Trash toolbar hides its sort / multi-select controls when there is one item or none.
- Debounce intervals tuned per surface — 450 ms for the heavy global-search backend call, 280 ms for the session-list backend search and in-chat search, 220 ms for purely client-side filtering; all surfaces are IME-composition-safe.

### Fixed

- Queued user messages — text typed while the agent is still working — were dropped from the Claude transcript; they now render correctly, including messages that contain images.
- **Search-jump scroll** in long sessions — clicking a global-search hit could land at the wrong scroll position because images, code highlighting, and structured-diff blocks kept pushing the target row down after the initial scroll. `ChatView.flashMessage` now self-stabilizes via a rAF loop that re-reads `offsetTop` each frame for ~1.6 s and yields immediately on any user wheel / pointerdown / keydown.
