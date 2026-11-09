# Handoff: starting Milestone 4 (Control Panel, Software Library, audio, Bluetooth, theming)

Written at the end of the M3 session (2026-11-06) for whoever picks up next.
Read this first, then `docs/DESIGN.md` §18 (M4) and the sections it points
to, `docs/STYLE.md`, and `docs/DECISIONS.md` (D-001…D-042; the "Not yet"
lines of D-030…D-042 are M3's open ends).

## 0. Start here

1. **Check your branch has M3.** `git log --oneline | grep -c "M3 accepted"`
   must print 1, and `crates/retro-progman` must exist. The M1/M2 branch
   `claude/blissful-heisenberg-j88hfe` was GitHub's default branch when M3
   ended and does **not** contain M3; M3 and the Touch Display 2 work are on
   `claude/retroos-m3-desktop-32rra9`. If your branch was cut from the old
   default, stop and tell the user (they should merge the M3 branch or make
   it the default), or with their OK `git fetch origin
   claude/retroos-m3-desktop-32rra9` and build on that.
2. **First job: the Touch Display 2** (D-042). The code is in; it has not
   been seen on hardware. Ask the user to run `docs/HW_CHECKLIST.md`
   "Displays" D.1–D.6 on the latest CI image and fix what they report
   before starting M4 proper (likely topics: which way up — `rotate=` /
   `retroos.rotate=`; text size at 1280×720; touch mapping; multi-output).
3. Then M4, in the order of §7 below.

## 1. Where things stand

- **M1, M2 and M3 are complete** and accepted on real hardware: a Raspberry
  Pi 5 (8 GB), 5 A USB-C supply, the bootloader settings card from CI
  (SD first, no network-install screen), the Raspberry Pi 15.6" 1080p
  monitor. The user tests CI images and reports by photo of the screen.
- **The luggable**: the Pi is going into the user's own "luggable" PC case,
  designed in a separate Claude session (3D printed). Its screen is the
  **10.1" Raspberry Pi Touch Display 2** (DSI, 1200×1920 portrait panel
  shown landscape as 1920×1200, touch).
  The first attempt failed with Station's "no usable display" (D-042 has the
  why); the fix is pushed and awaiting the hardware check (§0.2).
- Measured on the Pi 5 (`docs/BUDGET.md`): power-on → POST screen 6 s,
  → `C:\>` 9 s; idle userspace (AnonPages + Shmem) text mode 44 MB, Desktop
  52.8 MB (target 160); Firefox windowed on the GPU.
- CI (`.github/workflows/ci.yml`): lint, unit and host tests on ubuntu-24.04;
  image build + QEMU M1, M2, M3 tests on ubuntu-24.04-arm (~22–30 min). The
  flashable image is the `retroos-image` artifact, uploaded ~6 min in, before
  the QEMU tests (screens and logs: `qemu-tests`); the Pi 5 bootloader card
  is `retroos-bootloader-rpi5`. Docs-only pushes start no run; a newer push
  cancels the branch's running one. snapshot.debian.org is flaky: the build
  falls back to deb.debian.org for missing files (`fallback_debs`).
- Hardware results: `docs/HW_CHECKLIST.md` (results table at the end).

## 2. Repository map

