# Code navigation spike: LSP startup, memory, latency, staleness

Run 2026-10-05 for `docs/internals/research/code-navigation.md` §7 ("Spike"), before M28. Question: can RADE run real language servers per Worktree for definition, references and call hierarchy? This spike measures what they cost and what RADE must do to keep their answers right while Agents edit files.

All numbers come from this machine: 24 cores, 91 GB RAM, `/tmp` on tmpfs, cargo registry already warm. Expect slower start-up on smaller machines. Numbers marked **UNVERIFIED** were not measured.

## Method

- **Driver.** A throwaway Python LSP client over stdio (about 250 lines, not product code; kept in the session scratchpad). It handles Content-Length framing and answers `workspace/configuration` and `client/registerCapability`. It records `$/progress` and rust-analyzer's `experimental/serverStatus`. It samples the RSS and CPU time of the whole process tree from `/proc` every 0.5 s. Each server is started with `start_new_session`, so orphans can be found by session id.
- **Servers.** All installed into scratch prefixes:
  - rust-analyzer 1.97.1 (`rustup component add rust-analyzer`).
  - typescript-language-server 6.0.1 (`npm install --prefix <scratch>/lsp`).
  - TypeScript 5.9.3 (`npm install --prefix <scratch>/ts59 typescript@5.9`), used as `tsserver.path`.
  - TypeScript 7.0.2 (`tsc --lsp --stdio`, the native Go server). Plain `npm install typescript` now installs TS 7.
- **Repos.** Copies made with `git clone --local --no-hardlinks` into scratch. The real repos were only read; `git status` is unchanged.
  - RADE @ 72c575a, about 81k lines of Rust.
  - globex-api (a work repo): 2,133 `.ts` files, TS 3.7.2 in the workspace.
  - globex-web (a work repo): 2,024 `.ts`/`.tsx` files, TS 5.9.3.
  - The TS copies use a read-only **symlink** to the real `node_modules`, so nothing was installed. RADE's `ghostty/` submodule is absent in a clone. One run stubbed it (vendored GLAD copied in, plus an empty `libghostty-internal.so`) so that the build scripts succeed.
- **Per run:**
  1. `initialize`, then `didOpen` the first symbol's file.
  2. Poll `references` until it returns a non-empty result. That moment is **first usable**.
  3. For each of 5 symbols: `references`, then `definition` from the first call site, which must land on the declaration. Then `prepareCallHierarchy`, `incomingCalls` and `outgoingCalls`. Each result is compared with `rg -w` text matches. Then a warm `references` round.
  4. A staleness test (below), then `shutdown` and `exit`, then an orphan check.
- **Symbols:**
  - RADE: `run_logged`, `resolve_base`, `task::set_field` (another `set_field` exists in rade-app), `bounded_output`, `append_log`.
  - Backend: `applyOperations`, `AuthService.authenticateUser`, `S3Provider.uploadBuffer`, `penaltyFromCounts`, `HttpRequest`.
  - App: `useLocalStorage`, `momentWithTZ`, `Modal`, `useHasPermissions`, `useSwitchCompany`.
- **Staleness test.** It works on closed files, because servers ignore disk changes to files the client has open. All other documents are `didClose`d first. Then the test writes to disk:
  1. It prepends 3 lines to a closed file that calls the symbol (its reference lines must shift by +3).
  2. It appends a new caller to another closed file (the new caller must appear).

  `references` is then asked at +3 s and at +13 s with no notification. Then the driver sends `workspace/didChangeWatchedFiles` and asks again 2 s later. Then it `didOpen`s the edited file and sends an unsaved `didChange` that adds 2 more lines (expect +5). Finally the edits are reverted, the driver notifies, and checks that the results are back to baseline.
- **Worktrees.** `git worktree add` on each scratch clone. The same server ran on both checkouts at once, then sat idle for 60 s.

## Numbers

### Start-up, memory, CPU

