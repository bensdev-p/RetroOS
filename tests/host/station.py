"""Drive a headless Station over its broker socket (host-side tests).

    python3 tests/host/station.py <bin-dir> <out-dir> [script]

Starts `station --headless 1920x1080 --exec retro-console --test-ipc` with a
private XDG_RUNTIME_DIR and HOME, then runs a script of steps:
    type <text> | key <combo> | wait <ms> | shot <name> | expect-task <n> | tasks
    expect-file <name> <text>   (file under HOME contains text, within 5 s)
    request <BROKER REQUEST>    (fields separated by spaces; must answer OK)
    expect <REQUEST> => <text>  (the reply, all lines, contains text)
    pointer <x> <y> | button <down|up|click> [right]   (mouse, logical pixels)
    windows                     (print the WINDOWS reply)
    expect-pixel <shot> <x> <y> <RRGGBB>   (a pixel of an earlier shot)
Extra Station arguments come from $STATION_ARGS (e.g. --personality desktop).
"""
import os, socket, subprocess, sys, tempfile, time, glob

def main():
    bindir, outdir = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
    script = sys.argv[3] if len(sys.argv) > 3 else None
    os.makedirs(outdir, exist_ok=True)
    run = tempfile.mkdtemp(prefix="retro-xdg-")
    os.chmod(run, 0o700)
    home = tempfile.mkdtemp(prefix="retro-home-")
    os.makedirs(os.path.join(home, "Documents"))
    with open(os.path.join(home, "README.TXT"), "w") as f:
        f.write("Welcome to RetroOS.\n")
    # A "graphical application" for launch tests: a second console window.
    apps = os.path.join(home, ".local", "share", "applications")
    os.makedirs(apps)
    with open(os.path.join(apps, "demo.desktop"), "w") as f:
        f.write("[Desktop Entry]\nType=Application\nName=Demo App\n"
                "Exec=retro-console --once --app-id retroos.demo --exec retrodos /K ECHO This is the demo app.\n")
    # Applications come from the test HOME only, not from the machine.
    nodata = tempfile.mkdtemp(prefix="retro-data-")
    # Every per-person folder inside the test's HOME: CI runners set
    # XDG_CONFIG_HOME to their own (programs then saved desktop.conf there).
    xdg = {k: os.path.join(home, v) for k, v in [("XDG_CONFIG_HOME", ".config"),
           ("XDG_DATA_HOME", ".local/share"), ("XDG_STATE_HOME", ".local/state"),
           ("XDG_CACHE_HOME", ".cache")]}
    env = dict(os.environ, **xdg, XDG_RUNTIME_DIR=run, HOME=home, USER="retro", XDG_DATA_DIRS=nodata,
               PATH=bindir + ":" + os.environ["PATH"], RUST_LOG="station=info")
    env.pop("WAYLAND_DISPLAY", None)
    log = open(os.path.join(outdir, "station.log"), "w")
    p = subprocess.Popen([os.path.join(bindir, "station"), "--headless", "1920x1080",
                          "--exec", os.environ.get("STATION_EXEC", "retro-console"), "--test-ipc"]
                         + os.environ.get("STATION_ARGS", "").split(), env=env, cwd=home,
                         stdout=log, stderr=subprocess.STDOUT)
    sock_path = None
    for _ in range(100):
        g = glob.glob(os.path.join(run, "retroos-station-*.sock"))
        if g:
            sock_path = g[0]; break
        time.sleep(0.05)
    if not sock_path:
        print("FAIL: no broker socket"); p.kill(); sys.exit(1)
    s = socket.socket(socket.AF_UNIX); s.connect(sock_path)
    f = s.makefile("rw")
    def req(line):
        f.write(line + "\n"); f.flush()
        first = f.readline().strip()
        out = [first]
        if first.startswith("OK\t") and line in ("TASKS", "WINDOWS"):
            for _ in range(int(first.split("\t")[1])):
                out.append(f.readline().strip())
        return out
    steps = open(script).read().splitlines() if script else [
        "wait 1500", "shot 01-boot", "type VER", "key enter", "type DIR", "key enter",
        "wait 600", "shot 02-dir", "type COLOR AMBER", "key enter", "wait 400", "shot 03-amber"]
    ok = True
    for st in steps:
        st = st.strip()
        if not st or st.startswith("#"):
            continue
        cmd, _, arg = st.partition(" ")
        if cmd == "wait":
            time.sleep(int(arg) / 1000)
        elif cmd == "type":
            req("TYPE\t" + arg); time.sleep(0.05)
        elif cmd == "key":
            r = req("KEY\t" + arg); time.sleep(0.05)
            if r[0] != "OK":
                print("FAIL key", arg, r); ok = False
        elif cmd == "shot":
            time.sleep(0.3)
            r = req("SCREENSHOT\t" + os.path.join(outdir, arg + ".png"))
            print("shot", arg, r[0])
        elif cmd == "expect-task":
            r = req("TASKS")
            n = int(r[0].split("\t")[1])
            if n != int(arg):
                print("FAIL tasks:", r); ok = False
        elif cmd == "expect-file":
            name, _, want = arg.partition(" ")
            want = want.replace("\\n", "\n")  # "\n" in a step is a line break
            # Poll for up to 5 s: a slow machine (CI) may still be saving.
            deadline = time.time() + 5
            while True:
                try:
                    got = open(os.path.join(home, name)).read()
                except OSError as e:
                    got = str(e)
                if want in got or time.time() > deadline:
                    break
                time.sleep(0.1)
            if want not in got:
                print("FAIL file", name, repr(got)); ok = False
        elif cmd == "request":  # raw broker request; must answer OK
            r = req(arg.replace(" ", "\t"))
            if not r[0].startswith("OK"):
                print("FAIL request", arg, r); ok = False
        elif cmd == "tasks":
            print("\n".join(req("TASKS")))
        elif cmd == "expect-pixel":
            from PIL import Image
            name, x, y, want = arg.split()
            px = Image.open(os.path.join(outdir, name + ".png")).convert("RGB").getpixel((int(x), int(y)))
            got = "%02X%02X%02X" % px
            if got != want.upper():
                print("FAIL pixel", name, x, y, "want", want, "got", got); ok = False
        elif cmd == "expect-golden":
            # expect-golden SHOT NAME X Y W H: that region of the screenshot
            # must equal tests/golden/NAME.png pixel for pixel (a deliberate
            # look change: RETRO_UPDATE_GOLDEN=1 rewrites the golden images).
            from PIL import Image, ImageChops
            shot, name, x, y, w, h = arg.split()
            x, y, w, h = int(x), int(y), int(w), int(h)
            got = Image.open(os.path.join(outdir, shot + ".png")).convert("RGB").crop((x, y, x + w, y + h))
            gold = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "golden", name + ".png")
            if os.environ.get("RETRO_UPDATE_GOLDEN"):
                os.makedirs(os.path.dirname(gold), exist_ok=True)
                got.save(gold)
                print("golden", name, "written")
            elif not os.path.exists(gold):
                print("FAIL golden", name, "missing (RETRO_UPDATE_GOLDEN=1 writes it)"); ok = False
            else:
                want = Image.open(gold).convert("RGB")
                diff = ImageChops.difference(got, want).getbbox() if want.size == got.size else (0, 0) + got.size
                if diff:
                    got.save(os.path.join(outdir, "golden-" + name + "-got.png"))
                    print("FAIL golden", name, "differs in", diff); ok = False
        elif cmd == "windows":
            print("\n".join(req("WINDOWS")))
        elif cmd == "expect":
            request, _, want = arg.partition(" => ")
            r = req(request.replace(" ", "\t"))
            got = "\n".join(r).replace("\t", " ")
            if want not in got:
                print("FAIL expect", request, "=>", want, "got:", got); ok = False
        elif cmd == "pointer":
            req("POINTER\t" + arg.replace(" ", "\t")); time.sleep(0.03)
        elif cmd == "button":
            r = req("BUTTON\t" + arg.replace(" ", "\t")); time.sleep(0.05)
            if r[0] != "OK":
                print("FAIL button", arg, r); ok = False
    req("QUIT")
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)

main()
