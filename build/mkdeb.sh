#!/bin/sh
# Build the retroos .deb (arm64): cross-compiled binaries, generated assets
# and the system configuration overlay. Output: $OUT/repo/retroos_<ver>_arm64.deb
#
#   build/mkdeb.sh [OUT]          (default OUT=out)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/out}
SNAPSHOT=$(grep -v '^#' "$ROOT/build/lock/snapshot" | head -1)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
TARGET=aarch64-unknown-linux-gnu
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}

# Cross-compilation against the arm64 sysroot (Debian/Ubuntu multiarch libs).
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig
if [ "$(uname -m)" = aarch64 ]; then NATIVE_LD=gcc; else NATIVE_LD=aarch64-linux-gnu-gcc; fi
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-$NATIVE_LD}

echo ">> cargo build ($TARGET, release)"
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked --target $TARGET \
    -p retrodos -p retro-console -p station -p retro-post -p retro-login -p retro-session
BIN=$ROOT/target/$TARGET/release

echo ">> host tools (sounds)"
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked -p retro-synth -p retro-fontc
HOST=$ROOT/target/release

STAGE=$OUT/stage/retroos
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$OUT/repo" "$OUT/cache"
cp -a "$ROOT/packaging/retroos/root/." "$STAGE/"

install -d "$STAGE/usr/bin" "$STAGE/usr/libexec/retroos" "$STAGE/usr/share/retroos/sounds" \
    "$STAGE/usr/share/retroos/fonts" "$STAGE/usr/share/doc/retroos"
for b in retrodos retro-console station retro-session retro-greeter retro-setup; do
    install -m 755 "$BIN/$b" "$STAGE/usr/bin/$b"
done
install -m 755 "$BIN/retro-post" "$STAGE/usr/libexec/retroos/retro-post"

"$HOST/retro-synth" "$STAGE/usr/share/retroos/sounds" 2>/dev/null
"$HOST/retro-fontc" psf "$STAGE/usr/share/retroos/fonts/phosphor-mono-8x16.psf"

# GNU Unifont glyphs as the console's fallback for characters we haven't
# drawn (3.7 MB instead of the 58 MB unifont packages).
UNIFONT_DEB=unifont_15.1.01-1_all.deb
UNIFONT_SHA=$(awk -v f="$UNIFONT_DEB" '$1==f {print $2}' "$ROOT/build/lock/assets.lock")
if [ ! -f "$OUT/cache/$UNIFONT_DEB" ]; then
    curl -fsSL -o "$OUT/cache/$UNIFONT_DEB.part" \
        "https://snapshot.debian.org/archive/debian/$SNAPSHOT/pool/main/u/unifont/$UNIFONT_DEB"
    mv "$OUT/cache/$UNIFONT_DEB.part" "$OUT/cache/$UNIFONT_DEB"
fi
echo "$UNIFONT_SHA  $OUT/cache/$UNIFONT_DEB" | sha256sum -c --quiet
rm -rf "$OUT/stage/unifont" && mkdir -p "$OUT/stage/unifont"
dpkg-deb -x "$OUT/cache/$UNIFONT_DEB" "$OUT/stage/unifont"
install -m 644 "$OUT/stage/unifont/usr/share/unifont/unifont.hex" "$STAGE/usr/share/retroos/fonts/unifont.hex"

install -m 644 "$ROOT/README.md" "$ROOT/docs/DESIGN.md" "$ROOT/docs/STYLE.md" "$STAGE/usr/share/doc/retroos/"
cat > "$STAGE/usr/share/doc/retroos/copyright" <<EOF
RetroOS — GPL-3.0-or-later. Phosphor Mono font — SIL OFL 1.1.
GNU Unifont glyphs (usr/share/retroos/fonts/unifont.hex) — SIL OFL 1.1 / GPL-2+ with font embedding exception.
EOF

cat > "$STAGE/DEBIAN/control" <<EOF
Package: retroos
Version: $VERSION
Architecture: arm64
Maintainer: RetroOS contributors <retroos@users.noreply.github.com>
Section: x11
Priority: optional
Depends: libc6 (>= 2.36), greetd, libseat1, libinput10, libxkbcommon0, xkb-data, libgbm1, libpixman-1-0, libudev1, alsa-utils, systemd, sudo, nftables, iwd, fdisk
Description: RetroOS user interface
 Phosphor BIOS boot screen, the Station compositor, the RetroDOS console
 and command shell, the login screen and first-boot Setup.
EOF
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
    systemd-sysusers retroos.conf >/dev/null 2>&1 || groupadd -f admin
    systemd-tmpfiles --create retroos.conf >/dev/null 2>&1 || true
    if [ -d /run/systemd/system ]; then
        systemctl daemon-reload || true
    fi
fi
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/postinst"
# Files that other packages also ship: divert theirs aside, the Debian way.
DIVERTS="/etc/iwd/main.conf"
cat > "$STAGE/DEBIAN/preinst" <<EOF
#!/bin/sh
set -e
if [ "\$1" = install ] || [ "\$1" = upgrade ]; then
    for f in $DIVERTS; do
        dpkg-divert --package retroos --add --rename --divert "\$f.distrib" "\$f" >/dev/null
    done
fi
exit 0
EOF
cat > "$STAGE/DEBIAN/postrm" <<EOF
#!/bin/sh
set -e
if [ "\$1" = remove ] || [ "\$1" = purge ]; then
    for f in $DIVERTS; do
        dpkg-divert --package retroos --remove --rename --divert "\$f.distrib" "\$f" >/dev/null || true
    done
fi
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/preinst" "$STAGE/DEBIAN/postrm"
# conffiles: everything we ship under /etc
(cd "$STAGE" && find etc -type f | sed 's|^|/|' | sort) > "$STAGE/DEBIAN/conffiles"

# Deterministic file times.
find "$STAGE" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
DEB=$OUT/repo/retroos_${VERSION}_arm64.deb
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null
echo ">> $DEB ($(du -h "$DEB" | cut -f1))"

# A trivial local apt repository for mmdebstrap.
(cd "$OUT/repo" && dpkg-scanpackages --multiversion . /dev/null > Packages 2>/dev/null && gzip -9nkf Packages)
