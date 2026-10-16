# Real-hardware checklist

Run on a **Raspberry Pi 5** (primary) and, when available, a Pi 4, for every
milestone image. Record results in `docs/BUDGET.md` (numbers) and as a dated
row in the results table at the end (pass/fail).

Flash: Raspberry Pi Imager → *Use custom* → `retroos-<ver>-rpi-arm64.img(.xz)`,
or `xz -dc retroos-*.img.xz | sudo dd of=/dev/sdX bs=4M conv=fsync status=progress`.

Optional but very useful: a USB-serial adapter on the Pi 5 debug UART
(3-pin JST next to the HDMI ports, 115200 8N1). RetroOS logs boot markers there.

## M1 — boot to RetroDOS

| # | Check | How | Expect |
|---|---|---|---|
| 1.1 | Power-on to POST screen | stopwatch from plugging in power | Phosphor BIOS screen, no rainbow splash, no kernel text |
| 1.2 | Startup chime | HDMI monitor/TV with speakers, or a USB audio device | chime plays during POST (Pi 5 has no headphone jack) |
| 1.3 | POST data is real | read the screen | correct model, 4-core Cortex-A76 @ 2400 MHz, RAM ≈ 8,000,000K, HDMI mode, SD size, keyboard/mouse counts |
| 1.4 | F8 Startup Menu | hold/tap F8 during POST | menu appears; ↑↓/digits/Enter work; countdown stops on any key |
| 1.5 | DEL Setup | tap DEL during POST | Startup Options page; F10 saves (check `/etc/retroos/startup.conf` later) |
| 1.6 | First-boot Setup | follow the screens | account created, computer name, time zone, keyboard; partition grown (`DIR` free space ≈ card size) |
| 1.7 | Automatic login → `C:\>` | after Setup | RetroDOS banner and prompt, blinking underline cursor |
| 1.8 | Power-on to `C:\>` time (second boot) | stopwatch | **target ≤ 7 s** (record actual) |
| 1.9 | Display modes | 1080p, 1440p, 4K, 1024×768 monitors | 1080p → 120×33 text; 4K → still ≥ 80×25, crisp integer scaling, border visible on letterbox |
| 1.10 | Keyboard | type, arrows, F1/F3, Ins, Ctrl+C | DOSKEY editing works; Ins switches underline ↔ block |
| 1.11 | Mouse | move the mouse at the prompt | inverse block mouse cursor follows |
| 1.12 | Commands | `DIR`, `COPY`, `TYPE`, `MD`, `CD`, `MEM`, `VER /R`, `TREE` | DOS output; `VER /R` shows "Raspberry Pi 5" |
| 1.13 | USB stick | insert a FAT/exFAT stick, `DIR D:` | (M2: auto-mount; M1: not yet — record) |
| 1.14 | POSIX shell | `UNIX`, then `exit` | bash prompt, returns to RetroDOS |
| 1.15 | Task swapper | `UNIX` is not a task; skip in M1 (M2 adds RUN) | — |
| 1.16 | Colour schemes | `COLOR AMBER`, `COLOR GREEN`, `COLOR PAPER`, `COLOR` | whole screen recolours |
| 1.17 | Screenshot | Print Screen | `DIR C:\SCREENS` shows SCREEN01.PNG |
| 1.18 | Idle RAM | `UNIX free -m` 60 s after the prompt | **target < 100 MB used** (MemTotal − MemAvailable) |
| 1.19 | Safe mode | F8 → 3 | plain login prompt on the screen; log in as the Setup user |
| 1.20 | Shut down / restart | `SHUTDOWN`, `RESTART` | clean power-off (green LED off) / reboot |
| 1.21 | Login screen | in Setup choose automatic login: No | Phosphor login dialog; wrong password message; F2 mode toggle |
| 1.22 | Thermal/idle CPU | `UNIX top` for 30 s at idle | no process using > 1% CPU at the prompt |
| 1.23 | Pi 4 | repeat 1.1–1.8, 1.18 | boots `kernel8.img`; ≤ 10 s to prompt |

## Results

| Date | Image | Board | Pass | Notes |
|---|---|---|---|---|
| | | | | |
