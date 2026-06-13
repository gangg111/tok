#!/data/data/com.termux/files/usr/bin/env python3
"""tok — universal token-diet proxy for CLI commands.

Filters and compresses command output before it reaches an LLM context.
Unlike rtk, tok compresses ANY command (generic adaptive pipeline), not just
a fixed list — and it is a single zero-dependency Python file that runs on
every OS where Python 3.8+ exists (including Termux/bionic where prebuilt
rtk binaries do not run).

Usage:
  tok <command> [args...]      run command through the best filter
  tok run -- <command> ...     force the generic filter
  tok proxy <command> ...      raw passthrough (no filtering)
  tok pipe [name]              filter stdin (named or generic filter)
  tok full                     print full raw output of the last run
  tok gain                     show cumulative token savings
  tok init [-g] [--settings P] install Claude Code PreToolUse rewrite hook
  tok hook claude|codex        hook entrypoint (reads JSON on stdin)
"""
import io
import json
import os
import re
import subprocess
import sys
import time

VERSION = "0.1.0"
CACHE_DIR = os.environ.get("TOK_CACHE") or os.path.join(
    os.path.expanduser("~"), ".cache", "tok")
MAX_LINES = int(os.environ.get("TOK_MAX_LINES", "60"))
HEAD_KEEP = 12
TAIL_KEEP = 18
LS_GROUP_MIN = 200
LS_DIR_KEEP = 40

# ── regexes ──────────────────────────────────────────────────────────
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[()][AB0]")
IMPORTANT_RE = re.compile(
    r"(?i)\b(error|failed|failure|fatal|panic|exception|traceback|abort"
    r"|denied|refused|cannot|unable|missing|not found|undefined|unresolved"
    r"|conflict|rejected|timeout|segfault|killed)\b|^E\s |^\s*\^|error\[")
WARNING_RE = re.compile(r"(?i)\bwarn(ing)?\b")
NOISE_RE = re.compile(
    r"(?i)^\s*(\d{1,3}(\.\d+)?%|\[[=\->#\. ]{3,}\]|[\.⠇-⣿]+)\s*$"
    r"|^\s*(downloading|resolving|fetching|receiving|unpacking|extracting"
    r"|compressing|counting|enumerating|progress|building wheel"
    r"|collecting)\b.*(%|\d+/\d+|kb|mb|kib|mib|eta|objects)"
    r"|^remote:\s*(counting|compressing|total|enumerating)"
    r"|^\s*(⠁|⠂|⠄|⡀|⢀|⠠|⠐|⠈)")
NUM_NORM_RE = re.compile(r"\b\d[\d\.,:%/]*\b")
HEX_NORM_RE = re.compile(r"\b[0-9a-f]{7,40}\b")


def strip_ansi(text):
    return ANSI_RE.sub("", text)


def resolve_cr(text):
    """Keep only the final frame of \r-overwritten progress lines."""
    out = []
    for line in text.split("\n"):
        if "\r" in line:
            line = line.split("\r")[-1]
        out.append(line)
    return "\n".join(out)


def norm_line(line):
    line = HEX_NORM_RE.sub("H", line)
    return NUM_NORM_RE.sub("N", line.strip())


def human(n):
    for unit in ("B", "K", "M", "G"):
        if n < 1024 or unit == "G":
            return ("%d%s" % (n, unit)) if unit == "B" else ("%.1f%s" % (n, unit)).replace(".0", "")
        n /= 1024.0
    return "?"


def count_tokens(s):
    return len(s.split())


