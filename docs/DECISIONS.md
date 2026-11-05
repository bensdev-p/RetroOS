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
- **Decision**: Station starts on Pixman. The launch broker switches it to GLES (creating the linux-dmabuf global) as it starts a graphical program (after the spawn, so Mesa's start-up overlaps the program's; the program cannot connect before Station returns to its event loop, so it sees dmabuf from its first request); ~60 s after the last graphical program and its processes are gone, Station drops GLES and the global. A failed GLES attempt (no GPU, software-only Mesa) is remembered and not retried. The switch re-creates the DRM surface, which may blank the screen for a frame. `renderer=gpu|cpu|auto` in `/etc/retroos/startup.conf` (or `RETROOS_RENDERER`) pins the choice.

---

## Decisions made during Milestone 3

### D-030 · 2026-10-17 · Desktop window management: what differs from STYLE.md §4 and DESIGN.md §5.4 — Accepted
- **Move/resize outline**: a Black line inside a White one instead of an XOR rectangle. XOR needs the pixels under the outline read back, which neither the Pixman nor the GLES path does cheaply; a two-tone line is visible on any background and costs eight solid rectangles.
- **Fonts**: frames, menus, boxes and icon labels use Lantern 12 (`assets/fonts/lantern-12.glyphs`, drawn for M3: proportional, bold, ASCII plus the UI symbols … ◆ ✓ ▸). Icon labels use Lantern 9 (`assets/fonts/lantern-9.glyphs`, added later in M3). Latin-1 letters for both Lantern fonts are composed from the drawn base letter and a drawn mark (`retro_gfx::font::accents`): an accented capital gives up repeated rows (two in Lantern 12, one in Lantern 9) so that its mark fits in the 13-/9-row cell, so É is slightly shorter than E; characters a font lacks show the replacement box.
- **Exit to DOS** is `Ctrl+Alt+F1` (DESIGN.md §5.4; the session's own VT is 1, so nothing is lost) and the broker request `PERSONALITY text`. The confirmation dialog with "Close all programs first" comes with the ◆ menu of the System Bar (M3 step 4), which will call the same request.
- **The main DOS Prompt cannot be closed** from its frame or with Alt+F4: closing it would end the session. Station shows a message box ("type EXIT at its prompt") instead. Other DOS Prompt windows close normally.
- **DOS Prompt windows keep their text size** when resized (more or fewer cells), instead of re-picking the largest scale that fits 80×25 as the full screen does.
- **Interim icons**: minimised windows use two icons drawn in code (a program window, a DOS Prompt) until the icon set (M3 step 7).
- **Consequences**: frames, the desktop pattern, icons and menus are Station's own drawing (desktop.rs, chrome.rs), cached per window and redrawn only when their state changes; the dithered desktop is one 64-pixel band drawn repeatedly (~1 MB at 1080p).

### D-031 · 2026-10-17 · Station frames only windows that ask for server-side decorations — Accepted
- **Rationale**: programs that bind `xdg-decoration` (retro-console, Qt, SDL, foot, mpv…) get our Windows 3.1 frame. GTK 3/4 programs, Firefox included, do not use that protocol and draw their own title bars (client-side decorations); framing them too would show two title bars. Their title-bar drags and border resizes arrive as `xdg_toplevel.move`/`resize` requests, which Station runs with the same outline, and maximise/minimise/full-screen requests work as for framed windows.
- **Consequences**: Firefox and GTK programs look like themselves until the GTK theme (M4) styles their title bars. A later option: the KDE server-decoration protocol, which GTK 3 programs without a custom title bar honour (Firefox keeps its own while tabs are in the title bar).

### D-032 · 2026-10-19 · Layer shell in Station; the System Bar is a client; desktop pattern and icons stay in Station — Accepted (M3 step 2, HANDOFF §7)
- **Decision**: Station implements wlr-layer-shell (`layers.rs`): Top/Overlay layers draw above windows (under a full-screen window, Top hides), Bottom/Background below them; exclusive zones shrink the work area, and windows whose title bar ends up under a new zone move down out of it. The **System Bar** (`retro-bar`) is a layer-shell client built on the new **retro-ui** toolkit; Station starts it with the Desktop (restarting it up to three times) and hides it in RetroDOS. The **dithered desktop and minimised-window icons** stay Station's own drawing: they are window-manager state, cost no process, and must exist before any client has started.
- **Alternatives**: draw the System Bar in Station like the Task List (no extra process, ~3.8 MB RSS saved) — rejected because the bar needs the application list, the clock, network and volume status (D-Bus, M4) and dialogs, which do not belong in the compositor's address space; a crash there should not take the display down.
- **Consequences**: menus are xdg popups with grabs (a click elsewhere closes them; Station now ends a stale menu grab when it focuses a window and re-sends the motion a closing grab swallows). Not yet: opening the ◆ menu with the Super key (Station has no channel to the bar yet), switching menus by sliding along the bar, keyboard repeat in retro-ui, a cursor protocol (Station resets the pointer to its arrow whenever the pointer moves to another program). Menu and button drawing moved to `retro_gfx::widgets` so Station's window menu and retro-ui menus are the same code.

### D-033 · 2026-11-04 · One colour scheme for the whole system; COLOR sets it — Accepted
- **Request** (tester, 2026-11-04): changing the DOS colours should re-theme all of RetroOS, not one console. STYLE.md §2 already pairs every text scheme with a GUI look; this makes them one setting.
- **Decision**: `scheme=` in `/etc/retroos/startup.conf` (classic, paper, amber, green, high) is the system's scheme. `COLOR <scheme>` switches its own console, asks Station (`SCHEME` broker request) to switch the session, and saves it with `/usr/libexec/retroos/set-scheme`, which the admin group may run without a password (sudoers; the helper accepts only the five names and rewrites one line). Station keeps the session's scheme, redraws its frames, menus, desktop pattern, icons and Task List (`retro_gfx::Theme`, part of the drawing metrics, so every cache is redrawn), passes it to the programs it starts (`RETROOS_SCHEME`), and tells every program that asked (`WATCH`): each console (and so EDIT, FILER and every text program in it, which draw with palette indices) and every retro-ui program (the System Bar). The POST screen, the Startup Menu, DEL Setup, the login screen and first-boot Setup read the saved scheme at start. `COLOR 1E` (attributes) still affects only its own console.
- **Alternatives**: a per-user setting (`~/.config`) — rejected because the POST and login screens run before anyone logs in; asking for the admin password on every COLOR — rejected as hostile for a colour change.
- **Consequences**: a non-admin user's COLOR changes the session only (RetroDOS says so). Monochrome schemes draw pictures (icons, the POST logo) in four brightness steps of the scheme. Firefox and GTK programs keep their own look until third-party theming (M4) follows the same setting. DEL Setup now keeps lines of startup.conf it does not edit (it used to drop `renderer=`, `ui_scale=`).

### D-034 · 2026-11-05 · The fast POST lasts ~2.5 s; a CI-built card sets the Pi 5 bootloader — Accepted
- **Fast POST**: the tester's Pi 5 never showed the 1 s fast POST: the monitor re-syncs to the display driver's mode at about the time it starts (the slow ~3.5 s POST was visible). At the tester's request the fast POST is now ~2.5 s (memory count 0.9 s, 0.15 s per device line, 1 s hold): +1.5 s of boot time for a POST screen that can be seen. The slow POST is unchanged; F8/DEL still work throughout.
- **Bootloader settings card**: most of the Pi 5's boot time is its EEPROM bootloader (BUDGET.md: `Starting OS` at 20.6 s with the 2026-10-30 release and Imager's *SD Card Boot* settings, which show the network-install screen after every cold boot and print to the UART). RetroOS cannot change the EEPROM from the running system (no `rpi-eeprom-config` or bootloader images in the image). CI builds `retroos-bootloader-rpi5.zip` (`build/mkeeprom.sh`): Raspberry Pi's own recovery image process (`rpi-eeprom` pinned to a commit, the same `pieeprom` release, `recovery.bin`) with `config/eeprom/boot.conf` (`BOOT_ORDER=0xf41`, `BOOT_UART=0`, `NET_INSTALL_AT_POWER_ON=0`, `NET_INSTALL_ENABLED=0`). Flash it to a spare card with Imager → *Use custom*, boot it once, then put the RetroOS card back.
- **Alternatives**: `rpi-eeprom` in the image with an ADMIN command (~10 MB of bootloader images on every card, and a firmware writer in the normal system); asking testers to use Raspberry Pi OS (`rpi-eeprom-config --edit`) — still fine, and documented.
- **Consequences**: Shift at power-on no longer starts a network install on a board with these settings (Space still opens the boot menu); Imager's *SD Card Boot* restores Raspberry Pi's defaults.

