# Real-hardware checklist

Run on a **Raspberry Pi 5** (primary) and, when available, a Pi 4, for every
milestone image. Record results in `docs/BUDGET.md` (numbers) and as a dated
row in the results table at the end (pass/fail).

Bootloader (once per board): RetroOS boots from the SD card, so the Pi 5's
EEPROM should try the card first. `ADMIN UNIX "vcgencmd bootloader_config"`
should show a `BOOT_ORDER` ending in 1 (e.g. `0xf461`); an order that tries
NVMe first (`0xf416`) adds several seconds to every boot, and Raspberry Pi's
default settings show a network-install screen after every cold boot
(~10 s in all, BUDGET.md). Fix: the **RetroOS bootloader settings card**
(D-034): download the `retroos-bootloader-rpi5` artifact of a CI run, flash
`retroos-bootloader-rpi5.img` to a spare card (Imager → *Use custom*), power
on with it, wait until the screen turns green (≥ 10 s), power off, put the
RetroOS card back. `vcgencmd bootloader_config` then shows `BOOT_ORDER=0xf41`,
`BOOT_UART=0`, `NET_INSTALL_AT_POWER_ON=0`, `NET_INSTALL_ENABLED=0`.
Raspberry Pi's defaults come back with Imager → *Misc utility images* →
*Bootloader (Pi 5 family)* → *SD Card Boot*.

Power: use the official 27 W USB-C supply (5.1 V / 5 A). A supply that
negotiates only 3 A shows "Selecting USB low current limit" / `max-current
3000` on the bootloader screen and can cut out under load (both LEDs off).
After an unexpected power-off: `ADMIN UNIX "vcgencmd get_throttled"` (bit 0
or 16 set = under-voltage).

Time boots on the **second** boot after flashing: the first one runs Setup.

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

## M2 — shell complete, networking, browser from the prompt

| # | Check | How | Expect |
|---|---|---|---|
| 2.1 | GPU rendering | `ADMIN UNIX journalctl -b -g rendering` | `GPU rendering renderer=V3D 7.1` (not "CPU rendering") |
| 2.2 | Wi-Fi join | `WIFI`, pick your network, type the passphrase | "Connected"; `NET /S` shows the SSID; `UNIX ping -c3 deb.debian.org` works |
| 2.3 | Wi-Fi remembered | `RESTART /Y`, then `NET /S` | reconnects by itself |
| 2.4 | Wired | plug in Ethernet, `NET /S` | "Connected" with an address |
| 2.5 | Firefox | `FIREFOX` | full-screen within ~5 s; RetroDOS shows "Running FIREFOX..." |
| 2.6 | WebGL | in Firefox open `about:support` | "Compositing: WebRender" (hardware), WebGL 2 driver renderer V3D |
| 2.7 | YouTube 1080p | play a 1080p video for 2 minutes | smooth; note dropped frames (right-click → Stats for nerds) |
| 2.8 | Task swapper | Ctrl+Alt+Tab while Firefox runs, `TASKS`, Ctrl+Alt+Tab back | both tasks listed; switching is instant |
| 2.9 | End a program | `START FIREFOX`, then `KILL FIREFOX` | Firefox closes; prompt keeps working |
| 2.10 | Return on exit | close Firefox (Ctrl+Q) | back at `C:\>`, `ECHO %ERRORLEVEL%` prints 0 |
| 2.11 | EDIT | `EDIT TEST.TXT`, type, mouse-select, Alt+F → Save, Exit | file saved; mouse clicks place the cursor |
| 2.12 | FILER | `FILER`, copy a file with F5, F9 to switch drives | copies; drive list shows C: and Z: (and D: with a stick) |
| 2.13 | Batch | edit `AUTOEXEC.BAT` to `ECHO Hello`, `RESTART /Y` | "Hello" before the prompt |
| 2.14 | MODE | `MODE CON COLS=80 LINES=25`, then `MODE CON RESET` | 80×25 letterboxed with a border; back to full screen |
| 2.15 | KEYB | `KEYB UK`, log off and on, type `#` and `£` | UK layout; `KEYB US` to undo |
| 2.16 | Idle RAM, text mode | `UNIX free -m` 60 s after the prompt | record (GLES now costs memory; see BUDGET.md) |
| 2.17 | RAM with Firefox | one tab of a news site, `UNIX free -m` | record |

