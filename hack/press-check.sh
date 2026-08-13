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
data="${2:-}"
game="${3:-poe2}"
item="${4:-item.txt}"

# An empty data dir is the normal case now. The exe carries both games inside
# it, so the flag is only for testing a freshly generated directory. Passing
# "" here is what proves the shipped default works.
data_flag=()
[ -n "$data" ] && data_flag=(--data-dir "$data")

# The game stays pinned here on purpose. Detection is exercised by
# both-games-check.sh, which is the only place two game windows exist at once.
# Pinning keeps this script measuring the price check and nothing else.
#
# The stand-in must carry the title THIS game uses. The overlay derives its
# window title from the game now, so a PoE1 run against a window titled
# "Path of Exile 2" finds nothing and the press goes nowhere. It used to work
# only because the title was a fixed default that ignored --game.
case "$game" in
    poe1) game_window="Path of Exile" ;;
    *)    game_window="Path of Exile 2" ;;
esac

items_dir="$(cd "$(dirname "$0")" && pwd)/items"
dir="$(dirname "$exe")"
log="$dir/press-check.log"
fake="$dir/press-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

# A leftover overlay owns the hotkey registration and answers the press itself,
# which reads as this run passing when it never started. Cost an hour once.
# Kill the overlay on the way out as well as on the way in.
#
# `timeout` no longer stops it. The exe is windows subsystem now, so launched
# from WSL there is no parent console to attach to, the process detaches from
# the interop proxy, and timeout kills the proxy while the Windows process runs
# on forever. One orphan reached 26900 frames against a 120 second timeout.
#
# It matters more than a stray process: a leftover overlay owns the hotkey
# registration and answers the next run's press itself, which reads as that run
# passing when it never started.
stop_overlays() {
    powershell.exe -Command "Get-Process poe-wayfinder* -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null 2>&1
}

trap stop_overlays EXIT INT TERM

stop_overlays
sleep 2

# The item files live in the repo so a fresh machine can run this. They are
# copied next to the exe because --fake-game reads them from its own directory.
if [ -f "$items_dir/$item" ]; then
    cp "$items_dir/$item" "$dir/$item"
fi

if [ ! -f "$item" ]; then
    echo "FAIL: no item file at $dir/$item"
    exit 1
fi

# The overlay waits for the clipboard to *change*, so it has to start as
# something else or the copy cannot be told apart from what was already there.
powershell.exe -Command "Set-Clipboard -Value 'press-check placeholder'" >/dev/null 2>&1

(timeout 130 "$exe" --fake-game "$game_window" 120 "$item" >"$fake" 2>&1 &)
sleep 5

# Debug, so the heartbeat is in the log. It is how a stopped loop is caught.
(timeout 120 "$exe" "${data_flag[@]}" --game "$game" --log-level debug >"$log" 2>&1 &)
sleep 12

# Parked somewhere definite before the press, so the pointer check later has a
# real distance to travel. Leaving it wherever it happened to be meant the
# check once compared a position against itself and read as a failure.
"$exe" --move-mouse 1400 900 >/dev/null 2>&1

pressed=0

for attempt in 1 2 3; do
    "$exe" "${data_flag[@]}" --game "$game" --press-hotkey || {
        echo "FAIL: the keys were not accepted."
        exit 1
    }

    # Injected input is occasionally swallowed before any window sees it, which
    # looks exactly like a broken hotkey. Retrying separates the two.
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

# The search is paced by the rate limiter, which is not optional. Waited for
# rather than slept through, so the pointer check below still happens while the
# game window exists. Sleeping a fixed 45s outlived the stand in and made the
# panel close for the wrong reason.
for _ in $(seq 1 45); do
    grep -q '"msg":"price check finished"' "$log" && break
    grep -q '"msg":"price check did not produce a price"' "$log" && break
    sleep 1
done

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
check "$log" '"msg":"the panel is up"' "the panel never appeared."
check "$log" '"msg":"price check finished"' "the price check never finished."

# The whole point of splitting the price check: the panel must appear on the
# item and its filters, not wait behind a network round trip. Both lines carry
# elapsed_ms, so this is measured rather than felt.
panel_at=$(grep '"msg":"the panel is up"' "$log" | grep -oE '"elapsed_ms":[0-9]+' | head -1 | cut -d: -f2)
pressed_at=$(grep '"msg":"price check hotkey pressed"' "$log" | grep -oE '"elapsed_ms":[0-9]+' | head -1 | cut -d: -f2)
done_at=$(grep '"msg":"price check finished"' "$log" | grep -oE '"elapsed_ms":[0-9]+' | head -1 | cut -d: -f2)

if [ -n "$panel_at" ] && [ -n "$pressed_at" ]; then
    to_panel=$((panel_at - pressed_at))

    if [ "$to_panel" -le 1200 ]; then
        echo "PASS: the panel was up ${to_panel}ms after the press."
    else
        echo "FAIL: the panel took ${to_panel}ms to appear. It must not wait on the search."
        fail=1
    fi

    if [ -n "$done_at" ]; then
        echo "note: the price landed $((done_at - pressed_at))ms after the press, off the critical path."
    fi
