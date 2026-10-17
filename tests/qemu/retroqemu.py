"""RetroOS QEMU test harness.

Boots out/qemu (generic arm64 kernel + the RetroOS root file system) on
`qemu-system-aarch64 -M virt` with virtio-gpu, drives it through QMP
(keyboard, screendumps) and reads the screen back as text by matching every
8x16 cell (at any integer scale) against the Phosphor Mono bitmaps.

Used by tests/qemu/test_m1.py; also importable for ad-hoc sessions.
"""
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

# QMP qcodes for characters on a US keyboard: char -> (qcode, shift)
_PLAIN = {c: c for c in "abcdefghijklmnopqrstuvwxyz0123456789"}
_PLAIN.update({" ": "spc", "-": "minus", "=": "equal", "[": "bracket_left", "]": "bracket_right",
               ";": "semicolon", "'": "apostrophe", "`": "grave_accent", "\\": "backslash",
               ",": "comma", ".": "dot", "/": "slash", "\n": "ret", "\t": "tab"})
_SHIFTED = {"!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9",
            ")": "0", "_": "minus", "+": "equal", "{": "bracket_left", "}": "bracket_right",
            ":": "semicolon", '"': "apostrophe", "~": "grave_accent", "|": "backslash", "<": "comma",
            ">": "dot", "?": "slash"}


def qcode(ch):
    if ch in _PLAIN:
        return _PLAIN[ch], False
    if ch.isalpha() and ch.isupper():
        return ch.lower(), True
    if ch in _SHIFTED:
        q = _SHIFTED[ch]
        return _PLAIN.get(q, q), True
    raise ValueError(f"no key for {ch!r}")


