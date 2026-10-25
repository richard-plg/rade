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
