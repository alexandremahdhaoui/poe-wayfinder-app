#!/usr/bin/env bash
#
# The rules every end to end harness has to obey, in one place.
#
# Sourced, never run. Every script under hack/ that starts the overlay sources
# this and then calls arm_harness once, near the top.
#
# It exists because the same four determinism bugs were rediscovered in four
# separate scripts:
#
#   1. A leftover overlay owns the hotkey registration and answers the next
#      run's press itself. That reads as the next run PASSING when it never
#      started. `timeout` does not stop it: the exe is windows subsystem, so
#      launched from WSL it detaches from the interop proxy and timeout kills
#      the proxy while the Windows process runs on. One orphan reached 26900
#      frames against a 120 second timeout. So: kill on entry AND from a trap.
#
#   2. Injected input is occasionally swallowed before any window sees it. One
#      lost press looks exactly like a broken hotkey. Retry three times; a
#      failure then a success is not a regression.
#
#   3. A fixed sleep is either too short, which is flaky, or long enough to
#      outlive the --fake-game stand-in, which closes the panel for the wrong
#      reason and reads as a pass. Wait for a CONDITION, bounded.
#
#   4. A saturated or non finite number reaches the panel and nothing fails. A
#      filter row once printed 9223372036854775807, which is i64::MAX, from
#      `f64::INFINITY as i64`. Every harness checks its own log for that now,
#      so whichever one happens to run is enough to catch it.
#
# Nothing here computes a screen coordinate from a logged rect. The rect in the
# log is in LOGICAL points and --move-mouse takes logical coordinates that it
# then scales by the real DPI. An attempt at it aimed at 1282,912 and put the
# cursor at 1923,1368 on a 150% display.

# shellcheck shell=bash

# Kill every overlay, including a --fake-game stand-in, which is the same exe.
stop_overlays() {
    powershell.exe -Command \
        "Get-Process poe-wayfinder* -ErrorAction SilentlyContinue | Stop-Process -Force" \
        >/dev/null 2>&1

    return 0
}

# Call once per harness, before anything is started. Kills what is already
# running, then guarantees the same kill on the way out however the script
# ends, including Ctrl+C and a failed `exit 1` half way down.
arm_harness() {
    trap stop_overlays EXIT INT TERM

    stop_overlays
    sleep 2
}

# wait_for <seconds> <file> <pattern...>
#
# Polls once a second until any pattern appears, then returns 0. Returns 1 when
# the budget runs out. Always prefer this to `sleep`.
wait_for() {
    local budget="$1" file="$2"
    shift 2

    local waited=0 pattern

    while [ "$waited" -lt "$budget" ]; do
        for pattern in "$@"; do
            grep -q "$pattern" "$file" 2>/dev/null && return 0
        done

        sleep 1
        waited=$((waited + 1))
    done

    return 1
}

# wait_for_file <seconds> <path>
wait_for_file() {
    local budget="$1" path="$2" waited=0

    while [ "$waited" -lt "$budget" ]; do
        [ -s "$path" ] && return 0

        sleep 1
        waited=$((waited + 1))
    done

    return 1
}

# press_until <exe> <log> [extra flags for the pressing process...]
#
# Presses, then waits up to six seconds for the running overlay to report it.
# Three attempts, because injected input is occasionally swallowed.
#
# The pressing process is a second copy of the same exe. It presses whatever
# ITS OWN --price-check-hotkey resolves to, so passing
# `--price-check-hotkey "Ctrl+Alt+D"` here drives the locked binding of the
# overlay already running, with no mouse involved at all.
press_until() {
    local exe="$1" log="$2"
    shift 2

    local before attempt

    before=$(grep -c '"msg":"price check hotkey pressed"' "$log" 2>/dev/null)
    before=${before:-0}

    for attempt in 1 2 3; do
        "$exe" "$@" --press-hotkey >/dev/null 2>&1

        local waited=0

        while [ "$waited" -lt 6 ]; do
            sleep 1
            waited=$((waited + 1))

            local now
            now=$(grep -c '"msg":"price check hotkey pressed"' "$log" 2>/dev/null)
            now=${now:-0}

            [ "$now" -gt "$before" ] && return 0
        done

        echo "note: press $attempt did not land, retrying. Injected input is sometimes swallowed."
    done

    return 1
}

