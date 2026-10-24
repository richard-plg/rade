#!/usr/bin/env bash
# M44 end-to-end tests (ADR-0006): the Review Loop. Part 1 (R118): the
# header names the Worktree's Agent, its task and commits; the files rail
# is a tree with lock / generated files folded last; Viewed per file (reset
# when the file changes); comments written and shown inline; the Pane's
# keys; whitespace hidden; Impact as a right rail; Discard… asks; spec H
# rows (tests/e2e/spec_h.py) and screenshots in dark and light. Temp git
# repo, temp HOME, the stand-in Agent (as `claude`), driven by the CLI.
# Usage: tests/e2e/m44.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m44
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
export FAKE_AGENT=claude FAKE_PROMPTS=$T/prompts.txt
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.claude]
command = "$FAKE"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
R=$T/api
mkdir -p "$R/src/auth" "$R/tests"
printf 'pub fn login(user: &str) -> bool {\n    !user.is_empty()\n}\n' >"$R/src/auth/login.rs"
printf 'pub fn otp() -> u32 {\n    0\n}\n' >"$R/src/auth/otp.rs"
printf 'fn main() {\n    println!("api");\n}\n' >"$R/src/main.rs"
printf '# api\n\nThe API.\n' >"$R/README.md"
printf '[[package]]\nname = "api"\nversion = "0.1.0"\n' >"$R/Cargo.lock"
printf '#[test]\nfn t() {}\n' >"$R/tests/login.rs"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init

rinfo() { rade debug review "$rv" info --json; }
rv_key() { rade debug review "$rv" key "$1" >/dev/null; }

echo "== start"
start_app
rade source add "$R" --name api >/dev/null
rade worktree add api feat >/dev/null
W=$(rade worktree list api --json | jq -r '.worktrees[] | select(.worktree=="feat") | .path')
# Three commits on the branch, then uncommitted work.
printf 'pub fn otp() -> u32 {\n    let code = 123456;\n    code\n}\n' >"$W/src/auth/otp.rs"
G -C "$W" commit -qam "Add an OTP code"
printf 'pub fn login(user: &str, code: u32) -> bool {\n    !user.is_empty() && code == crate::auth::otp::otp()\n}\n' >"$W/src/auth/login.rs"
G -C "$W" commit -qam "Check the OTP at login"
printf '[[package]]\nname = "api"\nversion = "0.2.0"\n' >"$W/Cargo.lock"
G -C "$W" commit -qam "Bump the version"
printf 'fn main() {\n        println!("api");\n}\n' >"$W/src/main.rs"          # whitespace only
printf '#[test]\nfn t() {\n    assert!(true);\n}\n' >"$W/tests/login.rs"
printf '# OTP\n\nOne-time codes for login.\n' >"$W/docs-otp.md"           # untracked

echo "== R118.1 header: Agent, task, commits"
ag=$(rade spawn --agent claude --source api --worktree feat "implement OTP login" --json | jq -r .pane)
eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$ag\") | .status'" done || true
rv=$(rade review open --source api --worktree feat)
check "R118-e diff loads: 6 files" eventually 20 is "rinfo | jq -r '.files | length'" 6
check "R118-e header: branch feat ← main" eventually 5 is "rinfo | jq -r '.head.branch + \" \" + .head.base'" "feat main"
check "R118-e header: the Agent claude with its glyph" eventually 5 is "rinfo | jq -r '.head.agent + \" \" + .head.glyph.glyph'" "claude claude"
check "R118-e header: the task it was spawned with" test "$(rinfo | jq -r .head.task)" = "implement OTP login"
check "R118-e header: 3 commits" test "$(rinfo | jq -r .head.commits)" = "3 commits"
check "R118-e …the popover lists them newest first" bash -c "rade debug review $rv info --json | jq -r '.commit_list[0]' | grep -q 'Bump the version'"