## M3 — the RetroOS Desktop

| # | Check | How | Expect |
|---|---|---|---|
| 3.1 | Switch to the Desktop | at `C:\>` type `DESKTOP` | dithered blue desktop; RetroDOS in a "DOS Prompt" window with a Silver frame and a Cobalt title bar, in one frame (no flicker, no logout) |
| 3.2 | Frames look right | photo of the top-left corner of the window | 4-px frame (8 screen pixels at 1080p) with notches, control-menu box at left, ▼ ▲ at right, crisp (no blur) |
| 3.3 | Mouse pointer | move over the frame edges and corners | ↔ ↕ and diagonal sizing arrows over the border; normal arrow over the title bar and desktop |
| 3.4 | Move | drag the title bar | a black-and-white outline follows; the window jumps there on release |
| 3.5 | Resize | drag the bottom-right corner | outline; DOS Prompt gets more/fewer columns and rows, same text size |
| 3.6 | Minimise / restore | click ▼, then double-click the icon at the bottom left | icon with the window's name; double-click brings it back |
| 3.7 | Maximise | click ▲, then double-click the title bar | fills the screen (title bar at the top), then back |
| 3.8 | Window menu, keys | Alt+Space, then Move, arrows, Enter; Alt+F4 on the DOS Prompt | menu under the control-menu box; Alt+F4 says it is the main session |
| 3.9 | Firefox windowed | `START FIREFOX` in the DOS Prompt; time from Enter to the window | a normal window (own title bar), opens within ~5 s **(confirms the M2 start-up delay fix)**; `ADMIN UNIX journalctl -b -g rendering` shows "GPU rendering" |
| 3.10 | Firefox windowed works | drag its tab bar, resize its edge, maximise/restore with its buttons, play a YouTube video, then full screen (F) and Esc | moves/resizes with the outline; video smooth; full screen covers everything and comes back |
| 3.11 | Alt+Tab | hold Alt, tap Tab twice, let go | a box with the next window's name; releasing Alt switches (an icon is restored) |
| 3.12 | Desktop idle RAM | close Firefox, wait 60 s, then in the DOS Prompt run the M2 memory command (below) | record; target RetroOS userspace (AnonPages + Shmem) < 160 MB |
| 3.13 | Exit to DOS | Ctrl+Alt+F1 | full-screen RetroDOS at once; `TASKS` still lists the other programs; `DESKTOP` puts the windows back where they were |
| 3.14 | Start in the Desktop | `RESTART`, tap F8, choose 2 (RetroOS Desktop) | boots straight into the Desktop with the DOS Prompt window |
| 3.16 | System Bar | look at the top of the screen in the Desktop | White bar, ◆ Programs Window at left, the right time at right; windows start below it |
| 3.17 | Bar menus | click ◆, then DOS Prompt; click Programs → a group → a program; click Window → another window | menus drop down under the title; a second DOS Prompt window opens; the program starts; the window comes to the front |
| 3.18 | Exit to DOS from the bar | ◆ → Exit to DOS…, tick "Close all programs first", OK | dialog in the middle of the screen; programs close; full-screen RetroDOS |
| 3.20 | Firmware boot log | `ADMIN UNIX vclog -m` right after a cold boot (photo of all of it) | the bootloader's timestamped messages: how long the 12 s before the kernel take |
| 3.21 | One colour scheme everywhere | in the Desktop, `COLOR AMBER` in a DOS Prompt with a second DOS Prompt open (◆ → DOS Prompt); then `RESTART /Y`; then `COLOR CLASSIC` | at once: both consoles, window frames, menus, the desktop pattern and the System Bar turn amber; after the restart the POST screen, the Startup Menu and RetroDOS are amber too; CLASSIC brings everything back |
| 3.22 | Boot times in one screen | cold boot (not the first after flashing), then `ADMIN UNIX /usr/libexec/retroos/boot-times` (one photo) | firmware and Linux times, the total from power-on to Station, the supply limit (5000 mA) and `throttled=0x0`; compare the total with the stopwatch: the difference is the monitor |
| 3.24 | Bootloader settings card | flash the card as above, boot it once, then RetroOS; `ADMIN UNIX "vcgencmd bootloader_config"`; cold boot, `ADMIN UNIX /usr/libexec/retroos/boot-times` and stopwatch | green screen on the settings card; the four settings shown; no network-install screen; `Starting OS` well under 20 s (was 20.6 s) |
| 3.25 | Fast POST visible | cold boot with `fast_post=yes` (the default) | the POST screen stays ≈ 2.5 s: the end of the memory count, the device lines and a short hold are visible |
| 3.26 | Program Manager | `DESKTOP` | Program Manager opens behind the DOS Prompt, left of the drive icons; typing still goes to the DOS Prompt. Click it: Main (File Cabinet, DOS Prompt), Accessories (Jot), Internet (Firefox) and the other groups; double-click DOS Prompt opens one; Alt+F opens File; Options → Minimize on Use is kept after a restart |
| 3.27 | About This Computer | ◆ → About This Computer… | version, total and unused memory, a bar per program (RetroOS first); start Firefox and watch its bar grow within a few seconds; photo |
| 3.28 | Jot | Program Manager → Accessories → Jot | type two lines, hold Backspace (repeats), Ctrl+A, Ctrl+C, click into a Firefox address bar, Ctrl+V (the text arrives); Ctrl+S → Save As `test.txt`; File → Open shows it; close with a change → asks to save |
| 3.29 | File Cabinet | Main → File Cabinet | C: lists the home folder; New Folder; copy `test.txt` into it (Ctrl+C, open the folder, Ctrl+V); Del → Wastebasket (the desktop bin shows paper within 3 s); View → Icons; Get Info; double-click `test.txt` opens Jot |
| 3.30 | Desktop drive icons | plug in a USB stick | a D: icon appears on the right edge within 3 s; double-click opens File Cabinet on it; Delete there asks to delete for good; EJECT D: in a DOS Prompt removes the icon |
| 3.31 | Pointers everywhere | in the Desktop, start Firefox; point at the page, a text box, a link, a window edge; START xeyes (X11) | the RetroOS arrow (Black outline, White fill) over Firefox's page as over the desktop, the same size; an I-beam over text boxes; a pointing hand over links; the sizing arrows at frame edges; xeyes's window framed like the others |
| 3.23 | POST visible | `ADMIN UNIX "sed -i s/fast_post=yes/fast_post=no/ /etc/retroos/startup.conf"`, cold boot; afterwards set it back to `yes` | the POST screen (memory count, devices) stays ≈ 4 s instead of ≈ 1 s; if it shows now, the fast one is hidden while the monitor re-syncs to the display driver's mode |
| 3.19 | Shut down from the bar | ◆ → Shut Down…, OK | the Pi powers off (green LED off) |
| 3.15 | Boot times (carried over) | stopwatch, second boot, text mode | record power-on → POST screen (1.1) and power-on → `C:\>` (1.8) |

