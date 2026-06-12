#!/data/data/com.termux/files/usr/bin/env python3
"""Benchmark 2: rtk's HOME TURF — replay rtk's own test fixtures
(tests/fixtures/) through both tools end-to-end via shim executables.
Token metric: whitespace split (rtk's own count_tokens)."""
import os, re, shutil, stat, subprocess

HOME = os.path.expanduser("~")
RTK = f"{HOME}/rtk-upstream/target/release/rtk"
TOK = f"{HOME}/tok/tok-native"
FIX = f"{HOME}/rtk-upstream/tests/fixtures"
WORK = f"{HOME}/tok/bench/turf"
OUT = f"{HOME}/tok/bench/outputs2"
BANNER_RE = re.compile(r"^\[rtk\].*hook.*\n?", re.M)
SHEBANG = "#!/data/data/com.termux/files/usr/bin/sh\n"

CASES = [
    ("gradle build FAILED",  "gradlew", "gradlew_build_failed_raw.txt", "gradlew assembleDebug",
     ["compileDebugKotlin", "Unresolved reference: MyService", "Type mismatch", "BUILD FAILED"]),
    ("gradle test FAILED",   "gradlew", "gradlew_test_failed_raw.txt", "gradlew test",
     ["5 tests completed, 2 failed", "BUILD FAILED"]),
    ("mvn test FAILED",      "mvn",     "mvn_test_fail_slice_raw.txt", "mvn test",
     ["RtkInducedFailTest", "expected: <expected> but was: <actual>", "BUILD FAILURE"]),
    ("mvn install OK",       "mvn",     "mvn_install_slice_raw.txt", "mvn install",
     ["BUILD SUCCESS"]),
    ("glab CI trace (job failed)", "glab", "glab_ci_trace_raw.txt", "glab ci trace 123",
     ["ERROR: Job failed: exit code 1"]),
]

os.makedirs(OUT, exist_ok=True)
rows = []
for i, (name, shim, fixture, cmdline, facts) in enumerate(CASES):
    d = f"{WORK}/{i}"
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    sp = f"{d}/{shim}"
    with open(sp, "w") as f:
        f.write(SHEBANG + f'cat "{FIX}/{fixture}"\n')
    os.chmod(sp, os.stat(sp).st_mode | stat.S_IEXEC)
    env = dict(os.environ, PATH=f"{d}:" + os.environ["PATH"])
    res = {}
    for kind, prefix in (("raw", ""), ("rtk", RTK + " "), ("tok", TOK + " ")):
        p = subprocess.run(prefix + cmdline, shell=True, cwd=d, env=env,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        out = p.stdout.decode("utf-8", "replace")
        if kind == "rtk":
            out = BANNER_RE.sub("", out)
        res[kind] = out
        with open(f"{OUT}/{i}_{kind}.txt", "w") as f:
            f.write(out)
    tr, tk, tt = (len(res[k].split()) for k in ("raw", "rtk", "tok"))
    fr = "OK" if all(f in res["rtk"] for f in facts) else \
         "BRAK:" + ",".join(f for f in facts if f not in res["rtk"])
    ft = "OK" if all(f in res["tok"] for f in facts) else \
         "BRAK:" + ",".join(f for f in facts if f not in res["tok"])
    rows.append((name, tr, tk, 100*(1-tk/tr), tt, 100*(1-tt/tr), fr, ft))

print("%-30s %6s | %6s %6s | %6s %6s | %-12s %-12s" %
      ("PRZYPADEK (fixture rtk)", "raw", "rtk", "oszcz", "tok", "oszcz", "fakty rtk", "fakty tok"))
print("-" * 110)
tot = [0, 0, 0]
for name, tr, tk, sr, tt, st, fr, ft in rows:
    tot[0] += tr; tot[1] += tk; tot[2] += tt
    print("%-30s %6d | %6d %5.1f%% | %6d %5.1f%% | %-12s %-12s" %
          (name, tr, tk, sr, tt, st, fr, ft))
print("-" * 110)
print("%-30s %6d | %6d %5.1f%% | %6d %5.1f%%" %
      ("RAZEM", tot[0], tot[1], 100*(1-tot[1]/tot[0]), tot[2], 100*(1-tot[2]/tot[0])))
