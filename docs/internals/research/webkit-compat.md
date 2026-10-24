# WebKitGTK compatibility of the Browser Pane on real-world web apps (M24 spike)

Researched 2026-10-03 on the reference machine (Ubuntu 26.04, WebKitGTK 2.52.6-0ubuntu0.26.04.1, GStreamer 1.28 with base/good/ugly/libav, **no** plugins-bad), against Chromium 151 (Playwright's Chrome for Testing, headless) through `rade browser --engine chromium`. No internal URLs were given, so public pages stand in: video (YouTube, hls.js), WebRTC samples, WebGL / WebGPU, SSO login pages (Google, Microsoft, GitHub — opened, nobody logged in), File System Access (Chrome Labs text editor), PWAs with IndexedDB + service workers (Excalidraw, Squoosh, tldraw), React's site.

Reproduce: `tests/research/webkit-compat.sh` (a fresh dev RADE per site in headless Weston, temp dirs) and `tests/research/chromium-compat.sh` (no app needed); both run `tests/research/compat-probe.js` and save console + screenshot under `target/research-*`.

## Results

| Feature / page | WebKitGTK Browser Pane | Chromium (headless) |
|---|---|---|
| **YouTube watch page** | **crash of the UI process (SIGSEGV in libwebkitgtk) — all of RADE dies**; same in WebKit's own MiniBrowser, headless Weston and the real GNOME session, with or without the web-process sandbox, DMA-BUF renderer or compositing; the web process dies first (`WebLoaderStrategy::internallyFailedLoadTimerFired`) | loads, video ready (MSE) |
| hls.js (MSE) | works: MSE, a `blob:` video at readyState 4 | works |
| Native HLS (`application/vnd.apple.mpegurl`) | no | "maybe" |
| Codecs | H.264, VP9 yes (warning: `vp9parse` missing); **AV1 no** | H.264, VP9, AV1 |
| WebRTC (`RTCPeerConnection`) | **undefined** (WebKitSettings `enable-webrtc` is off by default) | yes |
| getUserMedia | rejected `OverconstrainedError` (no camera; RADE answers no permission request) | rejected `NotFoundError` (no camera) |
| WebGL / WebGL2 | yes (software in headless) | yes |
| WebGPU (`navigator.gpu`) | **absent** | present, no adapter headless |
| File System Access (`showOpenFilePicker`) | **absent** | yes |
| OPFS (`navigator.storage.getDirectory`) | **absent** | yes |
| IndexedDB | works | works |
| Service workers | register and control (Excalidraw, Squoosh, text editor) | same |
| SSO pages (Google, Microsoft, GitHub) | login pages render (no authentication attempted) | same |
| SPAs (React site, Excalidraw, tldraw canvas, Squoosh) | render and run | same |
| `SharedArrayBuffer` | only where the page is cross-origin isolated: none here | Squoosh only (COOP/COEP) |
| User agent | Safari 60.5-like `AppleWebKit/605.1.15` | `HeadlessChrome/151` (sites may treat it as a bot) |

## What it means

The feature matrix uses `compat-probe.js`, page text and screenshots. A positive API-presence result is not an end-to-end feature test: authentication was not attempted; there was no real camera/microphone; file chooser permission/dialog behavior was not exercised; and WebGPU had no headless adapter. Codec strings indicate browser capability responses; only recorded video readiness establishes that a sampled media element loaded. Login-page rendering does not establish that a provider permits embedded sign-in. Service-worker registration and IndexedDB checks are scoped to the sampled origin/session.

- The local Browser regression fixtures and sampled prototype/SPA/PWA pages rendered and their automated checks passed. This establishes behavior for those fixtures and sampled pages, not compatibility with arbitrary sites.
- It is **not** for media-heavy sites: YouTube takes the whole app down, an upstream WebKitGTK bug RADE can't catch in-process. Nor for WebRTC, File System Access, OPFS, WebGPU or AV1.
- So M24 builds both halves with the Chromium engine treated as a priority, not an extra: `rade browser --engine chromium` supplies a separate engine (ADR-0026), and the `rade` skill tells Agents to use it for unsupported embedded pages. Its sampled YouTube page loaded; the probe exposed APIs missing from WebKit. Annotations are tested in both engines. API presence does not establish usable hardware, permissions, dialogs or every application flow.
- Not done here (open): enabling WebKit's WebRTC / media-stream settings with a permission prompt; installing `gstreamer1.0-plugins-bad` (fixes `vp9parse` / `fakevideosink` warnings, untested against the crash); reporting the crash upstream with a symbolized trace; running Browser Panes out of process so a WebKit crash can't end RADE.

## 2026-10-08 isolated recheck

Ran `NO_BUILD=1 KEEP=1 tests/research/webkit-compat.sh youtube` against the existing release binary, with a private D-Bus/headless Weston and throwaway state (`/tmp/rade-e2e.SGRcNN`). The installed library remains WebKitGTK **2.52.6-0ubuntu0.26.04.1** (`pkg-config`: 2.52.6). The YouTube watch page again killed the RADE app; the harness printed `CRASHED: rade-app died`. `target/research-webkit/app.log` records the same WebKit internal error at `WebLoaderStrategy::internallyFailedLoadTimerFired`. This recheck did not collect a new symbolized UI-process backtrace, so the precise current signal is not claimed.

Comparison: `tests/research/chromium-compat.sh ~/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome youtube` loaded the same page in **HeadlessChrome/151.0.0.0**, with a video at readyState 4 and no media error. Evidence: `target/research-chromium/youtube.probe.json`, console and screenshot. No account was used; neither run touched the user's real RADE session. The rest of the matrix and the real GNOME desktop were **not rerun**.

The new preview toolbar identifies WebKit and offers a separate Chromium window or the default browser. Chromium keeps the same URL and links annotation delivery to the sole Agent in the Browser's Tab (else the sole Worktree Agent); existing embedded annotations remain in RADE. These alternatives do **not** isolate or repair the WebKit UI-process crash. Media-heavy remote sites remain outside the supported embedded-preview scope; out-of-process isolation and a full release compatibility matrix remain open.

After this baseline recheck, RADE adds a narrow safeguard for `youtube.com`, `youtu.be`, `youtube-nocookie.com` and their subdomains: direct embedded loads, navigation decisions and final response policy decisions are refused, with the requested URL retained for the alternative-browser buttons. The CLI returns an immediate recommendation for known unsupported direct URLs. This host restriction is **not crash containment** for unknown pages or arbitrary remote media, and the original crash was reproduced before the guard. Postguard integration results must be recorded separately.

## 2026-10-08 final release-binary matrix

Ran the integrated local release binaries after the safeguard and UI changes, serially in isolated headless sessions:

```sh
NO_BUILD=1 KEEP=1 tests/research/webkit-compat.sh hlsjs webrtc webgl webgpu github_login fsa excalidraw react
tests/research/chromium-compat.sh ~/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome youtube hlsjs webrtc webgl webgpu github_login fsa excalidraw react
```

All **8 WebKit page opens/probes** and **9 Chromium page opens/probes** completed without engine/app death or harness open/probe errors. This count does not mean every feature worked: the WebGPU sample reports an unsupported runtime in both headless engines. Versions: WebKitGTK **2.52.6-0ubuntu0.26.04.1**, Chrome for Testing **151.0.7922.34**, GStreamer **1.28.2** (`libgstreamer1.0-0` 1.28.2-1; base 1.28.2-1ubuntu0.1, good 1.28.2-2ubuntu0.4, ugly/libav 1.28.2-1); plugins-bad remains uninstalled.

| Sample | Embedded WebKit | Headless Chromium |
|---|---|---|
| YouTube | Not loaded in this matrix. M24's direct CLI/link/new-window/HTTP-302 checks verify refusal and that RADE stays alive. The preguard crash remains recorded above. | Watch page rendered; paused video readyState 4, no media error. |
| hls.js | Page rendered; paused blob video readyState 4, no media error. | Same sampled readiness result. |
| WebRTC/getUserMedia | Sample page rendered, RTCPeerConnection absent; camera probe denied OverconstrainedError. | RTCPeerConnection present; camera probe denied NotFoundError. |
| WebGL | Page rendered; WebGL and WebGL2 contexts created. | Same context-creation results. |
| WebGPU | Sample shell rendered and explicitly reports navigator.gpu unavailable; no triangle. | API present, requestAdapter returns null; sample explicitly cannot run, no triangle. |
| GitHub login | Sign-in form rendered; no credentials entered. | Same rendering-only check. |
| Chrome Labs Text Editor | Rendered in Legacy Mode; File System Access/OPFS APIs absent; service worker controls the page. | Editor rendered, File System Access/OPFS APIs present; service worker controls the page. No file chooser or OPFS operation exercised. |
| Excalidraw | Toolbar/canvas rendered; service worker controls the page. | Same rendering/worker checks. No drawing/collaboration flow exercised. |
| React | Site rendered. | Site rendered. |

IndexedDB create/write transactions succeeded at all sampled origins. WebKit reported H.264/VP9 capability and no AV1 capability; Chromium reported all three. These are codec capability strings, not playback tests for each codec. The headless samples do not establish authentication, physical camera/microphone operation, real GPU acceleration, file-dialog access, audio playback or a complete end-user application flow. Google/Microsoft SSO, Squoosh and tldraw were not rerun in this final matrix; their earlier rows retain the 2026-10-03 scope.

Evidence: `target/research-webkit/` and `target/research-chromium/` hold per-site open/probe JSON, console output and screenshots. Logs: `/tmp/rade-final-webkit-matrix.log` and `/tmp/rade-final-chromium-matrix.log`. Visually inspected WebKit Excalidraw and WebGPU plus Chromium WebGPU screenshots. Earlier evidence, including the unguarded WebKit crash log and initial Chromium YouTube check, is preserved in `target/research-2026-10-08-before-final-matrix/`. The corrected M24 suite passed **118 checks**, including the HTTP-302 fixture self-check, engine recommendation and app-alive regression; headed Chromium remained skipped because headless Weston has no seat.

The release restriction remains narrow: embedded preview is intended for local development/prototypes, known unsafe YouTube hosts are refused, and Chromium is a separate alternative. This matrix does not provide crash isolation for unknown remote pages.