Memory command (3.12): `UNIX "grep -E 'MemTotal|MemFree|MemAvail|Cached|Shmem:|Slab|SUnrecl|KernelSt|PageTab|Vmalloc|Percpu|Cma|AnonPages' /proc/meminfo; ps -eo rss,comm --sort=-rss | head -12"`

## Results

| Date | Image | Board | Pass | Notes |
|---|---|---|---|---|
| 2026-10-17 | CI run 15 (d22b35a) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 2.1, 2.3, 2.8–2.14 | 2.2 Wi-Fi joins but `ping` lacks permission (fixed: ping_group_range); 2.4 not tested (no cable); 2.5 Firefox "huge, unusable" (fixed: D-027) so 2.6/2.7 untested; 2.15 KEYB "Access denied" from localectl (fixed: own helper, applies at once); 2.16 idle 276 MB (target 100); 2.17 1,247 MB with YouTube |
| 2026-10-17 | CI run 24 (ade8015) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 2.2, 2.5–2.7, 2.15, 2.16; boot times within target | idle userspace 44 MB (target 60), "used" 224 MB; Firefox normal size with GPU, but it opened a little late (fixed after: the GPU switch now overlaps Firefox's start-up, and the GPU stays up 60 s after the last program) |
| 2026-11-04 | CI run 30 (711b9eb) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 3.1–3.14, 3.16–3.19 (all M3 checks); 3.15 measured | sheet: `docs/hw/2026-11-04-m3-pi5.csv`. 3.9 Firefox window after ~7 s the first time, 2.4 s the second (start-up fix confirmed; the first start reads Firefox from the card); 3.12 idle userspace 79.4 MB (target 160), "used" 272.5 MB; 3.16 clock 6 h off — the time zone is still the image default (UTC, Setup was left at it); 3.15 power-off → `C:\>` **17.7 s** (target ≤ 10 s; POST-screen time not taken) — breakdown requested |
| 2026-11-05 | CI run 39 (8e9f458) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 3.20, 3.21; time zone set | 3.21 one colour scheme everywhere: good. 3.20 firmware log: bootloader until 7.25 s, kernel load/unpack 7.65–10.90 s (unpacked kernel in the next image), Linux ≈ 5 s; stopwatch 17.5 s (monitor separately powered, sometimes slow to show a picture) |
| 2026-11-05 | CI run 39 (8e9f458) | Pi 5 8 GB | — | bootloader 2024-06-05, BOOT_ORDER=0xf416 (NVMe first), PCIE_PROBE=1, BOOT_UART=1: explains most of the 7.25 s before the firmware; SD-first bootloader suggested |
| 2026-11-05 | CI run 40 (8796fe1) | Pi 5 8 GB, 5 A supply | supply, 3.20, 3.23 | 5000 mA confirmed. Stopwatch ≈ 20 s. vclog: Starting OS 20.57 s (was 10.90 s): the 2026-10-30 bootloader shows its network-install screen after every cold boot (`NET_INSTALL_AT_POWER_ON=1`), `BOOT_UART=1` still set. 3.23: the slow POST is visible, the fast one is hidden by the monitor re-syncing |
| 2026-11-05 | CI run 49 (5690705) | Pi 5 8 GB, 5 A supply | 3.22, 3.24, 3.25 | 3.24 settings card: the four settings read back. 3.25 fast POST visible ("maybe a hair longer, but good"). 3.22 (first boot, Setup ran): bootloader 14.48 s, Starting OS 16.47 s (was 20.57 s), POST 2.25–4.78 s after the kernel; 5.18 V, throttled 0x0. Power loss earlier the same day traced to the outlet/plug, not the Pi |
| 2026-11-06 | CI run 61 (092e9c3) | Pi 5 8 GB, 5 A supply, bootloader settings card | 3.26–3.30, boot stopwatch | 3.27–3.30 good. 3.26 good, but a DOS Prompt opened from Program Manager had smaller text that did not grow with the window (fixed: every DOS Prompt opens at 80×25 at 2×). Stopwatch: power-on → POST 6 s, → `C:\>` 9 s (was ≈ 20 s) |
| 2026-11-05 | CI run 40 (8796fe1) | Pi 5 8 GB, wall plug + USB-C cable | bootloader screen, journal, throttling | SD Card Boot bootloader installed (order f4, SD first). Pi screen at 7–8 s, RetroOS at ≈ 22 s, but that boot ran first-boot Setup (journal); second boot to be timed. throttled 0x0, EXT5V 4.98 V, but the supply negotiated only 3 A: occasional power-offs with no LEDs, official 27 W supply recommended |