# ── generic adaptive filter (works on ANY output) ────────────────────
def generic_filter(raw, exit_code=0, max_lines=None):
    max_lines = max_lines or MAX_LINES
    text = resolve_cr(strip_ansi(raw))
    lines = text.split("\n")

    # 1. drop noise, squeeze blanks, dedup similar consecutive lines
    kept = []          # list of [line, count]
    blank = False
    for ln in lines:
        s = ln.rstrip()
        if not s.strip():
            blank = True
            continue
        if NOISE_RE.search(s) and not IMPORTANT_RE.search(s):
            continue
        if kept and norm_line(kept[-1][0]) == norm_line(s):
            kept[-1][1] += 1
            continue
        if blank and kept:
            kept.append(["", 1])
        blank = False
        kept.append([s, 1])

    out_lines = []
    for ln, n in kept:
        if not ln:
            out_lines.append("")
        elif n > 1:
            out_lines.append("%s  (x%d)" % (ln, n))
        else:
            out_lines.append(ln)

    # 2. global cap: keep head, tail and ALL important lines in between
    if len(out_lines) > max_lines:
        head = out_lines[:HEAD_KEEP]
        tail = out_lines[-TAIL_KEEP:]
        middle = out_lines[HEAD_KEEP:-TAIL_KEEP]
        important = [l for l in middle if IMPORTANT_RE.search(l)]
        omitted = len(middle) - len(important)
        if len(important) > max_lines:
            extra = len(important) - max_lines
            important = important[:max_lines]
            important.append("... +%d more error/warning lines [tok full]" % extra)
        marker = ["... %d lines omitted [tok full]" % omitted] if omitted > 0 else []
        out_lines = head + important + marker + tail

    result = "\n".join(out_lines).strip("\n")
    if not result:
        return "ok" if exit_code == 0 else "exit %d (no output) [tok full]" % exit_code
    if exit_code != 0:
        result += "\nexit %d [tok full]" % exit_code
    return result


# ── named line-rule filters (rtk-style, ported from rtk TOML DSL) ────
RULES = {
    "gradle": {
        "strip": [
            r"^\s*$", r"^> Configur(e|ing) project", r"^> Resolving dependencies",
            r"^FAILURE: Build failed with an exception", r"^Execution failed for task",
            r"^\* (What went wrong:|Try:|Get more help)", r"^> Run with --",
            r"^> A failure occurred while executing", r"^> Compilation error\.",
            r"See the report at:", r"There were failing tests", r" PASSED$",
            r"^> Transform ", r"^Download(ing)?\s+http", r"^\s*<-+>\s*$",
            r"^> Task :.*UP-TO-DATE$", r"^> Task :.*NO-SOURCE$",
            r"^> Task :.*FROM-CACHE$", r"^> Task :.*SKIPPED$",
            r"^Starting a Gradle Daemon", r"^Daemon will be stopped",
            r"^> Task :[^ ]+$",          # bare executed task names
            r"^\s*\d+ problems? were found storing the configuration cache",
            r"^See the profiling report", r"^Watched directory hierarchies",
            r"^To honour the JVM settings", r"^Calculating task graph",
            r"^Configuration cache entry", r"^Reusing configuration cache",
            r"^Deprecated Gradle features were used",
            r"^You can use '--warning-mode all'",
            r"^For more on this, please refer to https://docs.gradle.org",
        ],
        "max_lines": 50, "on_empty": "gradle: ok",
    },
    "pytest": {
        "strip": [
            r"^\s*$", r"^platform |^cachedir|^rootdir|^plugins|^configfile",
            r"^collecting|^collected", r"^\s*[\.sxF]+\s*(\[\s*\d+%\])?\s*$",
            r"^=+ test session starts =+$", r"PASSED", r"^-+ live log",
        ],
        "max_lines": 60, "on_empty": "pytest: ok",
    },
    "npm": {
        "strip": [
            r"^\s*$", r"^npm (notice|timing|http)", r"^added \d+ packages? in",
            r"^\s*(up to date|audited)", r"^\d+ packages? are looking for funding",
            r"^\s*run `npm fund`",
        ],
        "max_lines": 40, "on_empty": "npm: ok",
    },
    "ffmpeg": {
        "strip": [
            r"^\s*$", r"^\s*configuration:", r"^\s*built with ",
            r"^\s*lib(avutil|avcodec|avformat|avdevice|avfilter|swscale"
            r"|swresample|postproc)\s",
            r"^Press \[q\]", r"^\s*Metadata:", r"^\s*(TSSE|encoder|major_brand"
            r"|minor_version|compatible_brands|handler_name|vendor_id)\s*:",
            r"^Stream mapping:", r"^\s*Stream #\d+:\d+ -> ",
            r"^\[(in|out)#\d+/\w+ @ 0x[0-9a-f]+\] video:",
        ],
        "max_lines": 30, "on_empty": "ffmpeg: ok",
    },
    "pip": {
        "strip": [
            r"^\s*$", r"^\s*(Downloading|Collecting|Using cached|Found existing"
            r"|Uninstalling|Attempting uninstall|Preparing metadata"
            r"|Installing build dependencies|Getting requirements"
            r"|Building wheels|Created wheel|Stored in directory)",
            r"^Requirement already satisfied",
            r"\[notice\] (A new release of pip|To update)",
        ],
        "max_lines": 40, "on_empty": "pip: ok",
    },
}


