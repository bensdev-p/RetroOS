# RetroOS — Design Document

Status: **DRAFT for review** (pre-M1). Nothing here is implemented yet.
Companion docs: [`STYLE.md`](STYLE.md) (visual/audio style guide), [`DECISIONS.md`](DECISIONS.md) (decision log).

---

## 0. Summary

RetroOS is a Raspberry Pi 5/4 (arm64) image that boots, in a few seconds, to a
1980s-style BIOS POST screen and then to **RetroDOS**, a DOS/ProDOS-flavoured
command shell. Typing `DESKTOP` turns the same session into a Windows 3.1 /
System 7 mashup desktop. Real modern software (Firefox, Chromium, RetroArch, mpv,
Geany, VS Code…) runs in both modes: full-screen from the prompt, windowed on the
desktop.

The fictional vendor is the **Phosphor Computer Company** ("PCC"), and the product
is **RetroOS 1.0**, which ships **RetroDOS** (text) and **RetroOS Desktop** (GUI).

The short version of the architecture:

| Layer | Choice |
|---|---|
| Kernel + firmware | Raspberry Pi kernel (6.18 at M1; `linux-image-rpi-2712` / `-v8`), Pi firmware, from archive.raspberrypi.com |
| Userland base | Debian 13 "trixie" arm64, *minimal* (no desktop task, no X server by default) |
| Image builder | `mmdebstrap` + hooks + `genimage`, run inside a pinned container; one command: `make image` |
| Init | systemd, cut to a small custom target |
| Login | `greetd` + our own text-mode greeter |
| Display | Wayland. Our own compositor **Station** (Rust, Smithay). Xwayland on demand |
| Text mode | Station in *text personality*: full-screen console client + full-screen apps, DOS-5-style task swapper |
| GUI mode | Station in *desktop personality*: windows, system bar, Program Manager, File Cabinet… |
| Shell | **RetroDOS `COMMAND`** (Rust), runs in a pty inside our terminal client |
| Our apps | Rust; TUI apps on `ratatui`, GUI apps on our own tiny toolkit **retro-ui** (software-rendered) |
| Networking | `iwd` (Wi-Fi) + `systemd-networkd` (Ethernet) + `systemd-resolved` |
| Bluetooth | BlueZ |
| Audio | PipeWire + WirePlumber (socket-activated per user) |
| Packages | apt (Debian + Raspberry Pi archives) via PackageKit (D-Bus activated, exits when idle); curated catalog on top |
| Privilege | polkit (on demand) + `admin` group; root locked |

Key idea: **one compositor process, two personalities.** Text mode is *not* the
Linux VT console; it is our compositor showing a full-screen terminal surface and
no window chrome. That makes "launch Firefox from the prompt" a trivial
operation (map a new full-screen surface), makes text↔GUI switching instant and
lossless (your DOS sessions become "DOS Prompt" windows; your windows become
task-swapper entries), and avoids fragile DRM-master hand-offs between
processes. Details in §6.

---

## 1. Goals and non-goals

**Goals**
- Authentic, cohesive 1985–1993 look and feel; modern capability underneath.
- Text mode and GUI mode are both first-class; modern GUI apps launch from the prompt.
- Lean: boot-to-prompt < 10 s on Pi 5; idle RAM < 100 MB (text) / < 200 MB (GUI).
- Single flashable `.img`, reproducible one-command build, QEMU-testable.
- Sane security: non-root user, signed package sources, updates, firewall.

**Non-goals**
- Writing a kernel, a libc, a GPU driver, a network stack, or a package manager.
- Running real DOS/Mac binaries natively (DOSBox-X / Basilisk II / Mini vMac are installable apps).
- Pixel-perfect clones of any real product. We borrow *idioms*, not artwork.
- Animations, transparency, blur, compositing effects.

---

## 2. Base system choice

