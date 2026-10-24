#!/usr/bin/env bash
# Stand-in ssh for E2E (no network), logged to $FAKE_SSH_LOG.
#   ssh -G <host>  → `identityfile` from the `Host <host>` block of ~/.ssh/config
#   ssh -T … -i <key> … git@github.com → GitHub's greeting for the account
#     (a ~/.config/gh-* dir) whose keys-authentication holds <key>.pub
echo "$*" >>"${FAKE_SSH_LOG:-/dev/null}"
if [[ $1 == -G ]]; then
    echo "hostname github.com"
    awk -v h="$2" '$1=="Host"{on=($2==h)} on && tolower($1)=="identityfile"{print "identityfile " $2}' "$HOME/.ssh/config" 2>/dev/null
    exit 0
fi
key=
while (($#)); do [[ $1 == -i ]] && { key=$2; shift; }; shift; done
pub=$(cut -d' ' -f1,2 "$key.pub" 2>/dev/null)
for d in "$HOME"/.config/gh-*/; do
    if [[ -n $pub ]] && grep -qxF "$pub" "$d/keys-authentication" 2>/dev/null; then
        echo "Hi $(jq -r .login "$d/login")! You've successfully authenticated, but GitHub does not provide shell access." >&2
        exit 1
    fi
done
echo "git@github.com: Permission denied (publickey)." >&2
exit 255
