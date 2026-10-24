# Third-party code

## limux — MIT

Source: https://github.com/am-will/limux @ `ae67aae`.
Used in: `crates/rade-ghostty-sys/` (from `rust/limux-ghostty-sys`), `crates/rade-app/build.rs`,
`crates/rade-app/src/terminal/` (from `rust/limux-host-linux/src/{terminal.rs, terminal/clipboard.rs, ime.rs, ime/*, main.rs}`;
M1 split `terminal.rs` into `terminal/{mod,callbacks,create,input}.rs` and added RADE changes),
`crates/rade-core/src/server.rs` (`peer_uid`: the `SO_PEERCRED` lookup follows `rust/limux-control/src/auth.rs`).
M4: the `GHOSTTY_ACTION_OPEN_URL` handler in `terminal/callbacks.rs` follows limux's (`terminal.rs`, reading `url`/`len` and dispatching on idle);
`terminal/links.rs` (Ctrl+click fallback) is RADE's own.
M5: the drop target and clipboard read in `terminal/clipboard.rs` start from limux's (`dropped_file_text`, `clipboard_has_text`); the image/uri-list choice, paste-file saving and quoting moved to RADE's own `crates/rade-core/src/paste.rs`.
M12: the Browser Pane (`crates/rade-app/src/browser_view.rs`) follows the shape of limux's WebKitGTK browser tab (`rust/limux-host-linux/src/pane.rs`: header with Back/Forward/Reload + URL entry over a WebView, localhost-first URL input, Web Inspector); its code and the automation library are RADE's own.
M13: the Terminal search bar (`crates/rade-app/src/terminal/search.rs`) follows limux's (`terminal.rs`: a search entry over the surface driving Ghostty's `search:` / `navigate_search:` / `end_search` binding actions); the count, Shift+Enter and the `START_SEARCH` / `SEARCH_TOTAL` / `SEARCH_SELECTED` handling are RADE's own.
Each ported file carries a header naming its origin.

```
MIT License

Copyright (c) 2026 Limux contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Ghostty — MIT

Source: https://github.com/ghostty-org/ghostty, used via the `ghostty/` submodule
(fork `richard-plg/ghostty`, branch `rade`, @ `a687fd69e` — a fork of `bvolpato/ghostty`), linked as `libghostty-internal.so`; its vendored GLAD is compiled into `rade-app`.
The .deb (M10, `scripts/package-deb.sh`) ships that library in `/usr/lib/rade` and Ghostty's built `shell-integration/`, `themes/`
and `terminfo` (`xterm-ghostty`) from `ghostty/zig-out/share` under `/usr/share/rade`.

```
MIT License

Copyright (c) 2024 Mitchell Hashimoto, Ghostty contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Mermaid — MIT

Source: https://github.com/mermaid-js/mermaid, npm `mermaid@12.0.0` `dist/mermaid.min.js`, vendored unmodified as
`crates/rade-app/assets/mermaid.min.js` (embedded in `rade-app`, injected into Markdown Viewers as a user script so
diagrams render offline). Its licence is `crates/rade-app/assets/mermaid.LICENSE` (MIT, Copyright (c) 2014 - 2022
Knut Sveidqvist); the bundle carries the notices of its own MIT/BSD/ISC dependencies (DOMPurify, d3, lodash, …) inline.

## Inter — OFL-1.1

Source: https://github.com/rsms/inter release v4.1 (`Inter-4.1.zip`, `extras/ttf/Inter-{Regular,Medium,SemiBold}.ttf`), unmodified.
Used in: `crates/rade-app/assets/fonts/` — embedded in `rade-app` as the `rade-fonts` GResource and loaded at startup (UI font, PRD R102.3,
ADR-0036 §4). Copyright (c) 2016 The Inter Project Authors. SIL Open Font License 1.1; full text: `crates/rade-app/assets/fonts/Inter-OFL.txt`.

## JetBrains Mono — OFL-1.1

