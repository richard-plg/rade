# The `rade` CLI: agents driving agents

Everything in RADE can be done from a terminal with `rade`, so **any coding agent can drive
RADE** — start other agents, wait for them, read their answers, open a browser, ask the language
server. That's how Claude Code gets Codex to review its work, or fans a task out to three agents.

![Claude Code starts a Codex reviewer beside it, waits for it and reads its findings](../images/delegate.gif)
<sub>[Watch as MP4](../images/delegate.mp4)</sub>

## The loop

```sh
rade spawn --agent codex --name rev "Review git diff HEAD~1; list bugs as path:line"
rade wait rev --until done --timeout 900
rade read rev --lines 150
rade send rev "Also check the error handling" --enter
rade close rev
```

- `spawn` prints the new Pane's id. From inside an Agent, the new one is its **Subagent**, placed
  in the same Tab (Agent Grid). `--tab new`, `--split right|down`, `--worktree new` override.
- `wait --until done|waiting|exited|error|running|idle|/regex/` blocks until that status (or text
  on screen). `waiting` means it's asking something: `read` it, then answer or ask the user.
- `read` returns screen text (`--scrollback` for all of it, at least 10,000 lines kept for Agents).
- `send` types text; `--enter` submits.

## Teach your agents

`rade setup` installs the **`rade` skill** for Claude Code (plus `rade-workspace` and
`rade-config`). For Codex, OpenCode and Antigravity, append [AGENTS-snippet.md](../AGENTS-snippet.md)
(installed at `/usr/share/rade/AGENTS-snippet.md`) to your `AGENTS.md`. After that you can just
say *"have codex review this"* or *"try it three ways and pick the best"*.

## Commands

| Command | For |
|---|---|
| `rade spawn` · `send` · `read` · `wait` · `list` · `focus` · `close` · `rename` | Panes and Agents |
| `rade notify "build finished" --title CI` | raise a Notification on your Pane |
| `rade open src/cart/Cart.tsx:12` | show the user a file in a Viewer beside you |
| `rade tab list` · `rename` · `close` | Tabs |
| `rade worktree add` · `list` · `remove` | Worktrees |
| `rade review status` · `request` · `add` · `send` · `open` | the Review loop |
| `rade fanout` · `status` · `judge` · `choose` · `discard` · `wait` | Fan-out |
| `rade browser open` · `snapshot` · `click` · `fill` · `press` · `eval` · `console` · `screenshot` · `annotations` | the Browser Pane |
| `rade code def` · `refs` · `callers` · `callees` · `impl` · `hover` · `symbols` · `impact` | code navigation |
| `rade source` · `project` · `workspace` · `account` · `config` | organization and config |
| `rade setup` | install hooks and skills |

`rade <command> --help` for each; `--json` prints the raw result. The full, stable contract is
[docs/API.md](../API.md).

## Environment

Inside a RADE Pane these are set:

| Variable | Example |
|---|---|
| `RADE_SOCKET` | `/run/user/1000/rade/rade.sock` |
| `RADE_PANE_ID` | `p_42` |
| `RADE_WORKSPACE` | `acme` |
| `RADE_SOURCE` | `shop-web` |
| `RADE_WORKTREE` / `RADE_WORKTREE_PATH` | `coupons` / `~/workspaces/acme/.worktrees/shop-web/coupons` |
| `RADE_ACCOUNT` | `acme-work` (Agent Panes with an account) |

Outside RADE, `rade` talks to the running app on `$XDG_RUNTIME_DIR/rade/rade.sock`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | unexpected failure |
| 2 | bad arguments |
| 3 | RADE isn't running |
| 4 | not found, or an ambiguous name (use the id) |
| 5 | a limit: spawn depth (3) or live Subagents (8) |
| 6 | `wait` timed out |
| 7 | forbidden: an Agent Account of another Workspace, or a command that overrides the account's login |

## Safety

The socket only accepts your own user. Agents run with your permissions — RADE doesn't sandbox
them; use each agent's own permission settings. See [SECURITY.md](../../SECURITY.md).

## Related

- [Showcase: Claude asks Codex for a review](../showcases/claude-asks-codex-for-a-review.md)
