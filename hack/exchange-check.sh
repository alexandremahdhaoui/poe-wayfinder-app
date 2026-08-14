#!/usr/bin/env bash
#
# A bulk exchange has to name what it wants to be paid in.
#
# An Orb of Augmentation priced at "~99 waystone-3".
#
# Currency does not go to the search endpoint, it goes to the bulk exchange.
# The exchange request has two halves: `want`, the thing you are selling, and
# `have`, the currencies you will accept for it. We sent `have` empty. An empty
# `have` does not mean "the usual currency", it means "price this in anything
# at all", so the exchange happily answered in tier 3 waystones. The number on
# the panel was real and completely useless.
#
# `poe_wayfinder_core::controller::bulk::currencies_to_price_in` fixed it: it
# fills `have` with the standard currencies of the game, minus the one being
# priced, because nothing is priced in itself.
#
# There were unit tests for that function on the day it was written and no
# harness that ran a currency item through the real exchange and looked at what
# went out. press-check.sh runs item-currency.txt but skips the filter row
# assertion for it and never looks at the request. So this exists.
#
# The expected `have` list is derived here the same way the Rust derives it,
# from the trade tag the run itself logged. Hardcoding "exalted" would pass for
# a Divine Orb and lie about every other currency.
#
# Needs a Windows host and the network. Run it from WSL after hack/deploy.sh.

set -uo pipefail

source "$(cd "$(dirname "$0")" && pwd)/harness.sh"

exe="${1:?usage: exchange-check.sh <exe> [item-file] [game]}"
item="${2:-item-currency.txt}"
game="${3:-poe2}"

case "$game" in
    poe1) game_window="Path of Exile" ;;
    *)    game_window="Path of Exile 2" ;;
esac

items_dir="$(cd "$(dirname "$0")" && pwd)/items"
dir="$(dirname "$exe")"
log="$dir/exchange-check.log"
fake="$dir/exchange-check-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

arm_harness

[ -f "$items_dir/$item" ] && cp "$items_dir/$item" "$dir/$item"

if [ ! -f "$item" ]; then
    echo "FAIL: no item file at $dir/$item"
    exit 1
fi

# The overlay waits for the clipboard to CHANGE, so it has to start as
# something else or the copy cannot be told apart from what was already there.
powershell.exe -Command "Set-Clipboard -Value 'exchange-check placeholder'" >/dev/null 2>&1

# 200 seconds of stand-in against a run that needs about 80. The stand-in has to
# outlive every assertion below, because a panel that closed because its game
# vanished says nothing about the overlay.
(timeout 210 "$exe" --fake-game "$game_window" 200 "$item" >"$fake" 2>&1 &)

wait_for 20 "$fake" 'fakegame' || echo "note: the stand-in printed nothing yet, continuing"

(timeout 180 "$exe" --game "$game" --log-level debug >"$log" 2>&1 &)

if ! wait_for 40 "$log" '"msg":"the frame loop is running'; then
    echo "FAIL: the overlay never got its frame loop running, so nothing below was measured."
    exit 1
fi

fail=0

if ! press_until "$exe" "$log" --game "$game"; then
    echo "FAIL: the press never reached the frame loop after three attempts."
    echo "      Nothing about the exchange request was measured."
    exit 1
fi

wait_for_check_to_settle 60 "$log"

# The request line is debug and carries the whole outgoing shape. Without it
# there is no way to tell a correct exchange from a wrong one after the fact,
# which is exactly why this bug lived in a released build.
if ! grep -q '"msg":"the request being sent"' "$log"; then
    echo "FAIL: nothing logged what was actually sent to the trade site."
    echo "      A wrong price can then only be diagnosed by reproducing it live."
    echo "Logs: $log and $fake"
    exit 1
fi

endpoint=$(field "$log" "the request being sent" endpoint)
tag=$(field "$log" "the request being sent" trade_tag)
priced_in=$(field "$log" "the request being sent" priced_in)

echo "note: endpoint=$endpoint trade_tag=$tag priced_in=$priced_in"

# A currency item that goes to the search endpoint is a different bug, but it
# also means nothing below is testing the exchange at all, so say so plainly.
if [ "$endpoint" = "Exchange" ]; then
    echo "PASS: the currency went to the bulk exchange."
else
    echo "FAIL: a currency item was routed to $endpoint rather than the exchange."
    echo "      Bulk currency priced through the search endpoint is priced against"
    echo "      single listings rather than the bulk market."
    fail=1
fi

if [ "$tag" = "none" ] || [ -z "$tag" ]; then
    echo "FAIL: the item reached the exchange with no trade tag."
    echo "      Without a tag the exchange has no idea what is being sold."
    fail=1
else
    echo "PASS: the exchange knows it is selling $tag."
fi

# THE assertion. "anything the seller offers" is what the app prints when the
# have list is empty, which is the released bug exactly.
if [ "$priced_in" = "anything the seller offers" ]; then
    echo "FAIL: the exchange asked to be paid in anything at all."
    echo "      This is the shipped bug: an Orb of Augmentation came back as"
    echo "      \"~99 waystone-3\" and the user has no way to know the number is junk."
    fail=1
elif [ -z "$priced_in" ]; then
    echo "FAIL: the request carried no priced_in field at all."
    fail=1
else
    echo "PASS: the exchange asked to be paid in $priced_in."
fi

# Derived rather than hardcoded, the same way currencies_to_price_in derives it:
# the game's standard currencies, minus the one being priced. A hardcoded
# "exalted" passes for a Divine Orb and says nothing about an Exalted Orb.
case "$game" in
    poe1) standard="chaos divine" ;;
    *)    standard="exalted divine" ;;
esac

expected=""

for currency in $standard; do
    [ "$currency" = "$tag" ] && continue

    expected="${expected:+$expected,}$currency"
done

if [ "$priced_in" = "$expected" ]; then
    echo "PASS: it asked for exactly the standard currencies of $game, minus $tag."
else
    echo "FAIL: it asked to be paid in \"$priced_in\" where \"$expected\" is right for $game."
    echo "      Anything outside the standard currencies puts the price in a unit"
    echo "      the user cannot compare against anything else."
    fail=1
fi

# Nothing is ever priced in itself. That request comes back empty every time and
# the user sees no price at all.
if echo ",$priced_in," | grep -q ",$tag,"; then
    echo "FAIL: it asked to be paid in $tag for $tag. That exchange has no offers."
    fail=1
fi

# The other end of the same bug. Whatever came back has to be a currency with a
# name, never a raw trade id with a tier number stuck on it.
assert_currency_is_named "$log" || fail=1

currency=$(field "$log" "the estimate behind the headline" currency)

if [ -z "$currency" ]; then
    echo "note: no estimate formed this run, so only the outgoing request was proved."
elif echo ",$priced_in," | grep -q ",$currency,"; then
    echo "PASS: the price came back in $currency, which is one of the currencies asked for."
else
    echo "FAIL: it asked to be paid in $priced_in and was answered in $currency."
    echo "      A price in a currency nobody asked for is the waystone bug again."
    fail=1
fi

assert_numbers_are_real "$log" || fail=1
assert_stand_in_survived "$log" || fail=1

if [ "$fail" -eq 0 ]; then
    echo
    echo "PASS: the exchange named what it wants to be paid in and was paid in it."
else
    echo
    echo "Logs: $log and $fake"
fi

exit "$fail"
