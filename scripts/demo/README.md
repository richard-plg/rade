# Demo scenes for the docs

Every screenshot and recording in `docs/images/` comes from these scripts. Each scene builds a
made-up world in a throwaway HOME (Workspace `acme` with real git repositories shop-web, shop-api,
shared-ui and billing — see `world.sh`; `more_world` adds the Workspaces `globex` and `personal`), runs the real `rade-app` in a headless Weston at
1440 × 900, drives it with the `rade` CLI and its test-mode `debug.*` methods, and writes
`target/demo/<scene>/<name>-dark.png`, `<name>-light.png`, and for some `<name>.mp4` / `.gif`.

Nothing touches your desktop, your real state or your agent logins (the E2E harness,
`tests/e2e/lib.sh`, is reused as is).

```sh
scripts/demo/workspace.sh            # one scene (NO_BUILD=1 to skip cargo build, KEEP=1 to keep its temp dir)
scripts/demo/collect.sh              # every scene, then copy what docs/ references into docs/images/
NO_RUN=1 scripts/demo/collect.sh     # only copy from the last runs
```

## The demo agent

`agent.sh` stands in for Claude Code / Codex (Presets `claude` and `codex` point at it in the
scenes' config). It prints a coding agent's turn, edits files for real, and reports its status
through `rade status --hook` like the real hooks. Each turn is a *play* in `plays/` chosen by a
regex on the prompt (the first match in file-name order wins; a scene whose prompts an earlier
play would also match calls `plays_first <play>…` from `lib.sh`); the directives are documented at
the top of `agent.sh`. Its banner says
`(demo)`. Real agents look like themselves.

## Adding a scene

Copy `workspace.sh`: `NAME=<scene>`, source `lib.sh` and `world.sh`, `start_app`, set the layout,
build the state, then `still <name>` for a dark + light pair or `rec_start <name>` …
`rec_stop` for a recording. Look at every image before using it in the docs. Keep names unique
across scenes: `collect.sh` finds images by file name.

## Narrated cuts for YouTube

```sh
scripts/demo/youtube.sh tour        # or grid → target/demo/youtube/<scene>.mp4 + youtube-<scene>.png
NO_RUN=1 MUSIC=… VOICE=am_michael scripts/demo/youtube.sh tour   # re-cut the last take: other music / voice
```

`narration/<scene>.tsv` holds the lines (label, on-screen caption, what the voice says). `tts.py`
reads them with Kokoro TTS (offline; the model goes to `target/tts/`), the scene runs with
`NARRATE=1` into `target/demo/<scene>-narrated/` — its `beat`s then last until the voice has read
every line so far, `mark`s note when each line starts — and `cut.py` lays voice, captions, sound
effects, ducked music (default: "Lofi Hip Hop Loop" by omfgdude, CC0) and an end card over it.
The narrated takes never reach `docs/images/`, except the `youtube-<scene>.png` thumbnails.
