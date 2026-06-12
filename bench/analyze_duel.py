#!/data/data/com.termux/files/usr/bin/env python3
"""Analiza pojedynku subagentow: ruch wynikow Bash w transkryptach."""
import json, math, re, sys

WORD_RE = re.compile(r"\w+|[^\w\s]")

def est_bpe(s):
    """Przybliżenie tokenów BPE: słowa dzielone ~4 znaki/token, interpunkcja 1."""
    n = 0
    for w in WORD_RE.findall(s):
        n += max(1, math.ceil(len(w) / 4))
    return n

def analyze(path):
    bash_ids, edit_like = {}, set()
    bash_cmds = []
    res = {"bash_bytes": 0, "bash_ws": 0, "bash_bpe": 0, "bash_calls": 0,
           "other_bytes": 0, "first_ts": None, "last_ts": None}
    with open(path) as f:
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
            content = msg.get("content")
            if not isinstance(content, list):
                continue
            for blk in content:
                if not isinstance(blk, dict):
                    continue
                if blk.get("type") == "tool_use":
                    if blk.get("name") == "Bash":
                        bash_ids[blk["id"]] = True
                        bash_cmds.append((blk.get("input") or {}).get("command", ""))
                elif blk.get("type") == "tool_result":
                    c = blk.get("content")
                    text = ""
                    if isinstance(c, str):
                        text = c
                    elif isinstance(c, list):
                        text = "".join(x.get("text", "") for x in c if isinstance(x, dict))
                    if blk.get("tool_use_id") in bash_ids:
                        res["bash_bytes"] += len(text.encode())
                        res["bash_ws"] += len(text.split())
                        res["bash_bpe"] += est_bpe(text)
                        res["bash_calls"] += 1
                    else:
                        res["other_bytes"] += len(text.encode())
    return res, bash_cmds

base = "/data/data/com.termux/files/home/.claude/projects/-data-data-com-termux-files-home/6076b0e2-1d38-4dd6-be67-34607060e7b1/subagents"
duel = {"rtk": f"{base}/agent-af6618fc86322506e.jsonl",
        "tok": f"{base}/agent-a805f560db5d4c0b3.jsonl"}
out = {}
for name, path in duel.items():
    out[name], cmds = analyze(path)
    print(f"--- agent {name}: {out[name]['bash_calls']} wywolan Bash ---")
    for c in cmds:
        print("   ", c[:110])
r, t = out["rtk"], out["tok"]
print()
print("%-28s %12s %12s %10s" % ("METRYKA (wyniki Bash)", "agent-rtk", "agent-tok", "tok lepszy o"))
for key, label in [("bash_bytes", "bajty"), ("bash_ws", "tokeny (whitespace)"),
                   ("bash_bpe", "tokeny (est. BPE)"), ("bash_calls", "liczba wywolan")]:
    rv, tv = r[key], t[key]
    save = "%.1f%%" % (100 * (1 - tv / rv)) if rv else "-"
    print("%-28s %12d %12d %10s" % (label, rv, tv, save))
