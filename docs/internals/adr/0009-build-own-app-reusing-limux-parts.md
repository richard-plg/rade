# Build RADE from scratch, reusing limux's MIT terminal-embedding parts

Forking limux and building fresh cost about the same (~20 vs ~23 person-weeks, see `docs/internals/research/limux-audit.md`), but limux's model inverts ours (a Pane holds Tabs; no Project/Worktree level) and keeps state inside GTK widgets in 5–9k-line files, so it cannot be unit-tested without a display. We build our own app and reuse, with attribution in `THIRD_PARTY.md`: the Ghostty FFI crate (`limux-ghostty-sys`), the terminal / GtkGLArea lifecycle code, and the headless Weston smoke test.

## Consequences

- Ghostty fork pinned: `bvolpato/ghostty` @ `a687fd69e1720d8ea2541aedd17f1c58f44be2c3` (≈8 Linux/GTK patches on top of manaflow-ai/ghostty). Keep the patch list in `docs/internals/research/libghostty-linux.md` so we can re-apply them if that fork stalls. Since 2026-10-24 the submodule points at our own GitHub fork `richard-plg/ghostty` (branch `rade` and tag `rade-base-a687fd69e` at the same commit), so a deleted or archived upstream fork cannot take the pinned commit away; our Ghostty patches go on `rade`.
- Workspace layout: `ghostty/` (submodule), `crates/rade-ghostty-sys`, `crates/rade-core` (no GTK), `crates/rade-app` (GTK4), `crates/rade-cli`.
