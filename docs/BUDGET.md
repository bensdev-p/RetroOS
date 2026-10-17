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

The text-mode figure is 2.8× the target and ~95 MB above QEMU's 181 MB, so most
of the gap is Pi-specific (GLES context and GPU buffers in Station, the RPi
kernel's own allocations on an 8 GB board). The next run collects
`/proc/meminfo` and per-process RSS so the cuts are chosen by data:
GLES-only-while-needed (D-023's fallback plan), fewer/smaller swapchain
buffers, kernel trimming.

### Sizes

| Item | Size |
|---|---|
| retroos.deb (all RetroOS binaries + assets) | 2.0 MB |
| station / retro-console / retrodos binaries | 2.9 MB / 0.8 MB / 0.7 MB |
| Image (uncompressed / xz) | _pending_ |
