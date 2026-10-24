# Code navigation for reading and reviewing: go to definition, references, call hierarchy

Researched 2026-10-05, during M26/M27 (v0.5.1). Question: what VS Code-like code tracing RADE needs to read code in the Viewer and review Agent-written diffs in the Review Pane, and how to build it. Two directions: **where is it defined?** (definition, outgoing calls) and **who uses it?** (references, incoming calls). Facts come from primary sources linked under Sources. Host checks were read-only. Anything I could not confirm is marked **UNVERIFIED**. Nothing was installed. Nothing here is decided until an ADR records it.

---

## 1. Summary

- **The question that matters most for review is "who else calls this changed function?"** Go to Definition helps you read. Incoming calls and references let you judge the *impact* of a diff. An "Impact" list (each changed function with its callers) is the feature no general editor gives a reviewer. RADE can compute it the same way for the owner, the Review Pane and Reviewer Agents.
- **Use an LSP client inside rade-app.** It talks to the real language servers (rust-analyzer, typescript-language-server, pyright/ty, gopls), with one server per (Worktree, language), started lazily and stopped when idle. LSP 3.16/3.17 standardizes every method we need: `definition`, `references`, `implementation`, `typeDefinition`, `prepareCallHierarchy` + `incomingCalls`/`outgoingCalls`, `hover`, `documentSymbol`, `workspace/symbol`, `documentHighlight` ([lsp]). Zed, Helix, Neovim and Claude Code all work this way ([zed-lang], [helix], [nvim], [cc-ci]).
- **Fallback is text search, not tree-sitter.** When no server runs, "References" becomes whole-word matches from the existing Search engine (`rade-core/src/search.rs`), labelled as text matches. GitHub's own navigation is name-based ("finds entities with a given name", [gh-nav]). GitHub archived stack-graphs on 2025-09-09 ([stack-graphs]). tree-sitter tags would add a C build and a grammar per language for a small gain over `rg -w`. Defer it.
- **SCIP/LSIF indexes are for servers like Sourcegraph, not for a local app.** A local LSP server gives the same precision with a warm index and no upload step ([sg-nav], [scip]).
- **Agents get the same thing through the socket:** `rade code def|refs|callers|callees|symbols|hover|impact`. Claude Code already has an `LSP` tool, but only once a code-intelligence plugin and the server binary are installed, and only in terminal sessions ([cc-tools], [cc-ci]). Codex has no built-in LSP. Only third-party MCP bridges and a feature request exist (**UNVERIFIED**, [codex-lsp]). With `rade code`, every Agent, Reviewer and Judge shares the owner's warm servers, and RADE can put a deterministic **impact list into Reviewer and Judge prompts**.
- **Two findings about the owner's stack:**
  1. `globex-api` pins **TypeScript 3.7.2**. typescript-language-server turns on call hierarchy only when tsserver is 3.8 or newer (`typescriptVersion.version?.gte(API.v380)`, [tls-src]). With the workspace's own tsserver, that repo gets no call hierarchy. RADE must point `tsserver.path` at a newer TypeScript ([tls]).
  2. `globex-agents` is on **TypeScript 7** (the native Go port, LSP-based, [ts7]). There, the TS 7 server itself is the right server.
- **Size:** about 4–5 weeks for MVP, Review integration and extras, plus a 2–3 day spike to measure memory and startup on the real repositories. The main risks are server installation, memory across many Worktrees (**UNVERIFIED** numbers), and the index going stale while Agents edit files.

---

## 2. Features, ranked by value for reviewing Agent-written changes

Value scale: ★★★ = needed to review a diff well; ★★ = speeds up reading; ★ = nice to have. "Where" = the Viewer (V), the Review Pane (R), Agents (A).