### 2.1 Options evaluated

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **Buildroot** | Tiny, fast boot, total control, great for appliances | **No runtime package manager** → fails "install a modern browser" requirement; cross-building Firefox/Chromium in Buildroot is hours and fragile; we'd maintain every app ourselves | ✗ |
| **Yocto** | Industrial-grade, reproducible | Same package-feed problem unless we host feeds for thousands of packages; very heavy to maintain | ✗ |
| **Alpine (linux-rpi)** | Small, apk is fast, OpenRC idle ~40 MB | musl breaks/limits binary software (Widevine, some games, box64/Steam); Pi-optimised Chromium/Firefox/Mesa patches arrive late or never | ✗ (close second) |
| **Raspberry Pi OS Lite via pi-gen** | Best hardware support | pi-gen is a root-only, loop-device, bash-stage pipeline; drags in a lot of default packages we'd then remove | ✗ as a *tool* |
| **rpi-image-gen** (RPi's newer mmdebstrap+genimage tool) | Official, layer-based, same building blocks we want | Young; host-arch and layout assumptions; another layer of YAML between us and mmdebstrap | Watch; may adopt later (D-004) |
| **Debian trixie arm64 + Raspberry Pi archive, built with mmdebstrap** | Huge arm64 catalog (the installer can offer real software on day one); glibc; Pi-patched kernel/firmware/Mesa/Chromium/Firefox from RPi archive; mmdebstrap is fast, rootless-capable, supports `SOURCE_DATE_EPOCH` and snapshot pinning | systemd + Debian conventions cost ~15–25 MB more idle RAM than Alpine/OpenRC | ✓ **Chosen** |
| NixOS / Arch ARM | Reproducible (Nix) / rolling | Nix closure sizes and Pi kernel support friction; Arch ARM dropped official Pi 5 focus | ✗ |

### 2.2 Decision

**Debian 13 "trixie" arm64, minimal, plus the Raspberry Pi apt archive for kernel,
firmware, and Pi-optimised Mesa/Chromium/Firefox/libcamera.** Built with
`mmdebstrap`. This is essentially "Raspberry Pi OS Lite's foundation, assembled
by us, with nothing above the kernel/libraries that we didn't choose."

Why this wins: the hard requirement that drives everything is *"install a modern
browser, games, emulators"* through a retro front-end wrapping a *real* package
source. Only a mainstream binary distribution satisfies that without us becoming
a distribution maintainer. Debian + RPi archive is the best-supported such
source on Pi hardware. The RAM cost of systemd is affordable inside the budget
(see §16).

---

## 3. Architecture overview

```
 ┌────────────────────────────── user session (uid=user) ───────────────────────────────┐
 │                                                                                       │
 │   ┌──────────────┐  ┌──────────────┐  ┌───────────┐  ┌───────────────────────────┐    │
 │   │ retro-console│  │ System Bar,  │  │ Firefox,  │  │ COMMAND (RetroDOS shell)  │    │
 │   │ (terminal    │  │ Programs,    │  │ mpv, games│  │ EDIT, FILER, CONTROL...   │    │
 │   │  client)     │◄─┤ File Cabinet,│  │ (Wayland/ │  │ (run inside console ptys) │    │
 │   │  owns ptys ──┼──┼──────────────┼──┼───────────┼─►│                           │    │
 │   └──────┬───────┘  │ Control Panel│  │ Xwayland) │  └────────────┬──────────────┘    │
 │          │          └──────┬───────┘  └─────┬─────┘               │ launch broker     │
 │          │ wayland         │ wayland        │ wayland             │ (unix socket)     │
 │   ┌──────▼─────────────────▼────────────────▼─────────────────────▼──────────────┐    │
 │   │                    STATION  (compositor, Rust/Smithay)                       │    │
 │   │   personality: TEXT (full-screen + task swapper)  |  DESKTOP (windows)       │    │
 │   │   renderer: GLES2 (V3D / virgl)  ── fallback: Pixman (CPU)                   │    │
 │   └──────┬───────────────────────────────┬───────────────────────────────────────┘    │
 │          │ libseat/logind                │ libinput                                   │
 │   PipeWire+WirePlumber (socket-activated)   systemd --user   dbus (user bus)          │
 └──────────┼───────────────────────────────┼───────────────────────────────────────────┘
            │                               │
 ┌──────────┴─────── system ────────────────┴───────────────────────────────────────────┐
 │ systemd · journald(volatile) · udevd · logind · dbus-broker · greetd(+retro-greeter) │
 │ iwd · networkd · resolved · timesyncd · bluetoothd(on demand) · nftables             │
 │ on-demand (D-Bus activated, exit when idle): polkit · PackageKit · udisks2           │
 │ retro-post (boot screen, runs once)                                                  │
 ├──────────────────────────────────────────────────────────────────────────────────────┤
 │ Linux 6.18 (RPi) · vc4/v3d DRM · brcmfmac · hci_uart/btbcm · snd · xhci · hid…       │
 │ Pi firmware / EEPROM bootloader                                                      │
 └──────────────────────────────────────────────────────────────────────────────────────┘
```

Everything the user *sees* is ours: retro-post, retro-greeter, Station (incl.
window decorations, cursors, task swapper, dialogs), retro-console (text
rendering), COMMAND and all TUI/GUI apps, themes for third-party apps, fonts,
icons and sounds.

---

## 4. Boot sequence and init

### 4.1 Timeline (Pi 5 target)

| t (approx) | Stage | What the user sees |
|---|---|---|
| 0.0 s | EEPROM bootloader, reads SD | black (rainbow splash disabled: `disable_splash=1`) |
| ~1.8 s | `start4.elf`/firmware loads `kernel_2712.img` + DTB (no initramfs) | black |
| ~2.0 s | kernel; `quiet loglevel=0 logo.nologo vt.global_cursor_default=0 fbcon=nodefer` with fbcon mapped off the display (`fbcon=map:9`) | black |
| ~3.0 s | systemd starts; `retro-post.service` is ordered *very* early (after `systemd-udevd`, before most of `sysinit.target`) | **POST screen** (§4.2) + chime |
| ~3–5 s | services come up in parallel behind the POST screen | memory count, device list |
| ~5 s | greetd → (autologin or retro-greeter) → user session → Station + console | `C:\>` prompt |

Target: **power-on → usable `C:\>` in ≤ 7 s on Pi 5**, ≤ 10 s on Pi 4. Firmware
time (~1.5–2.5 s on Pi 5 depending on EEPROM config and SD card) is outside our
control; we document an optional EEPROM tweak (`BOOT_ORDER`, `HDMI_DELAY=0`) in
Control Panel → Startup.

### 4.2 POST screen (`retro-post`)

A tiny Rust program that writes directly to a KMS dumb buffer (no GL, no
Wayland) and talks to ALSA directly (PipeWire isn't up yet). It shows *real*
data, styled as a 1980s BIOS:

```
 Phosphor BIOS v1.0  (C) 1987-1993 Phosphor Computer Company

 PCC Model 5 — ARM Cortex-A76 4-core @ 2400 MHz
 Memory Test :  8192000K OK
 Video       :  VideoCore VII, HDMI-A-1 1920x1080
 Storage     :  SD 64 GB (SDR104)   USB: 1 keyboard, 1 mouse
 Network     :  Ethernet up, Wi-Fi ready, Bluetooth ready

 Press F8 for Startup Menu, DEL for Setup
```

- The memory counter animates to `MemTotal`. It never delays boot: it is paced
  to finish when `retro-session` reports ready, and is skipped if boot finishes
  first ("Fast POST" is the default).
- **F8** → Startup Menu (MS-DOS 6 homage): `1. RetroDOS prompt  2. RetroOS Desktop
  3. Safe mode (POSIX shell)  4. Safe mode with networking  5. Restart  6. Shut down`.
- **DEL** → "Setup" = the text-mode Control Panel's Startup page (default mode,
  autologin, fast POST, chime on/off).
- Chime plays on HDMI (Pi 5 has no analog jack) or USB audio if present; failure
  to open audio is silent and non-fatal.
- Emits `RETRO-POST: done t=<ms>` to the kernel log/serial for the test harness.

### 4.3 Init

systemd, with a custom `retro.target` as default. We mask/omit everything not
needed: no `ModemManager`, `avahi`, `cups`, `apt-daily` at boot (moved to a
timer delayed 10 min), `man-db` timers, `e2scrub`, etc. journald is volatile
(RAM, capped 8 MB) by default to spare SD cards; persistent logs are a Control
Panel toggle. zram swap (25% of RAM, zstd) via `systemd-zram-generator`.

Why systemd and not runit/OpenRC/our own init: logind (seat & device access for a
rootless compositor), socket/D-Bus activation (lets PipeWire, PackageKit, polkit,
udisks2 cost zero RAM until used), parallel boot, and Debian package
compatibility. Writing our own PID 1 adds nothing user-visible. (D-003)

---

## 5. Login, session, and mode switching

### 5.1 First boot

First boot runs **SETUP.EXE**, a full-screen text wizard in the RetroDOS style
(`retro-setup`, a text program shown by Station; see D-017): language/keyboard → create user (name, password;
added to `admin`) → computer name → time zone → Wi-Fi (optional) → default
startup mode (RetroDOS / Desktop) → autologin yes/no. It also expands the root
partition and regenerates SSH host keys (SSH itself is off by default).

### 5.2 Login

`greetd` (small Rust login daemon, proven, PAM-based) runs **retro-greeter**,
a text program shown full-screen by Station + retro-console (D-017):

```
 RetroOS 1.0                                         Phosphor Computer Company

 Enter your name:     ben
 Enter your password: ********

 [Enter] Log in   [F2] Mode: RetroDOS   [F10] Shut down
```

If autologin is set, greetd starts the session directly (still PAM, still a
proper logind session).

### 5.3 The session

`retro-session` (tiny supervisor) sets the environment (XDG dirs, themes, §11),
starts **Station** with the chosen personality, restarts it on crash (bounded),
and logs to the journal. `systemd --user` provides the user D-Bus and socket
activation for PipeWire.

### 5.4 Mode switching

Mode is a property of the running Station, not a reboot or re-login:

| From | Action | Result |
|---|---|---|
| RetroDOS | `DESKTOP` (alias `WIN`) | Station switches to desktop personality. Every text session becomes a "DOS Prompt" window; every full-screen app becomes a normal window. |
| Desktop | ◆ menu → **Exit to DOS…** (or `Ctrl+Alt+F1`) | Station switches to text personality. Windows become task-swapper entries (still running); the focused DOS Prompt becomes the full screen. Option in dialog: "Close all programs first". |
| Either | Control Panel → Startup | Sets the default for the next boot. |
| Boot | F8 Startup Menu | One-off override. |

Because nothing is restarted, switching takes one frame.

---

## 6. Display stack

### 6.1 Components reused
- **KMS/DRM** (vc4 display + v3d render on Pi; virtio-gpu in QEMU), **GBM/EGL/GLES** from Mesa.
- **Wayland** protocol, **libinput**, **libseat** (logind backend), **xkbcommon**.
- **Smithay** (Rust compositor library: DRM/GBM/libinput/libseat backends, GLES and Pixman renderers, xdg-shell, layer-shell, Xwayland + X11 WM helpers, direct scan-out). (D-006)
- **Xwayland**, started lazily on first X11 client.

### 6.2 Station: one compositor, two personalities

**TEXT personality** ("RetroDOS")
- Every surface is full-screen; no decorations.
- The console client's sessions and every launched app are entries in a
  **task swapper** (homage to the DOS 5 Shell task swapper):
  `Ctrl+Alt+Tab` cycles, `Ctrl+Alt+Esc` (or `Ctrl+Alt+Del` once) opens a
  full-screen text **Task List** ("Switch to / End Task / Cancel").
- Dialogs/popups from apps are centred over their parent, drawn with a 1-px
  black frame and our drop shadow, so a full-screen Firefox file chooser still works.
- When a GUI app exits, focus returns to the session that launched it.

**DESKTOP personality** ("RetroOS Desktop")
- Server-side window decorations for every app that accepts them
  (`xdg-decoration`), and for all X11 apps via our X11 WM.
- Layer-shell clients for the System Bar and desktop; normal windows for apps.
- Stacking (overlapping) window manager; click-to-focus; keyboard: `Alt+Tab`,
  `Alt+F4`, `Alt+Space` window menu, `Win/Super` opens ◆ menu.
- No animation. Window move shows an XOR-style outline (1990 behaviour, and cheap).

### 6.3 Rendering, GPU, scaling
- Renderer: **GLES2** on Mesa (v3d on Pi 4/5, virgl in QEMU). Full-screen
  surfaces with suitable dmabufs use **direct scan-out** (zero-copy) — this is
  what makes full-screen games and video in text mode efficient.
- Fallback: **Pixman** (CPU) renderer when no GPU (QEMU without virgl,
  broken driver). Selected automatically; logged.
- Lazy GL: in text mode with only shm clients (console, TUI), we try to keep the
  GLES context unallocated and composite with Pixman, bringing GL up on the
  first dmabuf/GL client. If this proves fragile in Smithay we keep GLES always
  on and absorb ~20 MB (tracked in §16; D-007).
- **Integer scaling only.** Station picks the largest integer scale such that the
  logical desktop is ≥ 960×540 (1080p → 2×, 1440p → 2×, 4K → 4×, 1024×768 → 1×).
  Clients get `wl_output.scale`/`wp_fractional_scale` = integer, so modern apps
  render crisp at 2× and our bitmap UI stays pixel-exact. No fractional scaling.
- Cursors are drawn by Station from our cursor theme; clients use
  `wp_cursor_shape_v1` where supported, else Xcursor theme (same artwork).

### 6.4 Why not use the Linux VT console for text mode?
fbcon/kmscon give us no control over glyph rendering, palette fidelity,
mouse cursor, or seamless app launching; and handing DRM master between a
console process and a kiosk compositor is racy and causes mode-set flicker. A
compositor that is also the text console removes the whole class of problem.
Cost: the compositor is always resident (~10–35 MB; see §16). (D-005)

---

## 7. RetroDOS: shell architecture

### 7.1 Layers

```
 retro-console (Wayland client: grid renderer + pty host; uses alacritty_terminal for VT state)
        │  pty
 COMMAND (RetroDOS shell: line editor, parser, built-ins, batch interpreter)
        │  fork/exec            │ launch broker (unix socket → Station)
 TUI apps / POSIX programs      GUI apps (Firefox …)
```

- **retro-console** is a terminal emulator: our renderer, our fonts and palette,
  blinking block cursor, mouse cursor as an inverse block (DOS mouse driver
  homage), multiple sessions. VT state machine via the `alacritty_terminal`
  crate (well-tested; reinventing VT100 parsing adds nothing). Advertises
  `TERM=retro` (terminfo shipped; derived from `xterm-256color`) so ncurses
  apps (htop, vim, mc, ssh) work. 16-colour by default with the Phosphor16
  palette; 256/truecolour are honoured (`MODE CON COLORS=TRUE`) or quantised to
  16 in "Authentic" mode.
- **COMMAND** is a normal program in a pty. That means: `UNIX` (POSIX escape
  hatch) is just exec'ing bash in the same pty; ssh and curses tools just work; and
  the same shell runs in the desktop "DOS Prompt" window.

### 7.2 Filesystem presentation: drive letters

Drive letters (familiar, compact, works with `C:\>`), plus ProDOS/Mac-style
volume names shown in `DIR` headers and the GUI (`Volume in drive C is HOME`).

| Drive | Maps to | Notes |
|---|---|---|
| `A:`, `B:` | first/second removable *floppy-sized* media (USB sticks ≤ 64 GB if no other rule) | Yes, this is a joke that is also useful |
| `C:` | `/home/<user>` | The user's "hard disk" |
| `D:`…`Y:` | other removable/USB/SD volumes, in plug order, sticky per filesystem UUID | auto-mounted (udisks2) under `/media/<user>/<LABEL>` |
| `N:` | network shares (later; smb/sftp via gvfs-less FUSE, M5 stretch) | |
| `Z:` | `/` (the real root) | DOSBox homage; power users' window onto Linux |

Rules:
- `\` and `/` are both accepted; output uses `\`.
- Name lookup is **case-insensitive, case-preserving** (DOS semantics on a
  case-sensitive FS; ambiguous matches prompt).
- Long names shown in full by default. `SET LFN=OFF` shows synthetic 8.3 aliases
  (`PROGRA~1`) for authenticity; aliases are resolvable.
- Hidden = dotfiles; `ATTRIB +H/-H` renames to/from dot-prefix (with confirmation).
  `ATTRIB +R/-R` maps to the write bit. System `+S` shown for root-owned files.
- Raw POSIX paths are accepted anywhere (`CD /usr/share`) and displayed as `Z:\USR\SHARE`.

### 7.3 Commands

Built-ins (selection): `DIR CD MD RD COPY XCOPY MOVE REN DEL TYPE MORE CLS ECHO
SET PATH PROMPT VER VOL LABEL MEM DATE TIME FIND SORT TREE ATTRIB CHKDSK FORMAT
HELP EXIT COLOR MODE KEYB CHOICE PAUSE BEEP`.

RetroOS extensions: `RUN START TASKS KILL DESKTOP UNIX ADMIN SHUTDOWN RESTART
NET WIFI BT VOLUME MIXER INSTALL UNINSTALL LIBRARY UPDATE CONTROL FILER EDIT
ABOUT USERS PASSWD EJECT`.

Faithful details: `Bad command or file name`, `Abort, Retry, Fail?` on a yanked
USB stick during `COPY`, `F3` repeats last command (DOSKEY built in, plus
up/down history and Tab completion), `/?` on every command, `/P` `/W` on `DIR`,
`MEM` shows a real breakdown (conventional = RAM, "upper" = zram, by-process
table with `/C`), `VER` prints `RetroDOS Version 1.0  (6.18.y aarch64)`.

**Batch files**: `.BAT` interpreter supporting `ECHO [OFF]`, `@`, `REM`, `SET`,
`%VAR%`, `%1..%9`, `SHIFT`, `IF [NOT] EXIST/ERRORLEVEL/==`, `GOTO`, labels,
`CALL`, `FOR %%X IN (...) DO`, `PAUSE`, `CHOICE`. `C:\AUTOEXEC.BAT` runs at
session start. `ERRORLEVEL` = last exit status.

Dangerous commands (`FORMAT`, `DEL /S`, `RD /S`) require explicit confirmation;
`FORMAT` only operates on removable drives and needs `ADMIN` rights.

### 7.4 Program resolution and launching modern apps

When the user types `FOO args`:
1. Built-in command `FOO`?
2. `FOO.BAT` in current dir, then `PATH`?
3. Executable `foo` (case-insensitive) in current dir, then `PATH` (which
   includes the real Linux `$PATH`)?
4. Installed application whose `.desktop` `Name`, executable, or our catalog
   alias matches (`FIREFOX`, `CHROME`→chromium, `VSCODE`, `RETROARCH`)?

If the resolved target is a **GUI application** (it came from a `.desktop` file
without `Terminal=true`, or its ELF `DT_NEEDED` includes
libwayland-client/libX11/libgtk/libQt*/libSDL*), COMMAND does **not** exec it in
the pty. It sends a `Launch` request to Station's **launch broker**:

```
Launch { argv, cwd, env_delta, wait: bool, placement: Auto|FullScreen|Window }
 → { task_id } … later → Exited { task_id, status }
```

- In TEXT personality, Station spawns the app, makes its first toplevel
  full-screen and focused. The console shows
  `Running FIREFOX…  Ctrl+Alt+Tab switches back, Ctrl+Break ends it.`
- `RUN FOO` / bare `FOO` **waits** (DOS semantics); when the app exits, Station
  returns focus to the console, COMMAND prints the prompt with `ERRORLEVEL` set.
- `START FOO` returns immediately (app stays in the task swapper).
- In DESKTOP personality, the same request opens a normal window.
- `RUN /TEXT FOO` forces pty execution; `RUN /GUI FOO` forces broker.
- Station tracks app→task mapping via the spawned PID (cgroup per task through
  `systemd-run --user --scope`, which also gives clean "End Task" and
  per-app memory accounting for `MEM /C` and "About This Computer").
- X11-only apps work transparently (Xwayland lazy start, `DISPLAY` set in env).

### 7.5 POSIX escape hatch
- `UNIX` → runs the user's login shell (bash) in the same pty with a banner:
  `Type EXIT to return to RetroDOS.`
- `UNIX command…` runs a single POSIX command line.
- Startup Menu → Safe mode boots to a plain getty + bash on the Linux console
  (no Station) for recovery.
- `ADMIN command` = elevated execution (sudo with our prompt), for users in `admin`.

---

## 8. Text-mode applications

All are `ratatui`/`crossterm` TUIs in the RetroDOS visual language (STYLE.md §3),
mouse-enabled, Alt-key menus, F1 help. They run in any terminal, so they also
work over SSH and in the desktop DOS Prompt.

| Command | App | Spirit |
|---|---|---|
| `EDIT` | **EDIT** — full-screen editor: menus, mouse, multiple buffers, search/replace, UTF-8, line endings, syntax colouring for a few languages | EDIT.COM / QBasic editor |
| `FILER` | **Filer** — dual-panel file manager, F-key bar (F3 View, F4 Edit, F5 Copy, F6 Move, F7 MkDir, F8 Delete), drive switcher, archive browsing (zip/tar) | Norton Commander + ProDOS Filer |
| `CONTROL` | **Control Panel (text)** — same pages as GUI (network, sound, display, keyboard, date/time, users, startup, updates, Bluetooth) | MS-DOS 6 MEMMAKER/Setup screens |
| `WIFI` / `NET` | Network setup (iwd) | |
| `BT` | Bluetooth pairing (BlueZ) | |
| `MIXER` / `VOLUME` | Volume mixer (PipeWire) — vertical slider bars | Sound Blaster mixer |
| `LIBRARY` / `INSTALL` | **Software Library (text)** — browse/search/install/remove/update | |
| `ABOUT` | About box + system info | |

All system-touching logic lives in one shared crate (`retro-sys`, §10) so the
text and GUI front-ends cannot diverge.

---

## 9. GUI mode: RetroOS Desktop

### 9.1 Layout (Windows 3.1 × System 7)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◆  File  Programs  Window                          🔊  ▂▄▆ Wi-Fi   10:42 AM │ ← System Bar (Mac-style, global)
├──────────────────────────────────────────────────────────────────────────────┤
│░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░[C: HOME]░│
│░░┌─[-]═════════ Program Manager ═════════[▼][▲]┐░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
│░░│ File  Options  Window  Help                 │░░░░░░░░░░░░░░░░░░[D: USB ]░│ ← drive icons on desktop
│░░│ ┌Main──────┐ ┌Accessories┐ ┌Internet─┐      │░░░░░░░░░░░░░░░░░░░░░░░░░░░░│   (Mac Finder style)
│░░│ │[]  []  []│ │ []  []  []│ │ []  []  │      │░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
│░░│ └──────────┘ └───────────┘ └─────────┘      │░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
│░░└─────────────────────────────────────────────┘░░░░░░░░░░░░░░░░[Wastebasket]│
└──────────────────────────────────────────────────────────────────────────────┘
```

- **System Bar** (global, top): ◆ *Phosphor menu* (About This Computer, Control
  Panel, Software Library, DOS Prompt, Restart, Shut Down, Exit to DOS…),
  **Programs** menu (launcher tree mirroring Program Manager groups), **Window**
  menu (list of open windows/tasks), and right-side status items: volume,
  network, Bluetooth, battery (if any), clock. App menus stay *inside* windows
  (Win 3.1 style) — modern apps have in-window menus anyway, and global-menu
  protocols are fragile (D-010).
- **Desktop**: dithered pattern background, drive icons and Wastebasket down the
  right edge (Mac), double-click opens File Cabinet at that drive.
- **Program Manager**: MDI-style group windows of program icons; groups
  auto-populated from installed `.desktop` files by category + user-editable.
- **Windows**: chunky resizable frame, solid title bar with centred title,
  control-menu box left, minimise/maximise right (STYLE.md §4). Minimised windows
  become icons on the desktop's bottom edge (Win 3.1).

### 9.2 GUI applications (ours)

| App | Notes |
|---|---|
| **Program Manager** | launcher, groups, drag to rearrange, File → Run… |
| **File Cabinet** | file manager: icon/list views, drive bar, copy/move with progress dialog, open-with, Get Info, Wastebasket (XDG trash) |
| **Control Panel** | icon grid of applets: Network, Bluetooth, Sound, Display, Desktop, Mouse, Keyboard, Date & Time, Users, Startup, Updates, Software Sources, About |
| **Software Library** | installer front-end (§10.5) |
| **DOS Prompt** | a windowed retro-console; also hosts sessions handed over from text mode |
| **Jot** | Notepad-class text editor (GUI sibling of EDIT) |
| **Volume** | desk-accessory slider window + System Bar popup |
| **About This Computer** | Mac-style memory bar chart per running program (from cgroups) |
| **Media Player** | thin retro front-end over **libmpv** (M4) |
| **Arcade** | emulator front-end: ROM library over RetroArch cores / standalone emulators (M4–M5) |

### 9.3 retro-ui toolkit

A small Rust widget toolkit, **software rendered** into `wl_shm` buffers at
integer scale with our bitmap fonts. Rationale: our UI is flat rectangles,
1-bit-ish icons and bitmap text; a CPU renderer with damage tracking is
smaller, faster to start (~10 ms), and lighter on RAM than any GPU toolkit, and
gives exact pixel control. Widgets: menu bar/menus, buttons, check/radio, text
field, list, tree, icon view, scrollbars, sliders, progress, tabs-as-"More >>"
buttons, standard dialogs (open/save, message box, progress, About). Uses
`smithay-client-toolkit` for Wayland plumbing.

Accessibility: full keyboard navigation (Alt mnemonics, Tab order), a
High-Contrast scheme, and a 1-step-larger scale option. Screen reader support
(AT-SPI) is a known gap (§17).

---

## 10. System services and integration

All front-ends (text and GUI) go through **`retro-sys`**, a Rust crate wrapping
the D-Bus APIs below via `zbus`. No front-end shells out to CLIs for core flows.

### 10.1 Networking
- **iwd** for Wi-Fi (small, fast, clean D-Bus API incl. passphrase agent; ~3 MB).
- **systemd-networkd** for Ethernet DHCP, **systemd-resolved** for DNS, **timesyncd** for NTP.
- UI: `WIFI`/`NET` (text), Control Panel → Network (GUI), System Bar indicator.
  Scan list with signal bars, join (WPA2/WPA3-Personal; Enterprise via advanced
  dialog), forget, hidden SSID, static IP for Ethernet, Wi-Fi country (regdom).
- Why not NetworkManager: ~3× the RAM and a large dependency tree; iwd+networkd
  covers the consumer cases. (D-011)

### 10.2 Bluetooth
BlueZ `bluetoothd`, started only while Bluetooth is enabled. Pairing agent
implemented in `retro-sys` (PIN/passkey confirm dialogs in both modes). Audio
devices via PipeWire's BlueZ integration; HID (keyboards, mice, gamepads) via
kernel/BlueZ.

### 10.3 Audio
PipeWire + WirePlumber, socket-activated in the user session (zero RAM until
first use, except the chime which uses ALSA directly at boot). Volume/mute/
output-device selection in `MIXER`/`VOLUME`, Control Panel → Sound, System Bar.
UI sounds (STYLE.md §6) played by Station through PipeWire with a tiny
in-process player. Note: Pi 5 audio = HDMI, USB, or Bluetooth (no 3.5 mm jack).

### 10.4 Storage, input, power
- Removable media: **udisks2** (D-Bus activated) mounts on insert; Station/
  `retro-sys` assigns sticky drive letters; `EJECT D:` / drag to Wastebasket ejects.
- Keyboards/mice/gamepads: libinput in Station; gamepads exposed via logind
  `uaccess` to apps (SDL). Keyboard layout from Control Panel → `xkb` (shared by
  Station and SETUP).
- Power: `SHUTDOWN`/`RESTART`/menu items via logind. "It is now safe to turn off
  your computer." screen on shutdown (kept on-screen by Station until KMS is torn
  down, then the Pi powers off).

