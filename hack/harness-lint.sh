#!/usr/bin/env bash
#
# The harnesses themselves, checked without a Windows host.
#
# Every other script in this directory needs a real desktop, real key injection
# and usually the network, so none of them can run in a build gate. This one
# needs nothing. It runs in about a second and catches the two ways a harness
# stops testing anything while still exiting 0:
#
#   1. A grep for a log line the Rust no longer emits. Renaming a `.info()`
#      message is a one word change that no compiler and no test objects to,
#      and every assertion built on that message silently becomes "this string
#      is absent", which is what a passing grep -q looks like when inverted and
#      what a `check` helper reports as nothing at all. The four bugs that
#      reached a user were all invisible to the harnesses; a drifted message
#      would make the harnesses invisible to everything.
#
#   2. A harness that starts an overlay without arming the kill. A leftover
#      overlay owns the hotkey registration and answers the NEXT harness's
#      press itself, which reads as that harness passing when it never started.
#      That rule is structural, so it can be enforced structurally.
#
# It also syntax checks every script here, because a harness with a syntax
# error fails at the top and its "FAIL:" lines never print.
#
# Run it from anywhere. It touches nothing and starts nothing.

set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app="$(cd "$here/.." && pwd)"
core="$(cd "$app/../poe-wayfinder-core" && pwd)"

fail=0

say_fail() {
    echo "FAIL: $1"
    fail=1
}

# ------------------------------------------------------------------
# 1. Syntax.
# ------------------------------------------------------------------

echo "checking syntax"

for script in "$here"/*.sh; do
    if ! bash -n "$script" 2>/tmp/harness-lint-syntax.log; then
        say_fail "$(basename "$script") does not parse. None of its assertions run."
        sed 's/^/      /' /tmp/harness-lint-syntax.log
    fi
done

[ "$fail" -eq 0 ] && echo "  every script parses"

# ------------------------------------------------------------------
# 2. Every asserted log message still exists in the Rust.
# ------------------------------------------------------------------

echo
echo "checking asserted log messages against the source"

# This script is excluded from its own scan. It contains the pattern it
# searches with, so including it made the lint report that the Rust does not
# log a message called `[^`, which is true and useless.
scripts=$(ls "$here"/*.sh | grep -v '/harness-lint\.sh$')

# shellcheck disable=SC2086
messages=$(grep -oh '"msg":"[^"'"'"']*' $scripts 2>/dev/null \
    | sed 's/^"msg":"//' | sort -u)

checked=0
missing=0

while IFS= read -r message; do
    [ -n "$message" ] || continue

    # An alternation is a shell-side regex over two real messages rather than
    # one literal, so it is checked as its parts, not as itself.
    case "$message" in
        *"("*) continue ;;
    esac

    checked=$((checked + 1))

    if ! grep -rqF "\"$message" --include=*.rs "$app/src" "$core/src" 2>/dev/null; then
        say_fail "no Rust code logs \"$message\", but a harness asserts on it."
        echo "      Whichever harness greps for it is asserting on a string that can"
        echo "      never appear, so that assertion now proves nothing."
        grep -ln "$message" "$here"/*.sh 2>/dev/null | sed 's|^|      seen in |'
        missing=$((missing + 1))
    fi
done <<<"$messages"

if [ "$missing" -eq 0 ]; then
    echo "  all $checked asserted messages are still logged"
fi

# ------------------------------------------------------------------
# 3. Every harness that starts an overlay arms the kill.
# ------------------------------------------------------------------

echo
echo "checking every harness kills leftovers on entry and on exit"

armed=0

for name in press-check both-games-check refresh-check focus-check \
            league-check exchange-check pad-check; do
    script="$here/$name.sh"

    [ -f "$script" ] || { say_fail "$name.sh is missing."; continue; }

    if ! grep -q 'source .*harness.sh' "$script"; then
        say_fail "$name.sh does not source harness.sh, so it has its own copy of"
        echo "      the determinism rules and will drift from the other six."
        continue
    fi

    # Either the library call, which does both halves, or an explicit pair. The
    # trap alone is not enough: refresh-check.sh had the trap and no kill on
    # entry, so it inherited whatever the previous harness left running.
    if grep -q '^arm_harness' "$script"; then
        armed=$((armed + 1))
        continue
    fi

    if grep -q '^trap ' "$script" && grep -q '^stop_overlays' "$script"; then
        armed=$((armed + 1))
        continue
    fi

    say_fail "$name.sh starts an overlay without both killing leftovers on entry"
    echo "      and trapping the kill on exit. A leftover overlay owns the hotkey"
    echo "      and answers the next harness's press, which reads as a pass."
done

[ "$armed" -eq 7 ] && echo "  all 7 harnesses arm the kill"

echo

if [ "$fail" -eq 0 ]; then
    echo "PASS: the harnesses parse, assert on messages that exist, and clean up."
else
    echo "The end to end harnesses cannot be run in a build gate, so this is the"
    echo "only thing standing between a renamed log line and a suite that quietly"
    echo "stops testing."
fi

exit "$fail"
