# Agent CLI integration research

Checked 2026-09-30. Sources: official docs (via ctx7 and WebFetch) plus source code at these shallow-clone
commits: `openai/codex@a5cce88` (2026-09-30), `google-gemini/gemini-cli@38700b4` (2026-09-29),
`anomalyco/opencode@2fa3363` (2026-09-29), and `anomalyco/opentui` HEAD. Anything I could not confirm
from a doc or from source is marked **UNVERIFIED**.

RADE needs four things from each agent: (a) its status, (b) session resume, (c) image input as a pasted file path, (d) Shift+Enter for a newline.

---

## 1. Claude Code

| Need | Mechanism |
|---|---|
| Needs input | `Notification` hook with matcher `permission_prompt`, `idle_prompt`, `elicitation_dialog` or `agent_needs_input` |
| Turn finished | `Stop` hook |
| Error | `StopFailure` hook (API errors; matcher on `error_type`: `rate_limit`, `overloaded`, `authentication_failed`, `billing_error`, `server_error`, `unknown`, ...) |
| Session start | `SessionStart` hook (`source`: `startup`/`resume`/`clear`/`compact`/`fork`) |
| Resume | `claude -c` (latest in cwd), `claude -r <id-or-name>`, `claude --session-id <uuid>` (sets the id for a **new** session) |
| Image path | Yes. Docs: "Provide an image path to Claude, e.g. 'Analyze this image: /path/to/image.png'". `Ctrl+V` pastes a clipboard image as an `[Image #N]` chip |
| Shift+Enter | Works in Ghostty with no setup (docs table). `Ctrl+J` and `\`+Enter always work |
| Own OSC | Desktop notification by default in Ghostty/Kitty/iTerm2. `preferredNotifChannel: "terminal_bell"` sends BEL instead |

**Config files:** `~/.claude/settings.json` (user), `.claude/settings.json`, `.claude/settings.local.json`.
The `--settings <file-or-json>` flag layers extra settings onto a single launch.

```json
{
  "hooks": {
    "Notification": [{ "matcher": "permission_prompt|idle_prompt|elicitation_dialog",
                       "hooks": [{ "type": "command", "command": "rade hook claude needs-input" }] }],
    "Stop":         [{ "hooks": [{ "type": "command", "command": "rade hook claude stop" }] }],
    "StopFailure":  [{ "hooks": [{ "type": "command", "command": "rade hook claude error" }] }],
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "rade hook claude session-start" }] }]
  }
}
```
(The `a|b` regex matcher form is **UNVERIFIED**. Separate matcher entries, as shown in the docs, definitely work.)

**Stdin JSON, common to all hooks:** `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `permission_mode`, `prompt_id`,
`agent_id`/`agent_type` (inside subagents). Event-specific fields:
- Notification: `message`, `title`, `notification_type`.
- Stop: `last_assistant_message`.
- SessionStart: `source`, `model`.

**Environment:** the hook inherits the parent env, plus `CLAUDE_PROJECT_DIR` and `CLAUDE_EFFORT`. An env var such as
`RADE_PANE_ID` that RADE sets on the pane therefore reaches the hook.

**Hook output field `terminalSequence`:** Claude Code emits OSC 0/1/2/9/99/777 or BEL on the hook's behalf.
Hooks cannot write to `/dev/tty`.

**Session id:** read `session_id` from the SessionStart payload, or have RADE pre-assign one with `--session-id <uuid>`. `--resume`
keeps the same id unless you add `--fork-session`. Transcripts live under `~/.claude/projects/...` as `<session_id>.jsonl`.
Linux Wayland clipboard backend for `Ctrl+V` (wl-paste or another tool): **UNVERIFIED**, not documented.

Sources: https://code.claude.com/docs/en/hooks · https://code.claude.com/docs/en/cli-reference ·
https://code.claude.com/docs/en/terminal-config · https://code.claude.com/docs/en/common-workflows#work-with-images ·
https://code.claude.com/docs/en/interactive-mode

---

## 2. OpenAI Codex CLI