### 10.5 Software installation and updates
- **Sources**: Debian trixie (main, contrib, non-free-firmware; non-free opt-in),
  Debian security, Raspberry Pi archive, our own **RetroOS apt repo** (our
  packages + themes). All signed; keys in `/etc/apt/keyrings`.
- **Backend**: **PackageKit** (apt backend) over D-Bus — proven, handles
  locking, progress, authorisation via polkit, and exits when idle (0 MB idle).
- **Catalog**: `catalog/*.toml` in this repo — curated entries with retro name,
  pixel icon, category, blurb, package list, launch alias(es), and post-install
  hints. ~60 entries at M4 (browsers, emulators, games, media, dev tools, office).
  "Search everything" falls back to the full apt index.
- **Front-ends**: `LIBRARY` / `INSTALL <name>` / `UNINSTALL` / `UPDATE` (text),
  Software Library (GUI). Same operations, same catalog.
- **Icons for third-party apps**: catalog entries ship hand-drawn icons;
  uncatalogued apps get an automatically "retro-fied" icon (downscale to 32×32,
  quantise to Phosphor16 with ordered dither, add 1-px outline).
- **Updates**: daily PackageKit refresh (timer, randomised, 10 min after boot);
  notification in System Bar / at the prompt (`3 updates available. Type UPDATE.`).
  Security updates install automatically by default (`unattended-upgrades`,
  toggle in Control Panel). Kernel/firmware updates come from the RPi archive.
