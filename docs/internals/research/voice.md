# Voice prompts research (M46, T0)

Checked 2026-10-25 on the owner's machine (Ubuntu 26.04, RTX 5080 16 GB, CUDA 13.3) with Claude Code
2.1.310, codex-cli 0.164.0 and whisper.cpp `231638a` (shallow clone of `ggml-org/whisper.cpp` HEAD
that day). Every command ran in a throwaway git repository holding `src/checkout/useTotals.ts`,
`promo.ts` and `CheckoutForm.tsx`; the session files those runs created were deleted afterwards.
Recorded outputs (scrubbed) are the fixtures in `crates/rade-core/tests/fixtures/voice/`.

## 1. Claude Code headless as the voice agent

Command:
```sh
claude -p --model haiku --tools "Read,Grep,Glob" --max-turns 2 --no-session-persistence \
  --setting-sources "" --input-format stream-json --output-format stream-json \
  --include-partial-messages --verbose
```
stdin, one JSON object per line: `{"type":"user","message":{"role":"user","content":"<text>"}}`.

| Question | Answer |
|---|---|
| Several turns in one process? | Yes. A second line on stdin starts a new turn with the earlier ones in context (turn 2 recalled turn 1). |
| Session file? | None: no `<id>.jsonl` under `~/.claude/projects/`. Claude Code still creates an empty `projects/<cwd slug>/memory/` directory. |
| User hooks? | `--setting-sources ""` skips them (without it the init shows `hook_started` / `hook_response` from the user's settings). |
| `--max-turns` | Per user message. Message 1 asked for 3 file reads with `--max-turns 2`: 3 `Read` tool uses (two in one model turn), then `result` with `subtype: "error_max_turns"`, `is_error: true`, no `result` text, `num_turns: 3`. Message 2 in the same process answered normally (`subtype: "success"`). |
| Latency | Trivial Haiku turn without tools: `duration_ms` ≈ 1,300; 3 reads + max-turns stop: 3,822 ms; a one-word answer after them: 2,675 ms (thinking block first). |
| Stream shape | `system` `init` per message; `stream_event` with `message_start`, `content_block_start` (`tool_use` + `name` / `text` / `thinking`), `content_block_delta` (`text_delta`, `input_json_delta`, `thinking_delta`, `signature_delta`), `content_block_stop`, `message_delta`, `message_stop`; a full `assistant` message after each block holds the `tool_use` with its complete `input` (`file_path` absolute); `user` lines carry `tool_result`s; `rate_limit_event`; one `result` per user message. |

Transcripts (`<config dir>/projects/<slug(cwd)>/<session>.jsonl`, slug = non-alphanumerics → `-`):
besides `user` / `assistant` lines they hold `queue-operation`, `attachment`, `atis-latch`,
`api-request-shape`, `api-request-blob` (with `message.role` `user` / `system` / `assistant` — not
turns), `api-request`, `last-prompt`, `cost-state`, `mode`. A `user` line whose content is a list of
`tool_result` blocks is a tool result, not the user speaking.

## 2. Codex

`codex exec --ephemeral --sandbox read-only --skip-git-repo-check --json -` (prompt on stdin):
- No rollout written with `--ephemeral` (without it: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`).
- One prompt per process; there is no stream-json input, so a revision is a new process.
- Events: `thread.started` (`thread_id`), `turn.started`, `item.started` / `item.completed` with
  `item.type` `agent_message` (`text`), `command_execution` (`command` = `/usr/bin/zsh -lc 'cat src/…'`),
  `error` (config warnings), `turn.completed`. Files are read through shell commands in the
  read-only sandbox, not file tools.
- It still loads the user's Codex hooks and MCP servers (stderr showed an MCP auth error). Harmless
  for RADE once `RADE_*` is stripped from its env.
- No turn-limit flag.

Rollouts: the words the user typed and the answers are `event_msg` `item_completed` items of type
`UserMessage` / `AgentMessage`. `response_item` `message`s also exist, but the first user one holds
Codex's injected `# AGENTS.md instructions …` and `<environment_context>` texts.

## 3. whisper.cpp server (speech-to-text)

Build and run (no sudo; model 1.6 GB):
```sh
git clone --depth 1 https://github.com/ggml-org/whisper.cpp ~/.local/src/whisper.cpp
cd ~/.local/src/whisper.cpp
cmake -B build -DGGML_CUDA=1 -DCMAKE_CUDA_ARCHITECTURES=120 -DCMAKE_BUILD_TYPE=Release
cmake --build build -j 20 --target whisper-server
curl -L -o ~/.local/share/whisper/ggml-large-v3-turbo.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
build/bin/whisper-server -m ~/.local/share/whisper/ggml-large-v3-turbo.bin \
  --host 127.0.0.1 --port 8178 --inference-path /v1/audio/transcriptions -l auto
```
- `--inference-path /v1/audio/transcriptions` works; the multipart fields RADE sends (`file`,
  `model` — ignored, `response_format=json`, `language`, `prompt`) are all read
  (`examples/server/server.cpp` `get_req_parameters`). Answer: `{"text":" … \n … \n"}` (leading
  space, a newline per segment).
- Request parameters apply to that request only (a copy of the server defaults per request). A
  request **without** `language` uses the server's `-l`, whose default is `en` — so the docs must
  start the server with `-l auto` (or `-l vi`), and RADE sends `language` whenever the setting is not
  `auto`.
- The file can come on stdin (`-F "file=@-;filename=take.wav;type=audio/wav"`) with the rest of the
  request in a config on another fd (`-K /dev/fd/3`): both verified. A WAV whose header claims more
  data than sent (a cut buffer) is still transcribed.
- Speed: a 14 s Vietnamese clip with English words → 0.10–0.20 s per request (GPU, warm). Live
  partials once a second are cheap.
- Quality (synthetic voice from Google TTS, so English words are read with Vietnamese phonetics):
  `Ừ thì cái phần promo code A, nó đang bị trừ 2 lần. À ở cái excitoto A, sửa cho nó chỉ trừ 1 lần
  thôi. À mà nhớ viết test cho trường hợp percent luôn.` — Vietnamese right; `useTotals` misheard as
  `excitoto` with or without `prompt=useTotals applyPromo handleSubmit` (a TTS artefact; the voice
  agent, which sees `useTotals` in the session and the checkout, is the part meant to fix such
  names). A real voice is checked in the owner's live run (M46 exit criterion 6).
- `language=vi` vs auto: same text on this clip.

## 4. Icons
`microphone`, `quotes`, `arrow-elbow-down-left`, `warning-diamond` (regular) and `microphone-fill`
(fill) at `https://unpkg.com/@phosphor-icons/core@2.1.1/assets/…`: HTTP 200 each.