| # | Feature | VS Code / JetBrains / others | LSP method | Value | Where | Notes |
|---|---|---|---|---|---|---|
| 1 | **Incoming calls** ("who calls this?") | VS Code Call Hierarchy; IntelliJ Ctrl+Alt+H callers/callees toggle ([jb-hier]) | `prepareCallHierarchy` + `callHierarchy/incomingCalls` (3.16) | ★★★ | V R A | The core of review impact. A tree you can expand level by level. |
| 2 | **Find all references** | Shift+F12 ([vsc-keys]); GitHub "References" tab ([gh-nav]) | `textDocument/references` | ★★★ | V R A | Covers what call hierarchy misses: types, constants, fields, re-exports. |
| 3 | **Go to definition** (+ Ctrl+click) | F12, Ctrl+hover preview ([vsc-nav]) | `textDocument/definition` | ★★★ | V R A | Reading what a changed line calls. |
| 4 | **Diff impact**: changed symbols → their callers | none built in. This is the RADE-specific part. | `documentSymbol` ∩ hunk ranges, then #1/#2 | ★★★ | R A | "This diff changes 3 functions; 11 call sites in 6 files, 2 of them in tests." |
| 5 | **Back / forward history** | Ctrl+Alt+− / Ctrl+Shift+− ([vsc-keys]) | none (client-side) | ★★★ | V R | The Viewer already has a Back list (`viewer_navigate`, `MAX_BACK`). Add Forward. |
| 6 | **Outgoing calls** ("what does this call?") | Call Hierarchy callee mode | `callHierarchy/outgoingCalls` | ★★ | V R A | Useful for top-down reading of a new function. |
| 7 | **Hover** (signature, type, docs) | hover, Ctrl+K Ctrl+I | `textDocument/hover` | ★★ | V R | GtkSourceView 5.18 has `GtkSourceHoverProvider` (local GIR). The `sourceview5` 0.11 crate can subclass it (`subclass/hover_provider.rs`). |
| 8 | **Go to implementation** | Ctrl+F12 | `textDocument/implementation` | ★★ | V A | NestJS interfaces/abstract classes, Rust traits. |
| 9 | **Document outline / symbols** | Ctrl+Shift+O, breadcrumbs ([vsc-nav]) | `textDocument/documentSymbol` | ★★ | V | Also needed internally for #4. |
| 10 | **Workspace symbol search** | Ctrl+T | `workspace/symbol` | ★★ | V A | A command palette category. |
| 11 | **Highlight occurrences** | automatic | `textDocument/documentHighlight` | ★ | V | Cheap once hover works. |
| 12 | Go to type definition | palette only in VS Code | `textDocument/typeDefinition` (3.6) | ★ | V A | |
| 13 | Type hierarchy | IntelliJ Ctrl+H | `typeHierarchy/*` (3.17) | ★ | V | typescript-language-server has **no** type hierarchy provider ([tls-src]). rust-analyzer and ty do ([ty]). |
| 14 | Peek (inline definition/references) | Alt+F12 | client-side | ★ | V | A popover with a read-only GtkSourceView is enough. A real inline zone is costly in GTK. |
| — | Diagnostics | Problems panel | `publishDiagnostics` | out | — | Agents and verify runs (R63) already check builds. Showing squiggles over a read-only Viewer while Agents edit adds noise. |
| — | Rename / code actions | F2 | `rename` | out | — | The Viewer is read-only (ADR-0008). Agents edit. |

---

## 3. Approaches

| Approach | Accuracy | Setup | Languages | Speed / memory | Offline | Fit for RADE |
|---|---|---|---|---|---|---|
| **LSP client in rade-app** (spawn real servers) | Compiler-precise: generics, re-exports, trait impls, NestJS DI types | Server binaries on PATH or in `node_modules/.bin`. Needs config per language. | Any language with a server. Owner's stack: TS/JS, Rust, Python, Go. | Indexing at first start (seconds to minutes). Hundreds of MB to GB per server (**UNVERIFIED**, measure in the spike). | yes | **Recommended.** Matches Zed, Helix, Neovim and Claude Code. |
| **Text search** (`search.rs`, whole word, `ignore` walk) | Name-based. False positives on common names. No call/def distinction. | none (already built) | all | ms over 50k files (R22 timings) | yes | **Fallback** when no server runs or for removed diff lines. Labelled "text matches". |
| tree-sitter tags (`tags.scm`, [ts-tags]) | Name-based, but knows definition vs call | `tree-sitter` 0.27 + one grammar crate per language (C build) | ~24 at GitHub ([gh-nav]) | fast, small | yes | Small gain over text search. Defer. Note: GitHub's navigation is this ([gh-nav]). |
| stack-graphs | Precise, no build | crate 0.14.1 (2024-12) | few (TS/JS, Python, Java) | — | yes | **Archived 2025-09-09** ([stack-graphs]). Rejected. |
| SCIP / LSIF indexes ([scip]) | Precise | Run an indexer per commit (`scip-typescript`, `rust-analyzer scip`, …) and store the index | 10 indexers | batch job per change, small reader | yes | Built for code-search servers ([sg-nav]). Stale as soon as an Agent edits. Rejected for local use. Maybe later for "impact across repos". |
| universal-ctags | Name-based definitions only | system package (not installed here) | many | fast | yes | No references. Rejected. |
| **Hybrid: LSP + text fallback** | best available, labelled | as LSP | all | as LSP | yes | **The plan.** Sourcegraph does the same: precise when indexed, search-based otherwise ([sg-nav]). |

