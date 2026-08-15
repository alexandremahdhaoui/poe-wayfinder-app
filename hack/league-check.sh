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
# It never names a league, so it does not rot when the league changes.
#
# IT ALSO NEVER TALKS TO GGG ITSELF. It used to `curl` the league endpoint in
# parallel with the app's own fetch of the same endpoint, which is a second
# unthrottled request to GGG on every run, outside `http_adapter.rs` and outside
# the rate limiter. The rate limiter is not optional and GGG bans for
# violations, so a harness that doubles the request count of the thing it is
# measuring is not worth any assertion it buys.
#
# What replaces it is the app's own `read the league list` line, which already
# reports the league the list resolved to, and the `source` field on
# `searching this league`, which says where the league came from: trade api,
# configured, last run or fallback. Together those pin down every failure the
# curl was there to catch:
#
#   - a fetch that failed reads as source "fallback" or "last run"
#   - a fetch that succeeded and was then ignored shows the two lines disagree
#   - the Standard default shows as Standard with a source that is not the
#     trade api
#
# WHAT IS GIVEN UP: the league list itself is no longer read from an
# independent source, so this can no longer prove the app picked the RIGHT
# entry out of GGG's list, only that it used the entry it read. The shape of
# the answer is still checked below, because a "current" league that is
# Standard, hardcore or private means `league_list::current` picked wrong.
# `poe-wayfinder-core` unit tests own the rest of that rule.
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

# Does this name look like a league somebody is actually playing in?
#
# Not a reimplementation of `league_list::current` any more, because there is no
# independent list to run it over. It only judges the ANSWER the app reported.
# `current` coming back as Standard, as a hardcore league or as a private one
# means the selection rule picked wrong, and that is visible without asking GGG
# anything. A private league carries its code in brackets; a hardcore one is
# prefixed rather than flagged.
looks_temporary() {
    case "$1" in
        ""|Standard|Hardcore|"SSF Standard"|"SSF Hardcore") return 1 ;;
        "HC "*|"Hardcore "*|"SSF "*) return 1 ;;
        *Ruthless*) return 1 ;;
        *"("*")"*) return 1 ;;
    esac

    return 0
}

# One launch, one league list request, and that request is the app's own,
# through http_adapter.rs and through the rate limiter.
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

# A fetch that the trade site refused, or that never got out, leaves the app
# guessing. It says so on its own, and a run that ignores those lines reads a
# guess as an answer.
#
# Written out one grep per message rather than looped over a list, because
# harness-lint.sh scans this file for `"msg":"..."` literals and checks the Rust
# still logs each one. A message built up in a shell variable is invisible to
# that scan, so it would rot the moment somebody reworded the Rust.
check_absent() {
    if grep -q "$1" "$log"; then
        echo "FAIL: the app could not get a league and said so."
        grep "$1" "$log" | tail -1
        echo "      Whatever it searched after that is a guess, and the guess is"
        echo "      Standard, where junk sits unsold at absurd prices."
        fail=1
    fi
}

check_absent '"msg":"could not read the league list, so the league falls back"'
check_absent '"msg":"the trade api refused the league list, so the league falls back"'
check_absent '"msg":"nothing named a league, so the search falls back to Standard"'

# What the list resolved to, reported by the app rather than fetched a second
# time. "none" is the app saying the list held nothing it could use.
listed=$(field "$log" "read the league list" current)
url=$(field "$log" "read the league list" url)

echo "note: the app read the league list from ${url:-nowhere it logged} and picked ${listed:-nothing}"

if [ -z "$listed" ] || [ "$listed" = "none" ]; then
    echo "FAIL: the league list gave the app nothing to use."
    echo "      It then searches whatever is left, which is Standard."
    fail=1
elif looks_temporary "$listed"; then
    echo "PASS: the list resolved to $listed, which is shaped like a league being played."
else
    echo "FAIL: the list resolved to $listed, which is permanent, hardcore or private."
    echo "      core::controller::league_list::current picked the wrong entry, so"
    echo "      every price comes from a market almost nobody is trading in."
    fail=1
fi

searching=$(field "$log" "searching this league" league)
source_of=$(field "$log" "searching this league" source)

# THE self consistency assertion. The app read a league and then has to search
# that one. A fetch that succeeds and is then thrown away for a remembered or
# default value is exactly the shipped bug, and it is visible without asking
# GGG a second time.
if [ -n "$listed" ] && [ "$listed" != "none" ] && [ "$searching" = "$listed" ]; then
    echo "PASS: it is searching $searching, the league its own list lookup returned."
else
    echo "FAIL: it read $listed from the league list and searches ${searching:-no league at all}."
    echo "      Reading the right answer and then not using it prices every item"
    echo "      against a market it is not in."
    fail=1
fi

# Where the league came from, from the app's own field. "trade api" is the only
# correct answer with no --league: "last run" means a remembered value masked
# the fetch, "fallback" is the Standard default, and "configured" with an empty
# --league would mean the empty string won.
if [ "$source_of" = "trade api" ]; then
    echo "PASS: the league came from the trade site rather than from a default."
else
    echo "FAIL: with no --league the league came from \"${source_of:-nothing logged}\"."
    echo "      Only \"trade api\" means it was asked for. The config directory is"
    echo "      fresh, so there is nothing legitimate to remember."
    fail=1
fi

# Standard is a real answer when the trade site gives it, and a wrong one when
# it is a leftover default. The two are told apart by whether it was asked for.
if [ "$searching" = "Standard" ] && [ "$listed" != "Standard" ]; then
    echo "FAIL: it fell back to Standard, which is the bug this check exists for."
    echo "      Standard listings never expire, so a levelling item prices at divines."
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
pinned_source=$(field "$pinned_log" "searching this league" source)

# The right name reached by the wrong route is still a bug. "Standard" is also
# what the fallback produces, so without the source field a completely broken
# pin passes this by accident.
if [ "$pinned_source" = "configured" ]; then
    echo "PASS: the pinned league came from the flag rather than from a fallback."
else
    echo "FAIL: --league Standard was honoured by name but the source is"
    echo "      \"${pinned_source:-nothing logged}\". Standard is also what the"
    echo "      fallback produces, so this run proves nothing about pinning."
    fail=1
fi

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
