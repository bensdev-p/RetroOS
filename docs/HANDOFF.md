# Handoff: starting Milestone 3 (RetroOS Desktop)

Written at the end of the M1+M2 session (2026-10-17) for whoever picks up M3.
Read this first, then `docs/DESIGN.md` §6.2, §9 and §18 (M3),
`docs/STYLE.md` §4, and the decisions D-007/D-023/D-029 (GPU), D-025
(Xwayland), D-027 (scales) and D-028 (memory budget) in `docs/DECISIONS.md`.

## 1. Where things stand

- **M1 and M2 are complete** and verified on real hardware: a Raspberry Pi 5
  (8 GB) with the Raspberry Pi 15.6" 1080p monitor, owned by the user, who
  tests images and reports results as photos of the screen.
- All work is on branch `claude/blissful-heisenberg-j88hfe`. It is the **only
  branch on GitHub** (no `main` yet). A local annotated tag `v0.2-m2` marks the
  M2 commit `5162352`; the session could not push tags, so it may not exist on
  GitHub.
- CI (`.github/workflows/ci.yml`) is green: lint, unit and host tests on
  ubuntu-24.04; image build + QEMU M1 and M2 tests on ubuntu-24.04-arm
  (~16 min). The flashable image is the `retroos-image` artifact of each run.
- Hardware results so far: `docs/HW_CHECKLIST.md` (results table at the end).
  Measured numbers: `docs/BUDGET.md`. Idle text mode on the Pi 5: RetroOS
  userspace (AnonPages + Shmem) **44 MB** (target 60), MemTotal − MemAvailable
  224 MB (target 200; the remainder is kernel-side).

## 2. Repository map

| Path | What |
|---|---|
| `crates/station` | The Wayland compositor (Smithay 0.7). Text personality only so far. |
| `crates/retro-console` | Wayland terminal that shows RetroDOS (alacritty_terminal grid, our bitmap font). |
| `crates/retrodos` | The DOS-style shell (`retrodos`): builtins, batch files, launch-broker client, GUI-app detection. |
| `crates/retro-gfx` | Palette (Phosphor16), Canvas/Surface, bitmap fonts (Phosphor Mono 8×16 + Unifont fallback), text Grid. |
| `crates/retro-tui` | Minimal full-screen text UI layer (cells, keys incl. modifiers + SGR mouse, dialogs, fields). |
| `crates/retro-edit`, `retro-filer`, `retro-net` | EDIT, FILER, WIFI/NET text apps (binaries `retro-edit`, `retro-filer`, `retro-net`). |
| `crates/retro-sys` | System services over D-Bus for all front-ends (`net`: iwd + networkd). Extend here for M4 Control Panel. |
| `crates/retro-login` | Login screen (greetd greeter) and first-boot Setup (text). |
| `crates/retro-session` | The user session: picks text/desktop mode, runs Station + console, restarts on crash. |
| `crates/retro-post`, `retro-kms` | POST/BIOS screen with chime, F8 Startup Menu, DEL Setup (direct KMS). |
| `crates/retro-dosfs` | Drive letters, DOS path resolution, 8.3 names, wildcards. |
| `tools/retro-fontc`, `tools/retro-synth` | Build-time font compiler and sound synthesiser. |
| `packaging/retroos/root` | Files installed verbatim by the `retroos` .deb (systemd units, udev rules, helpers, policies). |
| `build/` | `mkdeb.sh`, `mkimage.sh`, `hooks/customize.sh`, package lists, pinned keyrings and snapshot. |
| `tests/host` | Headless-Station end-to-end tests (`station.py` + `*.steps` scripts). |
| `tests/qemu` | Full-image tests in QEMU (`retroqemu.py`, `test_m1.py`, `test_m2.py`). |
| `docs/` | DESIGN, STYLE, DECISIONS (D-001…D-029), BUDGET, HW_CHECKLIST, this file. |

## 3. Building and testing