**Rust client libraries** (crates.io API, 2026-10-05):

| Crate | Version / updated | Notes |
|---|---|---|
| `lsp-types` | 0.97.0 / 2024-06-04 | Types only. Widely used, but no release in over two years. Forks exist: Helix's, `ls-types` (3.17 + proposed 3.18, [ls-types]). |
| `async-lsp` | 0.2.4 / 2026-04-24 | Client and server, built on tower ([async-lsp]). Needs an async runtime. RADE has none: GTK main loop + threads. |
| `lsp-server` | 0.10.0 / 2026-07-16 | rust-analyzer's synchronous transport (crossbeam). It is written for servers, but the framing is symmetric. |
| `tower-lsp(-server)` | 0.20 (2023) / 0.23 (2026-09) | Server-side only. Not useful here. |

**Recommendation:** write a small synchronous client in `rade-core` with `serde_json::Value` and a few hand-written structs. It needs Content-Length framing, request ids, a reader thread per server, and about 12 methods. That is a few hundred lines, testable against a fake server script, with no async runtime and no stale types crate. Take `lsp-types` only if hand-written structs start to grow.

---

## 4. Recommended architecture

### 4.1 Where the code goes

```
rade-core/src/code/            GTK-free, unit-tested
  lsp.rs        framing (Content-Length), Client {spawn, request(timeout), notify}, reader thread,
                initialize/initialized/shutdown/exit, position encoding (UTF-16 ↔ char columns)
  servers.rs    registry: key (worktree root, language) → Client; root detection (nearest
                Cargo.toml / tsconfig.json / package.json / pyproject.toml, else the Worktree);
                lazy start, idle stop, LRU cap; didOpen/didChange of files RADE asked about
  nav.rs        definition/references/implementation → Vec<Location>; call hierarchy trees;
                text-search fallback (search.rs with whole word); result → rows for the panel/CLI
  impact.rs     diff hunks (diff.rs) ∩ documentSymbol ranges → changed symbols → incoming calls
                / references, deduped, test files flagged; render for prompts (≤ N lines)
rade-app/src/code.rs           `code.*` socket methods (off the GTK thread, like pane.wait)
rade-app/src/code_view.rs      Right Panel "Code" page (tree), popovers, Viewer/Review hooks
rade-cli/src/code.rs           `rade code …`
```

The server processes belong to **rade-app**, like the Chromium daemon and verify runs. The CLI and Agents reach them through the socket (ADR-0006), so the owner and every Agent share one warm index per Worktree. Server processes run with null stdin, in their own session, under the Workspace `[env]` (R23.3), the same as `verify.rs` `run_logged`. Each is killed on idle stop, on Worktree removal and on app quit.

### 4.2 Lifecycle and limits

