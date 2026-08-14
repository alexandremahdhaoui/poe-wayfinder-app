#!/usr/bin/env bash
#
# The data refreshes itself, from GGG and nobody else.
#
# Four things no unit test can see, because each needs a real socket and a real
# config directory:
#
#   1. a first run fetches, and fetches only www.pathofexile.com
#   2. what it writes is read back on the next launch, in preference to the
#      copy built into the binary
#   3. a corrupt cache falls back to the built in copy rather than refusing to
#      start, which is what keeps a bad night from bricking the tool
#   4. the second launch does NOT fetch again, which is the seven day throttle
#
# Point 4 is the one that matters for not annoying GGG. Point 3 is the one that
# matters for the user.
#
# Run it from WSL. It does not need the game.

set -uo pipefail

source "$(cd "$(dirname "$0")" && pwd)/harness.sh"

exe="${1:?usage: refresh-check.sh <exe>}"

dir="$(dirname "$exe")"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

# Relative on purpose. This is a Windows binary and $dir is a WSL path, so
# handing it "/mnt/c/..." makes Windows read the leading slash as the root of
# the current drive and quietly build C:\mnt\c\Users\... instead. Relative
# to the exe's own directory is the only spelling both sides agree on.
cfg="refresh-check-config"

rm -rf "$cfg"
mkdir -p "$cfg"

# Kills any leftover overlay and arms the trap that kills this one however the
# script ends. The reasoning lives in harness.sh.
#
# This script used to arm the trap WITHOUT killing on entry, which is half the
# rule and the wrong half. A leftover overlay from the previous harness holds
# the same config directory this one is about to build, so the "second launch
# fetched nothing" assertion below was being measured against a cache another
# process was still writing.
arm_harness

fail=0

say_fail() {
    echo "FAIL: $1"
    fail=1
}

# `timeout` does NOT stop a windows subsystem exe launched from WSL. It kills
# the interop proxy and the Windows process runs on forever. So each of the
# three launches below was still alive while the next one started: three
# overlays sharing one config directory, one of them still fetching, and the
# throttle assertion measuring a cache another process was writing.
#
# Killing after each launch is what makes the three launches three launches
# rather than one long overlapping mess.
run() {
    timeout 75 "$exe" --config-dir "$cfg" --log-level debug >"$1" 2>&1

    stop_overlays
    sleep 2
}

echo "first launch, empty config directory"
run "$dir/refresh-1.log"

# The refresh runs on a background thread and the overlay may exit before it
# finishes, so give it room. Three small requests per game.
for _ in $(seq 1 60); do
    [ -f "$cfg/data-poe2/stats.ndjson" ] && [ -f "$cfg/data-poe1/stats.ndjson" ] && break
    sleep 1
done

if grep -q '"origin":"embedded"' "$dir/refresh-1.log"; then
    echo "PASS: the first launch ran from the built in copy."
else
    say_fail "the first launch did not use the built in data."
fi

for game in poe1 poe2; do
    for table in stats items; do
        if [ -s "$cfg/data-$game/$table.ndjson" ]; then
            echo "PASS: refreshed $game/$table.ndjson"
        else
            say_fail "no refreshed $game/$table.ndjson was written."
        fi
    done

    if [ -f "$cfg/data-$game/refreshed-at" ]; then
        echo "PASS: $game carries a refresh stamp."
    else
        say_fail "$game has no refresh stamp, so the throttle cannot work."
    fi
done

# The allowlist claim. Every url the refresh logged must be the official site.
if grep -oE '"url":"https?://[^/"]+' "$dir/refresh-1.log" \
    | grep -v 'www\.pathofexile\.com' | grep -q .; then
    say_fail "the refresh reached a host that is not www.pathofexile.com:"
    grep -oE '"url":"https?://[^/"]+' "$dir/refresh-1.log" | sort -u
else
    echo "PASS: nothing outside www.pathofexile.com was contacted."
fi

echo
echo "second launch, cache present"
run "$dir/refresh-2.log"

if grep -q '"origin":"cache"' "$dir/refresh-2.log"; then
    echo "PASS: the refreshed cache won over the built in copy."
else
    say_fail "the second launch ignored the cache it just wrote."
fi

# The augments come from the game bundles and no API has them. A refresh that
# writes stats beside them must not take them away.
if grep '"msg":"loaded the game data"' "$dir/refresh-2.log" \
    | grep '"game":"poe2"' | grep -qE '"augments":[1-9]'; then
    echo "PASS: the item editor still has its augments after a refresh."
else
    say_fail "the refresh cost poe2 its augments, so the item editor is now empty."
fi

# The throttle. Nothing should have been fetched this time.
if grep -q '"msg":"refreshed the game data' "$dir/refresh-2.log"; then
    say_fail "the second launch refreshed again. The seven day throttle is not working."
else
    echo "PASS: the second launch fetched nothing."
fi

echo
echo "third launch, cache corrupted on purpose"
echo "{not json" > "$cfg/data-poe2/stats.ndjson"

run "$dir/refresh-3.log"

if grep '"msg":"loaded the game data"' "$dir/refresh-3.log" \
    | grep '"game":"poe2"' | grep -q '"origin":"embedded"'; then
    echo "PASS: a corrupt cache fell back to the built in copy."
else
    say_fail "a corrupt cache was not recovered from. A bad write bricks the tool."
    grep '"msg":"loaded the game data"' "$dir/refresh-3.log" | tail -2
fi

if grep -q '"msg":"loading game data"' "$dir/refresh-3.log"; then
    say_fail "a corrupt cache stopped the exe starting."
fi

# A refresh rebuilds the stat and item tables from what the trade api returns,
# so it is a place a bad number can enter the data rather than the panel. Cheap
# to check here and it costs nothing when it passes.
for launch in 1 2 3; do
    assert_numbers_are_real "$dir/refresh-$launch.log" >/dev/null || {
        echo "FAIL: launch $launch logged a value that cannot be a real number."
        assert_numbers_are_real "$dir/refresh-$launch.log"
        fail=1
    }
done

rm -rf "$cfg"

echo
if [ "$fail" -eq 0 ]; then
    echo "PASS: the data refreshes itself, from GGG only, once a week."
else
    echo "Logs: $dir/refresh-1.log, refresh-2.log and refresh-3.log"
fi

exit "$fail"
