# M9 live: a Fan-out of two real Codex Agents started from the palette's
# Fan-out… form with the real keyboard, a real read-only Codex Judge started
# by a real click on the bar's Judge button, and the Winner chosen through
# the real Choose Winner… button + dialog (squash onto main, then cleanup).
# Tiny task ("create hello.txt containing hi"): a few tokens per Agent.
# Everything lives in the run's temp dir: the repo, the Worktrees
# ($XDG_DATA_HOME/rade/worktrees) and the Judge's diff folder. Codex trusts
# only those folders through a `-c` override on its own Preset (`m9codex`);
# ~/.codex/config.toml is not written.
X=$T/files/m9repo
mkdir -p "$X"
printf '# m9\n' >"$X/README.md"
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
G -C "$X" init -q -b main && G -C "$X" add -A && G -C "$X" commit -qm init
rade source add "$X" --name lm9 >/dev/null
LM=$(rade source list --json | jq -r '.sources[] | select(.name == "lm9") | .source')
WTS=$XDG_DATA_HOME/rade/worktrees/$LM
if ! grep -q '^\[agents.m9codex\]' "$CONFIG"; then
    trust="\"$X\"={trust_level=\"trusted\"},\"$WTS/fanout-1-1\"={trust_level=\"trusted\"},\"$WTS/fanout-1-2\"={trust_level=\"trusted\"}"
    cat >>"$CONFIG" <<EOF

[agents.m9codex]
command = "codex --no-daemon -c check_for_update_on_startup=false -c 'sources={${trust//\"/\\\"}}'"
hooks = "codex"
resume = "codex --no-daemon -c check_for_update_on_startup=false resume {session_id}"

[fanout]
judge = "m9codex"
EOF
    sleep 1 # live reload
fi
want "M9 config reloaded with the m9codex Preset" 5 "rade --json debug palette '>spawn m9codex' | jq -r '.results[0].title'" "Spawn m9codex"
pinfo() { rade --json debug palette --ui info | jq -r "$1"; }
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }
fst() { rade fanout status fanout-1 --source lm9 --json | jq -r "$1"; }
# The form lists every Preset by name (sorted): built-ins plus [agents.*].
presets=$( { printf '%s\n' agy claude codex opencode; grep -o '^\[agents\.[A-Za-z0-9_-]*\]' "$CONFIG" | sed 's/^\[agents\.//; s/\]$//'; } | sort -u)

key "alt+$(proj_index lm9)"
want "M9 Alt+N → Source lm9" 3 "state | jq -r .active_source" "$LM"

echo "-- M9-1 R15.1 palette Fan-out… form with the real keyboard"
key ctrl+shift+p
type_text ">fan-out"
want "M9-1 'Fan-out…' is the first row" 3 "pinfo '.rows[0].title'" "Fan-out…"
key enter
want "M9-1 the form opens on its Prompt field" 3 "pinfo '\"\(.form.title) \(.form.focus)\"'" "Fan-out prompt"
type_text "create a file hello.txt containing hi"
key tab
for p in $presets; do # one spin button per Preset: 2 for m9codex, 0 for the rest
    [[ $p == m9codex ]] && type_text 2 || type_text 0
    key tab
done
want "M9-1 Agents set by typing into the counts: m9codex × 2" 3 "pinfo '.form.fields[1].value'" "m9codex=2"
want "M9-1 Tab left the counts for Base" 3 "pinfo .form.focus" base
shot m9-form
key enter
want "M9-1 Enter started fanout-1 (2 Worktrees)" 30 "fst '.members | map(.worktree) | join(\",\")'" "fanout-1-1,fanout-1-2"
want "M9-1 the Fan-out Tab is on screen" 5 "q 'tab | .name'" fanout-1
want "M9-1 keyboard on its first Agent" 5 "rade debug window --json | jq -r .focus.pane" "$(fst '.members[0].pane')"

