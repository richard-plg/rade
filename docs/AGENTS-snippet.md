<!-- Append to your AGENTS.md (Codex, OpenCode, Antigravity) to teach the agent to drive others through RADE. -->
## Other agents (RADE)

When `$RADE_SOCKET` is set you run inside RADE and can delegate to other coding agents with the `rade` CLI:

- `rade spawn --agent claude|codex|agy|opencode --name <n> "<prompt>"` opens a subagent next to you (or `--cmd "<command>"` for any program). It prints the pane id.
- `rade wait <n> --until done --timeout 1200` blocks until its turn ends and prints the status: `done`; `waiting` (it needs input: `rade read` it, then answer or ask the user); `error`/`exited` (it failed). After a RADE restart a pane that was mid-turn is `interrupted` in `rade list`: `rade send <n> "continue" --enter`.
- `rade read <n> --lines 200` returns its screen text (`--scrollback` for everything); `rade send <n> "<text>" --enter` sends a follow-up; `rade close <n>` closes it.
- `rade open <path>[:line]` shows the user a file (Markdown rendered) in a Viewer beside you.
- Verify a web change: `rade browser open 5173` (a Browser Pane beside you on `http://localhost:5173/`; `rade browser ports` lists dev servers in RADE panes), `rade browser snapshot` (the page as text, actionable elements tagged `[ref=e3]`), `rade browser click e3` / `fill e2 "text"` / `press Enter`, `rade browser wait --text "Saved"`, `rade browser console` (new console messages and JS errors), `rade browser eval "<js>"` (JSON), `rade browser screenshot /tmp/x.png`.
- `rade list --tree` shows your Workspace's panes and statuses (`--all`: every Workspace); `$RADE_WORKSPACE` names yours. `rade source add owner/repo` clones a repository into your Workspace and registers it. Exit codes: 4 not found or ambiguous name (use the id), 5 limit (max 8 live subagents, depth 3), 6 wait timed out.

Example: `rade spawn --agent claude --name rev "Review git diff; list bugs with path:line"; rade wait rev --until done; rade read rev --lines 150`.