- **Flatpak** (Flathub arm64) is an opt-in extra source in M5 if budget allows (disk, not RAM).

### 10.6 Users, permissions, security
- Root account locked. First user in `admin` (sudo + polkit admin). Additional
  users via Control Panel → Users / `USERS`.
- polkit rules: `admin` group may install packages, change network/system
  settings after password re-auth (cached 5 min). Network join for the active
  local seat user without admin.
- Device access through logind seats (`uaccess`), not world-writable nodes.
- nftables: default-deny inbound, allow established + DHCP/mDNS-off. SSH
  disabled by default; Control Panel toggle enables it and opens port 22.
- Compositor is rootless; Station has no privileges beyond the logind session.
- Our daemons: none run as root. (`retro-post` runs as a dedicated
  `retro-post` user with `video`/`audio` access only.)

---

## 11. Theming third-party applications

Goal: modern apps "fit in" without being broken. We ship a **"Phosphor" theme
family** packaged as `retroos-theme`:

| Toolkit / surface | Mechanism | Expected fidelity |
|---|---|---|
| Window frames | Station server-side decorations (xdg-decoration) + our X11 WM frames | Excellent for SSD-capable apps |
| GTK 3 | Our `gtk.css` theme ("Phosphor"), set via `settings.ini`, gsettings (dconf) and `GTK_THEME` | Very good (Firefox, Geany, GIMP, Thunderbird…) |
| GTK 4 (non-adwaita) | Same theme, GTK4 variant, `GTK_THEME` | Good |
| libadwaita | Accent/colour scheme only; libadwaita does not support themes | Poor — documented limitation; CSD headerbars remain |
| Qt 5/6 | Built-in **`Windows`** QStyle (which is already Win9x-style) + our palette and fonts via `qt5ct`/`qt6ct` config | Good |
| Firefox | Follows GTK theme; enterprise `policies.json`: system theme, our start page, no telemetry, UI font; optional static retro theme XPI | Good |
| Chromium | `initial_preferences`: use GTK theme, our start page | Fair–good |
| SDL / games / emulators | Full-screen; our cursor; nothing else to theme | n/a |
| Cursors | Xcursor theme "Phosphor" at 1×–4×, `XCURSOR_THEME/SIZE`, `wp_cursor_shape` | Excellent |
| Fonts | UI font "Lantern" shipped as **pixel-outline TTF** (square-pixel contours, crisp at integer multiples) + bitmap OTB; set as GTK/Qt UI font. Web/document content keeps normal fonts (DejaVu/Noto) so the web stays readable | Good |
| Icons | Minimal freedesktop icon theme "Phosphor" for common actions/mimetypes, inheriting from `hicolor`/Adwaita fallbacks | Partial (grows over time) |

