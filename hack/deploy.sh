#!/bin/sh
# Copy the Windows build out under a name that will not be overwritten.
#
# # Why versioned and not just poe-trader.exe
#
# Smart App Control decides per binary, by hash. A build it has allowed keeps
# running; a new build is a stranger again and is usually refused. Rebuilding
# the same source does NOT reproduce the hash, so an allowed binary that gets
# overwritten is gone for good.
#
# That happened twice in one afternoon. Two builds cleared, both were
# overwritten by the next `forge build`, and neither could be recovered.
#
# So each build lands under its own name and the previous ones stay. When one
# is found to run, point the launcher at it and it keeps working.

set -eu

SOURCE="../target/x86_64-pc-windows-gnu/release/poe-trader.exe"
DEST="${WIN_OUTPUT_PATH:-/mnt/c/Users/alexa/Desktop/testbin}"

[ -f "$SOURCE" ] || {
    echo "deploy: $SOURCE does not exist. Run: forge build poe-trader-windows" >&2
    exit 1
}

# The commit plus the hash. The commit says what the code is, the hash says
# which build, because two builds of one commit are different binaries.
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo nogit)
DIRTY=$(git status --porcelain 2>/dev/null | head -1)
[ -z "$DIRTY" ] || COMMIT="$COMMIT-dirty"

SHORT_HASH=$(md5sum "$SOURCE" | cut -c1-8)
NAME="poe-trader-$COMMIT-$SHORT_HASH.exe"

cp "$SOURCE" "$DEST/$NAME"

echo "deploy: wrote $DEST/$NAME"
echo
echo "Existing builds, newest last:"
ls -1t "$DEST"/poe-trader-*.exe 2>/dev/null | tail -8 | while read -r f; do
    echo "  $(basename "$f")"
done

echo
echo "Try it, and if Windows lets it run, keep it:"
echo "  cd \"$DEST\" && ./$NAME --list-windows"