class ScreenReader:
    """Turns screendumps of the RetroOS console into text."""

    def __init__(self, font_json):
        with open(font_json) as f:
            raw = json.load(f)
        self.glyphs = {}
        for cp, rows in raw.items():
            key = tuple(rows)
            # Prefer the lowest code point for identical bitmaps (space vs nbsp).
            if key not in self.glyphs or int(cp) < ord(self.glyphs[key]):
                self.glyphs[key] = chr(int(cp))

    def read(self, path, scale=None):
        img = Image.open(path).convert("RGB")
        w, h = img.size
        if scale is None:
            # The console uses the largest scale that gives >= 80x25 cells.
            scale = max(1, min(w // 640, h // 400))
        cw, chh = 8 * scale, 16 * scale
        cols, rows = w // cw, h // chh
        ox, oy = (w - cols * cw) // 2, (h - rows * chh) // 2
        px = img.load()
        lines = []
        for r in range(rows):
            line = []
            for c in range(cols):
                x0, y0 = ox + c * cw, oy + r * chh
                # Sample one pixel per font pixel (centre of the scaled block).
                samples = [[px[x0 + gx * scale + scale // 2, y0 + gy * scale + scale // 2] for gx in range(8)]
                           for gy in range(16)]
                flat = [p for row in samples for p in row]
                bg = max(set(flat), key=flat.count)
                bits = tuple(sum((1 << (7 - gx)) for gx in range(8) if samples[gy][gx] != bg) for gy in range(16))
                ch = self.glyphs.get(bits)
                if ch is None:
                    # The DOS underline cursor sits in rows 14-15: ignore them.
                    trimmed = bits[:14] + (0, 0)
                    ch = self.glyphs.get(trimmed, "?")
                line.append(ch)
            lines.append("".join(line).rstrip())
        return "\n".join(lines)


class Qemu:
    def __init__(self, qemu_dir, workdir, mem_mb=2048, smp=4, width=1280, height=800, extra_append=""):
        self.dir = os.path.abspath(qemu_dir)
        self.work = os.path.abspath(workdir)
        os.makedirs(workdir, exist_ok=True)
        self.qmp_path = os.path.join(workdir, "qmp.sock")
        self.serial_path = os.path.join(workdir, "serial.log")
        self.boot = 0
        self.mem, self.smp, self.size = mem_mb, smp, (width, height)
        self.extra_append = extra_append
        self.proc = None
        self.qmp = None
        self.t0 = None
        self.reader = None
        self.shots = 0

    def start(self, font_json, keep_disk=False):
        self.reader = ScreenReader(font_json)
        disk = os.path.join(self.work, "root.qcow2")
        if os.path.exists(disk) and not keep_disk:
            os.remove(disk)
        if not os.path.exists(disk):
            subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-F", "raw", "-b",
                            os.path.join(self.dir, "root.ext4"), disk, "8G"], check=True)
        with open(os.path.join(self.dir, "cmdline")) as f:
            append = f.read().strip()
        # Journal to the serial console so the harness can see RetroOS markers.
        append = append.replace("console=ttyAMA0 console=tty1", "console=tty1 console=ttyAMA0")
        append += " systemd.journald.forward_to_console=1 " + self.extra_append
        use_kvm = os.uname().machine == "aarch64" and os.access("/dev/kvm", os.R_OK | os.W_OK) \
            and os.environ.get("RETRO_QEMU_ACCEL", "auto") != "tcg"
        for attempt in (["kvm", "host"], ["tcg,thread=multi", "cortex-a76"]):
            if attempt[0] == "kvm" and not use_kvm:
                continue
            if self._launch(append, disk, accel=attempt[0], cpu=attempt[1]):
                return
        raise RuntimeError("QEMU did not start; see " + os.path.join(self.work, "qemu.err") + ":\n"
                           + open(os.path.join(self.work, "qemu.err")).read()[-2000:])

    def _launch(self, append, disk, accel, cpu):
        cmd = ["qemu-system-aarch64", "-M", "virt", "-cpu", cpu, "-accel", accel, "-smp", str(self.smp),
               "-m", str(self.mem), "-kernel", os.path.join(self.dir, "vmlinuz"),
               "-initrd", os.path.join(self.dir, "initrd.img"), "-append", append,
               "-drive", f"file={disk},if=virtio,format=qcow2",
               "-device", f"virtio-gpu-pci,xres={self.size[0]},yres={self.size[1]}",
               "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci",
               "-netdev", "user,id=n0", "-device", "virtio-net-pci,netdev=n0",
               "-device", "virtio-rng-pci",
               "-display", "none", "-serial", f"file:{self.serial_path}",
               "-qmp", f"unix:{self.qmp_path},server=on,wait=off", "-no-reboot"]
        self.boot += 1
        if self.boot > 1:
            self.serial_path = os.path.join(self.work, f"serial-boot{self.boot}.log")
            cmd[cmd.index("-serial") + 1] = f"file:{self.serial_path}"
        if os.path.exists(self.qmp_path):
            os.remove(self.qmp_path)
        self.t0 = time.time()
        err = open(os.path.join(self.work, "qemu.err"), "a")
        err.write(f"--- boot {self.boot}: accel={accel}\n")
        err.flush()
        self.proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=err)
        s = None
        for _ in range(200):
            if self.proc.poll() is not None:
                self.boot -= 1
                return False  # QEMU gave up (e.g. KVM unusable); caller tries the next accelerator
            if os.path.exists(self.qmp_path):
                try:
                    s = socket.socket(socket.AF_UNIX)
                    s.connect(self.qmp_path)
                    break
                except OSError:
                    s = None
            time.sleep(0.1)
        if s is None:
            self.proc.kill()
            return False
        self.accel = accel
        self.qmp = s.makefile("rw")
        try:
            self.qmp.readline()
            self._qmp("qmp_capabilities")
        except (OSError, ValueError):
            # QEMU accepted the connection and then quit (KVM failing late).
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
            self.qmp = None
            self.boot -= 1
            return False
        if accel == "kvm":
            time.sleep(1)
            if self.proc.poll() is not None:
                self.boot -= 1
                return False
        return True

    def _qmp(self, cmd, **args):
        self.qmp.write(json.dumps({"execute": cmd, "arguments": args}) + "\n")
        self.qmp.flush()
        while True:
            r = json.loads(self.qmp.readline())
            if "return" in r or "error" in r:
                return r

    def elapsed(self):
        return time.time() - self.t0

    # ------------------------------------------------------------- input

    def key(self, *combo):
        """key('ctrl', 'alt', 'tab') or key('ret')."""
        self._qmp("send-key", keys=[{"type": "qcode", "data": k} for k in combo])
        time.sleep(0.08)

    def type(self, text):
        for ch in text:
            q, shift = qcode(ch)
            if shift:
                self.key("shift", q)
            else:
                self.key(q)

    # ------------------------------------------------------------ output

    def screendump(self, name=None):
        self.shots += 1
        name = name or f"shot{self.shots:03d}"
        ppm = os.path.join(self.work, name + ".ppm")
        r = self._qmp("screendump", filename=ppm)
        if "error" in r:
            return None
        png = os.path.join(self.work, name + ".png")
        Image.open(ppm).save(png)
        os.remove(ppm)
        return png

    def screen_text(self, name=None):
        p = self.screendump(name)
        return self.reader.read(p) if p else ""

    def serial(self):
        try:
            with open(self.serial_path, errors="replace") as f:
                return f.read()
        except FileNotFoundError:
            return ""

    def wait_serial(self, needle, timeout):
        end = time.time() + timeout
        while time.time() < end:
            s = self.serial()
            if needle in s:
                return s
            if self.proc.poll() is not None:
                raise RuntimeError("QEMU exited")
            time.sleep(0.5)
        raise TimeoutError(f"serial: {needle!r} not seen in {timeout}s")

    def wait_text(self, needle, timeout, name="wait"):
        end = time.time() + timeout
        last = ""
        while time.time() < end:
            last = self.screen_text(name)
            if needle in last:
                return last
            if self.proc.poll() is not None:
                raise RuntimeError("QEMU exited")
            time.sleep(1.0)
        raise TimeoutError(f"screen: {needle!r} not seen in {timeout}s; last screen:\n{last}")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                self._qmp("quit")
            except Exception:
                pass
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def font_json(path):
    """Export the font from the built retro-fontc."""
    tool = os.path.join(ROOT, "target", "release", "retro-fontc")
    if not os.path.exists(tool):
        subprocess.run(["cargo", "build", "--release", "-q", "-p", "retro-fontc"], cwd=ROOT, check=True)
    subprocess.run([tool, "json", path], check=True)
    return path