# wait_for_check_to_settle <seconds> <log>
#
# A price check ends one of three ways: it finished, it never got as far as
# searching, or the trade api refused it. Waiting only for "finished" burns the
# whole budget on the two failure paths and then asserts anyway.
wait_for_check_to_settle() {
    wait_for "$1" "$2" \
        '"msg":"price check finished"' \
        '"msg":"the price check stopped before it could search"' \
        '"msg":"searching the trade site"'
}

# lines_after <file> <pattern>
#
# Everything logged from the first match onwards. The log is append ordered, so
# this is how an assertion is scoped to "after the panel took focus" without
# sampling a count before and after and racing whatever happens in between.
lines_after() {
    awk -v needle="$2" 'index($0, needle) { seen = 1 } seen' "$1"
}

# assert_numbers_are_real <file>
#
# Bug 4. A filter row printed 9223372036854775807, which is i64::MAX, produced
# by `f64::INFINITY as i64`. The user sees a bound nobody can have rolled and
# the trade site gets a filter nothing can match.
#
# `describe` in src/driver/overlay_loop/win.rs now writes "NOT FINITE: ..." for
# any bound that is not finite, so that string is the direct signal. The rest
# catches a value that reached the log by some other route: Rust prints f64
# infinity as `inf` and NaN as `NaN`, and no honest field in this app carries a
# fifteen digit integer. The log line has no timestamp, so there is nothing
# large and legitimate to trip over.
#
# Each field is pulled out whole before it is judged, and the token has to sit
# on its own inside the value. Matching the raw line instead flagged every
# single log line in the file, because `"level":"info"` contains `inf`.
assert_numbers_are_real() {
    local file="$1" hits

    hits=$(grep -oE '"[a-z_]+":("[^"]*"|-?[0-9]+(\.[0-9]+)?)' "$file" 2>/dev/null \
        | grep -E '(^|[^0-9A-Za-z_])(NOT FINITE|inf|NaN|[0-9]{15,})([^0-9A-Za-z_]|$)' \
        | sort -u | head -5)

    if [ -n "$hits" ]; then
        echo "FAIL: a value that cannot be a real number reached the log."
        echo "      The user sees a filter bound nobody could have rolled and the"
        echo "      trade site gets a filter nothing matches."
        echo "$hits" | sed 's/^/      /'

        return 1
    fi

    echo "PASS: every number logged is finite and within reach."

    return 0
}

# assert_currency_is_named <file>
#
# Bug 3, the half that is visible in any log. A bulk exchange sent with an empty
# `have` list means "price this in anything the seller offers", so an Orb of
# Augmentation came back as "~99 waystone-3". A currency the user can act on is
# a name. A trade id with a number stuck on the end is the exchange answering in
# tier 3 waystones.
assert_currency_is_named() {
    local file="$1" hits

    hits=$(grep -oE '"(currency|price)":"[^"]*"' "$file" 2>/dev/null \
        | grep -E '\-[0-9]+"' | sort -u | head -5)

    if [ -n "$hits" ]; then
        echo "FAIL: a price came back in a raw trade id rather than a currency."
        echo "      This is the empty exchange have list: the seller answered in"
        echo "      whatever they had, and the number on screen means nothing."
        echo "$hits" | sed 's/^/      /'

        return 1
    fi

    return 0
}

# assert_stand_in_survived <file>
#
# A stand-in that exits mid run takes the panel down with it, and the panel
# closing is what several harnesses assert. Sleeping a fixed 45 seconds outlived
# a stand-in once and the run passed for entirely the wrong reason.
assert_stand_in_survived() {
    local file="$1"

    if grep -q '"msg":"the game window is gone' "$file"; then
        echo "FAIL: the game stand-in disappeared while the run was still asserting."
        echo "      Anything the panel did after this point says nothing about the"
        echo "      overlay. Give --fake-game more seconds than the run needs."

        return 1
    fi

    return 0
}

# field <file> <msg> <field>
#
# The value of one field on the last occurrence of one log line.
#
# A quoted value is taken whole. Stopping at the first comma instead read
# `"priced_in":"exalted,divine"` as `exalted` and the assertion built on it
# compared the wrong string, which is a harness that fails on a correct build.
field() {
    grep "\"msg\":\"$2\"" "$1" 2>/dev/null | tail -1 \
        | grep -oE "\"$3\":(\"[^\"]*\"|[^,}]*)" | head -1 \
        | sed -e "s/^\"$3\"://" -e 's/^"//' -e 's/"$//'
}