echo "== R118.2 rail: tree, generated last"
rail() { rinfo | jq -r '[.rail[] | if .kind=="file" then .path elif .kind=="folder" then .path else "generated" end] | join(" ")'; }
check "R118-e rail: files in a tree, Cargo.lock in the folded generated group" test "$(rail)" = "docs-otp.md src/ src/auth/ src/auth/login.rs src/auth/otp.rs src/main.rs tests/ tests/login.rs generated"
check "R118-e …indented by depth" test "$(rinfo | jq -r '[.rail[] | select(.path=="src/auth/otp.rs") | .indent][0]')" = 38
check "R118-e …Cargo.lock folded in the diff too" test "$(rinfo | jq -r '[.files[] | select(.path=="Cargo.lock") | .folded][0]')" = true
check "R118-e …and last in the diff" test "$(rinfo | jq -r '.files[-1].path')" = Cargo.lock

echo "== R118.3 Viewed"
check "R118-e 0/6 viewed" test "$(rinfo | jq -r .head.viewed)" = "0/6 viewed"
rade debug review "$rv" select docs-otp.md >/dev/null
rv_key v
check "R118-e v: docs-otp.md viewed, 1/6" eventually 5 is "rinfo | jq -r .head.viewed" "1/6 viewed"
check "R118-e …folded in the diff" test "$(rinfo | jq -r '[.files[] | select(.path=="docs-otp.md") | .folded][0]')" = true
check "R118-e …the rail ticks it" test "$(rinfo | jq -r '[.rail[] | select(.path=="docs-otp.md") | .viewed][0]')" = true
check "R118-e …and the next unread file is selected" test "$(rinfo | jq -r .selected)" = src/auth/login.rs
rv_key v
check "R118-e v again: login.rs viewed too, 2/6" eventually 5 is "rinfo | jq -r .head.viewed" "2/6 viewed"
printf '# OTP\n\nOne-time codes.\n' >"$W/docs-otp.md"
check "R118-e a viewed file that changes is unread again" eventually 10 is "rinfo | jq -r .head.viewed" "1/6 viewed"
check "R118-e …and unfolds" test "$(rinfo | jq -r '[.files[] | select(.path=="docs-otp.md") | .folded][0]')" = false

echo "== R118.6 whitespace"
check "R118-e src/main.rs has a hunk" test "$(rinfo | jq -r '[.files[] | select(.path=="src/main.rs") | .hunks][0]')" = 1
rv_key w
check "R118-e w hides the whitespace-only change" eventually 10 is "rinfo | jq -r '[.files[] | select(.path==\"src/main.rs\")] | length'" 0
check "R118-e …the option shows checked" test "$(rinfo | jq -r .head.whitespace)" = true
check "R118-e …Viewed marks survive it" test "$(rinfo | jq -r .head.viewed)" = "1/5 viewed"
rv_key w
check "R118-e w again shows it" eventually 10 is "rinfo | jq -r '[.files[] | select(.path==\"src/main.rs\")] | length'" 1

echo "== R118.4 inline comments"
rade debug review "$rv" select src/auth/otp.rs >/dev/null
rade debug review "$rv" comment 'let code = 123456;|let code = 123456;|Read it from the config.' >/dev/null
check "R118-e the card sits right under the commented line" bash -c "rade debug review $rv info --json | jq -r .visible_text | grep -A1 -x '    let code = 123456;' | tail -1 | grep -qx '\[comment\] Read it from the config.'"
check "R118-e …the footer counts 1 draft" test "$(rinfo | jq -r .drafts)" = "1 draft"
check "R118-e …the primary says Request changes (1)" test "$(rinfo | jq -r .primary)" = "Request changes (1)"
check "R118-e …the rail counts it on otp.rs" test "$(rinfo | jq -r '[.rail[] | select(.path=="src/auth/otp.rs") | .comments][0]')" = 1
echo "== R118.4 the composer takes the keyboard"
rade debug review "$rv" select tests/login.rs >/dev/null
rade debug review "$rv" point 'assert!(true);|assert' >/dev/null
rv_key c
sleep 0.5
rinfo | jq -c '{composer_focus, focus, composer}' >"$OUT/focus.json"
check "R118-e c: the composer has the keyboard" eventually 3 is "rinfo | jq -r .composer_is_focus" true
echo "== R118.5 keys"
rade debug review "$rv" select docs-otp.md >/dev/null
rv_key N
check "R118-e N: next file header" test "$(rinfo | jq -r .selected)" = src/auth/login.rs
rv_key bracketright
check "R118-e ]: the comment card" bash -c "c=\$(rade debug review $rv info --json); test \"\$(echo \"\$c\" | jq -r .cursor)\" = \"\$(echo \"\$c\" | jq -r '.cards[0].row')\""
rv_key n
check "R118-e n: the next hunk after the card" bash -c "rade debug review $rv info --json | jq -e '.cursor > .cards[0].row' >/dev/null"
rv_key P
check "R118-e P: back to a previous file header" bash -c "rade debug review $rv info --json | jq -e '.selected != null' >/dev/null"

