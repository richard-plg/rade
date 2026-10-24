# Agent glyphs replace the status LED

Status: accepted 2026-10-24 (owner decisions on `docs/plans/viewers-html-pdf-agent-marks.md` Q3 and the glyph sheet; PRD R115). Amends ADR-0036 §4 (Phosphor Regular only) and §5 ("LED 7 px square, status drawn once").

## Context

Every Agent showed the same 7 px LED square on Navigator rows, Tab labels and in the Inbox; the `+` menu showed a lettered badge (`C` / `X`). With several agents side by side the owner could not tell a Claude Code Pane from a Codex one without reading names. The vendors' logos would say it at a glance, but Anthropic's and OpenAI's brand guidelines forbid changing their colours and need approval to use them at all, and Google's Antigravity mark needs a brand request; Lobe Icons (MIT) and Simple Icons (CC0) license the drawings, not the trademarks. RADE's status colours are the point of the mark, so a tinted vendor logo is not an option.

## Decision

- **Option C, A now.** RADE draws its own glyph per agent and colours it by the Agent Status. Official marks (option B) may come later behind `[ui] agent_marks = "official"` once written permission exists; none ship now.
- **Glyphs** are Phosphor **Bold** (MIT, `@phosphor-icons/core@2.1.1`), chosen by the owner from the sheet `docs/internals/design/redesign-2026-10-23/r115-glyphs.png` over Regular: at 14 px Regular strokes are too thin to carry the status colour. Claude Code `asterisk`, Codex `terminal` (`>_`), Antigravity `rocket-launch`, OpenCode `brackets-curly`, any other Preset `robot`. A Pane's glyph comes from its Preset's name, else the program its command or recorded invocation runs (`design::AgentMark`, `Pane::mark`), so `claude-sonnet` or a custom Preset running `claude` gets Claude's glyph. None is a vendor's logo.
- **Size 14 px**, symbolic, recoloured by CSS (`.agent-glyph`, `glyph-<status>`): running `run` pulsing (the LED's 1.6 s opacity cycle), waiting `wait` with a 6 px glow (`-gtk-icon-shadow`), done `ok`, error `err`, interrupted `wt`, idle / exited `fg3`. Colour stays paired with text where it carries meaning (`needs input`, R102).
- **Where:** Navigator Agent rows, Tab labels (the lead Agent: the Agent Grid's lead, else the first Agent), Pane headers before the name, the Inbox pill and its rows (a live Agent Pane's glyph in the event's colour), the system monitor's BY PANE rows, and the `+` menu's account rows (no status: `fg2`, `acc` when selected) in place of the lettered badge. The Usage page keeps its badges.
- **The LED stays** where it shows a status, not an agent: the Statusline counts, a collapsed parent's worst status in the Navigator, Activity and Project home rows, and a shell's row in the system monitor.

## Amendment (2026-10-24, after the owner tried it)

- **Hand-started agents** (PRD R115.4): Codex fires `SessionStart` only at its first prompt and agy has no hook, so a `codex` or `agy` typed in a shell looked like a shell. The app now reads each Terminal's foreground process every 3 s (`process::foreground_argv`: the shell's `tpgid` → that process's argv) and matches it against the Presets (`preset::detect`, the hook's rule; Presets started through an interpreter never match). The match lives in `Pane.detected`, never saved, so Session Restore keeps restoring only what hooks or spawns made Agents.
- **The user's own icon** (PRD R115.5): `[agents.<name>] icon` draws a file the user supplies in its own colours, with the status as a 5 px LED at its lower right. This is option B without RADE shipping or recolouring anyone's mark: the file and its use are the user's.

## Amendment 2 (2026-10-24): the agents' own logos

- **Decision (owner):** after trying RADE's glyphs the owner chose to ship the agents' official logos, recoloured by status, as the default: Claude (the Claude spark) for Claude Code, Codex, Antigravity, OpenCode; `robot` stays for any other Preset. Option A's Phosphor glyphs are dropped (`asterisk`, `terminal`, `rocket-launch`, `brackets-curly`).
- **Source:** the single-colour SVGs of Lobe Icons (`@lobehub/icons-static-svg@1.98.0`, MIT), paths unchanged, wrapped as `agent-<name>-symbolic.svg` (`fill="currentColor"`) so GTK recolours them like the glyphs. Preview: `docs/internals/design/redesign-2026-10-23/r115-logos.png`.
- **Risk, accepted by the owner:** the MIT licence covers the drawings, not the trademarks. Anthropic's and OpenAI's brand guidelines forbid changing their marks' colours and ask for approval of use; Google asks for a brand request. Shipping recoloured marks in the `.deb` and the public repository goes against those guidelines. `THIRD_PARTY.md` names the marks' owners and says RADE is not affiliated. If a vendor objects, the fallback is this ADR's option A (the Phosphor glyphs, recoverable from git) or a Preset's `icon`.

## Consequences

- Spec A rows A.29–A.31, A.43 and A.79 and spec F row F.7 describe the glyph; `spec_a.py` / `spec_f.py` check it; goldens A–D (m37) and E / F (m39) are re-approved after the owner sees them.
- Five more icons in the `rade-icons` GResource, listed in the spec README "Icons" (marked Bold) and `THIRD_PARTY.md`.
- A new agent in RADE needs a glyph decision; until then it shows `robot`.