---

## 12. Language choices

**Rust for all our system components** (Station, retro-console, COMMAND,
retro-post, retro-greeter, retro-session, retro-sys, retro-ui, TUI/GUI apps,
build helpers).

- Memory safety where it matters: we parse untrusted input everywhere (terminal
  escape sequences, Wayland client requests, filenames, package metadata).
- Ecosystem fit: Smithay (compositor), smithay-client-toolkit, zbus (D-Bus),
  alacritty_terminal (VT), ratatui (TUI), drm/gbm/input crates.
- Compiled, fast, small: release profile `opt-level="s"`, LTO, `panic="abort"`,
  `codegen-units=1`, stripped → expected 0.5–4 MB per binary; startup in ms.
- Cross-compiles cleanly to aarch64.

Considered: **C + wlroots** (most mature compositor library, but C for a large
UI codebase is a liability; wlroots API churn), **Zig** (pleasant, but pre-1.0
language and thin library ecosystem for Wayland/D-Bus/VT). (D-008)

Build-time tooling (font compiler, icon compiler, sound synthesiser) is also
Rust, as small `xtask`-style binaries, so the asset pipeline has no Python
runtime dependency. Shell (POSIX sh) only for image-assembly glue.

---

## 13. Build system and reproducibility

### 13.1 One command