- Start on the first navigation request for a (root, language). Never at app start, never for every Worktree. Show "rust-analyzer indexing…" in the Status Bar while `$/progress` reports work.
- `[code] idle_minutes = 15`: shut down after no requests. `[code] max_servers = 4`: the least recently used one is stopped first. A request against a stopped server restarts it. On a cold start, return text matches at once, marked "indexing — text matches", and offer Retry.
- One server per **Worktree** root, never shared across Worktrees: they are different checkouts on disk. Helix's `roots` and Neovim's `root_markers` do the same per-root activation ([helix], [nvim]).
- **Keeping the index fresh while Agents edit:** Agents write files constantly. Prefer servers that watch files themselves (rust-analyzer `files.watcher = "server"`, **UNVERIFIED** for typescript-language-server). Otherwise RADE sends `workspace/didChangeWatchedFiles` from a gio FileMonitor on the Worktree. The spike must verify this with an Agent editing while references are requested.
- **Position encoding:** the LSP default is UTF-16. 3.17 lets the client offer `utf-8` (`general.positionEncodings`, [lsp]). GtkTextIter offsets are characters, so one tested conversion lives in `lsp.rs`.

### 4.3 Config: `[code]` in the layered config (ADR-0017)

```toml
[code]                         # user config.toml / workspace.toml / project.toml; .rade.toml only if trusted
enabled = true
idle_minutes = 15
max_servers = 4

[code.server.typescript]       # built-in default shown; extensions .ts .tsx .js .jsx .mjs .cts
command = ["typescript-language-server", "--stdio"]
init = { tsserver = { path = "~/.local/share/rade/typescript/node_modules/typescript/lib" } }  # e.g. for TS 3.7 repos

[code.server.rust]
command = ["rust-analyzer"]
settings = { "rust-analyzer" = { files = { watcher = "server" } } }

[code.server.python]
command = ["pyright-langserver", "--stdio"]   # or ["ty", "server"] / ["basedpyright-langserver", "--stdio"]
```

- Add `code` to the layered allow-list (user → workspace → project → trusted `.rade.toml`). `rade config explain code.server.typescript` shows which layer won, the same as other keys.
- Resolve the command in the Worktree's `node_modules/.bin` first, then on PATH (with the Workspace env). If it is missing, the panel shows one line with the install command, like Claude Code's `Executable not found in $PATH` ([cc-ci]). RADE never installs anything.

### 4.4 Trust: language servers are executables

- **Repo-provided config** (`.rade.toml [code]`) applies only while that exact content is trusted (`rade config trust`, R29.4). It is read from the main checkout, never from a Task Worktree (ADR-0024 §1). This is the same reason Zed's Restricted Mode skips `.zed/settings.json` language servers until the owner trusts the worktree ([zed-trust]), and VS Code's workspace settings can "contain paths to executables" ([vsc-trust]).
- **Built-in defaults still run repo code.** rust-analyzer runs `build.rs` and proc macros by default (`cargo.buildScripts.enable`, `procMacro.enable` = true, [ra-conf]). typescript-language-server loads the **workspace's** tsserver from `node_modules` first ([tls]). So "a server on the owner's PATH" is not "no repo code". RADE Sources are repositories the owner added, and Agents already run arbitrary commands in them. The threat is a freshly cloned third-party repo. The owner decides (D2). The recommendation is lazy start, only on an explicit navigation action, plus a per-Source `code.enabled = false`.

### 4.5 The diff: old side vs new side

