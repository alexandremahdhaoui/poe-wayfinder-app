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
# WHAT THIS ASSERTS ON, AND WHY IT CHANGED.
#
# The first version of this script read `priced_in` off the request log line and
# compared it against a shell reimplementation of the same rule. That proved
# nothing. The driver PRODUCES `priced_in` by calling
# `bulk::currencies_to_price_in`, which is the same function the request builder
# calls separately. Delete the `have` argument from `to_exchange_json` entirely
# and every one of those assertions still passes, because both sides of the
# comparison come from the same source. It was testing that a function equals
# itself.
#
# The assertion that cannot be faked is the ANSWER. If the request really
# carried `have`, the trade site can only answer in a currency that was asked
# for. If `have` was dropped on the way to the wire, the exchange answers in
# whatever the seller had, which is how an Orb of Augmentation came back as
# "~99 waystone-3". So the currency on the estimate is the primary assertion
# here, and no estimate is a FAIL rather than a note, because a run that formed
# no estimate measured nothing about the wire at all.
#
# The derived `have` list below is kept, but it is secondary and it is honest
# about what it covers: it catches the RULE being wrong, never the request
# dropping the list.
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

# The request line is debug and carries a SUMMARY of the outgoing request that
# the driver re-derives for the log: the endpoint, the trade tag, the currencies
# the rule says to ask for, the filter count and the item. It is not the request
# body and it is not read off the socket. It is enough to tell which endpoint an
# item went to and which rule fired, and that is all it is used for below.
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

# SECONDARY, and weaker than it looks. Both sides of this comparison come from
# the same rule: the log field is produced by calling currencies_to_price_in and
# the expected value below reimplements currencies_to_price_in in shell. So this
# catches the RULE changing to something wrong. It cannot catch the list being
# dropped between the rule and the socket, because the log field would still be
# right. The answer assertion further down is the one that catches that.
#
# Derived rather than hardcoded even so: a hardcoded "exalted" passes for a
# Divine Orb and says nothing about an Exalted Orb.
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
if echo ",$priced_in," | grep -qF ",$tag,"; then
    echo "FAIL: it asked to be paid in $tag for $tag. That exchange has no offers."
    fail=1
fi

# The other end of the same bug. Whatever came back has to be a currency with a
# name, never a raw trade id with a tier number stuck on it.
assert_currency_is_named "$log" || fail=1

# A refused query still reaches "price check finished", so without this a run
# where the trade api threw the search out looks like a quiet run with no
# offers. press-check.sh already treats this line as a failure. It is checked
# here BEFORE the estimate, because a refused search is the most likely reason
# no estimate formed and saying so is more useful than "no estimate".
if grep -q '"msg":"searching the trade site"' "$log"; then
    echo "FAIL: the trade api refused the exchange search:"
    grep '"msg":"searching the trade site"' "$log" | tail -1
    echo "      Nothing below this line measured what was on the wire."
    fail=1
fi

# THE assertion this script exists for.
#
# Everything above reads fields the driver derived from the same function the
# request builder calls, so all of it still passes if the have list never
# reaches the wire. This does not. The trade site can only answer in a currency
# it was offered, so a price denominated in one of them is proof the list
# travelled. A price in anything else is the waystone bug, live.
currency=$(field "$log" "the estimate behind the headline" currency)

if [ -z "$currency" ]; then
    echo "FAIL: no estimate formed, so nothing proved the have list reached the wire."
    echo "      Every assertion above reads a field the driver re-derives with the"
    echo "      same function the request builder calls. Only the currency the"
    echo "      exchange answered in is evidence the list was actually sent."
    echo "      Re-run it. A currency with no offers at all in this league is the"
    echo "      only innocent explanation and it is not one this can assume."
    grep '"msg":"no estimate was formed"' "$log" | tail -1
    fail=1
elif echo ",$priced_in," | grep -qF ",$currency,"; then
    echo "PASS: the price came back in $currency, which is one of the currencies asked for."
    echo "      The exchange can only answer in a currency it was offered, so the"
    echo "      have list reached the trade site rather than only the log."
else
    echo "FAIL: it asked to be paid in $priced_in and was answered in $currency."
    echo "      A price in a currency nobody asked for is the waystone bug again:"
    echo "      the have list is missing from the request that went out."
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
