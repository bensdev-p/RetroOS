#!/usr/bin/env python3
"""Milestone 3 end-to-end test in QEMU: the RetroOS Desktop.

    python3 tests/qemu/test_m3.py [OUT_DIR]        (default: out)

Runs after test_m1.py, on the disk it leaves behind (Setup already done,
automatic login). Switches to the Desktop with DESKTOP, checks the frames
and the dithered desktop on screen, opens a second window, minimises it,
switches with Alt+Tab, leaves with Exit to DOS (Ctrl+Alt+F1), and starts
Firefox in a window. Results: OUT/test-m3/{report.json,*.png,serial*.log}.

At 1280x800 Station's art scale is 1: frames are 4 px, title bars 18 px,
the System Bar is 20 px, and the first window's client area starts at (4, 43).
"""
import json
import os
import re
import shutil
import sys
import time

from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from retroqemu import Qemu, font_json, ROOT  # noqa: E402

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "out"))
WORK = os.path.join(OUT, "test-m3")
M1 = os.path.join(OUT, "test-m1")

COBALT, BLACK, SILVER = (0x1F, 0x3B, 0xB3), (0x0A, 0x0A, 0x0F), (0xBD, 0xBD, 0xB8)

results = {"checks": [], "metrics": {}}


def check(name, cond, detail=""):
    results["checks"].append({"name": name, "ok": bool(cond), "detail": detail})
    print(("PASS " if cond else "FAIL ") + name + (f" — {detail}" if detail and not cond else ""))
    return cond


