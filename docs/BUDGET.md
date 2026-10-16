# Resource budget — measured

Definitions (DESIGN.md §16):
- **Idle RAM** = MemTotal − MemAvailable, 60 s after the prompt appears, no user programs open.
- **Boot time** = power-on → usable `C:\>` (stopwatch on hardware); in QEMU we report
  `systemd-analyze` (kernel + userspace) because TCG emulation is 10–30× slower than a Pi 5
  and its wall-clock time says nothing about real hardware.

Targets: boot to prompt < 10 s on Pi 5 (aim 7 s) · idle RAM < 100 MB text mode · < 200 MB GUI mode.

## Milestone 1

### QEMU (`make test-qemu`, TCG on a 4-core x86_64 host, generic arm64 kernel, virtio-gpu 1280×800, 2 GB)

| Metric | Value | Notes |
|---|---|---|
| Idle RAM, text mode | _pending_ | |
| systemd-analyze (kernel + userspace), second boot | _pending_ | emulated; not comparable to hardware |
| Failed units | _pending_ | |

### Raspberry Pi 5 (8 GB)

| Metric | Value | Notes |
|---|---|---|
| Power-on → POST screen | _to measure_ | HW_CHECKLIST 1.1 |
| Power-on → `C:\>` | _to measure_ | HW_CHECKLIST 1.8 |
| Idle RAM, text mode | _to measure_ | HW_CHECKLIST 1.18 |

### Sizes

| Item | Size |
|---|---|
| retroos.deb (all RetroOS binaries + assets) | 2.0 MB |
| station / retro-console / retrodos binaries | 2.9 MB / 0.8 MB / 0.7 MB |
| Image (uncompressed / xz) | _pending_ |
