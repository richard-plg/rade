# Releasing RADE

Why it works this way: [ADR-0012](internals/adr/0012-releases-semver-keep-a-changelog-self-hosted-ci.md), runners: [ADR-0042](internals/adr/0042-public-repo-ci-on-github-hosted-runners.md), the generated changelog: [ADR-0043](internals/adr/0043-changelog-generated-from-commits.md).

## Versions

- One source of truth: `[workspace.package] version` in `Cargo.toml` (`rade --version`, `rade-app --version`, the `.deb`). Never edit it by hand; `scripts/release.sh` does.
- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). [API.md](API.md) is stable from 1.0.0 ([ADR-0041](internals/adr/0041-redesign-ships-as-1-0-0.md)):
  - **MAJOR** (`1.4.2 → 2.0.0`): any incompatible change to the `rade` CLI / socket contract, the config files (`config.toml`, `workspace.toml`, `project.toml`, `.rade.toml`) or `state.json`.
  - **MINOR** (`1.0.0 → 1.1.0`): new features and compatible additions (a command, method, optional param, result field or config key).
  - **PATCH** (`1.0.0 → 1.0.1`): bug fixes only.
  - Pre-releases: `1.1.0-rc.1` (the `.deb` becomes `1.1.0~rc.1`, which dpkg sorts before `1.1.0`; the GitHub Release is marked pre-release).
  - Before 1.0 (0.1.0 … 0.7.1): 0.MINOR for features and breaking changes, 0.x.PATCH for fixes (ADR-0012).
- Package versions (`scripts/version.sh`): the tagged commit → `0.1.0`; any other build (CI artifacts, local `scripts/package-deb.sh`) → `0.1.0+git202610011200.abc1234`, which upgrades over `0.1.0` and is older than `0.1.1`.

## Commits and the changelog

- Subjects follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) with R-IDs (or a milestone) as the scope:
  `feat(R18): Browser Pane …` · `fix(R13.2,R11.2): …` · `docs(M12): checks …` · `test(R7.2): …` · `ci: …` · `chore(release): v0.2.0`.
  Types: `feat fix perf refactor docs test build ci chore style revert`; `!` after the scope marks a breaking change. CI checks new commits (`scripts/lint-commits.sh origin/main..HEAD` locally).
