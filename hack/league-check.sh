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
# it. This asserts the three halves that matter:
#
#   - with no --league, the app asks the trade site which league is current
#   - the league it searches is the one the trade site named
#   - naming a league still pins it, and costs no request
#
# Two things stop it rotting when the league changes:
#
#   1. It never names a league. It fetches the list and picks from it with the
#      same rule as `core::controller::league_list::current`: the first league
#      that is not permanent, not hardcore and not private, falling back to the
#      first of any kind. Taking the first `"id"` in the JSON instead agrees
#      with the app only for as long as GGG keeps listing the temporary league
#      first, and disagrees loudly the day they do not.
#
#   2. It reads the endpoint out of the app's own log line rather than
#      hardcoding one. If the app ever asks a different URL, this follows.
#
# It also runs against its OWN config directory. `resolve_league` falls back to
# a remembered league from a previous session, so against the real config
# directory a completely broken fetch is masked by whatever the last good run
# wrote. A fresh directory has nothing to remember, so a broken fetch shows up
# as the Standard fallback, which is the bug.
#
# Needs a Windows host and the network. Run it from WSL after hack/deploy.sh.

set -uo pipefail

source "$(cd "$(dirname "$0")" && pwd)/harness.sh"

exe="${1:?usage: league-check.sh <exe>}"

dir="$(dirname "$exe")"
log="$dir/league-check.log"
pinned_log="$dir/league-check-pinned.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

# Relative on purpose. This is a Windows binary and $dir is a WSL path, so
# handing it "/mnt/c/..." makes Windows read the leading slash as the root of
# the current drive and quietly build C:\mnt\c\Users\... instead.
cfg="league-check-config"

# A fresh config directory is also a stale data cache, so the weekly refresh
# fires and pulls three tables per game before this measures anything. The
# throttle is judged on the mtime of `refreshed-at`, so an empty stamp with
# today's date makes the refresh skip while leaving the cache itself absent, and
# the run loads the built in copy. Six downloads saved per run and, more to the
# point, six fewer requests at GGG per invocation of check-all.sh.
fresh_config() {
    rm -rf "$cfg"
    mkdir -p "$cfg/data-poe1" "$cfg/data-poe2"

    : >"$cfg/data-poe1/refreshed-at"
    : >"$cfg/data-poe2/refreshed-at"
}

fresh_config

cleanup() {
    stop_overlays
    rm -rf "$cfg"
}

trap cleanup EXIT INT TERM

stop_overlays
sleep 2

fail=0

# The app's own rule for which league is current, in shell. Keep this in step
# with core::controller::league_list::current. A private league carries its code
# in brackets; a hardcore one is prefixed rather than flagged.
current_league() {
    local ids="$1" id

    while read -r id; do
        [ -n "$id" ] || continue

        case "$id" in
            Standard|Hardcore|"SSF Standard"|"SSF Hardcore") continue ;;
            "HC "*|"Hardcore "*) continue ;;
            *"("*")"*) continue ;;
        esac

        echo "$id"

        return 0
    done <<<"$ids"

    echo "$ids" | head -1
}

# The first launch is what tells us which URL the app itself uses for the
# league list, so the expected answer below is fetched from the same place
# rather than from a URL written here.
(timeout 90 "$exe" --config-dir "$cfg" --game poe2 --log-level debug >"$log" 2>&1 &)

if ! wait_for 60 "$log" '"msg":"searching this league"'; then
    echo "FAIL: the app never reported which league it searches."
    echo "      A tool that cannot say which market it priced against cannot be"
    echo "      argued with when the price is wrong."
    echo "Log: $log"
    exit 1
fi

if grep -q '"msg":"read the league list"' "$log"; then
    echo "PASS: with no --league the app asked the trade site which league is current."
else
    echo "FAIL: the app never read the league list, so its league is a guess."
    echo "      That guess was Standard, where junk sits unsold at absurd prices."
    fail=1
fi

url=$(field "$log" "read the league list" url)

if [ -z "$url" ]; then
    url="https://www.pathofexile.com/api/trade2/data/leagues"

    echo "note: the app logged no league list url, falling back to $url"
fi

leagues=$(curl -s --max-time 20 -H "accept: application/json" "$url")
ids=$(echo "$leagues" | grep -oE '"id":"[^"]+"' | cut -d'"' -f4)

if [ -z "$ids" ]; then
    echo "SKIP: the trade site did not answer with a league list, so nothing here"
    echo "      can be told apart from a network problem."
    exit 0
fi

expected=$(current_league "$ids")

echo "note: the trade site lists $(echo "$ids" | wc -l) leagues, current is $expected"

searching=$(field "$log" "searching this league" league)

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
    echo "      Standard listings never expire, so a levelling item prices at divines."
    fail=1
fi

# Whatever it settled on has to be a league that exists. A league the trade site
# has never heard of returns an empty search rather than an error, so it reads
# as "nothing is listed" rather than as a broken league.
if echo "$ids" | grep -qxF "${searching:-}"; then
    echo "PASS: $searching is a league the trade site actually lists."
else
    echo "FAIL: it is searching ${searching:-nothing}, which is not in the league list."
    echo "      That search comes back empty and reads as an item nobody is selling."
    fail=1
fi

assert_numbers_are_real "$log" || fail=1

stop_overlays
sleep 2

# Naming a league must still pin it. Detection that overrides the user is as
# wrong as a default that ignores them. Its own config directory again, so the
# remembered league from the run above cannot be what makes this pass.
fresh_config

(timeout 75 "$exe" --config-dir "$cfg" --game poe2 --league "Standard" --log-level debug >"$pinned_log" 2>&1 &)

if ! wait_for 50 "$pinned_log" '"msg":"searching this league"'; then
    echo "FAIL: the pinned run never reported a league."
    fail=1
fi

pinned=$(field "$pinned_log" "searching this league" league)

if [ "$pinned" = "Standard" ]; then
    echo "PASS: --league still pins the league."
else
    echo "FAIL: --league Standard was not honoured. It reports ${pinned:-no league at all}."
    echo "      Anyone playing Standard, or checking a Standard price on purpose,"
    echo "      silently gets league prices instead."
    fail=1
fi

if grep -q '"msg":"read the league list"' "$pinned_log"; then
    echo "FAIL: it fetched the league list even though a league was named."
    echo "      That is a request per launch against GGG for an answer already given."
    fail=1
else
    echo "PASS: a named league costs no request."
fi

if [ "$fail" -eq 0 ]; then
    echo
    echo "PASS: the league is detected when unset and pinned when set."
else
    echo
    echo "Logs: $log and $pinned_log"
fi

exit "$fail"
