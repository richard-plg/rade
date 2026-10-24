# Native Rust + GTK4 app embedding Ghostty through a libghostty fork

Smooth GPU rendering (PRD R4) is the top requirement, so terminals render with Ghostty's own OpenGL renderer inside a GtkGLArea, in a Rust + GTK4/libadwaita app. Upstream libghostty does not support embedding on Linux (maintainers declined it in ghostty-org/ghostty#11722, 2026-03-21), so we build against a maintained fork that adds a GTK4 platform to the embedded apprt (manaflow-ai → bvolpato chain used by limux), pinned to an exact commit.

## Considered Options

- **Electron/Tauri + xterm.js WebGL** (Orca, Superset): easiest file tree/markdown/diff UI, but measurably worse terminal throughput and scroll smoothness. Rejected for R4.
- **libghostty-vt + own renderer**: officially supported, but we would write a GPU renderer. Rejected: months of work.

## Consequences

- We own a Ghostty fork: pin a commit, keep patches minimal, document them in `docs/research/libghostty-linux.md`, rebase deliberately.
- Rich UI (Markdown, Mermaid) uses WebKitGTK 6 inside Viewer Panes only; never for Terminals. (Extended by ADR-0011: Browser Panes use the same WebKitGTK 6.)