echo "== R118.7 Impact rail"
rv_key i
check "R118-e i: Impact shows as a rail" eventually 5 is "rinfo | jq -r .impact_rail.shown" true
rv_key i
check "R118-e i again hides it" eventually 5 is "rinfo | jq -r .impact_rail.shown" false

echo "== screenshots"
rade debug review "$rv" select src/auth/otp.rs >/dev/null
sleep 0.5
rade debug action color_scheme_dark >/dev/null
sleep 0.6
shot review-loop-dark
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
TOKENS=$ROOT/crates/rade-app/src/tokens.toml
spec_h() { python3 "$HERE/spec_h.py" "$SOCK" "$TOKENS" "$1" "$rv" >"$OUT/spec-h-$1.txt" 2>&1; }
check "R118-e spec H rows, dark ($OUT/spec-h-dark.txt)" spec_h dark
rade debug action color_scheme_light >/dev/null
sleep 0.6
shot review-loop-light
check "R118-e spec H rows, light ($OUT/spec-h-light.txt)" spec_h light

echo "== R118.8 Discard… asks"
rade debug review "$rv" discard >/dev/null
check "R118-e Discard… opens a confirmation, the draft stays" test "$(rinfo | jq -r .drafts)" = "1 draft"
check "R118-e …naming the count" test "$(rinfo | jq -r .discard_dialog)" = "Discard 1 draft comment?"
rade debug review "$rv" discard_confirm >/dev/null
check "R118-e …confirmed: no drafts" eventually 5 is "rinfo | jq -r .drafts" ""

echo "== R119.1 Request changes: a Round"
ui() { rpc debug.ui "$1"; }
prompts() { cat "$FAKE_PROMPTS" 2>/dev/null; }
cstate() { rinfo | jq -r --arg id "$1" '.card_states[] | select(.id==($id|tonumber)) | .state'; }
G -C "$W" config user.email t@t; G -C "$W" config user.name t
rade debug review "$rv" select src/auth/otp.rs >/dev/null
rade debug review "$rv" comment 'let code = 123456;|let code = 123456;|Read it from the config.' >/dev/null
rade debug review "$rv" select tests/login.rs >/dev/null
rade debug review "$rv" comment 'assert!(true);|assert!(true);|Test the real thing.' >/dev/null
c1=$(rinfo | jq -r '.card_states[] | select(.round==null) | .id' | head -1)
c2=$(rinfo | jq -r '.card_states[] | select(.round==null) | .id' | tail -1)
check "R119-e two drafts: Request changes (2)" test "$(rinfo | jq -r .primary)" = "Request changes (2)"
sleep 1.1 # the Agent's last turn ended in an earlier second than the Round
rade debug review "$rv" send >/dev/null
check "R119-e the Agent got one prompt headed Review round 1" eventually 10 bash -c "grep -c 'Review round 1: comments on the changes in this worktree (feat)' '$FAKE_PROMPTS' | grep -qx 1"
check "R119-e …with both places" bash -c "grep 'Review round 1' '$FAKE_PROMPTS' | grep -q 'src/auth/otp.rs:2' && grep 'Review round 1' '$FAKE_PROMPTS' | grep -q 'tests/login.rs:3'"
check "R119-e a snapshot ref for round 1" bash -c "git -C '$W' for-each-ref --format='%(refname)' refs/rade/review/ | grep -qx refs/rade/review/feat/1"
check "R119-e the comments stay, sent in round 1" test "$(rinfo | jq -r '[.card_states[] | .round | tostring] | join(",")')" = "1,1"
check "R119-e header: Round 1 · changes requested" eventually 5 is "rinfo | jq -r .head.round" "Round 1 · changes requested"
check "R119-e footer: 2 open, the primary is Approve" test "$(rinfo | jq -r '.drafts + "|" + .primary')" = "2 open|Approve"
check "R119-e Navigator: the Worktree row says R1 · 2 open" eventually 5 is "ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.title==\"feat\") | .review'" "R1 · 2 open"
check "R119-e sent, the Agent's turn not ended yet or just ended" bash -c "s=\$(rade debug review $rv info --json | jq -r '.card_states[0].state'); [[ \$s == sent || \$s == untouched ]]"