| Need | Mechanism |
|---|---|
| Needs input | `PermissionRequest` hook. Legacy `notify` only fires `agent-turn-complete` |
| Turn finished | `Stop` hook, or legacy `notify` (`type: "agent-turn-complete"`) |
| Error | No dedicated error hook. `Interrupt` event exists; its semantics are **UNVERIFIED** |
| Session start | `SessionStart` hook (`source`: `startup`/`resume`/`clear`/`compact`) |
| Resume | `codex resume --last`, `codex resume <SESSION_ID>` (UUID or name); `--all` disables cwd filtering |
| Image path | Yes. A bracketed paste whose whole content is one image path (quoted, shell-escaped or `file://`) is attached as an image (`handle_paste_image_path`). CLI flag: `-i/--image FILE[,FILE]` |
| Ctrl+V | `fixed.paste_image` bound to `Ctrl+V` / `Ctrl+Alt+V`. Uses `arboard` with the `wayland-data-control` feature |
| Shift+Enter | Default composer `insert_newline` = `Ctrl+J`, `Ctrl+M`, `Enter`, `Shift+Enter`, `Alt+Enter`; `submit` = `Enter`. The TUI probes for and pushes kitty keyboard flags |
| Own OSC | `tui.notifications` (default `true`), `tui.notification_method = auto\|osc9\|bel`. `auto` picks OSC 9 when it detects Ghostty (`TERM_PROGRAM=ghostty`, `TERM=xterm-ghostty`, or `GHOSTTY_RESOURCES_DIR`), else BEL. `tui.notification_condition` defaults to `unfocused` |

**Config files:** `~/.codex/config.toml` (inline `[hooks]`) or `~/.codex/hooks.json`. Per-project: `<repo>/.codex/`.
Hooks are on by default; `[features] hooks = false` turns them off.

```toml
# ~/.codex/config.toml
notify = ["rade", "hook", "codex", "notify"]   # legacy: JSON appended as final argv arg, stdin=null

[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = "rade hook codex needs-input"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "rade hook codex stop"

[[hooks.SessionStart]]
[[hooks.SessionStart.hooks]]
type = "command"
command = "rade hook codex session-start"
```

**Stdin JSON (hooks):** `session_id`, `cwd`, `hook_event_name`, `transcript_path`, `model`, `permission_mode`, and `turn_id` on
turn-scoped events. Event-specific fields:
- Stop: `stop_hook_active`, `last_assistant_message`.
- PermissionRequest: `tool_name`, `tool_input` (with `.description`).

**Legacy `notify` payload (argv):** `{"type":"agent-turn-complete","thread-id","turn-id","cwd","client","input-messages","last-assistant-message"}`.

**Trust gotcha:** non-managed hooks, user-level ones included, are "marked for review and skipped until trusted". Trust is recorded
against the hook's hash, and the user grants it through `/hooks` in the TUI. `--dangerously-bypass-hook-trust` skips the check for one
invocation. Legacy `notify` has no trust gate (source: `legacy_notify.rs`, spawned directly).

**Environment:** hooks start from an empty env and then replay the "session snapshot" env, minus a small denylist
(`command_runner.rs`). Whether a pane-level `RADE_PANE_ID` survives is **UNVERIFIED**; it probably does.

**Session id:** the `session_id` in the SessionStart payload. Rollouts live at `~/.codex/sessions/.../rollout-<ts>-<uuid>.jsonl`.

Sources: https://learn.chatgpt.com/docs/hooks (formerly developers.openai.com/codex/hooks) ·
https://learn.chatgpt.com/docs/config-file/config-reference · codex-rs `cli/src/main.rs` (ResumeCommand),
`tui/src/notifications/mod.rs`, `tui/src/keymap.rs`, `tui/src/clipboard_paste.rs`, `hooks/src/legacy_notify.rs`

---

## 3. Gemini CLI — SUPERSEDED

> Superseded 2026-05-19 by the Google Antigravity CLI (`agy`, section 5); RADE ships no `gemini` Preset. Kept for reference.

