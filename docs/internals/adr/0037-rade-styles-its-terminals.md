# RADE styles its Terminals over the user's Ghostty config

Status: accepted 2026-10-24 (owner decisions after reviewing the 1.0 build; PRD R102.6, R102-i, R112). Amends ADR-0036 §6 and replaces R20.8 as the default.

## Context

Since M14 (R20.8) RADE used its own Terminal colors only when the user's Ghostty config chose none. The owner's config sets `theme = Catppuccin Mocha`, `background-opacity = 0.97` and 8 × 4 padding, so on their machine every Terminal ignored the approved design (spec A.51, A.52, `m-term`): other colors, a second padding inside the Pane's own margin and Ghostty's 12 pt font. Goldens mask terminal contents and the headless suites run without a Ghostty config, so no check saw it.

## Decision

- By default (`[terminal] theme = "rade"`, `config.toml` only) RADE loads its look **after** the user's Ghostty files, so it wins: theme files generated from `tokens.toml` (`term-0`…`term-15`, `term-fg` on `bg0`, `acc` cursor, `term-sel` selection; light / dark follow the style live), `background-opacity = 1`, Ghostty padding 0 (the Pane's 10 × 12 px margin is the padding), every font key of the user's Ghostty config reset (`font-family*`, `font-style`, `font-feature`), then the `[terminal]` font of the Settings page (PRD R112): by default **Maple Mono Medium at 14 px** (`font-size` = px × 0.75), line height 1.15 (`adjust-cell-height` = (line height − 1) × 100 %), ligatures on.
- Maple Mono (OFL-1.1) ships in the `rade-fonts` GResource with Inter and JetBrains Mono. Ghostty builds its own fontconfig config (`FcInitLoadConfigAndFonts`), which never sees fonts GTK added, so at start RADE writes the font files and a `fonts.conf` (it includes the system's, or the user's `FONTCONFIG_FILE`, and adds the folder) to `$XDG_RUNTIME_DIR/rade/fonts/` and sets `FONTCONFIG_FILE` for itself; children inherit it and see the same fonts plus RADE's. Nothing is installed. A family fontconfig lacks is answered with its best match (on the owner's machine `JetBrains Mono` → Noto Sans, a proportional font in a cell grid), so Settings lists only families GTK sees.
- Size is in px like VS Code; Ghostty's own `font-size` (12 pt on Linux by default) is not taken, since a chosen size cannot be told from the default. The owner chose Maple Mono Medium after comparing Geist Mono, Maple Mono and Monaspace Neon, Regular and Medium (`docs/design/redesign-2026-10-23/font-compare.png`); 12.5 px Regular read thin and small on their screen.
- Everything else (keybinds, scrollback, cursor style, shell integration) stays the user's Ghostty config.
- `[terminal] theme = "ghostty"` restores the 0.7 behavior (R20.8): the Ghostty config's look, RADE's colors only when it has no theme.
- The 16 ANSI colors are design tokens with contrast pairs on `bg0` (≥ 4.5 : 1 except the background-like 0 in dark and 7 / 15 in light).

- **Code outside Terminals** (R112.4, R112.5): the Viewer, Review and Markdown use the `[code_font]` (empty = the Terminal's) and the token colors `syn-*` taken from GitHub Dark / Light Default (MIT), the owner's VS Code theme, on RADE's `bg0`: GtkSourceView schemes `rade-dark` / `rade-light` and the CSS of Markdown code blocks are generated from `tokens.toml`. The Viewer's header is a Pane header; Markdown follows the visual system (Inter, hairlines, `acc` links) with its variables set from the tokens. WebKit's sandbox gets the bundled fonts folder (`add_path_to_sandbox`).
- **File icons** (R8.5): Catppuccin's VS Code icons (MIT, 16 px outlines that sit with Phosphor) replace PKief's Material Icon Theme, drawn in one ink per theme from the tokens (files `fg2`, folders `fg3`, an open folder `acc`) by build.rs; the palette's file rows use them too. Chosen by the owner over Catppuccin in color, Symbols and Phosphor (`docs/design/redesign-2026-10-23/icon-compare.png`).

## Consequences

- A user's Ghostty theme no longer shows in RADE unless they opt out; this is listed in the CHANGELOG.
- A user's explicit `palette = …` / `background = …` keys in the Ghostty file still override the theme file (Ghostty applies config keys over a theme); RADE does not strip them.
- Ghostty explicit-colour overrides aside, the Terminal now matches the mockup in both themes; `target/design-run/term-look.sh` shot it headless with the owner's Ghostty visual keys.
