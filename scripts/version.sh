#!/usr/bin/env bash
# Print RADE's package version (ADR-0012). The single source of truth is
# [workspace.package] version in Cargo.toml (`rade --version`, `rade-app --version`).
#   HEAD tagged v<version>  → <version>                      (a release)
#   anything else           → <version>+git<YYYYmmddHHMM>.<sha> (dev/CI build;
#                             sorts above <version>, below the next release)
# A SemVer pre-release (0.2.0-rc.1) becomes 0.2.0~rc.1 so dpkg sorts it first.
# `--base` prints only the Cargo.toml version.
set -euo pipefail
cd "$(dirname "$0")/.."
BASE=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)
: "${BASE:?no version in Cargo.toml}"
[[ ${1:-} == --base ]] && { echo "$BASE"; exit; }
V=${BASE/-/\~}
if git tag --points-at HEAD | grep -qxF "v$BASE"; then echo "$V"; exit; fi
echo "$V+git$(git log -1 --format=%cd --date=format:%Y%m%d%H%M).$(git rev-parse --short HEAD)"
