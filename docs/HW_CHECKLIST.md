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
| 3.19 | Shut down from the bar | ◆ → Shut Down…, OK | the Pi powers off (green LED off) |
| 3.15 | Boot times (carried over) | stopwatch, second boot, text mode | record power-on → POST screen (1.1) and power-on → `C:\>` (1.8) |

Memory command (3.12): `UNIX "grep -E 'MemTotal|MemFree|MemAvail|Cached|Shmem:|Slab|SUnrecl|KernelSt|PageTab|Vmalloc|Percpu|Cma|AnonPages' /proc/meminfo; ps -eo rss,comm --sort=-rss | head -12"`

## Results

| Date | Image | Board | Pass | Notes |
|---|---|---|---|---|
| 2026-10-17 | CI run 15 (d22b35a) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 2.1, 2.3, 2.8–2.14 | 2.2 Wi-Fi joins but `ping` lacks permission (fixed: ping_group_range); 2.4 not tested (no cable); 2.5 Firefox "huge, unusable" (fixed: D-027) so 2.6/2.7 untested; 2.15 KEYB "Access denied" from localectl (fixed: own helper, applies at once); 2.16 idle 276 MB (target 100); 2.17 1,247 MB with YouTube |
| 2026-10-17 | CI run 24 (ade8015) | Pi 5 8 GB, Raspberry Pi monitor 1080p | 2.2, 2.5–2.7, 2.15, 2.16; boot times within target | idle userspace 44 MB (target 60), "used" 224 MB; Firefox normal size with GPU, but it opened a little late (fixed after: the GPU switch now overlaps Firefox's start-up, and the GPU stays up 60 s after the last program) |
