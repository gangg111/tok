#!/usr/bin/env python3
"""Generate the SVG benchmark charts embedded in README.md (assets/*.svg).

Pure stdlib, deterministic: same data in -> byte-identical SVG out.
Data below mirrors the tables in README.md / BENCHMARK.md.
"""
import os
import xml.etree.ElementTree as ET

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

BG, BORDER, GRID = "#0d1117", "#30363d", "#21262d"
FG, MUTED = "#e6edf3", "#8b949e"
C_TOK, C_RTK = "#3fb950", "#6e7681"
FONT = "'Segoe UI', Helvetica, Arial, sans-serif"

W, LEFT, RIGHT, TOP, ROW = 820, 260, 90, 66, 44
PLOT = W - LEFT - RIGHT


def bar_chart(name, title, subtitle, rows, vmax, ticks, tick_suffix, note=None):
    """rows: (label, rtk_value, rtk_text, tok_value, tok_text); bars clamp at 0."""
    n = len(rows)
    body_h = n * ROW
    h = TOP + body_h + 20 + (18 if note else 0) + 12
    s = []
    s.append(
        '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" '
        'viewBox="0 0 %d %d" role="img" aria-label="%s">' % (W, h, W, h, esc(title))
    )
    s.append(
        '<rect x="0.5" y="0.5" width="%d" height="%d" rx="10" fill="%s" stroke="%s"/>'
        % (W - 1, h - 1, BG, BORDER)
    )
    s.append(text(24, 32, esc(title), 17, FG, bold=True))
    s.append(text(24, 52, esc(subtitle), 12, MUTED))
    # legend (top-right)
    lx = W - 150
    s.append('<rect x="%d" y="22" width="11" height="11" rx="2" fill="%s"/>' % (lx, C_TOK))
    s.append(text(lx + 17, 32, "tok", 12, FG))
    s.append('<rect x="%d" y="22" width="11" height="11" rx="2" fill="%s"/>' % (lx + 60, C_RTK))
    s.append(text(lx + 77, 32, "rtk", 12, FG))
    # grid + tick labels
    for t in ticks:
        x = LEFT + PLOT * t / vmax
        s.append(
            '<line x1="%.1f" y1="%d" x2="%.1f" y2="%d" stroke="%s" stroke-width="1"/>'
            % (x, TOP - 4, x, TOP + body_h, GRID)
        )
        s.append(
            text(x, TOP + body_h + 14, "%g%s" % (t, tick_suffix), 10, MUTED, anchor="middle")
        )
    for i, (label, rv, rt, tv, tt) in enumerate(rows):
        y = TOP + i * ROW
        total = label.startswith("TOTAL")
        if total:
            s.append(
                '<line x1="16" y1="%d" x2="%d" y2="%d" stroke="%s" stroke-width="1"/>'
                % (y, W - 16, y, BORDER)
            )
        s.append(
            text(LEFT - 12, y + 26, esc(label), 13, FG if total else MUTED,
                 anchor="end", bold=total)
        )
        for off, v, txt, color in ((5, rv, rt, C_RTK), (22, tv, tt, C_TOK)):
            bw = max(PLOT * max(v, 0) / vmax, 2)
            s.append(
                '<rect x="%d" y="%d" width="%.1f" height="13" rx="3" fill="%s"/>'
                % (LEFT, y + off, bw, color)
            )
            s.append(
                text(LEFT + bw + 7, y + off + 11, esc(txt), 12,
                     FG if color == C_TOK else MUTED, bold=total and color == C_TOK)
            )
    if note:
        s.append(text(24, h - 14, esc(note), 11, MUTED))
    s.append("</svg>")
    svg = "\n".join(s)
    ET.fromstring(svg)  # well-formedness check
    path = os.path.join(OUT, name)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(svg)
    print("wrote %s (%d bytes)" % (path, len(svg)))


def text(x, y, content, size, fill, anchor="start", bold=False):
    return (
        '<text x="%s" y="%s" font-family="%s" font-size="%d" fill="%s"'
        '%s%s>%s</text>'
        % (
            "%.1f" % x, "%.1f" % y, FONT, size, fill,
            ' text-anchor="%s"' % anchor if anchor != "start" else "",
            ' font-weight="600"' if bold else "",
            content,
        )
    )


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def pct(rtk, tok):
    return rtk, "%g%%" % rtk, tok, "%g%%" % tok


def ms(rtk, tok):
    return rtk, "%g ms" % rtk, tok, "%g ms" % tok


os.makedirs(OUT, exist_ok=True)

