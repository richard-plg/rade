# A terminal-native, dark-first visual system from one token file

Status: accepted 2026-10-23 (M33, PRD R102, R103). Supersedes the visual direction of PRD R20 (M14, recorded in R20 and docs/internals/checks/M14.md; no ADR). Owner decision 4 in docs/internals/design/redesign-2026-10-23.md §9; design §4–§6, build plan §3.

RADE still looks like GNOME Settings wrapped around a terminal: white chrome, 99 px pills, terminals flush with the sidebar, one status repeated in six or seven places, a warning icon on "No changes", a Toast covering `Comment / Send to Agent`. Colors are written by hand across `style.css` and `style-dark.css`, and `contrast.py` checks a hand-kept copy of them, so the two drift. The owner approved a terminal-native direction (Ghostty, Zed, Warp, Linear dark) with the mockup as reference.

## Decision

1. **Dark-first, terminal-native.** Mono for metadata (branches, paths, counts, ports, keys, times), uppercase letter-spaced section labels, hairlines instead of shadows on chrome, square LEDs for status, one cold accent (mint-cyan) used for focus, selection and the one primary button per screen. No scanlines, CRT effects or decorative gradients. Still GTK4 / libadwaita, restyled with CSS and a few custom widgets (flat tabs, text tabs for the Inspector, link table, popover Inbox).
2. **Tokens** (dark default / light):

   | Token | Dark | Light |
   |---|---|---|
   | bg0 terminal / bg1 chrome / bg2 raised / bg3 hover | `#07090b` `#0c0e11` `#12151a` `#191d23` | `#fcfcfb` `#f3f4f5` `#ffffff` `#eceef0` |
   | line / line2 | white 7 % / 12 % | `rgb(16 22 30)` 8 % / 14 % |
   | fg / fg2 / fg3 | `#e6e9ed` `#9aa3ae` `#5e6773` | `#14181d` `#525c67` `#89919b` |
   | accent | `#4fe3c8` | `#0b8f7b` |
   | waiting / running / done / error | `#f5b84b` `#6cb6ff` `#7bd88f` `#ff6b6b` | `#a8690b` `#2a6fc0` `#25844a` `#cf3d3d` |
   | worktree | `#c4a7ff` | `#7a55d6` |

3. **One source of truth.** `crates/rade-app/src/tokens.toml` (dark + light) is the only place a color is written; `build.rs` generates the GTK CSS (`@define-color`) and the mockup's `tokens.css`. No raw hex color outside token files: a CI grep fails on `#[0-9a-fA-F]{3,8}` in `style*.css` and Rust UI code. `docs/internals/design/contrast.py` reads `tokens.toml`; every text/background and status/background pair passes WCAG AA in both themes.
4. **Type and icons are bundled.** Inter (UI, 12.5–13 px, 400/500/600) and JetBrains Mono (metadata, 11–12 px, `tnum`), both OFL, ship in the .deb and are listed in `THIRD_PARTY.md`. Icons are Phosphor (MIT) Regular, plus Fill for states, renamed `*-symbolic.svg` and added to the existing GResource (`assets/icons/`); a unit test checks every icon name used in code exists in the GResource.
5. **Status is drawn once.** The LED (7 px square; running pulses slowly, waiting glows) is CSS, not an icon, on the Agent row; text only for `needs input`; color always paired with shape or text. The **Attention Ring is retired**: the waiting state shows as the glowing LED plus `needs input` in one place (design §5), the Inbox becomes a popover and a Toast appears only when the waiting Pane is hidden and never covers actions.
6. **Shape and motion.** Radii 4–6 px (dialogs 10 px), no pills; chips are a hairline border with mono text; only dialogs and the palette cast a shadow; 120 ms ease-out, no sliding panels. Terminal theme matches `bg0`; light keeps its own palette (M14).
7. **Design is checked by machine.** Spec sheets `docs/internals/design/spec/*.md` give each screen's measurements; the hidden `debug.ui {"op":"geometry","widget":…}` (test mode, ADR-0021) returns allocation, margins, font and computed colors so E2E checks them. Golden screenshots of screens A–F (mockup data, light + dark) are a CI gate; the first images are approved by a reviewer agent comparing them with the mockup side by side (`target/design-diff/index.html`), then compared by pixel with a small threshold, terminal and clock areas masked.

## Spike (M35)

Before building the chrome, M35 renders titlebar, Navigator, tabs and statusline with the real tokens in GTK and records here what GTK CSS can and cannot do:

