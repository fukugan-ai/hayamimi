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
# trunk, or a tip with a single parent, keeps it. The last section covers the
# fourth way in: a tip stacked on an earlier verification merge goes when it has
# no ordinary commit the trunk lacks and merging it leaves the trunk's own tree.
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

# The fourth way in. A tip stacked on an earlier verification merge: its first
# parent is a merge commit that never reached the trunk, so the third way keeps it.

# Commit file $3 with content $4 in directory $2 of repo $1 (message = file name).
put() {
  printf '%s\n' "$4" >"$work/$1/$2/$3"
  git -C "$work/$1/$2" add "$3"
  git -C "$work/$1/$2" commit -q -m "$3"
}

# Two verification merges in one detached tree vt: feat-b, then feat-c on top.
make_stacked_tree() {
  make_repo "$1"
  make_branch "$1" feat-b
  make_branch "$1" feat-c
  make_verify_tree "$1" feat-b vt
  git -C "$work/$1/.claude/worktrees/vt" merge -q --no-ff -m "verify feat-c" feat-c
}

# yes when none of the three older rules would release the tree in $2 of repo $1:
# the tip is not an ancestor of the trunk, its tree is not in the trunk's history,
# and a parent is outside the trunk. Without this a positive case could pass by them.
older_rules_keep() {
  local repo="$work/$1" tip tree seen parent outside=no
  tip="$(git -C "$repo/.claude/worktrees/$2" rev-parse HEAD)"
  tree="$(git -C "$repo" rev-parse "$tip^{tree}")"
  seen="$(git -C "$repo" log --format=%T main | grep -Fxc "$tree" || true)"
  for parent in $(git -C "$repo" rev-list --parents -n 1 "$tip" | cut -d' ' -f2-); do
    git -C "$repo" merge-base --is-ancestor "$parent" main || outside=yes
  done
  if [ "$seen" = "0" ] && [ "$outside" = "yes" ] && ! git -C "$repo" merge-base --is-ancestor "$tip" main; then
    echo yes
  else
    echo no
  fi
}

# Positive: feat-b and feat-c both land with their content, with another commit
# between them, so no tree in the trunk's history is the tip's tree. Nothing in
# the tip is missing from the trunk, and the trunk has moved on past it.
make_stacked_tree stacked
out="$(remove_tree stacked vt --dry-run)"
check "the same tree is refused while the trunk lacks its branches" "$(says "$out" '幹の commit だけの merge でもない')"
git -C "$work/stacked" merge -q --no-ff -m "land feat-b" feat-b
put stacked . d.txt d
git -C "$work/stacked" merge -q --no-ff -m "land feat-c" feat-c
check "the older rules keep a tip stacked on an unlanded verification merge" "$(older_rules_keep stacked vt)"
out="$(remove_tree stacked vt --dry-run)"
check "a stacked merge that leaves the trunk's tree as it is is reported removable" "$(says "$out" '→ 外す')"
check "--dry-run leaves the stacked tree on disk" "$(tree_exists stacked vt)"
out="$(remove_tree stacked vt)"
check "the same stacked tree goes without --dry-run" "$([ "$(tree_exists stacked vt)" = no ] && echo yes || echo no)"
check "the removal prints how to put the stacked tree back" "$(says "$out" 'git worktree add --detach')"

# Negative: every ordinary commit is in the trunk, but the tip's merge commit
# carries a file of its own (folded in with --amend, no conflict involved), so
# merging the tip would add that file to the trunk. The tree would change.
make_stacked_tree changes
printf 'extra\n' >"$work/changes/.claude/worktrees/vt/extra.txt"
git -C "$work/changes/.claude/worktrees/vt" add extra.txt
git -C "$work/changes/.claude/worktrees/vt" commit -q --amend --no-edit
git -C "$work/changes" merge -q --no-ff -m "land feat-b" feat-b
put changes . d.txt d
git -C "$work/changes" merge -q --no-ff -m "land feat-c" feat-c
out="$(remove_tree changes vt)"
check "a tip whose merge would change the trunk is refused" "$(says "$out" '幹の commit だけの merge でもない')"
check "a tip whose merge would change the trunk stays on disk" "$(tree_exists changes vt)"

# Negative: an evil merge. The only content that is not in the trunk is a conflict
# resolution made inside the tip. Every ordinary commit is in the trunk, and the
# trunk kept its own side of the conflict.
make_repo evil
put evil . shared.txt base
git -C "$work/evil" switch -q -c feat-x
put evil . shared.txt x
git -C "$work/evil" switch -q main
git -C "$work/evil" switch -q -c feat-z main~1
put evil . z.txt z
git -C "$work/evil" switch -q main
put evil . shared.txt main
tree="$work/evil/.claude/worktrees/vt"
git -C "$work/evil" worktree add -q --detach "$tree" main
git -C "$tree" merge -q --no-ff -m "verify feat-z" feat-z
git -C "$tree" merge --no-ff -m "verify feat-x" feat-x >/dev/null 2>&1 || true
printf 'resolved\n' >"$tree/shared.txt"
git -C "$tree" add shared.txt
git -C "$tree" commit -q -m "resolve by hand"
git -C "$work/evil" merge -q --no-ff -m "land feat-z" feat-z
git -C "$work/evil" merge -q --no-ff -s ours -m "land feat-x without content" feat-x
out="$(remove_tree evil vt)"
check "a hand-resolved merge whose merge into the trunk conflicts is refused" "$(says "$out" '幹の commit だけの merge でもない')"
check "a hand-resolved merge stays on disk" "$(tree_exists evil vt)"

# Negative: the content is in the trunk, but feat-c's commit is not. It was
# cherry-picked, so the merge leaves the trunk's tree as it is while the tip still
# carries an ordinary commit the trunk lacks.
make_stacked_tree copied
git -C "$work/copied" merge -q --no-ff -m "land feat-b" feat-b
put copied . d.txt d
git -C "$work/copied" cherry-pick feat-c >/dev/null
out="$(remove_tree copied vt)"
check "a tip with an ordinary commit the trunk lacks is refused" "$(says "$out" '幹の commit だけの merge でもない')"
check "a tip with an ordinary commit the trunk lacks stays on disk" "$(tree_exists copied vt)"

echo
echo "cases=$cases failures=$failures"
[ "$failures" = "0" ]