| Need | Mechanism |
|---|---|
| Needs input | `Notification` hook (`notification_type: "ToolPermission"`, `message`, `details`) |
| Turn finished | `AfterAgent` hook (fires once per turn after the final response) |
| Error | No dedicated error event in the hooks reference (**UNVERIFIED** that none exists) |
| Session start | `SessionStart` (`source`: `startup`/`resume`/`clear`). `SessionEnd` (`reason`) |
| Resume | `gemini --resume` / `-r` (latest), `-r <index>`, `-r <uuid>`. `--session-id <id>` starts a **new** session with a given id. `--list-sessions` |
| Image path | Pasted text that is a valid file path is rewritten to `@<escaped path> ` (`parsePastedPaths`). Whether `@` attaches images as image parts: **UNVERIFIED** |
| Ctrl+V | `input.paste` = `Ctrl+V`/`Alt+V`. Linux image clipboard uses `wl-paste` on Wayland and `xclip` on X11, saving `clipboard-<ts>.png` to a per-project temp dir |
| Shift+Enter | `input.newline` = `Ctrl+Enter`, `Alt+Enter`, `Shift+Enter`, `Ctrl+J`. The CLI enables the kitty keyboard protocol (or modifyOtherKeys) when detected. No Ghostty caveat is listed |
| Own OSC | Only when `general.enableNotifications: true` (off by default, "experimental"). Emits OSC 9, falling back to BEL |

**Config files:** `~/.gemini/settings.json` (user), `.gemini/settings.json` (project), `/etc/gemini-cli/settings.json`.
`hooksConfig.enabled` defaults to `true`.

```json
{
  "hooks": {
    "Notification": [{ "matcher": "*", "hooks": [{ "name": "rade-input", "type": "command", "command": "rade hook gemini needs-input" }] }],
    "AfterAgent":   [{ "matcher": "*", "hooks": [{ "name": "rade-stop",  "type": "command", "command": "rade hook gemini stop" }] }],
    "SessionStart": [{ "matcher": "*", "hooks": [{ "name": "rade-start", "type": "command", "command": "rade hook gemini session-start" }] }]
  }
}
```

**Stdin JSON (all hooks):** `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`.

**Environment:** `GEMINI_PROJECT_DIR`, `GEMINI_SESSION_ID`, `GEMINI_CWD`, `GEMINI_PLANS_DIR`, `CLAUDE_PROJECT_DIR`. Beyond these, hooks get the full
parent env unless `enableEnvironmentVariableRedaction` is on or the process runs in GitHub CI (`environmentSanitization.ts`).

**Trust:** project hooks are fingerprinted, and a changed hook triggers a warning. The docs do not describe a trust gate for user-level hooks.

**Session storage:** `~/.gemini/tmp/<project_hash>/chats/`.

**`AfterAgent` return value:** the hook must return nothing or `{}`, because `decision:"deny"` forces a retry.

Sources: gemini-cli `docs/hooks/{index,reference}.md`, `docs/cli/{notifications,session-management,cli-reference}.md`,
`docs/reference/{keyboard-shortcuts,configuration}.md`, `packages/cli/src/ui/utils/clipboardUtils.ts`,
https://github.com/google-gemini/gemini-cli/tree/main/docs

---

## 4. OpenCode

| Need | Mechanism |
|---|---|
| Needs input | Plugin event `permission.asked` (also `question.asked`) |
| Turn finished | Plugin event `session.status` with `status.type === "idle"`. `session.idle` still exists but its schema marks it `// deprecated` |
| Error | `session.error` (`properties.sessionID?`, `properties.error`) |
| Session start | `session.created` (`properties.sessionID`, `info`) |
| Resume | `opencode -c` (last session), `opencode -s <sessionID>`, `--fork`. `opencode session list` |
| Image path | Yes. A pasted path that resolves to an `image/*` or PDF file is read and attached (`pasteInputText`, then `readLocalAttachment`) |
| Ctrl+V | `input_paste` = `ctrl+v`. Linux reads the clipboard with `wl-paste -t image/png`, then `xclip` |
| Shift+Enter | `input_newline` = `shift+return,ctrl+return,alt+return,ctrl+j`. Docs say some terminals need to send CSI u (`\e[13;2u`). Ghostty sends it natively (kitty protocol) |
| Own OSC | TUI built-in notifications ("Session done", "Permission needs input", errors) via opentui `triggerNotification`, **only when the terminal is blurred**. opentui picks **OSC 777** for Ghostty (heuristic on TERM_PROGRAM/XTVERSION) and also supports OSC 9 and OSC 99 |

