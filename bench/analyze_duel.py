#!/usr/bin/env python3
"""Analiza pojedynku subagentow: ruch wynikow shellowych w transkryptach.

Uzycie:
  analyze_duel.py                      # domyslne sciezki z sesji na telefonie
  analyze_duel.py rtk.jsonl tok.jsonl  # dowolne transkrypty (np. pojedynek na PC)

Liczy wyniki narzedzi Bash i PowerShell (na PC pojedynek szedl PowerShellem,
bo globalny hook PreToolUse na Bash przepisywalby komendy przez rtk obu agentom).
"""
import json, math, re, sys
from datetime import datetime

WORD_RE = re.compile(r"\w+|[^\w\s]")
SHELL_TOOLS = {"Bash", "PowerShell"}

def est_bpe(s):
    """Przybliżenie tokenów BPE: słowa dzielone ~4 znaki/token, interpunkcja 1."""
    n = 0
    for w in WORD_RE.findall(s):
        n += max(1, math.ceil(len(w) / 4))
    return n

def parse_ts(ts):
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return None

def analyze(path):
    shell_ids = {}
    shell_cmds = []
    res = {"bash_bytes": 0, "bash_ws": 0, "bash_bpe": 0, "bash_calls": 0,
           "other_bytes": 0, "out_tokens": 0, "first_ts": None, "last_ts": None}
    with open(path, encoding="utf-8") as f:
        for line in f:
            try:
                d = json.loads(line)
            except ValueError:
                continue
            ts = d.get("timestamp")
            if ts:
                res["first_ts"] = res["first_ts"] or ts
                res["last_ts"] = ts
            msg = d.get("message") or {}
            usage = msg.get("usage") or {}
            res["out_tokens"] += usage.get("output_tokens") or 0
            content = msg.get("content")
            if not isinstance(content, list):
                continue
            for blk in content:
                if not isinstance(blk, dict):
                    continue
                if blk.get("type") == "tool_use":
                    if blk.get("name") in SHELL_TOOLS:
                        shell_ids[blk["id"]] = True
                        shell_cmds.append((blk.get("input") or {}).get("command", ""))
                elif blk.get("type") == "tool_result":
                    c = blk.get("content")
                    text = ""
                    if isinstance(c, str):
                        text = c
                    elif isinstance(c, list):
                        text = "".join(x.get("text", "") for x in c if isinstance(x, dict))
                    if blk.get("tool_use_id") in shell_ids:
                        res["bash_bytes"] += len(text.encode())
                        res["bash_ws"] += len(text.split())
                        res["bash_bpe"] += est_bpe(text)
                        res["bash_calls"] += 1
                    else:
                        res["other_bytes"] += len(text.encode())
    t0, t1 = parse_ts(res["first_ts"] or ""), parse_ts(res["last_ts"] or "")
    res["seconds"] = (t1 - t0).total_seconds() if t0 and t1 else 0
    return res, shell_cmds

if len(sys.argv) == 3:
    duel = {"rtk": sys.argv[1], "tok": sys.argv[2]}
else:
    base = "/data/data/com.termux/files/home/.claude/projects/-data-data-com-termux-files-home/6076b0e2-1d38-4dd6-be67-34607060e7b1/subagents"
    duel = {"rtk": f"{base}/agent-af6618fc86322506e.jsonl",
            "tok": f"{base}/agent-a805f560db5d4c0b3.jsonl"}
out = {}
for name, path in duel.items():
    out[name], cmds = analyze(path)
    print(f"--- agent {name}: {out[name]['bash_calls']} wywolan shell ---")
    for c in cmds:
        print("   ", c[:110])
r, t = out["rtk"], out["tok"]
print()
print("%-28s %12s %12s %10s" % ("METRYKA (wyniki shell)", "agent-rtk", "agent-tok", "tok lepszy o"))
for key, label in [("bash_bytes", "bajty"), ("bash_ws", "tokeny (whitespace)"),
                   ("bash_bpe", "tokeny (est. BPE)"), ("bash_calls", "liczba wywolan"),
                   ("out_tokens", "tokeny wyjsciowe agenta"), ("seconds", "czas zadania [s]")]:
    rv, tv = r[key], t[key]
    save = "%.1f%%" % (100 * (1 - tv / rv)) if rv else "-"
    print("%-28s %12.1f %12.1f %10s" % (label, rv, tv, save))
