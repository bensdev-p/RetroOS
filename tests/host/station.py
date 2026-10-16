"""Drive a headless Station over its broker socket (host-side tests).

    python3 tests/host/station.py <bin-dir> <out-dir> [script]

Starts `station --headless 1920x1080 --exec retro-console --test-ipc` with a
private XDG_RUNTIME_DIR and HOME, then runs a script of steps:
    type <text> | key <combo> | wait <ms> | shot <name> | expect-task <n>
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
    env = dict(os.environ, XDG_RUNTIME_DIR=run, HOME=home, USER="retro",
               PATH=bindir + ":" + os.environ["PATH"], RUST_LOG="station=info")
    env.pop("WAYLAND_DISPLAY", None)
    log = open(os.path.join(outdir, "station.log"), "w")
    p = subprocess.Popen([os.path.join(bindir, "station"), "--headless", "1920x1080",
                          "--exec", os.environ.get("STATION_EXEC", "retro-console"), "--test-ipc"], env=env, cwd=home,
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
        if first.startswith("OK\t") and line == "TASKS":
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
            req("KEY\t" + arg); time.sleep(0.05)
        elif cmd == "shot":
            time.sleep(0.3)
            r = req("SCREENSHOT\t" + os.path.join(outdir, arg + ".png"))
            print("shot", arg, r[0])
        elif cmd == "expect-task":
            r = req("TASKS")
            n = int(r[0].split("\t")[1])
            if n != int(arg):
                print("FAIL tasks:", r); ok = False
        elif cmd == "tasks":
            print("\n".join(req("TASKS")))
    req("QUIT")
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)

main()