**Config (a plugin, not JSON hooks):** a JS/TS file in `~/.config/opencode/plugins/` (global) or `.opencode/plugins/` (project) is auto-loaded
at startup.

```js
// ~/.config/opencode/plugins/rade.js
export const Rade = async ({ $, directory }) => {
  if (!process.env.RADE_PANE_ID) return {};
  const send = (ev, sid) => $`rade hook opencode ${ev} ${sid ?? ""}`.quiet().nothrow();
  return {
    event: async ({ event }) => {
      const p = event.properties ?? {};
      if (event.type === "permission.asked") await send("needs-input", p.sessionID);
      else if (event.type === "session.status" && p.status?.type === "idle") await send("stop", p.sessionID);
      else if (event.type === "session.status" && p.status?.type === "busy") await send("running", p.sessionID);
      else if (event.type === "session.error") await send("error", p.sessionID);
      else if (event.type === "session.created") await send("session-start", p.sessionID);
    },
  };
};
```

**Event shape:** the plugin `event` hook receives the v1 SDK `Event` type (`{type, properties}`), per `packages/plugin/src/index.ts`.
Whether `permission.asked` appears in the v1 union rather than only in v2 (`data`) is **UNVERIFIED**. The docs list it, and the built-in
TUI plugin uses `event.properties.sessionID` for it.

**Subagent filtering:** subagent sessions also emit `session.status`. Filter on the session's `parentID` (the built-in plugin does this).

**Storage:** `~/.local/share/opencode/`.

Sources: https://opencode.ai/docs/plugins · https://opencode.ai/docs/cli · https://opencode.ai/docs/keybinds ·
opencode `packages/tui/src/{clipboard.ts,component/prompt/index.tsx,feature-plugins/system/notifications.ts}`,
`packages/schema/src/session-status-event.ts` · opentui `packages/native/src/terminal.zig`

---

## Implications for RADE

- **Status in general:** every agent can drive status through a hook or plugin that calls `rade status <state> --session-id <id>` (API.md). RADE injects
  `RADE_PANE_ID` into the pane env, and the hook forwards it together with `session_id` from stdin. For Claude and Gemini the env var
  demonstrably passes through. For Codex it probably does (**UNVERIFIED**).
- **Claude:** `rade setup` merges the Notification, Stop, StopFailure and SessionStart hooks into `~/.claude/settings.json`. It must
  merge and never overwrite. Alternative that leaves user files alone: launch with `claude --settings <rade-hooks.json>`.
- **Claude resume:** RADE can pre-assign ids with `claude --session-id <uuid>` and restore with `claude -r <uuid>`, so no payload
  parsing is needed.
- **Codex:** `rade setup` writes `[[hooks.PermissionRequest]]`, `[[hooks.Stop]]` and `[[hooks.SessionStart]]` into `~/.codex/config.toml`.
  The user must then trust them once in `/hooks`, and RADE should detect and prompt for that. Fallback: `notify = [...]` for
  turn-complete only, which has no trust gate. Resume with `codex resume <session_id>`.
- **Gemini:** `rade setup` writes Notification, AfterAgent and SessionStart hooks into `~/.gemini/settings.json`, each with a `name`.
  There is no error signal; treat a process exit as the end state. RADE can pre-assign ids with `--session-id <id>` and resume with
  `-r <id>`.
- **OpenCode:** `rade setup` drops `~/.config/opencode/plugins/rade.js`. The plugin no-ops unless `RADE_PANE_ID` is set. Resume with
  `opencode -s <sessionID>`, taking the id from the `session.created` event.
- **Image paste:** RADE should save the clipboard image to a file and send its path as a **bracketed paste** (Ghostty's paste already
  does this). Codex and OpenCode attach an image only when the paste is a lone path. Gemini turns the path into `@path`. Claude
  accepts a path in the prompt text.
- **Shift+Enter and OSC:** Shift+Enter needs nothing from RADE beyond leaving Ghostty's kitty keyboard protocol intact; don't
  intercept Shift+Enter in GTK. RADE should also handle OSC 9, OSC 777 and BEL from the embedded Ghostty surface as a secondary
  "attention" signal, because Codex, OpenCode and Claude emit them natively.

