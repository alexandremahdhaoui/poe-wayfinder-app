#!/usr/bin/env bash
#
# One exe, no flags, both games.
#
# This is the only thing that proves the three claims that cannot be unit
# tested, because each needs a real window and a real desktop:
#
#   1. the exe starts with NO arguments at all and finds its own game data
#   2. it works out which game is running from the open window titles
#   3. when the other game comes to the front it follows, and the follow
#      reaches the trade api and not only the log line
#
# Claim 3 is the one worth the script. Flipping a GameVersion field while the
# price controller keeps its old TradeUrls looks completely correct in a log
# and searches the wrong API. That bug was live for `league` until this work,
# so the URL is asserted rather than the message.
#
# Needs a Windows host. Run it from WSL after `hack/deploy.sh`.

set -uo pipefail

exe="${1:?usage: both-games-check.sh <exe>}"

dir="$(dirname "$exe")"
items_dir="$(cd "$(dirname "$0")" && pwd)/items"
log="$dir/both-games.log"
fake2="$dir/both-games-poe2.log"
fake1="$dir/both-games-poe1.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

fail=0

check() {
    if grep -q "$2" "$1"; then
        [ -n "${3:-}" ] && echo "PASS: $3"
        return 0
    fi

    echo "FAIL: ${4:-$3}"
    fail=1
}

# A leftover overlay owns the hotkey registration and answers the press itself,
# which reads as this run passing when it never started.
powershell.exe -Command "Get-Process poe-trader* -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null 2>&1
sleep 2

for item in item.txt item-poe1.txt; do
    [ -f "$items_dir/$item" ] && cp "$items_dir/$item" "$dir/$item"
done

# The two stand-ins. One window per game, both open at once, which is the only
# state that tells foreground detection apart from "pick whatever is open".
(timeout 300 "$exe" --fake-game "Path of Exile 2" 290 item.txt      >"$fake2" 2>&1 &)
sleep 2
(timeout 300 "$exe" --fake-game "Path of Exile"   290 item-poe1.txt >"$fake1" 2>&1 &)
sleep 3

# No --data-dir, no --game, no --window-title. That is the whole point.
(timeout 280 "$exe" --log-level debug >"$log" 2>&1 &)
sleep 12

check "$log" '"msg":"loaded the game data"' \
    "the exe started with no arguments at all." \
    "the exe did not get as far as loading data. It still needs a flag."

# embedded on a fresh machine, cache once the weekly refresh has run. Both
# mean the exe found its own data. "directory" would mean it read a folder.
if grep '"msg":"loaded the game data"' "$log" | grep -qE '"origin":"(embedded|cache)"'; then
    echo "PASS: the exe found its own game data."
else
    echo "FAIL: the exe did not find its own game data."
    fail=1
fi

if grep -q 'MissingRequired\|loading config' "$log"; then
    echo "FAIL: config loading refused the empty command line:"
    grep 'MissingRequired\|loading config' "$log" | tail -1
    fail=1
fi

# Both games' tables must be in memory, or a switch has nothing to switch to.
loaded=$(grep -c '"msg":"loaded the game data"' "$log")

if [ "$loaded" -ne 2 ]; then
    echo "FAIL: $loaded game tables loaded. Both are needed to follow a switch."
    fail=1
else
    echo "PASS: both games' tables are held at once."
fi

# Exact title match. PowerShell's AppActivate matches by PREFIX, so asking it
# for "Path of Exile" raises the PoE2 window and the whole test reads as a
# detection failure that is really a harness failure.
#
# Raised repeatedly rather than once. The raising process exits immediately and
# the foreground goes back to whatever had it, so a single raise can be gone
# again before the overlay's once a second poll looks. That is a property of
# this harness, not of the overlay: a real player leaves the game in front.
raise_until() {
    local title="$1"
    local want="$2"

    for _ in $(seq 1 15); do
        "$exe" --focus-window "$title" >/dev/null 2>&1

        sleep 1

        if grep '"msg":"the game changed"' "$log" | tail -1 | grep -q "\"to\":\"$want\""; then
            return 0
        fi
    done

    return 1
}

# Raise PoE1. Detection must follow the foreground window, not the open list.
if raise_until "Path of Exile" poe1; then
    echo "PASS: the overlay followed the foreground window to PoE1."
else
    echo "FAIL: PoE1 came to the front and the overlay did not follow."
    grep '"msg":"the game changed"' "$log" | tail -3
    fail=1
fi

check "$log" '"window_title":"Path of Exile"' \
    "the overlay retargeted its window to the PoE1 title." \
    "the overlay still watches the PoE2 window after switching to PoE1."

# The press proves the switch reached the parser. A PoE1 item parsed with PoE2
# tables produces no filter rows.
"$exe" --move-mouse 1400 900 >/dev/null 2>&1
sleep 1

for attempt in 1 2 3; do
    "$exe" --press-hotkey >/dev/null 2>&1

    for _ in 1 2 3 4 5 6; do
        sleep 1
        grep -q '"msg":"price check hotkey pressed"' "$log" && break 2
    done

    echo "note: press $attempt did not land, retrying"
done

for _ in $(seq 1 45); do
    grep -q '"msg":"price check finished"' "$log" && break
    grep -q '"msg":"price check did not produce a price"' "$log" && break
    sleep 1
done

# Only the checks that happened AFTER the switch say anything about the
# parser. Counting every finished check let a PoE2 press pass as proof that
# PoE1 parsing worked.
after=$(awk '/the game changed/{seen=1} seen' "$log" | grep '"msg":"price check finished"' | tail -1)

if echo "$after" | grep -qE '"stat_rows":[1-9]'; then
    echo "PASS: a PoE1 item parsed against the PoE1 tables after the switch."
else
    echo "FAIL: after switching to PoE1 the item produced no filter rows, so the parser did not follow."
    echo "  ${after:-no price check ran after the switch}"
    fail=1
fi

# The assertion that matters most. The searched URL must be the PoE1 one.
if grep -q '"msg":"searching the trade site"' "$log"; then
    echo "note: the trade api refused a search, so the url is read from the request line instead"
fi

after_url=$(awk '/the game changed/{seen=1} seen' "$log" | grep -oE '"url":"[^"]*"' | tail -1)

if echo "$after_url" | grep -qE '/api/trade/'; then
    echo "PASS: the search went to the PoE1 api."
elif echo "$after_url" | grep -qE '/api/trade2/'; then
    echo "FAIL: after switching to PoE1 the search still went to /api/trade2/."
    grep -oE '"url":"[^"]*"' "$log" | tail -3
    fail=1
else
    echo "note: no search url was logged, so the api switch is unproven in this run"
fi

# And back again.
if raise_until "Path of Exile 2" poe2; then
    echo "PASS: the overlay followed back to PoE2."
else
    echo "FAIL: PoE2 came to the front and the overlay did not follow back."
    grep '"msg":"the game changed"' "$log" | tail -3
    fail=1
fi

# One press, one check, across a switch as well.
presses=$(grep -c '"msg":"price check hotkey pressed"' "$log")
checks=$(grep -c '"msg":"price check finished"' "$log")

if [ "$checks" -gt "$presses" ]; then
    echo "FAIL: $presses presses produced $checks checks. Each extra one costs a request against the rate limit."
    fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo
    echo "PASS: one exe, no flags, both games."
else
    echo
    echo "Logs: $log, $fake2 and $fake1"
fi

exit "$fail"
