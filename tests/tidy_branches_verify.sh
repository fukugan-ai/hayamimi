#!/usr/bin/env bash
# Tests for the `verify/` rule of scripts/tidy-branches.sh.
#
# Usage: tests/tidy_branches_verify.sh <work dir>
#
# The work dir must not exist or must be empty. It holds throwaway repositories
# and stand-ins for the main-lock binary, and is left in place afterwards.
# Exit status 0 means every case passed.
set -euo pipefail

work="${1:?usage: tests/tidy_branches_verify.sh <work dir>}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../scripts" && pwd)"
mkdir -p "$work"
if [ -n "$(ls -A "$work")" ]; then
  echo "work dir is not empty: $work" >&2
  exit 2
fi

failures=0
cases=0

# A stand-in for the main-lock binary. $1 = name, $2 = exit code, $3 = output.
make_lock() {
  mkdir -p "$work/locks"
  printf '#!/bin/sh\n[ -z "%s" ] || echo "%s"\nexit %s\n' "$3" "$3" "$2" >"$work/locks/$1"
  chmod +x "$work/locks/$1"
}

# A throwaway repository with the script copied in, one commit on main.
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
}

# Run the dry run (or --remove when $4 is "remove") and keep the report.
report() {
  local repo="$work/$1" lock="$2" mode="${3:-}"
  if [ "$mode" = "remove" ]; then
    "$repo/scripts/tidy-branches.sh" --main-lock "$lock" --remove 2>&1
  else
    "$repo/scripts/tidy-branches.sh" --main-lock "$lock" 2>&1
  fi
}

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

has_line() {
  # $1 = report, $2 = branch, $3 = text the branch's line must carry
  if printf '%s\n' "$1" | grep -F "  $2 " | grep -Fq "$3"; then echo yes; else echo no; fi
}

branch_exists() {
  if git -C "$work/$1" rev-parse --verify -q "refs/heads/$2" >/dev/null; then echo yes; else echo no; fi
}

make_lock free 0 ""
make_lock held 2 "main への push を止めた。lock の持ち主は someone"
make_lock stale 0 "警告: someone の lock は 60 分を超えている。通す"

# Positive: level with main, unheld, lock free -> reported removable, then removed.
make_repo positive
git -C "$work/positive" branch verify/level
out="$(report positive "$work/locks/free")"
check "verify branch level with main and a free lock is reported removable" "$(has_line "$out" verify/level 外せる)"
check "the dry run removes nothing" "$(branch_exists positive verify/level)"
out="$(report positive "$work/locks/free" remove)"
check "--remove deletes it" "$([ "$(branch_exists positive verify/level)" = no ] && echo yes || echo no)"

# Negative: the lock is held by someone -> kept.
make_repo held
git -C "$work/held" branch verify/level
out="$(report held "$work/locks/held" remove)"
check "a held lock keeps the verify branch" "$(has_line "$out" verify/level 残す)"
check "a held lock keeps the verify branch on disk" "$(branch_exists held verify/level)"

# Negative: a stale or corrupt lock passes with a warning -> not free, kept.
make_repo stale
git -C "$work/stale" branch verify/level
out="$(report stale "$work/locks/stale" remove)"
check "a stale lock (warning on a pass) keeps the verify branch" "$(has_line "$out" verify/level 残す)"
check "a stale lock keeps the verify branch on disk" "$(branch_exists stale verify/level)"

# Negative: the main-lock binary is missing -> cannot tell, kept.
make_repo missing
git -C "$work/missing" branch verify/level
out="$(report missing "$work/no-such-binary" remove)"
check "a missing main-lock binary keeps the verify branch" "$(has_line "$out" verify/level 残す)"
check "a missing binary keeps the verify branch on disk" "$(branch_exists missing verify/level)"

# Negative: the branch has a commit of its own -> never removed, lock free or not.
make_repo ahead
git -C "$work/ahead" switch -q -c verify/work
git -C "$work/ahead" commit -q --allow-empty -m work
git -C "$work/ahead" switch -q main
out="$(report ahead "$work/locks/free" remove)"
check "a verify branch with its own commit is kept" "$(has_line "$out" verify/work 未統合)"
check "a verify branch with its own commit is kept on disk" "$(branch_exists ahead verify/work)"

# Negative: a worktree holds the branch -> kept.
make_repo held_by_worktree
git -C "$work/held_by_worktree" branch verify/level
git -C "$work/held_by_worktree" worktree add -q "$work/held_by_worktree/.claude/worktrees/w" verify/level
out="$(report held_by_worktree "$work/locks/free" remove)"
check "a verify branch held by a worktree is kept" "$(branch_exists held_by_worktree verify/level)"

# Negative control: a branch that is not under verify/ keeps the old rule.
make_repo other
git -C "$work/other" branch feat/fresh
out="$(report other "$work/locks/free" remove)"
check "a level branch outside verify/ is still read as not started" "$(has_line "$out" feat/fresh 残す)"
check "a level branch outside verify/ stays on disk" "$(branch_exists other feat/fresh)"

# The lock is the only difference between the first and second case.
make_repo pair
git -C "$work/pair" branch verify/level
held_out="$(report pair "$work/locks/held")"
free_out="$(report pair "$work/locks/free")"
check "the free and held reports differ on the same branch" "$([ "$held_out" != "$free_out" ] && echo yes || echo no)"

# Under `sudo -n nice -n -5` $HOME is /root. The default main-lock binary must
# still be found, so a removable verify/ branch is reported removable. Only
# meaningful while the real lock is free, so the case asks the real binary first.
real_lock="$(getent passwd "$(id -un)" | cut -d: -f6)/.claude/hooks/rust/target/release/main-lock"
zero="0000000000000000000000000000000000000000"
real_out="$(printf 'refs/heads/main %s refs/heads/main %s\n' "$zero" "$zero" | "$real_lock" check tidy-branches 2>&1)" && real_rc=0 || real_rc=$?
if [ -x "$real_lock" ] && [ "$real_rc" = "0" ] && [ -z "$real_out" ] && sudo -n true 2>/dev/null; then
  make_repo sudo_home
  git -C "$work/sudo_home" branch verify/level
  out="$(sudo -n env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0='*' nice -n -5 "$work/sudo_home/scripts/tidy-branches.sh" 2>&1)"
  check "under sudo -n nice -n -5 the default main-lock is found and the branch is removable" "$(has_line "$out" verify/level 外せる)"
else
  echo "skip the sudo case (real lock not free, binary missing, or sudo unavailable)"
fi

echo
echo "cases=$cases failures=$failures"
[ "$failures" = "0" ]