def rules_filter(name, raw, exit_code=0):
    cfg = RULES[name]
    strip_res = [re.compile(p) for p in cfg["strip"]]
    lines = []
    for ln in resolve_cr(strip_ansi(raw)).split("\n"):
        s = ln.rstrip()
        if any(r.search(s) for r in strip_res) and not IMPORTANT_RE.search(s):
            continue
        lines.append(s)
    text = "\n".join(lines).strip("\n")
    if not text.strip():
        return cfg["on_empty"] if exit_code == 0 else "exit %d [tok full]" % exit_code
    return generic_filter(text, exit_code, cfg.get("max_lines"))


# ── specialized handlers ─────────────────────────────────────────────
def cap_list(items, keep):
    if len(items) > keep:
        return ",".join(items[:keep]) + ",...+%d" % (len(items) - keep)
    return ",".join(items)


def ls_ext_groups(files):
    groups, order = {}, []
    for name, sz in files:
        stem, _, ext = name.rpartition(".")
        ext = ext.lower() if stem and ext else "noext"
        if ext not in groups:
            groups[ext] = [0, 0]
            order.append(ext)
        groups[ext][0] += 1
        groups[ext][1] += sz
    order.sort(key=lambda e: (-groups[e][0], e))
    return [
        "%s:%d:%s" % (e, groups[e][0], human(groups[e][1]))
        if groups[e][1] >= 1024
        else "%s:%d" % (e, groups[e][0])
        for e in order
    ]


def h_ls(args):
    """Native compact listing — no perms/owner/date noise."""
    show_hidden = any(a in ("-a", "-la", "-al", "-lah", "-A") for a in args)
    paths = [a for a in args if not a.startswith("-")] or ["."]
    out = []
    for p in paths:
        p = os.path.expanduser(p)
        if not os.path.isdir(p):
            if os.path.exists(p):
                out.append("%s %s" % (p, human(os.path.getsize(p))))
            else:
                out.append("ls: %s: not found" % p)
                continue
            continue
        try:
            entries = sorted(os.scandir(p), key=lambda e: e.name)
        except OSError as e:
            out.append("ls: %s" % e)
            continue
        dirs, files, total = [], [], 0
        for e in entries:
            if not show_hidden and e.name.startswith("."):
                continue
            if e.is_dir(follow_symlinks=False):
                dirs.append(e.name + "/")
            else:
                try:
                    sz = e.stat(follow_symlinks=False).st_size
                except OSError:
                    sz = 0
                total += sz
                files.append((e.name, sz))
        hdr = "%s: %d dirs, %d files, %s" % (p, len(dirs), len(files), human(total))
        out.append(hdr)
        if dirs:
            out.append("  " + cap_list(dirs, LS_DIR_KEEP))
        # huge dir: a per-file listing would dwarf the savings — group by extension
        grouped = len(files) > LS_GROUP_MIN
        if grouped:
            items = ls_ext_groups(files)
        else:
            items = ["%s:%s" % (n, human(sz)) if sz >= 1024 else n for n, sz in files]
        line = "  by ext: " if grouped else "  "
        for f in items:
            if len(line) + len(f) > 96:
                out.append(line.rstrip(","))
                line = "  "
            line += f + ","
        if line.strip(" ,"):
            out.append(line.rstrip(","))
    return "\n".join(out), 0