```
make image          # → out/retroos-1.0-arm64.img  (+ .img.xz, .sha256, manifest)
make qemu           # boots it in QEMU (virt, virtio-gpu), see §15
make test           # unit tests + QEMU integration tests
```

`make image` runs inside a **pinned build container** (Debian trixie image by
digest, via podman or docker; falls back to running natively on a Debian
trixie host) so the host OS doesn't matter (x86_64 or arm64).

### 13.2 Pipeline

1. **Rust workspace** → cross-compiled for `aarch64-unknown-linux-gnu` against a
   Debian arm64 multiarch sysroot inside the container; `Cargo.lock` committed;
   vendored crates (`cargo vendor`) for offline/reproducible builds.
2. **Assets** → fonts (glyph sources → PSF2, OTB, pixel-TTF), icons (text sources
   → PNG, `.ico`-like sets), cursors (→ Xcursor), sounds (synth scripts → WAV),
   themes (GTK CSS assets rendered from sources).
3. **Packaging** → each component becomes a `.deb` (`cargo-deb` + small
   control files), collected into a local apt repo (`out/repo`), signed with a
   build key (release key in CI).
4. **Rootfs** → `mmdebstrap --variant=apt --architectures=arm64` with
   `--include` lists from `build/packages/*.list`, sources pinned to
   `snapshot.debian.org/<timestamp>` and the RPi archive, plus our local repo.
   Foreign-arch maintainer scripts run under `qemu-user-static` (binfmt) on
   x86_64 hosts; natively on arm64 hosts/runners.
   Hooks: config overlay (`config/rootfs/`), systemd presets/masks, locale/keymap
   defaults, user-less image (first-boot wizard), cleanup (docs/man/locales trimmed
   to save ~150 MB).
5. **Image** → `genimage`: MBR, `BOOT` FAT32 (256 MB: firmware, kernels, DTBs,
   overlays, `config.txt`, `cmdline.txt`) + `RETROROOT` ext4 (built with
   `mke2fs -d`, no loop devices, no root) sized to content + 20%; first boot
   grows it.
6. **Outputs** → `.img`, `.img.xz`, SHA-256, a **package manifest** (every
   .deb name/version/sha256) and an SBOM.

### 13.3 Reproducibility stance
- Pinned: container digest, Debian snapshot timestamp, Rust toolchain
  (`rust-toolchain.toml`), `Cargo.lock`, vendored crates, `SOURCE_DATE_EPOCH`.
- The Raspberry Pi archive has no snapshot service, so the build records a
  lockfile of RPi package versions and, for releases, mirrors those `.deb`s as a
  release asset; `make image LOCK=release-1.0` rebuilds from them.
- Target: identical package set and identical content for the same inputs;
  **bit-for-bit identical images** is a stretch goal (ext4/FAT metadata
  determinism via fixed UUIDs/timestamps), tracked but not blocking.

### 13.4 CI
GitHub Actions: `cargo fmt/clippy/test` (x86_64), cross build, and image build +
QEMU boot test on **native arm64 runners** (`ubuntu-24.04-arm`), which avoids
qemu-user slowness. Artifacts: image + manifest + screenshots from tests.

---

## 14. Repository layout

