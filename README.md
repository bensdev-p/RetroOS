# RetroOS

A retro-styled, lightweight operating system for the Raspberry Pi 5 and 4
(arm64): the look and feel of 1985–1993 — MS-DOS meets Windows 3.1 meets ProDOS
and System 7 — on top of a modern Linux kernel that runs real modern software.

RetroOS 1.0 is "made by" the fictional **Phosphor Computer Company**. It boots
to the **Phosphor BIOS** POST screen, then to **RetroDOS**, a DOS-style command
shell; the **RetroOS Desktop** (Milestone 3) is one command away.

- [`docs/DESIGN.md`](docs/DESIGN.md) — architecture, build system, milestones
- [`docs/STYLE.md`](docs/STYLE.md) — palette, fonts, icons, sounds, naming
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — decision log
- [`docs/BUDGET.md`](docs/BUDGET.md) — measured boot time and memory
- [`docs/HW_CHECKLIST.md`](docs/HW_CHECKLIST.md) — real-hardware test list

## Status: Milestone 1

| Works now | Arrives in |
|---|---|
| Phosphor BIOS POST screen with real hardware data, startup chime, F8 Startup Menu, DEL Setup | M1 ✓ |
| First-boot Setup (account, computer name, time zone, keyboard, automatic login) and the login screen | M1 ✓ |
| Station compositor (text personality: full-screen tasks, Ctrl+Alt+Tab task swapper, Print Screen) | M1 ✓ |
| RetroDOS: DOSKEY line editing, drive letters, pipes, redirection, ~35 commands, `UNIX` POSIX shell | M1 ✓ |
| `RUN FIREFOX` full-screen from the prompt, batch files, EDIT, FILER, Wi-Fi setup | M2 |
| RetroOS Desktop: windows, Program Manager, File Cabinet | M3 |
| Control Panel, Software Library, audio, Bluetooth, app theming | M4 |

## Using RetroDOS

```
C:\>DIR /W                 wide listing
C:\>CD DOCUME~1            8.3 aliases work, so does CD Documents
C:\>COPY *.TXT D:\         D: and up are USB drives; Z: is the whole Linux tree
C:\>TYPE README.TXT | MORE
C:\>MEM /C                 memory by program
C:\>COLOR AMBER            schemes: CLASSIC, AMBER, GREEN, PAPER, CONTRAST
C:\>UNIX                   a bash shell; EXIT comes back
C:\>HELP                   every command; COMMAND /? for details
```

Drives: `C:` is your home folder, `Z:` is `/`, removable media get `D:` upward.
Keys: ↑/↓ history, F3 repeat the last command, Tab completes file names,
Ins toggles insert/overtype, Ctrl+Alt+Tab switches tasks, Print Screen saves
a screenshot to `C:\SCREENS`.

## Building

One command builds the flashable image from source:

```sh
make image          # → out/retroos-<version>-rpi-arm64.img (+ .sha256, out/manifest.txt)
make image.xz       # compressed for distribution
```

Host requirements (Debian 13 or Ubuntu 24.04, x86_64 or arm64, run as root):
`mmdebstrap qemu-user-static dosfstools mtools e2fsprogs fdisk dpkg-dev xz-utils`,
the Rust toolchain from `rust-toolchain.toml` with the `aarch64-unknown-linux-gnu`
target, `gcc-aarch64-linux-gnu`, and arm64 dev packages (`libinput-dev:arm64
libxkbcommon-dev:arm64 libseat-dev:arm64 libgbm-dev:arm64 libudev-dev:arm64
libpixman-1-dev:arm64`). Or skip all that: `make container` builds inside the
pinned Debian container (`build/Containerfile`).

Reproducibility: Debian packages come from `snapshot.debian.org` at the
timestamp in `build/lock/snapshot`; keyrings and third-party assets are pinned
by hash under `build/`; `Cargo.lock` is committed; every installed package
and version is listed in `out/manifest.txt`.

## Flashing

Raspberry Pi Imager → *Choose OS* → *Use custom* → pick the `.img` or `.img.xz`.
Or on Linux:

```sh
xz -dc out/retroos-*-rpi-arm64.img.xz | sudo dd of=/dev/sdX bs=4M conv=fsync status=progress
```

First boot grows the root partition to fill the card and runs Setup.

## Testing

```sh
make test           # unit tests + headless Station end-to-end (screenshots in out/test-host)
make test-qemu      # boots the image in QEMU, walks through Setup, checks the prompt,
                    # measures boot time and memory (out/test-m1/report.json)
make qemu           # interactive QEMU session (VNC on :5901)
```

QEMU cannot emulate a Pi 5, so the QEMU image is the same root file system
with Debian's generic arm64 kernel on `-M virt` with virtio-gpu. Pi-specific
hardware is covered by `docs/HW_CHECKLIST.md`.

## Repository layout

```
crates/   retro-gfx (pixels, palette, fonts) · retro-dosfs (drive letters) · retrodos (shell)
          station (compositor) · retro-console (terminal) · retro-post (POST screen)
          retro-login (greeter + Setup) · retro-session · retro-tui · retro-kms
tools/    retro-fontc (font compiler) · retro-synth (sound synthesiser)
assets/   fonts/phosphor-mono-8x16.glyphs — the hand-drawn font source
packaging/retroos/root/   system configuration shipped in the retroos package
build/    mkdeb.sh, mkimage.sh, hooks, package lists, keyrings, locks, Containerfile
config/boot/   config.txt and cmdline.txt for the Pi firmware
tests/    host/ (headless Station) · qemu/ (image boot tests)
```

## License

GPL-3.0-or-later. The Phosphor Mono font is SIL OFL 1.1.
