#!/data/data/com.termux/files/usr/bin/env python3
"""Latency: tok-native vs rtk (warmup + 30 reps, mean ms per call)."""
import os, subprocess, time

HOME = os.path.expanduser("~")
RTK = f"{HOME}/rtk-upstream/target/release/rtk"
TOK = f"{HOME}/tok/tok-native"
UP = f"{HOME}/rtk-upstream"
HOOK_JSON = '{"tool_name":"Bash","tool_input":{"command":"git status"}}'

def measure(cmd, cwd=None, stdin=None, reps=30, warmup=3):
    for _ in range(warmup):
        subprocess.run(cmd, shell=True, cwd=cwd, input=stdin,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t0 = time.perf_counter()
    for _ in range(reps):
        subprocess.run(cmd, shell=True, cwd=cwd, input=stdin,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return (time.perf_counter() - t0) / reps * 1000

ROWS = [
    ("startup (--version)",      f"{RTK} --version",            f"{TOK} --version",            None, None),
    ("git status (cale wywol.)", f"{RTK} git status",           f"{TOK} git status",           UP,   None),
    ("ls (duzy katalog)",        f"{RTK} ls src/cmds/system",   f"{TOK} ls src/cmds/system",   UP,   None),
    ("hook PreToolUse",          f"{RTK} hook claude",          f"{TOK} hook claude",          None, HOOK_JSON.encode()),
]
print("%-26s %10s %10s  %s" % ("SCIEZKA", "rtk [ms]", "tok [ms]", "zwyciezca"))
print("-" * 62)
for name, rc, tc, cwd, stdin in ROWS:
    r = measure(rc, cwd, stdin)
    t = measure(tc, cwd, stdin)
    print("%-26s %10.1f %10.1f  %s" % (name, r, t,
          "tok (%.1fx szybszy)" % (r / t) if t < r else "rtk (%.1fx)" % (t / r)))
raw = measure("git status", UP)
print("\n(referencja: surowy git status = %.1f ms)" % raw)
