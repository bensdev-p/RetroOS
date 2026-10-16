#!/usr/bin/env python3
"""Milestone 1 end-to-end test in QEMU.

    python3 tests/qemu/test_m1.py [OUT_DIR]        (default: out)

Boots the image, walks through first-boot Setup with the keyboard, waits for
the RetroDOS prompt after automatic login, runs commands and checks the
screen, then measures boot time and idle memory from inside the guest.
Results: OUT/test-m1/{report.json,*.png,serial.log}.
"""
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from retroqemu import Qemu, font_json, ROOT  # noqa: E402

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "out"))
WORK = os.path.join(OUT, "test-m1")
TCG_SLOWDOWN = 1 if os.path.exists("/dev/kvm") else 1  # timeouts below already assume TCG

results = {"checks": [], "metrics": {}}


def check(name, cond, detail=""):
    results["checks"].append({"name": name, "ok": bool(cond), "detail": detail})
    print(("PASS " if cond else "FAIL ") + name + (f" — {detail}" if detail and not cond else ""))
    return cond


def main():
    os.makedirs(WORK, exist_ok=True)
    q = Qemu(os.path.join(OUT, "qemu"), WORK)
    q.start(font_json(os.path.join(WORK, "font.json")))
    try:
        run(q)
    finally:
        q.stop()
        with open(os.path.join(WORK, "report.json"), "w") as f:
            json.dump(results, f, indent=2)
    ok = all(c["ok"] for c in results["checks"])
    print("\nM1 QEMU test:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


def run(q):
    # 1. Boot screen.
    s = q.wait_serial("RETRO-POST: done", 600)
    m = re.search(r"RETRO-POST: start t=(\d+) (\d+)x(\d+)", s)
    check("POST screen drawn", m is not None, "no RETRO-POST start marker")
    if m:
        results["metrics"]["post_start_ms_since_kernel"] = int(m.group(1))

    # 2. First-boot Setup.
    t = q.wait_text("Welcome to RetroOS", 300, "setup-1-welcome")
    check("Setup welcome page", "Welcome to RetroOS" in t)
    q.key("ret")
    q.wait_text("Your Account", 60, "setup-2-account")
    q.type("Test User")
    q.key("tab")  # user name is suggested from the full name
    q.key("tab")
    q.type("retro123")
    q.key("tab")
    q.type("retro123")
    q.key("ret")
    t = q.wait_text("Your Computer", 60, "setup-3-computer")
    check("Account page accepted", "Your Computer" in t)
    q.key("ret"), q.key("ret"), q.key("ret")
    q.wait_text("Startup", 60, "setup-4-startup")
    q.key("ret"), q.key("ret")
    t = q.wait_text("Ready to Set Up", 60, "setup-5-confirm")
    check("Confirm page shows user", "test" in t)
    q.key("ret")
    t = q.wait_text("Setup Complete", 120, "setup-6-done")
    check("Setup completed", "RetroOS is ready" in t)
    q.key("ret")

    # 3. Automatic login → RetroDOS.
    t0 = q.elapsed()
    t = q.wait_text("C:\\>", 300, "dos-1-prompt")
    check("RetroDOS prompt after login", "RetroDOS Version 1.0" in t, t[-400:])
    results["metrics"]["setup_to_prompt_s_tcg"] = round(q.elapsed() - t0, 1)

    # 4. Commands.
    q.type("VER\n")
    t = q.wait_text("aarch64", 30, "dos-2-ver")
    check("VER reports the kernel", "RetroDOS Version 1.0" in t and "aarch64" in t)
    q.type("CLS\nMD GAMES\nCD GAMES\nECHO Hello from RetroOS > HELLO.TXT\nTYPE HELLO.TXT\n")
    t = q.wait_text("Hello from RetroOS", 30, "dos-3-files")
    check("MD/CD/ECHO >/TYPE", "C:\\GAMES>" in t and "Hello from RetroOS" in t)
    q.type("CLS\nDIR\n")
    t = q.wait_text("bytes free", 30, "dos-4-dir")
    check("DIR lists HELLO.TXT", "HELLO    TXT" in t)
    q.type("CLS\nMEM\n")
    t = q.wait_text("Conventional", 30, "dos-5-mem")
    check("MEM", "Conventional" in t)

    # 5. Measurements, reported by the guest over the serial port.
    q.type("CLS\n")
    time.sleep(60)  # let the system settle ("idle" = 60 s after reaching the prompt)
    q.type('UNIX "systemd-analyze > /dev/ttyAMA0; free -k > /dev/ttyAMA0; echo RETRO-MEASURE-END > /dev/ttyAMA0"\n')
    s = q.wait_serial("RETRO-MEASURE-END", 120)
    m = re.search(r"Startup finished in (.*?) = ([\d.]+)(ms|s)", s)
    if m:
        v = float(m.group(2)) * (0.001 if m.group(3) == "ms" else 1)
        results["metrics"]["systemd_analyze_total_s_tcg"] = round(v, 2)
        results["metrics"]["systemd_analyze"] = m.group(0)
    m = re.search(r"Mem:\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)", s)
    if m:
        total, avail = int(m.group(1)), int(m.group(6))
        used_mb = (total - avail) / 1024
        results["metrics"]["idle_ram_text_mode_mb"] = round(used_mb, 1)
        check("Idle RAM (text mode) under 100 MB", used_mb < 100, f"{used_mb:.1f} MB")
    q.screendump("dos-6-final")
    q.type('UNIX "systemctl --failed --no-legend > /dev/ttyAMA0; echo RETRO-FAILED-END > /dev/ttyAMA0"\n')
    s = q.wait_serial("RETRO-FAILED-END", 60)
    failed = s[s.rfind("RETRO-MEASURE-END"):s.rfind("RETRO-FAILED-END")].strip().splitlines()[1:]
    failed = [l for l in failed if ".service" in l or ".mount" in l]
    results["metrics"]["failed_units"] = failed
    check("No failed systemd units", not failed, "; ".join(failed))


if __name__ == "__main__":
    main()
