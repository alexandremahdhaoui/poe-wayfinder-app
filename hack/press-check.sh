#!/usr/bin/env bash
#
# The hotkey, end to end, with nobody at the keyboard.
#
# The one link no unit test reaches: a real key press, injected by one process,
# arriving at the frame loop of an overlay running in another. It is what broke
# in game and produced no evidence at all.
#
# It checks two things, because the failure was two things:
#
#   1. the frame loop keeps running. It once ran exactly one frame and stopped,
#      which reads in a log exactly like a hotkey Windows refused to deliver.
#   2. the press arrives. `hotkey ignored` counts as arriving. Without a game
#      open that is the correct answer, and it proves the press got there.
#
# Needs a Windows host. Run it from WSL after `hack/deploy.sh`.

set -euo pipefail

exe="${1:?usage: press-check.sh <exe> [data-dir] [game]}"
data="${2:-data-poe2}"
game="${3:-poe2}"

dir="$(dirname "$exe")"
log="$dir/press-check.log"

cd "$dir"
exe="./$(basename "$exe")"

# Debug, so the heartbeat is in the log. It is how the stopped loop is caught.
(timeout 25 "$exe" --data-dir "$data" --game "$game" --log-level debug >"$log" 2>&1 &)

# Long enough for the data to load and the window to open.
sleep 8

"$exe" --data-dir "$data" --game "$game" --press-hotkey

# Long enough for the press to cross into the hook thread and be read by a
# frame, several times over.
sleep 6

fail=0

if ! grep -q '"msg":"the frame loop is running' "$log"; then
    echo "FAIL: the frame loop never started."
    fail=1
fi

if ! grep -q '"msg":"frame loop alive"' "$log"; then
    echo "FAIL: the frame loop started and stopped. The hotkey is never read."
    fail=1
fi

if grep -qE '"msg":"(hotkey ignored|price check hotkey pressed)"' "$log"; then
    echo "PASS: an injected press reached the frame loop."
else
    echo "FAIL: the press never reached the frame loop."
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo
    echo "The log is at $log"
fi

exit "$fail"
