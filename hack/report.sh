#!/usr/bin/env bash

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

WS="$(cd .. && pwd)"
REF="$WS/reference/Exiled-Exchange-2/renderer/src/web"

rule() { printf '%s\n' "------------------------------------------------------------"; }

# A widget counts as covered when the capability catalogue names it. The
# price-check and ui capabilities predate the widget backlog and name a bare
# file rather than a directory, so they are matched separately.
covered() {
    case "$1" in
        price-check|ui) return 0 ;;
    esac

    grep -q "\"$1/" src/bin/poe-wayfinder-uiparity.rs 2>/dev/null
}

value() { printf '  %-34s %s\n' "$1" "$2"; }

echo
echo "poe-wayfinder, what is measured"
rule

arch=$(cargo run --quiet --release --bin poe-wayfinder-arch -- --root . --max 0 2>/dev/null)

value "wired public functions" "$(echo "$arch" | grep -E '^  wired' | sed 's/.*: //')"
value "architecture violations" "$(echo "$arch" | grep -E '^  violations' | sed 's/.*: //')"
value "unit tests" "$(echo "$arch" | grep -E '^  tests' | sed 's/.*: //')"
value "files with public code, no test" "$(echo "$arch" | grep -E 'no test' | sed 's/.*: //')"

rule

for stage in parity parity-overlay parity-poe1; do
    out=$(forge test run "$stage" 2>&1)
    pct=$(echo "$out" | grep -E '^  parity ' | sed 's/.*: //')
    gap=$(echo "$out" | grep -E '^  missing ' | sed 's/.*: //')

    value "$stage" "${pct:-not run}  (${gap:-?} missing)"
done

ui=$(cargo run --quiet --release --bin poe-wayfinder-uiparity 2>/dev/null)

value "ui parity" "$(echo "$ui" | grep -E '^  ui parity' | sed 's/.*: //')"
value "ui capabilities" "$(echo "$ui" | grep -E '^  implemented' | sed 's/.*: //')"
value "ui waived" "$(echo "$ui" | grep -E '^  waived' | sed 's/.*: //')"

rule

prod=0
tests=0

while read -r f; do
    n=$(awk '/^#\[cfg\(test\)\]/{seen=1} !seen{c++} END{print c+0}' "$f")
    prod=$((prod + n))
    tests=$((tests + $(wc -l < "$f") - n))
done < <(find ../poe-wayfinder-core/src ../poe-wayfinder-data/src src -name '*.rs' ! -name 'zz_generated*')

value "production lines" "$prod"
value "test lines" "$tests"

rule

if [ -d "$REF" ]; then
    total=0
    ported=0

    for dir in "$REF"/*/; do
        name=$(basename "$dir")
        lines=$(find "$dir" \( -name '*.vue' -o -name '*.ts' \) -exec cat {} \; 2>/dev/null | wc -l)
        total=$((total + lines))

        # A widget counts as covered when the capability catalogue names it,
        # not from a list kept by hand here. uiparity checks each capability is
        # actually reachable from src/driver, so this cannot drift into a claim.
        if covered "$name"; then
            ported=$((ported + lines))
        fi
    done

    # This counts a widget as covered once the catalogue names it, so it says
    # which widgets have been started, not how much of each is ported. The line
    # totals are the upstream size of those widgets, not a claim about ours.
    value "upstream widget lines" "$total"
    value "widgets with capabilities" "$ported lines' worth  ($((ported * 100 / total))%)"
    value "widgets with none" "$((total - ported))"

    echo
    echo "  widgets with no capability yet:"

    for dir in "$REF"/*/; do
        name=$(basename "$dir")

        covered "$name" && continue

        lines=$(find "$dir" \( -name '*.vue' -o -name '*.ts' \) -exec cat {} \; 2>/dev/null | wc -l)
        printf '    %-14s %5s lines\n' "$name" "$lines"
    done
else
    value "upstream reference" "not checked out, widget gap unknown"
fi

rule

value "embedded data" "$(du -shc ../poe-wayfinder-data/data/*/*.ndjson 2>/dev/null | tail -1 | cut -f1)"
value "windows exe" "$(stat -c%s ../target/x86_64-pc-windows-gnu/release/poe-wayfinder.exe 2>/dev/null | awk '{printf "%.1f MB", $1/1048576}')"
value "flags needed to run it" "0"

rule

if cargo llvm-cov --version >/dev/null 2>&1; then
    value "line coverage" "run: cargo llvm-cov --workspace --summary-only"
else
    value "line coverage" "NOT MEASURED, cargo-llvm-cov is not installed"
fi

echo
