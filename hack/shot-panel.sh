#!/usr/bin/env bash
# Bring the price panel up with a stand-in game and capture it.
#
# It exists because the panel cannot be looked at any other way from WSL: it
# only draws while a game window is in front, and it closes when that window
# goes away. Every visual change to the panel is checked with this.
set -u

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/harness.sh"

exe="${1:?usage: shot-panel.sh <exe> [item-file] [out.png]}"
item="${2:-item-rare.txt}"
out="${3:-panel.png}"

dir="$(dirname "$exe")"
log="$dir/shot-panel.log"
fake="$dir/shot-panel-fake.log"

cd "$dir" || exit 1
exe="./$(basename "$exe")"

arm_harness

[ -f "$here/items/$item" ] && cp "$here/items/$item" "$dir/$item"

powershell.exe -Command "Set-Clipboard -Value 'shot placeholder'" >/dev/null 2>&1

(timeout 120 "$exe" --fake-game "Path of Exile 2" 110 "$item" >"$fake" 2>&1 &)
wait_for 20 "$fake" 'fakegame' || true

(timeout 100 "$exe" --game poe2 --log-level debug >"$log" 2>&1 &)

if ! wait_for 60 "$log" '"msg":"the frame loop is running'; then
    echo "FAIL: the overlay never started"
    exit 1
fi

"$exe" --move-mouse 1400 900 >/dev/null 2>&1
press_until "$exe" "$log" --game poe2

wait_for 40 "$log" '"msg":"the panel is up"' || echo "note: no panel line yet"
sleep 1

script_win="$(wslpath -w "$dir/shot.ps1")"
out_win="$(wslpath -w "$dir/$out")"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$script_win" -Out "$out_win"

echo "shot-panel: $dir/$out"
grep -c '"msg":"price check finished"' "$log" 2>/dev/null || true
