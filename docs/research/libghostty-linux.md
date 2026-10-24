# Embedding Ghostty on Linux (GTK4) — findings

Researched 2026-09-30. Facts with sources; **UNVERIFIED** marks what must be proven in M0.

## Upstream status
- Upstream embedded libghostty (`ghostty.h`, `apprt/embedded.zig`) supports macOS/iOS only.
- Linux embedding PR/discussion declined 2026-03-21: focus moved to `libghostty-vt` (pure VT emulation, no renderer); a `libghostty-gtk4` is "under consideration" only. Maintainer advice: keep Linux support in a fork. — https://github.com/ghostty-org/ghostty/discussions/11722
- Proof of concept with correct IME: https://github.com/patcito/testty (single `main.zig`, GtkGLArea, IMContext `filter_keypress`, preedit, focus in/out).

## How the forks do it
Fork chain: `manaflow-ai/ghostty` → `bvolpato/ghostty` (used by limux, MIT) → `richard-plg/ghostty` (RADE's own, branch `rade`, since 2026-10-24). Patch shape (from cmux-linux `docs/ghostty-fork.md`, read only — AGPL):
- `ghostty.h`: `GHOSTTY_PLATFORM_GTK4` + `struct ghostty_platform_gtk4_s { void* gl_area; }`.
- `src/apprt/embedded.zig`: gtk4 platform variant.
- `src/renderer/OpenGL.zig`: `surfaceInit` → `prepareContext(null)` (same path as Ghostty's GTK apprt); `threadEnter/threadExit` no-ops because GtkGLArea owns the GL context.
- Build `libghostty.a` with `-Dapp-runtime=none`; link vendored GLAD + libGL.

Host side:
- `GtkGLArea`, `set_required_version(4,3)`, `set_auto_render(false)`; Ghostty `RENDER` action / wakeup → `queue_render`; `render` signal → `ghostty_surface_draw`.
- Keys: `EventControllerKey` → raw XKB keycode → `ghostty_surface_key`.
- IME: must add `GtkIMMulticontext` → `ghostty_surface_preedit` / committed text (cmux-linux lacks this; testty has it).
- Clipboard: runtime callbacks → GDK clipboard/primary → `ghostty_surface_complete_clipboard_request`.

## Embedded C API we rely on (fork `ghostty.h`)
| Need (PRD) | API |
|---|---|
| per-pane cwd/cmd/env (R1.4, R2.1) | `ghostty_surface_config_s`: `working_directory`, `command`, `env_vars`, `initial_input`, `wait_after_command` |
| link click (R7) | `GHOSTTY_ACTION_OPEN_URL`, `GHOSTTY_ACTION_MOUSE_OVER_LINK`. Verified in M4 (fork a687fd69e): the `link` config key "can't currently be set" (Config.zig TODO); the default matcher (`src/config/url.zig`, on unless `link-url = false`) already matches URLs and paths — rooted (`/`, `./`, `../`, `~/`) or bare relative with a `/` (`src/a.rs:4`), not slash-less names (`main.rs:10`). On Ctrl+click Ghostty resolves a relative match against its OSC 7 pwd and passes the absolute path only if that exists, else the raw text; OSC 8 links pass their URI. Hover with Ctrl underlines the match and sets the pointer shape itself. RADE falls back to the word under the pointer (read from the row) when a Ctrl+click emitted no `OPEN_URL`. |
| notifications (R3) | `GHOSTTY_ACTION_DESKTOP_NOTIFICATION` (OSC 9/777), `GHOSTTY_ACTION_RING_BELL`. Verified in M3: the fork @ a687fd69e has `src/terminal/osc/parsers/kitty_notification.zig` but nothing references it, so OSC 99 is not delivered. Core rate-limits OSC notifications (`Surface.showDesktopNotification`): at most one per second app-wide, identical title+body dropped within 5 s. |
| status fallback (R10) | `GHOSTTY_ACTION_COMMAND_FINISHED` (exit code, duration; needs OSC 133), `SHOW_CHILD_EXITED`, `SET_TITLE`, `PROGRESS_REPORT` |
| cwd tracking (R11) | `GHOSTTY_ACTION_PWD` (OSC 7) |
| `rade read` (R2) | `ghostty_surface_read_text` with a `GHOSTTY_POINT_SCREEN` selection; covers scrollback up to `scrollback-limit` (M0 verified) |
| `rade send` (R2) | `ghostty_surface_text` |
| future daemon (ADR-0005) | `GHOSTTY_SURFACE_IO_MANUAL` + `io_write_cb` + `ghostty_surface_process_output` (fork only) |

## Platform versions (Ubuntu 26.04)
GTK 4.22, libadwaita 1.9, WebKitGTK 6.0 (2.52). Crates: `gtk4` 0.11 (`v4_22`), `libadwaita` 0.9, `webkit6` 0.6 (`v2_52`).

## Open risks → M0 findings (2026-09-30, RADE spike `crates/rade-app`)
1. **GPU paths.** GTK uses EGL on Wayland (no GLX). On the reference machine the monitor is on the RTX 5080, so the default path is already NVIDIA: `GL_RENDERER=NVIDIA GeForce RTX 5080/PCIe/SSE2`, `4.6.0 NVIDIA 595.84`. PRIME offload env (`__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __EGL_VENDOR_LIBRARY_FILENAMES=…/10_nvidia.json`) gives the same renderer. Forcing the Intel iGPU (`__EGL_VENDOR_LIBRARY_FILENAMES=…/50_mesa.json DRI_PRIME=pci-0000_00_02_0`) renders on `Mesa Intel(R) Graphics (ARL)`, then Mutter kills the client: `wl_display.error 7 "failed to import supplied dmabufs: Could not bind the given EGLImage to a CoglTexture2D"`. A plain PyGObject GTK4 `GLArea` fails the same way, so it is a GTK/Mesa/Mutter cross-GPU issue, not Ghostty. `GDK_DISABLE=dmabuf` does not help. Mesa without `DRI_PRIME` falls back to `llvmpipe` and works. **Still open:** Intel iGPU with the monitor on the iGPU output (manual).
2. **IBus Vietnamese.** Verified in limux (same IME code, now ported to `crates/rade-app/src/terminal/ime/`). **Still open:** re-test in rade-app itself.
3. **`read_text` scrollback: works, bounded by `scrollback-limit`.** `GHOSTTY_POINT_SCREEN` TOP_LEFT → BOTTOM_RIGHT returns scrollback + screen. With default config: all 5000 lines of `seq 1 5000`. `scrollback-limit` counts *page memory*, not text bytes: the user's `scrollback-limit = 100000` kept 849 lines, `1000000` kept 898 lines (89 cols), because Ghostty keeps whole pages. The fork's default is 50 MB. RADE must set its own floor for Agent panes (or override per surface) or `rade read --scrollback` silently loses history.
4. **Hidden surfaces stop rendering; idle is cheap.** Hiding the pane (`set_visible(false)` → unmap → `set_auto_render(false)` + `ghostty_surface_set_occlusion(false)`) drops that surface's draws from ~40/s to 0 while its program keeps writing; draws resume on show. Two idle surfaces: 0.6% of one core (includes the 8 ms `ghostty_app_tick` timer). **Still open:** 20 surfaces. Note: a surface is only created on GLArea `realize`, so a Pane hidden before its first map has no process yet. **M1 solution:** call `gtk_widget_realize` on the GLArea once the window is realized (`TerminalHandle::ensure_started`); realizing needs no mapping, so the PTY starts in a never-shown Tab, and the surface is created occluded (0 draws/s, verified with `RADE_LOG_RENDERS=1` under headless Weston). Its grid uses Ghostty's default size until first shown.
5. **Headless E2E (ADR-0006) is viable.** `weston --backend=headless` (pixman) + client `llvmpipe` and `weston --backend=headless --renderer=gl` (NVIDIA EGL) both run the spike and the scrollback test.

## Sources
- https://github.com/ghostty-org/ghostty/discussions/11722
- https://mitchellh.com/writing/libghostty-is-coming
- https://github.com/bradwilson331/cmux-linux (AGPL — reference only)
- https://github.com/am-will/limux (MIT)
- https://github.com/patcito/testty
