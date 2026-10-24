# bench

R4 performance numbers. Needs a Wayland session, a release build (`cargo build --release`) and the Ghostty fork built (`ghostty/zig-out/lib`).

## throughput.sh

```sh
bench/throughput.sh [runs=3] [size_mb=100]
```

Generates a random base64 text file (100-column lines), then for each run:
- (a) `ghostty -e sh -c '<inner>'`: the system Ghostty (GTK apprt).
- (b) `rade-app` (throwaway state), the command in a right split via `rade spawn --split right --cmd 'sh -c <inner>'`.

`<inner>` writes `date +%s%N` before and after `cat file`, so the number is the time to push the file through the PTY and parser. App startup is not counted. The script kills each app when the end timestamp appears.

Caveats: the two binaries use different Ghostty versions (system 1.3.1 vs fork `a687fd69e`) and different grid sizes (full window vs half-width pane). Both read the same `~/.config/ghostty/config`.

## Results

| Date | Machine | Size | ghostty 1.3.1 | rade-app (fork) |
|---|---|---|---|---|
| 2026-09-30 | Arrow Lake + RTX 5080 (display on NVIDIA), GNOME Wayland | 100 MB | 780–786 ms (~127 MB/s) | 380–398 ms (~255 MB/s) |

## Idle / hidden rendering

`RADE_LOG_RENDERS=1 target/release/rade-app` prints draws per second per Pane (M0 used the spike's `RADE_SPIKE_LOG_RENDERS`). See `docs/checks/M0.md` rows 5 and 7.