else
    echo "FAIL: no elapsed_ms on the press or panel lines, so the delay cannot be measured."
    fail=1
fi

# With no --data-dir the exe must find its own data. A run that quietly needed
# a folder beside it is the thing this whole change was about.
# Either origin is correct with no flag. "embedded" is a fresh machine and
# "cache" is one the weekly refresh has already served. "directory" would mean
# it silently read a folder nobody asked for.
if [ -z "$data" ]; then
    if grep '"msg":"loaded the game data"' "$log" | grep -qE '"origin":"(embedded|cache)"'; then
        echo "PASS: the exe ran with no data flag and found its own data."
    else
        echo "FAIL: with no --data-dir the exe did not find its own data."
        grep '"msg":"loaded the game data"' "$log" | tail -2
        fail=1
    fi

    if grep '"msg":"loaded the game data"' "$log" | grep -q '"origin":"directory"'; then
        echo "FAIL: with no --data-dir the exe read a directory anyway."
        fail=1
    fi
fi

# One press, one check. Both watchers see the same press and they land a frame
# apart, so this is the only thing that catches a broken guard.
presses=$(grep -c '"msg":"price check hotkey pressed"' "$log")

if [ "$presses" -ne 1 ]; then
    echo "FAIL: one press produced $presses price checks. Each one is a request against the rate limit."
    fail=1
fi

# The panel is editable now. A finished check with no rows on it is a panel the
# user cannot adjust, which is the whole point of the filter block. Currency
# goes through the exchange endpoint and has no stats, so only rare items are
# held to this.
if [ "$item" != "item-currency.txt" ]; then
    if grep '"msg":"price check finished"' "$log" | grep -qE '"stat_rows":[1-9]'; then
        echo "PASS: the filter rows reached the panel."
    else
        echo "FAIL: the panel finished with no editable filter rows."
        grep '"msg":"price check finished"' "$log" | tail -1
        fail=1
    fi
fi

# An item with an empty rune socket must be offered runes to socket. That is the
# item editor, and it is the one part of the panel that changes the search rather
# than only narrowing it.
if [ "$item" = "item-runable.txt" ]; then
    if grep '"msg":"price check finished"' "$log" | grep -qE '"augments":[1-9]'; then
        echo "PASS: the item editor offered augments that fit the item."
    else
        echo "FAIL: an item with empty rune sockets was offered no augments."
        grep '"msg":"price check finished"' "$log" | tail -1
        fail=1
    fi
fi

# A search that matched something must come back with the listings behind it,
# because the suggested price is computed from them and nothing else.
if grep '"msg":"price check finished"' "$log" | grep -qE '"listings":[1-9]'; then
    if grep '"msg":"read the listings"' "$log" | grep -qE '"listings":[1-9]'; then
        echo "PASS: the listings behind the count were read."
    else
        echo "FAIL: the search matched items but no listing was read, so no price is offered."
        grep '"msg":"read the listings"' "$log" | tail -1
        fail=1
    fi
fi

# A price that cannot be read from the log cannot be argued about. "1 to 10
# div for a worthless item" took a live session to find because the finished
# line carried listings and quotes but never the number the user was shown.
if grep '"msg":"price check finished"' "$log" | grep -q '"price":'; then
    echo "note: it priced the item at $(grep '"msg":"price check finished"' "$log" |
        tail -1 | grep -oE '"price":"[^"]*"' | cut -d'"' -f4)"
else
    echo "FAIL: the finished line carries no price, so a wrong one cannot be diagnosed."
    fail=1
fi

# A refused query still "finishes". The error line is the only thing that
# separates a real answer from one the trade api threw out.
if grep -q '"msg":"searching the trade site"' "$log"; then
    echo "FAIL: the trade api refused the search:"
    grep '"msg":"searching the trade site"' "$log" | tail -1
    fail=1
fi

# The panel must go away when the user looks elsewhere. It is the rule that
# makes the overlay usable rather than a window stuck over the game, and no
# unit test can see it because it needs a real pointer.
# The panel must go away when the user looks elsewhere. It is the rule that
# makes the overlay usable rather than a window stuck over the game, and no
# unit test can see it because it needs a real pointer.
#
# Asserted on the lifecycle's own transition rather than on whether the panel
# is drawn. The window also stops being drawn when the game loses focus, and
# counting that passed once for the wrong reason.
if grep -q '"msg":"price check hotkey pressed"' "$log"; then
    "$exe" --move-mouse 30 30 >/dev/null 2>&1
    sleep 3

    if grep '"msg":"the panel lifecycle moved"' "$log" | grep -q '"to":"Closed"'; then
        echo "PASS: the panel closed when the pointer moved away."
    else
        echo "FAIL: the panel never closed after the pointer moved away."
        grep '"msg":"the panel lifecycle moved"' "$log" | tail -3
        fail=1
    fi
fi

if [ "$fail" -eq 0 ]; then
    echo "PASS: press, copy, parse, filter and search all ran."
else
    echo
    echo "Logs: $log and $fake"
fi

exit "$fail"
