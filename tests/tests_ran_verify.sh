#!/usr/bin/env bash
# Tests for scripts/tests-ran.sh and scripts/touched-checks.sh.
#
# Usage: tests/tests_ran_verify.sh <work dir>
#
# The work dir must not exist or must be empty. It holds one throwaway repository (a crate
# `demo` under native/ with a lib, three integration targets and a support module) and the
# fake test logs, and is left in place afterwards. Neither script starts cargo, so neither does
# this test. Exit status 0 means every case passed.
set -euo pipefail

work="${1:?usage: tests/tests_ran_verify.sh <work dir>}"
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

yes_if() { if "$@" >/dev/null 2>&1; then echo yes; else echo no; fi; }
has() { if printf '%s\n' "$1" | grep -Eq -- "$2"; then echo yes; else echo no; fi; }
lacks() { if printf '%s\n' "$1" | grep -Eq -- "$2"; then echo no; else echo yes; fi; }

repo="$work/repo"
mkdir -p "$repo/native/demo/src" "$repo/native/demo/tests/support"
git -C "$repo" init -q -b main
git -C "$repo" config user.name t
git -C "$repo" config user.email t@t
git -C "$repo" config commit.gpgsign false
: >"$repo/native/demo/Cargo.toml"
printf '#[cfg(test)]\nmod tests {\n    #[test]\n    fn old_unit() {}\n}\n' >"$repo/native/demo/src/lib.rs"
printf 'mod support;\n\n#[test]\nfn old_a() {\n    let _ = 1;\n}\n' >"$repo/native/demo/tests/a.rs"
printf '#[test]\nfn old_b() {}\n' >"$repo/native/demo/tests/b.rs"
printf '// shared helpers\n' >"$repo/native/demo/tests/support/mod.rs"
git -C "$repo" add -A
git -C "$repo" commit -q -m base
git -C "$repo" checkout -q -b feat

# The change: a new unit test, two new tests in a (one #[ignore]d), a body change of an old test,
# a changed support module, and a new untracked target c. A docs file changes too.
printf '#[cfg(test)]\nmod tests {\n    #[test]\n    fn old_unit() {}\n\n    #[test]\n    fn new_unit() {}\n}\n' >"$repo/native/demo/src/lib.rs"
printf 'mod support;\n\n#[test]\nfn old_a() {\n    let _ = 2;\n}\n\n#[test]\nfn new_a() {}\n\n#[test]\n#[ignore = "real run"]\nfn new_real() {}\n\n#[test]\nfn new_a_more() {}\n' >"$repo/native/demo/tests/a.rs"
printf '// shared helpers, changed\n' >"$repo/native/demo/tests/support/mod.rs"
printf '#[test]\nfn fresh_c() {}\n' >"$repo/native/demo/tests/c.rs"
mkdir -p "$repo/docs"
printf 'a note\n' >"$repo/docs/note.md"

tr="$script_dir/tests-ran.sh"
tc="$script_dir/touched-checks.sh"

# The logs: cargo prints `test <path> ... ok`; a lib test carries its module path.
mkdir -p "$work/logs"
all_ok="$work/logs/all_ok.log"
printf 'test tests::new_unit ... ok\ntest new_a ... ok\ntest new_a_more ... ok\ntest fresh_c ... ok\n' >"$all_ok"
empty="$work/logs/empty.log"
: >"$empty"
no_c="$work/logs/no_c.log"
printf 'test tests::new_unit ... ok\ntest new_a ... ok\ntest new_a_more ... ok\n' >"$no_c"
failed="$work/logs/failed.log"
printf 'test tests::new_unit ... ok\ntest new_a ... FAILED\ntest new_a_more ... ok\ntest fresh_c ... ok\n' >"$failed"
only_more="$work/logs/only_more.log"
printf 'test tests::new_unit ... ok\ntest new_a_more ... ok\ntest fresh_c ... ok\n' >"$only_more"
should_panic="$work/logs/should_panic.log"
printf 'test tests::new_unit ... ok\ntest new_a - should panic ... ok\ntest new_a_more ... ok\ntest fresh_c ... ok\n' >"$should_panic"
should_panic_failed="$work/logs/should_panic_failed.log"
printf 'test tests::new_unit ... ok\ntest new_a - should panic ... FAILED\ntest new_a_more ... ok\ntest fresh_c ... ok\n' >"$should_panic_failed"
longer_should_panic="$work/logs/longer_should_panic.log"
printf 'test tests::new_unit ... ok\ntest new_a_more - should panic ... ok\ntest fresh_c ... ok\n' >"$longer_should_panic"
with_real="$work/logs/with_real.log"
printf 'test tests::new_unit ... ok\ntest new_a ... ok\ntest new_a_more ... ok\ntest fresh_c ... ok\ntest new_real ... ok\n' >"$with_real"

out="$("$tr" "$repo" main "$empty" 2>&1 || true)"
check "an empty log names every new test that is not ignored" \
  "$([ "$(has "$out" '^MISSING new_unit ')" = yes ] && [ "$(has "$out" '^MISSING new_a ')" = yes ] && [ "$(has "$out" '^MISSING new_a_more ')" = yes ] && [ "$(has "$out" '^MISSING fresh_c ')" = yes ] && echo yes || echo no)"
check "an empty log leaves the exit status at 1" "$(yes_if bash -c '! "$0" "$1" main "$2"' "$tr" "$repo" "$empty")"
check "an unchanged test is not named, and neither is a test whose body alone changed" \
  "$([ "$(lacks "$out" 'old_unit')" = yes ] && [ "$(lacks "$out" 'old_a')" = yes ] && [ "$(lacks "$out" 'old_b')" = yes ] && echo yes || echo no)"