| Path | What |
|---|---|
| `crates/station` | The Wayland compositor (Smithay 0.7): text and desktop personalities, XWM, layer-shell, launch broker. |
| `crates/retro-console` | Wayland terminal that shows RetroDOS (the DOS Prompt; app id `retroos.console`). |
| `crates/retrodos` | The DOS-style shell: builtins, batch files, broker client, GUI-app detection. |
| `crates/retro-gfx` | Palette/schemes/themes, Canvas, bitmap fonts (Phosphor Mono, Lantern 12/9), widgets, icons (`icons`, `iconset`), pointers (`cursors`), screen rotation (`screen`). |
| `crates/retro-ui` | Toolkit for windowed and bar clients (wl_shm at 1×, viewporter; menus, controls, dialogs, clipboard). |
| `crates/retro-apps` | `.desktop` parsing (categories, icons, MIME types), shared with RetroDOS. |
| `crates/retro-bar` | System Bar (◆ menu, Programs, Window, clock; Restart / Shut Down / Exit to DOS). |
| `crates/retro-progman`, `retro-about`, `retro-jot`, `retro-cabinet` | Program Manager, About This Computer, Jot (`jot`), File Cabinet (`file-cabinet`). |
| `crates/retro-tui` | Full-screen text UI layer (EDIT, FILER, NET, Setup). |
| `crates/retro-edit`, `retro-filer`, `retro-net` | EDIT, FILER, WIFI/NET (edit/filer are also libraries for Jot/File Cabinet). |
| `crates/retro-sys` | System services for both front-ends: `net` (iwd + networkd), `clock`, `keyboard`, `display`, `startup`, `admin` (D-043). |
| `crates/retro-control` | Control Panel: applet model, CONTROL (text) and `control-panel` (Desktop) (D-043). |
| `crates/retro-login`, `retro-session` | Login screen + first-boot Setup; the user session (text/desktop, restarts on crash). |
| `crates/retro-post`, `retro-kms` | POST/BIOS screen, chime, F8 Startup Menu, DEL Setup (direct KMS, honours rotation). |
| `crates/retro-dosfs` | Drive letters, DOS paths, 8.3 names, wildcards. |
| `tools/retro-fontc`, `retro-synth`, `retro-cursors` | Build-time font compiler, sound synthesiser, Phosphor Xcursor theme writer. |
| `assets/` | Font glyph sources, `.icon` sources (text, one letter per pixel). |
| `packaging/retroos/root` | Files installed verbatim by the `retroos` .deb (units, udev rules, helpers, `.desktop` files, `/etc/retroos/startup.conf`). |
| `build/` | `mkdeb.sh`, `mkimage.sh`, `hooks/customize.sh`, `packages/*.list`, `mkeeprom.sh`, pinned keyrings/snapshot (`lock/`). |
| `config/boot` | `config.txt`, `cmdline.txt` for the Pi firmware. |
| `tests/host` | Headless-Station scenarios (`station.py`, `*.steps`, `run-all.sh`); goldens in `tests/golden`. |
| `tests/qemu` | Full-image tests in QEMU (`retroqemu.py`, `test_m1.py`…`test_m3.py`). |

## 3. Building and testing

```
cargo fmt --all && cargo clippy --workspace --all-targets   # must be clean (CI checks)
cargo test --workspace     # unit tests
tests/host/run-all.sh      # every headless Station scenario, exactly as CI; screens in out/test-host*/
make image                 # flashable image + QEMU variant (root; ~1 h off arm64: let CI do it)
make test-qemu             # M1–M3 QEMU tests on a built image
```

- **Host tests are the workhorse.** `tests/host/station.py BIN_DIR OUT_DIR
  SCRIPT` starts `station --headless 1920x1080 --test-ipc` with a private
  HOME, then runs steps: `type`, `key`, `hold`, `wait`, `shot`,
  `expect-task`, `expect-file`, `expect-pixel`, `expect-golden SHOT NAME X Y
  W H`, `pointer`, `button`, `request`. A rejected key or request fails the
  run. **Look at the screenshots**: several bugs were only visible there.
  A deliberate look change: rerun the scenario with `RETRO_UPDATE_GOLDEN=1`
  and commit the new PNGs; a mismatch saves `golden-NAME-got.png`.
- The dev container needs `xwayland x11-apps libegl1` for the X11 scenario
  (`apt-get install -y` works in the cloud container); the Rust toolchain
  is pinned by `rust-toolchain.toml`.
- Adding a crate changes `Cargo.lock`; the image build uses `--locked`.
  Commit the lock file.
- After `cargo fmt`, text replacements written against pre-format code
  silently miss; re-read the file. Chain commit commands with `&&`, never `;`
  (a failed check must stop the commit).