- **Added and context lines** (new side) are the Worktree on disk, which is HEAD plus uncommitted work, exactly what the server indexed. Use LSP directly.
- **Removed lines** (old side) exist only at the merge-base. Accurate navigation would need a second checkout plus a second server (double memory). The recommended approach: definition and references for a removed-line identifier use (a) LSP on the same name in the new tree when the symbol still exists, or (b) text search with `git grep -w <name> <base>`, labelled "at base, text match". D4 asks the owner.
- **Impact** (feature #4) covers the new side only, plus *deleted* symbols. A function removed in the diff is listed with text-search references in the new tree: these are callers the diff may have broken.

---

## 5. UX sketch

### 5.1 Keys: local to the Viewer and the Review Pane

These keys are hard-coded in the pane's key controller, like today's `Esc` and `Alt+Left` (`viewer.rs`). They are not app-wide `[keybindings]`, so they never reach Ghostty and the R17.3 collision test (`r17_3_defaults_never_plain_ctrl_or_shift_enter`) does not apply. They do not touch IBus (Ctrl+Shift+U) or GNOME's Ctrl+Alt arrows.

| Key | Action | VS Code equivalent |
|---|---|---|
| Ctrl+click / F12 | Go to definition (same Viewer; the old place goes on Back) | F12 |
| Shift+F12 | References → Code page | Shift+F12 |
| Alt+Shift+H | Call hierarchy (incoming) → Code page | Shift+Alt+H (**UNVERIFIED** on Linux, [vsc-keys] does not list it) |
| Ctrl+F12 | Implementations | Ctrl+F12 |
| Alt+F12 | Peek definition (popover) | Alt+F12 ([vsc-nav]; Linux default Ctrl+Shift+F10, [vsc-keys]) |
| Alt+Left / Alt+Right | Back / Forward | Ctrl+Alt+− (avoided: GNOME uses Ctrl+Alt combinations) |
| hover (500 ms) | signature/docs popover | hover |

App-wide: a palette prefix `$` for workspace symbols (`>` actions, `@` panes and `#` files are taken in `palette.rs`). Don't use Ctrl+Shift+O for "symbol in file": RADE already binds it to `open_browser`. The outline lives on the Code page instead.

### 5.2 Viewer with the Right Panel "Code" page

```
┌ Viewer: src/billing/invoice.service.ts ─────── ← → ⧉ ↗ ✕ ┐┌ Files · Changes · Search · Code ──────────┐
│ 41  async createInvoice(dto: CreateInvoiceDto) {          ││ ▾ createInvoice  (InvoiceService)          │
│ 42    const total = this.pricing.▁total▁(dto.items);      ││   Callers ▾  Callees   References   Outline│
│ 43    ...                                                 ││ ▾ InvoiceController.create   ctrl.ts:28    │
│ ╭ total(items: LineItem[]): Money ──────────────╮         ││     ▸ (route POST /invoices)               │
│ │ Sum of line items after discounts.            │ hover   ││ ▾ BillingJob.run            job.ts:77      │
│ ╰───────────────────────────────────────────────╯         ││     ▸ Scheduler.tick         sched.ts:12   │
│                                                           ││   invoice.service.spec.ts:55   test        │
│                                                           ││ ── typescript-language-server · 1.2 s ──   │
└───────────────────────────────────────────────────────────┘└────────────────────────────────────────────┘
```

- A click on a row opens it in the Viewer at that line, and the old location goes on Back. The tree expands lazily, one `incomingCalls` request per expansion. The footer names the source (server name, or "text matches").
- The Code page reuses the Right Panel switcher (`RightPanel::set_page`). It is a page, not a bottom panel, because M26 is simplifying the layout and a bottom panel would be new chrome.

### 5.3 Review Pane: Impact and navigation on diff lines

```
┌ Review: feat/otp-login vs dev ─────────────────────────────────────────────────────────────┐
│ Impact ▾  3 changed functions · 11 call sites in 6 files (2 tests) · 1 removed, still used │
│   ~ AuthService.verifyOtp      ← AuthController.login, SmsWebhook.handle, auth.spec.ts      │
│   ~ OtpRepo.consume            ← AuthService.verifyOtp                                      │
│   − legacyHash  (removed)      ← tools/migrate.ts:40   text match                          │
│ ▾ M src/auth/auth.service.ts  +24 −9                                                        │
│   @@ -88,7 +88,12 @@ verifyOtp                                                            │
│ + │ const ok = await this.otp.consume(user.id, code);   ← Ctrl+click → Viewer at definition │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
```

- Diff rows are GtkLabels with markup (`review_rows.rs`). A Ctrl+click maps x to a character through the label's `pango::Layout::xy_to_index`, then to (file, new line, column) from the `DiffLine`.
- Each Impact row is clickable. "Comment" on an Impact row creates a Review Comment anchored at that caller ("check this call site"), and Send to Agent delivers it (R11).

---

## 6. Agent CLI

```sh
rade code def      src/auth/auth.service.ts:88:23          # → path:line:col  kind  name
rade code refs     src/auth/auth.service.ts:88:23 [--json] # one line per reference, tests marked
rade code callers  AuthService.verifyOtp [--depth 2]       # name resolved with workspace/symbol, or file:line:col
rade code callees  src/auth/auth.service.ts:88:23
rade code symbols  src/auth/auth.service.ts | --query verifyOtp
rade code hover    src/auth/auth.service.ts:88:23
rade code impact   [--base dev|<ref>] [--json]              # changed symbols → callers (Review's Impact)
```

- Paths are relative to the caller's Worktree (`$RADE_PANE_ID` → Pane → Worktree), so an Agent in a Task Worktree queries its own checkout. Output is plain lines by default (Agents read text), and `--json` gives machine output. Every result says `source: lsp <server>` or `source: text`. Exit codes follow `docs/API.md`, with a new kind, `NO_SERVER`, that carries the install hint.
- **Reviewer, Judge and acceptance prompts** (R65 rubric, `batch.rs` Reviewer prompt) get `rade code impact --base <base>` output appended, capped at about 60 lines. Every Reviewer then sees the same deterministic callers list whatever its tools are. Claude Code without an LSP plugin, Codex and OpenCode all benefit. This fits ADR-0020's "deterministic facts in, judgment out".
- The `rade` skill gets a short section on `rade code` ("before changing a function's signature, run `rade code callers`").
- Compared with Claude Code's own `LSP` tool: Claude Code supports definition, references, type info, file symbols, workspace symbols, implementations and call hierarchies, read-only ([cc-tools]). It starts its own server per session. Five Agents in five sessions means five rust-analyzers on the same Worktree (**UNVERIFIED** that it doesn't share). `rade code` shares one. Both can coexist.

