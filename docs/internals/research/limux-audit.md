# limux audit — fork or build?

Audited 2026-09-30, read-only (no build). Repo `am-will/limux` @ `ae67aae` (2026-09-28), v0.1.30, MIT.
Paths are relative to the limux root unless noted. Spec: [PRD](../PRD.md), [API](../API.md), [CONTEXT](../../CONTEXT.md).

## 1. Size and structure

| Crate | Rust LOC (incl. tests) | `#[test]` | Role |
|---|---|---|---|
| `limux-host-linux` | 31,273 | 316 | GTK4 app: window, panes, terminal, IME, browser, session, socket bridge |
| `limux-core` | 7,113 (one file) | 19 | cmux-parity in-memory state + dispatcher (**not used by the GTK host**, see §4) |
| `limux-cli` | 5,589 | 52 | `limux` CLI, agent hooks, agent-team |
| `limux-control` | 1,319 | 28 | socket path, peer auth, framing, standalone server |
| `limux-ghostty-sys` | 712 | 1 | hand-written FFI to `ghostty.h` |
| `limux-protocol` | 206 | 5 | V1/V2 envelope types |
| **Total** | **46,212** | **421** | |

- Deps (`rust/limux-host-linux/Cargo.toml`): `gtk4 0.11` (feature `v4_10`), `libadwaita 0.9`, `webkit6 0.6` (optional feature `webkit`, default on). CLI declares `clap` but hand-parses args (`rust/limux-cli/src/main.rs:140-180`).
- Tests: almost all are pure unit tests. 13 `#[ignore]` GTK tests run only inside `scripts/xvfb-smoke-test.sh` (1,005-line headless Weston harness: multi-pane cycles, Ctrl+D, child reaping; optional real-GPU mode, `docs/maintainability.md:20-60`). Plus 4 shell tests (cwd inheritance, tab rename, window activate).
- CI (`.github/workflows/rust-quality.yml`): cargo-audit nightly; `scripts/check.sh` (fmt, clippy `-D warnings`, `cargo test`); GTK smoke under Weston on ubuntu-24.04; each job builds libghostty with Zig 0.16. Release workflows for .deb/AppImage/tarball/RPM/AUR.
- Activity: 342 commits since 2026-03; 113 in Sep 2026. 22 contributors, but September is ~88 commits by one person (Bruno Volpato/bvolpato), the owner (am-will) is now mostly merging. GitHub: 552 stars, 79 forks, 101 merged / 41 closed / 4 open PRs (44 merged in Sep), 9 open issues, 7 closed in Sep. No open IME issues.

## 2. Ghostty embedding

**Fork chain.** Submodule `ghostty` → `github.com/bvolpato/ghostty` @ `a687fd69e` (2026-09-28). Lineage: upstream (newest reachable Mitchell commit `ab0b9da9e`, 2026-07-22) → manaflow-ai/cmux fork (~200 non-merge commits since then: Kitty-graphics replay, Metal external surfaces, process-group reaping, atomic bracketed paste, font query API) → bvolpato's Linux patches:

| Commit | Patch |
|---|---|
| `ec9b2a0af` | feat(embedded): support GTK OpenGL hosts — `apprt/embedded.zig`, `renderer/OpenGL.zig`, `ghostty.h` (+120/−29) |
| `a5bae4307` | per-surface app-thread drawing for GtkGLArea (`renderer/Thread.zig`) |
| `1a89efa4d` | FreeType source path preservation |
| `b47aa453e` `ddabe542c` `ffc01873f` | host-resolved key (GTK keyval) in `ghostty_input_key_s` |
| `a687fd69e` | cancel mailbox waits during surface teardown |

`git diff ab0b9da9e HEAD`: 254 files, +70.7k/−2.0k (includes vendored libxev and Apple/Windows code). The Linux-specific delta is small (~8 commits); the manaflow delta is large and ~2 months behind upstream. Rebasing onto upstream means carrying the manaflow stack or re-extracting only what Linux needs.

**Binding.** `rust/limux-ghostty-sys/src/lib.rs` is a hand-written subset (not bindgen, `:1-4`). Action IDs are hard-coded ints (`:80-99`), so a header change silently shifts them; the only guard is one struct-layout test (`:658-683`). `build.rs` links `ghostty/zig-out/lib/libghostty-internal.so` + `epoxy` by relative path (`build.rs:6-22`).