### D-035 · 2026-11-05 · Program Manager: what M3 ships and how it starts — Accepted (M3 step 4, DESIGN.md §9.1–9.2)
- **Decision**: `retro-progman` (retro-ui) is one window with a File / Options / Window / Help menu bar and Windows 3.1 group windows drawn inside it: **Main** (DOS Prompt; File Cabinet and Control Panel join it as they arrive) and one group per category of the installed `.desktop` files (`retro_apps::group_of`, the System Bar's Programs groups). Double-click or Enter starts a program through Station's broker; arrows move in a group; Ctrl+Tab / Ctrl+F6 go to the next group; Alt+letter or F10 open the menus; the wheel and Page Up/Down scroll. Options → Minimize on Use is kept in `~/.config/retroos/progman.conf`.
- **Starting**: Station starts it once per session, with the first Desktop (not again after the user closes it; ◆ → Program Manager reopens it, and a second copy brings the first forward). Its first window opens **behind** the window that has the keyboard (the DOS Prompt that typed DESKTOP keeps typing) and at the **top right** of the work area, outside the cascade, so the DOS Prompt and the next windows keep their places. `RETROOS_NO_PROGMAN=1` leaves it out (host tests of other features).
- **Not yet** (vs. Windows 3.1): group windows are tiled by the program (Window → Tile), not moved, resized or minimised by the user; icons cannot be dragged between groups; File → Run… and Help → About are shown disabled; icons are the two drawn in code (`retro_gfx::icons`, moved there from Station) until the icon set (step 7), which also brings each program's own icon.
- **Consequences**: a session in the Desktop has one more task (TASKS counts it); its idle cost is one retro-ui process with a 1× shm buffer (measured with the other GUI numbers at acceptance).

### D-036 · 2026-11-05 · About This Computer counts each program's process tree — Accepted (M3 step 5)
- **Decision**: `retro-about` lists every program Station has (TASKS) with the proportional set size (PSS) of its process and all its descendants (`/proc/<pid>/smaps_rollup`), so Firefox's content processes count towards Firefox, and shared libraries are split between their users instead of counted twice. The first row, **RetroOS**, is the memory in use (MemTotal − MemAvailable, the BUDGET.md "used" figure) minus the programs: kernel, Station, the System Bar, services. Bars show each row's share of the memory in use. DESIGN.md §9.2 said "from cgroups": Station does not put programs in their own cgroups yet, and the process tree gives the same grouping today.
- **Consequences**: values match `ps`/smem-style PSS, not RSS (smaller for programs sharing libraries). Processes another user owns read as 0 and are left out. Fixed window of eight rows; more programs are cut (a list that scrolls comes with the retro-ui list widget).

### D-037 · 2026-11-05 · Jot: Notepad's job with EDIT's buffer — Accepted (M3 step 5)
- **Decision**: `jot` (crate `retro-jot`) is the Desktop's text editor in the manner of Windows 3.1 Notepad: File New / Open… / Save / Save As…, Edit Undo / Cut / Copy / Paste / Delete / Select All, Search Find… / Find Next, the usual keys (Ctrl+Z/X/C/V/A/S/O/N/F, F3, Shift+Del, Ctrl+Ins, Shift+Ins, Ctrl+arrows), mouse selection with drag, Windows 3.1 scroll bars, and a "save the changes?" question before New, Open and Exit. It edits with EDIT's own buffer (`retro_edit::buffer`, now a library as well as EDIT's program), so tabs, CR LF files and saving (write beside, then rename) behave the same in both editors.
- **Font**: Phosphor Mono 8×16 (STYLE.md §4 lists Jot with the DOS Prompt), not Lantern: text files are column-aligned, and one cell per character keeps tabs, selection and the cursor simple.
- **Dialogs**: Open / Save As is a File name field, the directory, and a list of `[folders]` and files (Tab between them, double-click or Enter opens a folder); no drive list or file-type filter (File Cabinet's drive bar and the mounts of M4 come first). Find is modeless with Match case.
- **Toolkit**: retro-ui gained `controls` (menu bar, text field, list box, check box), dialogs with a parent (Station centres them), `set_title`, keyboard repeat and the clipboard (wl_data_device; text only).
- **Not yet**: Word Wrap, Print, Help, Replace, Go To, drag and drop of files, a Unicode font fallback beyond Phosphor Mono's characters (others show the replacement box).

### D-038 · 2026-11-05 · File Cabinet: one folder per window, FILER's file code, the freedesktop.org trash — Accepted (M3 step 5)
- **Decision**: `file-cabinet` (crate `retro-cabinet`) shows one folder: a drive bar with the RetroDOS drives (`retro_dosfs::Dos`: C: the home folder, D: and on removable media, Z: the system), the folder as a DOS path (also the window title), and its files as a **list** (name, size, date) or **icons**, with a status line. Enter or a double-click opens a folder, or a file with the first program whose `.desktop` file lists its type (`MimeType=`, now parsed by retro-apps; types from a built-in extension table), and text (or a small file that reads as UTF-8) in Jot. File: Open, New Folder…, Rename… (F2), Delete… (Del; Shift+Del deletes for good), Get Info (Alt+Enter, folder sizes counted); Edit: Cut, Copy, Paste of files (the paths also go on the text clipboard), Select All; View: Icons / List, Hidden Files, Refresh (F5, also finds new USB drives); Backspace goes up; typing a letter jumps. The listing and copy/move/delete are FILER's (`retro_filer::panel`, now a library), so both file managers behave the same.
- **Wastebasket**: Delete moves to `~/.local/share/Trash` with a `.trashinfo` file (the freedesktop.org Trash spec), so other programs' trash views show the same files. Removable drives have none: File Cabinet then asks to delete for good.
- **Not yet**: the desktop's drive and Wastebasket icons (DESIGN.md §9.1) and a Wastebasket window to restore files; drag and drop; a progress dialog (copies run in the window's own thread, so a big copy freezes it until done); open-with choices; per-folder view settings.
- **Consequences**: Program Manager's Main group has File Cabinet first, then DOS Prompt.
