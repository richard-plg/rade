# M44 live (R118.4) on the real desktop with real input: in a Review Pane a
# real click selects a diff line, a real `c` opens the comment composer, real
# typing fills it, a real Enter adds the comment (a card), a real `c` + Esc
# cancels; a real click into the composer keeps the keyboard there. Each
# step records where the keyboard focus is (`debug.review info` → `focus`).
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) tests/live/run.sh m44
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LA=$T/files/ws-m44/api
mkdir -p "$LA/src"
printf 'pub fn otp() -> u32 {\n    0\n}\n' >"$LA/src/otp.rs"
LG -C "$LA" init -q -b main && LG -C "$LA" add -A && LG -C "$LA" commit -qm init
rade source add "$LA" >/dev/null
rade debug action color_scheme_light >/dev/null
FT=$(rade worktree add api feat)
printf 'pub fn otp() -> u32 {\n    let code = 123456;\n    code\n}\n' >"$FT/src/otp.rs"
RV=$(rade review open --source api --worktree feat)
sleep 1.5
rinfo() { rade debug review "$RV" info --json | jq -r "$1"; }
focus() { rinfo '.focus // [] | join(" < ")'; }
line_xy() { rade debug review "$RV" locate "$1" --json | jq -r '"\(.x) \(.y)"'; }

echo "-- R118.4 a real click on a line, a real c, real typing"
read -r x y <<<"$(line_xy 'let code = 123456;')"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    echo "  focus after the click: $(focus)"
    key c
    sleep 0.6
    echo "  focus after c: $(focus)"
    want "c opens the composer" 3 "rinfo '.composer != null'" true
    want "…and the composer has the keyboard" 3 "rinfo .composer_focus" true
    type_text "hello"
    echo "  focus after typing: $(focus)"
    want "real typing lands in the composer" 3 "rinfo .composer.text" hello
    shot m44-composer-light
    key enter
    want "a real Enter adds the comment (a card)" 3 "rinfo '.drafts'" "1 draft"
    want "…and closes the composer" 3 "rinfo '.composer == null'" true
else
    bad "real click on the diff line (pointer not placed)"
fi

echo "-- R118.4 a real c then Esc cancels"
read -r x y <<<"$(line_xy '    code')"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    key c
    sleep 0.6
    want "c opens the composer again" 3 "rinfo '.composer != null'" true
    key esc
    want "a real Esc closes it" 3 "rinfo '.composer == null'" true
    echo "  focus after Esc: $(focus)"
fi

echo "-- R118.4 a real click into the composer keeps the keyboard"
read -r x y <<<"$(line_xy '    code')"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    key c
    sleep 0.6
    read -r cx cy <<<"$(widget_xy review-composer-text 0)"
    if [[ -n $cx && $cx != null ]] && click "$cx" "$cy"; then
        echo "  focus after clicking the composer: $(focus)"
        type_text "abc"
        want "typing after a click into it lands there" 3 "rinfo .composer.text" abc
        key esc
        want "Esc closes it" 3 "rinfo '.composer == null'" true
    else
        bad "real click into the composer (pointer not placed)"
    fi
fi