---

## 5. Google Antigravity CLI (`agy`)

Checked 2026-09-30 against `agy` 1.2.14 (installed on the reference machine), `agy --help`, https://antigravity.google/docs/hooks/ and
https://github.com/google-gemini/gemini-cli/discussions/27274 (Gemini CLI → Antigravity CLI, 2026-05-19).

| Need | Mechanism |
|---|---|
| Needs input | **None.** No Notification/permission event; `PreToolUse` can *decide* permissions via stdout, so RADE must not hook it. `waiting` comes from R10.2 fallbacks (M6) |
| Turn finished | `Stop` hook (`terminationReason`, `error`, `fullyIdle`) |
| Running | `PreInvocation` (before each model call; several per turn) |
| Error | No dedicated event; `Stop` carries `error` (RADE maps Stop → `done` in M2) |
| Session start | No SessionStart. Every hook's stdin has `conversationId` → RADE records it from `PreInvocation` |
| Launch with prompt | `agy -i "<prompt>"` (`--prompt-interactive`); print mode `agy -p` |
| Resume | `agy --conversation <id>`, `agy -c` (most recent) |
| Image path | **UNVERIFIED** |
| Skills | `~/.gemini/antigravity-cli/skills/<name>/SKILL.md` (global), `.agents/skills/` (project); context file `AGENTS.md` |

**Hook config:** `~/.gemini/config/hooks.json` (global), `.agents/hooks.json` (workspace). Top-level keys are named groups, each
mapping events to matcher groups; `"enabled": false` disables a group. RADE owns the group `rade`:

```json
{
  "rade": {
    "PreInvocation": [{ "hooks": [{ "type": "command", "command": "[ -z \"$RADE_PANE_ID\" ] || rade status running --agent agy --hook; echo '{}'", "timeout": 10 }] }],
    "Stop":          [{ "hooks": [{ "type": "command", "command": "[ -z \"$RADE_PANE_ID\" ] || rade status done --hook; echo '{}'", "timeout": 10 }] }]
  }
}
```

Commands run in bash; `timeout` is in seconds (default 30). Stdin JSON (all events): `conversationId`, `workspacePaths`,
`transcriptPath`, `artifactDirectoryPath`, `modelName`. Stdout is JSON: `PreInvocation` may return `injectSteps`, `Stop` may return
`{"decision": "continue"}`. The behavior for empty stdout is not documented, so RADE's hooks print `{}` (no-op). Environment passed to
hooks: **UNVERIFIED** (not documented; RADE assumes the parent env incl. `RADE_PANE_ID`, like the other Agents). No trust gate is documented.
Settings (`~/.gemini/antigravity-cli/settings.json`): RADE does not touch it.