def h_find(cmd):
    p = run_raw(cmd)
    text = strip_ansi(p["out"])
    groups, order, errs = {}, [], []
    for ln in text.split("\n"):
        if not ln.strip():
            continue
        if ln.startswith("find:"):
            errs.append(ln)
            continue
        d, b = os.path.split(ln.rstrip("/"))
        d = d or "."
        if d not in groups:
            groups[d] = []
            order.append(d)
        groups[d].append(b)
    out = []
    for d in order:
        names = groups[d]
        out.append("%s/: %s" % (d, ",".join(names[:40])
                                + (",...+%d" % (len(names) - 40) if len(names) > 40 else "")))
    total = sum(len(v) for v in groups.values())
    if total:
        out.insert(0, "%d results in %d dirs" % (total, len(groups)))
    out.extend(errs[:5])
    return generic_filter("\n".join(out), p["code"], 80), p["code"]


def h_grep(cmd):
    p = run_raw(cmd)
    text = strip_ansi(p["out"])
    groups, order = {}, []
    misc = []
    for ln in text.split("\n"):
        if not ln.strip():
            continue
        m = re.match(r"^(.{1,200}?):(\d+):(.*)$", ln)
        if not m:
            misc.append(ln)
            continue
        f, num, content = m.group(1), m.group(2), m.group(3).strip()
        if f not in groups:
            groups[f] = []
            order.append(f)
        groups[f].append((num, content[:90]))
    # global grouping: identical match content across files → one line
    by_content, corder = {}, []
    total_hits = 0
    for f in order:
        for num, c in groups[f]:
            total_hits += 1
            key = c[:70]
            if key not in by_content:
                by_content[key] = {}
                corder.append(key)
            by_content[key].setdefault(f, []).append(num)
    out = []
    for c in corder[:25]:
        locs = by_content[c]
        loc_s = " ".join("%s:%s" % (f, ",".join(nums[:8]))
                         for f, nums in list(locs.items())[:30])
        if len(locs) > 30:
            loc_s += " ...+%d files" % (len(locs) - 30)
        out.append("%s <- %s" % (c, loc_s))
    if len(corder) > 25:
        out.append("...+%d distinct matches [tok full]" % (len(corder) - 25))
    out.extend(misc[:10])
    if order:
        out.insert(0, "%d matches in %d files" % (sum(len(v) for v in groups.values()), len(order)))
    text = "\n".join(out)
    lines = text.split("\n")
    if len(lines) > 100:
        text = "\n".join(lines[:100]) + "\n... +%d lines [tok full]" % (len(lines) - 100)
    return (text if text.strip() else ("no matches" if p["code"] == 1 else generic_filter(p["out"], p["code"]))), p["code"]