```
RetroOS/
├── Makefile                  # image, qemu, test, clean, fmt, lint
├── Cargo.toml                # Rust workspace
├── rust-toolchain.toml
├── build/
│   ├── Containerfile         # pinned build environment
│   ├── mkimage.sh            # orchestrates steps 4–6
│   ├── mmdebstrap/           # hooks (setup, essential, customize)
│   ├── packages/             # base.list, text.list, gui.list, apps.list, qemu.list
│   ├── genimage.cfg
│   └── lock/                 # snapshot timestamp, RPi package lock
├── config/
│   ├── boot/                 # config.txt, cmdline.txt
│   └── rootfs/               # overlay: /etc, systemd units/presets, polkit, nftables, greetd
├── crates/
│   ├── retro-gfx/            # pixel buffers, palette, bitmap font rendering, blit
│   ├── retro-assets/         # embedded fonts/icons/cursors/sounds (generated)
│   ├── retro-ipc/            # launch-broker + session protocol types
│   ├── retro-sys/            # D-Bus wrappers: iwd, bluez, pipewire, packagekit, udisks2, logind, timedated…
│   ├── retro-dosfs/          # drive-letter & DOS path mapping, 8.3 aliases
│   ├── retro-post/           # boot screen + chime
│   ├── retro-login/          # retro-greeter (greetd greeter) + retro-setup (first boot)
│   ├── retro-tui/            # cell/key layer for RetroOS text programs
│   ├── retro-session/        # session supervisor
│   ├── station/              # compositor (text + desktop personalities, X11 WM, broker)
│   ├── retro-console/        # terminal client (text mode + DOS Prompt window)
│   ├── retrodos/             # RetroDOS shell (binary `retrodos`) + batch interpreter
│   ├── retro-tui/            # shared TUI widgets/theme on ratatui
│   ├── tui-apps/             # edit, filer, control, wifi, bt, mixer, library, about
│   ├── retro-ui/             # GUI toolkit
│   └── gui-apps/             # sysbar, desktop, progman, filecab, control, library, jot, volume, about, mediaplayer, arcade
├── tools/                    # build-time: fontc, iconc, cursorc, synth
├── assets/
│   ├── fonts/                # glyph sources (.glyphs text format) + metadata
│   ├── icons/                # icon sources (.icon text format)
│   ├── cursors/
│   ├── sounds/               # synth patches (.synth)
│   └── palettes/
├── themes/                   # gtk-3.0, gtk-4.0, qt, firefox, chromium, fontconfig, xcursor
├── catalog/                  # software catalog entries (*.toml)
├── packaging/                # debian control files / cargo-deb metadata
├── tests/
│   ├── qemu/                 # harness (boot, keys, screenshots, budget measurement)
│   └── golden/               # reference screenshots
└── docs/
    ├── DESIGN.md  STYLE.md  DECISIONS.md
    ├── HW_CHECKLIST.md       # real-hardware test checklist (from M1)
    ├── BUDGET.md             # measured numbers per milestone
    └── USER_GUIDE.md         # (M5)
```

---

## 15. Testing strategy

1. **Unit tests** (`cargo test`, host): DOS path mapping, 8.3 aliases, command
   parsing, batch interpreter, catalog parsing, font/icon compilers.
2. **Golden-image render tests** (host, no QEMU): retro-console, retro-ui
   widgets, retro-post and greeter render into an off-screen buffer → PNG,
   compared with `tests/golden/` (exact match; pixel art makes this reliable).
   Station runs under Smithay's headless/winit backends for protocol tests.
3. **QEMU integration** (`make test-qemu`): `qemu-system-aarch64 -M virt -cpu
   cortex-a76 -m 2G` (or 8G), `virtio-gpu-pci` (+virgl when host supports it,
   else Pixman path), virtio-net, virtio-keyboard/tablet, `-device intel-hda`
   for audio. Boots the **same root filesystem** from the built `.img`; only the
   kernel differs (Debian generic arm64 kernel extracted by `make qemu`,
   because QEMU cannot emulate a Pi 5 and its `raspi4b` model lacks PCIe/USB).
   The Python harness drives QMP: waits for serial markers
   (`RETRO-POST: done`, `RETRO-SHELL: ready`), types commands with `send-key`,
   takes `screendump`s, compares against goldens (text-mode screens exactly;
   GUI with tolerance), and **records boot time and idle RAM** (`MemTotal -
   MemAvailable` after 60 s idle, plus per-unit/cgroup breakdown) into
   `docs/BUDGET.md`. Test cases per milestone: boot-to-prompt; `DIR`/`COPY`/
   batch; `RUN` a Wayland test client full-screen and return; `DESKTOP` and back;
   Firefox launch smoke test (M2+); package install from a local test repo (M4).
4. **Real hardware checklist** (`docs/HW_CHECKLIST.md`): Pi 5 and Pi 4 —
   boot time (stopwatch from power), HDMI modes (1080p, 1440p, 4K, 1024×768),
   dual HDMI, V3D GL in Firefox/WebGL and a game, HDMI and USB audio, chime,
   Wi-Fi (2.4/5 GHz, WPA3), Ethernet, Bluetooth keyboard/headphones/gamepad,
   USB stick hot-plug and eject, idle RAM on device, thermal/idle CPU, shutdown.

---

## 16. Resource budget plan

Measured definition: **idle RAM = MemTotal − MemAvailable, 60 s after reaching
the prompt/desktop, no user apps open**, Pi 5 8 GB, 1080p display. Estimates
below are what we'll measure against; actuals go in `docs/BUDGET.md` each
milestone.

| Component | Text mode (est.) | GUI mode (est.) |
|---|---|---|
| Kernel slab/page tables/CMA overhead, reserved | 25–30 MB | 25–30 MB |
| systemd PID 1 + journald (volatile) + udevd + logind + dbus-broker | 18–22 MB | 18–22 MB |
| iwd + networkd + resolved + timesyncd | 9–12 MB | 9–12 MB |
| greetd + systemd --user + user dbus | 6–8 MB | 6–8 MB |
| Station (Pixman/no-GL in text; GLES in GUI) incl. scan-out buffers | 10–15 MB | 35–50 MB |
| retro-console (1 session) + COMMAND | 4–6 MB | 4–6 MB |
| System Bar, desktop, Program Manager (retro-ui, shm buffers) | — | 15–25 MB |
| PipeWire + WirePlumber (only once audio used; GUI plays UI sounds) | (0) | 12–18 MB |
| **Total** | **~72–93 MB** (target < 100) | **~125–170 MB** (target < 200) |

Risks to the text budget: if lazy-GL in Station doesn't work out, add ~20 MB →
~95–113 MB; mitigations: smaller CMA usage via single-buffer 16-bpp scan-out in
text mode, trimming journald, or dropping resolved (iwd/networkd write
resolv.conf). We will report honestly if the < 100 MB target needs to move.

**Boot time** measured as (a) systemd-analyze kernel→prompt (from serial marker
timestamps) and (b) power-on→prompt stopwatch on hardware.

---