- Station env switches used by tests: `RETROOS_NO_BAR`, `RETROOS_NO_PROGMAN`,
  `RETROOS_NO_DESKTOP_ICONS`, `RETROOS_NO_XWAYLAND`, `RETROOS_SCHEME`,
  `RETROOS_RENDERER`, `RETROOS_UI_SCALE`, `RETROOS_ROTATE`.

## 4. Working with the user and the hardware

- The user flashes the CI artifact and reports by photo, often with short
  notes per checklist row ("3.31: good"). Give short, exact commands; RetroDOS
  runs Linux commands with `UNIX "…"` and as root with `ADMIN UNIX "…"`.
  Memory check: see the end of the M3 section of `docs/HW_CHECKLIST.md`.
- Add checklist rows before asking for a hardware run; record every run in
  the results table and numbers in `docs/BUDGET.md`. Never claim a hardware
  result you did not get from the user.
- The user is happy to iterate ("worst case we circle back"), likes the
  retro feel (kept the long-ish POST screen on purpose), and wants each
  milestone shippable.
- This cloud session cannot reach www.raspberrypi.com (network policy);
  web search works.

## 5. Conventions

- Rust for our components; match the surrounding code and comment density.
  No Electron, no GNOME/KDE/Xfce components.
- Every deviation from DESIGN.md gets a `docs/DECISIONS.md` entry
  (**next: D-052**).
- Small, descriptive commits; push to the session's designated branch; no PR
  unless the user asks. Keep model names out of the repo.
- Keep M1–M3 tests passing; add host scenarios and QEMU checks for new work.

## 6. Station internals

- `state.rs`: `Station` (Smithay states, `space`, `tasks`, `personality`,
  `scale` = output scale programs see, `art_scale` = pixel scale of our own
  art, `desktop`, `x11_overrides`, `touch_slot`); `output_size()` (logical,
  after rotation), `output_pixels()`; `spawn()` sets each program's env
  (`DISPLAY`, `XCURSOR_*`).
- `desktop.rs`: the desktop personality — placement, stacking, move/resize,
  minimise-to-icon, window menu, Alt+Tab/Esc/F4, shell icons (drives,
  Wastebasket), Program Manager management, app icons.
- `chrome.rs`: frames, title bars, icons, menus, boxes (pure drawing).
- `render.rs`: `Scene` → render elements; `transform_for()` (rotation);
  `offscreen*` (screenshots).
- `udev.rs`: one KMS output (first connected connector), output transform
  from `retro_gfx::screen`, Pixman ↔ GLES switching (`set_gpu`, D-029).
- `input.rs`: libinput → seat; hotkeys; touch as the left button
  (`panel_to_picture`).
- `xwayland.rs`: on-demand Xwayland + `XwmHandler` (D-039).
- `layers.rs`: layer-shell (System Bar). `broker.rs`: the line protocol
  (LAUNCH, TASKS, SWITCH, SCHEME, WATCH, SCREENSHOT, …).

## 7. Milestone 4 plan (DESIGN.md §18)

Suggested order, each step leaving a working image:

1. **Touch Display 2 follow-ups** from the hardware check (§0.2); then
   `wl_touch` for programs (Firefox finger scrolling) and, if the luggable
   wants both screens, more than one output.
2. **retro-sys complete + Control Panel** (text + GUI) with its applets:
   Display (scheme, rotation, scale, brightness for the DSI panel), Startup
   (startup.conf), Date & Time, Keyboard, Network (Wi-Fi Enterprise, static
   IP, country — deferred by D-026), Users, Sound, Bluetooth.
3. **Audio**: PipeWire/WirePlumber; `MIXER`/Volume; UI sounds (STYLE.md).
4. **Bluetooth** pairing in both personalities (keyboard, headphones, gamepad).
5. **Software Library** (text + GUI) on PackageKit + the catalog (~60
   entries) with retro-fied icons (`retro_gfx::iconset::retrofy`); updates,
   notifications, unattended security updates.