```
make test           # cargo test + all host tests (fast, no root)
make test-host      # headless Station scenarios only; screenshots in out/test-host*/
make image          # flashable image + QEMU variant (needs root; slow off arm64)
make test-qemu      # M1 then M2 QEMU tests (needs a built image)
cargo fmt --all && cargo clippy --workspace --all-targets   # must be clean (CI checks)
```

- **Host tests are the workhorse.** `tests/host/station.py BIN_DIR OUT_DIR
  SCRIPT` starts `station --headless 1920x1080 --test-ipc` with a private HOME
  (with a fake `DEMO` app in `~/.local/share/applications`), then runs steps:
  `type`, `key ctrl+alt+tab`, `wait MS`, `shot NAME`, `expect-task N`,
  `expect-file NAME TEXT`, `request BROKER REQUEST`. A rejected key or request
  fails the run. Look at the screenshots — several bugs were only visible there.
- **Image builds on an x86 container are very slow** (qemu-user emulation,
  ~1 h). Prefer pushing and letting CI build and run QEMU tests on the arm64
  runner. `mkimage.sh` registers the qemu-aarch64 binfmt handler itself.
- **Adding a crate changes `Cargo.lock`; the image build uses `--locked`.**
  Build once locally and commit the lock file before an image build starts.
- Network: Debian packages come only from `snapshot.debian.org` (pinned in
  `build/lock/snapshot`); the Raspberry Pi archive over **https**. crates.io
  works. The Ubuntu archive of the dev container does not (no `apt install`).
- After `cargo fmt`, text replacements written against pre-format code silently
  miss; re-read the file or use exact current text.

## 4. Working with the user and the hardware

- The user flashes the CI artifact on the Pi 5 and reports by photo. Give them
  short, exact commands. The memory check used:
  `UNIX "grep -E 'MemTotal|MemFree|MemAvail|Cached|Shmem:|Slab|SUnrecl|KernelSt|PageTab|Vmalloc|Percpu|Cma|AnonPages' /proc/meminfo; ps -eo rss,comm --sort=-rss | head -12"`
  and `UNIX free -m`. GPU check: `ADMIN UNIX journalctl -b -g rendering`.
- Add an M3 section to `docs/HW_CHECKLIST.md` before asking for a hardware run;
  record every run in its results table and numbers in `docs/BUDGET.md`.
- Never claim a hardware result you did not get from the user.

## 5. Conventions

- Rust everywhere for our components; match the surrounding code and comment
  density. No Electron, no GNOME/KDE/Xfce components.
- Every deviation from DESIGN.md gets a `docs/DECISIONS.md` entry (next: D-030).
- Commit small, descriptive commits; push to the session's designated branch;
  do not open a PR unless the user asks. Keep model names out of the repo.
- Keep the M1/M2 tests passing; add host scenarios and QEMU checks for M3.

## 6. Station internals you will touch first

- `state.rs`: `Station` (Smithay states, `space`, `tasks`, `personality`,
  `scale` = output scale clients see, `art_scale` = pixel scale of Station's
  own bitmap art, `children`, `broker`, `task_list`). `add_toplevel` /
  `configure_task` / `focus_task` / `remove_toplevel` implement the text
  personality: every toplevel is configured full-screen, dialogs (toplevels
  with a parent) are centred over their parent. `Personality` already exists
  (`--personality text|desktop`), but only text behaviour is implemented.
- `tasks.rs`: `Tasks` list + MRU focus; `TaskKind::{Console, App}`; tasks
  carry `pid`, `launch` id and `name`.
- `render.rs`: `Scene` → render elements (pointer, optional full-screen
  overlay, focused task and its dialogs). Desktop mode needs stacking order,
  frames, desktop background, icons — likely a separate scene builder.
- `udev.rs`: one KMS output; renderer is `Option<Gpu>` = Pixman (dumb
  buffers) or GLES (GBM), switched by `set_gpu()`: GPU when the broker
  launches a graphical program, CPU 60 s after the last one ends (D-029).
  `renderer=gpu|cpu|auto` in `/etc/retroos/startup.conf` pins it. Desktop mode
  will usually have GPU clients, so expect GLES most of the time there.
