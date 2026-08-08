#!/usr/bin/env bash
#
# The whole price check, end to end, with nobody at the keyboard.
#
# Five links: the key press, the copy, the parse, the filter, the search. Only
# the copy ever needed a game, because the overlay presses Ctrl+C at it and
# waits for the clipboard to change. `--fake-game` supplies exactly that and
# nothing else, so all five run here.
#
# It is worth having because the bugs hid behind that one link, and every one of
# them broke both games:
#
#   - the frame loop ran once and stopped, so the hotkey was never read
#   - one press was counted twice, so every check cost two searches
#   - the property filters used invented trade ids, so every armour search was
#     refused with "Unknown stat provided"
#   - and then with the right ids, refused again with "Unsupported stat domain",
#     because they belong in equipment_filters rather than in stats
#
# None of them showed up in a unit test and none showed up in the parity count.
#
# Needs a Windows host. Run it from WSL after `hack/deploy.sh`.

set -uo pipefail

exe="${1:?usage: press-check.sh <exe> [data-dir] [game] [item-file]}"
data="${2:-data-poe2}"
game="${3:-poe2}"
item="${4:-item.txt}"

dir="$(dirname "$exe")"
log="$dir/press-check.log"
fake="$dir/press-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

# A leftover overlay owns the hotkey registration and answers the press itself,
# which reads as this run passing when it never started. Cost an hour once.
powershell.exe -Command "Get-Process poe-trader* -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null 2>&1
sleep 2

if [ ! -f "$item" ]; then
    echo "FAIL: no item file at $dir/$item"
    exit 1
fi

# The overlay waits for the clipboard to *change*, so it has to start as
# something else or the copy cannot be told apart from what was already there.
powershell.exe -Command "Set-Clipboard -Value 'press-check placeholder'" >/dev/null 2>&1

(timeout 90 "$exe" --fake-game "Path of Exile 2" 80 "$item" >"$fake" 2>&1 &)
sleep 5

# Debug, so the heartbeat is in the log. It is how a stopped loop is caught.
(timeout 80 "$exe" --data-dir "$data" --game "$game" --log-level debug >"$log" 2>&1 &)
sleep 12

"$exe" --data-dir "$data" --game "$game" --press-hotkey || {
    echo "FAIL: the keys were not accepted."
    exit 1
}

# The search is paced by the rate limiter, which is not optional.
sleep 50

fail=0

check() {
    if grep -q "$2" "$1"; then
        return 0
    fi

    echo "FAIL: $3"
    fail=1
}

check "$log" '"msg":"the frame loop is running' "the frame loop never started."
check "$log" '"msg":"frame loop alive"' "the frame loop started and stopped. The hotkey is never read."
check "$log" '"msg":"price check hotkey pressed"' "the press never reached the frame loop."
check "$fake" 'answered Ctrl+C' "the overlay never asked the game to copy."
check "$log" '"msg":"price check finished"' "the price check never finished."

# One press, one check. Both watchers see the same press and they land a frame
# apart, so this is the only thing that catches a broken guard.
presses=$(grep -c '"msg":"price check hotkey pressed"' "$log")

if [ "$presses" -ne 1 ]; then
    echo "FAIL: one press produced $presses price checks. Each one is a request against the rate limit."
    fail=1
fi

# A refused query still "finishes". The error line is the only thing that
# separates a real answer from one the trade api threw out.
if grep -q '"msg":"searching the trade site"' "$log"; then
    echo "FAIL: the trade api refused the search:"
    grep '"msg":"searching the trade site"' "$log" | tail -1
    fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "PASS: press, copy, parse, filter and search all ran."
else
    echo
    echo "Logs: $log and $fake"
fi

exit "$fail"
