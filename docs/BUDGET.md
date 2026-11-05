# Resource budget — measured

Definitions (DESIGN.md §16):
- **Idle RAM** = MemTotal − MemAvailable, 60 s after the prompt appears, no user programs open.
- **Boot time** = power-on → usable `C:\>` (stopwatch on hardware); in QEMU we report
  `systemd-analyze` (kernel + userspace) because TCG emulation is 10–30× slower than a Pi 5
  and its wall-clock time says nothing about real hardware.

Targets: boot to prompt < 10 s on Pi 5 (aim 7 s) · idle RAM < 100 MB text mode · < 200 MB GUI mode.

## Milestone 1

### QEMU (`make test-qemu`, 2026-10-17: TCG on a 4-core x86_64 host, Debian generic arm64 kernel 6.12, virtio-gpu 1280×800, 2 GB)

All 14 M1 checks pass: POST screen, first-boot Setup, automatic login, RetroDOS
commands, no failed units, second boot straight to the prompt.

| Metric | Value | Notes |
|---|---|---|
| Idle RAM, text mode — classic used (total − free − buffers − cache + shmem) | **117 MB** | |
| Idle RAM, text mode — MemTotal − MemAvailable | 181 MB | inflated by the generic kernel's zone reserves on a 2 GB VM |
| Breakdown | anon 27.5 · shmem 21.3 · slab 27.6 · vmalloc 13.2 · kernel stacks 1.7 · page tables 1.4 · percpu 1.0 MB | processes: Station 17 MB RSS, retro-console 18 MB, systemd-* 7–13 MB each |
| systemd-analyze, second boot | 19.5 s kernel + 47.1 s userspace | emulated (TCG ≈ 15–30× slower than a Pi 5) |
| Slowest units, second boot | dev-vda 21.6 s, udev-trigger 13.8 s, retro-post 12.7 s, iwd 11.9 s, journald 7.3 s, nftables 7.3 s | relative order is the useful part |
| Failed units | none | |

What this says about the Pi: process memory is ~35 MB for RetroOS itself; the
rest is kernel-side and depends on the kernel. Biggest RetroOS-controlled items
to cut if the Pi misses 100 MB: Station's swapchain + the console's shm buffers
(~20 MB of shmem at 1280×800, ~40 MB at 1080p) and retro-console's private
copy of the screen. Those are M5 tuning items, prioritised by the Pi numbers.

### Raspberry Pi 5 (8 GB)

| Metric | Value | Notes |
|---|---|---|
| Power-on → POST screen | _to measure_ | HW_CHECKLIST 1.1 |
| Power-on → `C:\>` | 17.7 s (cold, 2026-11-04; see Milestone 3) | HW_CHECKLIST 1.8 |
| Idle RAM, text mode | _see Milestone 2_ | HW_CHECKLIST 1.18 |

## Milestone 2

### Raspberry Pi 5 (8 GB), first hardware run (2026-10-17, image from CI run 15)

| Metric | Value | Notes |
|---|---|---|
| Renderer | GPU (V3D) | HW_CHECKLIST 2.1 |
| Idle RAM, text mode (MemTotal − MemAvailable, `free -m` "used") | **276 MB** | ❌ target < 100 MB; breakdown requested (below) |
| With Firefox playing YouTube | 1,247 MB used, 220 MB shared | one tab |

Breakdown from `/proc/meminfo` on the Pi (idle at `C:\>`, second run):

| MB | What | Who controls it |
|---:|---|---|
| 278.7 | MemTotal − MemAvailable ("used") | |
| 45.8 | free memory MemAvailable holds back (zone watermarks; scales with RAM) | kernel |
| 62.0 | anonymous memory, all processes | RetroOS + systemd services |
| 31.5 | shmem: the console's two 1080p frame buffers (16.6) + Station's GPU buffers | RetroOS |
| 34.3 | unreclaimable slab | kernel (some driven by services) |
| 20.2 | vmalloc | kernel/drivers |
| 6.3 | kernel stacks, page tables, per-CPU | kernel |
| 10.0 | CMA in use (display scan-out) | kernel/Station |
| 68.5 | other kernel and driver pages (Wi-Fi/GPU firmware, DMA rings, …) | stock RPi kernel |

Biggest processes (RSS, includes shared libraries): station 103 MB (Mesa/LLVM
libraries mapped since GLES; 17 MB on Pixman in QEMU), retro-console 31 MB,
systemd 15, systemd-resolved 14, systemd (user) 12, networkd 11, udevd 11.

