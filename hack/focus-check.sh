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

source "$(cd "$(dirname "$0")" && pwd)/harness.sh"

exe="${1:?usage: focus-check.sh <exe> [item-file]}"
item="${2:-item.txt}"

game_window="Path of Exile 2"
items_dir="$(cd "$(dirname "$0")" && pwd)/items"
dir="$(dirname "$exe")"
log="$dir/focus-check.log"
fake="$dir/focus-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

arm_harness

[ -f "$items_dir/$item" ] && cp "$items_dir/$item" "$dir/$item"

if [ ! -f "$item" ]; then
    echo "FAIL: no item file at $dir/$item"
    exit 1
fi

powershell.exe -Command "Set-Clipboard -Value 'focus-check placeholder'" >/dev/null 2>&1

# The stand-in gets 200 seconds against a run that needs about 70. The old
# numbers were 110 and 110, which left no room: the eight second observation
# window below could land after the stand-in had gone, and a panel that closed
# because its game vanished proves nothing about focus. assert_stand_in_survived
# at the end is what turns a bad budget into a failure rather than a false pass.
(timeout 210 "$exe" --fake-game "$game_window" 200 "$item" >"$fake" 2>&1 &)

wait_for 20 "$fake" 'fakegame' || echo "note: the stand-in printed nothing yet, continuing"

(timeout 180 "$exe" --game poe2 --log-level debug >"$log" 2>&1 &)

# Waited for rather than slept through. `sleep 12` was either wasted time or,
# on a slow first run with a cold data cache, not enough, and the press then
# went to a process that was not reading the hotkey yet.
if ! wait_for 60 "$log" '"msg":"the frame loop is running'; then
    echo "FAIL: the overlay never started reading the hotkey."
    echo "      Nothing below was measured."
    echo "Logs: $log and $fake"
    exit 1
fi

fail=0

# Parked somewhere harmless. The pointer position does not decide anything here,
# because the locked binding takes focus without one, but leaving the cursor on
# top of the panel would let the ordinary hover rules move the lifecycle and
# muddy what the assertions below are measuring.
"$exe" --move-mouse 700 500 >/dev/null 2>&1
sleep 1

# --price-check-hotkey drives the pressing process, so this presses Ctrl+Alt+D,
# which the running overlay resolves as its LOCKED binding. That is the whole
# reproduction: a panel that takes focus on purpose, with no mouse involved.
if ! press_until "$exe" "$log" --game poe2 --price-check-hotkey "Ctrl+Alt+D"; then
    echo "FAIL: the locked press never reached the frame loop after three attempts."
    echo "      Ctrl+Alt+D is the binding both references ship. If it does not"
    echo "      arrive, the panel cannot be opened deliberately at all."
    echo "Logs: $log and $fake"
    exit 1
fi

if grep '"msg":"price check hotkey pressed"' "$log" | grep -q '"locked":true'; then
    echo "PASS: the locked binding fired rather than the plain one."
else
    echo "FAIL: Ctrl+Alt+D did not fire the locked check."
    echo "      It fired the ordinary one, which closes as soon as the pointer"
    echo "      moves, so the panel can never be read or clicked."
    fail=1
fi

if wait_for 20 "$log" '"msg":"the locked panel took focus and will stay open"'; then
    echo "PASS: the locked panel took focus."
else
    echo "FAIL: the locked panel never took focus, so it cannot be read or adjusted."
    echo "      Every button on it needs keyboard and mouse input to reach it."
    fail=1
fi

# This is the assertion the whole script exists for. The panel now holds focus
# and nothing is moving. It must still be drawn. Before the fix it was not, and
# the only way to see it was to alt-tab to it.
#
# Counted from the "took focus" line onwards rather than by sampling a count
# before and after a sleep. The oscillation starts on the very next frame after
# focus, so a "before" sample taken any time later than that already contains
# the first flip and the delta hides it.
sleep 8

flips=$(lines_after "$log" 'the locked panel took focus' \
    | grep -c '"msg":"the panel is not being drawn"')

if [ "${flips:-0}" -gt 0 ]; then
    echo "FAIL: the focused panel stopped being drawn ${flips} time(s) with nothing moving."
    echo "      This is what makes the overlay feel clunky and its buttons"
    echo "      unclickable: the window disappears out from under the click and"
    echo "      only comes back when you alt-tab."
    lines_after "$log" 'the locked panel took focus' \
        | grep '"msg":"the panel is not being drawn"' | tail -3
    fail=1
else
    echo "PASS: the focused panel stayed on screen."
fi

# The other direction of the same oscillation. A panel that hides and shows
# repeatedly ends up drawn, so counting only the hides at the end of the run
# would miss it. Every state change after focus is one flicker the user saw.
changes=$(lines_after "$log" 'the locked panel took focus' \
    | grep -cE '"msg":"the panel is (on screen|not being drawn)"')

if [ "${changes:-0}" -gt 0 ]; then
    echo "note: the panel changed visibility ${changes} time(s) after taking focus."
fi

# Drawn is not the same as reachable. The probe measures the real window, so a
# panel that is drawn but sitting behind the game is caught here.
if grep '"msg":"the panel is where it should be"' "$log" | tail -1 | grep -q '"verdict":"Visible"'; then
    echo "PASS: the panel is above the game rather than behind it."
elif grep -q '"msg":"the panel is where it should be"' "$log"; then
    echo "FAIL: the panel is drawn but not visible above the game."
    echo "      The user reads their own game through it and thinks nothing happened."
    grep '"msg":"the panel is where it should be"' "$log" | tail -1
    fail=1
else
    echo "FAIL: the panel window was never measured, so 'it is drawn' is unproven."
    fail=1
fi

# A locked panel that closes on its own is the other half of the contract: it
# stays until Escape, a click outside or Dismiss. Scoped to after it took focus,
# because an earlier lifecycle move belongs to a previous state.
if lines_after "$log" 'the locked panel took focus' \
    | grep '"msg":"the panel lifecycle moved"' | grep -q '"to":"Closed"'; then
    echo "FAIL: the locked panel closed on its own. It must stay until dismissed."
    echo "      Ctrl+Alt+D then becomes a key that flashes something and gives up."
    fail=1
else
    echo "PASS: the locked panel stayed open."
fi

assert_numbers_are_real "$log" || fail=1

# Last, because it explains away every failure above when it fires.
assert_stand_in_survived "$log" || fail=1

if [ "$fail" -eq 0 ]; then
    echo
    echo "PASS: a focused panel stays drawn, visible and open."
else
    echo
    echo "Logs: $log and $fake"
fi

exit "$fail"
