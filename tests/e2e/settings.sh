#!/usr/bin/env bash
# Settings E2E (PRD R112, ADR-0037): the Settings page (Ctrl+,), its search,
# live edits of config.toml that keep comments (a Terminal re-lays out without a
# restart), reset, a bad value, the color scheme, and the bundled Maple Mono
# reaching Ghostty's fontconfig. Screenshots dark + light → target/e2e-settings/.
# Usage: tests/e2e/settings.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-settings
source "$HERE/lib.sh"

CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$(dirname "$CONF")"
printf '# my settings\nidle_seconds = 5 # keep\n' >"$CONF"
start_app
ui() { rpc debug.ui "$1"; }
st() { ui '{"op":"app_settings"}'; }
cols() { st | jq '[.terminals[].columns] | max'; }
export -f rpc ui st
eventually 10 bash -c '[[ $(rpc debug.ui "{\"op\":\"app_settings\"}" | jq "[.terminals[].columns] | max") -gt 0 ]]'
C0=$(cols)

# R112-e: Ctrl+, opens the page; search narrows rows and categories.
ui '{"op":"key","keys":"ctrl+comma"}' >/dev/null
check "R112-e Ctrl+, opens Settings with every category" \
    is "st | jq -c '[.open, .categories]'" '[true,["Terminal","Code","Theme","Notifications","Agents","Source Control","Statusline"]]'
check "R112-e defaults: Maple Mono, Medium, 14, nothing modified" \
    is "st | jq -c '[(.rows[] | select(.key==\"terminal.font\") | .value), (.rows[] | select(.key==\"terminal.font_style\") | .value), (.rows[] | select(.key==\"terminal.font_size\") | .value), .modified]'" \
    '["Maple Mono","Medium","14",""]'
check "R112-e the font list has the bundled Maple Mono and its Medium style" \
    bash -c 'rpc debug.ui "{\"op\":\"app_settings\"}" | jq -e "(.fonts | index(\"Maple Mono\")) and (.styles | index(\"Medium\"))" >/dev/null'
shot settings-dark
ui '{"op":"app_settings_search","arg":"font size"}' >/dev/null
check "R112-e search 'font size' → the two size rows, categories Terminal and Code" \
    is "st | jq -c '[[.rows[].key], .categories]'" '[["terminal.font_size","code_font.size"],["Terminal","Code"]]'
ui '{"op":"app_settings_search","arg":"zzz"}' >/dev/null
check "R112-e search with no match → no categories" is "st | jq -c '.categories'" '[]'
ui '{"op":"app_settings_search","arg":""}' >/dev/null

# R112-e: a change is written at once, keeps comments, re-lays out the Terminals.
ui '{"op":"app_settings_set","key":"terminal.font_size","value":24}' >/dev/null
check "R112-e font size 24: config.toml keeps its lines and gains [terminal] font_size = 24" \
    is "cat '$CONF'" "$(printf '# my settings\nidle_seconds = 5 # keep\n\n[terminal]\nfont_size = 24')"
check "R112-e the row is modified, 1 modified" \
    is "st | jq -c '[(.rows[] | select(.key==\"terminal.font_size\") | .modified), .modified]'" '[true,"1 modified"]'
check "R112-e a Terminal has fewer columns without a restart (was $C0)" \
    eventually 5 bash -c "(( \$(rpc debug.ui '{\"op\":\"app_settings\"}' | jq '[.terminals[].columns] | max') < $C0 ))"
ui '{"op":"app_settings_reset","key":"terminal.font_size"}' >/dev/null
check "R112-e reset: the key and its emptied table are gone, the comments stay" \
    is "cat '$CONF'" "$(printf '# my settings\nidle_seconds = 5 # keep')"
check "R112-e reset: the Terminal is back to $C0 columns" \
    eventually 5 bash -c "(( \$(rpc debug.ui '{\"op\":\"app_settings\"}' | jq '[.terminals[].columns] | max') == $C0 ))"
