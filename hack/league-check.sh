#!/usr/bin/env bash
#
# The league must be the one being played, not the one in a default.
#
# The tool priced a worthless levelling sceptre at "1 to 10 div". The search
# was correct, the filters were correct and the listings were real. They were
# real STANDARD listings, where nobody delists and junk sits at absurd prices
# forever.
#
# `league` defaulted to "Standard" and the only thing that could change it was
# `league_from_whisper`, which reads a league out of a trade whisper in
# Client.txt. A player who has not been whispered a trade has no such line, so
# the default stood and nothing in any log said it was a guess.
#
# Nothing caught it because every harness pins the league or does not look at
# it. This asserts the two halves that matter:
#
#   - with no --league, the app asks the trade site which league is current
#   - the league it searches is the one the trade site named
#
# It fetches the league list itself and compares, so it keeps working when the
# league changes rather than hardcoding a name that rots in three months.
#
# Needs a Windows host and the network. Run it from WSL after hack/deploy.sh.

set -uo pipefail

exe="${1:?usage: league-check.sh <exe>}"

dir="$(dirname "$exe")"
log="$dir/league-check.log"
pinned_log="$dir/league-check-pinned.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

stop_overlays() {
    powershell.exe -Command "Get-Process poe-wayfinder* -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null 2>&1
}

trap stop_overlays EXIT INT TERM

stop_overlays
sleep 2

fail=0

# The expected answer comes from the same endpoint the app uses, so this does
# not hardcode a league name.
leagues=$(curl -s --max-time 20 \
    -H "accept: application/json" \
    "https://www.pathofexile.com/api/trade2/data/leagues")

expected=$(echo "$leagues" | grep -oE '"id":"[^"]+"' | head -1 | cut -d'"' -f4)

if [ -z "$expected" ]; then
    echo "SKIP: the trade site did not answer with a league list."
    exit 0
fi

echo "note: the trade site says the current league is $expected"

(timeout 60 "$exe" --game poe2 --log-level debug >"$log" 2>&1 &)
sleep 20

if grep -q '"msg":"read the league list"' "$log"; then
    echo "PASS: with no --league the app asked the trade site which league is current."
else
    echo "FAIL: the app never read the league list, so its league is a guess."
    fail=1
fi

searching=$(grep '"msg":"searching this league"' "$log" | tail -1 |
    grep -oE '"league":"[^"]*"' | cut -d'"' -f4)

if [ "$searching" = "$expected" ]; then
    echo "PASS: it is searching $searching."
else
    echo "FAIL: it reports ${searching:-no league at all} while the current league is $expected."
    echo "      Every price will come from a market the item is not in."
    fail=1
fi

# Standard is a real answer when the trade site gives it, and a wrong one when
# it is a leftover default. The two are told apart by whether it was asked for.
if [ "$searching" = "Standard" ] && [ "$expected" != "Standard" ]; then
    echo "FAIL: it fell back to Standard, which is the bug this check exists for."
    fail=1
fi

stop_overlays
sleep 2

# Naming a league must still pin it. Detection that overrides the user is as
# wrong as a default that ignores them.
(timeout 45 "$exe" --game poe2 --league "Standard" --log-level debug >"$pinned_log" 2>&1 &)
sleep 18

pinned=$(grep '"msg":"searching this league"' "$pinned_log" | tail -1 |
    grep -oE '"league":"[^"]*"' | cut -d'"' -f4)

if [ "$pinned" = "Standard" ]; then
    echo "PASS: --league still pins the league."
else
    echo "FAIL: --league Standard was not honoured. It reports ${pinned:-no league at all}."
    fail=1
fi

if grep -q '"msg":"read the league list"' "$pinned_log"; then
    echo "FAIL: it fetched the league list even though a league was named."
    fail=1
else
    echo "PASS: a named league costs no request."
fi

if [ "$fail" -eq 0 ]; then
    echo "PASS: the league is detected when unset and pinned when set."
else
    echo
    echo "Logs: $log and $pinned_log"
fi

exit "$fail"
