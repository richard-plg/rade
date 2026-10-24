#!/usr/bin/env bash
# Cut a release (docs/RELEASING.md, ADR-0012):
#   scripts/release.sh X.Y.Z [git-commit args, e.g. --trailer "Co-Authored-By: …"]
# On a clean main (or release/* for a hotfix) in sync with origin:
#   Cargo.toml + Cargo.lock version → X.Y.Z, CHANGELOG [Unreleased] → [X.Y.Z],
#   commit "chore(release): vX.Y.Z", annotated tag vX.Y.Z, push both atomically.
# The tag push runs .github/workflows/release.yml (CI gates, .deb, GitHub Release).
# NO_PUSH=1 stops before the push (inspect, then `git push --atomic origin HEAD vX.Y.Z`).
set -euo pipefail
V=${1:?usage: scripts/release.sh X.Y.Z [git-commit args]}; shift
cd "$(dirname "$0")/.."
REPO=https://github.com/richard-plg/rade
fail() { echo "release: $*" >&2; exit 1; }

[[ $V =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$ ]] || fail "$V is not SemVer X.Y.Z[-pre]"
branch=$(git branch --show-current)
[[ $branch == main || $branch == release/* ]] || fail "on '$branch': release from main, or release/* for a hotfix"
git diff --quiet && git diff --cached --quiet || fail "uncommitted changes"
git fetch -q --tags origin
[[ $(git rev-parse HEAD) == $(git rev-parse '@{u}') ]] || fail "$branch is not in sync with its upstream"
! git rev-parse -q --verify "refs/tags/v$V" >/dev/null || fail "tag v$V exists"
OLD=$(scripts/version.sh --base)
[[ $OLD != "$V" && $(printf '%s\n%s\n' "$OLD" "$V" | sort -V | tail -1) == "$V" ]] || fail "$V is not above $OLD"
notes=$(awk '/^## \[Unreleased\]/{f=1;next} /^## \[/{f=0} /^\[[^]]+\]: /{next} f' CHANGELOG.md | grep -v '^\s*$' || true)
[[ -n $notes ]] || fail "CHANGELOG.md [Unreleased] is empty: nothing to release"
PREV=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)

sed -i "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$V\"/" Cargo.toml
cargo update --workspace --offline -q
python3 - "$V" "$(date -u +%F)" "$PREV" "$REPO" <<'PY'
import re, sys
v, day, prev, repo = sys.argv[1:]
p = "CHANGELOG.md"
s = open(p).read()
s = s.replace("## [Unreleased]\n", f"## [Unreleased]\n\n## [{v}] - {day}\n", 1)
link = f"[{v}]: {repo}/compare/{prev}...v{v}" if prev else f"[{v}]: {repo}/releases/tag/v{v}"
s = re.sub(r"^\[Unreleased\]: .*$", f"[Unreleased]: {repo}/compare/v{v}...HEAD\n{link}", s, count=1, flags=re.M)
open(p, "w").write(s)
PY
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "chore(release): v$V" "$@"
git tag -a "v$V" -m "RADE v$V"
git --no-pager show --stat --format='%h %s' HEAD
[[ ${NO_PUSH:-} == 1 ]] && { echo "not pushed: git push --atomic origin HEAD v$V"; exit 0; }
git push --atomic origin HEAD "v$V"
echo "pushed v$V: $REPO/actions/workflows/release.yml"
