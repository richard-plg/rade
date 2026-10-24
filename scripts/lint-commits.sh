#!/usr/bin/env bash
# Check commit subjects against the convention (ADR-0012, CLAUDE.md):
#   <type>[(<scope>)][!]: <summary>     scope = R-IDs or a milestone, e.g.
#   feat(R18): Browser Pane …   fix(R13.2,R11.2): …   docs(M12): checks …
# Usage: scripts/lint-commits.sh <git-log revs>  (e.g. origin/main..HEAD, or <sha> -1)
set -euo pipefail
re='^(feat|fix|perf|refactor|docs|test|build|ci|chore|style|revert)(\([^()]+\))?!?: [^ ].*'
bad=0
while IFS=$'\t' read -r sha subject; do
    [[ $subject =~ $re || $subject == Revert\ * ]] && continue
    echo "::error::$sha does not follow <type>(<R-IDs>): <summary> — \"$subject\""
    bad=1
done < <(git log --no-merges --format='%h%x09%s' "${@:?usage: lint-commits.sh <git-log revs>}")
exit $bad
