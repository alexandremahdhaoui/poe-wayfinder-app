#!/usr/bin/env bash
#
# The exe, run from the Windows side, from a real Windows directory.
#
# Every other gate builds for Windows and then runs the result from WSL, where
# the interop proxy hands the process a console. A person does not do that. A
# person copies the exe somewhere on C: and types its name into PowerShell.
#
# Those are different enough to hide a bug that makes the app useless. The exe
# is built with `windows_subsystem = "windows"`, so it starts with no standard
# output at all, and every diagnostic printed nothing when launched from a
# Windows shell. It printed perfectly from WSL, which is the only place anyone
# had ever run it.
#
# This runs the real binary from C:, through cmd and through PowerShell, and
# asserts the output arrives. It starts no overlay, opens no window, presses no
# key and touches no network, so it is safe in a build gate.

set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app="$(cd "$here/.." && pwd)"

exe="${1:-$app/../target/x86_64-pc-windows-gnu/release/poe-wayfinder.exe}"

fail=0

# Every harness in this directory kills leftovers on entry and from a trap.
# This one starts no overlay, but it does start the exe, and an orphaned copy
# holds the file open. The next build then fails with `Permission denied` on
# the deploy copy, which reads as a broken build rather than as a stray
# process. That happened once already.
stop_smoke() {
    taskkill.exe /F /IM poe-smoke.exe >/dev/null 2>&1 || true
}

trap stop_smoke EXIT INT TERM

stop_smoke

say_fail() {
    echo "FAIL: $1"
    fail=1
}

if [ ! -f "$exe" ]; then
    echo "windows-smoke: $exe does not exist."
    echo "               Build it first:"
    echo "               cargo build --release --target x86_64-pc-windows-gnu \\"
    echo "                 -p poe-wayfinder-app --bin poe-wayfinder"
    exit 2
fi

if ! command -v powershell.exe >/dev/null 2>&1; then
    echo "windows-smoke: no powershell.exe, so there is no Windows side to run on."
    echo "               This gate needs a Windows host. Reporting failure rather"
    echo "               than success, because a skipped check that prints OK is"
    echo "               how a suite goes green while testing nothing."
    exit 2
fi

# A real Windows directory, not a \\wsl.localhost path. cmd refuses a UNC
# working directory and silently defaults to C:\Windows, which would make
# every assertion below measure the wrong thing.
# The Windows user is not the WSL user, so %TEMP% is asked for rather than
# guessed from $USER.
windows_temp="$(cmd.exe /d /c echo %TEMP% 2>/dev/null | tr -d '\r')"
win_dir="${WIN_SMOKE_DIR:-}"

if [ -z "$win_dir" ] && [ -n "$windows_temp" ]; then
    win_dir="$(wslpath -u "$windows_temp" 2>/dev/null)/poe-wayfinder-smoke"
fi

if [ -z "$win_dir" ] || [ ! -d "$(dirname "$win_dir")" ]; then
    win_dir="/mnt/c/Windows/Temp/poe-wayfinder-smoke"
fi

mkdir -p "$win_dir" || {
    echo "windows-smoke: cannot write to $win_dir"
    exit 2
}

# A name of its own, never poe-wayfinder.exe. The deploy directory holds builds
# that are allowed to run under Smart App Control, and overwriting one loses
# that permission for good.
cp "$exe" "$win_dir/poe-smoke.exe" || {
    echo "windows-smoke: copying the exe failed. Is one still running?"
    exit 2
}

drive_path="$(cd "$win_dir" && cmd.exe /d /c cd 2>/dev/null | tr -d '\r')"

if [ -z "$drive_path" ]; then
    say_fail "could not resolve $win_dir to a drive letter path."
    exit 1
fi

echo "windows-smoke: running from $drive_path"
echo

run_in_cmd() {
    local out="$win_dir/cmd-$1.txt"

    rm -f "$out"

    (cd "$win_dir" && cmd.exe /d /c "poe-smoke.exe $2 > cmd-$1.txt 2>&1") \
        >/dev/null 2>&1

    # cmd does not wait for a windows subsystem exe, so the redirect is still
    # being written when cmd returns. Wait for the file rather than sleeping a
    # guessed number of seconds.
    local waited=0

    while [ "$waited" -lt 100 ]; do
        [ -s "$out" ] && break
        sleep 0.1
        waited=$((waited + 1))
    done

    cat "$out" 2>/dev/null
}

run_in_powershell() {
    powershell.exe -NoProfile -Command \
        "Set-Location '$drive_path'; .\\poe-smoke.exe $1" 2>&1 | tr -d '\r'
}

expect() {
    local what="$1" needle="$2" output="$3"

    if printf '%s' "$output" | grep -qF "$needle"; then
        echo "  ok    $what"
        return
    fi

    say_fail "$what printed nothing containing \"$needle\"."
    echo "      A windows subsystem exe has no standard output until it attaches"
    echo "      to the parent console. If this broke, check attach_console."
    printf '%s' "$output" | head -5 | sed 's/^/      /'
}

echo "1. the diagnostics answer when cmd redirects them to a file"
expect "cmd, --list-gamepads" \
    "HID gamepads this build can see" \
    "$(run_in_cmd gamepads --list-gamepads)"

echo
echo "2. the diagnostics answer when a person types the name in PowerShell"
expect "powershell, --list-gamepads" \
    "HID gamepads this build can see" \
    "$(run_in_powershell --list-gamepads)"
expect "powershell, --list-windows" \
    "Visible windows, one per line" \
    "$(run_in_powershell --list-windows)"

echo
echo "3. an unknown flag does not silently start the overlay"
walkthrough="$(run_in_powershell "--pad-walkthrough $drive_path\\smoke.hex")"

if printf '%s' "$walkthrough" | grep -qF "no playstation pad found"; then
    echo "  ok    powershell, --pad-walkthrough says there is no pad"
else
    expect "powershell, --pad-walkthrough" "Walkthrough for a" "$walkthrough"
fi

echo
rm -rf "$win_dir"

if [ "$fail" -eq 0 ]; then
    echo "windows-smoke: the exe answers from the Windows side."
    exit 0
fi

echo "windows-smoke: the exe is silent from the Windows side, which is how a"
echo "               user would run it. It may still work perfectly from WSL."
exit 1
