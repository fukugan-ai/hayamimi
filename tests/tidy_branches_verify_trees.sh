#!/usr/bin/env bash
# Tests for the detached verification tree rule of scripts/tidy-branches.sh.
#
# Usage: tests/tidy_branches_verify_trees.sh <work dir>
#
# The work dir must not exist or must be empty. It holds throwaway repositories
# and is left in place afterwards. Exit status 0 means every case passed.
#
# A verification tree is a detached worktree on a `--no-ff` merge of a branch
# into a trunk commit. The rule under test: such a tree may go when the tip is a
# merge and every parent is an ancestor of the trunk; one parent outside the
# trunk, or a tip with a single parent, keeps it.
set -euo pipefail

work="${1:?usage: tests/tidy_branches_verify_trees.sh <work dir>}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../scripts" && pwd)"
mkdir -p "$work"
if [ -n "$(ls -A "$work")" ]; then
  echo "work dir is not empty: $work" >&2
  exit 2
fi

failures=0
cases=0

check() {
  local name="$1" ok="$2"
  cases=$((cases + 1))
  if [ "$ok" = "yes" ]; then
    echo "ok   $name"
  else
    echo "FAIL $name"
    failures=$((failures + 1))
  fi
}

# A throwaway repository with the script copied in: root, then `a` on main.
make_repo() {
  local repo="$work/$1"
  mkdir -p "$repo/scripts"
  cp "$script_dir/tidy-branches.sh" "$repo/scripts/"
  git -C "$repo" init -q -b main
  git -C "$repo" config user.name t
  git -C "$repo" config user.email t@t
  git -C "$repo" config commit.gpgsign false
  git -C "$repo" add scripts
  git -C "$repo" commit -q -m root
  printf 'a\n' >"$repo/a.txt"
  git -C "$repo" add a.txt
  git -C "$repo" commit -q -m a
}

# A branch off the root with one commit of its own (file $2), left unmerged.
make_branch() {
  local repo="$work/$1"
  git -C "$repo" switch -q -c "$2" main~1
  printf '%s\n' "$2" >"$repo/$2.txt"
  git -C "$repo" add "$2.txt"
  git -C "$repo" commit -q -m "$2"
  git -C "$repo" switch -q main
}

# A verification tree: `--no-ff` merge of branch $2 into main, detached at the
# merge, in a worktree named $3.
make_verify_tree() {
  local repo="$work/$1" tree="$work/$1/.claude/worktrees/$3"
  git -C "$repo" worktree add -q --detach "$tree" main
  git -C "$tree" merge -q --no-ff -m "verify $2" "$2"
}

# Land branch $2 in main without taking its content, so the merge in a
# verification tree never has its tree in the trunk's history.
land_without_content() {
  local repo="$work/$1"
  git -C "$repo" merge -q --no-ff -s ours -m "land $2" "$2"
}

remove_tree() {
  # $1 = repo, $2 = worktree name, $3 = optional flag
  "$work/$1/scripts/tidy-branches.sh" --remove-worktree "$work/$1/.claude/worktrees/$2" ${3:+"$3"} 2>&1 || true
}

tree_exists() {
  if [ -d "$work/$1/.claude/worktrees/$2" ]; then echo yes; else echo no; fi
}

says() {
  if printf '%s\n' "$1" | grep -Fq "$2"; then echo yes; else echo no; fi
}

# Positive: a --no-ff merge whose parents are both in the trunk is removed, and the
# content is not compared (the trunk took the branch with `-s ours`).
make_repo landed
make_branch landed feat-landed
make_verify_tree landed feat-landed vt
land_without_content landed feat-landed
out="$(remove_tree landed vt --dry-run)"
check "a merge of two trunk commits is reported removable" "$(says "$out" '→ 外す')"
check "--dry-run leaves the tree on disk" "$(tree_exists landed vt)"
out="$(remove_tree landed vt)"
check "the same tree goes without --dry-run" "$([ "$(tree_exists landed vt)" = no ] && echo yes || echo no)"

# Negative: one parent is a branch that has not landed -> kept, with the reason.
make_repo unlanded
make_branch unlanded feat-open
make_verify_tree unlanded feat-open vt
out="$(remove_tree unlanded vt)"
check "a merge with one parent outside the trunk is refused" "$(says "$out" '幹の commit だけの merge でもない')"
check "a merge with one parent outside the trunk stays on disk" "$(tree_exists unlanded vt)"

# Negative: one branch landed, another did not -> the unlanded parent keeps it.
make_repo mixed
make_branch mixed feat-one
make_branch mixed feat-two
make_verify_tree mixed feat-two vt
land_without_content mixed feat-one
out="$(remove_tree mixed vt)"
check "landing a different branch does not release the tree" "$(tree_exists mixed vt)"

# Negative: a tip with a single parent that is in the trunk has work of its own.
make_repo single
git -C "$work/single" worktree add -q --detach "$work/single/.claude/worktrees/vt" main
printf 'own\n' >"$work/single/.claude/worktrees/vt/own.txt"
git -C "$work/single/.claude/worktrees/vt" add own.txt
git -C "$work/single/.claude/worktrees/vt" commit -q -m own
out="$(remove_tree single vt)"
check "a tip with one parent and a commit of its own is refused" "$(says "$out" '幹の commit だけの merge でもない')"
check "a tip with one parent stays on disk" "$(tree_exists single vt)"

# Negative: an uncommitted file inside a releasable merge tree still keeps it.
make_repo dirty
make_branch dirty feat-dirty
make_verify_tree dirty feat-dirty vt
land_without_content dirty feat-dirty
printf 'x\n' >"$work/dirty/.claude/worktrees/vt/untracked.txt"
out="$(remove_tree dirty vt)"
check "an uncommitted file keeps a releasable merge tree" "$(says "$out" '未 commit が')"
check "a dirty merge tree stays on disk" "$(tree_exists dirty vt)"

# Control: the older rule is intact, a tip that is an ancestor of the trunk goes.
make_repo ancestor
git -C "$work/ancestor" worktree add -q --detach "$work/ancestor/.claude/worktrees/vt" main
out="$(remove_tree ancestor vt)"
check "a detached tree at the trunk tip is still removed" "$([ "$(tree_exists ancestor vt)" = no ] && echo yes || echo no)"

echo
echo "cases=$cases failures=$failures"
[ "$failures" = "0" ]
