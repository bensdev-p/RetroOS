#!/bin/sh
# Builds the RetroOS bootloader settings card for the Raspberry Pi 5:
#   build/mkeeprom.sh [OUT_DIR]      (default: out)
# -> OUT/bootloader/retroos-bootloader-rpi5.img (a card image for Raspberry
#    Pi Imager, "Use custom") and README.txt, also zipped as
#    OUT/retroos-bootloader-rpi5.zip, with Raspberry Pi's own EEPROM recovery files and the
#    settings in config/eeprom/boot.conf. D-034, HW_CHECKLIST "Bootloader".
#
# Same steps as rpi-eeprom's imager/make-release and make-recovery-images,
# without root: rpi-eeprom-config writes the settings into the bootloader
# image, rpi-eeprom-digest signs it, sfdisk/mkfs.fat/mtools make the card.
# Needs git, python3, sfdisk, mkfs.fat, mcopy, zip.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "${1:-$ROOT/out}" && cd "${1:-$ROOT/out}" && pwd)
. "$ROOT/build/lock/eeprom"     # EEPROM_REPO, EEPROM_COMMIT, EEPROM_RELEASE
CONF="$ROOT/config/eeprom/boot.conf"

W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT

echo ">> rpi-eeprom $EEPROM_COMMIT"
git init -q "$W/src"
git -C "$W/src" fetch -q --depth 1 "$EEPROM_REPO" "$EEPROM_COMMIT"
git -C "$W/src" checkout -q FETCH_HEAD
FW="$W/src/firmware-2712/default"
[ -f "$FW/pieeprom-$EEPROM_RELEASE.bin" ] || { echo "no pieeprom-$EEPROM_RELEASE.bin in firmware-2712/default" >&2; exit 1; }

echo ">> bootloader $EEPROM_RELEASE with $(basename "$CONF")"
mkdir "$W/files"
cp "$FW/recovery.bin" "$W/files/"
python3 "$W/src/rpi-eeprom-config" --config "$CONF" --out "$W/files/pieeprom.bin" "$FW/pieeprom-$EEPROM_RELEASE.bin"
sh "$W/src/rpi-eeprom-digest" -c 2712 -i "$W/files/pieeprom.bin" -o "$W/files/pieeprom.sig"
cp "$ROOT/config/eeprom/README.txt" "$W/files/README.txt"
# The settings really are in the image.
python3 "$W/src/rpi-eeprom-config" "$W/files/pieeprom.bin" > "$W/check.conf"
grep -v '^[[:space:]]*\(#\|$\)' "$CONF" | while read -r line; do
    grep -qx "$line" "$W/check.conf" || { echo "not in the image: $line" >&2; exit 1; }
done

echo ">> card image"
# One 256 MB FAT32 partition at 1 MB, like Raspberry Pi's recovery images.
start=2048 size=524288
dd if=/dev/zero of="$W/card.img" bs=1M count=258 status=none
printf 'label: dos\nunit: sectors\n\nstart=%s, size=%s, type=c\n' "$start" "$size" | sfdisk -q "$W/card.img"
dd if=/dev/zero of="$W/fat.img" bs=512 count="$size" status=none
mkfs.fat -F 32 -s 1 -n RETROBOOT "$W/fat.img" >/dev/null
mcopy -i "$W/fat.img" "$W"/files/* ::
dd if="$W/fat.img" of="$W/card.img" bs=512 seek="$start" conv=notrunc status=none
rm -rf "$OUT/bootloader" "$OUT/retroos-bootloader-rpi5.zip"
mkdir "$OUT/bootloader"
mv "$W/card.img" "$OUT/bootloader/retroos-bootloader-rpi5.img"
cp "$W/files/README.txt" "$OUT/bootloader/"
(cd "$OUT/bootloader" && zip -q "$OUT/retroos-bootloader-rpi5.zip" retroos-bootloader-rpi5.img README.txt)
(cd "$W/files" && sha256sum pieeprom.bin pieeprom.sig recovery.bin)
echo ">> $OUT/bootloader/ and $OUT/retroos-bootloader-rpi5.zip"
