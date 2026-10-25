# Contributing to RADE

Thanks for helping. This page is for people who build RADE from source, fix a
bug or add a feature. Using RADE is covered in [docs/](docs/README.md).

## Build from source

RADE targets Ubuntu 26.04 on GNOME Wayland. You need Rust (1.97+), Zig 0.16,
gettext and the GTK stack:

```sh
sudo apt install build-essential pkg-config gettext git \
  libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libgtksourceview-5-dev \
  libgl-dev libegl-dev libepoxy-dev
```

Then:

```sh
git submodule update --init                                          # Ghostty fork
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast -Dcpu=baseline)  # → ghostty/zig-out/lib
cargo build --release
target/release/rade-app
```

A build run from `target/` is the **dev flavor**: its window is titled
"RADE (dev)" and it has its own socket (`rade-dev.sock`) and state
(`~/.local/share/rade-dev`), so it never touches an installed RADE. The config
file `~/.config/rade/config.toml` is shared. Drive the dev app with
`target/release/rade`, not the installed `rade`.

`cargo install --path crates/rade-cli && cargo install --path crates/rade-app`
also works; the binaries keep loading libghostty from the clone's
`ghostty/zig-out`, so don't delete it.

### Build the .deb

```sh
scripts/package-deb.sh                              # → target/debian/rade_<version>_amd64.deb
sudo apt install ./target/debian/rade_*_amd64.deb
```

Packaging also needs `dpkg-dev`, `binutils`, `patchelf` (optional), `python3-gi`,
`gir1.2-gdkpixbuf-2.0` and `librsvg2-common`. A tagged commit packages as
`X.Y.Z`, any other commit as `X.Y.Z+git<date>.<sha>`, so re-installing upgrades
in place. There is no Flatpak or Snap: a sandbox can't spawn arbitrary agent
CLIs and PTYs.

### From a dev build to the package

If you ran RADE from `target/release` with your own launcher or a `~/.local/bin/rade` symlink:
quit RADE, install the .deb, then remove them (`rm ~/.local/bin/rade
~/.local/share/applications/rade.desktop`), or the dev CLI shadows `/usr/bin/rade` and talks to
the dev app. State, config and Worktrees stay where they were; `rade setup` needs no re-run (hooks
call plain `rade`).

## Docs screenshots and recordings

`docs/images/` is generated, never hand-edited: `scripts/demo/*.sh` build a made-up Workspace
with real git repositories, run the real app headless with a scripted demo agent, and take
screenshots (dark and light) and recordings (MP4 + GIF). `scripts/demo/collect.sh` runs them and
copies every image the docs reference. See [scripts/demo/README.md](scripts/demo/README.md).

## Where things are

- [CONTEXT.md](CONTEXT.md) — the glossary. Use these terms exactly in code, UI,
  docs and commits (Workspace, Source, Project, Worktree, Pane, Agent…).
- [docs/API.md](docs/API.md) — the CLI and socket contract. Stable since 1.0.0.
- [docs/internals/](docs/internals/) — requirements (`PRD.md`, IDs R1…R119),
  build order (`MILESTONES.md`), decisions (`adr/`), verification records
  (`checks/`), research notes and design specs.
- [CLAUDE.md](CLAUDE.md) — every test command and a map of the code; useful
  for humans too.
- `crates/rade-core` — all logic that can be tested without GTK (model,
  layout, config, git, review, usage…). `crates/rade-app` — the GTK4 /
  libadwaita app. `crates/rade-cli` — the `rade` binary.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
tests/e2e/m2.sh            # an E2E suite: headless Weston, the real app, driven by `rade`
scripts/check-removed.sh   # names removed in 1.0.0 stay gone
```

Every E2E suite (`tests/e2e/*.sh`) runs the real app in a headless Weston on a
private D-Bus with a throwaway HOME, so it never touches your desktop or
state. Screenshots land in `target/e2e-*/`. CI runs the same in
`ci/Dockerfile`; reproduce it with
`docker build --network=host -f ci/Dockerfile --target check .`
(see [docs/RELEASING.md](docs/RELEASING.md)).

Logic gets unit tests first, in `rade-core`. UI changes get an E2E check and a
screenshot you have looked at, in light and dark.

## Pull requests

- One change per PR. Say what it fixes or adds and how you checked it.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/)
  with a requirement ID or milestone as the scope when there is one:
  `feat(R2.4): …`, `fix(R13.2): …`, `docs: …`, `ci: …`. CI lints them
  (`scripts/lint-commits.sh origin/main..HEAD`).
- A change users can see adds a line under `## [Unreleased]` in
  [CHANGELOG.md](CHANGELOG.md), written for users, in the same commit. If it
  changes how a feature works, update its page in [docs/features/](docs/features/).
- `docs/API.md` is a stable contract: a breaking change to the CLI, socket,
  config or state files needs a new major version. Discuss it in an issue first.
- Don't edit the version in `Cargo.toml`; releases are cut with
  `scripts/release.sh` ([docs/RELEASING.md](docs/RELEASING.md)).
- Bigger decisions get an ADR in `docs/internals/adr/`.

## Code you may and may not reuse

RADE is MIT. MIT code (Ghostty, limux) may be reused with attribution in
[THIRD_PARTY.md](THIRD_PARTY.md). cmux-linux is AGPL: read it for ideas, never
copy its code.

## Reporting bugs

Open an issue with your Ubuntu version, `rade --version`, what you did, what
you expected and what happened. The app log is printed on stderr when you start
`rade-app` from a terminal. Security problems: see [SECURITY.md](SECURITY.md).
