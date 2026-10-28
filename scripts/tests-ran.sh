#!/usr/bin/env bash
# Checks that every test a worktree adds has been run and has passed.
#
# It lists the #[test] functions the worktree adds against the merge-base with BASE (a test whose
# `fn` line or whose #[test] / #[ignore] line is in an added hunk, and every test of an untracked
# .rs file) and looks in the test logs for the pass line `test <path>::<name> ... ok` of each (a
# #[should_panic] test prints `test <path>::<name> - should panic ... ok`, which counts too).
# A test that was written and never run, that failed, or that the run left out is named, and the
# exit code is 1. Nothing is written and cargo is not started.
#
# usage: scripts/tests-ran.sh <worktree> <base rev> <test log>...
#   exit 0  every added test that is not #[ignore]d has a pass line in some log
#   exit 1  at least one has none (printed as `MISSING <name> (<file>)`)
#   exit 2  usage, a log that cannot be read, or the worktree or the merge-base cannot be read
# An #[ignore]d test is printed as `ignored ok` or `ignored not-run` and never decides the exit
# code: its pass line comes from a real run, whose log can be given the same way.
# WAITING (a regex of test names, in the environment) names the tests that the caller kept out of
# the run on purpose, for a stated reason (a reader of records that a real run has not written
# yet). They are printed as `waiting` with the regex on every line and do not decide the exit code.
#
# How this can break, and what stops it:
#   first run    none: it reads git and the logs only.
#   contention   none: no cargo, no process, no port.
#   start        the worktree is a git worktree and BASE resolves; otherwise exit 2.
#   waiting      none.
#   blind spots  a test that is not new but depends on a changed helper is not named here; the
#                list from scripts/touched-checks.sh and the cargo exit code of that run cover it.
#                A test made by a macro is not seen, and neither is a multi-line #[test] attribute.
# Undo: nothing to undo.
set -u
WT=${1:?usage: scripts/tests-ran.sh <worktree> <base rev> <test log>...}
BASE=${2:?base rev}
shift 2
[ $# -gt 0 ] || { echo "tests-ran: no test log given" >&2; exit 2; }
# The logs are made absolute and read once before the cd: a log that cannot be read is an error
# here, not a pass line that "is not found".
logs=()
for log in "$@"; do
  [ -r "$log" ] || { echo "tests-ran: cannot read $log" >&2; exit 2; }
  logs+=("$(realpath "$log")")
done
cd "$WT" || exit 2
MB=$(git merge-base "$BASE" HEAD) || exit 2

# The added line ranges of one tracked file ("from-to from-to ..."), in the working file.
ranges_of() {
  git diff -U0 "$MB" -- "$1" | awk '/^@@/ {
    split($3, part, ","); from = substr(part[1], 2) + 0; count = (part[2] == "" ? 1 : part[2] + 0)
    if (count > 0) printf "%d-%d ", from, from + count - 1
  }'
}

# "<name> <ignored 0|1>" for each test of the file with a line in the ranges.
tests_in() {
  awk -v ranges="$2" '
    BEGIN {
      n = split(ranges, r, " ")
      for (i = 1; i <= n; i++) { split(r[i], b, "-"); lo[i] = b[1] + 0; hi[i] = b[2] + 0 }
    }
    function added(line,   i) {
      for (i = 1; i <= n; i++) if (line >= lo[i] && line <= hi[i]) return 1
      return 0
    }
    /^[[:space:]]*#\[([a-z_]+::)*test(\]|\()/ { seen = 1; mark = (mark || added(NR)); next }
    /^[[:space:]]*#\[ignore/ { ign = 1; mark = (mark || added(NR)); next }
    /^[[:space:]]*(#\[|\/\/)/ { next }
    /^[[:space:]]*(pub(\([a-z]+\))? )?(async )?fn [A-Za-z0-9_]+/ {
      if (seen) {
        name = $0
        sub(/^[[:space:]]*(pub(\([a-z]+\))? )?(async )?fn /, "", name)
        sub(/[^A-Za-z0-9_].*$/, "", name)
        if (mark || added(NR)) print name, (ign ? 1 : 0)
      }
      seen = 0; ign = 0; mark = 0
      next
    }
    { seen = 0; ign = 0; mark = 0 }
  ' "$1"
}

declare -A fresh=()
while IFS= read -r -d '' file; do
  fresh["$file"]=1
done < <(git ls-files --others --exclude-standard -z -- '*.rs')
mapfile -d '' -t files < <({ git diff --name-only -z "$MB" -- '*.rs'; git ls-files --others --exclude-standard -z -- '*.rs'; } | sort -z -u)

added=0; run_ok=0; ignored=0; ignored_ok=0; missing=0; waiting=0
for file in "${files[@]}"; do
  [ -f "$file" ] || continue
  if [ -n "${fresh[$file]:-}" ]; then ranges="1-999999"; else ranges=$(ranges_of "$file"); fi
  [ -n "$ranges" ] || continue
  while read -r name is_ignored; do
    [ -n "$name" ] || continue
    added=$((added + 1))
    if grep -E -q "^test (.*::)?${name}( - should panic)? \.\.\. ok$" "${logs[@]}"; then found=ok; else found=none; fi
    if [ "$is_ignored" = 1 ]; then
      ignored=$((ignored + 1))
      if [ "$found" = ok ]; then
        ignored_ok=$((ignored_ok + 1)); echo "ignored ok       $name ($file)"
      else
        echo "ignored not-run  $name ($file)"
      fi
    elif [ "$found" = ok ]; then
      run_ok=$((run_ok + 1))
    elif [ -n "${WAITING:-}" ] && printf '%s\n' "$name" | grep -E -q "$WAITING"; then
      waiting=$((waiting + 1)); echo "waiting $name ($file) [kept out of the run by the caller: $WAITING]"
    else
      missing=$((missing + 1)); echo "MISSING $name ($file)"
    fi
  done < <(tests_in "$file" "$ranges")
done
echo "tests-ran: base=$(git rev-parse --short "$MB") added=$added run_ok=$run_ok waiting=$waiting ignored=$ignored ignored_ok=$ignored_ok missing=$missing logs=${#logs[@]}"
[ "$missing" -eq 0 ]
