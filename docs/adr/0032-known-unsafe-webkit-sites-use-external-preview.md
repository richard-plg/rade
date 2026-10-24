# ADR-0032 — Refuse known unsafe YouTube pages in embedded WebKit

Status: accepted (2026-10-08)

The isolated YouTube compatibility recheck again killed the whole RADE process with WebKitGTK 2.52.6. A toolbar fallback cannot contain an in-process native crash. See `docs/research/webkit-compat.md` for baseline evidence.

Refuse `youtube.com`, `youtu.be`, `youtube-nocookie.com` and their subdomains in the embedded preview, using a parsed host and WebKit navigation/response policy decisions as well as the initial-load check. Keep the requested URL available for the visible Chromium/default-browser alternatives. Refuse known unsafe direct CLI URLs immediately with an engine recommendation. Local previews keep their existing behavior. Chromium annotations retain the same URL and an unambiguous Agent in the original Tab/Worktree; existing WebKit annotations stay in RADE.

This narrowly restricts support; it does not isolate WebKit crashes, intercept every possible subresource, or certify arbitrary remote media sites. Unknown sites can still exercise upstream bugs. Embedded preview remains intended for local development and prototypes. Revisit the restriction after a verified upstream fix or out-of-process embedded-browser isolation, with a release compatibility matrix before expanding support.
