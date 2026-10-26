#!/usr/bin/env bash
# E2E (M48, PRD R127): Updates — check, install, restart — against a fake GitHub
# (tests/e2e/fake-github.py), a fake installer (fake-install.sh = RADE_UPDATE_INSTALL)
# and a fake dpkg-query (fake-dpkg-query.sh). The Devel build honours the RADE_UPDATE_*
# variables (D4); the .deb never does. Nothing here reaches github.com, pkexec or apt.
#   R127-i  one request at start (User-Agent, no Authorization) → cell, Activity, Settings ›
#           Updates; relaunch within the day sends nothing; `[update] check = false`;
#           RADE_UPDATE_CHECK=0; the `Updated to RADE <v>` Toast after a restart
#   R127-j  security release: no Toast while an Agent waits, one after, none after a relaunch;
#           Skip This Version survives a relaunch, a higher release shows again
#   R127-k  Install: phases, `<v> <pct>%`, `<v> installing`, `restart for <v>`, the installer's
#           argv + SHA-256, the .deb deleted; wrong SHA256SUMS; installer exit 126; exit 1
#   R127-m  a hand install (dpkg reports a higher version) → `restart for <v>`, no download
#   R127-n  `rade version [--check] [--json]` with no app, the app, a fake socket; the hint
#   R127-o  Devel build without a URL; a non-deb install (Download)
#   R127-p  cells + About dark and light, the cell at 1280 px
#   R127-l  Restart… dialog, Not now, Restart: a new app, the same Panes interrupted (runs LAST:
#           the restarted app has no test mode and no RADE_UPDATE_* env, D27)
# Screenshots → target/e2e-m48/.
# Usage: tests/e2e/m48.sh   (NO_BUILD=1 skips the build, KEEP=1 keeps the temp dir,
#        ONLY="i j" runs only those sections: i j k m o n p l)
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m48}
export WESTON_WIDTH=1440
export RADE_UPDATE_CHECK=1
source "$HERE/lib.sh"

FAKEDIR=$T/ghdir
mkdir -p "$FAKEDIR" "$XDG_CONFIG_HOME/rade"
GHLOG=$T/gh.log
: >"$GHLOG"
python3 -I "$HERE/fake-github.py" "$T/gh.port" "$GHLOG" "$FAKEDIR" >"$OUT/fake-github.out" 2>&1 &
GH_PID=$!
trap 'kill $GH_PID 2>/dev/null; cleanup' EXIT
for _ in $(seq 50); do [[ -s $T/gh.port ]] && break; sleep 0.1; done
GHPORT=$(cat "$T/gh.port")

CUR=$("$BIN/rade-app" --version | awk '{print $2}')
IFS=. read -r MAJ MIN _ <<<"$CUR"
NEXT=$MAJ.$((MIN + 1)).0 NEXT2=$MAJ.$((MIN + 2)).0
UPD=$XDG_CACHE_HOME/rade-dev/update
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
export RADE_UPDATE_URL="http://127.0.0.1:$GHPORT/repos/richard-plg/rade/releases?per_page=20"
export RADE_UPDATE_DELAY=2
export RADE_UPDATE_INSTALL=$HERE/fake-install.sh
export RADE_UPDATE_DPKG_QUERY=$HERE/fake-dpkg-query.sh
echo "   running $CUR, next $NEXT, fake GitHub on :$GHPORT"