- The changelog is generated at release time from the commits since the last tag ([git-cliff](https://git-cliff.org) 2.14.2, [cliff.toml](../cliff.toml)): `feat` → **Added**, `perf` / `revert` → **Changed**, `fix` → **Fixed**, any breaking commit (`!` or a `BREAKING CHANGE:` footer) → **Changed** marked **Breaking:**; `docs`, `test`, `ci`, `chore`, `refactor`, `build` and `style` are left out. The line is the commit's description without the scope, so **write `feat` / `fix` / `perf` subjects for a user**.
- A footer `Changelog: <text>` replaces that line (when the subject has to stay technical); `Changelog: skip` leaves the commit out.
- `## [Unreleased]` in [CHANGELOG.md](../CHANGELOG.md) is optional hand-written text (an intro, a migration note, extra `### …` items): `scripts/release.sh` keeps it at the top of the new section and adds the generated items to the same groups.
- Preview the next section any time: `git cliff --unreleased --strip all`.
- Install git-cliff (the release script checks the version):
  ```sh
  v=2.14.2; f=git-cliff-$v-x86_64-unknown-linux-gnu.tar.gz
  gh release download v$v -R orhun/git-cliff -p "$f" -p "$f.sha512" -D /tmp/git-cliff
  (cd /tmp/git-cliff && sha512sum -c "$f.sha512" && tar xzf "$f" && install -m 755 git-cliff-$v/git-cliff ~/.local/bin/)
  ```
  (or `cargo install git-cliff --version 2.14.2 --locked`).

## Cut a release

1. On `main`, clean and pushed; CI green on the head commit ([Actions](https://github.com/richard-plg/rade/actions/workflows/ci.yml)). `scripts/release.sh` refuses otherwise: the release workflow does not test the code again (ADR-0044).
2. Preview the section (`git cliff --unreleased --strip all`, plus anything under `[Unreleased]`); pick the version by the rules above.
3. Run:
   ```sh
   scripts/release.sh 0.2.0
   # agents: append git-commit args, e.g. --trailer "Co-Authored-By: …"
   # NO_PUSH=1 to inspect first, then: git push --atomic origin HEAD v0.2.0
   ```
   It bumps `Cargo.toml` + `Cargo.lock`, writes `## [0.2.0] - <date>` (the `[Unreleased]` text + the generated items) with compare links, commits `chore(release): v0.2.0`, tags `v0.2.0` and pushes both.
4. The tag runs [release.yml](../.github/workflows/release.yml), ~6–8 min: it checks that the tag's commit is `chore(release): v0.2.0`, changes only `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`, and sits on a commit CI passed; then builds the `.deb` from the tag ([package.yml](../.github/workflows/package.yml): lintian, m10), attests it and publishes a GitHub Release `RADE v0.2.0` with `rade_0.2.0_amd64.deb`, `SHA256SUMS` and the changelog section as notes. The release commit itself is skipped by CI on `main`, so nothing runs twice. Watch: `gh run watch -R richard-plg/rade` (or `gh-watch`).
5. Check: `gh release view v0.2.0 -R richard-plg/rade`, then
   ```sh
   gh release download v0.2.0 -R richard-plg/rade -D /tmp/rade-v0.2.0
   (cd /tmp/rade-v0.2.0 && sha256sum -c SHA256SUMS && dpkg-deb --info rade_0.2.0_amd64.deb)
   ```
   With gh ≥ 2.81 also `gh release verify-asset v0.2.0 /tmp/rade-v0.2.0/rade_0.2.0_amd64.deb -R richard-plg/rade` (the immutable release's attestation), and `gh attestation verify /tmp/rade-v0.2.0/rade_0.2.0_amd64.deb -R richard-plg/rade` (the build provenance `release.yml` signs: which workflow run and commit built the `.deb`).

If the release workflow fails before publishing: re-run it when the failure was transient (it rebuilds from the tag). If the fix needs a commit, fix on main; since no Release exists yet, the tag may be recreated on the fixed commit (`git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z`, then `git tag -a vX.Y.Z -m 'RADE vX.Y.Z' && git push origin vX.Y.Z`) — done once, for v0.1.0, whose first run failed in the commit-lint job. Once a Release is published its tag never moves (immutable releases enforce it); fixes go in the next patch version. Releases are **immutable** (repo setting): a published Release's assets and tag can't change; a bad release is superseded by the next version, and can be marked pre-release or deleted.

## Hotfix an older release

When `main` has moved on with unreleased features and `0.2.0` needs a fix:

```sh
git switch -c release/0.2 v0.2.0          # branch from the tag
# fix + test, commit as fix(R…): <what the user sees> (the changelog line)
git push -u origin release/0.2
scripts/release.sh 0.2.1                   # allowed on release/* branches
git switch main && git cherry-pick <fix>   # bring the fix to main
```

## Install from a Release

```sh
gh release download -R richard-plg/rade -p 'rade_*_amd64.deb' -p SHA256SUMS -D /tmp/rade
(cd /tmp/rade && sha256sum -c SHA256SUMS) && sudo apt install /tmp/rade/rade_*_amd64.deb
```

## CI

[ci.yml](../.github/workflows/ci.yml) runs on push to `main` and `release/**`, on PRs (a newer push cancels the older run) and by hand, on GitHub-hosted runners (`ubuntu-latest`; never self-hosted, the repo is public: ADR-0042). Jobs, in parallel: commit lint, `check` (fmt, clippy, unit, contrast, trace, removed names), E2E in three shards (`a`, `b`, `c`, split by suite durations; a new suite goes into the lightest), and the `.deb` + m10 + lintian ([package.yml](../.github/workflows/package.yml), shared with release.yml). Image layers are cached on `ghcr.io/richard-plg/rade-ci` (`cache-check`, `cache-e2e`, `cache-deb`; registry cache, `mode=max`): pushes write it, pull requests only read it. Everything builds in `ci/Dockerfile`; reproduce a job locally with the same commands:

```sh
docker build --network=host -f ci/Dockerfile --target check .                       # fmt, clippy -D warnings, unit tests
docker build --network=host -f ci/Dockerfile --target e2e -t rade-ci:e2e . && ci/e2e.sh rade-ci:e2e run m2 m3   # E2E suites
docker build --network=host -f ci/Dockerfile --target deb -t rade-ci:deb \
  --build-arg RADE_VERSION=$(scripts/version.sh) --build-arg SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) . \
  && ci/e2e.sh rade-ci:deb m10                                                       # .deb + package E2E
```

- Not in CI: `tests/live` (needs the real desktop, keyboard and mouse) — run it by hand before a release that changes UI behaviour.
- The gate is CI on the release's parent commit: release.yml refuses a tag without it, and checks that the release commit changes only the version and changelog. (A public repo can also protect `main` with required checks.)
- **Flaky E2E**: m6's "one desktop banner, replaced in place" can fail when the machine is loaded (GLib's freedesktop backend sends a new banner if the first `Notify` reply hasn't arrived yet). `ci/e2e.sh` retries a failed suite once, alone, and leaves a warning annotation. A suite that fails twice fails the job.
- Disk: locally the build cache (cargo/zig cache mounts, ~4 GB, plus image layers) lives in your Docker; `docker builder prune` makes the next run cold. In CI the image layers (toolchain, libghostty, release builds) come from the GHCR registry cache, so only a toolchain or `ghostty` submodule change rebuilds libghostty; Rust builds cold every run (cache mounts are not exported). The `rade-ci` package must stay public (Settings → Packages) so pull requests from forks can read it.

### Timings (a 24-core workstation, measured 2026-10-01; a 4-vCPU hosted runner is slower: libghostty alone took ~9 min there before the layer cache)

| step | cold | warm |
|---|---|---|
| CI image toolchain (apt + Rust + Zig) | 36 s – 15 min (the Ubuntu mirror's speed) | cached layer, 0 s |
| libghostty (zig, ReleaseFast) | ~2 min | 0.5 s |
| `check`: fmt + clippy + unit tests | 62 s | 3–5 s (no Rust change) |
| E2E release build | 38 s | 0.1 s; ~30 s after a Rust change |
| E2E suites (13, 4 containers at a time) | ~2 min (m6 alone 65 s) | same |
| `.deb` (own `target/deb-build`) + m10 | 46 s + 5 s | ~35 s + 5 s after a Rust change |
| **Whole CI run**, wall clock | ~20 min with a slow mirror | **~3 min** ([first run](https://github.com/richard-plg/rade/actions/runs/36828472566)) |