def h_git(cmd):
    sub = next((a for a in cmd[1:] if not a.startswith("-")), "")
    if sub == "status":
        p = run_raw(["git", "status", "--porcelain=v1", "-b"] )
        if p["code"] != 0:
            return generic_filter(p["out"], p["code"]), p["code"]
        lines = p["out"].strip().split("\n") if p["out"].strip() else []
        branch = lines[0][3:] if lines and lines[0].startswith("##") else "?"
        mod, untracked, staged, other = [], [], [], []
        for ln in lines[1:]:
            st, path = ln[:2], ln[3:]
            if st == "??":
                untracked.append(path)
            elif st[0] in "MADRC" and st[0] != " ":
                staged.append("%s %s" % (st.strip(), path))
            elif st[1] in "MD":
                mod.append("%s %s" % (st.strip(), path))
            else:
                other.append(ln)
        out = [branch]
        def block(label, items):
            if items:
                head = items[:15]
                more = "  ...+%d" % (len(items) - 15) if len(items) > 15 else ""
                out.append("%s(%d): %s%s" % (label, len(items), "  ".join(head), more))
        block("staged", staged)
        block("changed", mod)
        block("untracked", untracked)
        block("other", other)
        if len(out) == 1:
            out.append("clean")
        return "\n".join(out), 0
    if sub == "log" and not any(a.startswith("--format") or a.startswith("--pretty") for a in cmd):
        n = "20"
        paths = []
        skip = False
        for i, a in enumerate(cmd[2:], start=2):
            if skip:
                skip = False
                continue
            if a in ("-n", "--max-count"):
                if i + 1 < len(cmd):
                    n = cmd[i + 1]
                    skip = True
                continue
            m = re.match(r"^-(\d+)$", a)
            if m:
                n = m.group(1)
            elif not a.startswith("-"):
                paths.append(a)
        p = run_raw(["git", "log", "--oneline", "--no-color", "--no-decorate", "-n", n] + paths)
        return generic_filter(p["out"], p["code"], 80), p["code"]
    if sub == "diff":
        p = run_raw(["git", "-c", "color.ui=false"] + cmd[1:])
        raw = p["out"]
        if "--stat" in cmd or "--name-only" in cmd or "--name-status" in cmd:
            return generic_filter(raw, p["code"], 80), p["code"]
        kept, files = [], 0
        for ln in strip_ansi(raw).split("\n"):
            if ln.startswith("diff --git"):
                files += 1
                kept.append("* " + ln.split(" b/")[-1])
            elif ln.startswith(("+++", "---", "index ")):
                continue
            elif ln.startswith("@@"):
                kept.append(ln.split("@@")[1].strip() and "@@" + ln.split("@@")[1] + "@@" or ln)
            elif ln.startswith(("+", "-")):
                kept.append(ln)
        out = "\n".join(kept)
        if len(kept) > 200:
            out = "\n".join(kept[:200]) + "\n... diff truncated (%d more lines) [tok full]" % (len(kept) - 200)
        return (out if out.strip() else "no diff"), p["code"]
    if sub in ("push", "pull", "fetch", "add", "commit", "checkout", "switch", "merge", "rebase"):
        p = run_raw(cmd)
        if p["code"] == 0:
            br = subprocess.run(["git", "rev-parse", "--abbrev-ref", "HEAD"],
                                capture_output=True, text=True).stdout.strip()
            essentials = [l for l in strip_ansi(p["out"]).split("\n")
                          if IMPORTANT_RE.search(l) or re.search(
                              r"(?i)\b(up.to.date|fast.forward|files? changed|insertion|deletion|->)\b", l)]
            msg = "ok %s %s" % (sub, br)
            if essentials:
                msg += "\n" + "\n".join(essentials[:6])
            return msg, 0
        return generic_filter(p["out"], p["code"]), p["code"]
    p = run_raw(cmd)
    return generic_filter(p["out"], p["code"]), p["code"]


def cargo_test_summary(kept, code):
    """On a fully green `cargo test`, collapse the per-suite `test result:`
    lines into one total; any failure keeps the detailed output untouched."""
    if code != 0:
        return None
    suites = passed = failed = ignored = 0
    secs = 0.0
    for ln in kept:
        t = ln.lstrip()
        if not t.startswith("test result:"):
            continue
        if not t.startswith("test result: ok. "):
            return None
        suites += 1
        for part in t[len("test result: ok. "):].split(";"):
            p = part.strip()
            m = re.match(r"(\d+) (passed|failed|ignored)\b", p)
            if m:
                n = int(m.group(1))
                if m.group(2) == "passed":
                    passed += n
                elif m.group(2) == "failed":
                    failed += n
                else:
                    ignored += n
            m = re.search(r"finished in ([\d.]+)s", p)
            if m:
                secs += float(m.group(1))
    if not suites or failed:
        return None
    s = "cargo test: %d passed" % passed
    if ignored:
        s += ", %d ignored" % ignored
    return s + " (%d suite%s, %.2fs)" % (suites, "" if suites == 1 else "s", secs)