---

## 7. Phased plan

| Phase | Scope | Size | Proof |
|---|---|---|---|
| **Spike** (before M28) | Throwaway `lsp.rs` against rust-analyzer (RADE, ~81k lines of Rust) and typescript-language-server (globex-api: 2,133 `.ts`; globex-web: 1,165 `.tsx` + 859 `.ts`). Servers go into a temp prefix, not system-wide. Measure time to first references and RSS after indexing. Test staleness while a script edits files. Check TS 3.7 + `tsserver.path`. | 2–3 days | numbers table in this file |
| **M28 — Code navigation in the Viewer + CLI (MVP)** | `rade-core/src/code/{lsp,servers,nav}.rs` with a fake LSP server (Python script) for unit/E2E tests; `[code]` layered config + trust; Viewer Ctrl+click/F12, Shift+F12, Alt+Shift+H, Alt+Left/Right with Forward; Right Panel Code page (callers/callees/references tree); text fallback; Status Bar indexing; `rade code def/refs/callers/callees/symbols`; TS/JS + Rust built-ins | ~2 weeks | `tests/e2e/m28.sh` with the fake server plus one real-server smoke test that is skipped (with the reason) when the binary is missing; light + dark screenshots |
| **M29 — Code navigation in Review + impact for Agents** | Ctrl+click on diff lines (new side LSP, removed lines text at base); the Impact section; `rade code impact`; impact in Reviewer/Judge/acceptance prompts; Review Comment from an Impact row | ~1 week | m29 with a seeded repo: changed function, 2 callers, a removed-but-used symbol |
| **M30 — Extras** | Hover popovers (`GtkSourceHoverProvider`), occurrence highlight, Outline tab, palette `$` workspace symbols, implementations, peek popover, `rade code hover`; Python (pyright or ty) and Go (gopls) built-ins | ~1 week | screenshots, unit tables |
| Later / only if asked | tree-sitter tags fallback; type hierarchy; a second server at merge-base for exact old-side navigation; SCIP for cross-repo impact (backend ↔ app) | — | — |

---

