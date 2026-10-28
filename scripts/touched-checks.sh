#!/usr/bin/env bash
# Prints the cargo arguments of the ordinary (not #[ignore]d) tests that a worktree's changes can
# reach. The list comes from the diff against the merge-base with BASE, not from a hand-picked set:
#   -p <crate> --lib / --bins   for each crate of native/ with a changed or new file
#   -p <crate> --test <target>  for each integration target of such a crate whose file changed,
#                               or that declares `mod support;` when a file of tests/support changed
# The crate's other integration targets are printed as `# not run: <crate> <names>`, so the part
# the list leaves out is on the screen and in the log, not silent.
#
# usage: scripts/touched-checks.sh <worktree> <base rev>      (one set of arguments per line, sorted)
# It reads git and the tree only: no cargo, no process, no port, nothing written.
#
# How this can break, and what stops it:
#   first run    none.
#   contention   none.
#   start        the worktree is a git worktree and BASE resolves; otherwise exit 2.
#   waiting      none.
# Undo: nothing to undo.
set -u
WT=${1:?usage: scripts/touched-checks.sh <worktree> <base rev>}
BASE=${2:?base rev}
cd "$WT" || exit 2
MB=$(git merge-base "$BASE" HEAD) || exit 2
paths=$({ git diff --name-only "$MB" -- native; git ls-files --others --exclude-standard -- native; } | sort -u)
crates=$(printf '%s\n' "$paths" | awk -F/ 'NF >= 3 { print $2 }' | sort -u)
for crate in $crates; do
  dir=native/$crate
  [ -f "$dir/Cargo.toml" ] || continue
  [ -f "$dir/src/lib.rs" ] && echo "-p $crate --lib"
  if [ -f "$dir/src/main.rs" ] || [ -d "$dir/src/bin" ]; then echo "-p $crate --bins"; fi
  support_changed=$(printf '%s\n' "$paths" | grep -c "^$dir/tests/support/")
  left=""
  for file in "$dir"/tests/*.rs; do
    [ -f "$file" ] || continue
    target=$(basename "$file" .rs)
    if printf '%s\n' "$paths" | grep -qxF "$dir/tests/$target.rs" \
      || { [ "$support_changed" -gt 0 ] && grep -q '^mod support;' "$file"; }; then
      echo "-p $crate --test $target"
    else
      left="$left $target"
    fi
  done
  [ -z "$left" ] || echo "# not run: $crate$left"
done | sort -u
