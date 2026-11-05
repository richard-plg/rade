#!/usr/bin/env bash
# Stand-in GitHub CLI for E2E (no network). Appends "GH_CONFIG_DIR=<dir>
# [GIT_SSH_COMMAND=<cmd>] <argv>" to $FAKE_GH_LOG for every call. An
# account is a config dir: `login` ({"login": …}), `hosts.yml`, and the
# keys on the account (`keys-authentication`, `keys-signing`, one
# "type base64" per line).
#   gh --version / gh auth login --help  → version / help (no --skip-ssh-key, like gh 2.46)
#   gh auth login …          → logs in as $FAKE_GH_LOGIN (writes login + hosts.yml with oauth_token)
#   gh api user [--jq .login] → the account's login
#   gh api user/keys | user/ssh_signing_keys [--jq …] → its keys
#   gh ssh-key add <pub> --type authentication|signing → adds the key
#   gh repo clone <spec> [d] → git clone $FAKE_GH_REMOTES/<repo>.git [d] (a missing remote fails)
#   gh pr create …           → prints a PR URL ($FAKE_GH_PR_URL, default …/pr/1); with
#                              $FAKE_GH_DIR: number n from $FAKE_GH_DIR/next (1, 2, …), URL
#                              https://github.com/acme/<repo>/pull/<n> (<repo> = the folder above the
#                              checkout: a Worktree's Source id), and an open PR without checks added
#                              to prs/<head>.json; a file fail-create holding a word found in the
#                              checkout's path fails this one call (the file is removed)
#   gh pr list --head <b> …  → $FAKE_GH_DIR/prs/<b>.json (`/` in <b> → `__`), else $FAKE_GH_PRS, else []
#   gh pr edit <n> --body <t> → writes <t> to $FAKE_GH_DIR/edits/<n>
#   gh run view <run> --job <job> --log-failed → $FAKE_GH_DIR/runs/<job>.log (missing → exit 1)
# $FAKE_GH_SLEEP=<s> sleeps before every pr / run command (a hung gh).
printf 'GH_CONFIG_DIR=%s %s%s\n' "${GH_CONFIG_DIR:-}" \
    "${GIT_SSH_COMMAND:+GIT_SSH_COMMAND=$GIT_SSH_COMMAND }" "$*" >>"${FAKE_GH_LOG:-/dev/null}"
dir=${GH_CONFIG_DIR:-$HOME/.config/gh}
logged_in() { [[ -f $dir/login ]] || { echo "gh: not logged in to any hosts" >&2; exit 4; }; }
[[ $1 == --version ]] && { echo "gh version ${FAKE_GH_VERSION:-2.46.0} (fake)"; exit 0; }
case "$1 $2" in
    "auth login")
        [[ " $* " == *" --help "* ]] && { echo "Usage: gh auth login [flags] --git-protocol --insecure-storage"; exit 0; }
        mkdir -p "$dir"
        printf '{"login": "%s"}\n' "${FAKE_GH_LOGIN:?}" >"$dir/login"
        printf 'github.com:\n    oauth_token: gho_fake\n    git_protocol: ssh\n    user: %s\n' "$FAKE_GH_LOGIN" >"$dir/hosts.yml"
        chmod 600 "$dir/hosts.yml"
        echo "✓ Logged in as $FAKE_GH_LOGIN" ;;
    "api user")
        logged_in
        if [[ $3 == --jq ]]; then jq -r "$4" "$dir/login"; else cat "$dir/login"; fi ;;
    "api user/keys") logged_in; cat "$dir/keys-authentication" 2>/dev/null; true ;;
    "api user/ssh_signing_keys") logged_in; cat "$dir/keys-signing" 2>/dev/null; true ;;
    "ssh-key add")
        logged_in
        type=authentication
        [[ $* =~ --type\ ([a-z]+) ]] && type=${BASH_REMATCH[1]}
        cut -d' ' -f1,2 "$3" >>"$dir/keys-$type" ;;
    "repo clone")
        repo=${3##*[/:]}; repo=${repo%.git}
        exec git clone -q "$FAKE_GH_REMOTES/$repo.git" "${4:-$repo}" ;;
    "pr create")
        [[ -n ${FAKE_GH_SLEEP:-} ]] && sleep "$FAKE_GH_SLEEP"
        [[ -z ${FAKE_GH_DIR:-} ]] && { echo "${FAKE_GH_PR_URL:-https://example.test/pr/1}"; exit 0; }
        top=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
        if [[ -f $FAKE_GH_DIR/fail-create ]] && [[ $top == *"$(cat "$FAKE_GH_DIR/fail-create")"* ]]; then
            rm -f "$FAKE_GH_DIR/fail-create"
            echo "pull request create failed: GraphQL: something went wrong (createPullRequest)" >&2; exit 1
        fi
        head=; title=; prev=
        for a in "$@"; do
            case $prev in --head) head=$a ;; --title) title=$a ;; esac
            prev=$a
        done
        n=$(cat "$FAKE_GH_DIR/next" 2>/dev/null || echo 1)
        echo $((n + 1)) >"$FAKE_GH_DIR/next"
        repo=$(basename "$(dirname "$top")")
        url="https://github.com/acme/$repo/pull/$n"
        mkdir -p "$FAKE_GH_DIR/prs"
        f="$FAKE_GH_DIR/prs/${head//\//__}.json"
        oid=$(git rev-parse HEAD 2>/dev/null || echo 0)
        jq -c --argjson n "$n" --arg t "$title" --arg u "$url" --arg o "$oid" \
            '. + [{number: $n, title: $t, url: $u, state: "OPEN", isDraft: false, baseRefName: "main",
                   headRefOid: $o, reviewDecision: "", mergeStateStatus: "CLEAN", statusCheckRollup: []}]' \
            <(cat "$f" 2>/dev/null || echo '[]') >"$f.tmp" && mv "$f.tmp" "$f"
        echo "$url" ;;
    "pr list")
        [[ -n ${FAKE_GH_SLEEP:-} ]] && sleep "$FAKE_GH_SLEEP"
        head=; prev=
        for a in "$@"; do [[ $prev == --head ]] && head=$a; prev=$a; done
        f="${FAKE_GH_DIR:-/nonexistent}/prs/${head//\//__}.json"
        if [[ -n $head && -f $f ]]; then cat "$f"; else echo "${FAKE_GH_PRS:-[]}"; fi ;;
    "pr edit")
        n=$3; body=; prev=
        for a in "$@"; do [[ $prev == --body ]] && body=$a; prev=$a; done
        mkdir -p "${FAKE_GH_DIR:?}/edits"
        printf '%s' "$body" >"$FAKE_GH_DIR/edits/$n" ;;
    "run view")
        [[ -n ${FAKE_GH_SLEEP:-} ]] && sleep "$FAKE_GH_SLEEP"
        job=; prev=
        for a in "$@"; do [[ $prev == --job ]] && job=$a; prev=$a; done
        f="${FAKE_GH_DIR:-/nonexistent}/runs/$job.log"
        [[ -f $f ]] || { echo "failed to get run log: job $job not found" >&2; exit 1; }
        cat "$f" ;;
    *) echo "fake gh: unsupported: $*" >&2; exit 1 ;;
esac
