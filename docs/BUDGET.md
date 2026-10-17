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
| Power-on → `C:\>` | _to measure_ | HW_CHECKLIST 1.8 |
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