bar_chart(
    "bench1.svg",
    "Token savings — 10 real-world commands",
    "share of output tokens removed (higher is better) · rows = phone (Termux) unless marked (PC) · whitespace metric",
    [
        ("ls (large directory)", *pct(80.3, 94.8)),
        ("find *.rs (entire src)", *pct(37.7, 61.3)),
        ("grep -rn (entire src)", *pct(47.3, 67.3)),
        ("git log -20", *pct(54.9, 87.4)),
        ("git status (dirty repo)", *pct(82.6, 84.1)),
        ("git diff (dirty repo)", *pct(6.1, 57.1)),
        ("gradle assembleDebug", *pct(90.3, 92.5)),
        ("cargo build (2 errors)", *pct(26.2, 34.5)),
        ("ffmpeg encode *", *pct(0.0, 75.5)),
        ("pkg list-installed *", -0.7, "-0.7%", 92.8, "92.8%"),
        ("ls C:\\Windows\\System32 (PC)", 0.0, "error", 99.9, "99.9%"),
        ("TOTAL — phone (Termux)", *pct(27.8, 86.6)),
    ],
    100, (25, 50, 75, 100), "%",
    note="* unknown to rtk — passed through raw · PC row: raw dir = 26,043 tokens, tok = 17; rtk fails without an ls binary on PATH",
)

bar_chart(
    "bench2.svg",
    "Token savings — rtk's own test fixtures",
    "end-to-end replay of fixtures from the rtk repo (gradle / maven / glab) · higher is better · phone (Termux)",
    [
        ("gradle build FAILED", *pct(60.5, 72.8)),
        ("gradle test FAILED", *pct(28.4, 43.3)),
        ("mvn test FAILED", *pct(48.7, 59.0)),
        ("mvn install OK", *pct(74.9, 92.1)),
        ("glab CI trace (failed)", *pct(45.9, 48.0)),
        ("TOTAL — phone (Termux)", *pct(54.9, 65.3)),
    ],
    100, (25, 50, 75, 100), "%",
    note="no PC rerun: the replay needs shell shims and on Windows neither rtk nor tok can spawn .bat shims (equal limitation)",
)

bar_chart(
    "latency.svg",
    "Latency — phone and PC",
    "mean of 30 runs after warm-up · lower is better · PC = Windows 11 x64",
    [
        ("startup — phone (Termux)", *ms(11.1, 9.2)),
        ("git status e2e — phone (Termux)", *ms(41.5, 25.5)),
        ("ls, large dir — phone (Termux)", *ms(17.9, 10.0)),
        ("PreToolUse hook — phone (Termux)", *ms(51.0, 10.0)),
        ("startup — PC", *ms(21.7, 10.8)),
        ("PreToolUse hook — PC", *ms(37.6, 13.8)),
    ],
    55, (10, 20, 30, 40, 50), " ms",
    note="the PreToolUse hook fires on every single Bash call — tok is 5.1x faster on the phone, 2.7x on the PC",
)

bar_chart(
    "latency-pc.svg",
    "Latency — Windows 11 x64 (PC)",
    "prebuilt rtk.exe 0.42.3 vs tok.exe built on the spot (rustc -O, 2.6 s) · mean of 30 runs · lower is better",
    [
        ("startup (--version)", *ms(21.7, 10.8)),
        ("PreToolUse hook", *ms(37.6, 13.8)),
    ],
    40, (10, 20, 30, 40), " ms",
    note="binary: tok 608 kB vs rtk 8.4 MB (14.2x) · ls C:\\Windows\\System32: tok 1.1 kB, rtk errors out (raw dir = 278 kB)",
)

bar_chart(
    "dedup.svg",
    "Session dedup — repeated command",
    "tokens emitted per call · phone scenario: find src *.rs · PC scenario: ls on a 30-file dir · rtk has no dedup",
    [
        ("1st call — phone (Termux)", 66, "66 tokens", 41, "41 tokens"),
        ("2nd, no changes — phone (Termux)", 66, "66", 7, '7 — "unchanged since last run"'),
        ("3rd, +1 file — phone (Termux)", 66, "66", 10, "~10 — one-line diff only"),
        ("1st call — PC", 62, "62 tokens", 14, "14 tokens"),
        ("2nd, no changes — PC", 62, "62", 8, '8 — "unchanged since last run"'),
        ("3rd, +1 file — PC", 62, "62", 14, "14 — full re-list"),
    ],
    70, (20, 40, 60), "",
    note="PC 3rd call falls back to the full (still compact) listing — re-wrapped ls lines no longer make a short diff",
)

bar_chart(
    "duel.svg",
    "Subagent duel — same task on live code",
    "two Claude subagents, twin repos with 2 planted bugs · bars normalized to the rtk agent (=100%) · lower is better",
    [
        ("result bytes — phone (Termux)", 100, "2794 B", 89.6, "2504 B"),
        ("result bytes — PC", 100, "1699 B", 111.3, "1891 B"),
        ("tokens, ws — phone (Termux)", 100, "357", 88.0, "314"),
        ("tokens, ws — PC", 100, "208", 106.3, "221"),
        ("tokens, BPE — phone (Termux)", 100, "1122", 89.4, "1003"),
        ("tokens, BPE — PC", 100, "642", 108.9, "699"),
        ("wall-time — phone (Termux)", 100, "97.2 s", 83.9, "81.5 s"),
        ("wall-time — PC", 100, "98.3 s", 115.5, "113.5 s"),
    ],
    120, (25, 50, 75, 100), "%",
    note="phone: tok won every metric · PC: rtk won every metric — tok's cargo tally pushed its agent into one extra verify run · all four lanes: 4/4 tests PASS + commit",
)