def h_cargo(cmd):
    p = run_raw(cmd)
    text = resolve_cr(strip_ansi(p["out"]))
    kept, in_block, warn = [], False, 0
    for ln in text.split("\n"):
        s = ln.rstrip()
        if s.startswith("error: could not compile"):
            in_block = False
            continue
        if re.match(r"^(error|warning)(\[|:)", s):
            if s.startswith("warning") and "generated" not in s:
                warn += 1
                in_block = False
                continue
            in_block = True
            kept.append(s)
            continue
        if in_block:
            if not s.strip():
                in_block = False
            else:
                kept.append(s)
            continue
        if re.match(r"^\s*(Compiling|Downloaded|Downloading|Updating|Locking|Checking|Documenting|Fresh|Adding)", s):
            continue
        if re.match(r"^\s*(Finished|Running|Doc-tests|test result)", s) \
                or re.match(r"^test .*(FAILED|panicked)", s) or "FAILED" in s \
                or re.match(r"^failures:", s):
            kept.append(s)
    summary = cargo_test_summary(kept, p["code"])
    if summary:
        kept = [ln for ln in kept
                if not re.match(r"^\s*(test result:|Running|Doc-tests|Finished)", ln)]
        kept.insert(0, summary)
    if warn:
        kept.append("(%d warnings hidden)" % warn)
    out = "\n".join(kept).strip("\n")
    if not out and p["code"] == 0:
        return "ok", 0
    return generic_filter(out, p["code"], 80), p["code"]


def h_rules(name):
    def handler(cmd):
        p = run_raw(cmd)
        return rules_filter(name, p["out"], p["code"]), p["code"]
    return handler


# ── execution plumbing ───────────────────────────────────────────────
LAST = {"raw": "", "code": 0}


def run_raw(cmd):
    try:
        proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        out = proc.stdout.decode("utf-8", "replace")
        LAST.update(raw=out, code=proc.returncode)
        return {"out": out, "code": proc.returncode}
    except FileNotFoundError:
        LAST.update(raw="%s: command not found" % cmd[0], code=127)
        return {"out": "%s: command not found" % cmd[0], "code": 127}


def save_raw(cmd, raw, code):
    try:
        os.makedirs(CACHE_DIR, exist_ok=True)
        with open(os.path.join(CACHE_DIR, "last.txt"), "w") as f:
            f.write(raw)
        with open(os.path.join(CACHE_DIR, "last.json"), "w") as f:
            json.dump({"cmd": cmd, "exit": code, "ts": time.time()}, f)
    except OSError:
        pass


def track(cmd, raw, filtered):
    try:
        os.makedirs(CACHE_DIR, exist_ok=True)
        with open(os.path.join(CACHE_DIR, "stats.jsonl"), "a") as f:
            f.write(json.dumps({"cmd": cmd[0] if cmd else "?", "ts": int(time.time()),
                                "in": count_tokens(raw), "out": count_tokens(filtered)}) + "\n")
    except OSError:
        pass


HANDLERS = {
    "git": h_git,
    "cargo": h_cargo,
    "gradle": h_rules("gradle"), "gradlew": h_rules("gradle"), "./gradlew": h_rules("gradle"),
    "pytest": h_rules("pytest"),
    "npm": h_rules("npm"), "pnpm": h_rules("npm"),
    "pip": h_rules("pip"), "pip3": h_rules("pip"),
    "ffmpeg": h_rules("ffmpeg"), "ffprobe": h_rules("ffmpeg"),
}
NATIVE = {"ls": h_ls}
GROUPING = {"find": h_find, "grep": h_grep, "rg": h_grep, "egrep": h_grep}


def dispatch(cmd):
    name = os.path.basename(cmd[0])
    if cmd[0].endswith("gradlew"):
        name = "gradlew"
    if name in NATIVE:
        filtered, code = NATIVE[name](cmd[1:])
    elif name in GROUPING:
        filtered, code = GROUPING[name](cmd)
    elif name in HANDLERS:
        filtered, code = HANDLERS[name](cmd)
    else:
        p = run_raw(cmd)
        filtered, code = generic_filter(p["out"], p["code"]), p["code"]
    save_raw(cmd, LAST["raw"] or filtered, code)
    track(cmd, LAST["raw"] or filtered, filtered)
    print(filtered)
    return code


# ── Claude Code hook ─────────────────────────────────────────────────
UNSAFE_RE = re.compile(r"[|;&<>`\n]|\$\(|>>|<<")
REWRITABLE = set(list(HANDLERS) + list(NATIVE) + list(GROUPING) + [
    "make", "go", "docker", "kubectl", "node", "python", "python3",
    "javac", "gcc", "clang", "tsc", "eslint", "ruff", "mypy", "ffmpeg",
    "pkg", "apt", "du", "df", "wc", "tree", "gh"])