echo "-- M9-2 both real Agents finish"
rade fanout wait fanout-1 --source lm9 --timeout 420 >/dev/null
want "M9-2 2/2 done" 5 "fst '\"\(.phase) \(.done)/\(.total)\"'" "finished 2/2"
for i in 1 2; do
    want "M9-2 fanout-1-$i has hello.txt = hi" 3 "cat $WTS/fanout-1-$i/hello.txt 2>/dev/null | tr -d '\n'" hi
done
check "M9-2 the repo's main checkout is untouched" test ! -e "$X/hello.txt"
for i in 1 2; do
    if rade read "$(fst ".members[$((i - 1))].pane")" | grep -qi 'trust'; then
        bad "M9-2 Codex $i asked to trust its folder (the -c override did not apply)"
    fi
done
want "M9-2 bar: sizes shown" 10 "rade debug fanout fanout-1 info | jq -r '.members[0].stats'" "1 file +1 −0"
shot m9-fanout-done

echo "-- M9-3 R15.3 real Judge (read-only Codex) by a real click on Judge"
read -r jx jy <<<"$(widget_xy rade-fanout-judge)"
click "$jx" "$jy"
want "M9-3 the click started the Judge" 10 "fst '.judge.agent'" m9codex
check "M9-3 it runs read-only (codex read-only sandbox)" test "$(fst .judge.read_only)" = true
JP=$(fst .judge.pane)
check "M9-3 its command has --sandbox read-only" bash -c "rade debug state | jq -r '.. | objects | select(.id? == \"$JP\") | .kind.command' | grep -q -- '--sandbox read-only --ask-for-approval never'"
rade fanout wait fanout-1 --source lm9 --until judged --timeout 420 >/dev/null
want "M9-3 its verdict was read from the TUI (a real Agent label)" 5 "fst '.judge.pick | test(\"^m9codex-[12]$\")'" true
check "M9-3 the Judge changed nothing (both Worktrees still 1 new file)" \
    test "$(git -C "$WTS/fanout-1-1" status --porcelain | wc -l) $(git -C "$WTS/fanout-1-2" status --porcelain | wc -l) $(git -C "$X" status --porcelain | wc -l)" = "1 1 0"
PICK=$(fst .judge.pick)
want "M9-3 bar: Judge picks $PICK" 5 "rade debug fanout fanout-1 info | jq -r .verdict" "Judge picks $PICK"
shot m9-judged

echo "-- M9-4 R15.4 Choose Winner… with a real click and the dialog's keys"
HEAD0=$(git -C "$X" rev-parse HEAD)
read -r cx cy <<<"$(widget_xy rade-fanout-choose)"
click "$cx" "$cy"
sleep 1
shot m9-choose-dialog
want "M9-4 the dialog pre-selects the Judge's pick" 3 "rade debug window --widget rade-fanout-who --json | jq -r '.widget != null'" true
key enter
want "M9-4 Enter chose the Winner" 60 "rade fanout list --source lm9 --json | jq '.fanouts | length'" 0
key esc # the "Winner chosen" note
want "M9-4 hello.txt (hi) is staged on main" 5 "git -C $X show :hello.txt 2>/dev/null" hi
check "M9-4 no commit was made" test "$(git -C "$X" rev-parse HEAD)" = "$HEAD0"
check "M9-4 cleanup removed both Worktrees" bash -c "! test -e $WTS/fanout-1-1 && ! test -e $WTS/fanout-1-2"
check "M9-4 …and their branches" bash -c "! git -C $X rev-parse -q --verify refs/heads/fanout-1-1 && ! git -C $X rev-parse -q --verify refs/heads/fanout-1-2"
check "M9-4 the Fan-out Tab closed" test "$(state | jq --arg p "$LM" '[.sources[] | select(.id == $p) | .worktrees[0].tabs[] | select(.name == "fanout-1")] | length')" = 0
shot m9-after
G -C "$X" reset -q --hard