"First usable" is the time from spawn to the first non-empty `references`. RSS covers the server's whole process tree.

| Server / repo | First usable | RSS at first usable | Peak RSS | CPU to first usable | Call hierarchy |
|---|---|---|---|---|---|
| rust-analyzer, defaults, RADE, cold `target/` (worktree, build scripts OK) | 5.9 s | 3.2 GB (RA 2.3 + `cargo check` 0.9) | **4.1 GB** | 30 s | yes |
| rust-analyzer, defaults, RADE clone (ghostty build scripts fail) | 8.2 s | 3.4 GB (RA 2.3 + rustc 1.0) | 4.1 GB | 32 s | yes |
| rust-analyzer, `checkOnSave=false`, `files.watcher=server` | 4.1 s | 2.1 GB | 2.3 GB | 9 s | yes |
| rust-analyzer, also build scripts and proc macros off | 3.9 s | 1.95 GB | 2.2 GB | 8 s | yes |
| typescript-language-server + workspace TS 3.7.2, backend | 4.0 s | 1.3 GB | 1.36 GB | 7 s | **no** (capability missing; `prepareCallHierarchy` → "TypeScript Server Error (3.7.2)") |
| typescript-language-server + `tsserver.path` TS 5.9.3, backend | 3.4 s | 1.54 GB | 1.66 GB | 7 s | yes |
| TS 7.0.2 native (`tsc --lsp --stdio`), backend | 0.27 s | 0.69 GB | 0.92 GB | 2.6 s | yes, but **wrong results** (see Findings) |
| typescript-language-server + workspace TS 5.9.3, app | 4.9 s | 2.1 GB | 2.17 GB | 9 s | yes |
| TS 7.0.2 native, app | never (all `references` empty) | — | 1.27 GB | — | returns nothing |

Idle footprint right after `initialize`, before any file is opened: rust-analyzer 45 MB, typescript-language-server 165–180 MB, TS 7 15 MB. tsserver does no work until a file is opened. rust-analyzer loads the whole workspace at once.

### Latency (ms; servers ready)

| | rust-analyzer (RADE) | TS 5.9 (backend) | TS 5.9 (app) |
|---|---|---|---|
| `references`, first time per symbol | 4–584 (`resolve_base` 584) | 24–223 (`authenticateUser` 223) | 20–78 |
| `references`, warm | 13–17 | 4–22 | 17–21 |
| `definition` | 7–28 | 2–4 | 2–3 |
| `prepareCallHierarchy` | 10–11 | 0.3–2 | 0.3–1 |
| `incomingCalls` | 12–16 | 4–21 | 17–19 |
| `outgoingCalls` | 11–25 | 1–5 | 1–14 |

### Correctness spot checks (references; LSP vs `rg -w`)

