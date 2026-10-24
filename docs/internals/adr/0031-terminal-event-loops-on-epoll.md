# Terminal event loops on epoll, not io_uring

Status: accepted 2026-10-06 (M30, PRD R89). Supersedes the M29 hypothesis in docs/checks/M29.md ("a fork of a 65 GB process is refused").

New Terminals failed with Ghostty's `error initializing surface err=error.SystemResources` (m19 failed 4 checks locally, 7 surfaces each run; the owner saw it too). M29 blamed `fork()` of a process with a ~65 GB writable mapping. M30 measured it:

- An error return trace (Ghostty built Debug in a scratch prefix, the trace dumped where `ghostty_surface_new` logs the error) puts the failure in `Surface.init` → `renderer.Thread.init` → `xev.Loop.init` → `IoUring.init` → `io_uring_setup` = `ENOMEM`, not in `fork()` (a terminal's process is started later, on its IO thread).
- io_uring rings are charged to the **user's** locked-memory budget (`RLIMIT_MEMLOCK`, 8 MB here, hard limit), shared by every process of that user. A 256-entry ring costs ~32 KB, so ~250 rings per user. libxev picks io_uring when available; Ghostty makes 2 rings per Terminal (renderer and IO loops) plus 2 at start. The owner's running v0.6.0 app held 76 rings; with it running, a probe process could create only 58 more before `ENOMEM`. A test app with ~30 Terminals ran out.
- The 65 GB mapping is JavaScriptCore's Gigacage (64 GB) and StructureID heap (4 GB) plus its 1 GB JIT pool, reserved at start because WebKitGTK initialises JSC in the UI process. Both are `MAP_NORESERVE` (`nr` in smaps VmFlags), Rss 0, not committed: a process holding 3 × 64 GB `MAP_NORESERVE` forks fine, while accounted (reserved) mappings over RAM + swap make `fork()` fail. They inflate VmData / VmSize only.

## Decision

1. **RADE's Ghostty uses libxev's epoll backend.** `ghostty_init` (where libxev probes io_uring with a 256-entry ring) runs with the soft `RLIMIT_MEMLOCK` set to 0 (`rade_core::process::without_locked_memory`), so the probe fails and every Ghostty loop uses epoll; the limit is restored right after, before any Terminal's process starts. Ghostty's own `async-backend = epoll` setting does the same in its GTK app, and Ghostty documents no meaningful difference between the backends. The embedded API has no backend switch, and the Ghostty fork is not ours to patch.
2. **The JSC reservations stay.** They cost no memory, and the Gigacage is a hardening of the Browser Pane's WebKit (disabling it with `GIGACAGE_ENABLED=0` would reach the web processes through the environment).
3. A test asserts the app holds no io_uring rings with several Terminals open, and that many Terminals all start (`tests/e2e/m30.sh`).

## Considered Options

- **Spawn without forking the big process (a spawn helper, `posix_spawn`)**: not the failure; fork was never refused here. Ghostty's `Command` needs a pre-exec hook (setsid, controlling terminal), so it would also need a Ghostty change.
- **Smaller rings or a raised memlock limit**: the hard limit is not the app's to raise, and smaller rings only move the ceiling.
- **Patch Ghostty to read `async-backend` in the embedded runtime**: the right upstream change; it needs a fork we control. Revisit if the submodule moves to one.

## Consequences

- RADE no longer uses the user's locked-memory budget at all (0 rings, was 2 + 2 per Terminal), so many Terminals across RADE instances, and other io_uring users, no longer starve each other.
- An app started before this change keeps its rings until it restarts.
- Also fixed on the way: `ghostty_config_load_string` was given names without a directory (`rade-default-theme`), which Ghostty unwraps with `.?` (undefined behaviour in ReleaseFast, a panic in Debug); the names are now absolute (`/rade-default-theme`).