- `headless.rs`: Pixman into an offscreen image (tests, screenshots).
- `input.rs`: libinput → seat; hotkeys (Ctrl+Alt+Tab, Ctrl+Alt+Esc/Del Task
  List, Ctrl+Alt+F1..F12, Print Screen); `xkb_config()` reads
  `/etc/default/keyboard`.
- `broker.rs`: line protocol on `$RETROOS_STATION` (PING, TASKS, SWITCH,
  LAUNCH with `=NAME=VALUE` env fields, KILL, END, KEYMAP, SCREENSHOT;
  TYPE/KEY/QUIT with `--test-ipc`). RetroDOS's `launch.rs` is the client.
- `tasklist.rs`: the full-screen Task List, drawn with `retro-gfx` into a
  `MemoryRenderBuffer` — a working example of Station-drawn UI.
- Protocols offered: compositor, shm, xdg-shell, xdg-decoration, seat, data
  device, output, viewporter, linux-dmabuf (GPU only). Missing for M3:
  layer-shell (System Bar/desktop as clients) and Xwayland/XWM.
- retro-console uses `wp_viewporter` to send a 1× buffer that Station enlarges
  with nearest-neighbour filtering (renderers are set to `TextureFilter::Nearest`).
  Any bitmap UI client (retro-ui apps) should do the same.

## 7. Milestone 3 plan (from DESIGN.md §18, with what M2 changed)

Suggested order, each step leaving a working image:

1. **Desktop personality in Station**: stacking window manager with
   server-side Win 3.1 frames (STYLE.md §4: 4-px frame, 18-px Cobalt title bar,
   control-menu box, minimise/maximise), click-to-focus, Alt+Tab, Alt+F4,
   Alt+Space window menu, move/resize with an XOR outline, minimise to icons on
   the bottom edge, dithered desktop. `DESKTOP` command and "Exit to DOS" switch
   personality live (no logout); the RetroDOS console becomes a "DOS Prompt"
   window. Firefox windowed works from day one. Respect D-027: windows use the
   client scale; frames and our art use `art_scale`.
2. **layer-shell** for System Bar and desktop clients (or draw them in Station
   first, like the Task List, and move them to clients later — decide and log).
3. **retro-ui toolkit** (DESIGN §9.3: software-rendered into wl_shm at 1×,
   enlarged via viewporter) and the **Lantern 12/9** proportional bitmap fonts
   (text glyph sources like `assets/fonts/phosphor-mono-8x16.glyphs`, compiled
   by `retro-fontc`).
4. **System Bar** (◆ Phosphor menu, Programs, Window, clock, network) and
   **Program Manager** (groups from `.desktop` categories).
5. **File Cabinet**, **Jot**, **About This Computer** (memory bar per program).
6. **Xwayland + X11 WM** (D-025): start Xwayland on the first launch of a
   program that links libX11/libxcb but not libwayland-client; `DISPLAY` for it.
7. **Icon set and cursor theme** (original art from text sources, D-015/D-021).
8. **Tests and acceptance**: host golden screenshots of the desktop, QEMU GUI
   test (switch to desktop, open/move/close windows), Firefox windowed on the
   Pi with the GPU, desktop idle RAM measured (target: userspace < 160 MB, D-028).

## 8. Open items carried over

- Firefox start-up latency fix (GPU switch after spawn, 60 s hold) is in the
  branch but not yet confirmed on the Pi.
- Boot stopwatch values (HW_CHECKLIST 1.1, 1.8) were reported "in spec" but
  not recorded; ask for the numbers at the next hardware run.
- Untested on hardware: Ethernet (2.4), Pi 4 (1.23).
- "used" memory is 24 MB over its 200 MB target, all kernel-side; M5 ideas in
  BUDGET.md (blacklist unused drivers; the Pi kernel pulls in an unused
  initramfs-tools — trim it).
- Deferred by D-026: EDIT multiple buffers/syntax colours, FILER archives,
  Wi-Fi Enterprise/static IP/country (M4 Control Panel).
