#!/usr/bin/env bash

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

WS="$(cd .. && pwd)"
REF="$WS/reference/Exiled-Exchange-2/renderer/src/web"

rule() { printf '%s\n' "------------------------------------------------------------"; }

value() { printf '  %-34s %s\n' "$1" "$2"; }

echo
echo "poe-trader, what is measured"
rule

arch=$(cargo run --quiet --release --bin poe-trader-arch -- --root . --max 0 2>/dev/null)

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

ui=$(cargo run --quiet --release --bin poe-trader-uiparity 2>/dev/null)

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
done < <(find ../poe-trader-core/src ../poe-trader-data/src src -name '*.rs' ! -name 'zz_generated*')

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

        case "$name" in
            price-check|ui) ported=$((ported + lines)) ;;
        esac
    done

    value "upstream widget lines" "$total"
    value "in a widget we have" "$ported  ($((ported * 100 / total))%)"
    value "in a widget we do not have" "$((total - ported))"

    echo
    echo "  widgets not started:"

    for dir in "$REF"/*/; do
        name=$(basename "$dir")

        case "$name" in
            price-check|ui) continue ;;
        esac

        lines=$(find "$dir" \( -name '*.vue' -o -name '*.ts' \) -exec cat {} \; 2>/dev/null | wc -l)
        printf '    %-14s %5s lines\n' "$name" "$lines"
    done
else
    value "upstream reference" "not checked out, widget gap unknown"
fi

rule

value "embedded data" "$(du -shc ../poe-trader-data/data/*/*.ndjson 2>/dev/null | tail -1 | cut -f1)"
value "windows exe" "$(stat -c%s ../target/x86_64-pc-windows-gnu/release/poe-trader.exe 2>/dev/null | awk '{printf "%.1f MB", $1/1048576}')"
value "flags needed to run it" "0"

rule

if cargo llvm-cov --version >/dev/null 2>&1; then
    value "line coverage" "run: cargo llvm-cov --workspace --summary-only"
else
    value "line coverage" "NOT MEASURED, cargo-llvm-cov is not installed"
fi

echo
