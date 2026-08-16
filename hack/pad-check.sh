#!/usr/bin/env bash
#
# The whole price check driven by a controller, with nobody touching one.
#
# Windows offers no way to synthesise a gamepad press. There is no hook, no
# injection API, nothing. So this harness cannot press a pad, and no harness
# ever will. What it can do is what `--fake-game` already does for the game:
# stand in for the one thing that cannot be automated, and leave every other
# link real.
#
# `--gamepad-script` replaces the pad source with a file of presses. Everything
# after it is the shipped code: the chord matcher, the price check, the panel,
# the focus model, the filter edits and the close.
#
# The script is counted in polls of the frame loop rather than in seconds, so
# the same file behaves the same on a fast machine and a slow one. That is the
# difference between a harness that flakes and one that means something.
#
# What this does NOT cover, and cannot:
#
#   - reading a real HID device. That is `--pad-walkthrough` with a pad in hand,
#     and the capture it writes, which core replays on every run.
#   - the game reacting to the same buttons. Nothing can hide a pad button from
#     the game, so only a player in a real session can judge that.
#
# Needs a Windows host. Run it from WSL after `hack/deploy.sh`.

set -uo pipefail

source "$(cd "$(dirname "$0")" && pwd)/harness.sh"

exe="${1:?usage: pad-check.sh <exe> [script]}"
script="${2:-open-edit-close.pad}"

here="$(cd "$(dirname "$0")" && pwd)"
scripts_dir="$here/pads"
items_dir="$here/items"
dir="$(dirname "$exe")"
log="$dir/pad-check.log"
fake="$dir/pad-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

require_killable_name "$exe"

arm_harness

if [ -f "$scripts_dir/$script" ]; then
    cp "$scripts_dir/$script" "$dir/$script"
fi

if [ ! -f "$script" ]; then
    echo "FAIL: no pad script at $dir/$script"
    exit 1
fi

if [ -f "$items_dir/item.txt" ]; then
    cp "$items_dir/item.txt" "$dir/item.txt"
fi

powershell.exe -Command "Set-Clipboard -Value 'pad-check placeholder'" >/dev/null 2>&1

(timeout 210 "$exe" --fake-game "Path of Exile 2" 200 item.txt >"$fake" 2>&1 &)

wait_for 20 "$fake" 'fakegame' || echo "note: the stand-in printed nothing yet, continuing"

# No hotkey is pressed anywhere in this harness. The pad is the only input, so
# a pass here means the pad path works on its own rather than riding on the
# keyboard one.
(timeout 180 "$exe" --game poe2 --gamepad-script "$script" \
    --gamepad-chord "L1+R1+Triangle" --log-level debug >"$log" 2>&1 &)

if ! wait_for 60 "$log" '"msg":"the frame loop is running'; then
    echo "FAIL: the overlay never started, so nothing below was measured."
    echo "Logs: $log and $fake"
    exit 1
fi

wait_for 30 "$log" '"msg":"a scripted pad is standing in for a real one' \
    || echo "note: the scripted pad line has not appeared yet"

wait_for_check_to_settle 90 "$log"

# The script keeps running after the check, walking the panel and editing a
# filter, so the assertions below need it to have played out.
wait_for 60 "$log" '"msg":"the panel was closed from the pad' \
    || echo "note: the close has not been logged yet"

fail=0

check() {
    if grep -q "$2" "$1"; then
        return 0
    fi

    echo "FAIL: $3"
    fail=1
}

check "$log" '"msg":"a controller chord fires the locked price check' \
    "the chord was never read out of the config."
check "$log" '"msg":"controller chord held"' \
    "the scripted chord never reached the frame loop."
check "$fake" 'answered Ctrl+C' \
    "the pad fired but the overlay never asked the game to copy."
check "$log" '"msg":"the panel is up"' \
    "the pad fired but no panel appeared."
check "$log" '"msg":"price check finished"' \
    "the price check never finished from a pad press."
check "$log" '"msg":"the pad moved in the panel"' \
    "the pad never moved the focus, so the panel is not navigable."
check "$log" '"msg":"the pad changed the search"' \
    "the pad never edited a filter, so a value cannot be changed with it."
check "$log" '"msg":"the panel was closed from the pad' \
    "the pad never closed the panel, so a player is stuck with a mouse."

# Assert on the wire rather than on a re-derivation. stat_rows is what the user
# reads, and a panel that finishes with none is a panel nobody can adjust.
rows="$(field "$log" "price check finished" stat_rows)"
rows="${rows:-0}"

if [ "$rows" -gt 0 ]; then
    echo "ok: the panel came up with $rows stat rows"
else
    echo "FAIL: the price check finished with no filter rows, so there was nothing to navigate."
    fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "PASS: a pad alone opened, drove and closed a price check."
    exit 0
fi

echo
echo "Logs: $log and $fake"
exit 1
