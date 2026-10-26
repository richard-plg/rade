# M48 — Updates: check, install, restart (implementation plan)

Owner request of 2026-10-26: RADE shows its version, tells you when a newer release is out, and
installs it from inside the app.

Owner decisions 2026-10-26 (from the design proposal in the session):
1. Automatic checks are **on** by default, with a clear line in Settings saying what is sent.
2. RADE **installs** the `.deb` itself (download → verify → `pkexec apt-get install`); it does not
   only download.
3. A **security** release raises one Toast; other releases stay quiet (a Statusline cell, Activity,
   Settings).
4. Install while the app runs, restart only when the owner says so (never automatic).

Every other choice is fixed below (§2). **Implementers make no product or design decision.** If
something is not covered here, or the code or a tool contradicts this plan, stop and report the
question instead of choosing.

Facts verified on 2026-10-26 (owner's machine, Ubuntu 26.04, RADE 1.0.0 → 1.1.0):
- `GET https://api.github.com/repos/richard-plg/rade/releases?per_page=2` (no token) answers 200,
  `x-ratelimit-limit: 60` (per IP per hour). Each release has `tag_name` (`v1.1.0`), `name`,
  `draft`, `prerelease`, `published_at`, `html_url`, `body` (the CHANGELOG section: `### Added` /
  `### Changed` / `### Fixed` groups, then a last line `Install: \`sudo apt install ./rade_….deb\` ·
  verify: …` added by `ci/publish-release.sh`) and `assets[]` with `name`, `size`,
  `browser_download_url` (`https://github.com/richard-plg/rade/releases/download/v1.1.0/rade_1.1.0_amd64.deb`)
  and `digest` (`sha256:<hex>`). Assets: `rade_<v>_amd64.deb` and `SHA256SUMS`.
- `pkexec apt-get install -y <path to .deb>` run from a process inside the desktop session shows
  the GNOME polkit dialog, and upgraded `rade` 1.0.0 → 1.1.0 **while the app ran**: the running
  app kept working (`/proc/<pid>/maps` shows `/usr/lib/rade/rade-app (deleted)` and
  `libghostty-internal.so (deleted)`), the new `rade` 1.1.0 CLI talked to the 1.0.0 app
  (`rade list`, `rade tab list` exit 0). apt printed `dpkg-preconfigure: unable to re-open stdin`
  (harmless).
- `std::env::current_exe()` of a process whose binary was replaced reads `… (deleted)`.
- `gh` 2.46 on the owner's machine has no `gh attestation` (needs ≥ 2.81): the app does not verify
  attestations (ADR "Not done").
- Phosphor 2.1.1 `regular/arrow-circle-up.svg` and `regular/arrow-clockwise.svg` answer 200 on
  unpkg.

---

## 0. Rules for whoever implements a task

- Read `CLAUDE.md` (project) first: Conventional Commits with R-IDs, tests first for core logic,
  `cargo clippy --workspace --all-targets` clean, `cargo test --workspace` green.
- Do only your task's files. Don't refactor, rename or "improve" anything else.
- Core logic (`rade-core`) gets unit tests named after the AC: `fn r127_a_…`. E2E checks are named
  `check "R127-i …"` (see `scripts/trace.sh`).
- UI strings, CSS classes, icon names, config keys, env names, debug JSON keys: copy them
  **exactly** from this plan.
- After each task: `cargo build --release && cargo clippy --workspace --all-targets && cargo test
  --workspace`, then commit with the message given in the task.
- **Never** let a test reach `api.github.com` or `github.com`, never run `pkexec`, `apt` or `dpkg -i`
  in a test, never install anything on this machine. Unit tests use fixtures; E2E uses
  `tests/e2e/fake-github.py`, `fake-install.sh`, `fake-dpkg-query.sh`.
- Never run `rade setup` for real, never push, never run `scripts/release.sh`.

---

## 1. Scope and IDs

- Milestone **M48**, ADR **0048**, PRD **R127**.
- Branch `m48-updates` from `main` (1.1.0).
- IDs M47 / R125 / R126 / ADR-0047 belong to branch `m47-accounts` (Agent Accounts with env, not merged). This plan uses M48 / R127 / ADR-0048 so they don't collide. Whichever merges second rebases: PRD order, `MILESTONES.md`, `CONTEXT.md`, the Settings `CATEGORIES` list (m47-accounts adds an `Accounts` category; `Updates` stays last), `settings_view.rs`, `docs/API.md` and the CI shard list are the expected conflicts.
- Release impact: **MINOR** (new command `rade version`, new socket method `app.info`, new config
  section `[update]`, new `debug.update`; nothing removed or changed). No release in this
  milestone.
- Out of scope (say so in the ADR): an apt repository / PPA, automatic install or restart, delta
  updates, attestation checks in the app, Flatpak / Snap, downgrades, updating a Devel build,
  architectures other than amd64.

## 2. Fixed decisions

| # | Decision |
|---|---|
| D1 | Term: **Update** = a newer RADE release on GitHub Releases that RADE can install. **Update check** = one request for the release list. |
| D2 | Source: `GET https://api.github.com/repos/richard-plg/rade/releases?per_page=20` with headers `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, `User-Agent: rade/<current version>`. No token, no cookie, no other header, no query beyond `per_page`. Sent with `curl` (core `update::curl_get(url, user_agent, follow)`): `curl -q -sS --noproxy 127.0.0.1,localhost,::1 --connect-timeout 15 --max-time 30 [-L --max-redirs 5] [--proto =https --proto-redir =https] -H … -w "\n%{http_code}" <url>` (`-q` first: no `~/.curlrc`; `-L …` when `follow`; the `--proto` pair whenever `RADE_UPDATE_URL` is not in effect). The release list uses `follow = false`; `SHA256SUMS` and the `.deb` use `follow = true` (GitHub answers their `browser_download_url` with a 302 to its asset host, verified 2026-10-26). Every child process of the update code (curl, dpkg-query, the installer, the restart helper) is spawned with `pre_exec(rade_core::process::no_inherit)`, stdin null; stdout piped only where it is read to the end, else null. |
| D3 | When (app): first check `RADE_UPDATE_DELAY` seconds after the window is shown (default 60), then a 1-hour GLib timer; the first check and every tick ask whether one is **due**: no cache, or ≥ 86,400 s since `checked_at`, or ≥ 3,600 s when the last check failed. A due check runs the request; a not-due one only re-reads the cache (so a relaunch within the day sends nothing). Every tick also re-reads the cache (the CLI may have written it) and runs the installed-version query (D14) off the UI thread. **Check now** (Settings, palette, `rade version --check`) always sends one request. At most one request runs at a time. **No check runs while the phase is `downloading`, `verifying`, `installing` or `restart_pending`**: a scheduled one is skipped; Check now / the palette action then raise the Toast `An update is already in progress` (in `restart_pending`: `Restart RADE to finish the update first`). |
| D4 | Enabled when all hold: env `RADE_UPDATE_CHECK` is not `0`; `[update] check = true` (default true); and the app is the Installed flavor **or** (the Devel flavor with `RADE_UPDATE_URL` set). `RADE_UPDATE_CHECK=0` also disables Check now (it is the test harness switch). `[update] check = false` disables only the automatic checks: the phase still comes from the cache (an offer found earlier still shows) and Check now still works. A Devel build without `RADE_UPDATE_URL` never checks (About says so). **Override variables** (`RADE_UPDATE_URL`, `RADE_UPDATE_DELAY`, `RADE_UPDATE_INSTALL`, `RADE_UPDATE_DPKG_QUERY`) are honoured **only in the Devel flavor** (they redirect a root install, so the .deb ignores them); `RADE_UPDATE_CHECK=0` works in both. Phase precedence at start: `devel` first, then `off`, then the rest. |
| D5 | Channel `[update] channel = "stable"` (default) or `"rc"`. Candidates: releases with `draft = false`, a `tag_name` that parses as `v<semver>` (D6), an asset named `rade_<deb version>_amd64.deb` and an asset named `SHA256SUMS`; `stable` also needs `prerelease = false` and no pre-release part. The **offer** = the highest candidate above the **floor**: the running version (`env!("CARGO_PKG_VERSION")`), or for install kind `deb` the higher of the running and the installed version (D14) — so an installed-but-not-restarted version is never offered again and nothing is a downgrade. None → no offer. The offer is always computed from the cached `releases` (D9), so a later failed check never hides it. |
| D6 | Versions (core `update::Version`): `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, PRE = dot-separated identifiers compared per SemVer 2.0.0 §11 (numeric < alphanumeric, numeric by value: `rc.2 < rc.10`; a version with PRE < the same without). BUILD is ignored for ordering (`1.1.0+git202610261200.abc1234` == `1.1.0`). From a tag: strip the leading `v`. From dpkg (`dpkg-query`): the **first** `~` → `-` (`1.1.0~rc.1` = `1.1.0-rc.1`). The `.deb` name replaces the **first** `-` with `~` (`rade_1.1.0~rc.1_amd64.deb`), as `scripts/version.sh` and `ci/publish-release.sh` do. Anything else is not a version (`None`). |
| D7 | Offer facts: **kind** = `major` (MAJOR above the running one), else `minor` (MINOR above), else `patch`; **security** = any candidate allowed by the channel, *above the running version and at or below the offer*, has a line exactly `### Security` in its body; **notes** = the bodies of the candidates allowed by the channel, above the running version and at or below the offer, newest first, each under `## RADE <version> · <published_at as YYYY-MM-DD>`, with every line starting `Install: ` removed, written under the title `# What's new since RADE <current>`. |
| D8 | Security marker: `cliff.toml` gets the parser `{ footer = "^[Ss]ecurity: *yes$", group = "<!-- 3 -->Security" }` as the **second** entry (after the skip parser), so a commit with the footer `Security: yes` is listed under `### Security`; a hand-written `### Security` under `## [Unreleased]` works the same way (release.sh already merges groups). `docs/RELEASING.md` documents it. |
| D9 | Cache (per flavor): `$XDG_CACHE_HOME/<flavor name>/update/update.json` (`~/.cache/rade/update/…` for the .deb, `rade-dev` for Devel; `paths::update_dir`). Fields: `checked_at` (Unix s, `null` = never), `error` (string or null), `releases` (the parsed candidates of the last good answer: version, tag, published, html_url, body, prerelease, deb {name, url, size, digest}, sums {name, url, size, digest}), `skipped` (version string or null), `toasted` (version string or null), `just_installed` (version string or null). Every write is **load → change only the writer's own fields → save** (`Cache::update(dir, |c| …)`), atomically through `update.json.<pid>.tmp` + rename, so the app and `rade version --check` never undo each other (the check writes `checked_at` / `error` / `releases`; the app also `skipped` / `toasted` / `just_installed`). Read leniently: a missing or unparsable file = empty cache (rewritten on the next write). The CLI and the app share it. Downloads go to the same folder. |
| D10 | Install kinds (core `update::install_kind`): **`devel`** = Devel flavor without `RADE_UPDATE_URL`; **`deb`** = Installed flavor with `restart_exe()` == `/usr/lib/rade/rade-app` and the installed-version query (D14) giving a version, or Devel flavor with `RADE_UPDATE_INSTALL` set; **`other`** = everything else (`cargo install`, an extracted .deb, …): it is told about updates but gets a **Download** button (opens the release's `html_url`) instead of Install. Any architecture but `x86_64` is `other`. |
| D11 | Install (only `deb`, only on a click): (1) download the offer's `.deb` with `curl -sS -L --fail --connect-timeout 15 --max-time 900 -o <dir>/<name>.part <url>`, progress = the `.part` file's size / the asset's `size`, polled every 250 ms; Cancel kills curl, removes the `.part` and goes back to `available` with no error; curl failing → failed `Download failed: <last stderr line>`; (2) fetch `SHA256SUMS` into memory (`curl_get`, `follow = true`); a failure → failed `Couldn't get SHA256SUMS: <error>`; (3) verify: SHA-256 of the file (core, `sha2`) equals the `SHA256SUMS` line for the file name **and** equals the asset `digest` when the API gave one; else delete the file and fail with `Download didn't match SHA256SUMS. Nothing was installed.`; on success rename `.part` → `.deb`; (4) run the install command (D12) with stdin null, stdout null, stderr captured (`wait_with_output`), off the UI thread; a spawn error → failed `Couldn't start the installer: <error>`; (5) on success delete the `.deb`, set `just_installed`, re-query the installed version (D14) and go to **restart pending**. |
| D12 | Install command: `RADE_UPDATE_INSTALL` set (Devel only, D4) → `sh -c "$RADE_UPDATE_INSTALL \"\$1\"" rade-install <deb path>`; else `pkexec /usr/bin/apt-get install -y --no-remove <deb path>`. Outcome (core `update::install_outcome(code, stderr)`): 0 → done; 126 → **cancelled** (the owner closed the polkit dialog: back to Available, the verified `.deb` is kept and reused by the next Install after a re-verify, no error shown); 127 → failed `Not authorized to install software.`; stderr containing `Could not get lock` → failed `Another package manager is running. Try again in a minute.`; any other → failed with the last non-empty stderr line (≤ 200 chars), else `apt-get exited with <code>`. |
| D13 | Asset URLs: with `RADE_UPDATE_URL` unset, the `.deb` and `SHA256SUMS` URLs must be exactly `https://github.com/richard-plg/rade/releases/download/<tag>/<asset name>`; with it in effect (Devel, D4) they must start with that URL's `scheme://host[:port]/`. Any other URL drops the release from the candidates (core `update::asset_allowed`). Redirects are followed only for `SHA256SUMS` and the `.deb` (D2); with the override not in effect they must stay on https. |
| D14 | Installed-version query: `RADE_UPDATE_DPKG_QUERY` set (Devel only) → `sh -c "$RADE_UPDATE_DPKG_QUERY"`; else `dpkg-query -W -f=${db:Status-Abbrev}${Version} rade` (argv, no shell). Core `update::installed_from_query(stdout)`: the trimmed stdout must start with `ii ` (installed; a removed package with config files left prints `rc `), the rest parsed per D6; anything else = not installed. It runs at start (for D10), on every hourly tick, after every check and after every install. Only for install kind `deb`: when it is **above** the running version and there is no offer above it (D5) → **restart pending** for that version, whoever installed it (the owner's own `apt install` while RADE runs counts). Kinds `other` / `devel` never enter restart pending. |
| D15 | Phases (core `update::Phase`): `devel`, `off` (`RADE_UPDATE_CHECK=0` only), `idle` (no offer: up to date or never checked; the cache's `error` only changes the About row when there is no offer), `checking`, `available` (an offer, whatever the last check's `error`; `skipped` marks a skipped one; when the installed version is also above the running one, About's buttons add `Restart…`), `downloading` (pct 0–100), `verifying`, `installing`, `restart_pending` (version), `failed` (error). `checking` keeps the previous offer for the views (the cell does not flicker). After a check, after Cancel, and after `failed` + a successful check: the phase is recomputed from the cache (`available` / `idle` / `restart_pending`); `failed` lasts until Try again, Skip or the next check. Core `update::phase_after(...)` computes it; the app never sets `available` / `idle` / `restart_pending` by hand. |
| D16 | Statusline cell (persistent widget like the REC cell, class `status-cell status-cell-update`, a flat `gtk::Button` like `hint()`, not focusable on click; icon 13 px, label JetBrains Mono, ellipsizing like the other cells' labels). Shown only in these phases; texts and classes from core `update::cell`: |
| | `available` (or `checking` while an offer is known), not skipped: icon `ph-arrow-circle-up-symbolic`, text `<v>`; security → `<v> security` + class `wait`; else major → `<v> major` + class `wait`. Tooltip `RADE <v> is available · click for details`. |
| | `downloading`: same icon, `<v> <pct>%`. `verifying` / `installing`: same icon, `<v> installing`. |
| | `restart_pending`: icon `ph-arrow-clockwise-symbolic`, text `restart for <v>`, class `acc`, tooltip `RADE <v> is installed · click to restart`. |
| | `failed`: icon `ph-warning-diamond-symbolic`, text `update failed`, class `err`, tooltip = the error. |
| | Click: `restart_pending` → the Restart dialog (D20); every other phase → Settings › Updates (D18). Placement: in the Statusline root after the `left` box (cells + REC) and before the system monitor. Fit (`statusline_fit`): drop order becomes hints → monitor → **update** → ports → identity (new `CellKind::Update`, `as_str` `"update"`). In `Statusline::fit` the update widget and a `CellKind::Update` cell are included whenever the last `CellView` given to it is `Some` (never judged by widget visibility, which `fit` itself sets); `UpdateCell::set` bumps the Statusline's `generation` and calls `fit(window_width())` so a cell that appears or changes text is refitted. |
| D17 | Toast (security only): `RADE <v> fixes a security issue`, button `Details` → Settings › Updates. Raised once per version (`toasted` in the cache), only while `available` and not skipped, and **not while any Pane is waiting** (`State::waiting_panes()` non-empty): `Chrome::after_update` calls `update::maybe_toast()` so it shows at the first model update with nobody waiting. After a restart on a new version: if `just_installed` == the running version → Toast `Updated to RADE <v>` with button `What's new`, which opens `https://github.com/richard-plg/rade/releases/tag/v<v>` with `update::launch` (= `viewer::launch_uri`, except in test mode — `handlers::DEBUG_SOCKET` — where it only records the URI in `launched`; every URI the update code opens goes through it); `just_installed` is cleared at every start whatever its value (written to the cache at once), so the Toast shows at most once. |
| D18 | Settings: new category `"Updates"` **after** `"Statusline"` (last). The section starts with the **About block** (§5.1), then the rows `update.check` and `update.channel` (§T2). `settings_view` gets `pub(crate) fn open_at(category: &str)` = open (if closed), set the selected category, then `scroll_to` inside `glib::idle_add_local_once` (after the first allocation, else the scroll lands at 0). `debug.ui app_settings` state gains `"category"` (the selected one). Palette action `("about", "About RADE")` and the cell's click use `open_at("Updates")`. |
| D19 | Release notes: `Release notes ›` (About) writes the D7 notes to `<update dir>/notes-<v>.md` and opens it with `app.update(|m| { let (s, w) = m.active(); let (s, w) = (s.id.clone(), w.id.clone()); m.open_viewer_at(&s, &w, &path, None, true, false) })` (the Markdown Viewer, focused), then closes Settings. |
| D20 | Restart dialog (`adw::AlertDialog`): heading `Restart RADE to finish updating?`; body = core `update::restart_body(&facts, &<installed version>)` from `overview::facts(&model, &|_| String::new())` (every Agent Pane of every Workspace): no Agent running or waiting → `RADE <v> opens with your Tabs and Panes as they are now.`; else one line per non-zero count, running first, with these exact strings: `1 Agent is running. It stops now; Session Restore reopens it as interrupted — send “continue” to go on.` / `<n> Agents are running. They stop now; Session Restore reopens them as interrupted — send “continue” to go on.`; `1 Agent is waiting for you: <name>. Session Restore reopens it as interrupted.` / `<n> Agents are waiting for you: <a>, <b>, <c> +<k>. Session Restore reopens them as interrupted.` (≤ 3 names in `facts` order, then `+k`; Session Restore makes running **and** waiting Agents `interrupted`, `agents.rs` `restore_pane`). Responses: `not_now` `Not now` (default and close response), `restart` `Restart` (appearance `suggested` when nobody runs, `destructive` when an Agent runs). |
| D21 | Restart: (1) core `update::spawn_restart(pid: u32, exe: &Path) -> std::io::Result<()>` (in `rade-core`, which has `libc`; `rade-app` has no `libc` dependency and gets none) spawns the helper `sh -c 'i=0; while [ "$i" -lt 300 ] && [ -e "/proc/$1" ] && [ "$(cut -d" " -f3 "/proc/$1/stat" 2>/dev/null)" != Z ]; do sleep 0.2; i=$((i+1)); done; flock -w 30 "$3" true 2>/dev/null; exec "$2"' rade-restart <own pid> <exe> <state.json.lock>` (after the pid wait it waits for the old app's `state.json.lock` flock to be free: found in T3, a zombie leader's other threads can still hold it, and the new app then refuses to start; `spawn_restart(pid, exe, lock)`) (the `Z` test: an exited parent not yet reaped — e.g. a child of the E2E shell — counts as gone) with stdin/stdout/stderr null, `pre_exec` = `no_inherit()` + `libc::setsid()`; an error → Toast `Couldn't restart: <error>` and no quit; (2) `app.quit()` (the normal quit: save, socket removed, `_exit`). The helper waits up to 60 s for the old pid to go, then execs the new binary, which takes the socket and `state.json` lock and restores the session. `restart_exe()` = `current_exe()` with a trailing ` (deleted)` removed; if that path is not a file → Toast `Can't find the new RADE at <path>` and no quit. |
| D22 | Skip: `Skip This Version` (About, `available` only) sets `skipped = <v>`: no cell, no Toast, About shows the skipped state (§5.1). A higher offer later is not skipped. There is no "remind me later" (the cell is already quiet). |
| D23 | Activity (new `activity::Kind::Update`, `kind_str` `"update"`, mark icon `ph-arrow-circle-up-symbolic`), mark CSS class `activity-update` (like the other marks in `activity_view.rs`), actor `RADE`, **once per text per run** (an offer read from the cache at start logs its row then; a skipped offer logs nothing): `<v> is available` (`<v> is available · security fix` when security), `<v> installed · restart to finish`, `update failed: <error>`. |
| D24 | `rade version [--check] [--json]` (local, works without the app; §3.1 item 8 has the texts). `--check` sends one request (D2, ignoring D3's due rule) and writes the cache, unless `RADE_UPDATE_CHECK=0` or a Devel build without `RADE_UPDATE_URL` (then it prints `rade: update checks are off (RADE_UPDATE_CHECK=0)` / `rade: a development build doesn't check for updates` on stderr and exits 0). `rade --version` stays as it is. Palette **Check for Updates** in `devel` / `off` raises the Toast `Update checks are off for this build` (devel) / `Update checks are off (RADE_UPDATE_CHECK=0)`. |
| D25 | Socket `app.info` → `{"version": "<CARGO_PKG_VERSION>", "pid": <u32>, "flavor": "rade"|"rade-dev", "update": "<phase as_str>"}`. CLI hint: when any call fails with a message containing `unknown method '`, the CLI calls `app.info`; if that answers with a different version, or also fails with `unknown method`, it prints a second line on stderr `rade: this rade is <cli>, the running RADE app is <app version | older>: restart RADE to finish updating` (exit code unchanged). |
| D26 | Config `[update]` = `UpdateConfig { check: bool /* true */, channel: UpdateChannel /* Stable */ }`, `#[serde(default, deny_unknown_fields)]`, `UpdateChannel` `#[serde(rename_all = "snake_case")]` with `Stable`, `Rc`. `"update"` added to `APP_WIDE` (user `config.toml` only). |
| D27 | Env (test hooks and power users; documented in `docs/features/updates.md` and API.md): `RADE_UPDATE_CHECK`, `RADE_UPDATE_URL` (the full releases-list URL), `RADE_UPDATE_DELAY` (seconds, clamped to 0–3600, unparsable = 60), `RADE_UPDATE_INSTALL`, `RADE_UPDATE_DPKG_QUERY`. All but `RADE_UPDATE_CHECK` are honoured only in the Devel flavor (D4). The app reads them at start and then removes all five from its own environment (add them to `scrub_inherited_agent_session_env` in `main.rs` — read first, scrub after), so Terminals and the restart helper don't inherit them; the restarted app therefore runs without them. `tests/e2e/lib.sh` exports `RADE_UPDATE_CHECK=${RADE_UPDATE_CHECK:-0}` next to `RADE_SYSTEM_MONITOR`. |
| D28 | No spec sheet and no goldens for M48 (the cell is off in every other suite, so existing goldens don't move). E2E takes screenshots of the cell states and the About block in dark and light and the implementer looks at them. |
| D29 | Settled during T2 (owner's planner, 2026-10-26): `curl_get` sends `Accept` / `X-GitHub-Api-Version` only for the release list (`follow = false`); `SHA256SUMS` and the `.deb` send only `User-Agent`. The cell in `downloading` / `verifying` / `installing` has the `available` tooltip and no class. `phase_after` returns `(Phase::Devel, None)` for `InstallKind::Devel`; `Off` is set by the app. A skipped offer's About row has no `Restart…`. `CONTEXT.md`'s **Update** sits at the end of its section. `Cache::update` is the only write path (no `Cache::save`). |

---

## 3. Text to add to the docs (Task T1 copies these)

### 3.1 PRD (`docs/internals/PRD.md`, after R124, before `## Non-goals (v1)`)

```markdown
## R127 — Updates (M48, ADR-0048)

The owner's request of 2026-10-26: see the version, learn about a newer release, install it from the app, restart when it suits.

1. **Who and why.** RADE ships as a `.deb` on GitHub Releases with no apt repository, so `apt upgrade` never brings a new RADE. Many Agents run in it at once: nothing may interrupt them — no modal, no automatic install, no automatic restart.
2. **Check.** One `GET https://api.github.com/repos/richard-plg/rade/releases?per_page=20` (no token, `User-Agent: rade/<version>`, nothing else sent) 60 s after start and then at most once a day (an hour after a failed check), from the Installed flavor only; `[update] check = false` stops the automatic checks, `RADE_UPDATE_CHECK=0` stops all of them. The answer and the time are cached in `~/.cache/rade/update/update.json`, shared with `rade version`.
3. **Which release.** The highest non-draft release above the running version with `rade_<v>_amd64.deb` and `SHA256SUMS` assets; `[update] channel = "stable"` (default) skips pre-releases, `"rc"` includes them. Versions compare per SemVer (`1.1.0-rc.1 < 1.1.0`, build metadata ignored). Kind: major / minor / patch; security when a release in between lists `### Security` (a `Security: yes` commit footer, git-cliff).
4. **Where it shows.** A Statusline cell only while there is something to say (`1.2.0`, `1.2.0 security`, `2.0.0 major`, `1.2.0 42%`, `restart for 1.2.0`, `update failed`), dropping after the monitor and before ports; one Activity row per version; one Toast only for a security release, held while an Agent waits; **Settings › Updates** with the About block (version, status, last check, Install / Skip This Version / Release notes / Check now) and the rows Check for updates and Update channel; palette **About RADE**, **Check for Updates**, **Restart to Finish Update…**.
5. **Install** (a click, `.deb` installs only): download into the cache with progress and Cancel, SHA-256 equal to `SHA256SUMS` and to the API's asset digest, then `pkexec /usr/bin/apt-get install -y --no-remove <deb>` (the system's password dialog). Cancelling the dialog goes back to Install; a failure says why. The running app keeps working on its loaded files; the new `rade` CLI talks to it.
6. **Restart.** The cell turns `restart for <v>` (also when the owner installed the package by hand). A click asks first, counting running Agents (they stop and come back interrupted through Session Restore) and waiting ones; Restart saves, quits and a helper starts the new binary once the old process is gone. Not now keeps everything running. The first start on the new version shows `Updated to RADE <v>` once.
7. **Other installs.** A Devel build never checks (About: `Development build`). A RADE not installed from the `.deb` is told about updates and gets Download (the release page) instead of Install.
8. **CLI.** `rade version [--check] [--json]`: `rade <cli>`, then `app <v> (pid <n>)` (`· restart RADE to finish updating` when it differs) or `app older than 1.2.0 (no app.info) · restart RADE to finish updating` or `app not running`, then `update <v> available (<kind>[, security]) · checked <ago>` / `update <v> skipped · checked <ago>` / `up to date · checked <ago>` / `not checked yet` / `last check failed: <error> · <ago>` (an offer known from the cache wins over a failed check's line; `<ago>` = `just now` / `5m ago` …). Socket `app.info` gives the app's version and pid. A call that fails with `unknown method` adds `rade: this rade is <cli>, the running RADE app is <v|older>: restart RADE to finish updating`.

AC:
- R127-a [M48] (unit): Given version strings `1.2.0`, `1.1.0`, `1.1.0-rc.1`, `1.1.0-rc.2`, `1.1.0-rc.10`, `v1.1.0`, dpkg `1.1.0~rc.1`, `1.1.0+git202610261200.abc1234`, `1.1`, `x`. When they are parsed and compared. Then `1.1.0-rc.1 < 1.1.0-rc.2 < 1.1.0-rc.10 < 1.1.0 < 1.2.0`, the tag and dpkg forms equal their SemVer forms, the build form equals `1.1.0`, the deb name of `1.1.0-rc.1` is `rade_1.1.0~rc.1_amd64.deb`, and `1.1` and `x` are not versions.
- R127-b [M48] (unit): Given the release-list fixture (1.0.0, 1.0.1, 1.1.0, 1.2.0-rc.1 as pre-release, a 1.3.0 draft, a 1.2.1 without the `.deb`, a 1.2.2 whose `.deb` URL is on another host). When the offer is picked for 1.0.0 on `stable`, for 1.0.0 on `rc`, for 1.1.0 on `stable`. Then it is 1.1.0 (minor) with notes `# What's new since RADE 1.0.0`, `## RADE 1.1.0 · 2026-10-26` before `## RADE 1.0.1 · 2026-10-25` and no `Install:` line; then 1.2.0-rc.1; then none.
- R127-c [M48] (unit): Given the fixture with `### Security` in 1.0.1's body, and a 2.0.0 added. When offers are picked for 1.0.0, 1.0.1 and 1.1.0. Then from 1.0.0 the offer is security, from 1.0.1 it is not, and 2.0.0 from 1.1.0 is major.
- R127-d [M48] (unit): Given no cache, a good check 23 h 59 min ago, 24 h ago, a failed check 59 min and 60 min ago, a broken `update.json`. When `due` is asked. Then due, not due, due, not due, due, due (the broken file reads as empty); a written cache reads back equal and is written through a temp file + rename.
- R127-e [M48] (unit): Given a file, its `SHA256SUMS` line and the asset digest, then a wrong line, a wrong digest, a file missing from `SHA256SUMS`, no digest; and asset URLs on `github.com/richard-plg/rade/releases/download/<tag>/`, on another repo, on `127.0.0.1:<port>` with and without `RADE_UPDATE_URL` on that port. When verified / allowed. Then only the matching file passes (no digest: `SHA256SUMS` alone decides); only the right GitHub URL is allowed, and the local one only with the override.
- R127-f [M48] (unit): Given the Installed flavor at `/usr/lib/rade/rade-app` with a dpkg version, at `~/.cargo/bin/rade-app`, the Devel flavor, the Devel flavor with `RADE_UPDATE_URL`, and `RADE_UPDATE_INSTALL`; install exits 0, 126, 127, 100 with `Could not get lock`, 100 with `E: Broken package`, and `/usr/lib/rade/rade-app (deleted)`. When the install kind, the install outcome and the restart path are worked out. Then `deb`, `other`, `devel`, `other`, `deb`; done, cancelled, `Not authorized to install software.`, `Another package manager is running. Try again in a minute.`, `E: Broken package`; and `/usr/lib/rade/rade-app`.
- R127-g [M48] (unit): Given every phase of §5 with and without security / major / skipped, for kinds `deb` and `other`. When the cell and the About block are built. Then their texts, icons, classes and buttons are §5.1 / D16's, and the cell is absent in `devel`, `off`, `idle`, `checking` (without an offer) and for a skipped offer.
- R127-h [M48] (unit): Given Agent facts with 0 running and 0 waiting, 1 running, 3 running and 5 waiting (`a` … `e`). When the restart body is built. Then it is D20's text for each (`… waiting for you: a, b, c +2. Session Restore reopens them as interrupted.`).
- R127-i [M48] (e2e): Given a fake GitHub serving a release one minor above the running version, `RADE_UPDATE_DELAY=2`. When the app starts. Then after one request (User-Agent `rade/<version>`, no Authorization) the cell reads `<v>`, Activity has `RADE <v> is available`, Settings › Updates shows `RADE <v> is available` with Install and Skip This Version; a relaunch within the day sends no request and shows the cell from the cache; with `[update] check = false` a relaunch sends no request even when due (cache aged 25 h by `debug.update`'s `age` op), the cell still shows from the cache, and Check now sends one; with `RADE_UPDATE_CHECK=0` nothing is sent and Check now is off.
- R127-j [M48] (e2e): Given the fake release marked security and an Agent Pane waiting. When the check finds it, then the Agent stops waiting, then the app relaunches. Then no Toast while it waits, one `RADE <v> fixes a security issue` Toast after, none after the relaunch; Skip This Version hides the cell and survives a relaunch, and a higher release later shows again.
- R127-k [M48] (e2e): Given the fake release with a 2 MB `.deb`. When Install is clicked with the fake installer exiting 0, then (fresh = app stopped, `dpkg-version` back to the running version, `$XDG_CACHE_HOME/rade-dev/update` removed, app started) with a wrong `SHA256SUMS`, with the installer exiting 126, and 1. Then `debug.update`'s `phases` log reads `available, downloading, verifying, installing, restart_pending` and the cell was seen as `<v> <pct>%` and `<v> installing` (slow fake: `slow` file + `install-sleep`) before `restart for <v>` and the installer got the `.deb` path whose SHA-256 matches, the `.deb` is deleted afterwards; the wrong sums show `Download didn't match SHA256SUMS. Nothing was installed.` without calling the installer; 126 returns to Install with no error; 1 shows `update failed` with the installer's last line.
- R127-l [M48] (e2e): Given restart pending with one Agent running and one waiting. When the cell is clicked (`debug.update cell_click`), Not now, then Restart. Then the dialog reads D20's two lines; Not now keeps the same pid; Restart ends the old pid and within 20 s a new app answers `app.info` with another pid, `rade list` shows the same Panes (both Agents `interrupted`), and `rade version` reads `app <v> (pid <new>)`.
- R127-m [M48] (e2e): Given the fake installed-version query printing a version above the running one (a hand install). When a check runs. Then the cell reads `restart for <that version>` with no download.
- R127-n [M48] (e2e): Given no app, then the app, then a fake socket answering `unknown method` to every call. When `rade version`, `rade version --json`, `rade version --check` and `rade list` run. Then the texts of item 8 (`app not running`, `app <v> (pid <n>)`, `update <v> available (minor) · checked just now`), the JSON keys of API.md, exactly one request for `--check` (which then reads `checked just now`), and the `restart RADE to finish updating` line on stderr for the fake socket.
- R127-o [M48] (e2e): Given the Devel build without `RADE_UPDATE_URL`, and a non-`deb` install (no `RADE_UPDATE_INSTALL`, URL set). When Settings › Updates opens. Then `Development build` with no request; then `RADE <v> is available` with Download (opens the release page, `debug.update` records the URI) instead of Install.
- R127-p [M48] (unit+e2e): Given cells Workspace, Worktree, counts, ports, update, identity, a monitor and two hints with widths that fit only after dropping 1, 3, 4 and 5 cells; and in the app the cell in each phase and the About block in `available` / `restart_pending`, dark and light, and the window at 1280 px (`debug.ui layout`) with the cell `available`. When `statusline_fit` runs and screenshots are taken. Then the drop order is hints → monitor → update → ports → identity; in the app at 1280 px `statusline.update` is the cell's text (not dropped, not clipped) and the images are written and looked at.
```

### 3.2 ADR `docs/internals/adr/0048-updates-from-github-releases.md`

```markdown
# 0048 — Updates from GitHub Releases, installed by the app, applied on restart

Status: accepted (2026-10-26)

## Context
RADE ships as a `.deb` on GitHub Releases (ADR-0012) with no apt repository, so nobody learns about a new version and `apt upgrade` never installs one. The app runs many Agents for hours: an update must not interrupt them. On 2026-10-26 the owner upgraded 1.0.0 → 1.1.0 with `pkexec apt-get install` while the app ran: dpkg replaces files by rename, the running process keeps its mapped binary and library, and the new `rade` CLI talks to the old app (the socket contract is stable within a MAJOR, ADR-0041).

## Decision
- The app asks the public GitHub Releases API once a day (no token, no identifier beyond `User-Agent: rade/<version>`), on by default with a line in Settings saying what is sent; `RADE_UPDATE_CHECK=0` and `[update] check = false` turn it off.
- It tells quietly: a Statusline cell and an Activity row; a Toast only for a release marked security (`### Security`, from a `Security: yes` commit footer), never while an Agent waits. No modal.
- Install is one click on a `.deb` install: download, SHA-256 against `SHA256SUMS` and the API's asset digest, then `pkexec apt-get install -y --no-remove` — the system's own authorization; RADE never stores or asks for a password.
- Restart is the owner's: the app keeps running the old version until they click Restart, which asks first (running Agents come back interrupted through Session Restore), quits normally, and a detached helper starts the new binary after the old process is gone (one instance per `state.json`, ADR-0021).
- Other installs are told and sent to the release page; Devel builds never check.

## Not done
An apt repository / PPA, automatic install or restart, delta updates, attestation checks in the app (needs gh ≥ 2.81 or a sigstore client), Flatpak / Snap (non-goal), downgrades, non-amd64 builds.
```

### 3.3 `docs/internals/MILESTONES.md` (append)

```markdown
## M48 — Updates — R127
Owner request and decisions of 2026-10-26 (`PLAN-updates.md`), ADR-0048. A daily check of GitHub Releases (on by default, stable or rc channel), a Statusline cell only when there is news, an Activity row, a Toast only for a security release (held while an Agent waits), Settings › Updates with the About block; Install = download + SHA-256 + `pkexec apt-get install` while the app runs; restart when the owner says, with Session Restore; `rade version [--check] [--json]`, socket `app.info`. Pure logic in `rade-core/src/update.rs`, app `update.rs` + `update_view.rs`, E2E `tests/e2e/m48.sh` with `fake-github.py`, `fake-install.sh`, `fake-dpkg-query.sh`. Checks: docs/internals/checks/M48.md.
```

### 3.4 `CONTEXT.md` (one term, alphabetical place in its section, same format as the others)

```markdown
**Update**:
A newer RADE release on GitHub Releases that RADE found and can install from the app; it takes effect when RADE restarts (R127).
_Avoid_: Upgrade (apt's word for the package step), patch
```

### 3.5 `docs/API.md`

- Under `## Commands`, after `rade setup`:
  ```markdown
  ### `rade version [--check] [--json]`
  1.2.0 (PRD R127, local; works without the app). Prints the CLI's version, the running app's (socket `app.info`) and what the last Update check found (`~/.cache/rade/update/update.json`, shared with the app). `--check` asks GitHub once now (not with `RADE_UPDATE_CHECK=0` or a Devel build without `RADE_UPDATE_URL`: it says why and exits 0). `--json`: `{"cli": "1.2.0", "app": {"version": "1.1.0", "pid": 4242} | {"version": null, "pid": null} (an app older than 1.2.0) | null (not running), "update": {"checked_at": <unix s> | null, "error": <string> | null, "latest": "1.3.0" | null, "kind": "major"|"minor"|"patch" | null, "security": bool, "skipped": "1.3.0" | null}}`. A call that fails with `unknown method` adds a stderr line `rade: this rade is <cli>, the running RADE app is <v|older>: restart RADE to finish updating`.
  ```
- In the socket methods list (where `app.present` is documented, else under Transport): `app.info` → `{"version", "pid", "flavor": "rade"|"rade-dev", "update": "devel"|"off"|"idle"|"checking"|"available"|"downloading"|"verifying"|"installing"|"restart_pending"|"failed"}` (1.2.0).
- Under `rade debug …`: §6's `debug.update`.
- A new section `## Update variables` right after `## Environment injected into every Terminal`: the five `RADE_UPDATE_*` variables of D27, what each does, that all but `RADE_UPDATE_CHECK` work only in a Devel build, and that the app removes them from its environment (Terminals never see them).

---

## 4. Architecture — files

### New
- `crates/rade-core/src/update.rs` (+ `update_tests.rs`, `#[cfg(test)] #[path = "update_tests.rs"] mod tests;` like the other `_tests` files): everything in §T2.
- `tests/fixtures/update/releases.json`: the R127-b list (real API shape: copy the field names and nesting of the 2026-10-26 answer; bodies like the real ones, including the `Install:` last line). `releases-security.json`: the same with `### Security\n\n- Fix …` in 1.0.1's body and a 2.0.0 release added.
- `crates/rade-app/src/update.rs`: the controller (timer, check worker, download / verify / install, restart helper, cache writes, Activity rows, Toast, `debug.update`).
- `crates/rade-app/src/update_view.rs`: the Statusline cell widget, the About block, the Restart dialog.
- `crates/rade-app/assets/icons/ph-arrow-circle-up-symbolic.svg`, `ph-arrow-clockwise-symbolic.svg` (verbatim from `https://unpkg.com/@phosphor-icons/core@2.1.1/assets/regular/<name>.svg`, as the other `ph-*` icons).
- `crates/rade-cli/src/version.rs`: `rade version`.
- `tests/e2e/m48.sh`, `tests/e2e/fake-github.py`, `tests/e2e/fake-install.sh`, `tests/e2e/fake-dpkg-query.sh`.
- `docs/features/updates.md`, `docs/internals/adr/0048-updates-from-github-releases.md`, `docs/internals/checks/M48.md`.

### Changed
- `rade-core`: `lib.rs` (`pub mod update;`), `config.rs` (`UpdateConfig`, `APP_WIDE`), `settings.rs` (category + 2 rows), `paths.rs` (`update_dir`), `chrome.rs` (`CellKind::Update`, `statusline_fit` order), `activity.rs` (`Kind::Update`, `Event::update`), `palette.rs` (3 actions).
- `rade-app`: `main.rs` (`mod update; mod update_view;`, start in `build`), `chrome.rs` (cell in the Statusline, `fit`, `info`, `after_update` hook), `settings_view.rs` (About block in the Updates section, `open_at`), `keys.rs` (3 actions), `handlers.rs` (`app.info`, `debug.update`), `activity_view.rs` (`kind_str`, mark icon), `style.css` (§5.3).
- `rade-cli`: `main.rs` (`Version` command, the unknown-method hint in `call_at`'s callers — §T4).
- `cliff.toml`, `docs/RELEASING.md` (D8), `tests/e2e/lib.sh` (D27), `.github/workflows/ci.yml` (`m48` in shard `a`), `CLAUDE.md`, `docs/API.md`, `docs/features/settings-and-config.md` (Updates rows), `docs/getting-started.md` (one line under install: "RADE tells you about new versions and installs them: see features/updates.md").

---

## 5. UI specification

### 5.1 About block (top of Settings › Updates)

**One persistent widget** owned by `update_view` (built once, updated in place on every phase / progress change — the 250 ms progress ticks never call `settings_view::render()`); `render()` only re-appends it after the `Updates` section head. A `gtk::Box` vertical, class `settings-about`, spacing 8, margins like a Settings row. Line 1: a horizontal box with the label `RADE <current version>` (class `settings-about-title`, the same font as `settings-section` but `fg`, 15 px, weight 600) and, right-aligned, a status label (class `settings-about-status` + one of `ok` / `wait` / `err` / `acc` / none). Line 2: the sub label (class `settings-about-sub`, `fg3`, 12 px, wraps). Line 3 (only while downloading): a `gtk::ProgressBar` (class `settings-about-progress`). Line 4: a horizontal box of buttons (spacing 8) — the primary one has `.suggested-action`, the others are flat — and, right-aligned, a flat link button `Release notes ›` (only when there is an offer). Line 5: the privacy line (class `settings-about-privacy`, `fg3`, 11 px): `RADE asks api.github.com for the release list once a day. It sends its version in the User-Agent and nothing else.`

Texts, from core `update::about(&Status) -> AboutView` (exact):

| Phase / condition | Status label (class) | Sub label | Buttons (primary first) |
|---|---|---|---|
| `devel` | `Development build` (none) | `Built from source: updates are off.` | — |
| `off` | `Update checks are off` (none) | `RADE_UPDATE_CHECK=0 is set.` | — |
| `idle` with `[update] check = false` (replaces the three `idle` rows below) | `Automatic checks are off` (none) | `Last checked <ago>.` / `Never checked.` | `Check now` |
| `idle`, never checked | `Not checked yet` (none) | `RADE checks a minute after it starts.` | `Check now` |
| `idle`, up to date | `Up to date` (ok) | `Checked <ago> · <Stable releases \| Release candidates too>.` | `Check now` |
| `idle`, last check failed | `Couldn't check` (wait) | `<error> · tried <ago>.` | `Check now` |
| `checking` | `Checking…` (none) | as before the check | `Check now` (insensitive) |
| `available` | `RADE <v> is available` (acc; wait when security or major) | security: `Fixes a security issue.`; major: `<v> changes the rade CLI, socket or config files: read the notes first, scripts and Agent skills may need changes.`; minor: `New features and fixes.`; patch: `Fixes.` — kind `other` appends ` Installed outside the .deb: download it from the release page.` | deb: `Install <v>`, `Skip This Version`; other: `Download`, `Skip This Version` |
| `available`, skipped | `RADE <v> skipped` (none) | `You skipped this version. RADE tells you again when a newer one comes out.` | deb: `Install <v>`; other: `Download` |
| `downloading` | `Downloading <v>… <pct>%` (none) | `<downloaded MB> of <size MB> MB` (one decimal) | `Cancel` |
| `verifying` | `Checking the download…` (none) | `SHA-256 against SHA256SUMS.` | — |
| `installing` | `Installing <v>…` (none) | `Approve the system dialog to continue.` | — |
| `restart_pending` | `RADE <v> is installed` (acc) | `Restart RADE to finish. This window keeps running <current> until then.` | `Restart…` |
| `failed` | `Update failed` (err) | `<error>` | `Try again` (= Install again), `Skip This Version` |
| `available` while the installed version is also above the running one (D15) | as `available` | as `available`, then ` RADE <installed> is installed already: restart to use it.` | as `available`, then `Restart…` |

`<ago>` = core `update::ago(secs)`: `just now` under 60 s, else `<design::age(secs)> ago` (`5m ago`, `2h ago`, `3d ago`) — not `activity::ago`, which says `now` / `5m`. Sizes in MB = bytes / 1,000,000, one decimal.

Button actions: `Check now` → `update::check_now()` (insensitive in the phases of D3's "no check" list); `Install <v>` / `Try again` → `update::install()`; `Download` → `update::launch(html_url)` (D17); `Skip This Version` → `update::skip()`; `Cancel` → `update::cancel()`; `Restart…` → the Restart dialog (D20); `Release notes ›` → D19.

### 5.2 Settings rows (category `Updates`)

| key | label | description | kind |
|---|---|---|---|
| `update.check` | `Check for updates` | `Once a day, ask GitHub whether a newer RADE is out. Off: only Check now asks.` | `Bool` |
| `update.channel` | `Update channel` | `Stable releases, or release candidates too (rc).` | `Choice(&["stable", "rc"])` |

Changing either re-renders the About block; switching the channel recomputes the offer from the cached releases (no request).

### 5.3 CSS (`style.css`, tokens only — `tokens_tests.rs` forbids raw hex)

```css
.status-cell-update { padding: 0 8px; min-height: 0; }
.status-cell-update.wait label, .status-cell-update.wait image { color: @wait; }
.status-cell-update.err label, .status-cell-update.err image { color: @err; }
.status-cell-update.acc label, .status-cell-update.acc image { color: @acc; }
.status-cell-update image { -gtk-icon-size: 13px; }
.settings-about { padding: 12px 16px; border-radius: 8px; background: @bg2; }
.settings-about-title { font-size: 15px; font-weight: 600; color: @fg; }
.settings-about-status.ok { color: @ok; }
.settings-about-status.wait { color: @wait; }
.settings-about-status.err { color: @err; }
.settings-about-status.acc { color: @acc; }
.settings-about-sub { font-size: 12px; color: @fg3; }
.settings-about-privacy { font-size: 11px; color: @fg3; }
```
Tokens are GTK named colors (`@wait`, as in `.status-cell-identity.wait` and `.status-voice`); `fg`, `fg3`, `bg2`, `acc`, `wait`, `ok`, `err` exist in both `[dark]` and `[light]` of `tokens.toml`. The cell's label and icon otherwise inherit the `.status-cell` rules like the other cells.

---

## 6. Debug API (test mode only, ADR-0021)

`debug.update {"op", "arg"?}` → every op answers `info` afterwards:
- `info` → `{"phase", "current", "install": "deb"|"other"|"devel", "enabled", "checked_at", "error", "installed": "<version>" | null, "offer": {"version", "kind", "security", "skipped", "html_url"} | null, "phases": [..every phase `as_str` entered since start, in order, consecutive repeats collapsed..], "cell": {"text", "icon", "classes", "tooltip"} | null, "about": {"status", "status_class", "sub", "buttons": [..], "notes": bool}, "toasts": [..texts raised by update..], "launched": [..URIs passed to launch_uri by update..], "requests": <n sent by this app>, "download": {"path", "bytes", "size"} | null, "installer": {"argv", "exit"} | null}`.
- `age` (`arg` = seconds: moves the cache's `checked_at` back by that much and writes it), `tick` (runs the hourly tick now), `check_now`, `install`, `cancel`, `skip`, `cell_click` (= a click on the Statusline cell), `restart_dialog` (opens D20 without clicking the cell), `dialog` (→ `{"open", "heading", "body", "responses"}` of the Restart dialog), `respond` (`arg` `not_now`|`restart`: answers the open Restart dialog; for `restart` it replies with `info` first and runs the restart in `glib::idle_add_local_once`, as `debug.quit` does — nothing answers after that), `notes` (= Release notes ›), `download` (= the Download button), `about_click` (`arg` = a button label of the About block).
- `debug.ui chrome` → `statusline.update` (inside `Statusline::info`): the cell's text, or null when absent or dropped by the fit. `debug.ui app_settings` gains `"category"` (D18).

---

## 7. Tasks

Order (one implementer each, sequential; each starts from the previous task's commit):
```
T1 docs ─► T2 core ─► T3 app ─► T4 CLI ─► T5 E2E ─► T6 docs + gate
```

### T1 — Spec docs
Files: `docs/internals/PRD.md`, `docs/internals/adr/0048-updates-from-github-releases.md`, `docs/internals/MILESTONES.md`, `CONTEXT.md`, `cliff.toml` (D8), `docs/RELEASING.md` (a bullet under "Commits and the changelog": `` A footer `Security: yes` lists the commit under **Security**: RADE raises a Toast for a release with a `### Security` group (R127). ``).
Do: paste §3.1–§3.4 verbatim. Check: `scripts/trace.sh --no-write` runs; `git cliff --unreleased --strip all` still runs (if git-cliff is installed; else say so).
Commit: `docs(M48): R127, ADR-0048, security changelog group`

### T2 — Core `update.rs`, config, settings, paths, chrome, activity, palette (tests first)
`config.rs`: D26. `settings.rs`: `CATEGORIES` += `"Updates"` after `"Statusline"`; rows of §5.2. `paths.rs`: `pub fn update_dir(env: Env) -> PathBuf` = `$XDG_CACHE_HOME` (else `~/.cache`) `/<flavor name>/update` (copy `browser_dirs_of`'s form, with a `_of(env, flavor)` twin and a unit test). `chrome.rs`: `CellKind::Update` (`"update"`), `statusline_fit` order `[Hint, Monitor, Update, Ports, Identity]` (extend the existing fit test). `activity.rs`: `Kind::Update`, `Event::update(text, at)` (actor `RADE`). `palette.rs` `ACTIONS` += `("about", "About RADE")`, `("check_updates", "Check for Updates")`, `("restart_update", "Restart to Finish Update…")` after `("open_settings", "Settings")`.

`update.rs` API (names exact):
```rust
pub const REPO_API: &str = "https://api.github.com/repos/richard-plg/rade/releases?per_page=20";
pub const DOWNLOAD_PREFIX: &str = "https://github.com/richard-plg/rade/releases/download/";
pub const CHECK_EVERY_S: u64 = 86_400;
pub const RETRY_AFTER_S: u64 = 3_600;
pub const INSTALLED_EXE: &str = "/usr/lib/rade/rade-app";

pub struct Version { major: u64, minor: u64, patch: u64, pre: Vec<Ident> }   // Ord, Display ("1.1.0-rc.1"), no build
impl Version { pub fn parse(s: &str) -> Option<Version>; pub fn from_tag(t: &str) -> Option<Version>;
               pub fn from_deb(s: &str) -> Option<Version>; pub fn deb_name(&self) -> String; pub fn is_pre(&self) -> bool; }
pub enum Kind { Major, Minor, Patch }                    // as_str "major"|"minor"|"patch"
pub struct Asset { pub name: String, pub url: String, pub size: u64, pub digest: Option<String> }  // digest = hex without "sha256:"
pub struct Release { pub version: Version, pub tag: String, pub published: String /* YYYY-MM-DD */,
                     pub html_url: String, pub body: String, pub prerelease: bool, pub deb: Asset, pub sums: Asset }
pub fn parse_releases(json: &str, base: Option<&str> /* RADE_UPDATE_URL */) -> Result<Vec<Release>, String>; // candidates only (D5 except the channel, D13)
pub enum Channel { Stable, Rc }
pub struct Offer { pub release: Release, pub kind: Kind, pub security: bool, pub skipped: bool, pub notes: String }
pub fn pick(releases: &[Release], floor: &Version, current: &Version, channel: Channel, skipped: Option<&str>) -> Option<Offer>; // above `floor` (D5); kind / security / notes vs `current` (D7)
pub struct Cache { pub checked_at: Option<u64>, pub error: Option<String>, pub releases: Vec<Release>,
                   pub skipped: Option<String>, pub toasted: Option<String>, pub just_installed: Option<String> }
impl Cache { pub fn load(dir: &Path) -> Cache; pub fn update(dir: &Path, f: impl FnOnce(&mut Cache)) -> std::io::Result<Cache>; } // load → f → save via update.json.<pid>.tmp + rename (D9); no other write path
pub fn due(cache: &Cache, now: u64) -> bool;
pub fn curl_get(url: &str, user_agent: &str, follow: bool, override_url: bool) -> Result<(u16, String), String>;   // D2
pub fn ago(secs: u64) -> String;                          // §5.1
pub fn installed_from_query(stdout: &str) -> Option<Version>;   // D14 (`ii ` prefix)
pub fn phase_after(cache: &Cache, current: &Version, installed: Option<&Version>, kind: InstallKind, channel: Channel) -> (Phase, Option<Offer>); // D5, D14, D15
pub fn spawn_restart(pid: u32, exe: &Path) -> std::io::Result<()>;   // D21 (libc::setsid + process::no_inherit in pre_exec)
pub fn check_error(status: u16, body: &str) -> String;   // 403/429 → "GitHub rate limit reached", other → "GitHub answered <status>"; curl errors pass through
pub fn sha256_file(path: &Path) -> std::io::Result<String>;
pub fn sums_entry(sums: &str, name: &str) -> Option<String>;    // "<hex>  <name>" lines, also "<hex> *<name>"
pub fn verify(path: &Path, sums: &str, asset: &Asset) -> Result<(), String>;     // D11 (3) text
pub fn asset_allowed(url: &str, tag: &str, name: &str, base: Option<&str>) -> bool;  // D13
pub enum InstallKind { Deb, Other, Devel }               // as_str
pub fn install_kind(devel: bool, exe: &Path, installed: Option<&Version>, override_install: bool, override_url: bool, arch: &str) -> InstallKind; // overrides count only when devel (D4)
pub fn restart_exe(current_exe: &Path) -> PathBuf;
pub fn install_argv(deb: &Path, override_cmd: Option<&str>) -> Vec<String>;      // D12
pub enum InstallOutcome { Done, Cancelled, Failed(String) }
pub fn install_outcome(code: i32, stderr: &str) -> InstallOutcome;
pub enum Phase { Devel, Off, Idle, Checking, Available, Downloading { pct: u8, bytes: u64 }, Verifying, Installing, RestartPending(Version), Failed(String) } // as_str per D15
pub struct Status { pub phase: Phase, pub current: Version, pub installed: Option<Version>, pub install: InstallKind, pub checked_at: Option<u64>,
                    pub error: Option<String>, pub offer: Option<Offer>, pub channel: Channel, pub auto: bool /* [update] check */, pub now: u64 }
pub struct CellView { pub icon: &'static str, pub text: String, pub class: Option<&'static str>, pub tooltip: String }
pub fn cell(s: &Status) -> Option<CellView>;              // D16
pub struct AboutView { pub title: String, pub status: String, pub status_class: Option<&'static str>, pub sub: String,
                       pub buttons: Vec<String> /* labels, primary first */, pub primary_enabled: bool, pub notes: bool, pub progress: Option<f64> }
pub fn about(s: &Status) -> AboutView;                    // §5.1 table
pub const PRIVACY: &str = "RADE asks api.github.com for the release list once a day. It sends its version in the User-Agent and nothing else.";
pub fn restart_body(facts: &[crate::overview::Fact], version: &Version) -> String;  // D20, lines joined with "\n\n"
pub fn toast_text(offer: &Offer) -> Option<String>;      // D17: Some only when security
pub fn activity_text(phase: &Phase, offer: Option<&Offer>) -> Option<String>;     // D23
pub fn version_lines(cli: &str, app: Option<(Option<&str>, Option<u32>)>, cache: &Cache, current: &Version, channel: Channel, now: u64) -> Vec<String>; // §3.1 item 8 (for T4)
```
Serde: `Cache` and `Release` / `Asset` serialize with `Version` as its string. Tests `r127_a_…` … `r127_h_…` and the unit half of `r127_p_…` (extend the existing `statusline_fit` test in `chrome.rs`), one fn per AC, tables inside, on the fixtures of §4; also `phase_after` (offer + error → `available`; installed above running with no higher offer → `restart_pending`; installed 1.2.0 and a 1.3.0 offer → `available` with `Restart…`), `installed_from_query` (`ii 1.1.0`, `rc 1.0.0`, empty), `Cache::update` (two writers changing different fields keep both) and `spawn_restart` (with `/bin/true` as `exe` and a pid that is already gone: the helper exits 0 at once). `curl_get` gets no unit test (it is exercised by m48).
Commit: `feat(R127): update model — versions, release choice, cache, verify, texts` / `Changelog: skip`

### T3 — App: controller, Statusline cell, About block, Restart dialog, socket
- `update.rs` (app): a thread-local `Controller { status: Status, cache: Cache, download: Option<Download>, deb_ready: Option<PathBuf>, requests: u32, toasts: Vec<String>, launched: Vec<String>, installer: Option<(Vec<String>, Option<i32>)>, logged: HashSet<String>, phases: Vec<&'static str> }`. `cache` is only a copy for the views: every write goes through `Cache::update` (D9) and its result replaces the copy.
  - `pub fn start()` (called at the end of `build` in `main.rs`): read env (D27), config, `Cache::load`, the installed-version query (D14) **off the UI thread** (`worktrees::in_background`), work out `InstallKind`, set the phase; if `just_installed == current` → D17's `Updated to RADE <v>` Toast and clear it; schedule the first check after `RADE_UPDATE_DELAY` and the hourly timer (D3).
  - `check(manual: bool)`: refused per D3's busy list; phase `checking`; off-thread `curl_get(url, "rade/<v>", false, ..)` + `parse_releases`; back on the UI thread: `Cache::update` (`checked_at = now`, and `error` = the D2 error text or `releases` = the answer with `error = None`), then the installed-version query, then `phase_after`, Activity rows (D23), `maybe_toast()`, re-render (cell + About block).
  - `tick()` (hourly, and `debug.update tick`): re-read the cache, run the installed-version query, `phase_after`, then `check(false)` when due and not busy.
  - `install()`, `cancel()`, `skip()` per D11–D12, D22. Download child polled with `glib::timeout_add_local(250 ms)`; verify and install off-thread; installer stdin null, stderr captured.
  - `maybe_toast()` per D17 (`app.chrome.toast(text, Some(("Details", Box::new(|| settings_view::open_at("Updates")))))`), recording `toasted` in the cache.
  - `restart_dialog()` and `restart()` per D20–D21 (`rade_core::overview::facts(&model, &|_| String::new())`; the helper through core `update::spawn_restart`, then `app.quit()`).
  - `pub fn launch(uri: &str)` per D17.
  - `pub fn info() -> Value` for `app.info` (D25) and `pub fn debug(p: &Value) -> Result<Value>` (§6).
  - React to config changes: in `keys.rs` `apply_config`, next to the `system_monitor` comparison, add `if old.update != app.config.borrow().update { crate::update::config_changed(); }` (`config_changed` recomputes the phase / offer from the cache and re-renders; no request).
- `update_view.rs`: `pub struct UpdateCell { pub button: gtk::Button, icon: gtk::Image, label: gtk::Label, last: RefCell<Option<CellView>> }` with `new()` and `set(&self, v: Option<CellView>)` (updates icon, label, classes, tooltip, visibility in place; then the D16 refit); `pub fn about_block() -> gtk::Widget` returning the one persistent About widget (§5.1) and `pub fn render_about(v: &AboutView)` updating it in place (buttons rebuilt only when their labels change); the Restart dialog.
- `chrome.rs`: an `update: UpdateCell` field in `Statusline`, its button in the root after `left`, before `monitor` (D16); `fit()` pushes the button and a `CellKind::Update` cell before the monitor when `update.last` is `Some` (D16); `Statusline::info()` gains `"update"` (§6); `after_update` calls `crate::update::maybe_toast()`; a `pub fn update_cell(&self) -> &UpdateCell` accessor on `Chrome` for `update.rs`.
- `settings_view.rs`: in `render()`, right after the `Updates` section head, append `update_view::about_block()`; add `open_at` (D18).
- `keys.rs` `run_action`: `"about" => settings_view::open_at("Updates")`, `"check_updates" => update::check_now()`, `"restart_update" => update::restart_dialog()` (no pending restart → Toast `No update is waiting for a restart`).
- `handlers.rs`: `"app.info" => Ok(crate::update::info())` next to `app.present`; `"debug.update" => crate::update::debug(&p)` next to `debug.voice`.
- `main.rs`: read the five `RADE_UPDATE_*` variables into `update`'s settings before `scrub_inherited_agent_session_env()`, and add them to that scrub (D27).
- `activity_view.rs`: `Kind::Update` → `"update"`, mark icon `ph-arrow-circle-up-symbolic`.
- Icons of §4, CSS of §5.3.
Check: build, clippy, tests; then by hand in a headless Weston (Devel build) with `RADE_UPDATE_URL` at a quick `python3 -m http.server` serving the fixture, `RADE_UPDATE_DELAY=2`, `RADE_UPDATE_INSTALL=/bin/true`, `RADE_UPDATE_DPKG_QUERY='echo ii 0.0.0'`: screenshot the cell and Settings › Updates in dark and light (`rade debug screenshot`) and **look at them**.
Commit: `feat(R127): update check, install and restart from the app` with footer `Changelog: RADE tells you when a new version is out (Statusline, Settings › Updates), installs the .deb for you while it keeps running, and restarts with your session when you say so.`

### T4 — CLI `rade version` and the unknown-method hint
- `version.rs`: `rade version [--check] [--json]` per D24 / §3.5, using `update::version_lines`, `Cache::load(paths::update_dir(..))`, `update::curl_get` + `parse_releases` + `Cache::update` for `--check`, and `app.info` through `call_at` with a 2 s timeout (`NotRunning` → app `null`; `unknown method` → `{"version": null, "pid": null}`). The overrides of D4 apply only when the CLI binary is the Devel flavor (`paths::Flavor::current()`); `--check` writes through `Cache::update`.
- `main.rs`: `/// Show the CLI's, the running app's and the update's versions (local).` `Version { #[arg(long)] check: bool }` — `--json` is the existing global `cli.json` flag. Dispatch it in `main()`'s `match &cli.cmd` like `Cmd::Setup` (`Cmd::Version { check } => version::run(*check, cli.json)`, which prints itself and returns `Ok(Value::Null)`), and add `Cmd::Version { .. }` to the `matches!` list of commands that print nothing more. The D25 hint goes in `main()`'s final `Err(e)` arm: after `eprintln!("rade: {e}")`, if `e.to_string()` contains `unknown method '` call `version::app_hint()` (one `app.info` call with a 2 s timeout through `call_at`) and print its line when it returns one; the exit code stays `e.kind.exit_code()`.
Tests: unit tests in `version.rs` for the text and JSON from a given cache (`r127_n_version_texts`).
Commit: `feat(R127): rade version shows the CLI, app and update versions` with footer `Changelog: rade version prints the CLI's and the running app's versions and whether an update is out (--check, --json).`

### T5 — E2E `tests/e2e/m48.sh`
Preamble from `m46.sh` (lib.sh, Weston at 1440, private XDG). Before sourcing: `export RADE_UPDATE_CHECK=1`. The fakes:
- `fake-github.py <port-file> <log> <dir>`: `GET /repos/richard-plg/rade/releases` → `<dir>/releases.json` read fresh on each request (`500` if `<dir>/fail` exists); `GET /download/<file>` → `302` to `/assets/<file>` (like GitHub); `GET /assets/<file>` → `<dir>/<file>`, sent in 64 KB chunks every 100 ms when `<dir>/slow` exists; log line per request `<n> <method> <path> ua=<User-Agent> auth=<yes|no>`.
- `fake-install.sh "$1"`: appends `<path> <sha256 of the file>` to `$RADE_E2E_T/install.log`; sleeps `$(cat $RADE_E2E_T/install-sleep 2>/dev/null || echo 0)` s; `cat $RADE_E2E_T/install-stderr >&2` when that file exists; exits `$(cat $RADE_E2E_T/install-exit 2>/dev/null || echo 0)`; on 0 first copies `$RADE_E2E_T/install-version` to `$RADE_E2E_T/dpkg-version`.
- `fake-dpkg-query.sh`: prints `ii <contents of $RADE_E2E_T/dpkg-version>` when that file is non-empty, nothing otherwise.
- The test writes `releases.json` with python from the running version (`target/release/rade-app --version`): next minor `X.(Y+1).0`, its `.deb` = 2 MB of `head -c` random bytes, `SHA256SUMS`, `digest`, URLs `http://127.0.0.1:<port>/download/<name>`, bodies with `### Added` and the `Install:` line.
- Env, **exported** in m48.sh so both the app and `rade version` see them: `RADE_UPDATE_URL=http://127.0.0.1:<port>/repos/richard-plg/rade/releases?per_page=20`, `RADE_UPDATE_DELAY=2`, `RADE_UPDATE_INSTALL="$ROOT/tests/e2e/fake-install.sh"`, `RADE_UPDATE_DPKG_QUERY="$ROOT/tests/e2e/fake-dpkg-query.sh"`; `$RADE_E2E_T/dpkg-version` starts as the running version.
- Agents: `tests/e2e/fake-agent.sh` as in m2 (one set `running`, one `waiting` with `rade status`).
Checks in AC order R127-i … R127-p **except R127-l, which runs last** (the restarted app runs without test mode and without the `RADE_UPDATE_*` env, D27), one `check "R127-x …"` each, through `debug.update`, `debug.ui chrome`, `debug.ui app_settings*`, `rade version`, the fake's log; R127-l: before `respond restart`, read the panes (`rade list`) and the pid (`app.info`); after it, expect nothing more from that call, wait ≤ 20 s for `app.info` with a new pid (use `rade version --json`), compare `rade list`, then `kill <new pid>`, poll until `kill -0` fails and `$XDG_RUNTIME_DIR/rade/rade-dev.sock` is gone (≤ 10 s), and set `APP_PID=` so `cleanup` doesn't touch it. R127-p's 1280 px part uses `debug.ui layout` like m40. R127-n's fake socket: a 10-line python Unix-socket server answering every request with the `unknown method` error, pointed to by `RADE_SOCKET`. Screenshots `target/e2e-m48/{cell-available,cell-security,cell-restart,cell-failed,about-available,about-restart}-{dark,light}.png`; look at each.
Add `m48` to `.github/workflows/ci.yml` shard `a`, `RADE_UPDATE_CHECK` to `lib.sh` (D27).
Run: `m48.sh`, `m35.sh`, `m37.sh`, `settings.sh`, `m40.sh` (`MODES=1280`).
Commit: `test(R127): m48 suite with a fake GitHub, installer and dpkg`

### T6 — Docs and gate
- `docs/features/updates.md` (new): what you see (cell states, Settings › Updates, the Toast), Install (what runs, the polkit dialog, what is verified), restart and Session Restore (running Agents come back interrupted), channels, turning it off, what is sent, other installs, `rade version`, the env variables.
- `docs/features/settings-and-config.md` (Updates rows), `docs/getting-started.md` (one line), `docs/API.md` (§3.5), `CLAUDE.md` (the m48 command line in Commands; code-map entries for `update.rs` core, app `update.rs` + `update_view.rs`, CLI `version.rs`).
- `scripts/trace.sh --strict` → every R127 AC traced.
- `docs/internals/checks/M48.md`: one row per AC and exit criterion: command, result, screenshot paths; open item: the owner's real run (§8.5).
- Full local CI (memory: docker check + all E2E shards + deb/m10) — report results.
Commits: `docs(R127): updates user guide` (`Changelog: skip`), `docs(M48): checks`.

---

## 8. Exit criteria (M48 done)
1. Every AC of R127 has a passing test (`scripts/trace.sh --strict` green).
2. `cargo clippy --workspace --all-targets` clean, `cargo test --workspace` green.
3. m48, m35, m37, m40 (1280), settings green locally; CI shards green.
4. Screenshots of T5 looked at in dark and light; no clipped text at 1280 px.
5. `docs/internals/checks/M48.md` records all of the above.
6. Owner (after the next release, e.g. 1.2.0, is published): on the installed 1.1.x the cell shows the new version within a minute of start, Install shows the polkit dialog and upgrades while Agents run, Restart brings the session back on the new version, `rade version` agrees.
