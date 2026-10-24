# M7 live: a Worktree created through the real New Worktree dialog (keyboard
# only), the Review Pane opened with its key, diff lines selected with the
# real mouse and commented with real keys, a file picked by a real click, and
# "Send to Agent" clicked for a real Codex (`codex --no-daemon`, prompt asks
# for exactly GOT-IT: minimal tokens). Everything lives in the run's temp dir
# (repo + $XDG_DATA_HOME/rade/worktrees); nothing else is touched.
X=$T/files/m7repo
mkdir -p "$X/src"
printf 'fn main() {\n    let x = 1;\n    println!("{}", x);\n}\n' >"$X/src/a.rs"
printf 'def f():\n    return 1\n' >"$X/src/b.py"
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
G -C "$X" init -q -b main && G -C "$X" add -A && G -C "$X" commit -qm init
rade source add "$X" --name lp >/dev/null
LP=$(rade source list --json | jq -r '.sources[] | select(.name == "lp") | .source')
WT=live-wt
WD=$XDG_DATA_HOME/rade/worktrees/$LP/$WT
# This session's Codex trusts only the temp repo and worktree (a `-c`
# override: the owner's ~/.codex/config.toml is not written).
if ! grep -q '^\[agents.codex\]' "$CONFIG"; then
    cat >>"$CONFIG" <<EOF

[agents.codex]
command = "codex --no-daemon -c check_for_update_on_startup=false -c 'sources={\"$X\"={trust_level=\"trusted\"},\"$WD\"={trust_level=\"trusted\"}}'"
EOF
fi
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }

echo "-- M7-1 R12.1 New Worktree dialog with the real keyboard"
key "alt+$(proj_index lp)"
want "M7-1 Alt+N → Source lp" 3 "state | jq -r .active_source" "$LP"
key ctrl+shift+b
sleep 0.8
shot m7-new-worktree-dialog
type_text "$WT"
sleep 0.3
key enter
want "M7-1 dialog (typed name + Enter) created the Worktree" 20 "rade worktree list lp --json | jq -r '.worktrees[] | select(.worktree == \"$WT\") | .path'" "$WD"
want "M7-1 on branch $WT from main" 3 "git -C $WD branch --show-current" "$WT"
want "M7-1 the new Worktree is on screen" 5 "q 'wt | .name'" "$WT"
want "R1.2 sidebar shows the Worktree level (2 Worktrees)" 3 "state | jq -r --arg p $LP '.sources[] | select(.id == \$p) | .worktrees | length'" 2
P=$(cur_pane)
rade send "$P" pwd --enter >/dev/null
want "R12 AC: its Terminal's pwd is the Worktree" 5 "screen_has $P $WD && echo y" y
shot m7-sidebar-worktree

echo "-- M7-2 changes + Codex beside them"
printf 'fn main() {\n    let count = 1;\n    println!("{}", count);\n}\n' >"$WD/src/a.rs"
printf 'def f():\n    return 2\n' >"$WD/src/b.py"
CX=$(rade spawn --source lp --worktree "$WT" --agent codex --name cx --split right --json | jq -r .pane)
rade wait "$CX" --until idle --timeout 90 >/dev/null
if rade read "$CX" | grep -qi 'trust'; then
    bad "M7-2 Codex asks to trust the folder (the -c override did not apply)"
    shot m7-codex-trust
fi
rade focus "$P" >/dev/null
key ctrl+shift+r
want "M7-2 Ctrl+Shift+R opens the Review Pane (focused)" 5 "q '[tab | .layout | .. | objects | select(.kind?.type? == \"review\")][0].id'" "$(cur_pane)"
RV=$(cur_pane)
want "M7-2 diff: 2 files" 15 "rade debug review $RV info --json | jq -r '.files | length'" 2
{ git -C "$WD" status --short; git -C "$WD" diff --stat main; stat -c '%y %s %n' "$WD"/src/* "$(git -C "$WD" rev-parse --absolute-git-dir)/index"; date +%T.%N; rade debug review "$RV" info --json; rade debug review "$RV" refresh >/dev/null; sleep 3; rade debug review "$RV" info --json | jq -c .files; } >"$OUT/m7-diff-state.txt" 2>&1
sleep 0.5
shot m7-review

echo "-- M7-3 comments with the real mouse and keys"
rade debug review "$RV" select src/a.rs >/dev/null
sleep 0.5
read -r x1 y1 <<<"$(rade debug review "$RV" locate 'let count' --json | jq -r '"\(.x) \(.y)"')"
read -r x2 y2 <<<"$(rade debug review "$RV" locate 'count);' --json | jq -r '"\(.x) \(.y)"')"
if drag "$x1" "$y1" "$((${x2%.*} + 60))" "$y2"; then
    key c
    want "M7-3 'c' opens the comment bar" 3 "rade debug review $RV info --json | jq -r .comment_bar" true
    type_text "Do not edit files. Reply with exactly: GOT-IT"
    key enter
    want "M7-3 comment on the dragged lines: src/a.rs:2-3" 5 "rade review list --source lp --worktree $WT --json | jq -r '.comments[0] | \"\(.path):\(.line)-\(.end_line)\"'" "src/a.rs:2-3"
else
    bad "M7-3 drag over the diff lines (pointer not placed)"
fi
if read -r x y <<<"$(widget_xy src/b.py)" && click "$x" "$y"; then
    want "M7-3 real click on the file row shows src/b.py" 3 "rade debug review $RV info --json | jq -r .selected" src/b.py
    sleep 0.3
    read -r x y <<<"$(rade debug review "$RV" locate 'return 2' --json | jq -r '"\(.x) \(.y)"')"
    if click "$x" "$y"; then
        key c
        type_text "Same here: only reply GOT-IT."
        key enter
        want "M7-3 second comment: src/b.py:2" 5 "rade review list --source lp --worktree $WT --json | jq -r '.comments[1] | \"\(.path):\(.line)\"'" "src/b.py:2"
    else
        bad "M7-3 click on a diff line (pointer not placed)"
    fi
else
    bad "M7-3 file row click (pointer not placed)"
fi
sleep 0.3
shot m7-review-comments

echo "-- M7-4 R13 AC: Send to Agent (real click) → one prompt in Codex"
if read -r x y <<<"$(widget_xy 'Send all Review Comments as one prompt')" && click "$x" "$y"; then
    want "M7-4 comments cleared after sending" 5 "rade review list --source lp --worktree $WT --json | jq '.comments | length'" 0
    want "M7-4 Codex finished its turn" 180 "rade wait $CX --until done --timeout 5 2>/dev/null" done
    want "M7-4 Codex received both path:line references" 5 "rade read $CX --scrollback | grep -c -e 'src/a.rs:2-3' -e 'src/b.py:2' | awk '{print (\$1 >= 2)}'" 1
    want "M7-4 Codex replied GOT-IT" 5 "rade read $CX --scrollback | grep -v 'exactly' | grep -c 'GOT-IT' | awk '{print (\$1 >= 1)}'" 1
    shot m7-codex-got-it
else
    bad "M7-4 Send button click (pointer not placed)"
fi

echo "-- cleanup: the run's Worktree (temp repo only)"
rade close "$CX" --force >/dev/null 2>&1
rade worktree remove lp "$WT" --force --delete-branch >/dev/null
check "M7-5 Worktree and branch removed (temp repo)" bash -c "[[ ! -e $WD ]] && ! git -C $X rev-parse -q --verify refs/heads/$WT"
