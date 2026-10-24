#!/usr/bin/env bash
# M16 end-to-end tests (ADR-0006, ADR-0017): Projects (R28), layered config
# (R29), the sidebar grouped by Project (R30), `rade project` / `rade config`
# and the removed M15 aliases (R31), the rade-config skill (R32). A Workspace
# `globex` with Sources backend (main, beacon-dev, beacon-prod), app and web; fake
# Agents only; screenshots light + dark. 1.0 (M36): project.toml v2, one base
# per Source link (R97); the owner's v1 files are migrated on read.
# Usage: tests/e2e/m16.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m16
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin"
user_config() {
    cat >"$CONF" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
[agents.claude]
command = "$FAKE"
[scm]
draft_command = "user-draft"
EOF
}
user_config
# Workspace A's own command for the `fake` Preset (R29-b).
cat >"$T/bin/agent-a.sh" <<EOF
#!/usr/bin/env bash
echo "agent-a in \$RADE_WORKSPACE"
exec "$FAKE" "\$@"
EOF
chmod +x "$T/bin/agent-a.sh"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
spawn() { rade spawn "$@" --json | jq -r .pane; }
state() { rade debug state; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
# Navigator rows (1.0.0, R104): kind:title@depth, a section as «Scratch».
rows() { ui sidebar | jq -r '[.sidebar[] | select(.kind != "agent") | if .kind == "section" then "«\(.title)»" else "\(.kind):\(.title)@\(.depth)" end] | join(" ")'; }
# The row of `kind` titled `title` (the first).
row() { ui sidebar | jq -c --arg k "$1" --arg t "$2" '[.sidebar[] | select(.kind == $k and .title == $t)][0]'; }
# The depth-0 row above a row: its Project, or a Scratch row.
parent_of() { ui sidebar | jq -r --arg k "$1" --arg t "$2" '.sidebar as $r | ($r | map(.kind == $k and .title == $t) | index(true)) as $i | if $i == null then "none" else ([$r[:$i][] | select(.depth == 0)] | last | "\(.kind):\(.title)") end'; }
# Whether the row is in the Scratch section.
in_scratch() { ui sidebar | jq -e --arg t "$1" '.sidebar as $r | ($r | map(.kind == "section") | index(true)) as $s | ($r | map(.title == $t) | index(true)) as $i | $s != null and $i != null and $i > $s' >/dev/null; }
home_name() { ui home | jq -r 'if .home.shown then .home.name else "none" end'; }
export -f ui rpc rows row parent_of in_scratch home_name
dialog() { rpc debug.dialog "$1"; }
active() { state | jq -r .active_source; }

V=$HOME/workspaces/globex
BK=$V/backend APP=$V/app WEB=$V/web DEMO=$HOME/code/demo
for r in "$BK" "$APP" "$WEB" "$DEMO"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && echo base >"$r/README.md" && G -C "$r" add -A && G -C "$r" commit -qm init
done
G -C "$BK" checkout -qb beacon-dev && echo dev >"$BK/dev.txt" && G -C "$BK" add -A && G -C "$BK" commit -qm "dev only"
G -C "$BK" checkout -q main && G -C "$BK" checkout -qb beacon-prod && echo prod >"$BK/prod.txt" && G -C "$BK" add -A && G -C "$BK" commit -qm "prod only"
G -C "$BK" checkout -q main
G -C "$APP" branch beacon
mkdir -p "$V/.rade"
cat >"$V/.rade/workspace.toml" <<EOF
name = "Globex"
[agents.fake]
command = "$T/bin/agent-a.sh"
[scm]
draft_command = "ws-draft"
EOF

start_app
rade source add "$DEMO" >/dev/null
rade workspace add "$V" >/dev/null
for r in "$BK" "$APP" "$WEB"; do rade source add "$r" >/dev/null; done
rade workspace switch globex >/dev/null
check "Sources in globex" test "$(rade source list --workspace globex --json | jq -r '[.sources[].source] | join(",")')" = backend,app,web,general@globex

echo "== R31-a/b/c the M15 aliases are gone"
hint=$(rade project add ./x 2>&1 >/dev/null; echo "rc=$?")
check "rade project add ./x → USAGE with the hint" test "$hint" = "rade: 'rade project add <path>' is now 'rade source add <path>'
rc=2"
check "an existing folder too" test "$(code rade project add "$WEB")" = 2
check "… same hint" grep -qF "is now 'rade source add <path>'" "$T/err"
for args in "spawn --project backend" "list --project backend" "tab list --project backend" "review list --project backend" "fanout --agents fake,fake --project backend go"; do
    # shellcheck disable=SC2086
    check "--project rejected: rade $args" test "$(code rade $args)" = 2
done
check "the socket no longer maps project.* to source.*" test "$(rpc project.rename '{"source":"web","name":"x"}' | jq -r .data.kind)" = USAGE
check "results carry no project/projects alias keys" test "$( (rade source list --json; rade list --json; rade tab list --json) | jq -s '[.[] | .. | objects | keys[] | select(. == "project" or . == "projects")] | length')" = 0
pe=$(spawn --source web --name envcheck)
rade send "$pe" 'echo "OLD=[$RADE_PROJECT] SRC=$RADE_SOURCE"' --enter
check "no RADE_PROJECT in a Pane" eventually 5 screen_has "$pe" "OLD=[] SRC=web"
rade close "$pe" --force >/dev/null

echo "== R28-a project add writes the file; again: exit 0, byte-identical (v2: one base per link, R97)"
PF=$V/.rade/projects/beacon/project.toml
check "rade project add prints the file" test "$(rade project add BEACON --source backend:beacon-dev --source app:beacon)" = "$PF"
sum=$(sha256sum "$PF")
check "again: exit 0" test "$(code rade project add BEACON --source backend:x)" = 0
check "… existing: true" test "$(rade project add BEACON --json | jq -r .existing)" = true
check "… file byte-identical" test "$(sha256sum "$PF")" = "$sum"
check "file: name, one base per source, no bases" bash -c "grep -qx 'name = \"BEACON\"' '$PF' && grep -qx 'base = \"beacon-dev\"' '$PF' && grep -qx 'base = \"beacon\"' '$PF' && ! grep -q '^bases' '$PF'"
check "an unknown Source → USAGE" test "$(code rade project add Other --source nope)" != 0
check "the Named Base form → USAGE" test "$(code rade project add Other --source backend:dev=beacon-dev)" = 2
check "project list" test "$(rade project list --json | jq -c '[.projects[] | [.project, .name, ([.links[] | "\(.source)@\(.base)"] | join("+"))]]')" = '[["beacon","BEACON","backend@beacon-dev+app@beacon"]]'
check "project show: every link's base and checkout" eventually 5 is "rade project show beacon --json | jq -c '[.links[] | \"\(.source)=\(.resolved_base):\(.checkout_kind)\"]'" '["backend=beacon-dev:project","app=beacon:project"]'
rade project show beacon >"$T/show.txt"
check "project show prints one line per link" bash -c "grep -qE '^ *backend  beacon-dev  project ' '$T/show.txt' && grep -qE '^ *app  beacon  project ' '$T/show.txt'"

echo "== R28 edge cases: a v1 file migrated (no branch for the first Named Base); one branch"
mkdir -p "$V/.rade/projects/edge"
printf 'bases = ["dev", "prod"]\n[[source]]\nid = "backend"\nbase = { prod = "beacon-prod" }\n' >"$V/.rade/projects/edge/project.toml"
check "show: backend has no base (the main branch) and 1 warning naming backend and dev" \
    bash -c "j=\$(rade project show edge --json); [[ \$(jq -r '.links[0].base' <<<\"\$j\") == null && \$(jq '.warnings | length' <<<\"\$j\") == 1 ]] && jq -r '.warnings[0]' <<<\"\$j\" | grep -q backend && jq -r '.warnings[0]' <<<\"\$j\" | grep -qw dev"
check "… the v1 file kept as project.toml.v1.bak" test -f "$V/.rade/projects/edge/project.toml.v1.bak"
e1=$(rade worktree add backend e1 --project edge)
check "… worktree add starts at the main branch" test "$(G -C "$e1" rev-parse HEAD)" = "$(G -C "$BK" rev-parse main)"
rade worktree remove backend e1 --force >/dev/null
printf '[[source]]\nid = "app"\nbase = "beacon"\n' >"$V/.rade/projects/edge/project.toml"
e2=$(rade worktree add app e2 --project edge)
check 'base = "beacon" starts at beacon' test "$(G -C "$e2" rev-parse HEAD)" = "$(G -C "$APP" rev-parse beacon)"
rade worktree remove app e2 --force >/dev/null
rm -r "$V/.rade/projects/edge"

echo "== R28-b / R99.1 each link's base per Source; --base overrides"
p1=$(rade worktree add backend p1 --project beacon --base beacon-prod)
check "--base beacon-prod starts at beacon-prod" test "$(G -C "$p1" rev-parse HEAD)" = "$(G -C "$BK" rev-parse beacon-prod)"
d1=$(rade worktree add backend d1 --project beacon)
check "the link's base (beacon-dev) starts d1" test "$(G -C "$d1" rev-parse HEAD)" = "$(G -C "$BK" rev-parse beacon-dev)"
check "worktree list records project and base" test "$(rade worktree list backend --json | jq -c '[.worktrees[] | select(.worktree != "main") | [.worktree, .project, .base]]')" = '[["p1","beacon","beacon-prod"],["d1","beacon","beacon-dev"]]'
a1=$(rade worktree add app a1 --project beacon)
check "app's base is beacon" test "$(G -C "$a1" rev-parse HEAD)" = "$(G -C "$APP" rev-parse beacon)"
check "a Source not in the Project → USAGE" test "$(code rade worktree add web x1 --project beacon)" = 2
check "… says so" grep -q "not in project 'beacon'" "$T/err"
check "--base main overrides the link's base" test "$(G -C "$(rade worktree add backend r1 --project beacon --base main)" rev-parse HEAD)" = "$(G -C "$BK" rev-parse main)"
rade worktree remove backend r1 --force >/dev/null

echo "== R28-c Review compares with the Worktree's base"
echo mine >"$d1/mine.txt"
rv=$(rade review open --source backend --worktree d1)
check "Review vs beacon-dev" eventually 10 is "rade debug review $rv info --json | jq -r .branch" beacon-dev
check "… only its own change (no dev.txt)" eventually 10 is "rade debug review $rv info --json | jq -r '[.files[].path] | join(\",\")'" mine.txt
check "title says vs beacon-dev" bash -c "rade debug review $rv info --json | jq -r .title | grep -q 'beacon-dev'"
rade close "$rv" >/dev/null

echo "== R30-a sidebar grouped by Project"
mkdir -p "$V/.rade/projects/otp"
printf 'name = "OTP"\n[[source]]\nid = "backend"\n' >"$V/.rade/projects/otp/project.toml"
# 1.0.0 (R104): Project → link (name + base) → its Worktrees; Scratch holds
# the Workspace root and the Source no Project links (web).
EXPECT="project:BEACON@0 link:backend@1 worktree:p1@2 worktree:d1@2 link:app@1 worktree:a1@2 project:OTP@0 link:backend@1 «Scratch» scratch:~/workspaces/globex@0 scratch:web@0"
check "rows: Projects with their links and Worktrees, then Scratch" eventually 5 is rows "$EXPECT"
check "no Source header rows, no 'also in' notes" test "$(ui sidebar | jq '[.sidebar[] | select(.kind=="source" and .id!="general@globex")] | length')" = 0
echo "== R30.2 / R104-i Alt+n follows the Projects' order (BEACON, OTP) → Project home"
rade debug action source_2 >/dev/null
check "Alt+2 = OTP's home" eventually 3 is home_name OTP
rade debug action source_1 >/dev/null
check "Alt+1 = BEACON's home, its row selected" eventually 3 bash -c "[[ \$(rpc debug.ui '{\"op\":\"home\"}' | jq -r .home.name) == BEACON && \$(rpc debug.ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.selected) | .title') == BEACON ]]"
rade focus "$(rade list --json | jq -r '[.panes[] | select(.source=="backend")][0].pane // empty')" >/dev/null 2>&1 || true

echo "== R30-d a file edit shows within 3 s"
printf '[[source]]\nid = "app"\n' >>"$V/.rade/projects/otp/project.toml"
t0=$(date +%s%N)
check "app is OTP's: a1 offers Move to Project ▸ OTP" eventually 4 bash -c "rade debug menu worktree app/a1 --json | jq -r '.items[].label' | grep -qx 'Move to Project ▸ OTP'"
t1=$(date +%s%N)
check "… within 3 s ($(((t1 - t0) / 1000000)) ms)" test $(((t1 - t0) / 1000000)) -lt 3000

echo "== R28-d a broken project.toml is reported; the rest works"
mkdir -p "$V/.rade/projects/broken"
printf 'name = \n' >"$V/.rade/projects/broken/project.toml"
check "project.list: error with the file path" test "$(rade project list --json | jq -r '.projects[] | select(.project=="broken") | .error' | grep -c "^$V/.rade/projects/broken/project.toml: line 1")" = 1
check "… the others still listed" test "$(rade project list --json | jq -r '[.projects[] | select(.error == null) | .project] | join(",")')" = beacon,otp
check "Navigator: the broken Project after the others, marked broken, the rest unchanged" eventually 4 is "ui sidebar | jq -r '[.sidebar[] | select(.kind==\"project\") | \"\(.title)\(if .meta == \"broken\" then \" (broken)\" else \"\" end)\"] | join(\",\")'" "BEACON,OTP,broken (broken)"
check "… its error in the row's tooltip" test "$(ui sidebar | jq -r '.sidebar[] | select(.kind=="project" and .meta=="broken") | .tooltip' | grep -c 'line 1')" = 1
check "spawning still works" test "$(spawn --source backend --worktree d1 --cmd 'echo hi' | head -c2)" = p_
rm -r "$V/.rade/projects/broken"

echo "== R30.3 Project menu"
check "items" test "$(rade debug menu project beacon --json | jq -r '[.items[].label] | join("|")')" = "Project settings…|Add source…|New Worktree…|Open project.toml|Customize Project…"
rade debug menu project beacon activate "New Worktree…" >/dev/null
check "dialog: New Worktree in BEACON" eventually 3 is "dialog '{}' | jq -r .heading" "New Worktree in BEACON"
check "… a Source dropdown, no Base dropdown (one base per link, R97)" test "$(dialog '{}' | jq -c '[.fields.source, .fields.base]')" = '["backend",null]'
check "… Create disabled without a name" test "$(dialog '{}' | jq -c .enabled)" = '["cancel"]'
dialog '{"op":"set","field":"name","value":"p2"}' >/dev/null
check "… hint names the link's base" test "$(dialog '{}' | jq -r .fields.hint)" = "Starts from beacon-dev."
dialog '{"op":"set","field":"source","value":"app"}' >/dev/null
check "… per Source" test "$(dialog '{}' | jq -r .fields.hint)" = "Starts from beacon."
dialog '{"op":"set","field":"source","value":"backend"}' >/dev/null
dialog '{"op":"respond","response":"create"}' >/dev/null
check "Create → Worktree p2 from beacon-dev, recorded" eventually 10 is "rade worktree list backend --json | jq -c '.worktrees[] | select(.worktree==\"p2\") | [.project, .base]'" '["beacon","beacon-dev"]'
check "… at beacon-dev" test "$(G -C "$V/.worktrees-dev/backend/p2" rev-parse HEAD)" = "$(G -C "$BK" rev-parse beacon-dev)"
rade debug menu project beacon activate "Open project.toml" >/dev/null
check "Open project.toml: a Viewer of it in General" eventually 5 is "state | jq -r '[.. | objects | select(.type? == \"viewer\") | .path] | map(select(. == \"$PF\")) | length'" 1
check "… General shown" test "$(active)" = general@globex
rade debug menu project beacon activate "Customize Project…" >/dev/null
check "Customize: an Agent customize-beacon in General" eventually 5 is "rade list --json | jq -r '.panes[] | select(.name==\"customize-beacon\") | .source'" general@globex
cz=$(rade list --json | jq -r '.panes[] | select(.name=="customize-beacon") | .pane')
check "… prompted with the file and the rade-config skill" eventually 5 screen_has "$cz" "rade-config skill"
rade close "$cz" --force >/dev/null

echo "== R29-a explain: project beats workspace beats user"
printf '[scm]\ndraft_command = "proj-draft"\n' >>"$PF"
J=$(rade config explain scm.draft_command --source backend --worktree d1 --json)
check "value and layer" test "$(jq -r '[.value, .from] | join(" ")' <<<"$J")" = "proj-draft project"
check "every layer's value" test "$(jq -c '[.layers[] | [.layer, .value]] | .[1:]' <<<"$J")" = '[["user","user-draft"],["workspace","ws-draft"],["project","proj-draft"]]'
BUILTIN=$(jq -r '.layers[0].value' <<<"$J")
f1=$XDG_CONFIG_HOME/rade/config.toml f2="~/workspaces/globex/.rade/workspace.toml" f3="~/workspaces/globex/.rade/projects/beacon/project.toml"
w=$(printf '%s\n' "$f1" "$f2" "$f3" | awk '{ if (length > m) m = length } END { print m }')
{
    echo 'scm.draft_command = "proj-draft"'
    printf "  ✓ %-10s %-${w}s   %s\n" project "$f3" '"proj-draft"'
    printf "    %-10s %-${w}s   %s\n" workspace "$f2" '"ws-draft"' user "$f1" '"user-draft"' built-in "" "$(jq -n --arg b "$BUILTIN" '$b')"
    echo "context: workspace globex · project beacon (worktree d1) · source backend"
} >"$T/explain.want"
rade config explain scm.draft_command --source backend --worktree d1 >"$T/explain.got"
check "explain text exactly" diff -u "$T/explain.want" "$T/explain.got"
check "backend without a Worktree: in two Projects → no Project layer" test "$(rade config explain scm.draft_command --source backend --json | jq -r .from)" = workspace
check "--project beacon" test "$(rade config explain scm.draft_command --project beacon --json | jq -r '[.value, .context.project] | join(" ")')" = "proj-draft beacon"
check "from a Pane in d1 (caller)" test "$(env RADE_PANE_ID="$(spawn --source backend --worktree d1)" rade config explain scm.draft_command --json | jq -r .from)" = project

echo "== R29-b an Agent command in A's workspace.toml applies in A only"
pa=$(spawn --source web --agent fake)
pd=$(spawn --source demo --agent fake)
check "a spawn in globex runs A's command" eventually 5 screen_has "$pa" "agent-a in globex"
check "… still the fake Agent" eventually 5 screen_has "$pa" "fake-agent session"
check "a spawn in default does not" eventually 5 screen_has "$pd" "fake-agent session"
check "… no agent-a line" bash -c "! rade read $pd --scrollback | grep -q agent-a"

echo "== R29-c an untrusted .rade.toml's setup does not run"
printf 'setup = "touch %s/setup-marker"\n' "$T" >"$WEB/.rade.toml"
check "explain: untrusted, not applied" test "$(rade config explain sources.web.setup --source web --json | jq -c '[.value, (.layers[] | select(.layer=="source") | [.untrusted, .value])]')" = "[null,[true,\"touch $T/setup-marker\"]]"
check "explain text says untrusted" bash -c "rade config explain sources.web.setup --source web | grep -q 'untrusted: rade config trust web'"
rade worktree add web w1 >/dev/null
s1=$(spawn --source web --worktree w1)
check "the first Terminal says untrusted" eventually 5 screen_has "$s1" "untrusted: rade config trust web"
sleep 1
check "… and setup did not run" test ! -e "$T/setup-marker"
check "trust" test "$(rade config trust web)" = "trusted $WEB/.rade.toml"
check "explain: trusted, applied" test "$(rade config explain sources.web.setup --source web --json | jq -r '[.from, .context.trusted] | join(" ")')" = "source true"
rade worktree add web w2 >/dev/null
spawn --source web --worktree w2 >/dev/null
check "after trust: setup runs" eventually 5 test -e "$T/setup-marker"
echo '# changed' >>"$WEB/.rade.toml"
check "an edit untrusts it again" test "$(rade config explain sources.web.setup --source web --json | jq -r '.context.trusted')" = false
check "trust survives in state.json (content)" test "$(jq -r '.trusted_configs.web' "$STATE" | head -c5)" = setup

echo "== R29.4 a .rade.toml that never ends (a FIFO; /dev/zero likewise) is an error, not a hang"
rm "$WEB/.rade.toml"
mkfifo "$T/fifo"
ln -s "$T/fifo" "$WEB/.rade.toml"
check "explain answers with the error" test "$(timeout 5 rade config explain --source web --json | jq -r '.errors[0]' | grep -c 'not a regular file')" = 1
check "trust refuses it" test "$(code timeout 5 rade config trust web)" = 2
rm "$WEB/.rade.toml"

echo "== R29-d app-wide keys below the user file are ignored"
printf '[keybindings]\nnew_tab = "<Alt>t"\n' >>"$V/.rade/workspace.toml"
check "explain (no key) lists it as ignored (app-wide)" bash -c "rade config explain --workspace globex | grep -q '^keybindings.new_tab  ignored (app-wide)  workspace ~/workspaces/globex/.rade/workspace.toml'"
check "… JSON" test "$(rade config explain --workspace globex --json | jq -r '.ignored[0] | [.layer, .key] | join(" ")')" = "workspace keybindings.new_tab"
check "explain of the key: ignored" test "$(rade config explain keybindings.new_tab --workspace globex --json | jq -r '[.from, (.layers[] | select(.layer=="workspace") | .ignored)] | join(" ")')" = " app-wide"

echo "== R30-c a waiting Agent in a folded Project shows on its header"
ag=$(spawn --source backend --worktree d1 --agent fake --name pa)
nav_toggle() { rpc debug.ui "{\"op\":\"nav_toggle\",\"kind\":\"project\",\"title\":\"$1\"}" >/dev/null; }
nav_toggle BEACON
check "fold BEACON" eventually 3 is "row project BEACON | jq -r .collapsed" true
check "folded: its rows hidden" test "$(rows | cut -d' ' -f1-3)" = "project:BEACON@0 project:OTP@0 link:backend@1"
rade status waiting --pane "$ag" >/dev/null
t0=$(date +%s%N)
check "header shows the waiting LED (R104-d)" eventually 2 is "row project BEACON | jq -r .led" waiting
t1=$(date +%s%N)
check "… within 1 s ($(((t1 - t0) / 1000000)) ms)" test $(((t1 - t0) / 1000000)) -lt 1000
check "… and its waiting badge" eventually 3 is "row project BEACON | jq -r .badge" 1
check "unfolded headers show no LED" test "$(row project OTP | jq -r .led)" = null
check "OTP's backend link doesn't carry d1's (BEACON's) Agent" test "$(ui sidebar | jq -r '[.sidebar[] | select(.kind=="project" and .title=="OTP")] | length')/$(ui sidebar | jq -r '.sidebar as $r | ($r | map(.title=="OTP") | index(true)) as $i | $r[$i+1] | "\(.title) \(.led) \(.badge)"')" = "1/backend null 0"

echo "== R30-e screenshots light + dark"
nav_toggle BEACON
rade debug action source_1 >/dev/null
sleep 0.5
shot projects-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot projects-dark
nav_toggle BEACON
sleep 0.3
shot projects-folded-dark
rade debug action color_scheme_light >/dev/null
nav_toggle BEACON

echo "== R30-b folding survives a restart; R29.2 Session Restore uses the Worktree's Project"
# No hooks: no session or recorded invocation, so the Preset's command is what restarts.
printf '#!/usr/bin/env bash\nn=$(( $(cat %s/runs 2>/dev/null || echo 0) + 1 )); echo $n >%s/runs\necho "agent-p run $n"\nexec "%s" "$@"\n' "$T" "$T" "$FAKE" >"$T/bin/agent-p.sh"
chmod +x "$T/bin/agent-p.sh"
printf '[agents.fake]\ncommand = "FAKE_NO_HOOKS=1 %s"\n' "$T/bin/agent-p.sh" >>"$PF"
ar=$(spawn --source backend --worktree d1 --agent fake --name restored)
check "an Agent in d1 runs BEACON's command" eventually 5 screen_has "$ar" "agent-p run 1"
nav_toggle OTP
stop_app
check "state.json keeps it (collapsed_projects)" test "$(jq -r '.collapsed_projects | index("globex/otp") != null' "$STATE")" = true
start_app
check "after restart: OTP folded by the owner, BEACON open" eventually 5 is "ui sidebar | jq -r '[.sidebar[] | select(.kind==\"project\") | \"\(.title)=\(.collapsed)\"] | join(\",\")'" "BEACON=false,OTP=true"
check "… restored with it too (backend is in BEACON and OTP: its Worktree's Project counts)" eventually 8 screen_has "$ar" "agent-p run 2"
check "worktree project/base survive" test "$(rade worktree list backend --json | jq -r '.worktrees[] | select(.worktree=="d1") | .base')" = beacon-dev

echo "== R31-d old names in config.toml give an error naming the new ones"
printf '[projects.web]\nsetup = "x"\n' >>"$CONF"
check "[projects.*] → names [sources.*]" eventually 5 grep -qF '[projects.*] is now [sources.*]' "$OUT/app.log"
user_config
printf '[keybindings]\nproject_1 = "<Alt>F1"\n' >>"$CONF"
check "project_1 → names source_1" eventually 5 grep -qF 'project_1 (now source_1)' "$OUT/app.log"
user_config

echo "== R30-f (M26) Projects over the same Sources: only recorded Worktrees; branches only suggest"
# The owner's shape: Projects beacon (dev/prod) and globex-web-main (main = master)
# both over globex-web + globex-api; main checkouts on an unrelated
# branch; Worktrees with no recorded Project. The v1 files are migrated on
# read (R97.2).
O=$HOME/workspaces/owner
OA=$O/globex-web OB=$O/globex-api
for r in "$OA" "$OB"; do
    mkdir -p "$r" && G -C "$r" init -q -b master && echo base >"$r/README.md" && G -C "$r" add -A && G -C "$r" commit -qm init
    G -C "$r" branch feat/dev/beacon && G -C "$r" branch feat/beacon && G -C "$r" checkout -qb feat/santi-ai
done
rade workspace add "$O" >/dev/null
for r in "$OA" "$OB"; do rade source add "$r" --workspace owner >/dev/null; done
rade workspace switch owner >/dev/null
chat=$(rade worktree add globex-web agent-chat)
G -C "$chat" checkout -qb feat/beacon-agent-chat
devw=$(rade worktree add globex-web dev-wt)
G -C "$devw" checkout -q feat/dev/beacon
fix=$(rade worktree add globex-web hotfix)
G -C "$fix" checkout -q master
# 1.0 (R99.1): a Source in several Projects needs --project, so the
# Worktrees with no recorded Project are made before the Projects exist.
mkdir -p "$O/.rade/projects/beacon" "$O/.rade/projects/globex-web-main"
printf 'bases = ["dev", "prod"]\n[[source]]\nid = "globex-web"\nbase = { dev = "feat/dev/beacon", prod = "feat/beacon" }\n[[source]]\nid = "globex-api"\nbase = { dev = "feat/dev/beacon", prod = "feat/beacon" }\n' >"$O/.rade/projects/beacon/project.toml"
printf 'bases = ["main"]\n[[source]]\nid = "globex-web"\nbase = { main = "master" }\n[[source]]\nid = "globex-api"\nbase = { main = "master" }\n' >"$O/.rade/projects/globex-web-main/project.toml"
mine=$(rade worktree add globex-web made-for-beacon --project beacon)
owner_split() { [[ $(parent_of worktree made-for-beacon) == project:beacon ]] && in_scratch agent-chat && in_scratch dev-wt && in_scratch hotfix; }
check "beacon: only its own Worktree; the rest under Scratch, whatever their branch" eventually 8 owner_split
SHARED=$(rows)
check "… with suggestions by branch" test "$(ui sidebar | jq -r '[.sidebar[] | select(.kind=="worktree" and .suggest) | "\(.title)\(.suggest)"] | join(",")')" = "agent-chat→ beacon?,dev-wt→ beacon?,hotfix→ globex-web-main?"
echo "== R30.4 Move to Project ▸ (persisted)"
check "Worktree menu lists both Projects and No Project" test "$(rade debug menu worktree globex-web/agent-chat --json | jq -r '[.items[].label | select(startswith("Move to Project"))] | join("|")')" = "Move to Project ▸ beacon|Move to Project ▸ globex-web-main|Move to Project ▸ No Project ✓"
rade debug menu worktree globex-web/agent-chat activate "Move to Project ▸ beacon" >/dev/null
check "agent-chat now under beacon" eventually 5 is "parent_of worktree agent-chat" project:beacon
check "…recorded (worktree list)" test "$(rade worktree list globex-web --json | jq -r '.worktrees[] | select(.worktree=="agent-chat") | .project')" = beacon
check "…saved in state.json" eventually 5 is "jq -r '[.sources[].worktrees[] | select(.name==\"agent-chat\") | .project] | .[0]' '$STATE'" beacon
check "…menu ticks beacon" test "$(rade debug menu worktree globex-web/agent-chat --json | jq -r '.items[].label | select(endswith("✓"))')" = "Move to Project ▸ beacon ✓"
rade debug action source_1 >/dev/null
check "Alt+1: beacon's home, its row selected (R104-i)" eventually 3 is "ui sidebar | jq -r '[.sidebar[] | select(.selected) | \"\\(.kind):\\(.title)\"] | join(\",\")'" "project:beacon"
sleep 0.5
shot shared-sources-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot shared-sources-dark
rade debug action color_scheme_light >/dev/null
rade debug menu worktree globex-web/agent-chat activate "Move to Project ▸ No Project" >/dev/null
check "No Project → back under Scratch" eventually 5 is rows "$SHARED"
G -C "$devw" checkout -q -b tmp-x
check "a branch switch moves no row (only its suggestion goes)" eventually 8 bash -c "in_scratch dev-wt && [[ \$(ui sidebar | jq -r '.sidebar[] | select(.kind==\"worktree\" and .title==\"dev-wt\") | .suggest') == null ]]"
check "… rows unchanged" test "$(rows)" = "$SHARED"
rade workspace switch globex >/dev/null

echo "== regression: a Workspace without Projects looks as before"
rade workspace switch default >/dev/null
check "no Project rows: 'No Projects yet', then Scratch with the root and demo" eventually 8 is rows "empty:No Projects yet@0 «Scratch» scratch:~@0 scratch:demo@0"

echo "== R32-a rade-config skill installed by rade setup"
rade setup --agent claude >/dev/null </dev/null
SK=$HOME/.claude/skills/rade-config/SKILL.md
check "installed" test -f "$SK"
check "frontmatter" test "$(head -2 "$SK" | tail -1)" = "name: rade-config"
check "describes project.toml, .rade.toml, explain, show" bash -c "grep -q 'project.toml' '$SK' && grep -q '.rade.toml' '$SK' && grep -q 'rade config explain' '$SK' && grep -q 'rade project show' '$SK'"
check "state.json only in a Never line" bash -c "grep 'state.json' '$SK' | grep -qv Never && exit 1 || exit 0"
check "every command it shows is rade (or a .rade file)" bash -c "awk '/^\`\`\`sh/{f=1;next} /^\`\`\`/{f=0} f && NF && \$1 != \"rade\" && \$1 !~ /^#/' '$SK' | wc -l | grep -qx 0"

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
