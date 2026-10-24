# Prior art — ADEs and Ghostty-based agent terminals

Researched 2026-09-30.

| Project | Stack | Linux | License | Relevant for RADE |
|---|---|---|---|---|
| [cmux](https://github.com/manaflow-ai/cmux) | Swift/AppKit + libghostty | no | — | Origin of the idea: vertical sidebar (branch, PR, cwd, ports, last notification), OSC 9/99/777 + `cmux notify`, jump-to-unread, socket CLI, in-app browser, resume integrations for many agents |
| [cmux-linux](https://github.com/bradwilson331/cmux-linux) | Rust + GTK4 + libghostty fork | yes | AGPL-3.0 | Reference only (ADR-0002). Layout-only restore; no IME; stale since 2026-03 |
| [limux](https://github.com/am-will/limux) | Rust + GTK4/libadwaita + libghostty fork | yes | MIT | Closest match. Crates: `limux-host-linux`, `limux-ghostty-sys`, `limux-core`, `limux-protocol`, `limux-control`, `limux-cli`. Workspaces, splits, tabs, WebKitGTK browser, socket CLI, agent env vars, notifications, session restore, .deb/AppImage. **M0 audits it: fork vs build (Q17).** |
| [Orca](https://github.com/stablyai/orca) | Electron + xterm.js | AppImage | MIT | Worktree fan-out, diff annotations → agent, VS Code-style file editor, design mode, CLI |
| [Superset](https://superset.sh) | Electron | partial | — | Workspace = worktree + branch + terminals + ports; app + CLI + SDK + MCP |
| Conductor | macOS app | no | proprietary | Minimal-config worktree runner |

## ADE capability checklist (Superset's definition)
1. Environment per task (worktree/sandbox/remote) with setup scripts.
2. Launch and track agent sessions; keep terminal state across restarts.
3. Isolate parallel work.
4. Show when an agent needs a human.
5. Turn output into reviewable work (diffs → git).

RADE covers 1–5 with R12, R10/R11, R12/R15, R3/R10, R13.

## Sources
- https://superset.sh/compare/what-is-an-agent-development-environment
- https://superset.sh/compare/best-agent-development-environment
- https://agentconn.com/blog/orca-ade-agent-fleet-parallel-coding-agents-2026/
