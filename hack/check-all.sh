#!/usr/bin/env bash
#
# Every end to end harness, in order, against one exe.
#
# There were seven scripts and a list of invocations in CLAUDE.md that had to be
# pasted by hand. Pasting it is how a harness gets skipped: the four bugs that
# reached a user this week each had a harness that either did not exist or was
# not in the list anyone pasted.
#
# Three rules it follows and the reason for each:
#
#   - It KEEPS GOING after a failure. Stopping at the first one hides the other
#     six, and these take minutes each, so a stop-first run costs a whole cycle
#     per bug.
#
#   - It says what is running BEFORE it runs it. The suite takes eight to
#     fifteen minutes and most of that is silence while a bounded wait counts
#     down. Silence looks like a hang, and a hang gets killed, which leaves an
#     orphan overlay owning the hotkey.
#
#   - It kills every overlay BETWEEN harnesses, not only inside them. Each
#     harness cleans up after itself, but a harness that is killed from outside
#     never reaches its own trap, and the leftover then answers the NEXT
#     harness's press, which reads as that harness passing when it never
#     started.
#
# Needs a Windows host, and everything except press-check needs the network.
# Run it from WSL after hack/deploy.sh.

set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

source "$here/harness.sh"

usage() {
    cat <<'EOF'
check-all.sh <exe> [--anyway] [--only NAME]

Runs every end to end harness against one deployed exe and prints one table.

  <exe>          path to the deployed poe-wayfinder exe, usually under
                 $WIN_OUTPUT_PATH. Pass the hashed name, not poe-wayfinder.exe.
  --anyway       run even though a real Path of Exile window is open. Read the
                 warning first: the harnesses open stand-in windows with the
                 game's own title and press keys at whatever answers.
  --only NAME    run one harness. NAME is any of the names in the table, for
                 example focus, league, exchange, refresh, both-games, or
                 press-poe2 / press-poe1 / press-currency / press-runable.

Exit code is the number of harnesses that failed, so 0 means everything passed.
EOF
}

exe=""
anyway=0
only=""

while [ $# -gt 0 ]; do
    case "$1" in
        --anyway) anyway=1 ;;
        --only) only="${2:?--only needs a harness name}"; shift ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "check-all: unknown argument $1" >&2; usage; exit 2 ;;
        *) exe="$1" ;;
    esac
    shift
done

if [ -z "$exe" ]; then
    usage
    exit 2
fi

if [ ! -f "$exe" ]; then
    echo "check-all: $exe does not exist."
    echo "           Build, then run hack/deploy.sh, then pass the hashed name it printed."
    exit 2
fi

dir="$(cd "$(dirname "$exe")" && pwd)"
base="$(basename "$exe")"
summary_log="$dir/check-all.log"

# ------------------------------------------------------------------
# Preflight. Nine harnesses against an exe that cannot start is nine
# meaningless failures and fifteen wasted minutes.
# ------------------------------------------------------------------

echo "check-all: $base"
echo

# Each harness traps for itself, and Ctrl+C reaches them because they run in
# this process group. A SIGTERM aimed at this script alone does not, so it gets
# its own trap: whatever the running harness had open dies with the suite
# rather than surviving to answer the next run's hotkey.
trap stop_overlays EXIT INT TERM

# Kill first, so the window list below is the real desktop rather than a
# stand-in left over from an interrupted run. A stand-in carries the game's own
# title, so without this the preflight would refuse to run because of a window
# this suite created itself.
stop_overlays
sleep 2

windows_log="$dir/check-all-windows.log"

# --list-windows is the cheapest thing that proves Smart App Control is letting
# this binary run at all. SAC blocks roughly two new unsigned builds in three,
# and from WSL the only symptom is `Invalid argument`.
if ! (cd "$dir" && "./$base" --list-windows) >"$windows_log" 2>&1; then
    echo "FAIL: the exe would not run at all."
    echo "      Smart App Control blocks most new unsigned builds. Touch a source"
    echo "      file, rebuild so the hash changes, deploy again and retry. Retrying"
    echo "      the same binary is useless: SAC decides per hash."
    tail -3 "$windows_log"
    exit 2
fi

echo "preflight: the exe runs."

# A real game running is not a small problem. Every harness opens a stand-in
# window titled "Path of Exile 2", presses hotkeys and sends Ctrl+C at whatever
# holds the foreground. Against a live session that means keys going into
# someone's game.
# --list-windows prints each title indented and quoted, so an exact quoted match
# is what tells "Path of Exile 2" apart from a browser tab that merely mentions
# it. The `2?` covers both games. POSIX classes rather than \s, because this
# runs under whatever grep the machine has.
if grep -qE '^[[:space:]]*"Path of Exile 2?"[[:space:]]*$' "$windows_log"; then
    if [ "$anyway" -eq 0 ]; then
        echo
        echo "REFUSED: a real Path of Exile window is open."
        echo "         These harnesses open stand-in windows with the same title and"
        echo "         inject key presses at whatever is in front. Against a live"
        echo "         session that is keys going into the game, and the overlay will"
        echo "         attach to the real game rather than the stand-in, so the run"
        echo "         measures nothing anyway."
        echo
        echo "         Close the game, or pass --anyway if you know it is safe."
        exit 2
    fi

    echo "preflight: a real game window is open and --anyway was passed. Continuing."
