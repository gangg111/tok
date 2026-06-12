#!/data/data/com.termux/files/usr/bin/env python3
"""Benchmark: raw vs rtk vs tok — token savings + information retention.

Token metric = whitespace-split count (identical to rtk's own count_tokens
in its test suite), plus chars. rtk's "[rtk] /!\\ No hook installed" banner
is stripped from rtk outputs (courtesy: it would not appear with a hook).
"""
import os, re, subprocess, sys, time, json

HOME = os.path.expanduser("~")
RTK = os.path.join(HOME, "rtk-upstream", "target", "release", "rtk")
TOK = os.path.join(HOME, ".local", "bin", "tok")
UP = os.path.join(HOME, "rtk-upstream")
SCRATCH = os.path.join(HOME, "tok", "bench", "scratch-repo")
CORPUS = os.path.join(HOME, "tok", "bench", "corpus")
OUTDIR = os.path.join(HOME, "tok", "bench", "outputs")
os.makedirs(OUTDIR, exist_ok=True)

BANNER_RE = re.compile(r"^\[rtk\].*hook.*\n?", re.M)

def sh(cmd, cwd=None):
    t0 = time.perf_counter()
    p = subprocess.run(cmd, shell=True, cwd=cwd,
                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    dt = time.perf_counter() - t0
    return p.stdout.decode("utf-8", "replace"), p.returncode, dt

def toks(s):
    return len(s.split())

CASES = [
    dict(name="ls (duzy katalog)", cwd=UP,
         raw="ls -la src/cmds/system",
         rtk=f"{RTK} ls src/cmds/system",
         tok=f"{TOK} ls src/cmds/system",
         facts=["ls.rs", "pipe_cmd.rs", "grep_cmd.rs"]),
    dict(name="find *.rs (cale src)", cwd=UP,
         raw='find src -name "*.rs"',
         rtk=f'{RTK} find src -name "*.rs"',
         tok=f'{TOK} find src -name "*.rs"',
         facts=["hook_cmd.rs", "toml_filter.rs"]),
    dict(name="grep -rn (cale src)", cwd=UP,
         raw='grep -rn "lazy_static!" src',
         rtk=f'{RTK} grep "lazy_static!" src',
         tok=f'{TOK} grep -rn "lazy_static!" src',
         facts=["toml_filter.rs", "408", "filter.rs"]),
    dict(name="git log -20", cwd=UP,
         raw="git log -20",
         rtk=f"{RTK} git log -20",
         tok=f"{TOK} git log -20",
         facts=["6785a6c", "regression"]),
    dict(name="git status (brudne repo)", cwd=SCRATCH,
         raw="git status",
         rtk=f"{RTK} git status",
         tok=f"{TOK} git status",
         facts=["NEW_FILE.txt", "main.rs", "utils.rs", "scratch_note.md", "README.md"]),
    dict(name="git diff (brudne repo)", cwd=SCRATCH,
         raw="git diff",
         rtk=f"{RTK} git diff",
         tok=f"{TOK} git diff",
         facts=["Token Annihilator", "bench edit", "utils.rs", "README.md"]),
    dict(name="gradle assembleDebug (replay)", cwd=CORPUS,
         raw="./gradlew assembleDebug --console=plain",
         rtk=f"PATH=.:$PATH {RTK} gradlew assembleDebug --console=plain",
         tok=f"{TOK} ./gradlew assembleDebug --console=plain",
         facts=["BUILD SUCCESSFUL", "33", "16", "17"]),
    dict(name="cargo build (2 bledy)", cwd=os.path.join(HOME, "tok", "bench", "errcrate"),
         raw="cargo build 2>&1; true",
         rtk=f"{RTK} cargo build",
         tok=f"{TOK} cargo build",
         facts=["E0425", "undefined_fn", "mismatched types"]),
    dict(name="ffmpeg encode (NIEZNANA dla rtk)", cwd=OUTDIR,
         raw="ffmpeg -y -f lavfi -i sine=frequency=440:duration=120 -b:a 192k tone.mp3",
         rtk=f"{RTK} ffmpeg -y -f lavfi -i sine=frequency=440:duration=120 -b:a 192k tone.mp3",
         tok=f"{TOK} ffmpeg -y -f lavfi -i sine=frequency=440:duration=120 -b:a 192k tone.mp3",
         facts=["tone.mp3"]),
    dict(name="pkg list-installed (NIEZNANA dla rtk)", cwd=HOME,
         raw="pkg list-installed 2>/dev/null; true",
         rtk=f"{RTK} pkg list-installed",
         tok=f"{TOK} pkg list-installed",
         facts=["zsh"]),
]

rows = []
fact_fail = []
for i, c in enumerate(CASES):
    res = {}
    for kind in ("raw", "rtk", "tok"):
        out, code, dt = sh(c[kind], cwd=c["cwd"])
        if kind == "rtk":
            out = BANNER_RE.sub("", out)
        res[kind] = out
        with open(os.path.join(OUTDIR, "%02d_%s.txt" % (i, kind)), "w") as f:
            f.write(out)
    tr, tk, tt = toks(res["raw"]), toks(res["rtk"]), toks(res["tok"])
    sr = 100.0 * (1 - tk / tr) if tr else 0.0
    st = 100.0 * (1 - tt / tr) if tr else 0.0
    missing_rtk = [f for f in c["facts"] if f not in res["rtk"]]
    missing_tok = [f for f in c["facts"] if f not in res["tok"]]
    if missing_tok:
        fact_fail.append((c["name"], "tok", missing_tok))
    rows.append((c["name"], tr, tk, sr, tt, st,
                 "OK" if not missing_rtk else "BRAK:" + ",".join(missing_rtk),
                 "OK" if not missing_tok else "BRAK:" + ",".join(missing_tok)))

print("\n%-36s %7s | %7s %6s | %7s %6s | %-22s %-22s" % (
    "PRZYPADEK", "raw", "rtk", "oszcz", "tok", "oszcz", "fakty(rtk)", "fakty(tok)"))
print("-" * 135)
tot = [0, 0, 0]
for name, tr, tk, sr, tt, st, fr, ft in rows:
    tot[0] += tr; tot[1] += tk; tot[2] += tt
    print("%-36s %7d | %7d %5.1f%% | %7d %5.1f%% | %-22s %-22s" % (
        name, tr, tk, sr, tt, st, fr, ft))
print("-" * 135)
print("%-36s %7d | %7d %5.1f%% | %7d %5.1f%%" % (
    "RAZEM", tot[0], tot[1], 100.0 * (1 - tot[1] / tot[0]), tot[2], 100.0 * (1 - tot[2] / tot[0])))
if fact_fail:
    print("\nUWAGA — tok zgubil fakty:", fact_fail)

# latency: 10x git status
print("\nLatencja (10x git status, sekundy lacznie):")
for label, cmd in (("raw git", "git status"), ("rtk", f"{RTK} git status"), ("tok", f"{TOK} git status")):
    t0 = time.perf_counter()
    for _ in range(10):
        sh(cmd, cwd=UP)
    print("  %-8s %.2fs (%.0f ms/wywolanie)" % (label, time.perf_counter() - t0,
                                                (time.perf_counter() - t0) * 100))
