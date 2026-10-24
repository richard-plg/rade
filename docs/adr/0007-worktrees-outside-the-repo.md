# Worktrees live under ~/.local/share/rade/worktrees

Status: superseded by ADR-0014 (M15) for Sources in a Workspace other than `default`.

Worktrees created by RADE go to `~/.local/share/rade/worktrees/<project>/<name>`, not inside the repo (`.rade/worktrees`) or next to it. Inside the repo, tools like ripgrep, tsc, and IDE indexers scan every copy; next to it clutters the user's code folder. Cost: long paths, hidden by showing short names in the UI.

Dev builds use `~/.local/share/rade-dev/worktrees` instead (ADR-0010).