Source: https://github.com/JetBrains/JetBrainsMono release v2.304 (`fonts/variable/JetBrainsMono[wght].ttf`), unmodified, renamed
`JetBrainsMono-Variable.ttf`. Used in: `crates/rade-app/assets/fonts/` (the `rade-fonts` GResource; metadata font, PRD R102.3).
Copyright 2020 The JetBrains Mono Project Authors. SIL Open Font License 1.1; full text: `crates/rade-app/assets/fonts/JetBrainsMono-OFL.txt`.

## Maple Mono — OFL-1.1

Source: https://github.com/subframe7536/maple-font release v7.9 (`MapleMono-TTF.zip`, sha256 `3a35f8f0…71d75` as published), unmodified:
`MapleMono-{Regular,Medium,Bold,Italic,MediumItalic,BoldItalic}.ttf`. Used in: `crates/rade-app/assets/fonts/` (the `rade-fonts`
GResource; the Terminal font, ADR-0037). Copyright 2022 The Maple Mono Project Authors. SIL Open Font License 1.1; full text:
`crates/rade-app/assets/fonts/MapleMono-OFL.txt`.

## Phosphor Icons — MIT

Source: https://github.com/phosphor-icons/core, npm `@phosphor-icons/core@2.1.1` (`assets/regular/<name>.svg`, `assets/fill/<name>-fill.svg`,
`assets/bold/<name>-bold.svg`), unmodified, renamed `ph-<name>-symbolic.svg` / `ph-<name>-fill-symbolic.svg` / `ph-<name>-bold-symbolic.svg`
(`robot` in Bold for agents without a logo: PRD R115, ADR-0038). Used in: `crates/rade-app/assets/icons/` (the `rade-icons`
GResource; PRD R102.4, ADR-0036 §4) — the icons of `docs/internals/design/spec/README.md` "Icons".

```
MIT License

Copyright (c) 2023 Phosphor Icons

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Libraries linked, not vendored

Markdown rendering uses the `comrak` (BSD-2-Clause) and `syntect` (MIT) crates; the file tree uses `ignore`
(MIT/Unlicense). WebKitGTK 6.0 and GtkSourceView 5 (LGPL-2.1+) are system libraries linked dynamically through
the `webkit6` and `sourceview5` crates (MIT).

## Catppuccin VS Code icons — MIT

Source: https://github.com/catppuccin/vscode-icons release v1.26.0 (`icons/mocha/*.svg`, `src/defaults/fileIcons.ts` and `folderIcons.ts`).
Used in: `crates/rade-app/assets/file-icons/` — the 619 SVGs those maps name, every color replaced by `currentColor` (build.rs paints them per theme from `tokens.toml`), and `map.json` derived from the maps by `vendor.py`; embedded in `rade-app` as GResources (file tree and palette file icons, R8.5, ADR-0037). Licence text: `crates/rade-app/assets/file-icons/LICENSE`.

```
MIT License

Copyright (c) 2023 Catppuccin
Copyright (c) 2023 thang-nm

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## GitHub VS Code theme — MIT

Source: https://github.com/primer/github-vscode-theme 6.3.5 (`themes/dark-default.json`, `light-default.json`). Used in: `crates/rade-app/src/tokens.toml` — the `syn-*` code token colors (one adjusted for contrast), for the Viewer, Review and Markdown code (ADR-0037). Copyright (c) 2020 Primer, MIT License (same text as above with that copyright line).

## Lobe Icons — MIT (agent logos)

Source: https://github.com/lobehub/lobe-icons, npm `@lobehub/icons-static-svg@1.98.0` (`icons/{claude,codex,antigravity,opencode}.svg`),
paths unchanged, wrapped as `crates/rade-app/assets/icons/agent-<name>-symbolic.svg` with `fill="currentColor"` (PRD R115, ADR-0038
amendment 2).

```
MIT License

Copyright (c) 2023 LobeHub

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The logos are trademarks of their owners: Claude and Claude Code of Anthropic PBC, Codex of OpenAI, Antigravity of Google LLC, OpenCode
of its authors. RADE is not affiliated with or endorsed by them; the marks only name the agent a Pane runs, and RADE recolours them to show
the agent's status (the owner's decision, ADR-0038).
