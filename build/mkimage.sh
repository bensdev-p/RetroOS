#!/bin/sh
# Build the RetroOS images from source.
#
#   build/mkimage.sh [OUT]
#
# Produces:
#   OUT/retroos-<ver>-rpi-arm64.img     flashable SD-card image (Pi 5 / Pi 4)
#   OUT/qemu/{vmlinuz,initrd.img,root.ext4,cmdline}   the same root file system
#                                       with a generic arm64 kernel, for QEMU
#   OUT/manifest.txt                    every package and version in the image
#
# Needs: mmdebstrap, qemu-user-static (binfmt) on non-arm64 hosts, dosfstools,
# mtools, e2fsprogs, fdisk, xz-utils. Runs as root (or inside the build container).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "${1:-$ROOT/out}" && cd "${1:-$ROOT/out}" && pwd)
WORK=$OUT/work
SNAPSHOT=$(grep -v '^#' "$ROOT/build/lock/snapshot" | head -1)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
KR=$ROOT/build/keyrings
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}
export RETRO_ROOT=$ROOT RETRO_WORK=$WORK

DEB=https://snapshot.debian.org/archive/debian/$SNAPSHOT
SEC=https://snapshot.debian.org/archive/debian-security/$SNAPSHOT
RPI=https://archive.raspberrypi.com/debian

[ -f "$OUT/repo/Packages" ] || "$ROOT/build/mkdeb.sh" "$OUT"

rm -rf "$WORK"
mkdir -p "$WORK"

list() { grep -hv '^\s*#' "$@" | sed '/^\s*$/d' | paste -sd, -; }
PKGS=$(list "$ROOT/build/packages/base.list" "$ROOT/build/packages/rpi.list")

# Generic arm64 kernel for the QEMU variant, fetched straight from the
# snapshot and verified: signed InRelease -> Packages.xz hash -> .deb hash.
fetch_qemu_kernel() {
    d=$WORK/qemu-debs
    mkdir -p "$d"
    curl -fsSL -o "$WORK/InRelease" "$DEB/dists/trixie/InRelease"
    gpgv --keyring "$KR/debian-archive-keyring.gpg" "$WORK/InRelease" 2>/dev/null
    curl -fsSL -o "$WORK/Packages.xz" "$DEB/dists/trixie/main/binary-arm64/Packages.xz"
    want=$(awk '/^SHA256:/{s=1;next} /^[A-Z]/{s=0} s && $3=="main/binary-arm64/Packages.xz"{print $1}' "$WORK/InRelease")
    echo "$want  $WORK/Packages.xz" | sha256sum -c --quiet
    xz -dc "$WORK/Packages.xz" > "$WORK/Packages"
    field() { awk -v P="$1" -v F="$2" '$0=="Package: "P{f=1} f && index($0, F": ")==1 {print substr($0, length(F)+3); exit} f && /^$/{f=0}' "$WORK/Packages"; }
    meta=$(grep -v '^#' "$ROOT/build/packages/qemu.list" | head -1)
    image=$(field "$meta" Depends | tr ',' '\n' | sed 's/^ *//; s/ .*//' | grep '^linux-image-[0-9]' | head -1)
    for p in $meta $image; do
        file=$(field "$p" Filename); sum=$(field "$p" SHA256)
        curl -fsSL -o "$d/$(basename "$file")" "$DEB/$file"
        echo "$sum  $d/$(basename "$file")" | sha256sum -c --quiet
    done
    echo ">> QEMU kernel: $image"
}
fetch_qemu_kernel

echo ">> mmdebstrap (snapshot $SNAPSHOT)"
mmdebstrap --mode=root --variant=apt --architectures=arm64 \
    --aptopt='Acquire::Check-Valid-Until "false"' \
    --aptopt='Acquire::Retries "3"' \
    --aptopt='APT::Install-Recommends "false"' \
    --dpkgopt='path-exclude=/usr/share/doc/*' \
    --dpkgopt='path-include=/usr/share/doc/*/copyright' \
    --dpkgopt='path-include=/usr/share/doc/retroos/*' \
    --dpkgopt='path-exclude=/usr/share/man/*' \
    --dpkgopt='path-exclude=/usr/share/info/*' \
    --dpkgopt='path-exclude=/usr/share/locale/*' \
    --dpkgopt='path-include=/usr/share/locale/locale.alias' \
    --include="$PKGS" \
    --setup-hook='mkdir -p "$1/boot/firmware"' \
    --customize-hook="$ROOT/build/hooks/customize.sh" \
    trixie "$WORK/rootfs" \
    "deb [signed-by=$KR/debian-archive-keyring.gpg] $DEB trixie main contrib non-free-firmware" \
    "deb [signed-by=$KR/debian-archive-keyring.gpg] $DEB trixie-updates main contrib non-free-firmware" \
    "deb [signed-by=$KR/debian-archive-keyring.gpg] $SEC trixie-security main contrib non-free-firmware" \
    "deb [signed-by=$KR/raspberrypi-archive-keyring.gpg] $RPI trixie main" \
    "deb [trusted=yes] copy:$OUT/repo ./"

