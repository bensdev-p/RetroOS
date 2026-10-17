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
    q.font = font_json(os.path.join(WORK, "font.json"))
    q.start(q.font)
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
    line = re.search(r"Startup finished in .*\(kernel\).*", s)
    if line:
        results["metrics"]["first_boot_systemd_analyze_tcg"] = line.group(0).strip()
    m = re.search(r"Mem:\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)", s)
    if m:
        total, avail = int(m.group(1)), int(m.group(6))
        results["metrics"]["idle_used_total_minus_available_mb"] = round((total - avail) / 1024, 1)
    q.screendump("dos-6-final")
    q.type('UNIX "systemctl --failed --no-legend > /dev/ttyAMA0; echo RETRO-FAILED-END > /dev/ttyAMA0"\n')
    s = q.wait_serial("RETRO-FAILED-END", 60)
    failed = s[s.rfind("RETRO-MEASURE-END"):s.rfind("RETRO-FAILED-END")].strip().splitlines()[1:]
    failed = [re.sub(r"\x1b\[[0-9;]*m", "", l).replace("●", "").split()[0] for l in failed if ".service" in l or ".mount" in l]
    results["metrics"]["failed_units"] = failed
    check("No failed systemd units", not failed, "; ".join(failed))

    # 6. Diagnostics for the report: who uses memory, and logs of anything that failed.
    units = " ".join(["retro-post"] + failed)
    q.type('UNIX "(echo RETRO-DIAG-BEGIN; ps -eo rss,pss,comm --sort=-rss | head -25; cat /proc/meminfo; '
           'grep -E \'^nr_free_cma|^nr_shmem \' /proc/vmstat; df -k /run /dev/shm /tmp; '
           f'for u in {units}; do echo retro123 | sudo -S -p \'\' journalctl -b -u $u --no-pager -o cat | tail -15; done; '
           'echo retro123 | sudo -S -p \'\' dmesg | grep -iE \'drm|virtio_gpu|cma\' | head -20; '
           'echo RETRO-DIAG-END) > /dev/ttyAMA0 2>&1"\n')
    s = q.wait_serial("RETRO-DIAG-END", 120)
    diag = s[s.rfind("RETRO-DIAG-BEGIN"):s.rfind("RETRO-DIAG-END")]
    with open(os.path.join(WORK, "diagnostics.txt"), "w") as f:
        f.write(diag)
    mi = {k: int(v) for k, v in re.findall(r"^(\w+):\s+(\d+) kB", diag, re.M)}
    if mi:
        classic = (mi["MemTotal"] - mi["MemFree"] - mi["Buffers"] - mi["Cached"] + mi["Shmem"]) / 1024
        results["metrics"]["idle_used_classic_mb"] = round(classic, 1)
        results["metrics"]["idle_breakdown_mb"] = {k: round(mi.get(k, 0) / 1024, 1) for k in
            ["AnonPages", "Shmem", "Slab", "VmallocUsed", "KernelStack", "PageTables", "Percpu", "Mlocked"]}
        # QEMU runs Debian's generic kernel with many modules; the < 100 MB target
        # is checked on the Pi (docs/HW_CHECKLIST.md 1.18). Here: a regression guard.
        check("Idle RAM regression guard (classic used < 150 MB in QEMU)", classic < 150, f"{classic:.1f} MB")

    # 7. Second boot: no Setup, straight to RetroDOS.
    q.type("RESTART /Y\n")
    q.proc.wait(timeout=180)
    q.start(q.font, keep_disk=True)
    t = q.wait_text("C:\\>", 600, "boot2-prompt")
    check("Second boot reaches RetroDOS", "RetroDOS Version 1.0" in t)
    results["metrics"]["second_boot_poweron_to_prompt_s_tcg"] = round(q.elapsed(), 1)
    q.type('UNIX "(systemd-analyze; systemd-analyze blame | head -12; echo RETRO-BOOT2-END) > /dev/ttyAMA0"\n')
    s = q.wait_serial("RETRO-BOOT2-END", 120)
    line = re.search(r"Startup finished in .*\(kernel\).*", s)
    if line:
        results["metrics"]["second_boot_systemd_analyze_tcg"] = line.group(0).strip()
    blame = s[s.rfind("Startup finished"):s.rfind("RETRO-BOOT2-END")].splitlines()[1:]
    results["metrics"]["second_boot_blame_tcg"] = [b.strip() for b in blame if b.strip()][:12]
    m = re.search(r"RETRO-POST: start t=(\d+)", q.serial())
    check("POST screen drawn (second boot)", m is not None)


if __name__ == "__main__":
    main()