**Rendering.** One `gtk::GLArea` per terminal (`rust/limux-host-linux/src/terminal.rs:1509-1515`), `platform_tag = GHOSTTY_PLATFORM_LINUX` (`:1728`), `ghostty_surface_draw` inside `connect_render` on the GTK main thread (`:1901-1905`). Ghostty's `ACTION_RENDER` → `queue_render()` (`:961-970`), so drawing is on demand. `auto_render` is kept true for FBO sizing (`:1512-1515`).

**Hidden surfaces.** On `unmap`: `set_auto_render(false)` + `ghostty_surface_set_occlusion(false)`; on `map`: occlusion true + refresh (`terminal.rs:1614-1635`, commit `679cd0b` 2026-09-19). Matches R4.2. No benchmark for R4.3 numbers.

**IME (R9).** Good design, matches what we need:
- Per pane `gtk::IMMulticontext` (IBus/Fcitx5/text-input-v3) + `IMContextSimple` fallback, `set_client_widget(gl_area)`, `set_use_preedit(true)` (`ime/contexts.rs:45-55`).
- `preedit-changed` → `ghostty_surface_preedit(text, cursor)` (`ime/contexts.rs:174-190`); commit → `send_committed_text` (`:153`, `:228`); cursor area from `ghostty_surface_ime_point` (`:137-143`).
- Routing and key/commit latching are pure, GTK-free, unit-tested logic (`ime/routing.rs:1-19`, `ime/state.rs:111-190`), written for the Plasma dead-key bug.
- Gaps: no `retrieve-surrounding`/`delete-surrounding` handlers (IBus-Bamboo in surrounding-text mode needs them; its preedit mode does not). No Vietnamese test anywhere. **Status: likely works, unverified. Test IBus-Bamboo/Unikey on day 1.**

**Clipboard.** Text only: read/confirm/write callbacks with OSC 52 consent dialog (`terminal/clipboard.rs:92,171,211`). No image handling (R5.1 missing). File drag-and-drop pastes paths (`terminal.rs:2208`).

**Actions handled** (`terminal.rs:936-1160`):

| Action | Handled | Notes |
|---|---|---|
| `DESKTOP_NOTIFICATION` (OSC 9/777) | yes `:992` | → callback with focus flag |
| `RING_BELL` | yes `:1129` | → unread + notification |
| `PWD` (OSC 7) | yes `:1023` | tracks workspace cwd |
| `OPEN_URL` | yes `:1107` | → in-app browser or `gio::AppInfo::launch_default_for_uri` (`pane.rs:1449`) |
| `MOUSE_OVER_LINK` | yes `:1062` | shows target URL popover |
| `SHOW_CHILD_EXITED`, `SET_TITLE`, `SCROLLBAR`, `RELOAD_CONFIG` | yes | |
| `COMMAND_FINISHED` (OSC 133) | **no** | not even bound in sys crate |
| `PROGRESS_REPORT`, OSC 99 | **no** | |

**read_text.** Used only for viewport text (`terminal.rs:444-470`, `GHOSTTY_POINT_VIEWPORT`). The CLI advertises `read-screen --scrollback --lines` but the host bridge ignores both. R2 `read --scrollback` needs a new call with `GHOSTTY_POINT_SCREEN`/`HISTORY`.

## 3. Domain model

- limux: **Window → Workspace (one folder) → split tree of Panes → Tabs inside each Pane** (cmux/Zed style). Persisted shape: `layout_state.rs:54-110` (`WorkspaceState{layout}`, `LayoutNodeState::{Pane,Split}`, `PaneState{tabs}`, `TabContentState::{Terminal{cwd,agent},Browser{uri}}` `:191`).
- RADE: **Project → Worktree → Tab → Layout of Panes**, plus General and Agent Grid. The nesting is inverted (our Tab holds Panes; theirs a Pane holds Tabs), and there is no Project/Worktree/General level, no git branch, no Agent Grid, no Title Bars. Zoom exists (`Ctrl+Shift+Z`).
- Separation: weak. The live model is GTK widgets: `struct Workspace` holds `gtk::Widget`, `ListBoxRow`, labels (`window.rs:37-70`); `AppState` holds 40+ widget fields (`window.rs:72-110`); `split_tree.rs` imports gtk (`:4-6`). Only the serde snapshot (`layout_state.rs`) and IME routing are testable without a display. The GTK-free `limux-core::ControlState` (`limux-core/src/lib.rs:599`) is a separate cmux-parity model that the app does not use.

## 4. Socket and CLI

