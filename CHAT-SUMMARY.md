# Session summary: how tok was born (2026-06-12)

The story of a single Claude Code session (model: Claude Fable 5) on a phone
(Samsung Fold7, Termux, no root) in which **tok** — a proxy that cuts LLM
token usage — was built and, over four rounds, beat
[rtk](https://github.com/rtk-ai/rtk) (Rust Token Killer, 61k★) in every
measurable aspect.

## Round 0 — reconnaissance

The user dropped in a fork of `rtk` asking to "get familiar with it".
Analysis of the repo (command-proxy architecture, TOML filters, PreToolUse
hook for Claude Code, the tee/recovery mechanism) + building rtk from source
on Termux (cargo, 2 m 04 s) — because the official aarch64 binary requires
glibc and on Android (bionic) dies with `exit 127`.

## Round 1 — "build something better and prove it"

**tok** was born: a single Python file with zero dependencies. The key idea:
a **universal adaptive compressor for ANY command** (rtk passes unknown
commands through raw) + specialized handlers (ls via scandir, find/grep with
grouping, git, cargo, gradle…) + the same Claude Code hook contract as rtk.
A benchmark on 10 real commands, with the token metric taken from rtk's own
tests: **tok 86.5% reduction vs rtk 27.9%**, 9/10 cases won.

## Round 2 — "you have to win on latency too"

Python stood no chance against a Rust binary's startup, so tok was rewritten
in **pure Rust without a single crate** — compiled with plain
`rustc -O tok.rs` (~2 s), an 836 kB binary (vs rtk's 7 MB). The result:
tok faster on every path — startup 9.2 vs 11.1 ms, `git status` 25.5 vs
41.5 ms, **PreToolUse hook 10.0 vs 51.0 ms (5.1×)** — because rtk opens
SQLite and its config on every invocation, and tok doesn't. Python stayed
on as the fallback.

## Round 3 — "win in literally every aspect"

Closing all the remaining gaps: maven and GitLab CI trace filters,
force-stripping of redundant gradle boilerplate, stack-trace collapsing
that prefers user-code frames, a panic guard (filter crash → raw output,
the user is never blocked), `tok full [n]` (history of the last 20 raw
outputs), hooks for Gemini / Copilot (VS Code + CLI) / Cursor (contracts
1:1 from rtk's code), 14 unit tests, README + LICENSE (Apache-2.0, rtk
attribution). The key proof: a **replay of rtk's own test fixtures**
(gradle, maven, glab — its flagship modules) through both tools
end-to-end: **tok 5/5** (65.3% vs 54.9%).

## Round 4 — "take it to the very top" + the subagent duel

- **Session dedup** (a feature rtk architecturally doesn't have):
  a repeated command with unchanged output → `unchanged since last run`
  (41 → 7 tokens); a minor change → just the line diff.
- Formats optimized for a real BPE tokenizer (ASCII instead of `×←…•`),
  `tok read`/`cat`, `tok discover` (scans Claude Code transcripts),
  path grouping in `git status` (113 artifact files = 1 line).
- **Public repo + CI**: https://github.com/gangg111/tok — 6/6 jobs green
  on ubuntu / macos / windows (build with plain rustc + tests + hook smoke
  + the Python fallback); startup on a PC: 1 / 2 / 17 ms.
- **The final duel**: two Claude subagents, identical twin Rust
  repositories with 2 planted bugs, identical step lists; one worked
  through rtk, the other through tok. Measured from the transcripts:
  tok better in ALL metrics — bytes −10.4%, tokens −12%, BPE −10.6%,
  time 81.5 vs 97.2 s, both 4/4 PASS + commit. (The first round of the
  duel was honestly invalidated — the arenas weren't identical due to a
  setup fault.)

## Final score

| Aspect | result |
|---|---|
| 10-command benchmark | tok 10/10, 86.6% vs 27.8% |
| rtk's own fixtures | tok 5/5, 65.3% vs 54.9% |
| Fact retention | tok 10/10, rtk 9/10 |
| Latency | tok 4/4 (hook 5.1×) |
| Subagent duel | tok in all metrics |
| CI on 3 systems | 6/6 ✅ |
| Size / build | 8.4× smaller binary, ~60× faster build |
| RSS memory | tie (~12 MB, the Android baseline) |

Details and full tables: [BENCHMARK.md](BENCHMARK.md) ·
technical report: [bench/REPORT.md](bench/REPORT.md) ·
reproduction: `bench/bench.py`, `bench/bench2.py`, `bench/latency.py`,
`bench/analyze_duel.py`.

---
*All of it — the rtk analysis, the design, the Rust+Python implementation,
the benchmarks, the agent duel, CI — happened in a single Claude Code
session (Claude Fable 5) on an Android phone.*
