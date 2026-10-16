#!/bin/sh
# Developer shortcut: rebuild retroos.deb, install it into the existing QEMU
# root tree from the last `make image`, and regenerate out/qemu/root.ext4.
# Use `make image` for anything you ship or measure.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/out}
Q=$OUT/work/rootfs-qemu
[ -d "$Q" ] || { echo "no $Q; run make image first" >&2; exit 1; }
"$ROOT/build/mkdeb.sh" "$OUT"
cp "$OUT"/repo/retroos_*_arm64.deb "$Q/tmp/retroos.deb"
chroot "$Q" dpkg -i /tmp/retroos.deb >/dev/null
rm -f "$Q/tmp/retroos.deb"
size=$(du -sm "$Q" | cut -f1); size=$(( size + size / 4 + 128 ))
rm -f "$OUT/qemu/root.ext4"
mke2fs -q -t ext4 -L RETROROOT -U 52455452-0002-4000-8000-0000000000aa -E root_owner=0:0 -d "$Q" "$OUT/qemu/root.ext4" "${size}M"
echo ">> refreshed $OUT/qemu/root.ext4"
