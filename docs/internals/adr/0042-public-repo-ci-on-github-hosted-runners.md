# Public repo: CI on GitHub-hosted runners, history starts fresh

RADE goes open source (2026-10-24). ADR-0012 put CI on the owner's self-hosted runner because the repo was private, and already noted the consequence: "The self-hosted runner must never serve a public repo … Going public means `runs-on: ubuntu-latest`." This ADR records that switch. ADR-0012's other decisions (SemVer, Keep a Changelog, Conventional Commits with R-IDs, tag-driven releases, SHA256SUMS) stand.

## Decisions

1. **Every job runs on `ubuntu-latest`**, still entirely inside `ci/Dockerfile`; the runner only needs Docker. A pull request from a fork never runs on the owner's machine.
2. **Cold builds.** A hosted runner starts empty, so each job builds the image (apt, Rust, Zig, libghostty) from scratch; job timeouts go from 60 to 120 minutes. `E2E_JOBS=2` on the 4-vCPU runner, because some suites assert timings.
3. **The public repo starts with one commit.** The private history (employer names in old fixtures and screenshots, old tags and Releases with their source archives) stays in the private repo, renamed `richard-plg/rade-private`. The public `richard-plg/rade` has no tags or Releases from before the switch.

## Considered options

- **Keep the self-hosted runner and require approval for outside contributors' workflows**: one mistaken approval runs a stranger's code on the owner's desktop. Rejected.
- **Drop the `pull_request` trigger and keep self-hosted**: pushes to `main` would still run on the owner's machine from a public repo, and contributors would get no CI. Rejected.
- **Rewrite the private history with `git filter-repo`**: keeps the 0.x history, but every blob, commit message, tag and Release archive would have to be scrubbed and verified. Rejected for a fresh start.

## Consequences

- CI is slower (no warm BuildKit cache) and its timing-sensitive E2E checks may need looser budgets on hosted hardware. A later change can cache image layers in the GitHub Actions cache.
- Commit SHAs quoted in `docs/internals/checks/*.md` point at the private history.
- Artifact attestations (unavailable for private repos on Free, ADR-0012) and branch protection with required checks become available.
