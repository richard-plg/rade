#!/usr/bin/env bash
# Stand-in ssh-keygen for E2E: `-f <file> -C <comment>` writes a fake key
# pair (private mode 600); every call is logged to $FAKE_SSH_KEYGEN_LOG.
echo "$*" >>"${FAKE_SSH_KEYGEN_LOG:-/dev/null}"
file= comment=
while (($#)); do
    case $1 in -f) file=$2; shift ;; -C) comment=$2; shift ;; esac
    shift
done
[[ -n $file ]] || { echo "fake ssh-keygen: -f required" >&2; exit 1; }
[[ -e $file ]] && { echo "$file already exists" >&2; exit 1; }
printf -- '-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n' >"$file"
chmod 600 "$file"
echo "ssh-ed25519 AAAAfake$(basename "$file")$RANDOM $comment" >"$file.pub"
