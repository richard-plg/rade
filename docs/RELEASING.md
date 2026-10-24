# Releasing RADE

Why it works this way: [ADR-0012](adr/0012-releases-semver-keep-a-changelog-self-hosted-ci.md), runners: [ADR-0042](adr/0042-public-repo-ci-on-github-hosted-runners.md).

## Versions

- One source of truth: `[workspace.package] version` in `Cargo.toml` (`rade --version`, `rade-app --version`, the `.deb`). Never edit it by hand; `scripts/release.sh` does.
- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). [API.md](API.md) is stable from 1.0.0 ([ADR-0041](adr/0041-redesign-ships-as-1-0-0.md)):
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
- A user-visible change adds a line to `## [Unreleased]` in [CHANGELOG.md](../CHANGELOG.md) in the same commit, under `### Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or `Security` ([Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)). Write it for a user, not as the commit subject.

## Cut a release

1. On `main`, clean and pushed; CI green on the head commit ([Actions](https://github.com/richard-plg/rade/actions/workflows/ci.yml)).
2. Read `[Unreleased]`; pick the version by the rules above.
3. Run:
   ```sh
   scripts/release.sh 0.2.0
   # agents: append git-commit args, e.g. --trailer "Co-Authored-By: …"
   # NO_PUSH=1 to inspect first, then: git push --atomic origin HEAD v0.2.0
   ```
   It bumps `Cargo.toml` + `Cargo.lock`, moves `[Unreleased]` to `## [0.2.0] - <date>` with compare links, commits `chore(release): v0.2.0`, tags `v0.2.0` and pushes both.
4. The tag runs [release.yml](../.github/workflows/release.yml): the full CI from the tag (fmt, clippy, unit, every E2E suite, `.deb` + m10), then a GitHub Release `RADE v0.2.0` with `rade_0.2.0_amd64.deb`, `SHA256SUMS` and the changelog section as notes. Watch: `gh run watch -R richard-plg/rade` (or `gh-watch`).
5. Check: `gh release view v0.2.0 -R richard-plg/rade`, then
   ```sh
   gh release download v0.2.0 -R richard-plg/rade -D /tmp/rade-v0.2.0
   (cd /tmp/rade-v0.2.0 && sha256sum -c SHA256SUMS && dpkg-deb --info rade_0.2.0_amd64.deb)
   ```
   With gh ≥ 2.81 also `gh release verify-asset v0.2.0 /tmp/rade-v0.2.0/rade_0.2.0_amd64.deb -R richard-plg/rade` (the immutable release's attestation).

If the release workflow fails before publishing: re-run it when the failure was transient (it rebuilds from the tag). If the fix needs a commit, fix on main; since no Release exists yet, the tag may be recreated on the fixed commit (`git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z`, then `git tag -a vX.Y.Z -m 'RADE vX.Y.Z' && git push origin vX.Y.Z`) — done once, for v0.1.0, whose first run failed in the commit-lint job. Once a Release is published its tag never moves (immutable releases enforce it); fixes go in the next patch version. Releases are **immutable** (repo setting): a published Release's assets and tag can't change; a bad release is superseded by the next version, and can be marked pre-release or deleted.

## Hotfix an older release

When `main` has moved on with unreleased features and `0.2.0` needs a fix:

```sh
git switch -c release/0.2 v0.2.0          # branch from the tag
# fix + test, commit as fix(R…): …, add it under [Unreleased] in CHANGELOG.md
git push -u origin release/0.2
scripts/release.sh 0.2.1                   # allowed on release/* branches
git switch main && git cherry-pick <fix>   # bring the fix (and its changelog line) to main
```

## Install from a Release

```sh
gh release download -R richard-plg/rade -p 'rade_*_amd64.deb' -p SHA256SUMS -D /tmp/rade
(cd /tmp/rade && sha256sum -c SHA256SUMS) && sudo apt install /tmp/rade/rade_*_amd64.deb
```

## CI

[ci.yml](../.github/workflows/ci.yml) runs on push to `main`, on PRs (a newer push cancels the older run) and by hand, on GitHub-hosted runners (`ubuntu-latest`; never self-hosted, the repo is public: ADR-0042). Everything builds in `ci/Dockerfile`; reproduce a job locally with the same commands:

```sh
docker build --network=host -f ci/Dockerfile --target check .                       # fmt, clippy -D warnings, unit tests
docker build --network=host -f ci/Dockerfile --target e2e -t rade-ci:e2e . && ci/e2e.sh rade-ci:e2e run m2 m3   # E2E suites
docker build --network=host -f ci/Dockerfile --target deb -t rade-ci:deb \
  --build-arg RADE_VERSION=$(scripts/version.sh) --build-arg SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) . \
  && ci/e2e.sh rade-ci:deb m10                                                       # .deb + package E2E
```

- Not in CI: `tests/live` (needs the real desktop, keyboard and mouse) — run it by hand before a release that changes UI behaviour.
- The release workflow is the gate: it publishes nothing unless the full CI passes on the tag. (A public repo can also protect `main` with required checks.)
- **Flaky E2E**: m6's "one desktop banner, replaced in place" can fail when the machine is loaded (GLib's freedesktop backend sends a new banner if the first `Notify` reply hasn't arrived yet). `ci/e2e.sh` retries a failed suite once, alone, and leaves a warning annotation. A suite that fails twice fails the job.
- Disk: locally the build cache (cargo/zig cache mounts, ~4 GB, plus image layers) lives in your Docker; `docker builder prune` makes the next run cold. A GitHub-hosted runner always builds cold.

### Timings (a 24-core workstation, measured 2026-10-01; a 4-vCPU hosted runner is slower and always cold)

| step | cold | warm |
|---|---|---|
| CI image toolchain (apt + Rust + Zig) | 36 s – 15 min (the Ubuntu mirror's speed) | cached layer, 0 s |
| libghostty (zig, ReleaseFast) | ~2 min | 0.5 s |
| `check`: fmt + clippy + unit tests | 62 s | 3–5 s (no Rust change) |
| E2E release build | 38 s | 0.1 s; ~30 s after a Rust change |
| E2E suites (13, 4 containers at a time) | ~2 min (m6 alone 65 s) | same |
| `.deb` (own `target/deb-build`) + m10 | 46 s + 5 s | ~35 s + 5 s after a Rust change |
| **Whole CI run**, wall clock | ~20 min with a slow mirror | **~3 min** ([first run](https://github.com/richard-plg/rade/actions/runs/36828472566)) |