echo "== R119.2 / R119.3 the Agent's turn: since round 1, comment states"
check "R119-e the Agent's turn ends (done)" eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$ag\") | .status'" done
# What the Agent did: the commented OTP line changed, a new file.
printf 'pub fn otp() -> u32 {\n    let code = config::otp_code();\n    code\n}\n' >"$W/src/auth/otp.rs"
printf 'pub fn otp_code() -> u32 { 42 }\n' >"$W/src/config.rs"
check "R119-e a Review Toast for the finished Agent" eventually 10 is "ui '{\"op\":\"chrome\"}' | jq -r '.chrome.review_toasts | length > 0'" true
ui '{"op":"toast_review"}' >/dev/null
check "R119-e Since round 1 lists the 2 files changed after it" eventually 15 is "rinfo | jq -r .head.scope_since" "Since round 1  2"
check "R119-e …shown first (the Toast's Review)" eventually 10 is "rinfo | jq -r '[.files[].path] | join(\" \")'" "src/auth/otp.rs src/config.rs"
check "R119-e the OTP comment: code changed" eventually 10 is "cstate $c1" changed
check "R119-e the test comment: untouched" test "$(cstate "$c2")" = untouched
check "R119-e All changes still lists every file" bash -c "rade debug review $rv scope all --json | jq -e '.files | length == 7' >/dev/null"
rade debug review "$rv" scope since >/dev/null

echo "== R119.3 Resolve, Follow up"
rade debug review "$rv" resolve "$c2" >/dev/null
check "R119-e resolved: 1 open" eventually 5 is "rinfo | jq -r .drafts" "1 open"
rade debug review "$rv" follow_up "$c1|Use the env var, not a file." >/dev/null
check "R119-e a follow-up draft: Request changes (1)" test "$(rinfo | jq -r .primary)" = "Request changes (1)"
sleep 1.1
rade debug review "$rv" send >/dev/null
check "R119-e the follow-up quotes the lines as they are now" eventually 10 bash -c "grep 'Review round 2' '$FAKE_PROMPTS' | grep -qF 'let code = config::otp_code();'"
check "R119-e round 2's prompt quotes round 1's comment" eventually 10 bash -c "grep 'Review round 2' '$FAKE_PROMPTS' | grep -q 'Re: round 1, src/auth/otp.rs:2: \"Read it from the config.\"'"
check "R119-e Navigator: R2 · 2 open" eventually 5 is "ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.title==\"feat\") | .review'" "R2 · 2 open"

echo "== R119.4 Approve"
check "R119-e the Agent's turn ends again" eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$ag\") | .status'" done
check "R119-e Approve menu: ask commit, ask commit + PR, commit myself, create PR" test "$(rinfo | jq -r '.approve_items | join("|")')" = "Ask Agent to commit|Ask Agent to commit and open a PR|Commit myself…|Create PR…"
sleep 1.1
rade debug review "$rv" approve ask_commit >/dev/null
check "R119-e the Agent is asked to commit (round 3, no push)" eventually 10 bash -c "grep -q 'Approved (review round 3). Commit these changes following this repository.s conventions. Do not push.' '$FAKE_PROMPTS'"
check "R119-e header: Round 3 · approved" eventually 5 is "rinfo | jq -r .head.round" "Round 3 · approved"
check "R119-e Navigator: approved" eventually 5 is "ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.title==\"feat\") | .review'" approved
printf '// later\n' >>"$W/src/config.rs"
check "R119-e a change after approval: changed after approval" eventually 15 is "rinfo | jq -r .head.round" "Round 3 · changed after approval"
check "R119-e …the Navigator says so too" eventually 5 is "ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.title==\"feat\") | .review'" "R3 · changed"