- **Definition from a call site landed on the declaration in 25/25 cases** across rust-analyzer, TS 3.7 and TS 5.9.
- **rust-analyzer.** References equal the text matches minus the declaration for 4 of 5 symbols. `set_field`: 17 references vs 24 text matches. The 6 extra text matches are the unrelated private `fn set_field` in `rade-app/src/tasks.rs`. LSP is right and text search is wrong.
- **Backend `authenticateUser`.** 1 reference vs 6 text matches. The other 5 are Cognito's `cognitoUser.authenticateUser`, a mock object, and a comment. LSP is right.
- **Backend `uploadBuffer`.** 8 references vs 29 text matches. The 8 are every call typed through `S3Provider`. The other ~20 are Jest mocks typed `any` (`s3Provider: { uploadBuffer: jest.fn() } as any`, `expect(s3.uploadBuffer)…`). **LSP misses uses through `any`.** For review impact these test files still matter.
- **App `Modal`.** 6 references vs 54 text matches (MUI's `Modal` and others share the name). `useHasPermissions`: 81 vs 82.
- **TS 3.7 vs 5.9.** TS 3.7 omits import specifiers from references: 5 vs 7 for `applyOperations`, 8 vs 12 for `penaltyFromCounts`. The call sites are the same.

### Staleness: are results right after an Agent edits closed files?

✓ = the new caller is found and the shifted lines are correct; ✗ = stale results.

| Server | Disk only +3 s | Disk only +13 s | + `didChangeWatchedFiles` (+2 s) | Open doc + unsaved `didChange` | Revert + notify |
|---|---|---|---|---|---|
| rust-analyzer default (`files.watcher = "client"`) | ✗ | ✗ | ✓ | ✓ | ✓ |
| rust-analyzer `files.watcher = "server"` | ✓ | ✓ | ✓ | ✓ | ✓ |
| tsserver 5.9 via typescript-language-server (backend, app) | ✓ | ✓ | ✓ | ✓ | ✓ |
| tsserver **3.7.2** via typescript-language-server | ✗ | ✗ | **✗** | lines ✓, new caller **✗** | ✓ (unchanged) |
| TS 7 native | ✗ | ✗ | ✓ | ✓ | ✓ |

- rust-analyzer in its default mode dynamically registers watchers for `**/*.rs`, `**/Cargo.{lock,toml}`, `**/rust-analyzer.toml` and `**/*.md` per crate, and then expects the client to send notifications.
- TS 7 registers `<root>/**/*` and every `node_modules/**/*` (including the symlink target). A client-side watcher for that would be large.
- tsserver 5.9 watches the files itself.
- **TS 3.7 never sees a new caller in a closed file**, even after a notification.

### Two Worktrees of the same repo (both servers running at once)

| Server | Per checkout at ready | After 60 s idle | Two checkouts |
|---|---|---|---|
| rust-analyzer (`checkOnSave=false`, watcher=server) | 2.07 / 2.12 GB | 2.24 / 2.29 GB | ~4.5 GB |
| TS 5.9, backend | 1.54 / 1.54 GB | 1.60 / 1.60 GB | ~3.2 GB |
| TS 5.9, app | 2.10 / 2.09 GB | 2.23 / 2.23 GB | ~4.5 GB |

Memory adds up linearly per Worktree. The two servers share nothing, and running both at once barely slowed start-up (4.7 s vs 5.2 s for RA; 4.0 s vs 4.5 s for the backend). **No server gives memory back while idle.** RSS grew 3–8 % over 60 s idle.

### Shutdown

- rust-analyzer and typescript-language-server exited in ≤ 0.7 s after `shutdown` + `exit`, with exit code 0 and **no orphans** in their session. That held even with a `cargo check` running under rust-analyzer.
- **TS 7 rejects `shutdown` sent with `"params": null`** (`InvalidParams: expected no params, got null`) and keeps running. The driver had to kill its process group after 15 s. With `params` omitted (the fixed driver), rust-analyzer and TS 5.9 behaved the same as before.

## Findings

1. **Start-up is not the problem; memory is.** Every server answered within 4–6 s of spawn on a cold start. After that, the slowest request seen was 0.6 s (first `references` on a symbol), and warm requests take ≤ 35 ms. Each server costs about **1.5–2.3 GB** for these repos, and the memory is never returned while idle.
2. **rust-analyzer's default `checkOnSave` is expensive and useless here.** At start-up it runs `cargo check` on the whole workspace. That added up to 1.4 GB of transient rustc RSS and about 20 s of CPU, and it wrote **636 MB into the Worktree's `target/`**, taking the cargo lock that the Agent's own builds use. Navigation doesn't need diagnostics (code-navigation.md §2: diagnostics out of scope).
3. **Build scripts and proc macros are cheap.** Turning them off saved about 0.2 s and 150 MB (`rust-analyzer-proc-macro-srv` is 44 MB). In a fresh Worktree without the ghostty build, build scripts fail (`health: warning`), and navigation still works fully. The 5 symbols tested do not depend on macro output. Items generated by derive macros (serde, glib) would be missing with proc macros off (**UNVERIFIED** here).
4. **TS 3.7.2 (globex-api) must not be served by its own tsserver.** It has no call hierarchy, and it never picks up new callers after edits. `tsserver.path` pointing at TS 5.9.3 fixes both on this unchanged TS 3.7-era project. The removed `suppressImplicitAnyIndexErrors` option did not prevent loading.
5. **`npm i typescript` now installs TS 7, which has no `tsserver.js`**, so typescript-language-server cannot use it. The install hint has to say `typescript@5`.
6. **The TS 7 native server is not usable on the owner's two repos yet.** TS 7 removed `baseUrl` (`error TS5102: Option 'baseUrl' has been removed`).
   - The backend imports `src/...` through `baseUrl`, so those imports go unresolved. `uploadBuffer` got 2 references instead of 8, and `HttpRequest` got 0 instead of 8.
   - The app's `paths: {"~*": ["./*"]}` was relative to `baseUrl: ./src`, so every reference came back empty.

   It is 10× faster to start and uses half the memory, so it is the right server for TS 7 repos (globex-agents), but not by default.
7. **Before the server is ready, requests return empty results, not errors.** RADE must not show "0 references" during indexing. Ready signals:
   - rust-analyzer: `experimental/serverStatus` with `quiescent: true`.
   - typescript-language-server: `$/progress` "Initializing JS/TS language features…" ends.
8. **Disk changes to a document the client has open are ignored.** The `didOpen` text wins until `didClose` or `didChange`.
9. LSP precision beats text search wherever names collide (`set_field`, `authenticateUser`, `Modal`). It misses uses through `any`-typed mocks, which are common in the NestJS specs.

## Recommendations for M28

| Topic | Recommendation | Why (measured) |
|---|---|---|
| **Idle timeout** | Keep `[code] idle_minutes = 15`. Idle stop is the only way to reclaim memory. | RSS never drops while idle (+3–8 % over 60 s). |
| **Max servers** | Keep `[code] max_servers = 4`, LRU. Budget about 2 GB per server (≈ 8–9 GB worst case). Mention the budget in `rade code doctor`. Reviewer and impact runs stop their server when they finish. | 1.5–2.3 GB each, linear per Worktree. A restart costs only 4–6 s, so stopping aggressively is cheap. |
| **Keeping results fresh** | Build **no** file watcher in M28. (1) Configure rust-analyzer with `files.watcher = "server"`. (2) Use tsserver ≥ 5, which watches by itself. (3) Don't keep documents open: `didOpen` → request → `didClose`, or send a full-text `didChange` when the file's mtime has changed since it was opened. (4) Before `rade code impact` and the Review Impact list, send `didChangeWatchedFiles` for the paths in the diff (`git status`/`diff.rs` already lists them). This cheap safety net also covers client-watch servers such as TS 7. | Staleness table: these settings were fresh within 3 s with no notifications. Open docs ignore disk changes. A recursive gio watcher would need one monitor per directory (GFileMonitor isn't recursive) and would have to cover `node_modules` for TS 7. |
| **rust-analyzer settings** (built-in default `[code.server.rust] settings`) | `{"rust-analyzer": {"checkOnSave": false, "files": {"watcher": "server"}, "cargo": {"targetDir": true}}}`. Leave `cargo.buildScripts.enable` and `procMacro.enable` at their defaults (on): they are cheap, and owner decision 2 already accepts running repo code on the first navigation action. | `checkOnSave` costs 1.4 GB, 20 s CPU, 636 MB `target/` and the cargo lock. `targetDir: true` keeps rust-analyzer's build-script builds in `target/rust-analyzer`, off the Agents' lock (**UNVERIFIED**, not measured). Lean mode saved only 0.2 s and 150 MB. |
| **TS server config** | Read `<root>/node_modules/typescript/package.json`. If it is missing or older than **5.0**, pass `initializationOptions.tsserver.path` = the owner's `[code.server.typescript] tsserver_path`. If that is unset, show one line: "This repo's TypeScript 3.7 is too old for code navigation. Install a newer one: `npm i -g typescript@5` and set `code.server.typescript.tsserver_path`". Always pass `disableAutomaticTypingAcquisition: true` (no npm runs). The install hint for the server: `npm i -g typescript-language-server typescript@5` (not plain `typescript`). | TS 3.7: no call hierarchy and stale results. TS 5.9 on the same project: correct and fresh. Plain `typescript` is TS 7 with no `tsserver.js`. |
| **TS 7 native server** | Not a built-in default in M28. Allow it as `command = ["tsc", "--lsp", "--stdio"]` for TS 7 repos (globex-agents). It needs client `didChangeWatchedFiles` (rule (4) above covers impact). | Fast and small, but wrong or empty results on `baseUrl`-based tsconfigs, and it doesn't watch files. |
| **Readiness** | Show "indexing" until rust-analyzer's `serverStatus.quiescent` arrives or tsserver's init `$/progress` ends. While indexing, answer with text matches, labelled as such. Ask for `experimental.serverStatusNotification` and `window.workDoneProgress` in `initialize`. | Empty results before ready (findings 7). Ready in 4–6 s. |
| **Timeouts** | UI 10 s, `impact` 60 s, as planned. | The worst first request was 0.6 s. |
| **Shutdown** | Send `shutdown` **without `params`**, then `exit`, wait 2 s, `SIGTERM` the process group, wait 2 s, `SIGKILL`. Spawn each server with `setsid`, as `run_logged` does. | TS 7 rejects `params: null` and stays alive. The others exit in < 1 s with no orphans. |
| **Impact list** | Use LSP callers, plus `rg -w` matches **in test files** that LSP did not return, labelled "text match". | `uploadBuffer`: ~20 `any`-typed mock uses that LSP can't see. |
| **Position encoding** | Offer `["utf-8", "utf-16"]`. rust-analyzer and TS 7 picked UTF-8. typescript-language-server returned no `positionEncoding`, which means UTF-16. Keep one tested converter for both. | `initialize` results. |
| **Type hierarchy** | Leave it out, as planned. rust-analyzer 1.97.1 did **not** advertise `typeHierarchyProvider` either, which corrects code-navigation.md §2 #13. | `initialize` results. |

## Risks

- **Memory with Fan-outs and Batches.** Five Worktrees under review means ~10 GB if every one gets a server. `max_servers` is the hard cap. A machine with 16 GB needs `max_servers = 2`. Document this in the `[code]` docs.
- **Measured on a fast, warm machine.** 24 cores, cargo registry cached, `/tmp` on tmpfs. Cold start-up on a laptop, and a first-ever rust-analyzer run that downloads crates, will be slower (**UNVERIFIED**).
- **rust-analyzer's server watcher and tsserver use inotify.** Several Worktrees × large trees could hit `fs.inotify.max_user_watches`. That limit is 524,288 here, and no warnings were seen. Ubuntu's default may be lower on other machines (**UNVERIFIED**).
- **The `tsserver_path` fallback depends on the owner having TS 5 installed.** Owner decision 1 (no downloads) means the backend has no call hierarchy until then. The text fallback covers that.
- **Monorepos were not tested** (pnpm/turbo: globex-billing, globex-agents). Root detection is still open.
- **Removed-line navigation (owner decision 4) and Agents editing *while* a request runs were not tested.** Only edit-then-query was tested.

## Reproduce

These are throwaway tools, not product code. The driver is `drive.py CONFIG.json OUT.json` with `cmd`, `root`, `init`, `settings`, `symbols` and `stale`. The configs were generated by a small `gen.py`. Raw JSON results and logs are kept per run under the spike's scratch `out/` directory. Install the servers into a scratch prefix as described in Method, never globally, and run against clones of the repos, never the real checkouts.
