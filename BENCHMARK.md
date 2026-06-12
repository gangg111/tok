# 🏆 tok vs rtk — full benchmark

> **tok** — universal token-diet proxy (single-file Rust, zero crates + Python fallback) ·
> opponent: **rtk 0.42.2** (rtk-ai/rtk, 61k★) built from source, with its full set of TOML filters.
> Token metric = `count_tokens` from rtk's own test suite (whitespace split).
> Reproduction: `bench/bench.py`, `bench/bench2.py`, `bench/latency.py`, `bench/analyze_duel.py`.
> Date: 2026-06-12, device: Samsung Fold7 / Termux aarch64 + GitHub Actions (ubuntu/macos/windows).

## 🎯 Aspect matrix

<table>
  <tr>
    <th align="left">Aspect</th>
    <th align="center">rtk</th>
    <th align="center">tok</th>
    <th align="center">winner</th>
  </tr>
  <tr><td>Token reduction — 10 real-world commands</td><td align="center">27.8%</td><td align="center"><b>86.6%</b></td><td align="center">✅ tok 10/10</td></tr>
  <tr><td>Reduction on <b>rtk's own test fixtures</b></td><td align="center">54.9%</td><td align="center"><b>65.3%</b></td><td align="center">✅ tok 5/5</td></tr>
  <tr><td>Commands unknown to the tool (ffmpeg, pkg…)</td><td align="center">0% (raw)</td><td align="center"><b>76–93%</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Retention of key facts</td><td align="center">9/10</td><td align="center"><b>10/10</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Latency (4 paths, incl. the hook)</td><td align="center">—</td><td align="center"><b>1.2–5.1× faster</b></td><td align="center">✅ tok 4/4</td></tr>
  <tr><td>Subagent duel on live code</td><td align="center">—</td><td align="center"><b>all metrics</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Session dedup (repeated command)</td><td align="center">none</td><td align="center"><b>41 → 7 tokens</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Binary size</td><td align="center">7.0 MB</td><td align="center"><b>836 kB</b></td><td align="center">✅ tok (8.4×)</td></tr>
  <tr><td>Build from source</td><td align="center">2 m 04 s (cargo)</td><td align="center"><b>~2 s</b> (plain rustc)</td><td align="center">✅ tok (~60×)</td></tr>
  <tr><td>Official binary on Termux/Android (bionic)</td><td align="center">❌ exit 127</td><td align="center">✅ works</td><td align="center">✅ tok</td></tr>
  <tr><td>CI: build + tests on ubuntu / macos / windows</td><td align="center">—</td><td align="center"><b>6/6 jobs ✅</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Fallback without a compiler</td><td align="center">none</td><td align="center">✅ tok.py (Python 3.8+)</td><td align="center">✅ tok</td></tr>
  <tr><td>Full-output recovery</td><td align="center">tee (on error only)</td><td align="center"><b><code>tok full</code>, history of 20</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Peak memory (RSS)</td><td align="center">~11.9 MB</td><td align="center">~11.9 MB</td><td align="center">🤝 tie</td></tr>
</table>

## 📊 Benchmark 1 — 10 real-world commands

<table>
  <tr>
    <th align="left">Case</th>
    <th align="right">raw</th>
    <th align="right">rtk</th>
    <th align="right">rtk savings</th>
    <th align="right">tok</th>
    <th align="right">tok savings</th>
  </tr>
  <tr><td>ls (large directory)</td><td align="right">173</td><td align="right">34</td><td align="right">80.3%</td><td align="right"><b>9</b></td><td align="right"><b>94.8%</b></td></tr>
  <tr><td>find *.rs (entire src)</td><td align="right">106</td><td align="right">66</td><td align="right">37.7% ⚠️*</td><td align="right"><b>41</b></td><td align="right"><b>61.3%</b></td></tr>
  <tr><td>grep -rn (entire src)</td><td align="right">150</td><td align="right">79</td><td align="right">47.3%</td><td align="right"><b>49</b></td><td align="right"><b>67.3%</b></td></tr>
  <tr><td>git log -20</td><td align="right">1298</td><td align="right">585</td><td align="right">54.9%</td><td align="right"><b>164</b></td><td align="right"><b>87.4%</b></td></tr>
  <tr><td>git status (dirty repo)</td><td align="right">69</td><td align="right">12</td><td align="right">82.6%</td><td align="right"><b>11</b></td><td align="right"><b>84.1%</b></td></tr>
  <tr><td>git diff (dirty repo)</td><td align="right">98</td><td align="right">92</td><td align="right">6.1%</td><td align="right"><b>42</b></td><td align="right"><b>57.1%</b></td></tr>
  <tr><td>gradle assembleDebug (replay of a real build)</td><td align="right">186</td><td align="right">18</td><td align="right">90.3%</td><td align="right"><b>14</b></td><td align="right"><b>92.5%</b></td></tr>
  <tr><td>cargo build (2 errors)</td><td align="right">84</td><td align="right">62</td><td align="right">26.2%</td><td align="right"><b>55</b></td><td align="right"><b>34.5%</b></td></tr>
  <tr><td>ffmpeg encode <i>(unknown to rtk)</i></td><td align="right">220</td><td align="right">220</td><td align="right">0.0%</td><td align="right"><b>54</b></td><td align="right"><b>75.5%</b></td></tr>
  <tr><td>pkg list-installed <i>(unknown to rtk)</i></td><td align="right">1941</td><td align="right">1955</td><td align="right">−0.7%</td><td align="right"><b>139</b></td><td align="right"><b>92.8%</b></td></tr>
  <tr><td><b>TOTAL</b></td><td align="right"><b>4325</b></td><td align="right"><b>3123</b></td><td align="right"><b>27.8%</b></td><td align="right"><b>578</b></td><td align="right"><b>86.6%</b></td></tr>