fi

echo

# ------------------------------------------------------------------
# The suite.
#
# Ordered cheapest and most fundamental first. press-check is the whole price
# check end to end, so when it fails the four below it are usually failing for
# the same reason and the table says so at a glance. refresh-check is last
# because it is the only one that downloads six tables.
# ------------------------------------------------------------------

names=()
labels=()

add() {
    names+=("$1")
    labels+=("$2")
}

add press-poe2     "press-check, poe2 rare"
add press-poe1     "press-check, poe1 rare"
add press-currency "press-check, poe2 currency"
add press-runable  "press-check, poe2 rune socket"
add exchange       "exchange-check, priced in"
add focus          "focus-check, focused panel"
add league         "league-check, current league"
add both-games     "both-games-check, follow"
add pad            "pad-check, a pad alone"
add refresh        "refresh-check, weekly data"

# A case rather than a table of command strings. A string would need `eval` to
# run, and `eval` plus a Windows path plus an empty data-dir argument is three
# ways to lose a quote. The empty '' argument is load bearing: it is the data
# dir, and empty is what proves the exe finds its own data with no flag.
run_harness() {
    case "$1" in
        press-poe2)     bash "$here/press-check.sh"      "$exe" "" poe2 item.txt ;;
        press-poe1)     bash "$here/press-check.sh"      "$exe" "" poe1 item-poe1.txt ;;
        press-currency) bash "$here/press-check.sh"      "$exe" "" poe2 item-currency.txt ;;
        press-runable)  bash "$here/press-check.sh"      "$exe" "" poe2 item-runable.txt ;;
        exchange)       bash "$here/exchange-check.sh"   "$exe" ;;
        focus)          bash "$here/focus-check.sh"      "$exe" ;;
        league)         bash "$here/league-check.sh"     "$exe" ;;
        both-games)     bash "$here/both-games-check.sh" "$exe" ;;
        refresh)        bash "$here/refresh-check.sh"    "$exe" ;;
        pad)            bash "$here/pad-check.sh"        "$exe" ;;
        *)              echo "check-all: no harness named $1"; return 2 ;;
    esac
}

# A misspelled --only would otherwise skip every harness and exit 0, which is
# the worst possible answer: a green run that tested nothing. That is the same
# shape as the bug this whole suite exists to stop.
if [ -n "$only" ]; then
    known=0

    for name in "${names[@]}"; do
        [ "$name" = "$only" ] && known=1
    done

    if [ "$known" -eq 0 ]; then
        echo "check-all: there is no harness named \"$only\"."
        echo "           Running nothing and reporting success would be worse, so this stops."
        echo
        echo "Known harnesses:"
        printf '  %s\n' "${names[@]}"
        exit 2
    fi
fi

results=()
durations=()
logs=()
ran=0
failed=0

started_all=$(date +%s)

for i in "${!names[@]}"; do
    name="${names[$i]}"

    if [ -n "$only" ] && [ "$name" != "$only" ]; then
        results+=("skipped")
        durations+=(0)
        logs+=("")
        continue
    fi

    ran=$((ran + 1))

    log="$dir/check-all-$name.log"
    logs+=("$log")

    echo "------------------------------------------------------------"
    echo "[$((i + 1))/${#names[@]}] ${labels[$i]}"
    echo "------------------------------------------------------------"

    started=$(date +%s)

    # tee so the run is watchable AND readable afterwards. PIPESTATUS rather
    # than $? because $? here is tee's, which is always 0. Getting that wrong
    # makes every harness pass.
    run_harness "$name" 2>&1 | tee "$log"
    code="${PIPESTATUS[0]}"

    finished=$(date +%s)
    durations+=($((finished - started)))

    if [ "$code" -eq 0 ]; then
        results+=("PASS")
    else
        results+=("FAIL")
        failed=$((failed + 1))
    fi

    # Between harnesses, not only inside them. A harness killed from outside
    # never reaches its own trap, and the orphan owns the hotkey registration,
    # so the next harness's press is answered by the wrong process.
    stop_overlays
    sleep 3

    echo
done

elapsed_all=$(($(date +%s) - started_all))

# ------------------------------------------------------------------
# One table. It is the only output anyone reads when the run passes.
# ------------------------------------------------------------------

{
    echo
    echo "============================================================"
    printf '%-16s %-32s %6s  %s\n' "HARNESS" "WHAT IT PROVES" "TIME" "RESULT"
    echo "============================================================"

    for i in "${!names[@]}"; do
        printf '%-16s %-32s %5ss  %s\n' \
            "${names[$i]}" "${labels[$i]}" "${durations[$i]}" "${results[$i]}"
    done

    echo "============================================================"
    printf '%s harness(es) ran in %sm %ss, %s failed.\n' \
        "$ran" "$((elapsed_all / 60))" "$((elapsed_all % 60))" "$failed"
} | tee "$summary_log"

if [ "$failed" -gt 0 ]; then
    echo
    echo "Each failing harness prints why it matters in its own output. Logs:"

    for i in "${!names[@]}"; do
        [ "${results[$i]}" = "FAIL" ] || continue

        echo "  ${names[$i]}: ${logs[$i]}"
    done
fi

# The count, so a caller can branch on how bad it is. 0 means everything passed.
exit "$failed"