def main():
    os.makedirs(WORK, exist_ok=True)
    disk = os.path.join(M1, "root.qcow2")
    if not os.path.exists(disk):
        print("test_m3 needs the disk from test_m1.py (run it first)")
        sys.exit(2)
    shutil.copy(disk, os.path.join(WORK, "root.qcow2"))
    q = Qemu(os.path.join(OUT, "qemu"), WORK)
    q.font = font_json(os.path.join(WORK, "font.json"))
    q.start(q.font, keep_disk=True)
    try:
        run(q)
    finally:
        q.stop()
        with open(os.path.join(WORK, "report.json"), "w") as f:
            json.dump(results, f, indent=2)
    ok = all(c["ok"] for c in results["checks"])
    print("\nM3 QEMU test:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


def window_text(q, name, box):
    """Text inside a window's client area (x, y, w, h) at font scale 1."""
    p = q.screendump(name)
    img = Image.open(p).convert("RGB")
    x, y, w, h = box
    crop = os.path.join(WORK, name + "-client.png")
    img.crop((x, y, x + w, y + h)).save(crop)
    return q.reader.read(crop, scale=1), img


def wait_window_text(q, needle, timeout, name, box):
    end = time.time() + timeout
    last = ""
    while time.time() < end:
        last, img = window_text(q, name, box)
        if needle in last:
            return last, img
        time.sleep(1.0)
    raise TimeoutError(f"window: {needle!r} not seen in {timeout}s; last:\n{last}")


def serial_after(q, marker, shell, timeout=180):
    """Run a shell command in the background, its output to the serial port."""
    q.type(f'UNIX "({shell}; echo {marker}) > /dev/ttyAMA0 2>&1 &"\n')
    s = q.wait_serial(marker, timeout)
    return s[: s.rfind(marker)]


DOS = (4, 43, 640, 400)


def run(q):
    t = q.wait_text("C:\\>", 600, "m3-1-prompt")
    check("Boots to RetroDOS", "RetroDOS Version 1.0" in t)

    # DESKTOP: the console becomes a framed DOS Prompt window on the desktop.
    q.type("CLS\nDESKTOP\n")
    time.sleep(5)
    t, img = wait_window_text(q, "C:\\>", 60, "m3-2-desktop", DOS)
    px = img.load()
    w, h = img.size
    corner = {px[w - 1, h - 1], px[w - 2, h - 1]}
    check("The desktop is a Cobalt/Black dither", corner == {COBALT, BLACK}, str(corner))
    check("The DOS Prompt has an active (Cobalt) title bar", px[100, 30] == COBALT, str(px[100, 30]))
    check("The window has a Silver frame", px[2, 200] == SILVER and px[0, 200] == BLACK,
          f"{px[0, 200]} {px[2, 200]}")
    check("RetroDOS runs in the DOS Prompt window", "C:\\>" in t, t[-300:])
    check("The System Bar runs along the top (White, Black rule)",
          px[600, 5] == (0xF5, 0xF5, 0xF0) and px[600, 19] == BLACK, f"{px[600, 5]} {px[600, 19]}")

    q.type("ECHO typed in a window\n")
    t, _ = wait_window_text(q, "typed in a window", 60, "m3-3-typing", DOS)
    check("Typing reaches the DOS Prompt window", t.count("typed in a window") >= 2, t[-300:])

    # A second window (another console) opens cascaded and gets the focus.
    q.type("START retro-console\n")
    second = (26, 65, 640, 400)
    t, _ = wait_window_text(q, "C:\\>", 120, "m3-4-second", second)
    q.type("ECHO second window\n")
    t, img = wait_window_text(q, "second window", 60, "m3-5-second-typing", second)
    check("A second program opens in its own window", "second window" in t, t[-300:])
    px = img.load()
    check("The new window is active, the DOS Prompt inactive",
          px[300, 52] == COBALT and px[200, 30] == SILVER, f"{px[300, 52]} {px[200, 30]}")

    # Minimise it from the window menu; its icon appears on the desktop.
    q.key("alt", "spc")
    time.sleep(1)
    q.screendump("m3-6-window-menu")
    q.key("n")
    time.sleep(2)
    p = q.screendump("m3-7-minimised")
    px = Image.open(p).convert("RGB").load()
    # Icon slot 0: (4, 744); its picture's Cobalt title bar at (28+5, 744+5).
    check("A minimised window becomes an icon", px[33, 749] == COBALT, str(px[33, 749]))
    s = serial_after(q, "RETRO-M3-WIN1", "retrodos /C TASKS /W")
    wins = s[s.rfind("Task  Position"):]
    check("TASKS /W lists it as minimised", "minimized" in wins, wins[-500:])

    # Alt+Tab brings it back.
    q.key("alt", "tab")
    time.sleep(2)
    q.screendump("m3-8-alt-tab")
    s = serial_after(q, "RETRO-M3-WIN2", "retrodos /C TASKS /W")
    wins = s[s.rfind("Task  Position"):]
    check("Alt+Tab restores the icon", "minimized" not in wins and wins.count("normal") >= 2, wins[-500:])

    # Exit to DOS: the DOS Prompt fills the screen; the other window keeps running.
    q.key("ctrl", "alt", "f1")
    time.sleep(3)
    q.type("CLS\nTASKS\n")
    t = q.wait_text("task(s)", 60, "m3-9-exit-to-dos")
    check("Exit to DOS (Ctrl+Alt+F1) returns to full-screen RetroDOS", "2 task(s)" in t, t[-400:])

    # Idle memory in the Desktop, without and with Firefox.
    q.type("CLS\nDESKTOP\n")
    time.sleep(20)
    s = serial_after(q, "RETRO-M3-MEM", "free -k; ps -eo rss,comm --sort=-rss | head -8")
    mm = re.findall(r"Mem:\s+(\d+)\s+\d+\s+\d+\s+\d+\s+\d+\s+(\d+)", s)
    if mm:
        total, avail = map(int, mm[-1])
        results["metrics"]["desktop_used_mb_tcg"] = round((total - avail) / 1024, 1)
    m = re.search(r"^\s*(\d+)\s+station\s*$", s, re.M)
    if m:
        results["metrics"]["station_rss_mb_desktop"] = round(int(m.group(1)) / 1024, 1)

    # Firefox in a window (not full screen).
    q.type('UNIX "(sleep 240; retrodos /C TASKS /W; echo RETRO-M3-FIREFOX) > /dev/ttyAMA0 2>&1 &"\n')
    time.sleep(2)
    q.type("START FIREFOX\n")
    s = q.wait_serial("RETRO-M3-FIREFOX", 600)
    wins = s[s.rfind("Task  Position"):s.rfind("RETRO-M3-FIREFOX")]
    q.screendump("m3-10-firefox")
    rows = [r for r in wins.splitlines() if re.match(r"\s+\d+\s+-?\d+,-?\d+", r)]
    check("Firefox opens as a third window", len(rows) >= 3, wins[-600:])
    full = [r for r in rows if "1280x800" in r or "fullscreen" in r]
    check("Firefox is windowed, not full screen", len(rows) >= 3 and not full, wins[-600:])


if __name__ == "__main__":
    main()
