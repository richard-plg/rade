# UI/UX trends for agentic terminals and ADEs (2026)

Researched 2026-10-02 for the redesign proposal (`v0.7.1:docs/design/redesign-proposal.md`).
Sources are product docs, changelogs and READMEs, read through web search and page fetches. Facts that came only from search summaries, or that could not be checked, are marked UNVERIFIED. No screenshots of other products are stored here because of licensing; the links point to them instead.

## 1. What the agentic terminals and ADEs do now

### Agent status and attention
The most consistent pattern across products is a status per Agent that you can see at a glance, plus one strong signal on the thing that needs you.
- **cmux:**
  - A pane that needs attention gets a **blue ring** and its tab lights up.
  - The sidebar row of each workspace shows the branch, PR status, cwd, listening ports and the latest notification text.
  - A notification moves through *received → unread (badge) → read (when viewed) → cleared*. Desktop alerts are suppressed when the window is focused and the workspace is active.
  - ⌘⇧U jumps to the latest unread notification.
  - Sources: https://github.com/manaflow-ai/cmux, https://cmux.com/docs/notifications. Screenshot: `docs/assets/notification-rings.png` in that repo.
- **Warp 2.0 ("Agentic Development Environment"):** an Agent Management UI lists every running agent. In-app and system notifications fire when an agent finishes or needs help. https://www.warp.dev/blog/reimagining-coding-agentic-development-environment
- **Zed:**
  - A Threads sidebar groups threads by project.
  - Parallel agents can run in separate worktrees.
  - `agent.notify_when_agent_waiting` and `agent.play_sound_when_agent_done` control alerts.
  - https://zed.dev/docs/ai/agent-panel.html
- **Claude Code desktop (Code tab):** the session sidebar can be filtered by status, project and environment, and grouped by project. Each session gets one worktree. Terminal, preview, diff and chat panes can be dragged into a grid. https://claude.com/blog/claude-code-desktop-redesign, https://code.claude.com/docs/en/desktop
- **Cursor 3.0** (2026-04-02):
  - An **Agents Window** lists every local and cloud agent.
  - **Agent Tabs** show agents side by side or in a grid. https://cursor.com/changelog/3-0
  - A tiled layout followed in 3.1 (UNVERIFIED).
- **Conductor, Superset and Orca:**
  - Each runs one workspace per task, with its own worktree, branch, terminals and diff.
  - Superset adds port management. Orca adds a card status per worktree.
  - https://www.conductor.build/docs/core/parallel-agents, https://superset.sh/, https://github.com/stablyai/orca
  - The exact badge colors and shapes are UNVERIFIED.

**Consensus for 2026:**
- Status per Agent lives **in the navigation sidebar**, not only on the pane.
- One loud signal (a ring, a pulse or a badge) marks only the Agent that **needs you**.
- An inbox is paired with a "jump to next unread" key.
- Desktop alerts are suppressed when you are already looking.

### Review loop
- **Zed:** "Review Changes" opens a multibuffer of every edited file, where each hunk can be accepted or rejected.
- **Codex app:** changes can be staged, unstaged or reverted per diff, file or hunk. Inline line comments go back to the agent as follow-ups. https://learn.chatgpt.com/docs/code-review?surface=app
- **Orca:** inline diff comments are sent back to the agent.

Comment-on-diff → Agent is now table stakes. RADE already has it (R13 Review Comments). The difference between products is in presentation: Review is a **full-width tab or multibuffer**, never a narrow split.

### Blocks and output structure
In Warp, a command and its output form one Block. A failed Block gets a red left bar and background, the selected Block gets a thicker border, and the command header stays sticky while its output scrolls. https://docs.warp.dev/terminal/blocks/block-basics

RADE embeds Ghostty, so it cannot restyle what is inside the terminal. The lesson is to put **semantic color at the edges** (captions, rails, rings) and leave the terminal content alone.

