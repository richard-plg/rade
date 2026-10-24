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
#   gh pr create …           → prints a PR URL ($FAKE_GH_PR_URL, default …/pr/1)
#   gh pr list …             → prints $FAKE_GH_PRS (JSON), default []
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
    "pr create") echo "${FAKE_GH_PR_URL:-https://example.test/pr/1}" ;;
    "pr list") echo "${FAKE_GH_PRS:-[]}" ;;
    *) echo "fake gh: unsupported: $*" >&2; exit 1 ;;
esac
