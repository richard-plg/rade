#!/usr/bin/env bash
# Publish the GitHub Release for the pushed tag (release.yml, ADR-0012). Runs on
# the thin runner: bash, curl, jq, coreutils. Expects the CI-built .deb in dist/.
# Draft → upload assets → publish, so it also works with immutable releases.
set -euo pipefail
: "${GITHUB_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${GITHUB_REF_NAME:?}"
TAG=$GITHUB_REF_NAME V=${GITHUB_REF_NAME#v}
fail() { echo "::error::$*" >&2; exit 1; }

[[ $(scripts/version.sh --base) == "$V" ]] || fail "tag $TAG != Cargo.toml version $(scripts/version.sh --base)"
DEB=dist/rade_${V/-/\~}_amd64.deb
[[ -f $DEB && $(ls dist/*.deb | wc -l) == 1 ]] || fail "expected exactly $DEB in dist/: $(ls dist)"
(cd dist && sha256sum -- *.deb > SHA256SUMS && cat SHA256SUMS)

notes=$(awk -v v="$V" '$0 ~ "^## \\[" {f = index($0, "## [" v "]") == 1; next} /^\[[^]]+\]: / {next} f' CHANGELOG.md)
[[ -n ${notes//[[:space:]]/} ]] || fail "no CHANGELOG.md section for [$V]"
notes+=$'\n\n'"Install: \`sudo apt install ./$(basename "$DEB")\` · verify: \`sha256sum -c SHA256SUMS\`"

api() { curl -sS --fail-with-body -H "Authorization: Bearer $GITHUB_TOKEN" \
          -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28" "$@"; }
pre=false; [[ $V == *-* ]] && pre=true
rel=$(api -X POST "https://api.github.com/repos/$GITHUB_REPOSITORY/releases" -d "$(jq -n \
        --arg t "$TAG" --arg n "RADE $TAG" --arg b "$notes" --argjson p $pre \
        '{tag_name: $t, name: $n, body: $b, draft: true, prerelease: $p}')")
id=$(jq -r .id <<<"$rel")
upload=$(jq -r '.upload_url | sub("\\{.*"; "")' <<<"$rel")
for f in "$DEB" dist/SHA256SUMS; do
    api -X POST -H "Content-Type: application/octet-stream" --data-binary "@$f" \
        "$upload?name=$(basename "$f")" | jq -r '"uploaded \(.name) \(.size) bytes"'
done
api -X PATCH "https://api.github.com/repos/$GITHUB_REPOSITORY/releases/$id" -d '{"draft": false}' \
    | jq -r '"published \(.html_url)"'
