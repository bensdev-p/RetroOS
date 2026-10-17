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

---

## Decisions made during Milestone 1

### D-016 · 2026-10-16 · The console advertises TERM=xterm-256color, not a custom `retro` terminfo — Accepted
- **Alternatives**: ship a `retro` terminfo entry (DESIGN.md §7.1 originally).
- **Rationale**: retro-console emulates xterm faithfully (alacritty_terminal), and a custom TERM breaks ssh and any remote host without the entry. RetroDOS detects the RetroOS console through `$RETROOS_CONSOLE` instead. `RETROOS_TERM` overrides for experiments.

### D-017 · 2026-10-16 · Login screen and first-boot Setup are text programs inside Station — Accepted
- **Alternatives**: a separate KMS dumb-buffer greeter (DESIGN.md §5.2 originally).
- **Rationale**: one input stack (libinput + xkb keymaps, so non-US passwords work), one renderer, one look. greetd runs `station --exec "retro-console --exec retro-greeter"`; first-boot Setup runs the same way as a root systemd service with a logind session (PAMName=login, TTYPath=/dev/tty1).
- **Consequences**: the login screen costs one Station start (~0.1 s); only `retro-post` (which runs before any session exists) draws on KMS directly.

### D-018 · 2026-10-16 · Debian packages come from snapshot.debian.org, pinned by timestamp — Accepted
- **Rationale**: this was already the reproducibility plan (DESIGN.md §13.3), and it is also the only Debian mirror reachable from the development environment. The built image itself points at the normal mirrors (deb.debian.org, security.debian.org, archive.raspberrypi.com).
- **Consequences**: `build/lock/snapshot` names the timestamp; keyrings are committed under `build/keyrings` with their source package hashes in `build/lock/keyrings.lock`.

### D-019 · 2026-10-16 · Disk layout addressed by a fixed MBR signature, no initramfs on the Pi — Accepted
- **Rationale**: `root=PARTUUID=52455452-02` works without an initramfs (LABEL= does not), and the RPi kernels have SD/ext4/vfat built in, saving ~0.5–1 s of boot. The signature spells "RETR".
- **Consequences**: images are deterministic in layout; the first boot grows partition 2 to fill the card (`retro-firstboot.service`).

### D-020 · 2026-10-16 · Kernel is whatever the Raspberry Pi archive ships for trixie (currently 6.18) — Accepted
- **Rationale**: DESIGN.md said 6.12 LTS, but the RPi trixie archive has moved to 6.18 (`linux-image-rpi-2712` 1:6.18.50). Tracking the vendor kernel is what gives working Wi-Fi/BT/V3D.

### D-021 · 2026-10-16 · Build-time assets come from code, not hand-made binary files — Accepted
- **Rationale**: sounds are synthesised by `tools/retro-synth` (patches are Rust functions rather than `.synth` text files, which bought nothing over code), the font is a text glyph source compiled at build time, and GNU Unifont's `.hex` is extracted from its Debian package (pinned) as the console's fallback for undrawn glyphs — 3.7 MB instead of the 58 MB `unifont` packages.

### D-022 · 2026-10-17 · Batch files: MS-DOS 6 semantics plus a few NT conveniences — Accepted
- **Rationale**: users' muscle memory and old scripts are DOS 6 (`%1`, `SHIFT`, `IF ERRORLEVEL n`, `CHOICE /C:`), but `EXIT /B`, `IF /I`, `IF DEFINED`, `::` comments and `&&`/`||` cost nothing and are what people reach for today. ERRORLEVEL follows COMMAND.COM: programs set it, built-ins that were internal commands (ECHO, IF, SET, CD, …) leave it alone when they succeed; `&&`/`||` look at the last command's own status instead. Running a batch file from another one without CALL returns to the caller, as if CALLed.
- **Consequences**: `C:\AUTOEXEC.BAT` (shipped in `/etc/skel`) runs when the top-level shell starts; `RETROOS_SAFE_MODE=1` skips it. Batch files are not supported as stages of a pipeline yet.

