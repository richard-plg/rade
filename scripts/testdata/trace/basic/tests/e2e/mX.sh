#!/usr/bin/env bash
check "R97-b e2e table shows the links" true
# check "R97-c in a comment" true
echo "R97-c is mentioned in a string, not a check name"
check 'R97-cd a similar id' true
check "first name" grep -q "R97-c" /dev/null   # R97-c only in a later argument
check 'R97-e single-quoted name' true
true # check "R97-c after an inline comment"