**Finding (D-028):** about 180 MB of the 279 MB is the stock Raspberry Pi
kernel, its drivers and its reserves, before any RetroOS process. Under the
"no custom kernel" rule, < 100 MB MemTotal − MemAvailable is not reachable on
an 8 GB Pi 5. The budget is re-stated as RetroOS's own share — **userspace
(AnonPages + Shmem) < 60 MB in text mode** (now 93.5 MB) — plus "used" < 200 MB.
Planned cuts, largest first:

1. ✅ retro-console draws at font scale 1 and Station enlarges it with nearest-
   neighbour scaling (viewporter): its 1080p buffers shrink 4×. Measured on the
   headless Station at 1920×1080: RSS 30.3 → 12.3 MB (anon 9.6 → 3.5, shmem
   16.2 → 4.1), screenshots pixel-identical.
2. ✅ (implemented, to be measured on the Pi) Station stays on Pixman in text
   mode and brings GLES up only while a graphical program runs (D-023's
   fallback): no Mesa, LLVM or GPU buffers at the prompt. `renderer=gpu` in
   `/etc/retroos/startup.conf` restores always-GPU.
3. Services: measure what dropping the per-user systemd instance and
   systemd-resolved would save (they come back with PipeWire/DNS needs).

### Raspberry Pi 5 (8 GB), after the M2 memory cuts (2026-10-17, CI run 24)

| Metric | Before (run 15) | After (run 24) | Target (D-028) |
|---|---:|---:|---:|
| RetroOS userspace: AnonPages + Shmem | 93.5 MB | **44.4 MB** (34.2 + 10.3) | < 60 MB ✅ |
| MemTotal − MemAvailable ("used") | 278.7 MB | **224.4 MB** | < 200 MB ❌ (kernel side) |
| …of which kernel watermark reserve | 45.8 MB | 49.8 MB | |
| Unreclaimable slab / vmalloc | 34.3 / 20.2 MB | 31.0 / 20.1 MB | |
| station RSS | 103 MB | 31.8 MB | |
| retro-console RSS | 30.8 MB | 9.9 MB | |
| Boot to POST / to `C:\>` | — | within target (reported by the tester; stopwatch values not recorded) | < 10 s |

The remaining gap to 200 MB is kernel-side (reserve, slab, vmalloc, drivers).
M5 candidates: stop auto-loading drivers RetroOS does not use yet (camera/ISP,
V4L2 codecs; vmalloc is mostly module memory), and re-check the watermark
reserve, which on an 8 GB board is set by the kernel's defaults, not by us.

## Milestone 3

### Headless Station, 1920×1080 (host, release build, 2026-10-17)

Station on Pixman with the DOS Prompt only (text mode), then after `DESKTOP`
with the DOS Prompt as a 1280×800 window:

| Process | Text mode (RSS / anon / shmem) | Desktop (RSS / anon / shmem) |
|---|---:|---:|
| station | 21.8 / 9.4 / 4.1 MB | 23.2 / 11.7 / 3.1 MB |
| retro-console | 10.4 / 3.4 / 4.1 MB | 8.2 / 2.3 / 3.0 MB |

With the System Bar (after M3 step 2): station 24.3 / 11.8 / 4.1 MB, retro-bar
**3.8 MB RSS (0.9 MB anonymous, 0.1 MB shmem: its 960×20 buffer)**.

The Desktop costs Station ≈ 2.3 MB of anonymous memory: the pattern band
(1920×128, ~1 MB), title bars (~0.2 MB per window at 1080p) and their
textures. (Headless Station also holds its 8 MB off-screen image.)

### QEMU and Raspberry Pi 5

| Metric | Value | Notes |
|---|---|---|
| Desktop idle, MemTotal − MemAvailable (QEMU TCG) | _from CI (test_m3 report.json)_ | |
| Desktop idle, RetroOS userspace (Pi 5) | **79.4 MB** (AnonPages 63.8 + Shmem 15.6) | HW_CHECKLIST 3.12, CI run 30; target < 160 MB (D-028) ✅ |
| Firefox windowed, GPU (Pi 5): Enter → window | **~7 s first start, 2.4 s second** | HW_CHECKLIST 3.9, CI run 30; target ~5 s. The second start (files cached) shows the M2 start-up fix works; the first is mostly reading Firefox from the SD card |
| Power-off → `C:\>` (Pi 5, cold) | **17.7 s** | HW_CHECKLIST 3.15 / 1.8, CI run 30; target ≤ 10 s (aim 7) ❌ — breakdown requested |

