# v1 deliberately excludes editing, git hosting, and sandboxed packaging

Status: accepted; the commit / push / PR exclusion is superseded by ADR-0013 (2026-10-02).

- **No editing**: Viewer is read-only; Agents edit files, humans use `$EDITOR`. Avoids conflicts with Agents writing the same file.
- **No commit/push/PR/GitHub/Linear**: Agents and the CLI do this; no auth integration in the app. *Superseded for commit / push / pull / PR by [ADR-0013](0013-human-confirmed-source-control.md) (M14): the app runs git / gh on the owner's click, Agents only draft the text. Linear and other trackers stay out.*
- **No Flatpak/Snap**: sandboxing blocks spawning arbitrary Agent CLIs and PTYs. Ship `.deb` + `cargo install`.

Revisit any of these only with a new ADR.
