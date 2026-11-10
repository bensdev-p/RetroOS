#!/bin/sh
# mmdebstrap customize hook: turn the freshly installed Debian tree into RetroOS.
#   $1 = root of the new system. Environment from mkimage.sh: RETRO_ROOT, RETRO_WORK.
set -eu
R=$1

echo ">> customize: system configuration"

# Final APT sources: the normal Debian mirrors (not the build snapshot).
rm -f "$R/etc/apt/sources.list"
cat > "$R/etc/apt/sources.list.d/debian.sources" <<'EOF'
Types: deb
URIs: http://deb.debian.org/debian
Suites: trixie trixie-updates
Components: main contrib non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: http://security.debian.org/debian-security
Suites: trixie-security
Components: main contrib non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
EOF
cat > "$R/etc/apt/sources.list.d/raspi.sources" <<'EOF'
Types: deb
URIs: http://archive.raspberrypi.com/debian
Suites: trixie
Components: main
Signed-By: /usr/share/keyrings/raspberrypi-archive-keyring.gpg
EOF
cat > "$R/etc/apt/apt.conf.d/50retroos" <<'EOF'
APT::Install-Recommends "false";
EOF
# Keep docs and translations out of future installs too.
cat > "$R/etc/dpkg/dpkg.cfg.d/01-retroos-nodoc" <<'EOF'
path-exclude=/usr/share/doc/*
path-include=/usr/share/doc/*/copyright
path-include=/usr/share/doc/retroos/*
path-exclude=/usr/share/man/*
path-exclude=/usr/share/info/*
path-exclude=/usr/share/locale/*
path-include=/usr/share/locale/locale.alias
path-exclude=/usr/share/lintian/*
EOF

echo retroos > "$R/etc/hostname"
cat > "$R/etc/hosts" <<'EOF'
127.0.0.1	localhost
127.0.1.1	retroos
::1		localhost ip6-localhost ip6-loopback
EOF
cat > "$R/etc/fstab" <<'EOF'
# RetroOS. Partitions are found by PARTUUID (disk signature 0x52455452).
PARTUUID=52455452-02  /               ext4  defaults,noatime,commit=60  0  1
PARTUUID=52455452-01  /boot/firmware  vfat  defaults,noatime,umask=0077,nofail,x-systemd.device-timeout=5s  0  2
EOF
cat > "$R/etc/default/keyboard" <<'EOF'
XKBMODEL="pc105"
XKBLAYOUT="us"
XKBVARIANT=""
XKBOPTIONS=""
BACKSPACE="guess"
EOF
echo "LANG=C.UTF-8" > "$R/etc/default/locale"
echo "Etc/UTC" > "$R/etc/timezone"
ln -sf /usr/share/zoneinfo/Etc/UTC "$R/etc/localtime"
# First boot creates the machine id.
echo uninitialized > "$R/etc/machine-id"
rm -f "$R/var/lib/dbus/machine-id"
# GTK programs (Firefox) pick their pointer theme from GTK's own settings,
# not XCURSOR_THEME, so name the Phosphor pointers there too (D-040).
# 32 px = 16 px art at the 2x art scale of a 1080p screen.
for v in 3.0 4.0; do
    mkdir -p "$R/etc/gtk-$v"
    printf '[Settings]\ngtk-cursor-theme-name=Phosphor\ngtk-cursor-theme-size=32\n' > "$R/etc/gtk-$v/settings.ini"
done
# ...and GSettings, which GTK on Wayland prefers when the GNOME schemas exist.
S="$R/usr/share/glib-2.0/schemas"
if [ -f "$S/org.gnome.desktop.interface.gschema.xml" ]; then
    printf "[org.gnome.desktop.interface]\ncursor-theme='Phosphor'\ncursor-size=32\n" > "$S/90_retroos.gschema.override"
    for c in "$R"/usr/lib/*/glib-2.0/glib-compile-schemas "$R"/usr/bin/glib-compile-schemas; do
        if [ -x "$c" ]; then chroot "$R" "${c#"$R"}" /usr/share/glib-2.0/schemas; break; fi
    done
fi
# Root has no password and cannot log in; the first user (Setup) is an admin.
chroot "$R" passwd -l root >/dev/null

