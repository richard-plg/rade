#!/usr/bin/env bash
# Guard for PRD R101.5 (ADR-0033): the engines removed in 1.0.0 stay gone.
#
# Scans crates/ (Rust, assets, CSS), skills/ and the CLI help (`rade --help`
# and every subcommand's, recursively) for the removed names:
#   Task(s), Batch(es), Initiative(s), Report(s), Digest(s), plan_gate,
#   `rade task|batch|initiative|digest|report|flow|lessons`, Event Journal,
#   Start here, RADE_TASK, RADE_INITIATIVE, Verified Run, Agent Profile
# and, in CSS, class names `rade-(task|batch|initiative|report|digest|
# start-here|journal|verify|plan)…`. Words are matched case-sensitively, so
# lowercase English ("the task every Agent gets", a Markdown task list)
# is fine. "Named Base" is allowed until M36 (decision log, M34).
#
# Exceptions (written here on purpose; add one only with a reason):
#   Not scanned at all: CHANGELOG.md, docs/adr/ before ADR-0033,
#     docs/checks/ before M34, docs/PRD.md / docs/API.md (they
#     record the removal), tests/.
#   Scanned files allowed to name the removed things:
EXCEPTIONS=(
    "crates/rade-core/src/rpc.rs"              # the removed-method contract (R101.2)
    "crates/rade-cli/src/main.rs"              # the removed-command check (R101.2)
    "crates/rade-core/src/store.rs"            # the v2 → v3 migration drops the old keys (R101.3)
    "crates/rade-core/tests/fixtures/state-0.7.1.json" # a 0.7.1 state.json (R101-c, R101-g)
    "crates/rade-app/assets/mermaid.min.js"    # vendored Mermaid (its own "Task" strings)
)
#
# Usage: scripts/check-removed.sh [--root DIR] [--bin PATH] [--no-help]
#   --root DIR  tree to scan (default: this script's repository)
#   --bin PATH  the `rade` binary whose help is scanned (default:
#               <root>/target/release/rade)
#   --no-help   skip the CLI help (e.g. no build at hand)
# Exit: 0 clean · 1 a removed name found (each printed as file:line: text)
#       · 2 usage error or no binary.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$here/.. bin= help=1
while (($#)); do
    case $1 in
        --root) root=${2:?--root needs a value}; shift 2 ;;
        --bin) bin=${2:?--bin needs a value}; shift 2 ;;
        --no-help) help=0; shift ;;
        -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 0 ;;
        *) echo "check-removed.sh: unknown option '$1'" >&2; exit 2 ;;
    esac
done
root=$(cd "$root" && pwd)
bin=${bin:-$root/target/release/rade}
WORDS='\b(Tasks?|Batch(es)?|Initiatives?|Reports?|Digests?)\b|plan_gate|rade (task|batch|initiative|digest|report|flow|lessons)\b|Event Journal|Start here|RADE_TASK|RADE_INITIATIVE|Verified Run|Agent Profile'
CSS='rade-(task|batch|initiative|report|digest|start-here|journal|verify|plan)'
found=0
excluded() { local e; for e in "${EXCEPTIONS[@]}"; do [[ $1 == "$e" ]] && return 0; done; return 1; }
while IFS= read -r -d '' f; do
    rel=${f#"$root"/}
    excluded "$rel" && continue
    pat=$WORDS
    [[ $f == *.css ]] && pat="$WORDS|$CSS"
    while IFS= read -r hit; do
        echo "$rel:$hit"
        found=1
    done < <(grep -nE -- "$pat" "$f" || true)
done < <(find "$root/crates" "$root/skills" -name target -type d -prune -o -type f -print0 2>/dev/null)
if ((help)); then
    [[ -x $bin ]] || { echo "check-removed.sh: no rade binary at $bin (build it, pass --bin, or --no-help)" >&2; exit 2; }
    # Every subcommand's help, breadth first: "rade", "rade project", …
    queue=("") seen=0
    while ((${#queue[@]})); do
        cmd=${queue[0]}; queue=("${queue[@]:1}")
        read -ra words <<<"$cmd"
        out=$("$bin" "${words[@]}" --help 2>&1) || true
        seen=$((seen + 1))
        while IFS= read -r hit; do
            echo "rade${cmd:+ $cmd} --help:$hit"
            found=1
        done < <(grep -nE -- "$WORDS" <<<"$out" || true)
        while IFS= read -r sub; do
            [[ -n $sub && $sub != help ]] && queue+=("${cmd:+$cmd }$sub")
        done < <(awk '/^Commands:/{c=1; next} c && /^[^ ]/{c=0} c && /^  [a-z]/{print $1}' <<<"$out")
    done
fi
if ((found)); then
    echo "check-removed.sh: removed names found (R101.5); remove them or add a reasoned exception" >&2
    exit 1
fi
if ((help)); then
    echo "check-removed.sh: clean (crates/, skills/, $seen CLI help pages)"
else
    echo "check-removed.sh: clean (crates/, skills/; CLI help skipped)"
fi