## 8. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| **Servers not installed** (here: `rust-analyzer` is only a rustup proxy, with "Unknown binary 'rust-analyzer' in official toolchain"; no typescript-language-server, pyright or ty; `gopls` 0.23.0 is present) | Feature looks broken | Text fallback plus a one-line install hint (`rustup component add rust-analyzer`, `npm i -g typescript-language-server typescript`). `rade code doctor` lists servers. |
| **Memory across many Worktrees** (Fan-outs and Batches create several at once) | GBs of RAM (**UNVERIFIED**, spike) | Lazy start only on a navigation request, `max_servers` LRU, idle stop. Reviewer prompts use one server per Worktree under review, stopped after the impact is computed. |
| Cold index (rust-analyzer, tsserver on 2k-file projects) | First request takes seconds | Text matches first, LSP results replace them when ready. Status Bar progress. |
| **Stale index while Agents edit** | Wrong lines and missing callers | Server-side watchers; `didChangeWatchedFiles` from a gio monitor; re-request on Viewer reload. The spike checks this. |
| Old toolchains (TS 3.7.2 in globex-api) | No call hierarchy (needs tsserver ≥ 3.8, [tls-src]) | `init.tsserver.path` to a newer TS. The panel says "call hierarchy needs TS ≥ 3.8" otherwise. |
| Monorepos (`globex-billing`: pnpm + turbo; `globex-agents`, `globex-portal`: pnpm workspaces) | Wrong root, duplicate servers, unresolved imports | Root = nearest `tsconfig.json` with the Worktree as the outer limit; one server per root. Owner override `[code.server.x] root = "…"`. |
| Repo code runs at server start (build.rs, proc macros, workspace tsserver) | Third-party clone runs code | D2. Lazy start on explicit action; per-Source `enabled = false`; trusted-only `.rade.toml`. |
| Server hangs or crashes | Frozen UI | All requests off the GTK thread with a timeout (Claude Code defaults to 60 s, [cc-plugins]; RADE: 10 s for UI, 60 s for `impact`). Restart at most 3 times, then "server crashed — Restart". |
| Concurrent M26/M27 UI changes (Right Panel, Review) | Merge conflicts | Start M28 after M27 lands. The Code page is a new page through `set_page`. |

---

## 9. Owner decisions

1. **How servers get installed.** (a) **Recommended:** RADE uses what is on PATH or in the repo's `node_modules/.bin`, shows the install command when one is missing, and `rade code doctor` lists them. (b) RADE downloads servers into `$XDG_DATA_HOME/rade/servers`, as Zed does ([zed-lang]). (c) Bundle servers in the .deb.
2. **When a server may start (repo code runs).** (a) **Recommended:** built-in defaults start lazily on the first navigation action in any Source; `.rade.toml [code]` only when trusted; per-Source `code.enabled = false`. (b) Opt-in per Source (`rade config set code.enabled true`). (c) As (a), but rust-analyzer starts with build scripts and proc macros off until the Source's `.rade.toml` is trusted.
3. **Languages in the MVP.** (a) **Recommended:** TypeScript/JS + Rust in M28, Python + Go in M30. (b) All four in M28 (+2–3 days). (c) TypeScript only.
4. **Removed lines in the Review.** (a) **Recommended:** LSP on the new side only; removed lines use the same name in the new tree, else `git grep` at base, labelled. (b) A temporary Worktree at the merge-base with its own server (exact, about 2× memory, slower). (c) No navigation on removed lines.
5. **Impact for Agents.** (a) **Recommended:** `rade code` CLI **and** automatic `impact` in Reviewer/Judge/acceptance prompts. (b) CLI only; Agents decide when to call it. (c) Nothing for Agents; rely on Claude Code's LSP plugins.

---

## Host facts (read-only, 2026-10-05)

| Fact | Command | Result |
|---|---|---|
| Language servers | `command -v …` | `gopls` (v0.23.0), `rg`, `node` v24.19.0. `rust-analyzer` is a rustup proxy without the component. None of typescript-language-server, tsgo, vtsls, pyright, basedpyright, ty, clangd, ctags. |
| GtkSourceView | `pkg-config --modversion gtksourceview-5`; GIR | 5.18.0; `HoverProvider`, `HoverContext`, `HoverDisplay`, `GutterRenderer` present. The `sourceview5` 0.11.2 crate has `subclass/hover_provider.rs`. |
| Owner's repos | `package.json` / `pyproject.toml` presence, `rg --files` counts | globex-web: React 17, TS ^5.3.3, 1,165 tsx + 859 ts. globex-api: NestJS 7.5, **TS 3.7.2**, 2,133 ts. globex-agents: **TS 7.0.2**, pnpm workspace. globex-billing: TS 5.9.3, pnpm + turbo. agent-registry, support-bot, dahua_access_control_server: Python (`pyproject.toml`). RADE: about 81k lines of Rust. |
| RADE deps | `Cargo.toml` | No async runtime (no tokio). `ignore`, `regex` and `search.rs` are available for the text fallback. |