### D-023 · 2026-10-17 · Station renders with GLES when a hardware GPU is present, Pixman otherwise — Accepted
- **Rationale**: DESIGN.md §6.3 wanted lazy GL (Pixman until the first GPU client). Switching renderers mid-session means re-creating the DRM compositor and re-importing every client buffer; not worth the fragility. Instead the choice is made once at startup: GBM + EGL + GLES on a real GPU (v3d through vc4's kmsro on the Pi), with `zwp_linux_dmabuf_v1` v4 feedback naming the render node so Mesa clients (Firefox) render on the GPU; Pixman into dumb buffers when EGL is missing or software-only (llvmpipe — QEMU without virgl), where it is cheaper than emulated GL. `RETROOS_RENDERER=pixman` forces the CPU path.
- **Consequences**: text mode on the Pi pays the GLES context cost (~10–20 MB, to be measured on hardware — BUDGET.md). If that breaks the 100 MB budget, the M5 fallback is to start text sessions on Pixman and restart Station on GLES when the first GPU app launches. Mesa (`libgl1-mesa-dri` etc.) is now in the image.

### D-024 · 2026-10-17 · USB storage is mounted by a udev rule + systemd template, not udisks2 — Accepted
- **Rationale**: udisks2 brings libblockdev, its plugins and a resident daemon for what RetroOS needs today: "a USB stick appears as D:". A udev rule starting `retro-automount@<dev>.service` (a 70-line shell helper) mounts USB file systems at `/media/<user>/<LABEL>` for the person at the screen, owned by them for FAT/exFAT/NTFS, `nosuid,nodev`; the unit is bound to the device, so pulling the stick unmounts it. `EJECT D:` stops the unit, which a polkit rule allows for the active local user and nothing else.
- **Consequences**: no desktop-style mount API for GUI apps (file choosers still see `/media/<user>`). If M3/M4 needs udisks2 (formatting, SMART, GVfs integration), it replaces this helper and RetroDOS keeps reading `/media/<user>`.

### D-025 · 2026-10-17 · Xwayland moves from M2 to M3 — Accepted
- **Rationale**: every program RetroOS preinstalls or offers one-click in M2 (Firefox, Chromium, GTK 3/4, Qt 5/6 and SDL 2 apps) runs natively on Wayland. X11-only programs need Xwayland *and* an X11 window manager (XWM: mapping, stacking, transient dialogs, override-redirect menus); M3 builds that WM anyway for the desktop's decorations, and it is far easier to test there (the M2 test environment cannot install X11 clients). Shipping a text-mode-only half of it now would be thrown away.
- **Consequences**: in M2 an X11-only program launched from RetroDOS fails with its own "cannot open display" message. M3 adds Xwayland, started on the first launch of a program that links libX11/libxcb but not libwayland-client (RetroDOS already reads DT_NEEDED for GUI detection), with `DISPLAY` set for it.

### D-026 · 2026-10-17 · M2 versions of EDIT, FILER and WIFI are deliberately smaller than DESIGN.md §8 — Accepted
- **What is in**: EDIT — one file at a time, menus, mouse, selection, clipboard, undo, find/change/go to line, line-end choice; FILER — two panels, view/edit/copy/move/mkdir/delete, drives, marks; WIFI/NET — wired status, scan/join/forget/hidden networks with WPA2/3-Personal.
- **Moved later**: EDIT multiple buffers and syntax colouring (M4 polish); FILER archive browsing (zip/tar, M4 with the Software Library's archive code); Wi-Fi Enterprise (802.1X), static IP and regulatory country (M4 Control Panel → Network, which shares `retro-sys::net`).
- **Rationale**: each moved item is self-contained and none blocks the M2 acceptance tests (Wi-Fi join on hardware, Firefox from the prompt). Time went to the launch broker, GPU path and Task List instead.

### D-027 · 2026-10-17 · Programs get output scale 1 up to 1440p; bitmap art scales itself — Accepted (supersedes part of DESIGN.md §6.3)
- **Rationale**: §6.3 gave every client the scale that keeps a ≥ 960×540 logical desktop, i.e. 2× at 1080p. On the first hardware test (Pi 5, 15.6" 1080p panel) that made Firefox a 960×540 browser: "screen is huge, unusable". The ≥ 960×540 rule is right for our bitmap text, not for modern apps sized in points.
- **Decision**: Station reports scale `floor(min(w/1920, h/1080))` (1 up to 1440p, 2 at 4K), overridable with `ui_scale=` in `/etc/retroos/startup.conf`. retro-console, the Task List and the POST/Setup screens already pick their own integer pixel scale from the physical size, so the text look is unchanged; Station's mouse arrow keeps the old 2× pixel size at 1080p.
- **Consequences**: M3's desktop personality will use the same split (our UI at its pixel scale, apps at the client scale). A Control Panel → Display setting for `ui_scale` comes in M4.

### D-028 · 2026-10-17 · The idle-RAM budget counts RetroOS's own memory — Accepted (revises DESIGN.md §16)
- **Finding**: on the first Pi 5 (8 GB) measurement, 279 MB "used" at an idle prompt broke down into ≈ 46 MB of watermark reserve (free memory MemAvailable does not count; it grows with RAM), ≈ 130 MB of kernel and driver memory from the stock Raspberry Pi kernel (slab, vmalloc, firmware and DMA buffers), and 93.5 MB of userspace (AnonPages + Shmem). The kernel share alone exceeds the original 100 MB target, and the brief rules out a custom kernel.
- **Decision**: the text-mode target becomes **RetroOS userspace (AnonPages + Shmem) < 60 MB**, reported with MemTotal − MemAvailable (target < 200 MB) for comparison. GUI mode: userspace < 160 MB. BUDGET.md reports all three numbers.
- **Consequences**: cuts are aimed where we have control: the console's frame buffers, GLES only while a graphical program runs, and resident services.

### D-029 · 2026-10-17 · GPU renderer only while graphical programs run — Accepted (revises D-023)
- **Rationale**: the Pi measurement (D-028) showed Station at 103 MB RSS on GLES against 17 MB on Pixman; at the prompt the GPU buys nothing (one shm buffer, enlarged).
- **Decision**: Station starts on Pixman. The launch broker switches it to GLES (creating the linux-dmabuf global) just before it starts a graphical program, so the program sees dmabuf when it connects; ~10 s after the last graphical program and its processes are gone, Station drops GLES and the global. A failed GLES attempt (no GPU, software-only Mesa) is remembered and not retried. The switch re-creates the DRM surface, which may blank the screen for a frame. `renderer=gpu|cpu|auto` in `/etc/retroos/startup.conf` (or `RETROOS_RENDERER`) pins the choice.
