# Terminal titles → Tab labels (R1.8)

Owner request during M12 (2026-10-01): Tab labels and Pane captions follow the terminal title (OSC 0/2) the program in the Pane sets, instead of a fixed "Tab N". The owner left the design decisions to the implementer. They are recorded here with their trade-offs.

## What the programs actually send (measured 2026-10-01)

Each program was run in a PTY, and every OSC 0/1/2 it wrote was logged (`scratchpad/titles.py`). Afterwards the same titles were checked in a dev-flavor RADE.

| Program | Titles | Notes |
|---|---|---|
| Claude Code 2.1.286 | `✳ Claude Code` while idle. During a turn: `◐ <session title>` / `◑ …` (U+25D0–25D3 half circles, about 1/s). Back to `✳ <session title>` when the turn ends, e.g. `✳ Just the word hi`, `✳ PONG reply`. Sends an empty OSC 0 at start (before the trust prompt) and on exit | Uses OSC 0. The session title replaces "Claude Code" after the first prompt |
| Codex CLI 0.159 (`--no-daemon -c check_for_update_on_startup=false`) | `RADE` (the cwd basename). At startup it shows `⠋ RADE`, `⠙ RADE` … (braille spinner, about 10/s), then `RADE`. Empty OSC 0 on quit | Codex was started and quit, with no prompt (quota). It shows nothing before its trust prompt |
| agy (Antigravity CLI) | none, idle or after a prompt | Its Tab stays `Tab N` unless a shell set a title before it |
| zsh (owner's config) | OSC 2 `richard@richard:/tmp` at the prompt, the command line while running (`eza --icons …`, `sleep 1`), plus OSC 1 (icon name) | OSC 1 is not a title and Ghostty does not report it |
| bash in RADE (Ghostty shell integration) | the cwd (`~`) at the prompt | Seen in E2E: a plain shell Tab is labelled `~` |

## Decisions

| # | Decision | Why / trade-off |
|---|---|---|
| 1 | **Sanitize** (`rade-core::title::sanitize`). Strip leading characters that are whitespace, control characters, `·` `•` `*`, U+2190–U+2BFF (arrows, technical symbols, geometric shapes such as Claude's ◐◑, dingbats such as ✳✢✶✻✽), U+2800–U+28FF (braille spinners, which fall inside that range) and emoji (U+1F000–U+1FAFF). Collapse runs of whitespace and control characters to one space. Keep at most 256 characters. An empty result is ignored, so the previous title stays | This uses character ranges rather than a list of specific glyphs, so new spinner frames from other Agents are covered too. Only *leading* characters are stripped, so titles such as `~/x`, `/tmp/a → b` and `user@host:~` keep their symbols. Ignoring empty titles keeps the label stable when Claude and Codex send `""` at start and exit. A title that really starts with an arrow or emoji loses that character, which is acceptable |
| 2 | **Shown length:** 60 characters with a trailing `…` (no space before it) in Tab labels and captions. The tooltip and `rade list --json` carry the whole sanitized title | This is what the owner asked for. AdwTabBar also ellipsizes narrow tabs on its own |
| 3 | **Throttle:** at most one model update per Pane every 250 ms (≤ 4/s), trailing edge. The newest title always lands (`title::delay` + `app/titles.rs`). A title that sanitizes to the current one causes no update at all | Spinner frames (Codex about 10/s, Claude about 1/s) sanitize to the same text, so they cause no GTK work. A burst of distinct titles ends on the last one (E2E: `step 40`). Each update is a normal `app.update` (sync + debounced save) |
| 4 | **Which title a Tab follows:** the Agent Grid Parent's, else the focused Pane's, else the first Pane in the Tab that has a title, else `Tab N` | This adds one step to the owner's rule ("else the focused Pane"): if the focused Pane has no title (a Viewer, Review or Browser opened beside an Agent), the label keeps showing the Agent's title instead of falling back to `Tab N` |
| 5 | **Rename:** new `Tab.renamed: bool`, persisted. A double-click on the tab bar renames the selected Tab (the first click of the double-click selects it). Palette action "Rename Tab…" (`rename_tab`) does the same. Both open a popover with an entry, pointing at the selected tab. Enter applies, Esc cancels. Entering an **empty name** clears the custom name: the label follows the title again and `name` goes back to `Tab <position>` | AdwTabBar has no in-place label editing, so a popover anchored on the tab is the closest thing to inline editing. A Fan-out Tab is created with `renamed = true`, so it keeps its `fanout-<n>` name. A Fan-out Tab saved before this change has `renamed = false` and follows its first Agent's title, which is acceptable |
| 6 | **No `rade tab rename` CLI command** | It was optional. Agents have no need to rename Tabs, and adding it would change API.md and the CLI while M12 work is going on in the same files. E2E renames through `debug.tabs {"op":"rename"}`, which takes the palette path: it opens the real entry, types the name and presses Enter |
| 7 | **Persistence:** the custom name is persisted (`renamed` + `name`). The **last title is persisted too** (`Pane.title` in state.json) | After a restore, the Tab first shows the program's last title and not `Tab N`. Agents and shells set a new title within a second (shell integration, Claude, Codex), and that replaces it. The downside is that a Pane whose new program never sets a title (agy) keeps a stale title until something sets one |
| 8 | **Captions / Title Bars:** `● name · preset · <i>title</i> · status · unread` | The title in italics separates it from the name and the Preset. Plain Terminal Panes have no caption, so their title appears only in the Tab label. The sidebar is unchanged |
| 9 | **Unread badge:** kept as a suffix on the label: `<label> (n)` | Behaviour as before. The tab tooltip is the full title on its first line, followed by the Agent Status summary |
| 10 | **Not changed:** Notification places (`Project / Worktree / Tab N / pane`) still use the Tab's `name` | They locate where the Notification came from, so they should not change while an Agent's title animates |

## Checks

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | PASS |
| Lint | `cargo clippy --workspace --all-targets` | PASS, 0 warnings |
| Unit tests | `cargo test --workspace` | PASS, 300. New in `rade-core/src/title.rs` (6 tests): measured glyphs stripped (✳ ◐ ⠋ ✢ ✶ ✻ ✽ · ⠹ ◑ • * 🤖), inner symbols kept, whitespace collapsed, empty ignored, 256 cap; ellipsize; throttle delay (0 / 250 / 150 / 0 ms); label: title → rename sticks → empty name returns to the title; Grid Parent > focused > any titled Pane > `Tab N`. Plus the `layout_view` caption test with an escaped title |
| E2E titles | `tests/e2e/titles.sh` | PASS, 21 / 0. `printf '\e]2;✳ Refactor auth\a'` → label `Refactor auth`, `rade list --json` title, tooltip. 160 braille frames → title `Build docs`; a long title → label ≤ 60 ending in `…`, while list keeps it whole. 40 titles at 20/s → ends at `step 40`. Rename → `auth work`, which stays after a new title (shown in the tooltip). Grid: the Parent's `Lead plan` labels the Tab while the child `Kid task` is focused. Relaunch: `auth work` restored, `renamed: true` and the title in state.json, empty rename → follows the restored shell's new title `~`. Screenshots `target/e2e-titles/titles.png`, `relaunch.png` (inspected) |
| E2E regressions | `tests/e2e/run.sh`, `m2.sh`, `m8.sh`, `m9.sh` | PASS: 44 / 85 / 93 / 79. On its first run, m8 failed once at "…and shown" (switching to a new Worktree). It passed on the re-run, and that code is unrelated to this change |
| LIVE | `tests/live/run.sh titles` (`getfacl /dev/uinput` → `user:richard:rw-`) | PASS, 7 / 0. Real Claude Code (`~/.claude-work`) "reply with exactly: PONG" → title `PONG reply` (✳ stripped), Tab label `PONG reply` within seconds. A real double-click on the tab (ydotool, pointer aimed in a closed loop) opened the rename entry; typing `live name` + Enter → label `live name`; a second Claude turn left it unchanged. Screenshots `target/live/titles-*.png`. (`debug screenshot` does not capture the popover itself; `rename_open` proves it) |

Known visual detail: in a headless screenshot the renamed tab sometimes keeps a hover highlight after the rename popover closes, because headless Weston has no pointer to move. On the real desktop, after the double-click and typing, the selected tab looked as usual.
