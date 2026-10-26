# Keybindings

RADE's own keys use `Ctrl+Shift` (or `Alt` / `Ctrl+Alt` with a number), so every plain `Ctrl+<key>`
still reaches the program in the terminal — readline, tmux, vim and the agents keep theirs.
Nothing collides with GNOME's, Ghostty's or IBus's defaults. `Shift+Enter` and `Ctrl+V` always go
to the program.

## Everywhere

| Keys | Action | Name in `[keybindings]` |
|---|---|---|
| `Ctrl+Shift+P` | Command palette | `command_palette` |
| `Ctrl+Shift+Space` | Agent Overview | `agent_overview` |
| `Ctrl+Shift+G` | Jump to the newest unread Notification | `jump_unread` |
| `Ctrl+Shift+T` | New Tab | `new_tab` |
| `Ctrl+Shift+W` | Close the Pane | `close_pane` |
| `Ctrl+Shift+Enter` | Split right | `split_right` |
| `Ctrl+Shift+D` | Split down | `split_down` |
| `Ctrl+Shift+H` / `J` / `K` / `L` | Focus left / down / up / right | `focus_left` … |
| `Ctrl+Shift+Z` | Zoom the Pane | `zoom` |
| `Ctrl+Shift+PgUp` / `PgDn` | Previous / next Tab | `prev_tab` / `next_tab` |
| `Ctrl+Shift+B` | New Worktree… | `new_worktree` |
| `Ctrl+Shift+R` | Review Pane | `open_review` |
| `Ctrl+Shift+O` | Browser Pane | `open_browser` |
| `Ctrl+Shift+E` | Show / hide the Inspector | `toggle_file_tree` |
| `Ctrl+Shift+M` | Changes | `show_changes` |
| `Ctrl+Shift+S` | Search | `search_worktree` |
| `Ctrl+Shift+N` | Add Source… | `add_source` |
| `Ctrl+,` | Settings | `open_settings` |
| `Alt+1` … `Alt+9` | Project 1–9 | `source_1` … |
| `Ctrl+Alt+1` … `Ctrl+Alt+9` | Workspace 1–9 | `workspace_1` … |

Only while the focus is in RADE's own interface (not in a terminal or a web page): `Ctrl+K`
palette, `Ctrl+B` Inspector, `Ctrl+G` newest unread.

## In a terminal

| Keys | Action |
|---|---|
| `Ctrl+click` | Open the path (`file:line`) in a Viewer, or a URL in your browser |
| `Ctrl+Shift+click` | Open the path in a new Viewer to the side |
| `Ctrl+Shift+F` | Search the screen and scrollback |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste (an image pastes as a saved PNG's path) |
| `Shift+Enter` | Newline in an agent's prompt |
| right-click (`Shift`+right-click over mouse programs) | Terminal menu |

These come from Ghostty and follow your Ghostty config.

## Viewer

| Keys | Action |
|---|---|
| `F12` / `Ctrl+click` | Definition |
| `Shift+F12` | References |
| `Alt+Shift+H` | Callers |
| `Ctrl+F12` | Implementations |
| `Ctrl+I` | Hover |
| `Alt+F12` | Peek definition |
| `Alt+Shift+O` | Outline |
| `Alt+Left` / `Alt+Right` | Back / Forward |
| `Ctrl++` / `Ctrl+−` / `Ctrl+0` | Zoom in / out / reset |
| `Esc` | Un-zoom, then close |

## Changes (the Inspector's list)

| Keys | Action |
|---|---|
| `Up` / `Down` | Move between rows |
| `Enter` | Open the file's diff (the keyboard stays in the list) |
| `Space` | Stage / unstage the selected files |
| `Delete` | Discard the selected files (asks) |
| `Ctrl+A` | Select every file of the card |
| `Ctrl+click` / `Shift+click` | Add to / extend the selection |
| `Ctrl+Enter` (in the message box) | Commit |
| `Up` on the first line / `Down` on the last | Earlier / later commit messages |

## Diff Pane

| Keys | Action |
|---|---|
| `F7` / `Shift+F7` | Next / previous change |
| `Ctrl+C` | Copy the selected lines |
| `Esc` | Close |

## Review Pane

| Keys | Action |
|---|---|
| `n` / `p` | Next / previous hunk |
| `N` / `P` | Next / previous file |
| `]` / `[` | Next / previous comment |
| `v` | Mark the file viewed |
| `c` | Comment on the selected lines |
| `w` | Hide whitespace changes |
| `i` | Impact |
| `Ctrl+Enter` | Request changes / Approve |

Plus the Viewer's code-navigation keys on diff lines.

## Agent Overview

| Keys | Action |
|---|---|
| type | Filter |
| arrows, `Tab` / `Shift+Tab` | Move; next / previous band |
| `Enter` / `Shift+Enter` | Jump / jump and Zoom |
| `Alt+R` | Review of the selected card's Worktree |
| `Alt+G` | Group by status ⇄ Workspace |
| `Alt+P` | Keyboard previews on / off |
| hold `Ctrl+Shift`, tap `Space` | Flip through cards; release `Ctrl` to jump |
| `Esc` | Clear the filter, else close |

## Browser Pane

| Keys | Action |
|---|---|
| `Ctrl+L` | URL bar |
| `F5` | Reload |
| `Alt+Left` / `Alt+Right` | Back / Forward |
| `Ctrl+Shift+A` | Annotate mode |

## Change a key

```toml
# ~/.config/rade/config.toml
[keybindings]
agent_overview = "<Super>space"
open_browser = ""                 # unbind
```

Keys use GTK's syntax (`<Ctrl><Shift>t`, `<Alt>1`). The file reloads when you save it.
