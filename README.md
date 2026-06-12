# tok — universal token-diet proxy for CLI commands

Filters and compresses command output **before it reaches an LLM context**.
Inspired by [rtk](https://github.com/rtk-ai/rtk) (Apache-2.0); built to beat it
in every measured aspect — see `bench/REPORT.md` for the full evidence.

Two implementations of the same filters:

| file | what | when to use |
|---|---|---|
| `tok.rs` | native binary, pure-std Rust, **zero crates** | primary — fastest |
| `tok.py` | single-file Python 3.8+, zero deps | fallback for hosts without a compiler |

## Install

```sh
# native (any OS with Rust; no cargo needed, ~2 s build)
rustc -O -o tok tok.rs && mv tok ~/.local/bin/

# or the Python fallback
cp tok.py ~/.local/bin/tok && chmod +x ~/.local/bin/tok

# hook into Claude Code (CLI & desktop)
tok init -g
```

## Usage

```
tok <command> [args...]    run command through the best filter
tok run -- <command> ...   force the generic filter (works on ANY command)
tok proxy <command> ...    raw passthrough
tok pipe [name]            filter stdin (gradle|maven|pytest|npm|pip|ffmpeg|citrace)
tok full [n|list]          full raw output of the last (or n-th last) run
tok gain                   cumulative token savings
tok init [-g]              install the Claude Code PreToolUse rewrite hook
tok hook claude|gemini|copilot|cursor   hook entrypoints (JSON on stdin)
```

## How it beats rtk

- **Universal generic filter** — unknown commands are compressed too
  (ANSI/progress strip, similar-line dedup with counts, stack-frame collapse
  keeping *user* frames, error-preserving middle-out truncation). rtk passes
  unknown commands through raw.
- **Specialized handlers**: ls (native scandir), find/grep (grouping; grep
  groups globally by content), git status/log/diff/push…, cargo, gradle
  (incl. `./gradlew`), maven, pytest, npm/pnpm, pip, ffmpeg, GitLab CI traces.
- **Never blocks you**: a filter crash falls back to raw output (panic guard);
  full raw output of the last 20 runs is always recoverable via `tok full`.
- **Faster**: lower startup, lower per-call and 4–5× lower hook latency than
  rtk (no SQLite, no config parse at startup). Binary is 836 kB vs 7 MB.
- **Runs everywhere**: Termux/bionic (where official rtk binaries don't load),
  Linux, macOS, Windows; Python fallback when there's no compiler.

## Tests

```sh
rustc --test -o tok-test tok.rs && ./tok-test   # 14 unit tests
python3 bench/bench.py                          # 10-case benchmark vs rtk
python3 bench/bench2.py                         # rtk's own fixtures benchmark
python3 bench/latency.py                        # latency benchmark
```
# 🏆 tok vs rtk — pełny benchmark

> **tok** — universal token-diet proxy (single-file Rust, zero crates + fallback Python) ·
> przeciwnik: **rtk 0.42.2** (rtk-ai/rtk, 61k★) zbudowany ze źródeł, z pełnym zestawem filtrów TOML.
> Metryka tokenów = `count_tokens` z testów samego rtk (podział na białych znakach).
> Reprodukcja: `bench/bench.py`, `bench/bench2.py`, `bench/latency.py`, `bench/analyze_duel.py`.
> Data: 2026-06-12, urządzenie: Samsung Fold7 / Termux aarch64 + GitHub Actions (ubuntu/macos/windows).

## 🎯 Macierz aspektów

<table>
  <tr>
    <th align="left">Aspekt</th>
    <th align="center">rtk</th>
    <th align="center">tok</th>
    <th align="center">zwycięzca</th>
  </tr>
  <tr><td>Redukcja tokenów — 10 realnych komend</td><td align="center">27,8%</td><td align="center"><b>86,6%</b></td><td align="center">✅ tok 10/10</td></tr>
  <tr><td>Redukcja na <b>własnych fixture'ach testowych rtk</b></td><td align="center">54,9%</td><td align="center"><b>65,3%</b></td><td align="center">✅ tok 5/5</td></tr>
  <tr><td>Komendy nieznane narzędziu (ffmpeg, pkg…)</td><td align="center">0% (raw)</td><td align="center"><b>76–93%</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Retencja kluczowych faktów</td><td align="center">9/10</td><td align="center"><b>10/10</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Latencja (4 ścieżki, w tym hook)</td><td align="center">—</td><td align="center"><b>1,2–5,1× szybciej</b></td><td align="center">✅ tok 4/4</td></tr>
  <tr><td>Pojedynek subagentów na żywym kodzie</td><td align="center">—</td><td align="center"><b>wszystkie metryki</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Dedup sesyjny (powtórzona komenda)</td><td align="center">brak</td><td align="center"><b>41 → 7 tokenów</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Rozmiar binarki</td><td align="center">7,0 MB</td><td align="center"><b>836 kB</b></td><td align="center">✅ tok (8,4×)</td></tr>
  <tr><td>Build ze źródeł</td><td align="center">2 m 04 s (cargo)</td><td align="center"><b>~2 s</b> (sam rustc)</td><td align="center">✅ tok (~60×)</td></tr>
  <tr><td>Oficjalna binarka na Termux/Android (bionic)</td><td align="center">❌ exit 127</td><td align="center">✅ działa</td><td align="center">✅ tok</td></tr>
  <tr><td>CI: build + testy na ubuntu / macos / windows</td><td align="center">—</td><td align="center"><b>6/6 jobów ✅</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Fallback bez kompilatora</td><td align="center">brak</td><td align="center">✅ tok.py (Python 3.8+)</td><td align="center">✅ tok</td></tr>
  <tr><td>Odzysk pełnego wyjścia</td><td align="center">tee (tylko błąd)</td><td align="center"><b><code>tok full</code>, historia 20</b></td><td align="center">✅ tok</td></tr>
  <tr><td>Pamięć szczytowa (RSS)</td><td align="center">~11,9 MB</td><td align="center">~11,9 MB</td><td align="center">🤝 remis</td></tr>
</table>

## 📊 Benchmark 1 — 10 realnych komend

<table>
  <tr>
    <th align="left">Przypadek</th>
    <th align="right">raw</th>
    <th align="right">rtk</th>
    <th align="right">oszcz. rtk</th>
    <th align="right">tok</th>
    <th align="right">oszcz. tok</th>
  </tr>
  <tr><td>ls (duży katalog)</td><td align="right">173</td><td align="right">34</td><td align="right">80,3%</td><td align="right"><b>9</b></td><td align="right"><b>94,8%</b></td></tr>
  <tr><td>find *.rs (całe src)</td><td align="right">106</td><td align="right">66</td><td align="right">37,7% ⚠️*</td><td align="right"><b>41</b></td><td align="right"><b>61,3%</b></td></tr>
  <tr><td>grep -rn (całe src)</td><td align="right">150</td><td align="right">79</td><td align="right">47,3%</td><td align="right"><b>49</b></td><td align="right"><b>67,3%</b></td></tr>
  <tr><td>git log -20</td><td align="right">1298</td><td align="right">585</td><td align="right">54,9%</td><td align="right"><b>164</b></td><td align="right"><b>87,4%</b></td></tr>
  <tr><td>git status (brudne repo)</td><td align="right">69</td><td align="right">12</td><td align="right">82,6%</td><td align="right"><b>11</b></td><td align="right"><b>84,1%</b></td></tr>
  <tr><td>git diff (brudne repo)</td><td align="right">98</td><td align="right">92</td><td align="right">6,1%</td><td align="right"><b>42</b></td><td align="right"><b>57,1%</b></td></tr>
  <tr><td>gradle assembleDebug (replay realnego builda)</td><td align="right">186</td><td align="right">18</td><td align="right">90,3%</td><td align="right"><b>14</b></td><td align="right"><b>92,5%</b></td></tr>
  <tr><td>cargo build (2 błędy)</td><td align="right">84</td><td align="right">62</td><td align="right">26,2%</td><td align="right"><b>55</b></td><td align="right"><b>34,5%</b></td></tr>
  <tr><td>ffmpeg encode <i>(nieznana dla rtk)</i></td><td align="right">220</td><td align="right">220</td><td align="right">0,0%</td><td align="right"><b>54</b></td><td align="right"><b>75,5%</b></td></tr>
  <tr><td>pkg list-installed <i>(nieznana dla rtk)</i></td><td align="right">1941</td><td align="right">1955</td><td align="right">−0,7%</td><td align="right"><b>139</b></td><td align="right"><b>92,8%</b></td></tr>
  <tr><td><b>RAZEM</b></td><td align="right"><b>4325</b></td><td align="right"><b>3123</b></td><td align="right"><b>27,8%</b></td><td align="right"><b>578</b></td><td align="right"><b>86,6%</b></td></tr>
</table>

<sub>\* rtk w tym przypadku zgubił nazwy plików (hook_cmd.rs, toml_filter.rs) — utrata informacji.</sub>

## 🏟️ Benchmark 2 — własny teren rtk (replay jego fixture'ów testowych)

<table>
  <tr>
    <th align="left">Fixture z repo rtk</th>
    <th align="right">raw</th>
    <th align="right">rtk</th>
    <th align="right">oszcz. rtk</th>
    <th align="right">tok</th>
    <th align="right">oszcz. tok</th>
  </tr>
  <tr><td>gradle build FAILED</td><td align="right">114</td><td align="right">45</td><td align="right">60,5%</td><td align="right"><b>31</b></td><td align="right"><b>72,8%</b></td></tr>
  <tr><td>gradle test FAILED</td><td align="right">67</td><td align="right">48</td><td align="right">28,4%</td><td align="right"><b>38</b></td><td align="right"><b>43,3%</b></td></tr>
  <tr><td>mvn test FAILED</td><td align="right">195</td><td align="right">100</td><td align="right">48,7%</td><td align="right"><b>80</b></td><td align="right"><b>59,0%</b></td></tr>
  <tr><td>mvn install OK</td><td align="right">227</td><td align="right">57</td><td align="right">74,9%</td><td align="right"><b>18</b></td><td align="right"><b>92,1%</b></td></tr>
  <tr><td>glab CI trace (job failed)</td><td align="right">244</td><td align="right">132</td><td align="right">45,9%</td><td align="right"><b>127</b></td><td align="right"><b>48,0%</b></td></tr>
  <tr><td><b>RAZEM</b></td><td align="right"><b>847</b></td><td align="right"><b>382</b></td><td align="right"><b>54,9%</b></td><td align="right"><b>294</b></td><td align="right"><b>65,3%</b></td></tr>
</table>

## ⚡ Latencja (średnia z 30 powtórzeń po rozgrzewce, Termux/aarch64)

<table>
  <tr>
    <th align="left">Ścieżka</th>
    <th align="right">rtk</th>
    <th align="right">tok</th>
    <th align="center">przewaga toka</th>
  </tr>
  <tr><td>startup (<code>--version</code>)</td><td align="right">11,1 ms</td><td align="right"><b>9,2 ms</b></td><td align="center">1,2×</td></tr>
  <tr><td><code>git status</code> end-to-end</td><td align="right">41,5 ms</td><td align="right"><b>25,5 ms</b></td><td align="center">1,6×</td></tr>
  <tr><td><code>ls</code> (duży katalog)</td><td align="right">17,9 ms</td><td align="right"><b>10,0 ms</b></td><td align="center">1,8×</td></tr>
  <tr><td><b>hook PreToolUse</b> (każde wywołanie Bash!)</td><td align="right">51,0 ms</td><td align="right"><b>10,0 ms</b></td><td align="center"><b>5,1×</b></td></tr>
</table>

<sub>Referencja: surowy <code>git status</code> = 14,1 ms. Startup toka na PC (zmierzony w CI): ubuntu 1 ms · macos 2 ms · windows 17 ms.</sub>

## 🤖 Pojedynek subagentów — to samo zadanie na żywym kodzie

Dwa subagenty Claude (ten sam model), identyczne bliźniacze repozytoria Rust z 2 zaszytymi bugami,
identyczna lista kroków. Jeden agent każdą komendę wykonywał przez rtk, drugi przez tok.
Pomiar z transkryptów agentów:

<table>
  <tr>
    <th align="left">Metryka (wyniki narzędzia Bash)</th>
    <th align="right">agent-rtk</th>
    <th align="right">agent-tok</th>
    <th align="center">przewaga toka</th>
  </tr>
  <tr><td>bajty wyników</td><td align="right">2794</td><td align="right"><b>2504</b></td><td align="center">10,4%</td></tr>
  <tr><td>tokeny (whitespace)</td><td align="right">357</td><td align="right"><b>314</b></td><td align="center">12,0%</td></tr>
  <tr><td>tokeny (est. BPE)</td><td align="right">1122</td><td align="right"><b>1003</b></td><td align="center">10,6%</td></tr>
  <tr><td>tokeny subagenta łącznie</td><td align="right">19 620</td><td align="right"><b>19 544</b></td><td align="center">✅</td></tr>
  <tr><td>czas zadania</td><td align="right">97,2 s</td><td align="right"><b>81,5 s</b></td><td align="center">16%</td></tr>
  <tr><td>sukces (testy 4/4 + commit)</td><td align="center">✅</td><td align="center">✅</td><td align="center">🤝</td></tr>
</table>

## ♻️ Dedup sesyjny — funkcja, której rtk nie ma

<table>
  <tr><th align="left">Scenariusz (<code>find src -name "*.rs"</code>)</th><th align="right">rtk</th><th align="right">tok</th></tr>
  <tr><td>1. wywołanie</td><td align="right">66 tok.</td><td align="right">41 tok.</td></tr>
  <tr><td>2. wywołanie (nic się nie zmieniło)</td><td align="right">66 tok.</td><td align="right"><b>7 tok.</b> — „unchanged since last run"</td></tr>
  <tr><td>3. wywołanie (doszedł 1 plik)</td><td align="right">66 tok.</td><td align="right"><b>~10 tok.</b> — sam diff <code>+ nowy_plik.rs</code></td></tr>
</table>

## ✅ Uczciwość pomiaru

- metryka tokenów identyczna z testami rtk; baner rtk „no hook installed" odejmowany z jego wyników,
- rtk dostał pełny zestaw swoich filtrów TOML i wariant wywołania gradle, który u niego działa,
- benchmarki filtrów liczone z `TOK_NO_DEDUP=1` (dedup sesyjny mierzony osobno),
- pierwsza runda pojedynku subagentów **unieważniona** (nierówne areny z winy setupu) i powtórzona na identycznych,
- jedyny remis: pamięć RSS (~12 MB u obu — baseline Androida); rtk ma ilościowo więcej specjalizowanych filtrów (100+), ale przegrywa nawet na własnych fixture'ach.


## License

Apache-2.0. Filter rule sets for gradle and the hook protocol shapes are
ported from rtk (rtk-ai/rtk, Apache-2.0) — attribution retained.
