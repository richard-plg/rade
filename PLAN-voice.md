# M46 — Voice prompts (implementation plan)

Owner request of 2026-10-25: in each Agent Pane, speak instead of typing. RADE listens, a fast
read-only agent works out what was meant from the session and the checkout, and writes a short,
clear prompt; the owner revises it by speaking again, then sends it to the Agent.

Owner decisions 2026-10-25:
1. Hold a key to speak, let go to draft; a composer card on the Pane shows the words live and then
   the draft, streaming.
2. The voice agent gets **read-only tools** (it must understand a fresh session from the source,
   not only from a transcript). No write, no edit.
3. The voice agent's session is **not kept**: one process per draft, all turns (revisions) in it,
   nothing written to disk, killed on Send / Discard.
4. `max_turns` and `timeout` are **settings** (Settings page + `config.toml`).
5. The UI follows the 1.0.0 visual system (ADR-0036); mockup
   `docs/internals/design/voice/mockup.html` (`#a`…`#f`, `-light`, `#all`).

Every other choice is fixed below (§2). **Implementers make no product or design decision.** If
something is not covered here, or the code or a tool contradicts this plan, stop and report the
question instead of choosing.

Facts verified on 2026-10-25 (T0, owner's machine, Claude Code 2.1.310, codex-cli 0.164.0;
details in `docs/internals/research/voice.md`):
- `claude -p --input-format stream-json --output-format stream-json --verbose
  --no-session-persistence` keeps several user turns in **one process** and writes **no** `.jsonl`
  session file (only an empty `projects/<slug>/memory/` dir); `--setting-sources ""` stops the
  user's hooks; a trivial Haiku turn ≈ 1.3 s.
- `--max-turns` counts **per user message**: hitting it ends that message with `result` subtype
  `error_max_turns`, `is_error: true`, no `result` text; the next message in the same process works.
- With `--include-partial-messages`: `stream_event` lines carry `content_block_start`
  (`tool_use` + `name`, `text`, `thinking`), `content_block_delta` (`text_delta`, `input_json_delta`,
  `thinking_delta`, `signature_delta`); complete `assistant` messages carry each `tool_use` with
  its full `input` (`file_path` absolute); `user` lines are tool results; one `result` per message.
- Claude transcripts hold many line types (`queue-operation`, `attachment`, `api-request-blob` with
  `message.role` = `user`/`system`, …): only `type` = `user` / `assistant` lines are turns; a
  `user` line whose content is a list of `tool_result` is not a user turn.
- `codex exec --ephemeral --sandbox read-only --skip-git-repo-check --json -` writes **no**
  rollout; it reads files with shell commands (`item.started` / `item.completed` of type
  `command_execution` with `command`), answers with `agent_message` items, ends with
  `turn.completed`; it still loads the user's Codex hooks (harmless: no `RADE_*` env). It takes one
  prompt per process (no multi-turn stdin).
- Codex rollouts: the user's real words are `event_msg` `item_completed` items of type
  `UserMessage` / `AgentMessage`; `response_item` user messages also hold injected
  `# AGENTS.md instructions` and `<environment_context>` texts, which must be skipped.
- The 5 icon URLs of §5.4 answer 200.

---

## 0. Rules for whoever implements a task

- Read `CLAUDE.md` (project) first: Conventional Commits with R-IDs, tests first for core logic,
  `cargo clippy --workspace --all-targets` clean, `cargo test --workspace` green.
- Do only your task's files. Don't refactor, rename or "improve" anything else.
- Core logic (`rade-core`) gets unit tests named after the AC: `fn r124_a_…`. E2E checks are named
  `check "R124-k …"` (see `scripts/trace.sh`).
- UI strings, CSS classes, icon names, config keys, debug JSON keys: copy them **exactly** from this
  plan.
- After each task: `cargo build --release && cargo clippy --workspace --all-targets && cargo test
  --workspace`, then commit with the message given in the task.
- Never run `rade setup` for real, never push. Never run a real `claude` / `codex` in tests except
  T0's spike and the opt-in smoke of T9 (`REAL=1`), which uses the owner's login and only checks
  that no session file appears.

---

## 1. Scope and IDs

- Milestone **M46**, ADR **0046**, PRD **R124**, spec sheet **I** (`docs/internals/design/spec/i-voice.md`).
- Built on `main` (1.0.1), independent of M45 (branch `m45-changes`, not merged). IDs R124 / ADR-0046 / M46 are kept so they don't collide with M45's R120–R123 / ADR-0045 when both land. Whichever merges second rebases: PRD order, `MILESTONES.md`, the Settings category list and the CI shard list are the only expected conflicts. One milestone per session.
- Release impact: MINOR (new config section `[voice]`, new keybinding `voice`, new `debug.voice`;
  no public CLI / socket method changes).
- Out of scope (say so in the ADR): wake word / always-on listening, voice for Shell / Browser /
  Viewer Panes, text-to-speech replies, a bundled speech model or server, cloud STT presets in the
  UI, keeping voice sessions or logs, a public `rade voice` command.

## 2. Fixed decisions

