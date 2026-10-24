# Browser Annotations, and Chromium as an external engine over a CDP pipe

Status: accepted 2026-10-03 (M24, PRD R71, R72). Builds on ADR-0011 (Browser Pane: WebKitGTK, isolated-world automation) and ADR-0006 (the socket is the control surface); revisits ADR-0011's "Chromium/CDP in a separate window" option, as that ADR asked for when Agents need trusted input.

M24's spike (docs/internals/research/webkit-compat.md) opened public pages in a Browser Pane and in Chromium. WebKitGTK 2.52 on Ubuntu 26.04 renders SPAs, SSO login pages, WebGL, IndexedDB and service workers, but has no WebRTC, File System Access, OPFS or WebGPU, and **YouTube crashes its UI process — taking all of RADE down** (MiniBrowser crashes the same way: a WebKitGTK bug, not RADE's). Agents also need trusted input for some pages (ADR-0011 consequence). And the owner wants to point at what is wrong on a page (and later on a design prototype, M25) instead of describing it.

## Decision

1. **Keep WebKitGTK as the in-app Browser Pane; no CEF / embedded Chromium.** Embedding Chromium means shipping and updating a browser, a second GPU process tree in the app and a much larger `.deb`; the Pane covers dev servers and prototypes, which is what it is for.
2. **Chromium is an external engine of `rade browser`** (`--engine chromium`), for verification and annotation: the same commands, arguments, results and refs. RADE finds one (`[browser] chromium` in the user's `config.toml`, Playwright's Chrome for Testing, then `PATH`) and never ships or downloads one.
3. **CDP over `--remote-debugging-pipe`, no port.** Chromium's DevTools endpoint on a TCP port is reachable by every local process (ADR-0011's objection); the pipe is reachable only by its parent. So a small daemon (`rade __chromium`, started by the first `open`, its own process group) owns Chromium and the pipe, and serves the CLI the `browser.*` methods on a same-UID 0600 Unix socket beside RADE's (`rade[-dev]-chromium.sock`) with RADE's own server code. No WebSocket client is needed (no new dependency; `base64`, already in the lock file, decodes screenshots). The daemon also keeps what a one-shot CLI can't: the session (so the isolated world and its refs live between commands), console events, dialogs, annotations.
4. **A throwaway profile per daemon** in the temp dir (0700), deleted when it exits; background networking, sync, component updates and first-run UI off; `--no-sandbox` like Playwright (Ubuntu's AppArmor refuses the user-namespace sandbox to unprofiled binaries). Headless unless `--headed`; a headed Chromium quits with its window, a headless one after 30 idle minutes. The owner's browser profiles are never used.
5. **One page library for both engines.** `browser.js` (snapshot, refs, actions) and the new `annotate.js` live in `rade-core/assets` and run in an isolated world named `rade` in both: WebKit's script world, Chromium's `Page.createIsolatedWorld` + `addScriptToEvaluateOnNewDocument(worldName)`. Snapshots are identical for the same page. Chromium's `click` and `press` use trusted input (`Input.dispatchMouseEvent` at the element's center, `Input.dispatchKeyEvent`); `fill` keeps the native setter.
6. **Annotations come only from RADE's world.** The overlay (a closed shadow root over the page) posts to a WebKit script message handler registered for the `rade` world, or to a CDP binding with `executionContextName: "rade"`; page scripts can't reach either, so a page can't add or fake annotations, nor read the list. It can, however, see the keystrokes typed into the comment box (its window-level key listeners run first) and remove the overlay; annotate on pages you trust with what you type. The host (not the page) crops the screenshot. In the Pane, annotations are kept on the Browser Pane in `state.json` (Session Restore keeps them); the daemon keeps Chromium's in memory.
7. **Send to Agent like Review Comments**, and only to an Agent Pane (an explicit `--to` / `--pane` that is a plain Terminal is refused: the prompt quotes page text a shell would run) (R13.2): one prompt, pasted with Enter, to the Agent in the Browser's Tab (else the Worktree's only one; choose; spawn), shared code with the Review Pane. Chromium's go to the Agent whose Pane opened it, else `--to`.

## Considered Options

- **CEF or a WebView2-like Chromium embed in the Pane**: rejected above (size, updates, a second engine in the app process tree).
- **CDP on `--remote-debugging-port` + a small WebSocket client (or `tungstenite`)**: an open local port any process can drive, and a dependency. The pipe needs neither.
- **No daemon: each CLI call attaches anew**: a new CDP session gets a new isolated world (refs gone), and console messages between calls are lost.
- **Playwright as a dependency** (Node): a second runtime for RADE's own use; Agents can still run Playwright themselves, and the docs recommend Playwright tests as `[task] verify`.
- **Comment box in GTK instead of in the page**: the headed Chromium has no GTK; one overlay serves both and the owner types where they point.
- **Persist Chromium's annotations to disk**: its profile and page are throwaway; send them or they go with it.

## Consequences

- Same-UID processes are still not a boundary (ADR-0021): an Agent can run Chromium itself, or edit the user's `config.toml` to point `[browser] chromium` at another binary. The key is app-wide, so a Workspace or Project file (more often Agent-edited) can't set it.
- `--no-sandbox` means a page exploit in that Chromium runs with the owner's rights, as in Playwright's default; use it on your dev servers and pages you'd open anyway.
- The WebKitGTK crash on YouTube is upstream; Agents should use `--engine chromium` for media-heavy sites (the `rade` skill says so). A Browser Pane is not isolated from the app process tree, so such a crash still ends RADE.
- Chromium needs a display only with `--headed` (Wayland when `WAYLAND_DISPLAY` is set). CI installs Chrome for Testing (`ci/Dockerfile`) and runs the headless part; without one the E2E records the skip.
- Annotations in the Pane live with the Pane; closing it drops them.