### Raspberry Pi 5 (8 GB), Desktop idle (2026-11-04, CI run 30)

Desktop with the System Bar and the DOS Prompt window (maximised), Firefox
closed, from `/proc/meminfo` on the Pi:

| Metric | Desktop idle | Text mode (run 24) | Target |
|---|---:|---:|---:|
| RetroOS userspace: AnonPages + Shmem | **79.4 MB** (63.8 + 15.6) | 44.4 MB | GUI < 160 MB ✅ |
| MemTotal − MemAvailable ("used") | 272.5 MB | 224.4 MB | — |
| Slab (unreclaimable) | 64.0 (39.7) MB | — (31.0) | |
| VmallocUsed | 20.0 MB | 20.1 MB | |
| CMA in use | 10.0 MB | — | |
| station RSS | 112.2 MB | 31.8 MB | |
| retro-console RSS | 13.4 MB | 9.9 MB | |

station's RSS includes the Mesa/V3D libraries it mapped while Firefox ran
on the GPU; those are file pages (counted as available, not in "used").
Whether Station had already dropped back to the CPU renderer when this was
taken (60 s after the last graphical program) is to be confirmed.

### Raspberry Pi 5 boot breakdown (2026-11-04, CI run 30, cold power-on, text mode)

The stopwatch says power-off → `C:\>` **17.7 s**. From the Pi's own logs:

| Stage | Time | From |
|---|---:|---|
| Kernel start → userspace | 1.23 s | `systemd-analyze` |
| Userspace → graphical.target (greetd starts) | 2.91 s | `systemd-analyze`, critical chain: zram swap → local-fs → sysinit → dbus → iwd → network.target → greetd |
| POST screen on display | 2.85 s → 3.88 s after kernel start (1.03 s shown) | `RETRO-POST` markers |
| retro-post.service | 2.14 s, of which ~1.1 s waiting for the display | `systemd-analyze blame` |
| User manager ready (autologin) | 4.42 s after kernel start | journal |
| **Linux total, kernel start → `C:\>`** | **≈ 4.5–5 s** | (prompt shortly after the user manager) |
| **Before the kernel: firmware/bootloader (and the monitor waking up)** | **≈ 12–13 s** (by subtraction) | to be measured: `vclog -m` (raspi-utils-core, now in the image) and the power-on → POST stopwatch |

So the Linux side is already inside the 7 s aim; the gap to the 10 s target
is all before the kernel starts. Candidates, to check against the firmware
log: the monitor being powered by (and starting with) the Pi, HDMI/EDID
waits in the bootloader, the bootloader's boot-order and power-supply
checks. None of these can be fixed in config.txt alone; EEPROM settings
(`BOOT_ORDER`, `DISABLE_HDMI`…) are the likely lever.

### Raspberry Pi 5 firmware log (2026-11-05, CI run 39, cold power-on)

`vclog -m` timestamps (ms since power-on) split the 17.5 s stopwatch time:

| Stage | Time | Note |
|---|---:|---|
| EEPROM bootloader | 0 → 7.25 s | before the firmware's first message ("Initial voltage"). The tester's board has bootloader 2024-06-05 with `BOOT_ORDER=0xf416` (NVMe **first**, then SD, USB) and `PCIE_PROBE=1`: it waits for an NVMe drive that is not there before trying the card. SD first (`0xf461` or Raspberry Pi Imager's "SD Card Boot" bootloader) should take this to ~1–2 s; a board setting, not part of the image |
| Firmware: voltages, HDMI EDID, device tree + overlay | 7.25 → 7.65 s | |
| Firmware: loading and unpacking the kernel | 7.65 → 10.90 s ("Starting OS 10902 ms") | the kernel was gzip-compressed (10.3 MB, ~3× unpacked) and the firmware unpacked it: the image now ships it unpacked; expected saving ~2–2.5 s, to be measured |
| Linux and RetroOS to `C:\>` | 10.9 → ≈ 16 s | ≈ 5 s, as measured above |
| Monitor locks on to the signal | ≈ 1.5 s | the tester's monitor, separately powered |