| # | Decision |
|---|---|
| D1 | Terms: **Voice Composer** = the card RADE shows on one Agent Pane while you dictate; **Voice Draft** = the prompt text it writes. One Voice Composer at a time in the whole app. It belongs to the Pane it was opened on, even if focus moves. Only **Agent Panes** (a Pane with an Agent Preset or an agent mark) get it; F8 on any other Pane → Toast `Voice works on Agent Panes`. |
| D2 | Key: keybinding action `voice`, default `F8` (free in Ghostty, GNOME, IBus/fcitx Vietnamese input, and unused by Claude Code / Codex TUIs). **Hold ≥ 250 ms = push-to-talk** (release ends the take). **Tap < 250 ms = toggle** (the take runs until the next tap). Key auto-repeat presses are ignored. The window losing focus, the Pane closing or `max_recording_secs` ends a take like a release. The release handler matches the bound key's keyval whatever the modifiers are at release time. |
| D3 | Pane header (Agent Panes only): mic button before Zoom, `ph-microphone-symbolic`, `.pane-button .voice-button`, tooltip `Voice prompt (hold F8)`; click = tap (D2 toggle). While this Pane's take records: icon `ph-microphone-fill-symbolic`, class `recording`; while its composer is open and not recording: class `open`. Not focusable (like Zoom). |
| D4 | Audio: `[voice] record_command`, default `pw-record --rate 16000 --channels 1 --format s16 -` (raw s16le mono 16 kHz on stdout), run with `sh -c` in its own process group, killed (group) at the end of a take. RADE keeps the PCM **in memory only** (cap `max_recording_secs`), computes the level every 50 ms (RMS → 0..1) for the meter. No audio file is ever written. |
| D5 | Speech-to-text: an **OpenAI-compatible transcription endpoint** `POST [voice] stt_url` (multipart: `file` = WAV built in memory, `model` = `stt_model`, `language` = `stt_language` unless `auto`, `prompt` = vocabulary (D8), `response_format` = `json`), sent with `curl`: URL, form fields, headers and the key in a curl config written to a pipe on fd 3 (`-K /dev/fd/3`), the WAV on stdin (`-F file=@-;filename=take.wav;type=audio/wav`). The key comes from `stt_key_command` (stdout, trimmed), only when set; never in argv, env, logs or state. Default `stt_url = "http://127.0.0.1:8178/v1/audio/transcriptions"` (whisper.cpp `whisper-server --port 8178 --inference-path /v1/audio/transcriptions -l auto` — without `-l` the server assumes English for requests with no `language`; also fits speaches, OpenAI, Groq). Request budget: 30 s. |
| D6 | Live words: with `live_preview = true`, every 1200 ms while recording RADE posts the **whole take so far** (skipped while the previous request runs); the newest answer replaces the heard text; its last 3 words are *tentative* (`fg3`). After the take ends, one final request; its text is the take's words. Empty final text → state `Didn't catch that` (D14), nothing sent to the agent. |
| D7 | Voice agent (`[voice] agent = "claude"`, default): one process per composer, **started at the first press** (it warms up and receives the context while you speak), fed by stream-json on stdin, killed on Send, Discard, composer close, Pane close and app quit. Command (built in core, §T4): `claude -p --model <model or "haiku"> --tools <"Read,Grep,Glob" or ""> --max-turns <max_turns> --no-session-persistence --setting-sources "" --input-format stream-json --output-format stream-json --include-partial-messages --verbose --append-system-prompt <§3.5>`. cwd = the Pane's cwd. Env = the Workspace env + the Pane's Agent Account env (`account::apply_env`), **minus** `RADE_PANE_ID`, `RADE_SOCKET` and every other `RADE_*` the Pane gets (so `rade status --hook` is a no-op and no Pane status changes). `[voice] agent = "codex"`: one `codex exec --ephemeral --sandbox read-only --skip-git-repo-check --json [-m <model>] -` **per turn** (the turn message carries the previous draft; no warm start). `[voice] agent_command` (non-empty) replaces the built command entirely; its protocol is still `agent`'s (tests use it). |
| D8 | Context (built off the UI thread at the first press, sent as the first stream-json message **before** the words; Codex: prefixed to each turn): (1) the Pane: agent, model (`chrome` header logic), cwd, Worktree / link / base; (2) the session tail: Claude transcript `<claude config dir>/projects/<slug(cwd)>/<session_id>.jsonl` (slug = every char not `[A-Za-z0-9]` → `-`; fallback `projects/*/<session_id>.jsonl`), Codex `<codex home>/sessions/**/rollout-*<session_id>.jsonl` (newest); last 6 exchanges, user text + assistant text only (tool calls and results dropped), each message cut to 2,000 chars, total ≤ 16,000 chars keeping the newest; no session id or no file → the Pane's screen (last 120 lines, trailing blanks trimmed); (3) git: branch, `git status --short` (≤ 60 lines), `git diff --stat HEAD` (≤ 40 lines), `git log -5 --format=%s`; (4) rules: `scm::RULES_FILES` found at the checkout root, ≤ 6,000 chars total; (5) layout: `git ls-files` folded to directories of depth ≤ 2 with file counts, ≤ 3,000 chars; (6) the same Tab's Viewer: its path and, if any, selected line range. **Vocabulary** (also sent to STT as `prompt`): identifiers from (2) and the changed paths' file stems — tokens matching `[A-Za-z_][A-Za-z0-9_.]{2,}` that contain `_`, `.`, a digit or an inner capital — deduped, newest first, ≤ 800 chars, space-joined. |
| D9 | Agent output = **the Voice Draft only**. Text deltas stream into the draft; tool-use events become the activity line (Claude: `Read <relative path>`, `Searched “<pattern>”`, `Listed <glob>`, other tools `<Tool>`; Codex `command_execution`: `Ran <command without the shell wrapper>`) with `n / max_turns` (Claude: tool uses counted per message); the turn's final text replaces the streamed text (Claude `result`; Codex the last `agent_message`; a surrounding code fence is stripped). **Max steps reached** (`error_max_turns`) with no text: RADE sends one more turn in the same process, `Write the prompt now from what you have; use no more tools.`; if that also fails → D17 `Voice agent failed: used all <n> steps`. Codex has no turn limit flag: `max_turns` does not apply to it (Settings description says so). |
| D10 | Draft format (enforced by §3.5, not by code): line 1 = the goal in one sentence; then 0–5 `- ` bullets (where, constraints, done when); file references as `path:line` in backticks; the owner's language unless `draft_language = "en"`; no new requirements; ≤ 1.2 × the words of the take (fillers removed); a part it cannot resolve is kept and marked `[?] "<words>"`. |
| D11 | Revise: holding F8 again while a draft exists starts a new take; the draft dims (`stale`); on release a turn `REVISE` (§3.5) carries the draft **as it is in the text box** (manual edits included) and the new words. The answer replaces the draft; words that differ from the previous draft are marked `changed` (word-level LCS, core) until the next keystroke in the draft. `revision n` counts revisions. Esc during a revise take = keep the draft unchanged. |
| D12 | Send: **Enter** (focus anywhere in the composer, draft non-empty, state Ready) = `pane.send` with `paste: true, enter: true` (the existing path, so the Pane's status becomes `running`); **Alt+Enter** = paste only (no Enter); **Shift+Enter** = newline in the draft. Then the composer closes, the voice process is killed and focus returns to the Pane's Terminal. |
| D13 | Esc: Listening → cancel the take (no draft yet → close the composer). Drafting → stop the turn (process killed; streamed text kept as the draft; state Ready; the next take starts a new process with the context and this draft). Ready → close and discard; if the draft was edited by hand or revised, the first Esc changes the footer to `Esc again to discard` for 2 s. Close button (`ph-x`) = discard without asking. |
| D14 | States (header dot, label; colours are tokens): `listening` (dot `err` + 3 px `err` 22 % ring, `Listening`, or `Revising` when a draft exists), `transcribing` (dot `run`, `Transcribing`, ≤ the final STT request), `drafting` (dot `run`, `Drafting`), `ready` (dot `acc`, `Ready`), `error` (dot `err`, text per D17), `empty` (dot `fg3`, `Didn't catch that`). Header time: take length `m:ss` while listening; `m:ss · <s.s> s` (take · draft time) when ready; `revision n · <s.s> s` after a revise. |
| D15 | Placement: a `gtk::Overlay` child of the Pane's body, bottom-aligned, margins 10 px, width = Pane width − 20 px, max 760 px, centred. The Pane keeps its focus glow. If the Pane body is < 300 px high the composer fills the body and scrolls. The composer takes keyboard focus when it opens (keys never reach the Terminal while it is open); the draft `gtk::TextView` gets focus when Ready. |
| D16 | Primary button: the composer's **Send** is the window's one `.suggested-action` while the composer is open (Changes' Commit loses it, as with a modal). Send and Paste only are insensitive unless state is Ready and the draft is non-empty. |
| D17 | Errors (state `error`, activity line `.voice-activity.error`, icon `ph-warning-diamond-symbolic`): record command missing or exits → `Can't record: <first stderr line>`; STT connect error → `No speech-to-text server at <host:port> · Set up voice…` (`Set up voice…` = link → Settings page filtered `voice`); STT HTTP ≥ 400 → `Speech-to-text failed: <status> <first line of body>`; agent can't start → `Can't start <claude|codex>: <error>`; agent exits non-zero → `Voice agent failed: <first stderr line>`; turn longer than `timeout_secs` → `Took longer than <n> s` (process killed, streamed text kept). Header label: `Can't record`, `Can't transcribe`, `Voice agent failed`, `Timed out`. Hold F8 again retries; nothing typed is lost. |
| D18 | Statusline: while a take records, a cell `.status-voice` (`ph-microphone-fill-symbolic` 13 px `err`, text `REC m:ss` JetBrains Mono 11 px `err`) after the location cells. Gone when not recording. |
| D19 | Config: `[voice]` is **user-only** (not in `config::LAYERED`): it runs commands and sends audio and code context to endpoints; a Workspace / Project / `.rade.toml` `[voice]` is reported by `rade config explain` as `ignored (user config only)`. |
| D20 | Settings page: new category `"Voice"` after `"Agents"` (before `"Statusline"`) with the rows of §T2; text keys (`stt_url`, `stt_model`, `stt_key_command`, `record_command`, `agent_command`, `model`) are `config.toml`-only (the page has no text kind) and listed in `docs/features/voice.md`. |
| D21 | Reduce Animation: none needed — the only motion is the level meter (bars follow the level, no easing) and the caret. Under Reduce Animation the meter still updates (it is information, not decoration). |
| D22 | Debug: new hidden `debug.voice` (§6); `debug.ui key` gains `press:<key>` / `release:<key>` so tests hold F8. |

---

## 3. Text to add to the docs (Task T1 copies these)

### 3.1 PRD (`docs/internals/PRD.md`, after the last requirement — R119 on `main`)

```markdown
## R124 — Voice prompts (M46, ADR-0046)

Speak to an Agent Pane instead of typing: RADE transcribes, a read-only voice agent writes a short, clear prompt from what was said, the session and the checkout, the owner revises it by speaking again and sends it.

1. **Open and talk.** On an Agent Pane, hold `F8` (keybinding `voice`) to talk and let go to draft; a tap toggles talking on and off; the Pane header's mic button toggles too. A Voice Composer opens at the bottom of that Pane: what you say appears live, then the Voice Draft streams in. One Voice Composer at a time. Other Panes: a Toast says voice works on Agent Panes.
2. **Speech to text.** Audio is recorded by `[voice] record_command` into memory only and sent to an OpenAI-compatible transcription endpoint (`[voice] stt_url`, default a local whisper.cpp server), with names from the session and the checkout as the vocabulary hint. Live words while talking (`live_preview`), a final pass after.
3. **Understanding.** The voice agent (Claude Code headless by default, Codex optional) gets the Pane's session tail (transcript or rollout; the screen when there is none), git status, the rules files, the checkout's layout and the Viewer's file, and may read, search and list files of the checkout (no writing, no editing, no other tools), within `max_turns` steps and `timeout_secs` per turn.
4. **Draft.** One goal sentence, up to 5 bullets, `path:line` references, the owner's language (or English), fillers and repeats removed, nothing invented; what it could not resolve is marked `[?]`.
5. **Revise.** Holding `F8` again with a draft speaks a revision ("drop the second point", "say it in English"); the whole draft is rewritten in the same voice agent process and the changed words are marked. The draft can also be edited by hand.
6. **Send.** Enter sends the draft to the Agent (bracketed paste + Enter); Alt+Enter pastes it without Enter; Esc cancels a take, stops a draft or discards (asking once after edits).
7. **Nothing kept.** The voice agent runs without session persistence, without the user's hooks and without the Pane's `RADE_*` variables; it is killed when the composer closes. No audio file is written.
8. **Settings.** `[voice]` in the user's `config.toml` only; the Settings page's Voice category has agent, tools, max steps, timeout, max recording, live words, draft language, speech language.

AC:
- R124-a [M46] (unit): Given a Claude config dir with `projects/-home-u-ws-shop/<id>.jsonl` holding 9 exchanges with tool calls and tool results. When the context is built for cwd `/home/u/ws/shop` and session `<id>`. Then it holds the last 6 exchanges' user and assistant texts only, no tool input or result, each message ≤ 2,000 chars, the whole tail ≤ 16,000 chars with the newest kept; a session file elsewhere under `projects/*/` is found by the fallback.
- R124-b [M46] (unit): Given a Codex home with two rollouts, one named with the session id. When the context is built. Then the session tail comes from that rollout's user / assistant messages; with no session id the bundle holds the screen text's last 120 lines with trailing blank lines trimmed.
- R124-c [M46] (unit): Given a checkout with `CLAUDE.md`, 3 changed files and 500 tracked files. When the context is built. Then it has the branch, `git status --short`, `git diff --stat HEAD`, the last 5 subjects, `CLAUDE.md` (≤ 6,000 chars) and a layout of ≤ 3,000 chars of depth-2 directories with counts.
- R124-d [M46] (unit): Given a session tail naming `handleSubmit`, `use_totals`, `promo.ts`, `the`, `Fix` and changed path `src/checkout/applyPromo.ts`. When the vocabulary is built. Then it is `applyPromo promo.ts use_totals handleSubmit` order-by-newest, without plain words, ≤ 800 chars.
- R124-e [M46] (unit): Given the default `[voice]` and a Pane with account `acme-work`. When the Claude command is built. Then argv is §T4's exact list with `--max-turns 6` and `--tools Read,Grep,Glob`; `max_turns = 3` and `tools = "none"` give `--max-turns 3` and `--tools ""`; the env has the account's `CLAUDE_CONFIG_DIR` and no `RADE_PANE_ID`, `RADE_SOCKET` or other `RADE_*` key.
- R124-f [M46] (unit): Given the recorded stream fixtures `crates/rade-core/tests/fixtures/voice/claude-stream.jsonl` (message 1 ends `error_max_turns` after 3 Reads, message 2 answers `applyPromo`) and `codex-exec.jsonl`. When parsed. Then message 1 gives activities `Read src/checkout/useTotals.ts`, `Read src/checkout/promo.ts`, `Read src/checkout/CheckoutForm.tsx` and a max-turns end, message 2 gives deltas `appl`, `yPromo` and final text `applyPromo`; Codex gives `Ran cat src/checkout/useTotals.ts` and its last agent message as final text; a final text wrapped in a code fence is unwrapped.
- R124-g [M46] (unit): Given a Voice Composer model. When F8 is pressed and released after 400 ms, pressed and released after 100 ms twice, auto-repeat presses arrive, Esc is pressed in each state, and a turn passes `timeout_secs`. Then the states and actions follow D2, D11, D13 and D17 (table in `voice/composer_tests.rs`).
- R124-h [M46] (unit): Given draft `Fix A in x.ts.\n- add a test` and revision `Fix A in x.ts.\n- keep the API`. When changed spans are computed. Then exactly the words `keep the API` are marked.
- R124-i [M46] (unit): Given `stt_key_command = "echo sk-test"`. When the STT request is built. Then the key appears only in the curl config text (never in argv), the form has `model`, `prompt` and `response_format`, `language` only when not `auto`; `{"text":" ừ thì sửa cái này "}` parses to `ừ thì sửa cái này` and a partial's last 3 words are tentative.
- R124-j [M46] (unit): Given defaults. When `[voice]` is read and the Settings rows are listed. Then the defaults are D-table values, the Voice rows exist with §T2's bounds, and a `.rade.toml` with `[voice]` is explained as `ignored (user config only)`.
- R124-k [M46] (e2e): Given an Agent Pane whose fake Claude session file holds `handleSubmit`, a fake STT and a fake voice agent. When F8 is held for 3 s and released. Then the composer shows `Listening` and at least 2 live partials while held, the Statusline shows `REC`, the STT `prompt` contained `handleSubmit`, the voice agent's first message held the session text and `git status`, the draft streamed in with activity `Read …` and the state is `Ready`.
- R124-l [M46] (e2e): Given a Ready draft edited by hand. When F8 is held again and released. Then the draft was `stale` while held, the agent's `REVISE` message carried the edited draft, the new draft is shown with `changed` marks, the header says `revision 1` and the same voice agent process answered (one pid).
- R124-m [M46] (e2e): Given a Ready draft on a sink Agent Pane (`cat -v`). When Enter is pressed. Then the Pane received the draft in bracketed paste followed by Enter, the composer is closed and the voice agent process is gone; Alt+Enter on another draft pastes without Enter.
- R124-n [M46] (e2e): Given the fake voice agent logging its env and running `rade status --hook`. When a draft is made. Then its env has no `RADE_PANE_ID` / `RADE_SOCKET`, the Pane's status did not change, and no file was created under `$XDG_RUNTIME_DIR`, `$TMPDIR` or the Claude config dir. With `REAL=1` and `claude` installed: a real Haiku draft leaves no new `.jsonl` under the account's `projects/`.
- R124-o [M46] (e2e): Given `timeout_secs = 10` and a voice agent that hangs after one delta. When a take is drafted. Then after 10 s (±2 s) the state is `Timed out` with that delta kept as the draft; changing Voice › `Max steps` to 3 on the Settings page writes `max_turns = 3` under `[voice]` (other lines unchanged) and the next voice agent's argv has `--max-turns 3`.
- R124-p [M46] (e2e): Given no STT server. When a take ends. Then the state is `Can't transcribe` with `No speech-to-text server at 127.0.0.1:<port>`; a failing `record_command` gives `Can't record: …`; `Set up voice…` opens the Settings page filtered to `voice`.
- R124-q [M46] (e2e): Given a Shell Pane and an Agent Pane. When F8 is pressed on the Shell Pane, the Agent Pane's mic button is clicked twice, and F8 is pressed while another Pane's composer is open. Then a Toast `Voice works on Agent Panes`; the click toggles a take on then off; the open composer moves to the focused Agent Pane only after it is closed (the press is ignored while it is open on another Pane and a Toast `Voice is open on <pane>` shows).
- R124-r [M46] (e2e): Given scenes I-a, I-c and I-e in dark and light at 1440 × 900. When `debug.ui geometry` measures them. Then every row of `docs/internals/design/spec/i-voice.md` is within tolerance and the screenshots match the approved goldens `tests/e2e/golden/m46/`.
```

### 3.2 ADR `docs/internals/adr/0046-voice-prompts.md`

```markdown
# 0046 — Voice prompts through pluggable speech-to-text and a read-only voice agent

Status: accepted (owner, 2026-10-25).

## Context
The owner wants to dictate tasks to coding Agents in Vietnamese mixed with English identifiers, with fillers and repeats, and get a clean prompt they can revise by voice before it reaches the Agent. A plain transcription would carry the "ừ à" and the misheard names into the Agent's session.

## Decision
- Speech-to-text is an OpenAI-compatible transcription endpoint, not an engine inside RADE. Default: a local whisper.cpp server (large-v3-turbo on a GPU is well under a second per take); the same setting points at speaches, OpenAI or Groq. RADE records with `pw-record` into memory and posts through `curl`, like Detailed limits (ADR-0035).
- Names from the session and the checkout are sent as the vocabulary hint, and again to the voice agent, which fixes what the recogniser still misheard.
- The voice agent is Claude Code headless (Haiku) with Read / Grep / Glob only, so it understands a fresh session from the source. It starts at the first key press, keeps every revision of one draft in one process (stream-json input) and runs without session persistence, without the user's hooks and without the Pane's `RADE_*` variables. Codex is supported per turn with `--ephemeral --sandbox read-only`.
- The draft stays in RADE's Voice Composer until sent: rewriting text inside each Agent's TUI input is different per Agent and breaks on multi-line input.
- `[voice]` is user config only: it runs commands and sends code context out.

## Not done
Wake word or always-on listening, text-to-speech, bundling a speech model or server, voice on non-Agent Panes, keeping voice sessions or logs.
```

### 3.3 `docs/internals/MILESTONES.md` (append)

```markdown
## M46 — Voice prompts — R124
Owner request and decisions of 2026-10-25 (`PLAN-voice.md`), ADR-0046, mockup `docs/internals/design/voice/mockup.html`, spec I. Hold F8 (or the Pane header's mic) on an Agent Pane: live words from an OpenAI-compatible STT endpoint (local whisper.cpp by default), a read-only Claude Code (Haiku) voice agent with the session tail, git facts, rules and layout writes a short Voice Draft, revise by speaking again in the same process, Enter sends / Alt+Enter pastes; nothing kept (no session file, no hooks, no `RADE_*`, no audio file); `max_turns` / `timeout_secs` settings. Pure logic in `rade-core/src/voice/`, app `voice.rs` + `voice_view.rs`, E2E `tests/e2e/m46.sh` with `fake-stt.py`, `fake-mic.sh`, `fake-voice-agent.py`. Checks: docs/internals/checks/M46.md.
```

### 3.4 `CONTEXT.md` (two terms, alphabetical place and format of the file)

- **Voice Composer** — the card at the bottom of an Agent Pane where the owner dictates: live words, then the Voice Draft, revised by speaking again, sent with Enter.
- **Voice Draft** — the prompt RADE's voice agent writes from what the owner said, the Pane's session and the checkout; it reaches the Agent only when sent.

### 3.5 The voice agent's texts (core constants in `voice/agent.rs`, copied verbatim)

`SYSTEM` (passed with `--append-system-prompt`; Codex: first part of each turn):
```text
You turn a developer's spoken words into a prompt for a coding agent working in this repository.
The words come from speech recognition: expect fillers ("ừ", "à", "uh"), repeats, restarts and
misheard names. Use the CONTEXT and, only when needed, read or search files to find what they mean
(the real file, function or command). Do not change anything.

Write only the prompt, nothing before or after it:
- Line 1: the goal in one sentence. Nothing else on it: no cause, no explanation.
- Then 0 to 5 lines starting with "- ": where, constraints, how to know it is done. No blank lines.
- Refer to code as `path:line` or `name` in backticks, using real names from the repository.
- Language: {LANGUAGE}.
- Say what to do, not why: no cause analysis, no plan, no steps or checks they did not ask for.
- Keep the developer's intent and scope. Do not add requirements or opinions.
- Fillers, repeats and false starts go. Keep it about as long as what they meant, never longer.
- A misheard name you worked out is just written as the real name; never mention the mishearing.
- Only words you could not work out stay, marked in place as [?] "<their words>".

Example (the repository has `handleSubmit` in `src/checkout/CheckoutForm.tsx`):
SPEECH: ừ thì cái hàm hen đồ sấp mít á, nó bị gọi hai lần, à khi bấm enter á, sửa giùm, à mà đừng đụng cái validate
PROMPT:
Sửa `handleSubmit` bị gọi hai lần khi nhấn Enter.
- Ở `src/checkout/CheckoutForm.tsx`.
- Không sửa phần validate.
```
`{LANGUAGE}` = `the language of SPEECH, never another one (Vietnamese speech → a Vietnamese prompt); English technical terms stay as they are` for `same`, `English` for `en`.

Turns. The first press spawns the process (Claude) and builds the bundle while the owner speaks; no turn is sent before the words (a context-only turn would cost a whole model turn).
- First turn, after the first take: `CONTEXT\n<render(bundle)>\n\nSPEECH\n<words>`.
- Revision: `REVISE\nCURRENT DRAFT\n<draft as in the text box>\n\nSPEECH\n<words>\n\nRewrite the whole prompt with this change.`
- Codex (one process per turn): every turn starts with the `CONTEXT` part; a revision adds `CURRENT DRAFT`.
- After a stopped or timed-out turn the next take starts a new process; its first turn is the First form with `CURRENT DRAFT` added when a draft exists.

---

## 4. Architecture — files

### New
| File | What |
|---|---|
| `crates/rade-core/src/voice/mod.rs` | `VoiceConfig` re-export, `pub mod` lines, shared types (`Take`, `State`, `Event`). |
| `crates/rade-core/src/voice/context.rs` + `context_tests.rs` | D8: transcript / rollout location and tail, screen fallback, git facts, rules, layout, Viewer, `vocabulary`, `render` (the `CONTEXT` text). Pure functions over paths and command output strings; one `gather(dir, …)` that runs git. |
| `crates/rade-core/src/voice/agent.rs` + `agent_tests.rs` | D7: `claude_argv`, `codex_argv`, `env_for` (strip `RADE_*`), §3.5 texts, `turn_message`, stream parsing (`parse_claude_line`, `parse_codex_line` → `AgentEvent { Delta, Tool{label}, Final{text}, Error{text} }`), fence strip, activity labels. |
| `crates/rade-core/src/voice/stt.rs` + `stt_tests.rs` | D5–D6: WAV header for s16le 16 kHz mono, curl argv + config text, response parse, tentative split, level (RMS) of a PCM chunk, error texts. |
| `crates/rade-core/src/voice/words.rs` | D11: word tokenizer + LCS `changed_spans(old, new) -> Vec<Range<usize>>` (byte ranges in `new`). Tests inline (`#[cfg(test)]`). |
| `crates/rade-core/src/voice/composer.rs` + `composer_tests.rs` | D2, D11–D14, D17: the Voice Composer state machine (`Composer::on(Event) -> Vec<Action>`), and `view()` → `ComposerView` (header label, dot class, time text, footer hints, Send / Paste sensitivity, activity text) — the UI renders this only. |
| `crates/rade-app/src/voice.rs` | Controller: one `Session` (Pane id, record child, PCM buffer, STT timer, agent child + reader thread, turn timer), events → `Composer::on` → actions; `send`; `debug.voice`. |
| `crates/rade-app/src/voice_view.rs` | The Voice Composer widget (§5), overlay on the Pane, keys inside it, meter drawing, changed marks with `gtk::TextTag`s. |
| `tests/e2e/m46.sh` | E2E for R124-k…r. |
| `tests/e2e/fake-stt.py` | OpenAI-compatible fake: answers scripted texts in order (`FAKE_STT_SCRIPT` JSON), logs each request (form fields, WAV header facts, length) to `FAKE_STT_LOG`; `--down` not started = connection refused. |
| `tests/e2e/fake-mic.sh` | `record_command` stand-in: writes 16 kHz s16le (a 440 Hz tone at the level `FAKE_MIC_LEVEL`) at real-time pace until killed; `FAKE_MIC_FAIL=1` → `pw-record: no source` on stderr, exit 1. |
| `tests/e2e/fake-voice-agent.py` | Speaks Claude's stream-json (from T0's fixture shapes): logs argv, env, cwd and each stdin message to `FAKE_VOICE_LOG`, emits `Read` tool use, then deltas of a scripted draft, then `result`; runs `rade status --hook` once; `FAKE_VOICE_HANG=1` stops after one delta; one process answers several turns. |
| `tests/fixtures/voice/claude-stream.jsonl`, `codex-exec.jsonl`, `claude-transcript.jsonl`, `codex-rollout.jsonl` | Recorded by T0 (real CLI output, identifiers replaced with fixture names). |
| `docs/internals/research/voice.md` | T0's verified facts (commands run, versions, outputs). |
| `docs/internals/adr/0046-voice-prompts.md` | §3.2. |
| `docs/internals/design/spec/i-voice.md` | §5.3. |
| `docs/features/voice.md` | T10: setup (whisper.cpp server with CUDA + large-v3-turbo, systemd user unit example; cloud endpoint example), keys, settings, privacy. |
| `docs/internals/checks/M46.md` | Gate (T10). |
| 6 icons in `crates/rade-app/assets/icons/` | §5.4. |

### Changed
`rade-core`: `config.rs` (`VoiceConfig`, default keybinding `("voice", "F8")`), `settings.rs` (category + rows), `layers.rs` (`voice` → `ignored (user config only)` in `explain`), `lib.rs` (`pub mod voice`), `palette.rs` (action `("voice", "Voice Prompt")`).
`rade-app`: `keys.rs` (hold / tap / release for `voice`; `debug_key` `press:` / `release:`), `layout_view.rs` (mic button, overlay slot), `chrome.rs` (Statusline `REC` cell), `handlers.rs` (`debug.voice` route), `app.rs` (quit kills the session; Pane close ends it), `settings_view.rs` (`Set up voice…` = open filtered `voice`, if not already a public fn), `style.css` (+ `style-dark.css` only if a rule must differ), `main.rs` (mod lines).
Tests/docs: `.github/workflows/ci.yml` (append `m46` to the lightest shard), `docs/API.md` (`debug.voice`, `debug.ui key press:/release:`), `docs/keybindings.md`, `docs/features/settings-and-config.md` (Voice rows), `CLAUDE.md` (command line + code map), `docs/internals/PRD.md`, `MILESTONES.md`, `CONTEXT.md`.

---

## 5. UI specification

The mockup (`docs/internals/design/voice/mockup.html`) is the picture; §5.1–§5.3 are what tests check.

### 5.1 Voice Composer

```
┌ ● Listening 0:06 ▁▃▅▇▅▃▁…         → ✻ planner · checkout-v2  ✕ ┐  .voice-header (34 px)
│ ❝ ừ thì cái phần promo code á, nó đang bị trừ hai lần, à ở…  show │  .voice-heard
│ ┌──────────────────────────────────────────────────────────────┐ │
│ │ Fix the promo discount being applied twice: `useTotals()` …  │ │  .voice-draft (TextView)
│ │ - Apply it once, in `applyPromo()` (`src/checkout/promo.ts:31`)│ │
│ └──────────────────────────────────────────────────────────────┘ │
│ 🗎 Read src/checkout/useTotals.ts                          2 / 6 │  .voice-activity
├──────────────────────────────────────────────────────────────────┤
│ Hold F8 to revise  Esc discard        [Paste only Alt ↵] [↵ Send ↵]│ .voice-footer
└──────────────────────────────────────────────────────────────────┘
```

- **Card** `.voice-composer`: `bg2`, 1 px `line2`, radius 8 px, `shadow`; class `listening` → border `acc-line` + `glow` (the focus glow token).
- **Header** `.voice-header` (h 34 px, padding 0 6 0 12 px, gap 8 px, bottom border 1 px `line`):
  `.voice-dot` (8 × 8 px circle; classes `rec` / `run` / `ready` / `err` / `idle` per D14) ·
  `.voice-state` (Inter 12 px 600 `fg`) · `.voice-time` (JetBrains Mono 11 px `fg3`) ·
  `.voice-meter` (only while listening: 28 bars 2 px wide, gap 2 px, height 3–16 px from the last 28 levels, `acc`; a `gtk::DrawingArea` 110 × 16 px) ·
  spacer · `.voice-target` (JetBrains Mono 11 px `fg3`: `→`, the agent glyph 12 px (R115), Pane name in `fg2` 500, ` · `, Worktree or link name) ·
  close `.voice-close` (flat 22 × 22 px, `ph-x-symbolic` 13 px `fg3`, tooltip `Discard (Esc)`).
- **Heard** `.voice-heard` (padding 8 12 px, gap 8 px): `ph-quotes-symbolic` 14 px `fg3`, text Inter italic 12.5 px / 1.5 `fg2`, tentative words `fg3` (Pango span). After the draft is Ready: folded to one line, ellipsized end, with a `show` / `hide` link button (`.voice-heard-toggle`, JetBrains Mono 11 px `fg3`) at the right. Hidden when there are no words.
- **Draft** `.voice-draft` (`gtk::TextView` in a `ScrolledWindow`, margin 0 12 px, `bg0`, 1 px `line`, radius 6 px, padding 9 11 px, Inter 13.5 px / 1.55 `fg`, wrap word-char, min height 70 px, max 14 lines then scroll); focused → border `acc-line`. Placeholder (overlay label `fg3`): listening without a draft `The prompt appears here when you let go of F8.`; error without a draft `Nothing heard yet.`. Text tags: backtick spans → `code` (JetBrains Mono 12.5 px, background `bg3`, the backticks hidden with `invisible`), `changed` (background `acc-dim`, underline `acc-bar` — GTK: `underline-rgba`), `unsure` for `[?] "…"` (JetBrains Mono 12 px 500 `wait` on `wait-dim`). While revising: the whole view at opacity 0.5 (class `stale`, not editable). While drafting: not editable, a 2 × 15 px `acc` caret after the text (a child anchor widget).
- **Activity** `.voice-activity` (padding 7 12 0 px, min height 24 px, gap 8 px, JetBrains Mono 11 px `fg3`): icon 13 px (`ph-file-text` Read, `ph-magnifying-glass` Grep, `ph-list` Glob, `ph-info` the listening note, `ph-warning-diamond` error), text, and `.voice-step` `n / max` at the right. Listening note: `Reading the session and <worktree or link> while you speak`. Error: class `error` → `err`, the `Set up voice…` link `acc`. Hidden when empty (Ready without error).
- **Footer** `.voice-footer` (padding 8 8 8 12 px, margin-top 8 px, top border 1 px `line`, gap 14 px, Inter 11.5 px `fg3`): hints left (`kbd` style = the existing key-hint style A.13: JetBrains Mono 10.5 px `fg3`, 1 px `line2`, radius 3 px, `bg2`), spacer, `Paste only` + kbd `Alt ↵` (secondary button, 26 px, A.15 style) and `Send` + kbd `↵` (`.suggested-action`, `acc` / `acc-fg`, 600, 26 px, radius 6 px, icon `ph-arrow-elbow-down-left` 14 px; its kbd border `acc-fg` at 30 %).
  Hint texts by state: listening (no draft) `Release F8 to draft`, `Esc cancel`; listening in toggle mode `Press F8 to draft`, `Esc cancel`; revising `Release F8 to apply`, `Esc keep the draft`; transcribing / drafting `Esc stop`; ready `Hold F8 to revise`, `Esc discard`; after edits, first Esc `Esc again to discard` (`wait`); error `Hold F8 to try again`, `Esc close`; empty `Hold F8 and speak`, `Esc close`. (`F8` = the bound key's label.)

### 5.2 Pane header and Statusline
- Mic button (D3): 22 × 22 px flat `.pane-button .voice-button`, icon 13 px `fg3`; `.recording` → icon `err`, background `err` at 14 % (token `rec-dim`, §5.5); `.open` → `acc` on `acc-dim`.
- Statusline cell (D18) `.status-voice`.

### 5.3 Spec sheet I (`docs/internals/design/spec/i-voice.md`, same columns and rules as the other sheets)

Scenes (1440 × 900, scale 1, dark + light, the mockup's data: Shop / shop-web / checkout-v2, Agent Pane `planner` (claude · opus) on the left 1.35 : 1 of `tests`): **I-a** listening (first hold, 6 s), **I-c** ready, **I-e** revised (2 changed bullets, one `[?]`). Scene data is written by `debug.voice scene <a|c|e>` (§6), not by real audio.

| ID | Element | Value | Tolerance | How measured |
|---|---|---|---|---|
| I.1 | Composer box | `bg2`, 1 px `line2`, radius 8 px, `shadow` | exact | `geometry voice-composer → background, border, radius` |
| I.2 | Composer place | left/right 10 px inside the Pane body, bottom 10 px, width ≤ 760 px, centred | ±1 px | `geometry voice-composer → x, y, width` vs `geometry pane-body nth=0` |
| I.3 | Listening ring | border `acc-line`; glow screenshot | exact; golden | `geometry voice-composer → border` (I-a); screenshot |
| I.4 | Header | 34 px high, padding 0 6 0 12 px, bottom 1 px `line`, gap 8 px | ±1 px | `geometry voice-header → height, padding, border.bottom` |
| I.5 | State dot | 8 × 8 px, radius 4 px; `err` (I-a), `acc` (I-c) | ±1 px; exact | `geometry voice-dot → width, height, background` |
| I.6 | State label | Inter 12 px 600 `fg`; `Listening` (I-a), `Ready` (I-c, I-e) | ±0.5 px; exact text | `geometry voice-state → font, color, label` |
| I.7 | Time | JetBrains Mono 11 px `fg3`; `0:06` (I-a), `0:11 · 3.2 s` (I-c), `revision 2 · 1.9 s` (I-e) | exact | `geometry voice-time → font, color, label` |
| I.8 | Meter | 110 × 16 px, visible only in I-a | ±1 px; exact | `geometry voice-meter → width, height, visible` |
| I.9 | Target | JetBrains Mono 11 px `fg3`; label `→ planner · checkout-v2` | exact | `geometry voice-target → font, color, label` |
| I.10 | Close | 22 × 22 px, icon 13 px `fg3` | ±1 px | `geometry voice-close → width, height` |
| I.11 | Heard text | Inter italic 12.5 px `fg2`, padding 8 12 px; quote icon 14 px `fg3` | ±0.5 px; exact | `geometry voice-heard → padding, font, color` |
| I.12 | Heard folded | I-c: one line (height ≤ 20 px), `show` toggle JetBrains Mono 11 px `fg3` | ±1 px | `geometry voice-heard → height`; `geometry voice-heard-toggle → font, label` |
| I.13 | Draft box | `bg0`, 1 px `line` (`acc-line` when focused, I-c), radius 6 px, padding 9 11 px, margin 0 12 px | ±1 px; exact | `geometry voice-draft → background, border, radius, padding, margin` |
| I.14 | Draft text | Inter 13.5 px `fg`, line height 1.55 | ±0.5 px; exact | `geometry voice-draft → font, color` |
| I.15 | Code spans | JetBrains Mono 12.5 px on `bg3`; backticks not drawn | exact; golden | `debug.voice info` → `draft_tags` has `code` ranges; screenshot |
| I.16 | Changed marks | I-e: `acc-dim` behind exactly the two changed bullets' words | exact | `debug.voice info` → `changed` ranges = 2; screenshot |
| I.17 | Unsure mark | I-e: `[?] "agent tests"` JetBrains Mono 12 px 500 `wait` on `wait-dim` | exact | `debug.voice info` → `unsure` = 1; screenshot |
| I.18 | Placeholder | I-a: `The prompt appears here when you let go of F8.` `fg3` | exact text | `geometry voice-placeholder → color, label` |
| I.19 | Activity line | JetBrains Mono 11 px `fg3`, padding 7 12 0 px, min height 24 px; I-a text `Reading the session and checkout-v2 while you speak` | ±1 px; exact | `geometry voice-activity → font, color, padding, height, label` |
| I.20 | Footer | padding 8 8 8 12 px, top 1 px `line`, Inter 11.5 px `fg3`; hints I-c `Hold F8 to revise`, `Esc discard` | ±1 px; exact | `geometry voice-footer → padding, border.top, font`; `debug.voice info` → `hints` |
| I.21 | Send | `acc` fill, `acc-fg` 600 12 px, 26 px high, radius 6 px; the only `.suggested-action` on screen | exact | `geometry voice-send → background, color, height, radius`; `count .suggested-action = 1` |
| I.22 | Paste only | secondary (A.15), 26 px high | exact | `geometry voice-paste → height, background, border` |
| I.23 | Disabled buttons | I-a: Send and Paste only insensitive (opacity 0.45) | exact | `geometry voice-send → opacity` |
| I.24 | Mic button | 22 × 22 px; I-a `recording`: icon `err`, background `rec-dim`; I-c `open`: `acc` on `acc-dim` | ±1 px; exact | `geometry voice-button nth=0 → width, color, background, classes` |
| I.25 | REC cell | I-a: `REC 0:06` JetBrains Mono 11 px `err`, icon 13 px | exact | `geometry status-voice → font, color, label` |
| I.26 | Contrast | `fg2` on `bg2`, `fg3` on `bg0`/`bg2`, `wait` on `wait-dim`/`bg0`, `acc-fg` on `acc`, `err` on `bg1` ≥ 4.5 : 1 | ≥ | `contrast.py` |
| I.27 | 1280 px / Text scale 200 % | no clipped label, no horizontal scroll in the composer | exact | `debug.ui clipped`, `hscroll` (m40 helpers) |

### 5.4 Icons to add (copy verbatim from unpkg, Phosphor 2.1.1)
`https://unpkg.com/@phosphor-icons/core@2.1.1/assets/regular/<name>.svg` → `crates/rade-app/assets/icons/ph-<name>-symbolic.svg` for `microphone`, `quotes`, `arrow-elbow-down-left`, `warning-diamond`; `…/assets/fill/microphone-fill.svg` → `ph-microphone-fill-symbolic.svg`. (`file-text`, `magnifying-glass`, `info`, `x` exist; `list` was added by T8.) T0 checks each URL answers 200 first. `tokens_tests.rs` `r102_e…` lists them.

### 5.5 CSS and tokens
- New token `rec-dim` in `tokens.toml`: dark `#ff6b6b24`, light `#c230301f` (err at 14 % / 12 %), with its row in the README's token tables (AA: decorative, no contrast set). `build.rs` regenerates `tokens.css`.
- `style.css` (tokens only, no raw hex — `tokens_tests.rs` enforces): `.voice-composer`, `.voice-composer.listening`, `.voice-header`, `.voice-dot.{rec,run,ready,err,idle}` (`rec` adds `box-shadow: 0 0 0 3px` of `rec-dim`), `.voice-state`, `.voice-time`, `.voice-meter`, `.voice-target`, `.voice-target .pane-name`, `.voice-close`, `.voice-heard`, `.voice-heard-toggle`, `.voice-draft`, `.voice-draft:focus-within`, `.voice-draft.stale`, `.voice-placeholder`, `.voice-activity`, `.voice-activity.error`, `.voice-step`, `.voice-footer`, `.voice-footer .key-hint` (reuse the existing kbd class — grep `palette-item-hint` / A.13's class and use it, don't add a new kbd style), `.voice-send`, `.voice-paste`, `.voice-button.recording`, `.voice-button.open`, `.status-voice`.

---

## 6. Debug API

`debug.voice {"op", …}` (test mode only, ADR-0021). Every op drives the same code paths as the UI.

`info` →
```json
{"open": true, "pane": "planner", "state": "listening"|"transcribing"|"drafting"|"ready"|"error"|"empty"|null,
 "mode": "hold"|"toggle"|null, "revision": 0, "label": "Listening", "time": "0:06",
 "heard": "ừ thì …", "tentative": "á sửa cho nó", "heard_folded": false,
 "draft": "Fix …", "draft_edited": false, "draft_tags": {"code": [[s,e]…], "changed": [[s,e]…], "unsure": [[s,e]…]},
 "activity": "Read src/checkout/useTotals.ts", "step": "2 / 6", "error": null,
 "hints": ["Hold F8 to revise", "Esc discard"], "send_enabled": true, "paste_enabled": true,
 "agent": {"pid": 1234, "turns": 1, "argv": ["claude", "-p", …]} | null,
 "recorder": {"pid": 1240, "bytes": 192000} | null, "stt_requests": 3}
```
Ops: `info`; `toggle` (= the mic button of `pane`); `send`, `paste` (= Enter / Alt+Enter); `escape`; `type` (`text`: inserts into the draft as typing does, marks it edited); `heard_toggle`; `setup_link` (= `Set up voice…`); `scene` (`arg` `a`|`c`|`e`: fills the composer of the focused Agent Pane with the mockup's data and state, no processes — golden scenes only).
`debug.ui key` gains `press:<key>` and `release:<key>` (e.g. `press:F8`, `release:F8`): a press without release holds; a second `press:` while held is an auto-repeat.

---

## 7. Tasks

Order (one implementer for T0–T6; T7 and T8 in parallel worktrees; then one for T9–T10):
```
T0 spike ─► T1 docs ─► T2 config ─► T3 context ─► T4 agent ─► T5 stt + words ─► T6 composer model
                                                                                     ├─► T7 controller (implementer B)
                                                                                     └─► T8 view       (implementer C)
                                                                                              └─► T9 E2E ─► T10 docs, gate
```
B owns `voice.rs`, `handlers.rs`, `app.rs`, `keys.rs`; C owns `voice_view.rs`, `layout_view.rs`, `chrome.rs`, `style.css`, `tokens.toml`, icons. The contract between them is core `voice::composer` (`Composer`, `Event`, `Action`, `ComposerView`) plus this tiny app interface, which T6 adds as stubs in `voice.rs` / `voice_view.rs` so both compile from the start:
`voice_view::open(pane_id) / close() / render(&ComposerView) / set_levels(&[f32]) / append_draft(&str) / set_draft(&str, &DraftTags) / draft_text() -> String` and `voice::on_ui(Event)`.

### T0 — Spike: verify the UNVERIFIED facts (no product code) — DONE 2026-10-25 except whisper-server (see research doc)
1. `claude -p` stream-json with `--tools Read,Grep,Glob --max-turns 2` in a temp git repo: send a turn needing 3 reads; record whether the limit applies per user message, the exact error/`result` subtype when it is hit, and whether turn 2 still works. Record 2 turns' full output with `--include-partial-messages` → `tests/fixtures/voice/claude-stream.jsonl` (replace paths and names with `src/checkout/useTotals.ts` etc.). Check that `--tools ""` really leaves no tool, and that no `.jsonl` appears under `~/.claude/projects/` (list before / after).
2. `codex exec --ephemeral --sandbox read-only --skip-git-repo-check --json -` with a prompt on stdin in the same repo: record output → `codex-exec.jsonl`; check no new rollout file under `$CODEX_HOME/sessions` (if one appears, report — D7 changes).
3. A real Claude transcript and Codex rollout (from this machine, a throwaway session in the temp repo) → `claude-transcript.jsonl`, `codex-rollout.jsonl` (trimmed to ≤ 12 exchanges, names replaced).
4. whisper.cpp: build `whisper-server` (CUDA) in the scratchpad, `--inference-path /v1/audio/transcriptions --port 8178`, model `large-v3-turbo`; post a 5 s Vietnamese+English WAV with `curl -F file=@… -F response_format=json -F prompt="handleSubmit useTotals"`; record the response shape and the time. If the path option or field names differ, report.
5. The 5 icon URLs of §5.4 answer 200.
Write `docs/internals/research/voice.md` (commands, versions, outputs, timings). **If any finding contradicts D5, D7 or D9, stop and report.**
Commit: `docs(M46): voice research — claude stream-json, codex exec, whisper-server` (`Changelog: skip`) + `test(R124): voice stream and transcript fixtures`.

### T1 — Spec docs
Files: `docs/internals/PRD.md`, `docs/internals/adr/0046-voice-prompts.md`, `docs/internals/MILESTONES.md`, `CONTEXT.md`, `docs/internals/design/spec/i-voice.md` (§5.3 with the README's header/rules lines), `docs/internals/design/spec/README.md` (row I in the screens table; `rec-dim` in both token tables).
Do: paste §3.1–§3.4, §5.3 verbatim. Check: `scripts/trace.sh --no-write` runs.
Commit: `docs(M46): R124, ADR-0046, spec sheet I`

### T2 — Config, keybinding, Settings rows (core)
`config.rs`:
```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct VoiceConfig {
    pub agent: VoiceAgent,          // "claude" | "codex", default Claude
    pub model: String,              // "" = "haiku" (claude) / codex's own default
    pub agent_command: String,      // "" = built-in (D7)
    pub tools: VoiceTools,          // "read" | "none", default Read
    pub max_turns: u32,             // default 6; clamp 1..=20 when read (fn max_turns())
    pub timeout_secs: u32,          // default 60; clamp 10..=300 (fn timeout())
    pub max_recording_secs: u32,    // default 120; clamp 10..=600
    pub live_preview: bool,         // default true
    pub draft_language: DraftLanguage, // "same" | "en", default Same
    pub stt_url: String,            // default "http://127.0.0.1:8178/v1/audio/transcriptions"
    pub stt_model: String,          // default "whisper-1" (ignored by whisper.cpp, needed by OpenAI)
    pub stt_language: String,       // default "auto"; Settings offers auto / vi / en; config.toml takes any
                                    // ISO 639-1 code (2–3 lowercase letters) — anything else reads as "auto" (fn stt_language())
    pub stt_key_command: String,    // default ""
    pub record_command: String,     // default "pw-record --rate 16000 --channels 1 --format s16 -"
}
```
Enums `#[serde(rename_all = "snake_case")]`, `Copy`, `Default`. `Config.voice: VoiceConfig`. `DEFAULT_KEYBINDINGS` += `("voice", "F8")` with a comment like the others (why F8 is free). Not added to `LAYERED`; `layers.rs` `explain` reports a `[voice]` key in a non-user layer as `ignored (user config only)`.
`settings.rs`: category `"Voice"` after `"Agents"` (before `"Statusline"`); rows:
| key | label | description | kind |
|---|---|---|---|
| `voice.agent` | `Voice agent` | `Who turns your words into a prompt. Claude Code is faster (one process per draft).` | `Choice(&["claude", "codex"])` |
| `voice.tools` | `Voice agent reads files` | `Read, search and list files of the checkout to understand you. Never writes.` | `Choice(&["read", "none"])` |
| `voice.max_turns` | `Max steps` | `Claude Code turns per draft for reading and searching files (1 to 20). Codex has no limit.` | `Integer { min: 1, max: 20 }` |
| `voice.timeout_secs` | `Draft timeout (seconds)` | `Stop a draft that takes longer (10 to 300).` | `Integer { min: 10, max: 300 }` |
| `voice.max_recording_secs` | `Max recording (seconds)` | `A take stops by itself after this long (10 to 600).` | `Integer { min: 10, max: 600 }` |
| `voice.live_preview` | `Live words` | `Show what you say while you speak (one speech-to-text request per second).` | `Bool` |
| `voice.draft_language` | `Draft language` | `The language you spoke, or always English.` | `Choice(&["same", "en"])` |
| `voice.stt_language` | `Speech language` | `The language you speak, for speech-to-text. Auto detects it per take; set it when you always speak one language (more reliable on short takes). Other ISO 639-1 codes (ja, zh, fr, …) in config.toml.` | `Choice(&["auto", "vi", "en"])` |
`palette.rs`: action `("voice", "Voice Prompt")` (runs the tap behaviour on the focused Pane).
Tests: `r124_j_voice_config_defaults_settings_and_layers`.
Commit: `feat(R124): [voice] settings and the F8 keybinding` / `Changelog: skip`

### T3 — `voice/context.rs` (tests first)
API:
```rust
pub struct Bundle { pub sections: Vec<(String, String)>, pub vocabulary: String }
pub fn claude_transcript(config_dir: &Path, cwd: &Path, session: &str) -> Option<PathBuf>;
pub fn codex_rollout(codex_home: &Path, session: &str) -> Option<PathBuf>;
pub fn claude_tail(jsonl: &str) -> Vec<(Role, String)>;   // last 6 exchanges, D8 cuts
pub fn codex_tail(jsonl: &str) -> Vec<(Role, String)>;
pub fn screen_tail(screen: &str) -> String;               // last 120 lines, trailing blanks trimmed
pub fn layout(ls_files: &str) -> String;                  // depth-2 dirs + counts, ≤ 3,000 chars
pub fn vocabulary(tail: &[(Role, String)], changed: &[String]) -> String;
pub fn gather(input: &GatherInput) -> Bundle;             // runs git in input.dir; reads rules
pub fn render(b: &Bundle) -> String;                      // "## Session\n…\n\n## Git\n…"
```
`GatherInput { dir, agent: Option<String>, model, session_id, claude_config_dir, codex_home, screen: Option<String>, where_label, viewer: Option<(PathBuf, Option<(u32,u32)>)> }`. Section names, in order: `Pane`, `Session` (or `Screen`), `Git`, `Rules`, `Layout`, `Viewer`; empty sections omitted.
Tests: `r124_a_…`, `r124_b_…`, `r124_c_…` (temp git repo), `r124_d_…` (fixtures from T0).
Commit: `feat(R124): voice context from the session, git and the checkout` / `Changelog: skip`

### T4 — `voice/agent.rs` (tests first)
`claude_argv(cfg, append_prompt) -> Vec<String>` = exactly
`["claude","-p","--model",<m>,"--tools",<"Read,Grep,Glob"|"">,"--max-turns",<n>,"--no-session-persistence","--setting-sources","","--input-format","stream-json","--output-format","stream-json","--include-partial-messages","--verbose","--append-system-prompt",<SYSTEM with LANGUAGE>]`;
`codex_argv(cfg) -> Vec<String>` = `["codex","exec","--ephemeral","--sandbox","read-only","--skip-git-repo-check","--json"] + ["-m", m] if set + ["-"]`;
with `agent_command` set: `["sh","-c",agent_command]`. `env_for(base: &Env) -> Env` drops every key starting `RADE_`. `turn_message(kind: First{context}|Revise{draft}, words) -> String` (§3.5), `claude_stdin_line(text) -> String` (`{"type":"user","message":{"role":"user","content":<text>}}`). Parsers per T0's fixtures → `AgentEvent`. `activity(tool, input) -> String` labels (paths relative to cwd). `strip_fence`.
Tests: `r124_e_…`, `r124_f_…`.
Commit: `feat(R124): voice agent command, turns and stream parsing` / `Changelog: skip`

### T5 — `voice/stt.rs`, `voice/words.rs` (tests first)
`wav(pcm: &[i16]) -> Vec<u8>`; `level(chunk: &[i16]) -> f32` (RMS / 32768, sqrt-scaled to 0..1); `curl_argv() -> Vec<String>` (`curl -sS --max-time 30 -K /dev/fd/3 -F file=@-;filename=take.wav;type=audio/wav`); `curl_config(cfg, key: Option<&str>, vocabulary) -> String` (`url = "…"`, `header = "Authorization: Bearer …"` only with a key, `form = "model=…"`, `form = "response_format=json"`, `form = "language=…"` unless auto, `form-string = "prompt=…"`; values escaped for curl config); `parse(body) -> Result<String, String>` (`text` trimmed; else the D17 texts); `split_tentative(text) -> (stable, tentative)`; `error_text(...)`. `words::changed_spans`.
Tests: `r124_h_…`, `r124_i_…`.
Commit: `feat(R124): speech-to-text request and draft word diff` / `Changelog: skip`

### T6 — `voice/composer.rs` model + app stubs (tests first)
`Event`: `KeyDown{repeat: bool, at_ms}`, `KeyUp{at_ms}`, `Tap` (button / palette), `FocusLost`, `PaneClosed`, `Escape{at_ms}`, `Levels`, `Partial(String)`, `Final(String)`, `SttError(String)`, `RecordError(String)`, `AgentEvent(AgentEvent)`, `AgentExited{ok, stderr}`, `Tick{at_ms}` (timeouts, max recording, 2 s Esc window), `DraftEdited`, `Send`, `Paste`.
`Action`: `StartRecorder`, `StopRecorder{keep: bool}`, `SttPartial`, `SttFinal`, `SpawnAgent`, `SendTurn(String)`, `KillAgent`, `SendToPane{enter: bool, text}`, `Close`, `FocusDraft`, `Render`.
`view() -> ComposerView` with every text of §5.1 (labels, hints, time, placeholder, activity, step, button sensitivity, classes).
Table test `r124_g_composer_states` covering every row of D2, D11, D13, D14, D17. App: add the stub fns of §7's interface (`todo!()`-free: empty bodies) so B and C compile.
Commit: `feat(R124): Voice Composer state machine` / `Changelog: skip`

### T7 — Controller (implementer B, worktree)
`voice.rs`: recorder child (`sh -c record_command`, stdout reader thread → PCM + levels → main loop via `glib::MainContext` channel), STT requests on a worker thread (one at a time; live every 1200 ms while recording if `live_preview`), agent child with stdin writer + stdout line reader → `AgentEvent`s, turn timer, process-group kill on every exit path (`process.rs` helpers), Pane close / app quit hooks (`app.rs`), context `gather` off-thread at the first press (screen text read on the UI thread first via the existing `pane.read` code), `SendToPane` through the existing `pane.send` function with `paste: true`. `keys.rs`: `voice` binding = press / release / repeat handling (D2) in the capture handler (release via `connect_key_released`), ignored while the palette / Settings / Overview is open; `debug_key` `press:` / `release:`. `handlers.rs`: `debug.voice` (§6). Toasts of D1 / R124-q.
Check: `cargo test --workspace`, clippy; by hand in a headless Weston with `tests/e2e/fake-*.{py,sh}` (T9 writes them; B may write first drafts of the fakes in its worktree and hand them to T9).
Commit: `feat(R124): dictate to an Agent Pane — hold F8, a voice agent drafts the prompt` with footer `Changelog: Voice prompts: hold F8 on an Agent Pane, speak, and a read-only voice agent turns it into a clear prompt you can revise by voice before sending.`

### T8 — View (implementer C, worktree)
`voice_view.rs` per §5.1 (render only from `ComposerView`; keys inside the composer → `voice::on_ui`), `layout_view.rs` mic button + overlay slot, `chrome.rs` REC cell, icons §5.4, token `rec-dim`, CSS §5.5, `debug.voice scene a|c|e` data (mockup texts verbatim). Geometry names = the CSS classes.
Check: build; screenshot the three scenes in a headless Weston in dark and light (`rade debug screenshot`) and **look at them** next to the mockup (`#a`, `#c`, `#e`, `-light`).
Commit: `feat(R124): Voice Composer, mic button and REC cell` / `Changelog: skip`

### T9 — E2E (after B and C merge)
`tests/e2e/m46.sh` (preamble from `m44.sh`: private XDG, D-Bus, Weston, `rade` helpers; config sets `[voice] stt_url` to the fake's port, `record_command = "tests/e2e/fake-mic.sh"`, `agent_command = "python3 -I tests/e2e/fake-voice-agent.py"`; a fake Claude config dir with a session file named after the Agent Pane's session id (set by `rade status --session-id`)). Checks in AC order R124-k … R124-r, one `check "R124-x …"` each; R124-n's `REAL=1` part skips with the reason when `claude` is missing. Screenshots dark + light of scenes I-a, I-c, I-e and of a live drafting state → `target/e2e-m46/`; look at each. Goldens `tests/e2e/golden/m46/` via `APPROVE=1` — **the owner approves them**. Add `m46` to `.github/workflows/ci.yml` (lightest shard) and to `tests/e2e/m40.sh`'s 1280 px / 200 % scene list if it takes a list (I.27).
Run: `m46.sh`, `m35.sh`, `m37.sh`, `m40.sh` (MODES both), `settings.sh`.
Commit: `test(R124): m46 suite with fake mic, STT and voice agent`

### T10 — Docs and gate
- `docs/features/voice.md` (new): what it does (one screenshot from the demo), set up speech-to-text (whisper.cpp with CUDA: build line, `large-v3-turbo` download, `whisper-server --port 8178 --inference-path /v1/audio/transcriptions -m …`, a systemd user unit; PhoWhisper note; cloud: OpenAI / Groq `stt_url` + `stt_key_command = "secret-tool lookup …"` example), keys, settings and config keys with defaults, privacy (what leaves the machine, nothing kept).
- `docs/keybindings.md` (F8 hold / tap; in the composer Enter, Alt+Enter, Shift+Enter, Esc), `docs/features/settings-and-config.md` (Voice rows), `docs/API.md` (`debug.voice`, `debug.ui key press:/release:`), `CLAUDE.md` (m46 command line + code-map entries for `voice/`, `voice.rs`, `voice_view.rs`).
- Demo: `scripts/demo/voice.sh` (scene I-c via `debug.voice scene c` on the made-up repo) + `NO_RUN=1 scripts/demo/collect.sh`; look at the images.
- `scripts/trace.sh --strict` → every R124 AC traced.
- `docs/internals/checks/M46.md`: one row per AC and exit criterion: command, result, screenshot paths; open items: goldens I (owner), a real-microphone run on the owner's desktop (below).
- Full local CI (memory: docker check + all E2E shards + deb/m10) — report results.
Commits: `docs(R124): voice prompts user guide` (`Changelog: skip`), `docs(M46): checks`.

---

## 8. Exit criteria (M46 done)
1. Every AC of R124 has a passing test (`scripts/trace.sh --strict` green).
2. `cargo clippy --workspace --all-targets` clean, `cargo test --workspace` green.
3. m46, m35, m37, m40, settings green locally; CI shards green.
4. Screenshots of T9 looked at in dark and light; no clipped text at 1280 px / 200 %.
5. `docs/internals/checks/M46.md` records all of the above.
6. Owner: approves goldens I; one real run on the desktop — local whisper-server + real Claude Code Haiku, a Vietnamese take on a fresh session and on a session with history, one spoken revision, Send — and notes the time from release to Ready.
