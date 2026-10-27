#!/usr/bin/env bash
# Runs, on HEAD, the tests of the CI line that read the result directories a push
# changes, and writes the mark that the pre-push of main-lock asks for before a push to
# main changes docs/results.
#
# usage: scripts/check-records.sh <mark file> <the words that start cargo on this machine>
#   The words are everything before the cargo sub-command (nice, user, target directory,
#   cargo-slot, the cargo binary); the script appends
#   "test --release --jobs 2 --no-fail-fast -p <crate> --test <name> ..." for each crate.
#   The push starts from origin/main.
#
# A reader is a file native/<crate>/tests/<name>.rs that spells docs/results or uses the
# RESULTS constant of the tests' support module, and whose <name> is one of the --test
# names of the cargo test lines in .github/workflows/test.yml. It runs when the push
# changes a directory under docs/results that the file names, or when the file uses
# RESULTS or names no directory literally. The changed directories are read with
# --no-renames (a file moved away counts for the directory it left) and -z (git writes a
# name as it is: it quotes a name that is not ASCII, or that holds a double quote or a
# backslash, whatever core.quotepath says). The lists are derived on every run, so a new
# reader joins without an edit here.
#
# How this can break, and what stops it:
#   first run    the release target may be cold (minutes); the words may carry a slot wait.
#                cargo runs in native/, where the workspace is (the root has no Cargo.toml).
#   contention   cargo's own rules for the words you pass (a slot, a target lock).
#   start        tracked files equal HEAD (`git diff --quiet HEAD`) and origin/main resolves:
#                the mark names HEAD, so a test of an edit that is not in HEAD would be marked.
#   waiting      none: no prompt, no stdin.
#   zero         fewer than 2 readers of docs/results in the CI line (the derivation broke),
#                no changed directory, or no reader chosen: an error, not a pass.
# Undo: remove the mark file.
set -u

die() {
  echo "check-records: $1" >&2
  exit "${2:-2}"
}

ci_tests() {
  grep -E 'cargo test .*--test ' .github/workflows/test.yml | grep -oE -- '--test [a-z0-9_]+' | cut -d' ' -f2 | sort -u
}

named_dirs() {
  grep -ohE 'docs/results/[a-z0-9_.-]+' "$1" | cut -d/ -f3 | sort -u
}

readers() {
  local name file
  for name in $(ci_tests); do
    for file in native/*/tests/"$name".rs; do
      [ -f "$file" ] && grep -qE 'docs/results|\bRESULTS\b' "$file" && echo "$file"
    done
  done
}

reads_changed() {
  local named dir
  grep -qE '\bRESULTS\b' "$1" && return 0
  named=$(named_dirs "$1")
  [ -n "$named" ] || return 0
  for dir in $2; do
    printf '%s\n' "$named" | grep -qxF -- "$dir" && return 0
  done
  return 1
}

crate_and_name() {
  local name=${1##*/}
  printf '%s:%s\n' "$(basename "$(dirname "$(dirname "$1")")")" "${name%.rs}"
}

run_crate() {
  local crate=$1 name args=()
  shift
  for name in "$@"; do args+=(--test "$name"); done
  (cd native && "${cargo[@]}" test --release --jobs 2 --no-fail-fast -p "$crate" "${args[@]}")
}

mark=${1:-}
[ -n "$mark" ] || die "the mark file is missing"
shift
[ "$#" -gt 0 ] || die "the words that start cargo are missing"
cargo=("$@")
repo=$(git rev-parse --show-toplevel) || exit 2
cd "$repo" || exit 2
git diff --quiet HEAD || die "tracked files differ from HEAD; commit first"
git rev-parse --verify --quiet "origin/main^{commit}" > /dev/null || die "origin/main does not resolve"
changed=$(git diff --name-only --no-renames -z origin/main HEAD -- docs/results | tr '\0' '\n' | cut -d/ -f3 | sort -u)
[ -n "$changed" ] || die "no file under docs/results differs from origin/main; nothing to check"
files=$(readers)
[ "$(printf '%s\n' $files | grep -c .)" -ge 2 ] || die "the CI line yields fewer than 2 readers of docs/results; the derivation broke"
chosen=""
for file in $files; do
  reads_changed "$file" "$changed" && chosen="$chosen $(crate_and_name "$file")"
done
[ -n "$chosen" ] || die "no reader names the changed directories: $(echo $changed)"
echo "check-records: changed: $(echo $changed)"
echo "check-records: readers:$chosen"
for crate in $(printf '%s\n' $chosen | cut -d: -f1 | sort -u); do
  run_crate "$crate" $(printf '%s\n' $chosen | grep "^$crate:" | cut -d: -f2) || die "$crate failed" 1
done
sha=$(git rev-parse HEAD)
printf '%s %s\n' "$sha" "$(date -Is)" > "$mark.new" && mv "$mark.new" "$mark" || exit 2
echo "check-records: marked $sha"
