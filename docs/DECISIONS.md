# Decision Log

Format: ID · date · decision · alternatives · rationale · consequences.
Status: **Proposed** until the design is approved; then **Accepted**.

---

### D-001 · 2026-10-16 · Use the Raspberry Pi Linux kernel and firmware; no custom kernel — Accepted (hard requirement)
- **Rationale**: modern GPU (V3D/Mesa), Wi-Fi/BT (brcmfmac/btbcm), USB, audio only work on a real Linux kernel.
- **Consequences**: kernel 6.12 LTS from archive.raspberrypi.com: `kernel_2712.img` (Pi 5, 16 K pages) and `kernel8.img` (Pi 4 / compatibility).

### D-002 · 2026-10-16 · Base userland: Debian 13 trixie arm64 + Raspberry Pi archive — Proposed
- **Alternatives**: Buildroot, Yocto, Alpine, pi-gen/Raspberry Pi OS Lite, rpi-image-gen, NixOS, Arch ARM.
- **Rationale**: the installer must wrap a real package source with modern browsers/emulators/games; only a mainstream binary distro provides that without us maintaining thousands of packages. Debian + RPi archive is the best-supported such source on Pi (Pi-patched Mesa, Chromium, Firefox, firmware). glibc avoids musl incompatibilities.
- **Consequences**: ~15–25 MB more idle RAM than Alpine/OpenRC; must aggressively trim default packages/services.

### D-003 · 2026-10-16 · systemd as init, cut to a custom `retro.target` — Proposed
- **Alternatives**: runit, OpenRC, s6, own PID 1.
- **Rationale**: logind (rootless compositor, seat device access), socket/D-Bus activation (PipeWire, PackageKit, polkit, udisks2 cost 0 RAM until used), parallel boot, Debian compatibility. Own init adds nothing user-visible.
- **Consequences**: ~18–22 MB for PID 1 + journald + udevd + logind + dbus-broker.

### D-004 · 2026-10-16 · Build with mmdebstrap + genimage in a pinned container — Proposed
- **Alternatives**: pi-gen (root, loop devices, heavy defaults), rpi-image-gen (same building blocks, younger, more opinionated), Buildroot/Yocto (see D-002).
- **Rationale**: fast, rootless-capable, host-arch-agnostic (qemu-user for maintainer scripts), supports snapshot pinning and `SOURCE_DATE_EPOCH`. Revisit rpi-image-gen once it supports our needs; migration would be mostly mechanical.

### D-005 · 2026-10-16 · Text mode is rendered by our compositor, not the Linux VT/fbcon — Proposed
- **Alternatives**: fbcon with custom PSF font; kmscon; a KMS console that hands DRM master to a separate kiosk compositor on `RUN`.
- **Rationale**: full control of glyphs/palette/cursor; launching full-screen GUI apps becomes "map a surface"; instant, lossless text↔GUI switching; no DRM-master hand-off races or mode-set flicker.
- **Consequences**: the compositor is always resident in text mode (budget risk R3, mitigated by D-007).

### D-006 · 2026-10-16 · Compositor built on Smithay (Rust) — Proposed
- **Alternatives**: wlroots (C), writing raw DRM/Wayland ourselves, Mir, Weston/cage as-is.
- **Rationale**: same language as the rest of the system; provides DRM/GBM/libinput/libseat, GLES + Pixman renderers, xdg/layer shell, Xwayland and X11 WM helpers, direct scan-out; used in production compositors (cosmic-comp, niri).
- **Consequences**: some features may need upstream contributions; wlroots fallback documented as a (costly) contingency.

### D-007 · 2026-10-16 · Lazy GPU initialisation in text mode, Pixman until a GPU client appears — Proposed
- **Rationale**: saves ~20 MB of text-mode idle RAM.
- **Consequences**: if renderer switching proves fragile, keep GLES always on and re-measure; report honestly against the 100 MB target.

### D-008 · 2026-10-16 · Rust for all system components and tools — Proposed
- **Alternatives**: C (+wlroots), Zig, C++.
- **Rationale**: memory safety for untrusted-input parsers; strong crates (Smithay, zbus, alacritty_terminal, ratatui); small fast binaries with LTO; clean aarch64 cross-compiles.

### D-009 · 2026-10-16 · Our own GUI toolkit (retro-ui), software-rendered — Proposed
- **Alternatives**: GTK/Qt with a retro theme, iced/egui/slint.
- **Rationale**: pixel-exact bitmap UI, ~10 ms startup, low RAM, no GPU context per app; general toolkits fight integer-pixel bitmap rendering.
- **Consequences**: we own widget code; accessibility (AT-SPI) is a gap (R10).

### D-010 · 2026-10-16 · Global System Bar without global app menus — Proposed
- **Rationale**: Mac-style bar for system menu/launcher/status; app menus stay in-window like Win 3.1 because modern apps have in-window menus and global-menu protocols are fragile.

### D-011 · 2026-10-16 · iwd + systemd-networkd + resolved instead of NetworkManager — Proposed
- **Rationale**: ~3× less RAM, clean D-Bus API for our own UIs; covers Ethernet/Wi-Fi consumer cases.
- **Consequences**: no VPN/mobile-broadband UI at 1.0; WPA-Enterprise via advanced dialog.

### D-012 · 2026-10-16 · PackageKit (apt backend) + curated catalog for the Software Library — Proposed
- **Alternatives**: calling apt directly from a privileged helper we write; aptdaemon.
- **Rationale**: proven privileged backend with progress, locking and polkit auth; D-Bus activated, exits when idle (0 MB idle); avoids writing our own root daemon.

### D-013 · 2026-10-16 · Drive letters (C: = home, Z: = root) with ProDOS/Mac volume names — Proposed
- **Alternatives**: ProDOS-style `/VOLUME/PATH` only.
- **Rationale**: drive letters make the `C:\>` prompt and DOS commands feel right; volume names add the Mac/ProDOS flavour in `DIR` headers and the desktop; `Z:` gives power users the whole Linux tree.

### D-014 · 2026-10-16 · Integer scaling only — Proposed
- **Rationale**: keeps bitmap fonts/icons pixel-exact; modern apps render crisply at 2×/3×/4×.
- **Consequences**: on some monitor sizes the logical desktop is larger/smaller than ideal; user can pick a lower scale.

### D-015 · 2026-10-16 · All fonts, icons, cursors and sounds are original and built from text/code sources — Proposed
- **Rationale**: cohesive alternate-history identity, no licensing ambiguity, diff-reviewable assets, reproducible builds. GNU Unifont used only as a fallback for glyphs we haven't drawn.