check "an ignored test is reported as not run and is not counted as missing" \
  "$([ "$(has "$out" '^ignored not-run +new_real ')" = yes ] && [ "$(lacks "$out" '^MISSING new_real')" = yes ] && echo yes || echo no)"
check "the summary line counts 4 added tests that are run-able, 1 ignored, 4 missing" \
  "$(has "$out" 'added=5 run_ok=0 waiting=0 ignored=1 ignored_ok=0 missing=4 ')"

out="$("$tr" "$repo" main "$all_ok" 2>&1 || true)"
check "a log with a pass line for each test gives exit 0" "$(yes_if "$tr" "$repo" main "$all_ok")"
check "a module path before the name still counts as the pass line" "$(has "$out" 'run_ok=4 .*missing=0 ')"

out="$("$tr" "$repo" main "$failed" 2>&1 || true)"
check "a FAILED line is no pass line: that test is named and the exit status is 1" \
  "$([ "$(has "$out" '^MISSING new_a ')" = yes ] && [ "$(yes_if bash -c '! "$0" "$1" main "$2"' "$tr" "$repo" "$failed")" = yes ] && echo yes || echo no)"

out="$("$tr" "$repo" main "$should_panic" 2>&1 || true)"
check "the pass line of a #[should_panic] test (\`- should panic ... ok\`) counts" \
  "$([ "$(has "$out" 'run_ok=4 .*missing=0 ')" = yes ] && [ "$(yes_if "$tr" "$repo" main "$should_panic")" = yes ] && echo yes || echo no)"
out="$("$tr" "$repo" main "$should_panic_failed" 2>&1 || true)"
check "a #[should_panic] line that ends in FAILED is no pass line" "$(has "$out" '^MISSING new_a ')"
out="$("$tr" "$repo" main "$longer_should_panic" 2>&1 || true)"
check "the should-panic line of a longer name does not stand for a shorter name" \
  "$([ "$(has "$out" '^MISSING new_a ')" = yes ] && [ "$(lacks "$out" '^MISSING new_a_more')" = yes ] && echo yes || echo no)"

out="$("$tr" "$repo" main "$only_more" 2>&1 || true)"
check "the pass line of a longer name does not stand for a shorter name" \
  "$([ "$(has "$out" '^MISSING new_a ')" = yes ] && [ "$(lacks "$out" '^MISSING new_a_more')" = yes ] && echo yes || echo no)"

out="$("$tr" "$repo" main "$no_c" 2>&1 || true)"
check "a test of an untracked file is counted" "$(has "$out" '^MISSING fresh_c ')"

out="$(WAITING='fresh_c' "$tr" "$repo" main "$no_c" 2>&1 || true)"
check "a name kept out on purpose is reported as waiting, with its regex, and does not decide the exit status" \
  "$([ "$(has "$out" '^waiting fresh_c .*kept out of the run by the caller: fresh_c')" = yes ] && [ "$(yes_if env WAITING=fresh_c "$tr" "$repo" main "$no_c")" = yes ] && echo yes || echo no)"
check "WAITING does not cover another name" \
  "$(yes_if bash -c '! WAITING=fresh_c "$0" "$1" main "$2"' "$tr" "$repo" "$only_more")"

out="$("$tr" "$repo" main "$with_real" 2>&1 || true)"
check "an ignored test with a pass line from a real run is reported as ok" "$(has "$out" '^ignored ok +new_real ')"

check "a log that cannot be read is exit 2, not a missing test" \
  "$(yes_if bash -c '"$0" "$1" main "$2"; [ $? -eq 2 ]' "$tr" "$repo" "$work/logs/nope.log")"
check "no log at all is exit 2" "$(yes_if bash -c '"$0" "$1" main; [ $? -eq 2 ]' "$tr" "$repo")"
check "a base that does not resolve is exit 2" \
  "$(yes_if bash -c '"$0" "$1" no-such-rev "$2"; [ $? -eq 2 ]' "$tr" "$repo" "$all_ok")"

list="$("$tc" "$repo" main)"
check "touched-checks names the lib of the touched crate" "$(has "$list" '^-p demo --lib$')"
check "touched-checks names a target whose file changed" "$(has "$list" '^-p demo --test a$')"
check "touched-checks names a target that is new and untracked" "$(has "$list" '^-p demo --test c$')"
check "touched-checks names no bin target for a crate without one" "$(lacks "$list" '--bins')"
check "touched-checks prints the target it leaves out, with its crate" "$(has "$list" '^# not run: demo b$')"

printf '// shared helpers, twice changed\n' >>"$repo/native/demo/tests/support/mod.rs"
git -C "$repo" add -A
git -C "$repo" commit -q -m change
git -C "$repo" checkout -q -b support-only main
printf '// only the support module\n' >>"$repo/native/demo/tests/support/mod.rs"
list="$("$tc" "$repo" main)"
check "a change in tests/support reaches the targets that declare mod support, and only those" \
  "$([ "$(has "$list" '^-p demo --test a$')" = yes ] && [ "$(lacks "$list" '^-p demo --test b$')" = yes ] && echo yes || echo no)"
git -C "$repo" checkout -q -- native/demo/tests/support/mod.rs
git -C "$repo" checkout -q -b docs-only main
mkdir -p "$repo/docs"
printf 'x\n' >"$repo/docs/other.md"
check "a change outside native names no crate" "$([ -z "$("$tc" "$repo" main)" ] && echo yes || echo no)"

echo
echo "cases=$cases failures=$failures"
[ "$failures" = "0" ]