## Sources

- [lsp]: LSP 3.17 specification. https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- [vsc-nav]: VS Code, Code Navigation. https://code.visualstudio.com/docs/editing/editingevolved
- [vsc-keys]: VS Code default keyboard shortcuts. https://code.visualstudio.com/docs/reference/default-keybindings
- [vsc-trust]: VS Code Workspace Trust. https://code.visualstudio.com/docs/editing/workspaces/workspace-trust
- [jb-hier]: IntelliJ IDEA, structure and hierarchy. https://www.jetbrains.com/help/idea/viewing-structure-and-hierarchy-of-the-source-code.html
- [gh-nav]: GitHub, Navigating code on GitHub. https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github
- [stack-graphs]: github/stack-graphs (archived 2025-09-09). https://github.com/github/stack-graphs
- [ts-tags]: tree-sitter, Code Navigation (tags queries). https://tree-sitter.github.io/tree-sitter/4-code-navigation.html
- [scip]: sourcegraph/scip. https://github.com/sourcegraph/scip
- [sg-nav]: Sourcegraph, Code navigation (precise vs search-based). https://sourcegraph.com/docs/code-search/code-navigation
- [zed-lang]: Zed, Configuring languages. https://zed.dev/docs/configuring-languages
- [zed-trust]: Zed, Worktree trust. https://zed.dev/docs/worktree-trust.html
- [helix]: Helix, Languages. https://docs.helix-editor.com/languages.html
- [nvim]: Neovim `lsp.txt` (vim.lsp.config/enable, root_markers, grr/gri/grt/gO). https://raw.githubusercontent.com/neovim/neovim/master/runtime/doc/lsp.txt
- [cc-ci]: Claude Code, Code intelligence plugins. https://code.claude.com/docs/en/plugins/code-intelligence
- [cc-tools]: Claude Code, Tools reference, "LSP tool behavior". https://code.claude.com/docs/en/tools-reference
- [cc-plugins]: Claude Code, Plugin manifest reference, `lspServers` (`requestTimeout` default 60000). https://code.claude.com/docs/en/plugins-reference
- [codex-lsp]: third-party write-up of Codex CLI LSP status and MCP bridges (**not primary; UNVERIFIED**). https://codex.danielvaughan.com/2026/04/25/codex-cli-lsp-integration-language-server-semantic-code-intelligence/
- [tls]: typescript-language-server README (tsserver lookup, `tsserver.path`). https://github.com/typescript-language-server/typescript-language-server
- [tls-src]: typescript-language-server `src/lsp-server.ts` (call hierarchy gated on TS ≥ 3.8; references, implementation, workspace symbol providers; v6.0.1, node ≥ 22.22.2). https://raw.githubusercontent.com/typescript-language-server/typescript-language-server/master/src/lsp-server.ts
- [ts7]: Announcing TypeScript 7.0 (2026-07-08). https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/
- [ty]: ty language server features. https://docs.astral.sh/ty/features/language-server/
- [ra-conf]: rust-analyzer configuration. https://rust-analyzer.github.io/book/configuration.html
- [async-lsp]: oxalica/async-lsp. https://github.com/oxalica/async-lsp
- [ls-types]: ls-types (lsp-types fork). https://docs.rs/crate/ls-types/latest
- crates.io API (versions and dates for lsp-types, async-lsp, lsp-server, tower-lsp, tree-sitter, scip, stack-graphs), queried 2026-10-05. https://crates.io/api/v1/crates/<name>

## Owner decisions (2026-10-05)
1. Servers: **use PATH / the repo's node_modules; show the install command when missing** (no downloads, no bundling).
2. Start: **on the first navigation action in a repo**; per-repo off switch; repo `.rade.toml` config only once trusted.
3. Languages: **TS/JS + Rust in M28**; Python + Go in M30.
4. Removed lines in Review: **look the name up in the current tree, else `git grep` at the base, labelled as text matches**.
5. Agents: **`rade code …` CLI + impact added automatically to Reviewer/Judge/acceptance prompts** (recommendation applied).
Plan: spike → M28 → M29 → M30, built automatically after 0.6.0 ships; one release **0.7.0** after M30.