## Field findings (2026-09-30, live test in RADE)
- **Codex 0.159 runs hooks inside its shared app-server daemon** (`codex app-server --managed-daemon`, started once per login), not in the TUI process of the Pane. The hook therefore lacks `RADE_PANE_ID` and no-ops. Fix: the `codex` Preset launches `codex --no-daemon` (and resumes with `codex --no-daemon resume <id>`); verified: status reached `done` via the Stop hook.
- RADE started from inside a Claude Code shell inherits `CLAUDECODE` / `CLAUDE_CODE_*` session markers; Claude in a RADE Pane then reports "Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker". `rade-app` now removes `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_CODE_*` at startup (keeps `CLAUDE_CONFIG_DIR`).
- **Agents started through aliases lose their config on resume** (2026-09-30). The owner starts Claude as `clw` = `CLAUDE_CONFIG_DIR="$HOME/.claude-work" claude --dangerously-skip-permissions`; the session lives in `~/.claude-work`, so the plain Preset resume `claude --resume <id>` said "No conversation found" and dropped the flag. Verified on this machine: the Agent is the hook's grandparent (`rade` ← `/bin/sh -c` ← `claude --dangerously-skip-permissions`), `/proc/<pid>/cmdline` shows the alias-expanded argv and `/proc/<pid>/environ` holds `CLAUDE_CONFIG_DIR` (same UID, readable). Native binaries run as argv[0] `claude` / `codex` (`~/.local/bin`); npm-installed Codex/OpenCode run as `node …/bin/codex`, hence the "first non-flag argument" match. Config-selecting env vars: Claude `CLAUDE_CONFIG_DIR`; Codex `CODEX_HOME`; OpenCode `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`; agy 1.2.14 has none documented (`agy --help`, and none in the binary's strings besides auth/telemetry `AGY_*`/`ANTIGRAVITY_*`). Value-taking flags per `--help` (claude 2.1.285, codex 0.159.2, agy 1.2.14, opencode) are the Presets' `value_flags` (preset.rs). Codex (clap) rejects a repeated flag (`--no-daemon --no-daemon` → "cannot be used multiple times"), so a resume argument already kept from the recorded argv is not appended twice; Claude accepts repeats. Codex top-level options before `resume` apply to the resumed session.
- Typing `codex` by hand in a RADE Pane (instead of `rade spawn --agent codex`) again uses the shared daemon, so no status and no session id reach RADE and restore falls back to a plain shell. A PATH shim does not work (the user's `.zshrc` re-prepends `~/.local/bin`). `rade setup` therefore appends a marked block to existing `~/.zshrc` / `~/.bashrc` that, only when `RADE_PANE_ID` is set, defines `codex()` → `command codex --no-daemon "$@"` (skipped if `--no-daemon` is already given). Verified in zsh and bash; idempotent.
- **agy 1.2.14 hooks.json shape (from its bundled docs `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/hooks.md`):** `PreToolUse`/`PostToolUse` use grouped `{matcher, hooks: [...]}`, but `PreInvocation`, `PostInvocation`, `Stop` take a **flat** list of handlers `[{type, command, timeout}]`. The grouped form is rejected: `invalid hook "rade": command hook must specify 'command'` (agy log), so no hook ran. `Stop` output requires `decision` (anything but `"continue"` lets it stop). Fixed; verified live: agy Pane reached `done` with `conversationId` recorded.

## 6. Fallback signals measured (2026-10-01, M6)

What each Agent emits besides its hooks, for the R10.2 fallbacks. Measured by running each CLI interactively in a PTY that answers
terminal queries like Ghostty (`TERM=xterm-ghostty`, `TERM_PROGRAM=ghostty`, focus-out reported) and logging OSC / BEL / title
sequences with timestamps (claude 2.1.x with `~/.claude-work`, codex 0.159 `--no-daemon`, agy 1.2.14; prompts "reply with exactly: PONG"
and a permission prompt "Run this shell command: curl -s https://example.com" with `--permission-mode default`).

| Signal | Claude Code | Codex | agy |
|---|---|---|---|
| OSC 9 / 777 notification | OSC 777 `notify;Claude Code;Claude needs your permission` ~6 s after a permission prompt (same event as the Notification hook); none on a finished turn | OSC 9 with the reply text at turn end (`9;PONG`), plus the user's own configured notifications (a second OSC 9) | none |
| BEL | only with `preferredNotifChannel: terminal_bell` | only with `tui.notification_method = bel` | none |
| OSC 9;4 progress | `9;4;3` (busy) while working, `9;4;0` at turn end; stays busy while a permission prompt waits | none | none observed (strings in the binary, not emitted in this run) |
| Title (OSC 0) | `◐ <task>` spinner while running, `✳ <task>` idle | braille spinner `⠙ …` while running | none |
| Screen while running | animated spinner + timer | animated spinner | animated spinner ("Generating…") |
| OSC 133 command finished | from the user's shell when a hand-started Agent exits (Ghostty shell integration) | same | same |

Conclusions for RADE (PRD R10.2): the Agent-agnostic signals are OSC/bell ("wants attention"), output idle (every Agent animates while
it works, so a still screen means it stopped) and OSC 133 command-finished (a hand-started Agent exited). Titles and OSC 9;4 are
Agent-specific conventions, and only Claude Code (which has every hook) emits progress, so RADE does not use them. agy has no signal
at all for "needs input" except its spinner stopping. Codex's turn-end OSC 9 and Claude's permission OSC 777 duplicate their hooks,
hence the R3.1 coalescing rule.