6. **Theming of others' programs**: GTK3/GTK4/Qt/Firefox/Chromium colours
   and fonts (the pointer theme is done: D-040).
7. **Media Player** (libmpv) and **Arcade v1** (RetroArch); Geany
   preinstalled, VS Code in the catalog.
8. **Acceptance**: install/remove apps from both front-ends; BT audio and a
   gamepad on the Pi; budget re-measured.

## 7a. M4 progress

- **Step 2 started (2026-11-07, D-043)**: `retro-sys` gained `startup`,
  `clock` (timedated), `keyboard`, `display` (backlight via logind) and
  `admin` (runs `/usr/libexec/retroos/*` helpers through `sudo -n`); new
  helper `set-startup`; sudoers NOPASSWD for it and `set-keyboard`; polkit
  rule for timedated. New crate `retro-control`: the applet model
  (`lib.rs`), CONTROL (`text.rs`, binary `retro-control`) and the Desktop
  Control Panel (`gui/main.rs`, binary `control-panel`). Applets: Display,
  Date & Time, Keyboard, Network (opens NET), Startup. Host test
  `control.steps` (writes `~/startup.conf` via `RETROOS_STARTUP_CONF`).
  Hardware: 4.1–4.7 and D.1–D.6 passed on the Pi (2026-11-07); 4.8
  rotation reworked (D-044), monochrome text attributes (D-044); 4.8–4.10
  to run.
- **Network page done (D-045)**: `retro_sys::netconf` (country, fixed wired
  address, enterprise provisioning) + helpers `set-wifi-country`,
  `set-wired`, `set-wifi-enterprise`; Desktop *Wireless Networks* dialog
  (`retro-control/src/gui/wifi.rs`; `RETROOS_TEST_WIFI=1` = pretend adapter
  for host tests); NET asks for enterprise sign-in too. HW rows 4.11–4.15.
- **Users done (D-046)**: `retro_sys::users` + `admin::authorize`
  (sudo -K / -S -v / -n), helper `user-admin` (not NOPASSWD), form model
  `retro_control::forms` drawn by `text.rs` and `gui/formdlg.rs`. HW rows
  4.16–4.20 (4.16 also proves the sudo sequence, untestable in CI).
- **Step 3 started (D-047)**: PipeWire packages + user units;
  `retro_sys::sound` (pw-dump/wpctl; `RETROOS_TEST_SOUND=1` = pretend);
  crate `retro-mixer` (MIXER, VOLUME); Control Panel → Sound; host test
  `sound.steps`; HW rows 4.21–4.25.
- **Next in step 3**: System Bar volume item + popup, the Desktop Volume
  window, interface sounds (Station `SOUND` request + console bell); then
  Bluetooth (step 4).

## 8. Open items carried over

- Touch Display 2 unverified on hardware (D-042, HW_CHECKLIST D.1–D.6).
- "Not yet" lists in D-030…D-042, notably: X11 ↔ Wayland clipboard; animated
  busy pointer; SVG-only app icons; dialog icons (Stop/Caution/Note).
- "used" memory (MemTotal − MemAvailable) is kernel-heavy; M5 ideas in
  BUDGET.md (blacklist unused drivers, trim initramfs-tools).
- Untested on hardware: Ethernet (2.4), Pi 4 (1.23).
- Deferred by D-026: EDIT multiple buffers/syntax colours, FILER archives.

## 9. What M3 built (for orientation)

Desktop personality (D-030/031), layer-shell (D-032), retro-ui + System Bar,
system-wide colour scheme (D-033), Lantern 12/9 with Latin-1, Program
Manager (D-035), About This Computer (D-036), Jot (D-037), File Cabinet
(D-038), desktop drive icons and Wastebasket, Xwayland + XWM (D-039),
Phosphor pointers everywhere incl. GTK settings (D-040), icon set with
retro-fied third-party icons (D-041), golden screenshot regions, QEMU M3
test, Pi 5 boot time 20 s → 9 s (unpacked kernel, bootloader settings card).