### Compact chrome
- **Ghostty 1.2:**
  - `gtk-titlebar-style=tabs` merges tabs into the titlebar.
  - The tab overview uses `AdwTabButton`.
  - A bell shows as an emoji in the tab title.
  - https://www.omgubuntu.co.uk/2025/10/ghostty-1-2-new-features-for-linux
- **Ghostty 1.3** (2026-03-09):
  - Native scrollbars.
  - A scrollback search overlay.
  - Spatial split navigation.
  - No flicker on split actions.
  - https://ghostty.org/docs/install/release-notes/1-3-0
- **Ghostty config:** splits are separated by **dimming the unfocused one** (`unfocused-split-opacity`, default 0.8) rather than by heavy borders. `split-divider-color` sets the divider. https://ghostty.org/docs/config/reference
- **Linear's redesign** aimed to reduce visual noise, tighten alignment and increase density in the sidebar, tabs and headers. It moved theming to LCH, generated from three inputs (base, accent, contrast). https://linear.app/now/how-we-redesigned-the-linear-ui

### Command palette first
- Every product in this survey has a global palette. Several also offer a "universal input" (Warp's single prompt-or-command box).
- The palette doubles as a **shortcut teacher**: each row shows its key binding.
- RADE's palette (R14) already shows key bindings. What is missing is an always-visible palette entry point and hints.

## 2. GNOME and libadwaita (what RADE's users expect)

Sources: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/, https://nyaa.place/blog/libadwaita-1-9, https://nyaa.place/blog/libadwaita-1-6, https://thisweek.gnome.org/posts/2025/09/twig-218/

### Widgets by version
- **1.6:**
  - The system **accent color** (blue `#3584e4`, teal `#2190a4`, green `#3a944a`, yellow `#c88800`, orange `#ed5b00`, red `#e62d42`, pink `#d56199`, purple `#9141ac`, slate `#6f8396`).
  - `AdwSpinner`, `AdwBottomSheet`, `AdwMultiLayoutView`.
- **1.7:** `AdwToggleGroup` and `AdwInlineViewSwitcher`, a segmented control that suits the Files ⇄ Changes switch.
- **1.8:** `AdwShortcutsDialog`.
- **1.9:**
  - **`AdwSidebar`, `AdwSidebarSection` and `AdwSidebarItem`**: sections, suffix widgets per row, context menus, a drop target and a built-in search filter. It is **ListBox-based**, so it suits Projects and Worktrees but not a file tree.
  - Reduced-motion support.
  - `style-dark.css` is deprecated; use one stylesheet with `@media (prefers-color-scheme)`.
  - `AdwTabView` has sections for pinned tabs.

### CSS
GTK CSS supports `var()`, `color-mix()` and `oklch()`. The libadwaita variables to build on:
- `--accent-bg-color`, `--accent-color`
- `--window-bg-color`, `--view-bg-color`
- `--sidebar-bg-color`, `--secondary-sidebar-bg-color`
- `--headerbar-bg-color`, `--popover-bg-color`
- `--success-color`, `--warning-color`, `--error-color`
- `--border-opacity` (15 %), `--dim-opacity` (55 %), `--window-radius`

### Style classes
- `.navigation-sidebar`: rounded, padded rows with a **neutral (not accent) selection**.
- `.boxed-list`, `.card`, `.toolbar`, `.osd`.
- `.caption`, `.heading`, `.dimmed`, `.numeric` (tabular figures), `.monospace`.

### Fonts
- GNOME 48 introduced **Adwaita Sans** (a customised Inter) and **Adwaita Mono** (an Iosevka build). https://www.omgubuntu.co.uk/2025/02/gnome-introduces-new-ui-monospace-adwaita-fonts
- Ubuntu's UI font is **Ubuntu Sans**, which is what the owner's machine has.
- GTK apps inherit the system font. An app should not force its own UI font.

### Ptyxis, GNOME's default terminal (Fedora, RHEL, Ubuntu)
https://gitlab.gnome.org/chergert/ptyxis, https://itsfoss.com/ptyxis-terminal-features/
- A plain headerbar with tabs in an `AdwTabBar`.
- A searchable **tab overview with live previews**.
- The palette follows light and dark mode, and "Window Dressing" can tint the whole window from the palette.
- The **header bar turns red under sudo or root**: contextual color is used sparingly and only when it means something.

### What GNOME users expect
- The app follows the system light or dark preference **and the accent color**.
- Navigation selection is neutral.
- Headerbar buttons are flat and symbolic.
- No translucency or blur: GTK on Wayland has no backdrop blur. This is our inference, not an explicit HIG rule.
- Reduced motion is respected.
- The terminal content follows the light or dark scheme as well, as Ptyxis does.

## 3. Typography, color and density in dev tools (2026)

### Type
- UI fonts in use:
  - Inter, which GNOME (Adwaita Sans) and Linear both use.
  - IBM Plex Sans (Zed).
  - Geist (Vercel; OFL, on Google Fonts).
- Monospace fonts in use:
  - JetBrains Mono, the free default.
  - Geist Mono, Lilex (Zed), Iosevka / Adwaita Mono.
  - Berkeley Mono, the paid "premium" choice.
  - Source for these trends: https://madegooddesigns.com/best-monospace-fonts-2026/ (lower trust).
- Use **tabular figures** (`.numeric`, `font-variant-numeric: tabular-nums`) for counts, timers and diff stats.

### Color
- Design tokens in OKLCH or LCH (Linear). GTK supports `oklch()` natively.
- **WCAG 2.2 AA is still the bar.** APCA has been dropped from the WCAG 3 draft. https://adrianroselli.com/2026/04/wcag3-contrast-as-of-april-2026.html
- Pair status color with a **shape** (✓ ! × spinner), so status never relies on color alone.

### Density
- Pro tools have moved to roughly 24–28 px rows and roughly 26–30 px pane headers, with 11–12 px secondary text.
- GNOME's default rows are taller (about 40–46 px). A "compact" variant is acceptable when it stays within the HIG's spacing rhythm (multiples of 6).

## 4. Anti-patterns to avoid
- **Hard-coding accent colors.** RADE's status palette and `#26a269` git colors are hard-coded today; they should derive from libadwaita variables where a variable exists.
- **Accent-colored selection** in a sidebar that also carries status color. The selection then competes with "needs you".
- **Loud badges everywhere.** RADE's orange count pills appear on 5 sidebar rows at once, so none of them stands out.
- **A review UI in a narrow split.** Every surveyed product gives diffs full width.
- **A dark-only terminal inside a light app**, unless the user chose it.
- **Glassmorphism or translucency** on GTK/Linux, and **all-monospace UIs**. A design-intelligence suggestion of "JetBrains Mono everywhere" was rejected because it reads as costume, not tool, in a GNOME app.
- **Motion that ignores reduced motion.** libadwaita 1.9 makes this a first-class setting.
- **Desktop-notifying a pane the user is already looking at.** RADE's `smart` mode already avoids this.

## 5. Implications for RADE (summary)
1. Put Agents **into the sidebar** under their Worktree, each with a status glyph and a timer. This is the one change that most closes the gap with cmux, Zed and Claude desktop.
2. Reserve one loud signal for `waiting`: an amber ring on the pane plus a sidebar emphasis. Every other status stays quiet.
3. Use neutral selection, system accent and font, and libadwaita variables. The terminal palette follows the light or dark scheme.
4. Tab titles are names, never raw paths. Pane captions show name · agent · status pill · diff stat.
5. Review opens full width (as a Tab, or zoomed), not as a third column.
6. Make the palette and key bindings discoverable from the chrome: a palette entry point plus shortcut hints in a slim status bar.
