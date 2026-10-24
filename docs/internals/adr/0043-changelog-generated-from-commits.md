# The changelog is generated from commits (git-cliff)

Supersedes ADR-0012 §4 (the hand-curated `CHANGELOG.md`). Decided 2026-10-24 by the owner when RADE went open source (ADR-0042). ADR-0012 rejected git-cliff as the generator because "the existing history isn't conventional and agent-written subjects are R-ID-dense, not user-facing". The public history now starts fresh, every commit is linted as Conventional Commits in CI, and the owner wants the changelog to write itself.

## Decisions

1. **`scripts/release.sh X.Y.Z` generates the section** with [git-cliff](https://git-cliff.org) 2.14.2 (pinned; the script refuses another version) from the commits since the last `v*` tag, configured by `cliff.toml`: `feat` → Added, `perf` / `revert` → Changed, `fix` → Fixed, any breaking commit (`!`, `BREAKING CHANGE:`) → Changed with **Breaking:** in front; `docs`, `test`, `ci`, `chore`, `refactor`, `build`, `style` are left out. The line is the description without the R-ID scope.
2. **Subjects are written for users.** A `feat` / `fix` / `perf` subject becomes a changelog line as is. When it must stay technical, a footer `Changelog: <text>` replaces the line; `Changelog: skip` drops the commit.
3. **`[Unreleased]` stays, optional.** Hand-written text there (an intro, a migration note, extra items) goes first in the new section, and the generated items join its `### …` groups. The 1.0.0 section is that text, since the public history has no `feat` / `fix` commits before it.
4. The format stays [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) (Added / Changed / Fixed), and the section is still the GitHub Release notes (`ci/publish-release.sh`). git-cliff runs only on the releaser's machine; CI does not need it.

## Considered options

- **Generate the whole file** (`git cliff -o CHANGELOG.md`): would drop the hand-written 0.x sections, whose commits are not in the public history. Rejected; release.sh inserts one section.
- **GitHub's generated release notes**: built from merged PRs and labels; most changes are pushed to `main` directly. Rejected.
- **release-please / release-plz**: rejected again for the reasons in ADR-0012 (a bot and a PAT; versioning against crates.io).

## Consequences

- A vague `feat` subject is a vague changelog line. Review `git cliff --unreleased --strip all` before a release.
- The 0.x compare links are gone from `CHANGELOG.md`: their tags exist only in the private history.