R=$WORK/rootfs
chroot "$R" dpkg-query -W -f='${Package}\t${Version}\t${Architecture}\n' | sort > "$OUT/manifest.txt"
echo ">> $(wc -l < "$OUT/manifest.txt") packages"

# ------------------------------------------------------------- QEMU tree
echo ">> QEMU variant (generic kernel)"
Q=$WORK/rootfs-qemu
cp -a "$R" "$Q"
mkdir -p "$Q/tmp/qemu-debs"
cp "$WORK"/qemu-debs/*.deb "$Q/tmp/qemu-debs/"
mount -t proc proc "$Q/proc"
mount --bind /dev "$Q/dev"
mount -t sysfs sys "$Q/sys"
trap 'umount "$Q/proc" "$Q/dev" "$Q/sys" 2>/dev/null || true' EXIT
chroot "$Q" sh -c 'dpkg -i /tmp/qemu-debs/*.deb >/tmp/qemu-dpkg.log 2>&1 || (tail -20 /tmp/qemu-dpkg.log; exit 1)'
umount "$Q/proc" "$Q/dev" "$Q/sys"
trap - EXIT
mkdir -p "$OUT/qemu"
cp "$(ls "$Q"/boot/vmlinuz-*-arm64 | tail -1)" "$OUT/qemu/vmlinuz"
cp "$(ls "$Q"/boot/initrd.img-*-arm64 | tail -1)" "$OUT/qemu/initrd.img"
rm -rf "$Q/tmp/qemu-debs" "$Q"/tmp/*.log "$Q/boot/firmware"/*
sed -i 's|^PARTUUID=52455452-02 |/dev/vda |; /boot\/firmware/d' "$Q/etc/fstab"
echo "console=ttyAMA0 console=tty1 root=/dev/vda rw quiet loglevel=3 vt.global_cursor_default=0 systemd.show_status=false" > "$OUT/qemu/cmdline"

# ------------------------------------------------------------- file systems
mkext4() { # dir out label uuid
    size=$(du -sm "$1" | cut -f1)
    size=$(( size + size / 4 + 128 ))
    rm -f "$2"
    E2FSPROGS_FAKE_TIME=$SOURCE_DATE_EPOCH mke2fs -q -t ext4 -L "$3" -U "$4" \
        -E root_owner=0:0,hash_seed=52455452-0000-4000-8000-000000000000 \
        -d "$1" "$2" "${size}M"
}

echo ">> boot partition"
BOOTDIR=$WORK/bootfiles
rm -rf "$BOOTDIR" && mkdir -p "$BOOTDIR"
cp -a "$R/boot/firmware/." "$BOOTDIR/"
find "$R/boot/firmware" -mindepth 1 -delete
BOOT_MB=256
rm -f "$WORK/boot.vfat"
mkfs.vfat -n RETROBOOT -F 32 -i 52455452 -C "$WORK/boot.vfat" $((BOOT_MB * 1024)) >/dev/null
MTOOLS_SKIP_CHECK=1 mcopy -s -m -i "$WORK/boot.vfat" "$BOOTDIR"/* ::/

echo ">> root partitions"
mkext4 "$R" "$WORK/root.ext4" RETROROOT 52455452-0002-4000-8000-000000000000
mkext4 "$Q" "$OUT/qemu/root.ext4" RETROROOT 52455452-0002-4000-8000-0000000000aa

echo ">> disk image"
IMG=$OUT/retroos-$VERSION-rpi-arm64.img
root_sectors=$(( $(stat -c %s "$WORK/root.ext4") / 512 ))
boot_start=8192
boot_sectors=$(( BOOT_MB * 2048 ))
root_start=$(( boot_start + boot_sectors ))
total=$(( root_start + root_sectors + 2048 ))
rm -f "$IMG"
truncate -s $(( total * 512 )) "$IMG"
sfdisk -q "$IMG" <<EOF
label: dos
label-id: 0x52455452
unit: sectors
start=$boot_start, size=$boot_sectors, type=c, bootable
start=$root_start, size=$root_sectors, type=83
EOF
dd if="$WORK/boot.vfat" of="$IMG" bs=512 seek=$boot_start conv=notrunc,sparse status=none
dd if="$WORK/root.ext4" of="$IMG" bs=512 seek=$root_start conv=notrunc,sparse status=none
(cd "$OUT" && sha256sum "$(basename "$IMG")" > "$(basename "$IMG").sha256")
echo ">> $IMG ($(du -h --apparent-size "$IMG" | cut -f1))"