echo ">> customize: services"
systemctl --root="$R" set-default graphical.target
systemctl --root="$R" enable retro-post.service retro-firstboot.service retro-setup.service greetd.service retroos-updates.timer \
    systemd-networkd.service systemd-resolved.service systemd-timesyncd.service iwd.service nftables.service
ln -sf /usr/lib/systemd/system/greetd.service "$R/etc/systemd/system/display-manager.service"
# Sound: PipeWire's sockets in every user session; it (and WirePlumber, which
# it wants) start when a program first connects (D-047). The package scripts
# normally do this; done here too so a chroot install cannot miss it.
systemctl --root="$R" --global enable pipewire.socket pipewire-pulse.socket wireplumber.service \
    >/dev/null 2>&1 || echo "!! could not enable PipeWire user units" >&2
# Bluetooth is off until the user turns it on (BT ON, Control Panel →
# Bluetooth; D-054): no bluetoothd in memory otherwise. The bluez package
# enables it when installed.
systemctl --root="$R" disable bluetooth.service >/dev/null 2>&1 || true
# Its media-button helper too (3.6 MB in every session, HW 4.25): set-bluetooth
# turns it on with Bluetooth.
systemctl --root="$R" --global disable mpris-proxy.service >/dev/null 2>&1 || true
systemctl --root="$R" mask getty@tty1.service
for t in apt-daily.timer apt-daily-upgrade.timer man-db.timer e2scrub_all.timer dpkg-db-backup.timer \
         e2scrub_reap.service systemd-networkd-wait-online.service; do
    systemctl --root="$R" mask "$t" >/dev/null 2>&1 || true
done
ln -sf ../run/systemd/resolve/stub-resolv.conf "$R/etc/resolv.conf"

echo ">> customize: boot files"
# raspi-firmware's hooks copy firmware, kernels and DTBs to /boot/firmware.
# Make sure every kernel is there under the name the firmware expects.
B="$R/boot/firmware"
mkdir -p "$B"
# The kernels ship gzip-compressed; the Pi firmware unpacks them itself,
# which took ~3 s of every Pi 5 boot (vclog: overlay loaded at 7.65 s,
# "Starting OS" at 10.90 s). Unpacked they are read from the card in well
# under a second (BUDGET.md, Milestone 3 boot breakdown).
kernel_img() {
    if gzip -t "$1" 2>/dev/null; then gzip -dc "$1" > "$2"; else cp "$1" "$2"; fi
}
for k in "$R"/boot/vmlinuz-*-rpi-2712; do [ -e "$k" ] && kernel_img "$k" "$B/kernel_2712.img"; done
for k in "$R"/boot/vmlinuz-*-rpi-v8; do [ -e "$k" ] && kernel_img "$k" "$B/kernel8.img"; done
for d in "$R"/usr/lib/linux-image-*-rpi-2712 "$R"/usr/lib/linux-image-*-rpi-v8; do
    [ -d "$d/broadcom" ] && cp "$d"/broadcom/bcm27*.dtb "$B/"
    [ -d "$d/overlays" ] && mkdir -p "$B/overlays" && cp -r "$d"/overlays/. "$B/overlays/"
done
if [ -d "$R/usr/lib/raspi-firmware" ]; then
    cp -n "$R"/usr/lib/raspi-firmware/* "$B/" 2>/dev/null || true
fi
# No initramfs on the Pi: the RPi kernel has SD, ext4 and vfat built in.
rm -f "$B"/initramfs* "$B"/initrd*
cp "$RETRO_ROOT/config/boot/config.txt" "$B/config.txt"
cp "$RETRO_ROOT/config/boot/cmdline.txt" "$B/cmdline.txt"

echo ">> customize: cleanup"
rm -rf "$R"/var/cache/apt/*.bin "$R"/var/cache/apt/archives/*.deb "$R"/var/cache/apt/archives/partial/* "$R"/var/lib/apt/lists/* "$R"/var/log/*.log "$R"/var/log/apt/* \
       "$R"/usr/share/doc/*/changelog* "$R"/usr/share/man "$R"/tmp/* "$R"/var/tmp/*
mkdir -p "$R/usr/share/man"