echo "== R119.6 the Agent's side"
st() { rade review status --source api --worktree feat --json; }
check "R119-e review status: 3 rounds, the last changed after approval" test "$(st | jq -r '[.rounds[].state] | join(",")')" = "changes_requested,changes_requested,changed_after_approval"
check "R119-e …comments with round, state, resolved" test "$(st | jq -r '[.comments[] | "\(.round):\(.state):\(.resolved // false)"] | join(",")')" = "1:changed:false,1:untouched:true,2:untouched:false"
check "R119-e …text output" bash -c "rade review status --source api --worktree feat | head -1 | grep -qx 'Round 3 · changed after approval · 2 open · [0-9]*/[0-9]* viewed'"
rade review request --source api --worktree feat --summary 'Done: **OTP** from config; tests next.' >/dev/null
check "R119-e request: the summary in the header (Markdown stripped)" eventually 5 is "rinfo | jq -r .head.summary" "Done: OTP from config; tests next."
inbox_review() { rpc debug.ui '{"op":"inbox"}' | jq -e '.inbox.rows[] | select(.text | test("Ready for review: Done")) | select(.review==true)' >/dev/null; }
check "R119-e …a Ready for review row in the Inbox with a Review button" eventually 5 inbox_review

echo "== R119-g screenshots"
rade debug review "$rv" scope all >/dev/null
rade debug review "$rv" select src/auth/otp.rs >/dev/null
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null 2>&1 || true
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+, $2}}"; }
# Golden masks: Terminal contents, running glyphs, times, the note line.
masks() {
    local args=() w
    for w in terminal glyph-running nav-row-meta review-commit-age review-note; do args+=("$(geo "$w" '"all":true')"); done
    python3 - "${args[@]}" <<'PY' >"$1"
import json, sys
out = []
for arg in sys.argv[1:]:
    for g in json.loads(arg):
        if g.get("visible", True):
            out.append({"x": g["x"] - 2, "y": g["y"] - 2, "w": g["width"] + 4, "h": g["height"] + 4,
                        "why": ",".join(g["classes"])[:40]})
print(json.dumps(out, indent=1))
PY
}
GOLD=$HERE/golden/m44
mkdir -p "$GOLD"
for th in dark light; do
    rade debug action color_scheme_$th >/dev/null
    sleep 0.8
    shot review-round-$th
    masks "$OUT/masks-h-$th.json"
    if [[ ! -s $GOLD/h-$th.png && ${APPROVE:-} == 1 ]]; then
        cp "$OUT/review-round-$th.png" "$GOLD/h-$th.png" && echo "  approved (first run): h-$th"
    fi
    check "R119-g golden H $th within threshold (tests/e2e/golden/m44/, APPROVE=1 writes it)" \
        python3 "$HERE/golden-diff.py" "$GOLD/h-$th.png" "$OUT/review-round-$th.png" --masks "$OUT/masks-h-$th.json" --out "$OUT/h-$th-diff.png"
done
rade debug action color_scheme_light >/dev/null

echo "== R119-f restore"
kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true
start_app
check "R119-f rounds, states, summary back after kill -9" eventually 10 is "st | jq -r '[(.rounds | length), (.open), (.summary)] | join(\"|\")'" "3|2|Done: **OTP** from config; tests next."
rv=$(rade review open --source api --worktree feat)
check "R119-f the Review shows Round 3 and the summary again" eventually 15 is "rinfo | jq -r '.head.round + \"|\" + .head.summary'" "Round 3 · changed after approval|Done: OTP from config; tests next."
check "R119-f Viewed marks back (login.rs, unchanged since)" test "$(rinfo | jq -r .head.viewed)" = "1/7 viewed"

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
