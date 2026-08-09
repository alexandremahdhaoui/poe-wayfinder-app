#!/usr/bin/env bash

set -uo pipefail

DEST="${WIN_OUTPUT_PATH:-/mnt/c/Users/alexa/Desktop/testbin}"
KEEP=3
APPLY=0
BUILDS=0

usage() {
    cat <<'EOF'
cleanup.sh [--yes] [--keep N] [--builds]

Reports what it would remove and removes nothing. Pass --yes to act.

  --keep N   deployed exes to keep, newest first. Default 3.
  --builds   also remove the cargo target directory. It is 8G+ and the next
             build takes minutes instead of seconds.

Never touches: golden-*.exe, data*/ , *.cmd, any exe a launcher points at,
or any log that is not from press-check.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --yes) APPLY=1 ;;
        --builds) BUILDS=1 ;;
        --keep) KEEP="${2:?--keep needs a number}"; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "cleanup: unknown argument $1" >&2; usage; exit 1 ;;
    esac
    shift
done

say() {
    case "$APPLY" in
        1) echo "$*" ;;
        *) echo "would $*" ;;
    esac
}

freed_kb=0

remember_size() {
    [ -e "$1" ] || return 0
    freed_kb=$((freed_kb + $(du -sk "$1" 2>/dev/null | cut -f1)))
}

stop_stray_overlays() {
    local running
    running=$(powershell.exe -Command \
        "(Get-Process poe-trader* -ErrorAction SilentlyContinue | Measure-Object).Count" \
        2>/dev/null | tr -d '\r\n ')

    if [ -z "$running" ] || [ "$running" = "0" ]; then
        echo "no overlay is running"
        return 0
    fi

    say "stop $running running overlay process(es)"

    if [ "$APPLY" -eq 1 ]; then
        powershell.exe -Command \
            "Get-Process poe-trader* -ErrorAction SilentlyContinue | Stop-Process -Force" \
            >/dev/null 2>&1
        sleep 2
    fi
}

launcher_targets() {
    grep -ho 'poe-trader[A-Za-z0-9._-]*\.exe' "$DEST"/*.cmd 2>/dev/null | sort -u
}

prune_deployed_exes() {
    local protected keep_list all

    protected=$(launcher_targets)

    if [ -n "$protected" ]; then
        echo "kept because a .cmd launches it:"
        echo "$protected" | sed 's/^/  /'
    fi

    all=$(cd "$DEST" 2>/dev/null && ls -1t poe-trader-*.exe 2>/dev/null)

    if [ -z "$all" ]; then
        echo "no deployed exe to prune"
        return 0
    fi

    keep_list=$(echo "$all" | head -n "$KEEP")

    echo "kept because it is one of the newest $KEEP:"
    echo "$keep_list" | sed 's/^/  /'

    local removed=0

    while read -r exe; do
        [ -n "$exe" ] || continue
        echo "$keep_list" | grep -qxF "$exe" && continue
        echo "$protected" | grep -qxF "$exe" && continue

        remember_size "$DEST/$exe"
        say "remove $exe"
        [ "$APPLY" -eq 1 ] && rm -f "$DEST/$exe"
        removed=$((removed + 1))
    done <<<"$all"

    echo "$removed old exe(s)"
}

remove_test_artifacts() {
    local f

    for f in press-check.log press-check-fake.log item.txt item-poe1.txt \
             item-currency.txt item-runable.txt \
             both-games.log both-games-poe1.log both-games-poe2.log \
             refresh-1.log refresh-2.log refresh-3.log; do
        [ -f "$DEST/$f" ] || continue

        remember_size "$DEST/$f"
        say "remove $f"
        [ "$APPLY" -eq 1 ] && rm -f "$DEST/$f"
    done

    # refresh-check.sh builds this beside the exe and removes it on a clean
    # run. An interrupted run leaves 5 MB of fetched ndjson behind.
    if [ -d "$DEST/refresh-check-config" ]; then
        remember_size "$DEST/refresh-check-config"
        say "remove refresh-check-config/"
        [ "$APPLY" -eq 1 ] && rm -rf "$DEST/refresh-check-config"
    fi
}

remove_build_cache() {
    local target="$(cd "$(dirname "$0")/../.." && pwd)/target"

    [ -d "$target" ] || return 0

    if [ "$BUILDS" -eq 0 ]; then
        echo "build cache is $(du -sh "$target" 2>/dev/null | cut -f1), left alone. Pass --builds to remove it."
        return 0
    fi

    remember_size "$target"
    say "remove $target"
    [ "$APPLY" -eq 1 ] && rm -rf "$target"
}

echo "cleanup, deploy directory $DEST"
echo

stop_stray_overlays
echo

prune_deployed_exes
echo

remove_test_artifacts
echo

remove_build_cache
echo

if [ "$freed_kb" -ge 1024 ]; then
    printf 'total %s MB\n' "$((freed_kb / 1024))"
else
    printf 'total %s KB\n' "$freed_kb"
fi

if [ "$APPLY" -eq 0 ]; then
    echo
    echo "nothing was removed. Run again with --yes."
fi
