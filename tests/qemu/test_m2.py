#!/usr/bin/env python3
"""Milestone 2 end-to-end test in QEMU.

    python3 tests/qemu/test_m2.py [OUT_DIR]        (default: out)

Runs after test_m1.py, on the disk it leaves behind (Setup already done,
automatic login). Checks batch files, NET /S, a USB stick as D: and EJECT, an EDIT round trip, which
renderer Station picked, and launching Firefox from the prompt with the
task swapper. Results: OUT/test-m2/{report.json,*.png,serial*.log}.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from retroqemu import Qemu, font_json, ROOT  # noqa: E402

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "out"))
WORK = os.path.join(OUT, "test-m2")
M1 = os.path.join(OUT, "test-m1")

results = {"checks": [], "metrics": {}}


def check(name, cond, detail=""):
    results["checks"].append({"name": name, "ok": bool(cond), "detail": detail})
    print(("PASS " if cond else "FAIL ") + name + (f" — {detail}" if detail and not cond else ""))
    return cond


def main():
    os.makedirs(WORK, exist_ok=True)
    disk = os.path.join(M1, "root.qcow2")
    if not os.path.exists(disk):
        print("test_m2 needs the disk from test_m1.py (run it first)")
        sys.exit(2)
    shutil.copy(disk, os.path.join(WORK, "root.qcow2"))
    q = Qemu(os.path.join(OUT, "qemu"), WORK)
    # A USB stick with a FAT file system and one file on it.
    stick = os.path.join(WORK, "stick.img")
    if os.path.exists(stick):
        os.remove(stick)
    subprocess.run(["mkfs.vfat", "-C", "-n", "RETROSTICK", stick, "32768"], check=True,
                   stdout=subprocess.DEVNULL)
    note = os.path.join(WORK, "STICK.TXT")
    with open(note, "w") as f:
        f.write("Hello from the USB stick\n")
    subprocess.run(["mcopy", "-i", stick, note, "::STICK.TXT"], check=True)
    q.extra_args = ["-device", "qemu-xhci,id=xhci",
                    "-drive", f"if=none,id=stick,format=raw,file={stick}",
                    "-device", "usb-storage,bus=xhci.0,drive=stick"]
    q.font = font_json(os.path.join(WORK, "font.json"))
    q.start(q.font, keep_disk=True)
    try:
        run(q)
    finally:
        q.stop()
        with open(os.path.join(WORK, "report.json"), "w") as f:
            json.dump(results, f, indent=2)
    ok = all(c["ok"] for c in results["checks"])
    print("\nM2 QEMU test:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


def run(q):
    t = q.wait_text("C:\\>", 600, "m2-1-prompt")
    check("Boots to RetroDOS", "RetroDOS Version 1.0" in t)
    s = q.serial()
    m = re.search(r"(GPU rendering|CPU rendering)[^\n]*", s)
    results["metrics"]["station_renderer"] = m.group(0).strip() if m else "unknown"

    # Batch files: parameters, labels, FOR.
    q.type("CLS\nECHO @ECHO OFF > T.BAT\nECHO ECHO batch-ok %1 >> T.BAT\n"
           "ECHO FOR %%F IN (*.BAT) DO ECHO found %%F >> T.BAT\nT hello\n")
    t = q.wait_text("found T.BAT", 60, "m2-2-batch")
    check("Batch file with %1 and FOR", "batch-ok hello" in t and "found T.BAT" in t, t[-300:])

    # NET /S: the wired link (QEMU user networking hands out 10.0.2.15).
    q.type("CLS\nNET /S\n")
    t = q.wait_text("Wireless", 60, "m2-3-net")
    check("NET /S shows the wired link", "Connected" in t and "10.0.2.15" in t, t[-400:])

    # USB stick: mounted for the user as D:, then ejected.
    q.type("CLS\nTYPE D:\\STICK.TXT\n")
    t = q.wait_text("from the USB stick", 60, "m2-3b-usb")
    check("USB stick appears as D:", "Hello from the USB stick" in t, t[-300:])
    q.type("EJECT D:\n")
    t = q.wait_text("removed safely", 60, "m2-3c-eject")
    check("EJECT D: unmounts the stick", "can now be removed safely" in t, t[-300:])

    # EDIT round trip.
    q.type("CLS\nEDIT NOTE.TXT\n")
    q.wait_text("F10=Menu", 60, "m2-4-edit")
    q.type("Written in EDIT")
    q.key("ctrl", "s")
    time.sleep(1)
    q.key("ctrl", "q")
    q.type("CLS\nTYPE NOTE.TXT\n")
    t = q.wait_text("Written in EDIT", 60, "m2-5-type")
    check("EDIT saves a file", "Written in EDIT" in t)

    # Firefox from the prompt. Ask for the task list on the serial port a
    # while after starting it (keyboard focus moves to Firefox).
    q.type('CLS\nUNIX "(sleep 240; retrodos /C TASKS; free -k; echo RETRO-M2-TASKS) > /dev/ttyAMA0 2>&1 &"\n')
    time.sleep(2)
    t0 = q.elapsed()
    q.type("START FIREFOX\n")
    s = q.wait_serial("RETRO-M2-TASKS", 600)
    tasks = s[s.rfind("Task  Program"):s.rfind("RETRO-M2-TASKS")]
    check("FIREFOX starts as a task", "FIREFOX" in tasks, tasks[-600:])
    mm = re.findall(r"Mem:\s+(\d+)\s+\d+\s+\d+\s+\d+\s+\d+\s+(\d+)", tasks)
    if mm:
        total, avail = map(int, mm[-1])
        results["metrics"]["used_with_firefox_mb_tcg"] = round((total - avail) / 1024, 1)
    q.screendump("m2-6-firefox")
    results["metrics"]["firefox_task_seen_after_s_tcg"] = round(q.elapsed() - t0, 1)

    # Back to the prompt with the task swapper, then end Firefox.
    q.key("ctrl", "alt", "tab")
    t = q.wait_text("C:\\>", 60, "m2-7-swapped-back")
    check("Ctrl+Alt+Tab returns to RetroDOS", "C:\\>" in t)
    q.type("KILL FIREFOX\n")
    time.sleep(15)
    q.type('UNIX "(retrodos /C TASKS; echo RETRO-M2-AFTER) > /dev/ttyAMA0 2>&1"\n')
    s = q.wait_serial("RETRO-M2-AFTER", 120)
    after = s[s.rfind("RETRO-M2-TASKS"):]
    after = after[after.rfind("Task  Program"):]
    check("KILL FIREFOX ends it", "FIREFOX" not in after, after[-400:])


if __name__ == "__main__":
    main()