- Transport: Unix socket, newline-delimited JSON with V1/V2 envelopes (`limux-protocol`), 1 MiB request cap, 64 connections (`limux-control/src/request_io.rs:7-8`). Owner-only socket file. Error codes are JSON-RPC-like (`control_bridge.rs:45-50`).
- Auth: `SO_PEERCRED` with modes `localUser` (default, same UID), `limuxOnly` (descendants only), `allowAll` (`limux-control/src/auth.rs:14-31`). Meets R2.5/R2-c.
- **Two dispatchers.** The GTK host only serves 24 methods (`control_bridge.rs:18-43`: workspace.*, pane.list/create/focus, surface.read_text/send_text/send_key/create/close/focus, notification.create, tab.action). `limux-core` defines ~100 cmux methods (`limux-core/src/lib.rs:8-60`) for a standalone mock server (`limux-control/src/lib.rs:7`, `server.rs:13`). The CLI (`main.rs:202-247`) exposes tmux/cmux verbs (`browser *`, `pipe-pane`, `swap-pane`, …) many of which the live app rejects. About 12k LOC of the control stack is aspirational or duplicated.
- vs API.md: `spawn`≈`new-pane`/`pane.create` (no placement rules, depth/child limits, presets); `send`≈`send`/`send-key`; `read`≈`read-screen` (viewport only); `wait` **missing** (`wait-for` is a tmux-style file signal with a 5 s poll, `main.rs:3311-3333`); `list`≈`list-panes`; `close`/`focus` exist; `notify` exists; `status` missing; `project`/`worktree`/`fanout` missing (`agent-team` is a single-workspace split, no worktrees); `setup`≈`hooks setup`.
- Env injected per terminal via `ghostty_surface_config.env_vars` (`terminal.rs:1745-1765`): `LIMUX_SOCKET`, `LIMUX_WORKSPACE_ID`, `LIMUX_PANE_ID`, `LIMUX_TAB_ID`, `LIMUX_SURFACE_ID`; AppImage loader env restored for children.

## 5. Notifications, session, agents, browser

- Notifications: OSC/bell/`notify` → sidebar dot + message label, tab/workspace unread, libadwaita toast, desktop notification via raw D-Bus `org.freedesktop.Notifications` with a click-to-focus route (`window.rs:853-885`, `:3408`); sound configurable. No Notification panel/list (R3.2), no newest-first history.
- Session restore: `$XDG_DATA_HOME/limux/session.json` (`layout_state.rs:282-288`), directory lock + atomic rename + merge across instances (`session_store.rs:105-131`, 621 lines of tests). Restores layout, cwd, browser URI, and **resumes Claude/Codex/OpenCode/Gemini by session id** (`layout_state.rs:115-190`), ids captured by hooks. Closest to R11 of anything we have seen.
- Agent hooks: `limux hooks setup` installs Claude/Codex/Gemini hooks (`hooks/*.json`, `limux-cli/src/agent_hooks.rs`); they record session ids for restore only. No Agent Status (running/waiting/done) and no status dot.
- Browser: WebKitGTK tab type with a full browser CLI; out of RADE v1 scope (PRD non-goals). webkit6 is already wired, which helps R7.3 Markdown.

## 6. Code quality

- God files (>1,500 lines): `window.rs` 9,082; `limux-core/lib.rs` 7,113; `pane.rs` 5,098; `limux-cli/main.rs` 4,495; `terminal.rs` 3,813; `shortcut_config.rs` 2,553; `control_bridge.rs` 1,819; `layout_state.rs` 1,780. This goes against their own rule "prefer small domain modules over monolithic files" (`docs/maintainability.md`).
- `unsafe`: 171 sites, 102 in `terminal.rs` (FFI calls, C strings); one `static mut` (`terminal.rs:3025`, epoxy symbol). Surfaces are keyed by raw pointer in a thread-local `SURFACE_MAP` with identity checks against reuse.
- Errors: `anyhow`/`io::Result`; ~560 non-test `unwrap/expect`; diagnostics via `eprintln!`, no logging crate.
- Docs: AGENTS.md/CLAUDE.md, maintainability rules, careful comments on GTK/GL lifetime bugs. Strict gate: clippy `-D warnings` and a live Weston smoke test in CI.

## 7. Gap table (PRD → limux)

Effort in person-weeks for one dev with AI help. "Fork" = reach our spec inside limux. "Build" = from scratch, reusing `limux-ghostty-sys`, IME and clipboard modules, session/hook ideas.