## 17. Risks and mitigations

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | Smithay rough edges on Pi's split display (vc4) / render (v3d) DRM devices | GUI/apps fail on hardware | Smithay supports separate render nodes (used by cosmic-comp/niri on Pi); test on HW at M1; Pixman fallback; worst case port Station to wlroots via FFI (large, unlikely) |
| R2 | QEMU can't emulate Pi 5; Pi-specific paths (V3D, HDMI audio, brcmfmac, BT) untestable in CI | Late HW bugs | Hardware-agnostic userspace (generic KMS/libinput/iwd/BlueZ); HW checklist every milestone; dev on a real Pi |
| R3 | Text-mode < 100 MB with an always-on compositor | Budget miss | Lazy GL / Pixman in text mode; measure from M1; fallback options in §16 |
| R4 | Theming modern apps: libadwaita and Electron apps won't fully conform | Look inconsistency | Server-side frames where possible; document; prefer GTK3/Qt apps in the catalog |
| R5 | 16 K-page Pi 5 kernel breaks some software (certain JITs, box64, some emulators) | App failures | Control Panel → "Memory Manager": switch to 4 K-page `kernel8.img` (yes, it's our EMM386); catalog marks known-incompatible apps |
| R6 | Xwayland integration (override-redirect, focus, games) | X11 apps misbehave | Use Smithay's X11 WM support; test matrix of X11 apps (RetroArch X11, DOSBox, older games) |
| R7 | Reproducibility with an un-snapshotted RPi archive | Rebuild drift | Version lockfile + mirrored release .debs (§13.3) |
| R8 | SD card wear and slowness hurts boot time | Slow boot, corruption | noatime, volatile journal, zram, `commit=60`; recommend A2 cards; USB/NVMe boot works out of the box (Pi 5) |
| R9 | Scope: building a WM, toolkit, terminal, shell, and ~15 apps | Schedule | Milestones are vertical slices; reuse (alacritty_terminal, ratatui, Smithay, PackageKit, iwd, BlueZ); defer Arcade/Media Player polish to M5 |
| R10 | Accessibility (screen readers) unsupported in our toolkit | Excludes users | Keyboard-complete UI, high-contrast & large scale; AT-SPI bridge considered post-1.0 |
| R11 | Firmware time before kernel isn't controllable | < 10 s on slow cards | Documented EEPROM tweaks; measured and reported separately |
| R12 | Firefox/Chromium RAM | Not in budget by definition | Excluded per spec; catalog notes; zram helps on 4 GB Pis |

### Feasibility flags (things I'm telling you up front)
- **QEMU fidelity**: "testable in QEMU" will mean the same rootfs on a generic
  arm64 kernel with virtio-gpu, not an emulated Pi. Pi-specific hardware is
  verified by checklist.
- **"Push the retro theme onto modern apps"**: achievable for GTK3/Qt/X11 apps
  and all window frames; *not* achievable for libadwaita/Electron internals.
- **Bit-for-bit reproducible images**: stretch goal; "same inputs → same
  package set and content" is the committed bar.
- **Pi 5 audio**: no analog output on Pi 5 — HDMI/USB/Bluetooth only.
- **Development environment note**: this cloud workspace is x86_64 without
  QEMU or a container runtime preinstalled; I'll install `qemu-system-arm`,
  `qemu-user-static`, `mmdebstrap`, and an aarch64 cross toolchain here (or use
  CI's arm64 runners) — no impact on the design.

---

## 18. Milestone plan

Each milestone ends with: a bootable image, green automated QEMU tests, an updated
HW checklist pass (as far as hardware is available to you), and measured numbers
in `docs/BUDGET.md`.

### M1 — Boots to POST screen and RetroDOS
- Build pipeline end-to-end (`make image`, `make qemu`, `make test`), container, CI.
- Kernel/firmware/config.txt for Pi 5 + Pi 4; quiet boot.
- Asset pipeline v1: Phosphor 8×16 text font, Phosphor16 palette, chime.
- `retro-post` (POST screen, F8 menu stub, chime), greetd + `retro-greeter`
  (login + minimal first-boot SETUP: user/password/hostname/timezone).
- Station v1: TEXT personality only, Pixman+GLES, libinput, xkb, integer scaling.
- retro-console v1 + COMMAND v1: prompt, line editing/history, drive mapping,
  `DIR CD MD RD COPY DEL REN TYPE CLS ECHO SET VER MEM HELP EXIT UNIX SHUTDOWN`.
- **Accept**: QEMU boots to `C:\>` and passes command tests; Pi 5 HW boot to
  prompt; boot time & text-mode RAM measured.

### M2 — Shell complete, networking, browser from the prompt
- COMMAND: full command set (§7.3), batch interpreter, AUTOEXEC.BAT, 8.3 mode, completion.
- Launch broker; GUI-app detection; full-screen apps in TEXT personality;
  task swapper + Task List; Xwayland on demand.
- `EDIT`, `FILER`, `WIFI`/`NET` (iwd + networkd), `TASKS`, `KILL`.
- Firefox preinstalled; `FIREFOX` launches full-screen with GPU and returns to the prompt.
- Safe-mode boot entries; F8 menu complete.
- **Accept**: Wi-Fi join from text UI on HW; Firefox WebGL + YouTube 1080p smoke test on Pi 5; budget re-measured.

### M3 — Desktop
- Station DESKTOP personality: SSD frames, stacking WM, window menu,
  minimise-to-icon, X11 WM, layer-shell, `DESKTOP` ↔ Exit to DOS live switch.
- retro-ui toolkit v1; System Bar; desktop with drive icons & Wastebasket;
  Program Manager; File Cabinet; DOS Prompt window (incl. adopted text sessions); Jot.
- Lantern UI fonts, icon set v1, cursor theme.
- **Accept**: QEMU GUI golden tests; Firefox windowed on Pi 5 with GPU; GUI idle RAM measured.

### M4 — Control Panel, installer, audio, Bluetooth, third-party theming
- `retro-sys` complete; Control Panel (text + GUI) with all applets.
- Software Library (text + GUI) on PackageKit + catalog (~60 entries) +
  retro-fied icons; updates + notifications; unattended security updates.
- PipeWire/WirePlumber; `MIXER`/Volume; UI sounds.
- Bluetooth pairing in both modes (keyboard, headphones, gamepad).
- GTK3/GTK4/Qt/Firefox/Chromium theming; cursor/font integration.
- Media Player (libmpv) and Arcade v1 (RetroArch); code editor (Geany preinstalled, VS Code in catalog).
- **Accept**: install/remove apps from both front-ends; BT audio and gamepad on HW.

### M5 — Polish, performance, documentation
- Profile and tune boot time and RAM against budget; trim packages; startup
  ordering; binary size.
- Visual/audio polish pass; Arcade and Media Player polish; Flatpak opt-in (if worthwhile).
- README with screenshots, USER_GUIDE, flashing guide, troubleshooting.
- Release pipeline: signed apt repo, versioned image, reproducibility report.
- **Accept**: all budget targets met or deviations justified; full HW checklist on Pi 5 and Pi 4.

---

## 19. Resolved questions (2026-10-16)

1. **Branding**: keep Phosphor Computer Company / RetroDOS / RetroOS Desktop.
2. **Default startup mode**: RetroDOS (text mode).
3. **Preinstalled browser**: Firefox only; Chromium one-click in the Software Library.
4. **Hardware**: a Raspberry Pi 5 is available for the hardware checklist. Pi 4 support stays in scope but is verified opportunistically.
5. **CI**: GitHub Actions approved, including arm64 runners.