- `box-shadow` glow for the focused Pane border and the waiting LED.
- No `color-mix()` in GTK CSS assumed: hover / selected / translucent variants become precomputed tokens in `tokens.toml`.
- Font feature `tnum` (`font-feature-settings`) on JetBrains Mono / Inter for aligned numbers.
- Phosphor icons at 16 px, scale 1× and 2×: crisp, recolored as symbolic.
- Where GTK differs from the mockup, the difference and why.

Results (M35 spike, 2026-10-23: GTK 4.22.4, libadwaita 1.9.1, Pango 1.57.0, headless Weston at scale 1 and 2, a throwaway window with the real dark tokens, titlebar, a tab, a focused Pane and a statusline; fontconfig limited to DejaVu):

| Probe | Result | Consequence |
|---|---|---|
| `@define-color` names with hyphens (`acc-line`, `acc-fg`) | Parsed, no CSS error, used in `border` / `box-shadow` | Token names stay as in the spec README; `tokens.css` is `@define-color` only |
| `color-mix(in srgb, @acc 50%, transparent)` | Supported by GTK 4.22 (renders the 50 % mix) | Still not used: mixed colors stay precomputed `-dim` / `-line` / `-bar` tokens (the mockup's `tokens.css`, `contrast.py` and older GTK builds read the same values) |
| CSS custom properties (`--x`, `var(--x)`) | Supported | Not needed for colors; style rules use `@token` |
| `box-shadow` glow (`0 0 0 1px @acc-line, 0 0 18px @glow`) and the LED glow (`0 0 6px @wait`) | Drawn as in the mockup; the 10 % outer halo is faint, as in Chrome. A GtkPaned clips its children's outset shadows at its own edges, so a split Pane's 1 px ring vanished on three sides (seen with `geometry … drawn`, M35 fix round 1): the focused Pane's ring is its 1 px `acc-line` border, the halo stays a shadow and shows where the Paned does not clip | `glow` and `shadow` keep their geometry in `style.css`; their `tokens.toml` value is the color part (`glow` = the outer halo color, transparent in light) |
| `font-feature-settings: "tnum"` | Applied by Pango: Inter `1111` / `8888` = 22 / 33 px without, 34 / 34 px with; JetBrains Mono digits are tabular anyway | Mono text keeps `tnum` (spec), counts in Inter get it too |
| Bundled fonts without system copies | `pango::FontMap::add_font_file` (Pango ≥ 1.56) on files works; Pango cannot read fonts from a GResource, so the app writes the bundled bytes to a cache file first | Inter is bundled as the **static** Regular / Medium / SemiBold TTFs: Inter 4's variable file is named family `Inter Variable`, and the spec / R102-d require family `Inter`. JetBrains Mono's variable file is family `JetBrains Mono` and is bundled as is |
| Phosphor SVG (`fill="currentColor"`, 256 grid) as `*-symbolic.svg` at 16 px, scale 1× and 2× | Crisp at both scales, recolored by CSS `color` (e.g. `wt` on `git-branch`), Fill icons too | Icons go in the existing `rade-icons` GResource under `/rade/icons/scalable/actions` as `ph-<name>[-fill]-symbolic.svg` |
| Font sizes read back through Pango | Reported in pt (13 px → 9.75 pt) | `debug.ui geometry` converts to px (× 4 / 3) |

Where GTK differs from the mockup: none of the probed effects; the differences are the spec's own (AA-adjusted token values, flat Workspace mark, precomputed mixes, decision log).

## Considered Options

- **Keep the M14 light-first libadwaita look and polish it**: the owner rejected it after seeing both; it cannot show the Navigator density the model needs. Rejected (owner decision 4).
- **Hand-written colors in two CSS files** (today): drifts from `contrast.py` and the mockup. Rejected for one token file.
- **Lucide or Tabler icons**: stroke-based, need outlining before GTK can recolor them as symbolic. Rejected for Phosphor's filled paths.
- **System fonts only**: metadata alignment and the mockup's look depend on a mono with `tnum`; system fonts vary per machine. Rejected for bundling.
- **Human approval of the first goldens**: the owner is away for the unattended run. Rejected for a reviewer agent (build plan §7.2).

## Consequences

- Every widget's color and size is testable; changing a token regenerates CSS for app and mockup alike.
- The .deb grows by the bundled fonts and icons (a few MB); `THIRD_PARTY.md` lists them.
- Golden screenshots make unintended UI changes fail CI; intended ones re-approve the image in the same commit.
- R20's ACs stay proven by docs/internals/checks/M14.md for 0.7.x; 1.0 is checked against R102 / R103.
