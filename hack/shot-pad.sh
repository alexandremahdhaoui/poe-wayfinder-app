#!/usr/bin/env bash
# Capture the panel while a scripted pad holds a row focused.
#
# The gamepad row highlight cannot be photographed any other way from here:
# there is no pad on this machine and Windows cannot synthesise one, so
# --gamepad-script stands in and the script parks on a row rather than closing.
set -u

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/harness.sh"

exe="${1:?usage: shot-pad.sh <exe> [pad-file] [out.png]}"
pad="${2:-focus-hold.pad}"
out="${3:-pad.png}"

dir="$(dirname "$exe")"
log="$dir/shot-pad.log"
fake="$dir/shot-pad-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

arm_harness

cp "$here/pads/$pad" "$dir/$pad"
[ -f "$here/items/item.txt" ] && cp "$here/items/item.txt" "$dir/item.txt"

powershell.exe -Command "Set-Clipboard -Value 'shot-pad placeholder'" >/dev/null 2>&1

(timeout 150 "$exe" --fake-game "Path of Exile 2" 140 item.txt >"$fake" 2>&1 &)
wait_for 20 "$fake" 'fakegame' || true

(timeout 130 "$exe" --game poe2 --gamepad-script "$pad" \
    --gamepad-chord "L1+R1+Triangle" --log-level debug >"$log" 2>&1 &)

if ! wait_for 60 "$log" '"msg":"the frame loop is running'; then
    echo "FAIL: the overlay never started"
    exit 1
fi

wait_for 60 "$log" '"msg":"the panel is up"' || echo "note: no panel line"
sleep 2

powershell.exe -NoProfile -ExecutionPolicy Bypass \
    -File "$(wslpath -w "$dir/shot-window.ps1")" \
    -Title "poe-wayfinder" -Out "$(wslpath -w "$dir/$out")" -NoFocus

grep -oE '"rect":"[^"]*"' "$log" | tail -1
echo "shot-pad: $dir/$out"
