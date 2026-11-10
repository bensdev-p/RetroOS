#!/bin/sh
# Check that every package in catalog/*.toml exists for arm64 in Debian
# trixie (main, contrib) or the Raspberry Pi archive (catalog/README.md).
#   tools/check-catalog.sh [CACHE_DIR]     (needs the network the first time)
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
C=${1:-${TMPDIR:-/tmp}/retro-catalog-check}
mkdir -p "$C"
get() { # name url decompressor
    [ -s "$C/$1.txt" ] && return 0
    curl -fsS -m 300 "$2" | $3 | sed -n 's/^Package: //p' > "$C/$1.txt"
}
get main http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.xz "xz -dc"
get contrib http://deb.debian.org/debian/dists/trixie/contrib/binary-arm64/Packages.xz "xz -dc"
get rpi http://archive.raspberrypi.com/debian/dists/trixie/main/binary-arm64/Packages.gz "gzip -dc"
bad=0
for p in $(sed -n 's/^packages = \[\(.*\)\]/\1/p' "$ROOT"/catalog/*.toml | tr -d '",' ); do
    if ! grep -qx "$p" "$C/main.txt" "$C/contrib.txt" "$C/rpi.txt"; then
        echo "missing: $p"; bad=1
    fi
done
n=$(grep -c '^\[\[app\]\]' "$ROOT"/catalog/*.toml | awk -F: '{s+=$2} END {print s}')
[ $bad = 0 ] && echo "catalog: $n programs, every package found"
exit $bad