def hook_claude():
    data = sys.stdin.read().strip()
    if not data:
        return 0
    try:
        v = json.loads(data)
    except ValueError:
        return 0
    cmd = (v.get("tool_input") or {}).get("command", "")
    if not cmd or UNSAFE_RE.search(cmd):
        return 0
    first = os.path.basename(cmd.split()[0])
    rewrite_all = os.environ.get("TOK_REWRITE_ALL") == "1"
    if first in ("tok", "rtk", "cd", "export", "source", "vim", "nano", "ssh"):
        return 0
    if not rewrite_all and first not in REWRITABLE and not cmd.split()[0].endswith("gradlew"):
        return 0
    ti = dict(v.get("tool_input") or {})
    ti["command"] = "tok " + cmd
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow",
        "permissionDecisionReason": "tok auto-rewrite",
        "updatedInput": ti}}))
    return 0


def init_hook(argv):
    path = None
    if "--settings" in argv:
        path = argv[argv.index("--settings") + 1]
    elif "-g" in argv:
        path = os.path.join(os.path.expanduser("~"), ".claude", "settings.json")
    else:
        path = os.path.join(os.getcwd(), ".claude", "settings.json")
    settings = {}
    if os.path.exists(path):
        with open(path) as f:
            try:
                settings = json.load(f)
            except ValueError:
                print("tok init: %s is not valid JSON, aborting" % path)
                return 1
    hooks = settings.setdefault("hooks", {})
    pre = hooks.setdefault("PreToolUse", [])
    entry = {"matcher": "Bash", "hooks": [{"type": "command", "command": "tok hook claude"}]}
    if any("tok hook" in json.dumps(e) for e in pre):
        print("tok hook already installed in %s" % path)
        return 0
    pre.append(entry)
    if os.path.dirname(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(settings, f, indent=2)
    print("tok hook installed → %s" % path)
    return 0


def gain():
    p = os.path.join(CACHE_DIR, "stats.jsonl")
    if not os.path.exists(p):
        print("no stats yet")
        return 0
    tin = tout = n = 0
    with open(p) as f:
        for ln in f:
            try:
                d = json.loads(ln)
                tin += d["in"]; tout += d["out"]; n += 1
            except (ValueError, KeyError):
                pass
    pct = 100.0 * (1 - tout / tin) if tin else 0.0
    print("tok: %d commands, %d → %d tokens (%.1f%% saved)" % (n, tin, tout, pct))
    return 0


def main():
    argv = sys.argv[1:]
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip())
        return 0
    if argv[0] == "--version":
        print("tok %s" % VERSION)
        return 0
    if argv[0] == "full":
        p = os.path.join(CACHE_DIR, "last.txt")
        if os.path.exists(p):
            sys.stdout.write(open(p).read())
        else:
            print("no cached output")
        return 0
    if argv[0] == "gain":
        return gain()
    if argv[0] == "hook":
        # claude and codex share a byte-identical PreToolUse contract
        # (tool_input.command in, hookSpecificOutput.updatedInput out), so the
        # Claude handler serves both. The richer agents (gemini/cursor/copilot)
        # live only in the Rust binary — this fallback covers the common case.
        return hook_claude()
    if argv[0] == "init":
        return init_hook(argv[1:])
    if argv[0] == "pipe":
        raw = sys.stdin.read()
        name = argv[1] if len(argv) > 1 else None
        if name in RULES:
            print(rules_filter(name, raw))
        else:
            print(generic_filter(raw))
        return 0
    if argv[0] == "proxy":
        p = run_raw(argv[1:])
        sys.stdout.write(p["out"])
        return p["code"]
    if argv[0] == "run":
        cmd = argv[1:]
        if cmd and cmd[0] == "--":
            cmd = cmd[1:]
        if not cmd:
            print("tok run: no command")
            return 2
        p = run_raw(cmd)
        save_raw(cmd, p["out"], p["code"])
        filtered = generic_filter(p["out"], p["code"])
        track(cmd, p["out"], filtered)
        print(filtered)
        return p["code"]
    return dispatch(argv)


if __name__ == "__main__":
    sys.exit(main())
