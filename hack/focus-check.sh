#!/usr/bin/env bash
#
# The panel must survive taking focus.
#
# Four separate complaints in one play session were all one bug:
#
#   "the overlay is clunky, doesn't really work half the time"
#   "i cannot click on any of the button on the opened overlay window"
#   "pressing ctrl-alt-d i don't see anything, then if I alt tab i see it"
#   "have to press many times"
#
# None of them were the hotkey. Every press landed and every panel painted in
# under 20ms. What happened is that the panel took focus, so the GAME stopped
# being the foreground window, so `should_draw` returned false and the panel
# stopped being drawn. Focus returned to the game, it drew again, and it
# cycled. The panel vanished out from under the click.
#
# press-check.sh could not see it. It moves the pointer AWAY to prove the panel
# closes, and never leaves a panel focused, which is the only state that breaks.
#
# The locked binding is what makes this deterministic: it takes focus on
# purpose rather than waiting for a pointer to arrive. No mouse position is
# involved, so there is nothing to get flaky.
#
# Deliberately NOT done here: moving the pointer onto the panel. The rect in
# the log is in logical points and --move-mouse takes logical coordinates that
# it scales by the real DPI, so aiming at a logged rect lands somewhere else
# entirely on a scaled display. An attempt at it aimed at 1282,912 and put the
# cursor at 1923,1368.
#
# Needs a Windows host. Run it from WSL after hack/deploy.sh.

set -uo pipefail

exe="${1:?usage: focus-check.sh <exe> [item-file]}"
item="${2:-item.txt}"

game_window="Path of Exile 2"
items_dir="$(cd "$(dirname "$0")" && pwd)/items"
dir="$(dirname "$exe")"
log="$dir/focus-check.log"
fake="$dir/focus-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

stop_overlays() {
    powershell.exe -Command "Get-Process poe-wayfinder* -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null 2>&1
}

trap stop_overlays EXIT INT TERM

stop_overlays
sleep 2

[ -f "$items_dir/$item" ] && cp "$items_dir/$item" "$dir/$item"

if [ ! -f "$item" ]; then
    echo "FAIL: no item file at $dir/$item"
    exit 1
fi

powershell.exe -Command "Set-Clipboard -Value 'focus-check placeholder'" >/dev/null 2>&1

(timeout 120 "$exe" --fake-game "$game_window" 110 "$item" >"$fake" 2>&1 &)
sleep 5

(timeout 110 "$exe" --game poe2 --log-level debug >"$log" 2>&1 &)
sleep 12

fail=0

"$exe" --move-mouse 700 500 >/dev/null 2>&1
sleep 1

pressed=0

for attempt in 1 2 3; do
    "$exe" --game poe2 --price-check-hotkey "Ctrl+Alt+D" --press-hotkey >/dev/null 2>&1

    for _ in 1 2 3 4 5 6; do
        sleep 1

        if grep -q '"msg":"price check hotkey pressed"' "$log"; then
            pressed=1
            break
        fi
    done

    [ "$pressed" -eq 1 ] && break

    echo "note: press $attempt did not land, retrying"
done

if [ "$pressed" -eq 0 ]; then
    echo "FAIL: the locked press never reached the frame loop."
    exit 1
fi

if grep '"msg":"price check hotkey pressed"' "$log" | grep -q '"locked":true'; then
    echo "PASS: the locked binding fired rather than the plain one."
else
    echo "FAIL: Ctrl+Alt+D did not fire the locked check."
    fail=1
fi

if grep -q '"msg":"the locked panel took focus and will stay open"' "$log"; then
    echo "PASS: the locked panel took focus."
else
    echo "FAIL: the locked panel never took focus, so it cannot be read or adjusted."
    fail=1
fi

# This is the assertion the whole script exists for. The panel now holds focus
# and nothing is moving. It must still be drawn. Before the fix it was not,
# and the only way to see it was to alt-tab to it.
hidden_before=$(grep -c '"msg":"the panel is not being drawn"' "$log")

sleep 8

hidden_after=$(grep -c '"msg":"the panel is not being drawn"' "$log")

flips=$((hidden_after - hidden_before))

if [ "$flips" -gt 0 ]; then
    echo "FAIL: the focused panel stopped being drawn $flips time(s) with nothing moving."
    echo "      This is what makes the overlay feel clunky and its buttons unclickable."
    grep '"msg":"the panel is not being drawn"' "$log" | tail -3
    fail=1
else
    echo "PASS: the focused panel stayed on screen."
fi

# Drawn is not the same as reachable. The probe measures the real window, so a
# panel that is drawn but sitting behind the game is caught here.
if grep '"msg":"the panel is where it should be"' "$log" | tail -1 | grep -q '"verdict":"Visible"'; then
    echo "PASS: the panel is above the game rather than behind it."
else
    echo "FAIL: the panel is drawn but not visible above the game."
    grep '"msg":"the panel is where it should be"' "$log" | tail -1
    fail=1
fi

# A locked panel that closes on its own is the other half of the contract: it
# stays until Escape, a click outside or Dismiss.
if grep '"msg":"the panel lifecycle moved"' "$log" | grep -q '"to":"Closed"'; then
    echo "FAIL: the locked panel closed on its own. It must stay until dismissed."
    fail=1
else
    echo "PASS: the locked panel stayed open."
fi

if [ "$fail" -eq 0 ]; then
    echo "PASS: a focused panel stays drawn, visible and open."
else
    echo
    echo "Logs: $log and $fake"
fi

exit "$fail"