ui '{"op":"app_settings_set","key":"terminal.font_size","value":99}' >/dev/null
check "R112-e a value out of range: shown as an error, the file unchanged" \
    bash -c "rpc debug.ui '{\"op\":\"app_settings\"}' | jq -e '.error | test(\"not allowed\")' >/dev/null && ! grep -q font_size '$CONF'"

# R112-h: the code font follows the Terminal unless set (Viewer, Review, Markdown).
check "R112-h code font defaults to the Terminal's: Maple Mono 500, 13 px" \
    bash -c "rpc debug.ui '{\"op\":\"app_settings\"}' | jq -e '.code_css | test(\"Maple Mono.*13px.*font-weight: 500\")' >/dev/null"
check "R112-h the family row shows 'Same as Terminal' unmodified" \
    is "st | jq -c '[.rows[] | select(.key==\"code_font.family\") | .modified]'" '[false]'
ui '{"op":"app_settings_set","key":"terminal.font","value":"JetBrains Mono"}' >/dev/null
check "R112-h a new Terminal font moves the code font with it" \
    bash -c "rpc debug.ui '{\"op\":\"app_settings\"}' | jq -e '.code_css | test(\"JetBrains Mono\")' >/dev/null"
ui '{"op":"app_settings_reset","key":"terminal.font"}' >/dev/null
ui '{"op":"app_settings_set","key":"code_font.family","value":"JetBrains Mono"}' >/dev/null
ui '{"op":"app_settings_set","key":"code_font.size","value":15}' >/dev/null
check "R112-h its own family and size: [code_font] written, CSS JetBrains Mono 15 px Regular" \
    bash -c "grep -q '^family = \"JetBrains Mono\"' '$CONF' && rpc debug.ui '{\"op\":\"app_settings\"}' | jq -e '.code_css | test(\"JetBrains Mono.*15px.*font-weight: 400\")' >/dev/null"
ui '{"op":"app_settings_search","arg":"code"}' >/dev/null
shot settings-code
ui '{"op":"app_settings_search","arg":""}' >/dev/null
ui '{"op":"app_settings_reset","key":"code_font.family"}' >/dev/null
ui '{"op":"app_settings_reset","key":"code_font.size"}' >/dev/null

# R112-f: the color scheme follows the setting live.
ui '{"op":"app_settings_set","key":"appearance.color_scheme","value":"light"}' >/dev/null
check "R112-f color scheme light → the app is light" eventually 3 is "st | jq .dark" false
shot settings-light
ui '{"op":"app_settings_reset","key":"appearance.color_scheme"}' >/dev/null
check "R112-f reset → dark again" eventually 3 is "st | jq .dark" true

ui '{"op":"key","keys":"escape"}' >/dev/null
check "R112-e Esc closes Settings" eventually 3 is "st | jq .open" false

# R112-g: Ghostty's fontconfig sees the bundled fonts (ADR-0037), nothing installed.
FC=$XDG_RUNTIME_DIR/rade/fonts/fonts.conf
check "R112-g fonts.conf includes the system config and RADE's fonts folder" \
    bash -c "grep -q '<include ignore_missing=\"yes\">' '$FC' && grep -q '<dir>$XDG_RUNTIME_DIR/rade/fonts</dir>' '$FC'"
check "R112-g under it 'Maple Mono:style=Medium' resolves to the bundled MapleMono-Medium.ttf" \
    is "FONTCONFIG_FILE='$FC' fc-match -f '%{file}' 'Maple Mono:style=Medium'" "$XDG_RUNTIME_DIR/rade/fonts/MapleMono-Medium.ttf"
check "R112-g no font was installed under the temp HOME" \
    bash -c "! find '$HOME' '$XDG_DATA_HOME' -iname 'MapleMono*' | grep -q ."
stop_app

echo "settings: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