# ---- helpers ---------------------------------------------------------------
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
# check "<name>" "<shell expression>": the expression is evaluated here (variables and helpers in
# scope) and shown when it fails; with a command instead, as in lib.sh.
check() {
    local name=$1; shift
    if [[ $# == 1 ]]; then
        if eval "$1"; then ok "$name"; else bad "$name"; echo "    (was: $1)"; fi
    elif "$@"; then ok "$name"; else bad "$name"; fi
}
up() { rpc debug.update "{\"op\":\"$1\"${2:+,\"arg\":$2}}"; } # op [json arg] → info
uq() { up info | jq -r "$1"; }
ui() { rpc debug.ui "{\"op\":\"$1\"${2:+, \"arg\": \"$2\"}}"; }
chrome() { ui chrome | jq -c .chrome; }
reqs() { wc -l <"$GHLOG" | tr -d ' '; }
want() { [[ -z ${ONLY:-} || " $ONLY " == *" $1 "* ]]; }
conf() { # extra TOML after the Agent preset
    cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOT
idle_seconds = 3600
[agents.fake]
command = "$HERE/fake-agent.sh"
session_id_flag = "--session-id"
resume = "$HERE/fake-agent.sh --resume {session_id}"
${1:-}
EOT
}
# rel <version[:security]>…: releases.json + the .debs + SHA256SUMS the fake serves.
rel() {
    python3 -I - "$FAKEDIR" "$GHPORT" "$CUR" "$@" <<'PY'
import hashlib, json, os, sys
d, port, cur = sys.argv[1:4]
specs = sys.argv[4:]
base = "http://127.0.0.1:%s" % port
sums, out = [], []
def dig(b): return hashlib.sha256(b).hexdigest()
for spec in specs:
    v, _, sec = spec.partition(":")
    name = "rade_%s_amd64.deb" % v
    p = os.path.join(d, name)
    if not os.path.exists(p):
        open(p, "wb").write(os.urandom(2_000_000))
    data = open(p, "rb").read()
    sums.append("%s  %s\n" % (dig(data), name))
    body = "### Added\n\n- Something new in %s\n" % v
    if sec: body += "\n### Security\n\n- Fix a thing\n"
    body += "\nInstall: `sudo apt install ./%s` · verify: `sha256sum -c SHA256SUMS`" % name
    out.append((v, name, len(data), dig(data), body))
open(os.path.join(d, "SHA256SUMS"), "w").write("".join(sums))
sd = open(os.path.join(d, "SHA256SUMS"), "rb").read()
rels = []
for i, (v, name, size, h, body) in enumerate(out):
    tag = "v" + v
    rels.append({
        "tag_name": tag, "name": "RADE " + tag, "draft": False, "prerelease": False,
        "published_at": "2026-10-2%d T10:00:00Z".replace(" ", "") % (i + 5),
        "html_url": "%s/release/%s" % (base, tag), "body": body,
        "assets": [
            {"name": name, "size": size, "digest": "sha256:" + h,
             "browser_download_url": "%s/download/%s" % (base, name)},
            {"name": "SHA256SUMS", "size": len(sd), "digest": "sha256:" + dig(sd),
             "browser_download_url": "%s/download/SHA256SUMS" % base},
        ]})
json.dump(rels, open(os.path.join(d, "releases.json"), "w"))
PY
}
# fresh [ver]: app stopped, cache + installer state cleared, dpkg reports the running version.
fresh() {
    [[ -n $APP_PID ]] && stop_app
    rm -rf "$UPD" "$FAKEDIR/fail" "$FAKEDIR/slow" "$T"/install.log "$T"/install-sleep \
        "$T"/install-stderr "$T"/install-exit
    echo "$CUR" >"$T/dpkg-version"
    echo "${1:-$NEXT}" >"$T/install-version"
    rel "$NEXT"
}
cache_set() { # python expression on the dict `c`
    python3 -I - "$UPD/update.json" "$1" <<'PY'
import json, sys
c = json.load(open(sys.argv[1])); exec(sys.argv[2]); json.dump(c, open(sys.argv[1], "w"))
PY
}
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
both() { shot "$1-dark"; light; shot "$1-light"; dark; }
about_open() { rade debug action about >/dev/null; sleep 0.8; }
about_close() { ui app_settings_close >/dev/null; sleep 0.4; }
export -f rpc

conf
rade_up() { start_app; rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null; }
close_inspector() { [[ $(ui chrome | jq -r .chrome.inspector.shown) == true ]] && rpc debug.ui '{"op":"key","keys":"ctrl+shift+e"}' >/dev/null; }

# ======================================================================== i
if want i; then
echo "== R127-i check at start, cache, config, env"
fresh; : >"$GHLOG"
rade_up; close_inspector
eventually 20 is "uq '[.requests, .cell.text // \"\"] | join(\" \")'" "1 $NEXT"
sleep 0.5
shot i-available
check "R127-i one request, User-Agent rade/$CUR, no Authorization" \
    "[[ \$(reqs) == 1 ]] && grep -q ' GET /repos/richard-plg/rade/releases.* ua=rade/$CUR auth=no\$' '$GHLOG'"
check "R127-i the Statusline cell reads $NEXT" "[[ \$(uq .cell.text) == '$NEXT' && \$(chrome | jq -r .statusline.update) == '$NEXT' ]]"
check "R127-i Activity has '$NEXT is available'" "ui activity | jq -r '.activity[].line' | grep -q 'RADE.*$NEXT is available\|$NEXT is available'"
rade debug action about >/dev/null; sleep 0.8
check "R127-i Settings › Updates shows 'RADE $NEXT is available' with Install and Skip This Version" \
    "[[ \$(ui app_settings | jq -r .category) == Updates && \$(uq .about.status) == 'RADE $NEXT is available' ]] && up info | jq -e '.about.buttons | index(\"Install $NEXT\") != null and index(\"Skip This Version\") != null' >/dev/null"
both about-available; about_close
both cell-available

# a relaunch within the day: no request, the cell from the cache
stop_app; rade_up
sleep 5
check "R127-i a relaunch within the day sends no request and shows the cell from the cache" \
    "[[ \$(reqs) == 1 && \$(uq .requests) == 0 && \$(uq .cell.text) == '$NEXT' ]]"

# the Updated toast after a restart on the new version
stop_app
cache_set "c['just_installed'] = '$CUR'"
rade_up
eventually 10 is "uq '.toasts | join(\"|\")'" "Updated to RADE $CUR"
check "R127-i the first start after an update raises 'Updated to RADE $CUR' once and clears just_installed (updated toast)" \
    "[[ \$(uq '.toasts | join(\"|\")') == 'Updated to RADE $CUR' ]] && [[ \$(jq -r .just_installed '$UPD/update.json') == null ]]"
stop_app; rade_up; sleep 1
check "R127-i …and not again at the next start (updated toast)" "[[ \$(uq '.toasts | length') == 0 ]]"

# [update] check = false: no automatic request even when due; the cell stays; Check now asks
stop_app
conf $'[update]\ncheck = false'
rade_up; up age 90000 >/dev/null
sleep 5
check "R127-i check = false: due (aged 25 h) and still no request, the cell shows from the cache" \
    "[[ \$(reqs) == 1 && \$(uq .requests) == 0 && \$(uq .cell.text) == '$NEXT' && \$(uq .enabled) == false ]]"
up check_now >/dev/null
eventually 10 is "reqs" 2
check "R127-i check = false: Check now sends one request" "[[ \$(reqs) == 2 ]]"
stop_app; conf

# RADE_UPDATE_CHECK=0: nothing is sent, Check now is off
RADE_UPDATE_CHECK=0 rade_up
up age 90000 >/dev/null
sleep 4
up check_now >/dev/null; sleep 1
check "R127-i RADE_UPDATE_CHECK=0: no request at start, Check now sends nothing, phase off, no buttons" \
    "[[ \$(reqs) == 2 && \$(uq .requests) == 0 && \$(uq .phase) == off && \$(uq '.about.buttons | length') == 0 && \$(uq .about.status) == 'Update checks are off' ]]"
stop_app
fi

# ======================================================================== j
if want j; then
echo "== R127-j security Toast, Skip"
fresh; rel "$NEXT:sec"; touch "$FAKEDIR/fail"; : >"$GHLOG"
conf
rade_up; close_inspector
rade source add "$T/shop" >/dev/null 2>&1 || { mkdir -p "$T/shop"; G -C "$T/shop" init -q -b main; G -C "$T/shop" commit -q --allow-empty -m init; rade source add "$T/shop" --name shop >/dev/null; }
w=$(rade spawn --agent fake --source shop --name asker "hello" --json | jq -r .pane)
eventually 15 is "rade list --json | jq -r --arg p $w '.panes[]|select(.pane==\$p)|.status'" done
rade status waiting --pane "$w"
sleep 3 # the failed first check (500) has happened
rm -f "$FAKEDIR/fail"
up check_now >/dev/null
eventually 10 is "uq .phase" available
sleep 3
shot j-security-waiting
check "R127-j the cell reads '$NEXT security' and no Toast is raised while an Agent waits" \
    "[[ \$(uq .cell.text) == '$NEXT security' && \$(uq '.toasts | length') == 0 ]]"
both cell-security
rade status done --pane "$w"
eventually 15 is "uq '.toasts | join(\"|\")'" "RADE $NEXT fixes a security issue"
check "R127-j one 'RADE $NEXT fixes a security issue' Toast once the Agent stops waiting" \
    "[[ \$(uq '.toasts | join(\"|\")') == 'RADE $NEXT fixes a security issue' ]]"
stop_app; rade_up; sleep 5
check "R127-j none after the relaunch (the cache says it was shown)" "[[ \$(uq '.toasts | length') == 0 && \$(uq .cell.text) == '$NEXT security' ]]"
up skip >/dev/null
check "R127-j Skip This Version hides the cell" "[[ \$(uq .cell) == null && \$(uq .offer.skipped) == true ]]"
stop_app; rade_up; sleep 4
check "R127-j …and survives a relaunch" "[[ \$(uq .cell) == null && \$(uq .offer.skipped) == true ]]"
rel "$NEXT:sec" "$NEXT2"
up check_now >/dev/null
eventually 10 is "uq '.cell.text // \"\"'" "$NEXT2 security"
check "R127-j a higher release shows again (not skipped)" "[[ \$(uq .offer.version) == '$NEXT2' && \$(uq .offer.skipped) == false ]]"
stop_app
fi

# ======================================================================== k
if want k; then
echo "== R127-k install"
conf
sample() { # until the phase is one of $1 (regex): one "<phase>|<cell>" per line in $2
    local i
    : >"$2"
    for i in $(seq 400); do
        up info | jq -r '"\(.phase)|\(.cell.text // "")"' >>"$2"
        [[ $(tail -1 "$2") =~ ^($1)\| ]] && return 0
        sleep 0.05
    done
    return 1
}
phases() { uq '[.phases[]] | .[(index("available") // 0):] | join(", ")'; }

echo "-- exit 0 (slow download, slow installer)"
fresh; touch "$FAKEDIR/slow"; echo 2 >"$T/install-sleep"; : >"$GHLOG"
rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
up install >/dev/null
sample 'restart_pending' "$T/k-samples"
SUM=$(sha256sum "$FAKEDIR/rade_${NEXT}_amd64.deb" | cut -d' ' -f1)
check "R127-k phases read available, downloading, verifying, installing, restart_pending" \
    "[[ \$(phases) == 'available, downloading, verifying, installing, restart_pending' ]]"
check "R127-k the cell was '$NEXT <pct>%', then '$NEXT installing', then 'restart for $NEXT'" \
    "python3 -I - '$T/k-samples' '$NEXT' <<'PY'
import re, sys
t = [l.split('|', 1)[1].strip() for l in open(sys.argv[1])]
v = sys.argv[2]
def first(pred):
    return next((i for i, x in enumerate(t) if pred(x)), None)
a = first(lambda x: re.fullmatch(re.escape(v) + r' \d+%', x))
b = first(lambda x: x == v + ' installing')
c = first(lambda x: x == 'restart for ' + v)
sys.exit(0 if None not in (a, b, c) and a < b < c else 1)
PY"
check "R127-k the installer got the .deb path, its SHA-256 matches the served file, one call" \
    "[[ \$(wc -l <'$T/install.log') == 1 ]] && [[ \$(cut -d' ' -f2 '$T/install.log') == '$SUM' ]] && [[ \$(cut -d' ' -f1 '$T/install.log') == */rade_${NEXT}_amd64.deb ]]"
check "R127-k the .deb is deleted afterwards" "[[ -z \$(ls '$UPD'/*.deb 2>/dev/null) ]]"
check "R127-k the .deb came through the 302 redirect (a /download/ then an /assets/ request)" \
    "grep -q ' GET /download/rade_${NEXT}_amd64.deb ' '$GHLOG' && grep -q ' GET /assets/rade_${NEXT}_amd64.deb ' '$GHLOG'"
check "R127-k the cell reads 'restart for $NEXT'" "[[ \$(uq .cell.text) == 'restart for $NEXT' && \$(uq .phase) == restart_pending ]]"
both cell-restart
about_open
check "R127-k About says 'RADE $NEXT is installed' with Restart…" \
    "[[ \$(uq .about.status) == 'RADE $NEXT is installed' ]] && up info | jq -e '.about.buttons | index(\"Restart…\") != null' >/dev/null"
both about-restart; about_close

echo "-- wrong SHA256SUMS"
fresh; : >"$GHLOG"
sed -i 's/^[0-9a-f]\{64\}/0000000000000000000000000000000000000000000000000000000000000000/' "$FAKEDIR/SHA256SUMS"
rade_up
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
up install >/dev/null
eventually 30 is "uq .phase" failed
check "R127-k wrong sums: 'Download didn't match SHA256SUMS. Nothing was installed.', the installer never called" \
    "[[ \$(uq .about.sub) == \"Download didn't match SHA256SUMS. Nothing was installed.\" && \$(uq .about.status) == 'Update failed' ]] && [[ ! -s '$T/install.log' ]]"

echo "-- installer exit 126 (polkit dialog closed)"
fresh; echo 126 >"$T/install-exit"
rade_up
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
up install >/dev/null
eventually 30 is "uq '.installer.exit'" 126
eventually 10 is "uq .phase" available
check "R127-k exit 126: back to Install, no error, no failed phase" \
    "[[ \$(uq .phase) == available && \$(uq .about.status) == 'RADE $NEXT is available' ]] && up info | jq -e '(.phases | index(\"failed\")) == null and (.about.buttons | index(\"Install $NEXT\") != null)' >/dev/null"

echo "-- installer exit 1"
fresh; echo 1 >"$T/install-exit"; printf 'E: Unable to locate package rade\nE: the installer broke\n' >"$T/install-stderr"
rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
up install >/dev/null
eventually 30 is "uq .phase" failed
sleep 0.5
check "R127-k exit 1: 'update failed' with the installer's last line" \
    "[[ \$(uq .cell.text) == 'update failed' && \$(uq .about.sub) == 'E: the installer broke' ]] && uq .cell.tooltip | grep -q 'the installer broke'"
both cell-failed
stop_app
fi

# ======================================================================== m
if want m; then
echo "== R127-m a hand install"
fresh; echo "$NEXT2" >"$T/dpkg-version"; : >"$GHLOG"
rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "restart for $NEXT2"
eventually 10 is "reqs" 1
check "R127-m the cell reads 'restart for $NEXT2' after a check, with no download" \
    "[[ \$(uq .cell.text) == 'restart for $NEXT2' && \$(uq .phase) == restart_pending ]] && ! grep -q '/download/\|/assets/' '$GHLOG' && [[ \$(uq .download) == null ]]"
stop_app
fi

# ======================================================================== o
if want o; then
echo "== R127-o Devel build, a non-deb install"
fresh; : >"$GHLOG"
RADE_UPDATE_URL= rade_up; close_inspector
sleep 5
rade debug action about >/dev/null; sleep 0.8
check "R127-o a Devel build without RADE_UPDATE_URL: 'Development build', no request" \
    "[[ \$(uq .phase) == devel && \$(uq .about.status) == 'Development build' && \$(reqs) == 0 && \$(uq .requests) == 0 && \$(uq .cell) == null ]]"
stop_app; : >"$GHLOG"
fresh
RADE_UPDATE_INSTALL= rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
check "R127-o a non-deb install: 'RADE $NEXT is available' with Download instead of Install" \
    "[[ \$(uq .install) == other && \$(uq .about.status) == 'RADE $NEXT is available' ]] && up info | jq -e '.about.buttons[0] == \"Download\" and (.about.buttons | index(\"Install $NEXT\") == null)' >/dev/null"
up download >/dev/null
check "R127-o Download opens the release page (the URI is recorded, nothing installed)" \
    "[[ \$(uq '.launched[-1]') == 'http://127.0.0.1:$GHPORT/release/v$NEXT' && ! -s '$T/install.log' ]]"
stop_app
fi

# ======================================================================== n
if want n; then
echo "== R127-n rade version"
fresh; : >"$GHLOG"
NOAPP=$T/none.sock
v() { RADE_SOCKET=$NOAPP rade version "$@"; }
check "R127-n no app, no cache: 'rade $CUR', 'app not running', 'not checked yet'" \
    "[[ \$(v) == \$'rade $CUR\napp not running\nnot checked yet' ]]"
check "R127-n --json: cli, app null, update keys of API.md" \
    "[[ \$(v --json | jq -cS .) == '{\"app\":null,\"cli\":\"$CUR\",\"update\":{\"checked_at\":null,\"error\":null,\"kind\":null,\"latest\":null,\"security\":false,\"skipped\":null}}' ]]"
rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT"
check "R127-n with the app: 'app $CUR (pid <pid>)' and 'update $NEXT available (minor) · checked just now'" \
    "[[ \$(rade version) == \$'rade $CUR\napp $CUR (pid $APP_PID)\nupdate $NEXT available (minor) · checked just now' ]]"
check "R127-n --json with the app: pid, latest, kind, checked_at" \
    "[[ \$(rade version --json | jq -c '[.app.pid, .app.version, .update.latest, .update.kind, .update.security, (.update.checked_at | type)]') == '[$APP_PID,\"$CUR\",\"$NEXT\",\"minor\",false,\"number\"]' ]]"
B=$(reqs)
OUTV=$(rade version --check 2>&1)
check "R127-n --check sends exactly one request and then reads 'checked just now'" \
    "[[ \$(reqs) == $((B + 1)) ]] && grep -q 'checked just now' <<<\"\$OUTV\""
up age 600 >/dev/null; rade version --check >/dev/null
check "R127-n --check wrote the cache the app shares ('checked_at' moved)" "[[ \$(rade version --json | jq '.update.checked_at') -ge \$(( \$(date +%s) - 5 )) ]]"
OFF=$(RADE_UPDATE_CHECK=0 rade version --check 2>&1 >/dev/null); B=$(reqs)
check "R127-n --check with RADE_UPDATE_CHECK=0 says why and sends nothing" "[[ \$OFF == 'rade: update checks are off (RADE_UPDATE_CHECK=0)' ]] && [[ \$(reqs) == $B ]]"
stop_app
# a fake socket answering 'unknown method' to everything (an app older than 1.2.0)
FAKE=$T/old.sock
python3 -I - "$FAKE" <<'PY' &
import json, os, socket, sys
p = sys.argv[1]
s = socket.socket(socket.AF_UNIX); s.bind(p); s.listen(8)
while True:
    c, _ = s.accept()
    buf = b""
    while not buf.endswith(b"\n"):
        d = c.recv(65536)
        if not d: break
        buf += d
    try: m = json.loads(buf)["method"]
    except Exception: m = "?"
    r = {"jsonrpc": "2.0", "id": 1, "error": {"code": -32601, "message": "unknown method '%s'" % m, "data": {"kind": "USAGE"}}}
    c.sendall((json.dumps(r) + "\n").encode()); c.close()
PY
FAKE_PID=$!
eventually 5 test -S "$FAKE"
fv() { RADE_SOCKET=$FAKE rade "$@"; }
check "R127-n an app that doesn't know app.info: 'app older than 1.2.0 (no app.info) · restart RADE to finish updating'" \
    "[[ \$(fv version | sed -n 2p) == 'app older than 1.2.0 (no app.info) · restart RADE to finish updating' ]]"
check "R127-n --json: app {version null, pid null}" "[[ \$(fv version --json | jq -c .app) == '{\"version\":null,\"pid\":null}' ]]"
fv list >/dev/null 2>"$T/hint.err"; RC=$?
check "R127-n a call failing with 'unknown method' prints the restart line on stderr, exit code unchanged" \
    "[[ \$RC == 2 ]] && [[ \$(sed -n 1p '$T/hint.err') == \"rade: unknown method 'pane.list'\" ]] && [[ \$(sed -n 2p '$T/hint.err') == 'rade: this rade is $CUR, the running RADE app is older: restart RADE to finish updating' ]] && [[ \$(wc -l <'$T/hint.err') == 2 ]]"
kill $FAKE_PID 2>/dev/null
fi

# ======================================================================== p
if want p; then
echo "== R127-p cells and the About block, 1280 px"
fresh; rel "$NEXT:sec"
rade_up; close_inspector
eventually 20 is "uq '.cell.text // \"\"'" "$NEXT security"
layout() { rpc debug.ui "{\"op\":\"layout\",\"width\":$1,\"height\":$2}" >/dev/null; sleep 1.2; }
layout 1280 800
shot p-1280
check "R127-p at 1280 px statusline.update is the cell's text, not dropped" \
    "[[ \$(chrome | jq -r .statusline.update) == '$NEXT security' ]] && [[ \$(chrome | jq -r '.statusline.dropped | tostring') != *update* ]]"
check "R127-p at 1280 px no label is clipped and nothing scrolls sideways" \
    "[[ \$(ui clipped | jq .count) == 0 && \$(ui hscroll | jq .count) == 0 ]]"
layout 1440 900
about_open
check "R127-p the About block in 'available' (security): 'Fixes a security issue.' with Install and Skip" \
    "[[ \$(uq .about.sub) == 'Fixes a security issue.' && \$(uq .about.status_class) == wait ]]"
both about-security; about_close
stop_app
fi

# ======================================================================== l (last)
if want l; then
echo "== R127-l restart"
fresh; echo "$NEXT" >"$T/dpkg-version"
mkdir -p "$T/shop"; [[ -d $T/shop/.git ]] || { G -C "$T/shop" init -q -b main; G -C "$T/shop" commit -q --allow-empty -m init; }
conf
rade_up; close_inspector
rade source add "$T/shop" --name shop >/dev/null 2>&1
run=$(rade spawn --agent fake --source shop --name worker "hello" --json | jq -r .pane)
ask=$(rade spawn --agent fake --source shop --name asker "hello" --json | jq -r .pane)
eventually 15 is "rade list --json | jq -r --arg p $run '.panes[]|select(.pane==\$p)|.status'" done
eventually 15 is "rade list --json | jq -r --arg p $ask '.panes[]|select(.pane==\$p)|.status'" done
rade status running --pane "$run"; rade status waiting --pane "$ask"
eventually 20 is "uq '.cell.text // \"\"'" "restart for $NEXT"
OLD=$(rpc app.info | jq -r .pid)
BEFORE=$(rade list --json | jq -cS '[.panes[] | {pane, name}] | sort_by(.pane)')
up cell_click >/dev/null; sleep 0.8
D=$(rpc debug.update '{"op":"dialog"}')
shot l-dialog
check "R127-l the dialog reads D20's two lines (1 running, 1 waiting)" \
    "[[ \$(jq -r .heading <<<'$D') == 'Restart RADE to finish updating?' ]] && jq -r .body <<<'$D' | grep -qF '1 Agent is running. It stops now; Session Restore reopens it as interrupted — send “continue” to go on.' && jq -r .body <<<'$D' | grep -qF '1 Agent is waiting for you: asker. Session Restore reopens it as interrupted.'"
up respond '"not_now"' >/dev/null; sleep 0.5
check "R127-l Not now closes the dialog and keeps the same app (pid $OLD)" \
    "[[ \$(rpc debug.update '{\"op\":\"dialog\"}' | jq -r .open) == false && \$(rpc app.info | jq -r .pid) == $OLD ]]"
up cell_click >/dev/null; sleep 0.8
rpc debug.update '{"op":"respond","arg":"restart"}' >/dev/null
# nothing answers after this: wait for a new app (rade version --json, 20 s)
NEW=
for _ in $(seq 200); do
    NEW=$(rade version --json 2>/dev/null | jq -r '.app.pid // empty' 2>/dev/null)
    [[ -n $NEW && $NEW != "$OLD" ]] && break
    NEW=; sleep 0.1
done
check "R127-l the old pid ends and within 20 s a new app answers app.info with another pid" "[[ -n '$NEW' && '$NEW' != '$OLD' ]] && ! kill -0 $OLD 2>/dev/null"
wait "$APP_PID" 2>/dev/null; APP_PID=
if [[ -n $NEW ]]; then
    sleep 2
    AFTER=$(rade list --json | jq -cS '[.panes[] | {pane, name}] | sort_by(.pane)')
    check "R127-l rade list shows the same Panes (both Agents interrupted)" \
        "[[ '$AFTER' == '$BEFORE' ]] && [[ \$(rade list --json | jq -r --arg a $run --arg b $ask '[.panes[]|select(.pane==\$a or .pane==\$b)|.status]|join(\",\")') == 'interrupted,interrupted' ]]"
    check "R127-l rade version reads 'app $CUR (pid $NEW)'" "[[ \$(rade version | sed -n 2p) == 'app $CUR (pid $NEW)' ]]"
    # SIGTERM ends the app without unlinking its socket (plan: "socket gone" cannot hold):
    # the process is gone and nothing answers on the stale socket; the test removes it.
    kill "$NEW" 2>/dev/null
    for _ in $(seq 100); do kill -0 "$NEW" 2>/dev/null || break; sleep 0.1; done
    check "R127-l the new app was stopped (pid gone, nothing answers on the socket)" \
        "! kill -0 $NEW 2>/dev/null && ! rade list >/dev/null 2>&1"
    rm -f "$SOCK"
else
    bad "R127-l no new app within 20 s"
fi
fi

cp "$GHLOG" "$OUT/fake-github.log" 2>/dev/null
echo
echo "m48: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