| Req | limux | Fork | Build |
|---|---|---|---|
| R1 Project/Worktree/Tab/Pane, General, Zoom | partial (folder workspaces, inverted tabs-in-pane, zoom) | 3–4 (rewire 14k lines of window/pane) | 3 |
| R2 CLI + socket + limits + skill | partial (socket, peer creds, env, send/read/list/focus) | 2 | 2–3 |
| R3 Notifications + panel + smart desktop | partial (sources, unread, D-Bus desktop, click-to-focus) | 1 | 1.5 |
| R4 GPU render, hidden no-render, bench | has (GLArea, on-demand, occlusion); no bench | 0.5 | 1.5 |
| R5 Image paste | partial (DnD paths only) | 0.5 | 0.5 |
| R6 Shift+Enter | depends on Ghostty kitty protocol; unverified | 0.2 | 0.2 |
| R7 path:line links, Viewer, Markdown | partial (URL open, webkit wired) | 2 | 2 |
| R8 File tree | missing | 1.5 | 1.5 |
| R9 IME (Vietnamese) | has (multicontext + preedit), unverified | 0.2 verify | 0.5 port |
| R10 Agent Status + wait | missing (hooks only for restore) | 1.5 | 1.5 |
| R11 Session Restore + agent resume | has (4 agents, atomic, locked) | 0.5 remodel | 1.5 |
| R12 Worktrees | missing | 1 | 1 |
| R13 Diff review | missing | 2 | 2 |
| R14 Command palette | missing in host | 1 | 1 |
| R15 Fan-out/Judge/Winner | partial (`agent-team` split, no worktrees) | 1.5 | 1.5 |
| R16 Presets + setup | partial (hard-coded enum of 4 agents, hooks setup) | 1 | 1 |
| R17 Config, Ghostty config, keybinds | partial (Ctrl+Alt defaults, ghostty config read, editors) | 0.5 | 1 |
| **Total** | | **~20** | **~23** |

Also needed on the fork path: deleting or quarantining `limux-core`, the mock server, and the browser CLI (≈ −15k LOC, 1 week). On the build path: GL/surface lifetime hardening that limux paid for in bugs (`0e89d54`, `679cd0b`, `a687fd69e`, shutdown context selection); budget 1–2 weeks.

## 8. Recommendation: BUILD, reusing limux pieces

Totals are close. The deciding factor is the domain model, which touches every feature.

**Why build**
1. **The model is inverted and tied to widgets.** Tabs-in-pane vs our Tab→Layout→Pane, no Project/Worktree level, and state held in GTK widgets (`window.rs:37-110`). R1, R2.3–2.4 (Agent Grid), R3.3, R11, R15 all rest on it; refactoring 14k lines of `window.rs`+`pane.rs` in place costs as much as writing a clean, testable model, and ends up worse.
2. **The reusable value is small, clean, and MIT.** `limux-ghostty-sys` (712 LOC), the IME module (`ime/*`, ~660 LOC), clipboard callbacks, the terminal widget lifecycle patterns, peer-cred auth (`auth.rs`), session merge/atomic save, and agent resume-command logic can be lifted with attribution (ADR-0002). This is the hard, bug-paid part.
3. **The control plane doesn't match ours and carries dead weight.** Two dispatchers, ~100 cmux/tmux/browser verbs, V1/V2 envelopes; ADR-0006 wants one JSON-RPC contract that e2e tests exercise. Deleting and reshaping costs about as much as writing API.md fresh, and upstream merges would conflict constantly anyway.

**Risks of building**
1. **GL/surface lifetime bugs.** limux's history shows many (realize/unrealize pairing, context-current on free, mailbox teardown). Mitigation: copy `terminal.rs` lifecycle code nearly verbatim for M0 and port their Weston smoke harness early.
2. **We depend on bvolpato's Ghostty fork anyway.** One maintainer, 2 months behind upstream, on top of a large manaflow delta. Mitigation: pin `a687fd69e`; keep a list of the 8 Linux patches (§2) so we can re-apply them onto upstream or manaflow ourselves.
3. **Vietnamese IME is unproven.** No surrounding-text support; nobody has tested IBus-Bamboo. Mitigation: M0 gate. Build a spike with limux's IME module plus `retrieve-surrounding`; running a limux release binary with IBus-Bamboo before writing code costs 10 minutes.

If the team would rather ship a demo within 2 weeks and accept long-term cost, forking is viable: its rendering, IME, restore, and packaging work today. But we would spend M1–M3 fighting its model.