</table>

<sub>\* rtk lost file names in this case (hook_cmd.rs, toml_filter.rs) — information loss.</sub>

## 🏟️ Benchmark 2 — rtk's home turf (replay of its own test fixtures)

<table>
  <tr>
    <th align="left">Fixture from the rtk repo</th>
    <th align="right">raw</th>
    <th align="right">rtk</th>
    <th align="right">rtk savings</th>
    <th align="right">tok</th>
    <th align="right">tok savings</th>
  </tr>
  <tr><td>gradle build FAILED</td><td align="right">114</td><td align="right">45</td><td align="right">60.5%</td><td align="right"><b>31</b></td><td align="right"><b>72.8%</b></td></tr>
  <tr><td>gradle test FAILED</td><td align="right">67</td><td align="right">48</td><td align="right">28.4%</td><td align="right"><b>38</b></td><td align="right"><b>43.3%</b></td></tr>
  <tr><td>mvn test FAILED</td><td align="right">195</td><td align="right">100</td><td align="right">48.7%</td><td align="right"><b>80</b></td><td align="right"><b>59.0%</b></td></tr>
  <tr><td>mvn install OK</td><td align="right">227</td><td align="right">57</td><td align="right">74.9%</td><td align="right"><b>18</b></td><td align="right"><b>92.1%</b></td></tr>
  <tr><td>glab CI trace (job failed)</td><td align="right">244</td><td align="right">132</td><td align="right">45.9%</td><td align="right"><b>127</b></td><td align="right"><b>48.0%</b></td></tr>
  <tr><td><b>TOTAL</b></td><td align="right"><b>847</b></td><td align="right"><b>382</b></td><td align="right"><b>54.9%</b></td><td align="right"><b>294</b></td><td align="right"><b>65.3%</b></td></tr>
</table>

## ⚡ Latency (mean of 30 runs after warm-up, Termux/aarch64)

<table>
  <tr>
    <th align="left">Path</th>
    <th align="right">rtk</th>
    <th align="right">tok</th>
    <th align="center">tok's edge</th>
  </tr>
  <tr><td>startup (<code>--version</code>)</td><td align="right">11.1 ms</td><td align="right"><b>9.2 ms</b></td><td align="center">1.2×</td></tr>
  <tr><td><code>git status</code> end-to-end</td><td align="right">41.5 ms</td><td align="right"><b>25.5 ms</b></td><td align="center">1.6×</td></tr>
  <tr><td><code>ls</code> (large directory)</td><td align="right">17.9 ms</td><td align="right"><b>10.0 ms</b></td><td align="center">1.8×</td></tr>
  <tr><td><b>PreToolUse hook</b> (every single Bash call!)</td><td align="right">51.0 ms</td><td align="right"><b>10.0 ms</b></td><td align="center"><b>5.1×</b></td></tr>
</table>

<sub>Reference: raw <code>git status</code> = 14.1 ms. tok startup on a PC (measured in CI): ubuntu 1 ms · macos 2 ms · windows 17 ms.</sub>

## 🤖 Subagent duel — the same task on live code

Two Claude subagents (same model), identical twin Rust repositories with 2 planted bugs,
identical list of steps. One agent ran every command through rtk, the other through tok.
Measured from the agents' transcripts:

<table>
  <tr>
    <th align="left">Metric (Bash tool results)</th>
    <th align="right">agent-rtk</th>
    <th align="right">agent-tok</th>
    <th align="center">tok's edge</th>
  </tr>
  <tr><td>result bytes</td><td align="right">2794</td><td align="right"><b>2504</b></td><td align="center">10.4%</td></tr>
  <tr><td>tokens (whitespace)</td><td align="right">357</td><td align="right"><b>314</b></td><td align="center">12.0%</td></tr>
  <tr><td>tokens (est. BPE)</td><td align="right">1122</td><td align="right"><b>1003</b></td><td align="center">10.6%</td></tr>
  <tr><td>total subagent tokens</td><td align="right">19,620</td><td align="right"><b>19,544</b></td><td align="center">✅</td></tr>
  <tr><td>task time</td><td align="right">97.2 s</td><td align="right"><b>81.5 s</b></td><td align="center">16%</td></tr>
  <tr><td>success (tests 4/4 + commit)</td><td align="center">✅</td><td align="center">✅</td><td align="center">🤝</td></tr>
</table>

## ♻️ Session dedup — a feature rtk doesn't have

<table>
  <tr><th align="left">Scenario (<code>find src -name "*.rs"</code>)</th><th align="right">rtk</th><th align="right">tok</th></tr>
  <tr><td>1st call</td><td align="right">66 tokens</td><td align="right">41 tokens</td></tr>
  <tr><td>2nd call (nothing changed)</td><td align="right">66 tokens</td><td align="right"><b>7 tokens</b> — "unchanged since last run"</td></tr>
  <tr><td>3rd call (1 file added)</td><td align="right">66 tokens</td><td align="right"><b>~10 tokens</b> — just the diff <code>+ new_file.rs</code></td></tr>
</table>

## ✅ Measurement fairness

- token metric identical to rtk's own tests; rtk's "no hook installed" banner subtracted from its results,
- rtk got its full set of TOML filters and the gradle invocation variant that works for it,
- filter benchmarks measured with `TOK_NO_DEDUP=1` (session dedup measured separately),
- the first round of the subagent duel was **invalidated** (unequal arenas due to a setup fault) and re-run on identical ones,
- the only tie: RSS memory (~12 MB for both — Android baseline); rtk has numerically more specialized filters (100+), yet loses even on its own fixtures.
