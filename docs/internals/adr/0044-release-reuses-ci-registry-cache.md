# A release reuses CI's verdict; CI caches layers on GHCR and shards E2E

Amends ADR-0012 §5 ("The tag runs the whole CI workflow again from the tag") and ADR-0042 §2 (the Actions layer cache). Decided 2026-10-25 by the owner after 1.0.0 and 1.0.1.

Measured then: `scripts/release.sh` pushes the release commit and its tag together, so CI on `main` and the Release workflow (which called `ci.yml`) both tested the same commit, ~18 min each. For 1.0.0 the two disagreed: CI on `main` failed while the Release passed. The `.deb` job took 17 min because the libghostty layer (530 s) was rebuilt although cached: the Actions cache is capped at 10 GB per repository and two runs wrote it at once. E2E took 14 min (30 suites, 2 at a time on a 4-vCPU runner).

## Decisions

1. **One verdict per commit.** CI skips a push whose head commit is `chore(release): v…`. The Release workflow no longer runs CI. Its `verify` job requires the tag's commit to be `chore(release): <tag>`, to change only `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`, and to sit on a commit for which `ci.yml` succeeded (Actions API). `scripts/release.sh` checks the same before tagging. The Release then builds the `.deb` from the tag, runs lintian and m10, attests and publishes.
2. **The `.deb` job is one reusable workflow** (`package.yml`) called by `ci.yml` and `release.yml`, so both build the package the same way.
3. **Registry cache on GHCR instead of `type=gha`.** `ghcr.io/richard-plg/rade-ci:cache-{check,e2e,deb}`, `mode=max`. Pushes write it; a pull request's token can only read it, and the package is public so forks can. One E2E shard writes the e2e cache; the other two read it.
4. **E2E in three shards** (`a`, `b`, `c`), split by measured suite durations (greedy, slowest first). A new suite goes into the lightest shard.

## Considered options

- **Keep the Release running the full CI, skip CI on `main` for the release commit**: removes the duplicate but a release still waits ~18 min to re-test unchanged code. Rejected.
- **Pass the built `.deb` from CI to the Release**: the tag's commit differs from its parent (the version), so the package must be built from the tag anyway.
- **Cache cargo's target dir** (sccache, or exporting BuildKit cache mounts): would save the 2–3 min Rust builds; more moving parts. Not now.

## Consequences

- A release commit made by hand (not by `release.sh`), or one that also changes code, is refused by the Release workflow.
- The code in a release was tested on its parent commit, not on the tag; only the version string and changelog differ, and m10 runs on the tag's own `.deb`.
- The `rade-ci` package on GHCR must stay public, or pull requests from forks build without the cache (slower, not wrong).
