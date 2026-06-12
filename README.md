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

## License

Apache-2.0. Filter rule sets for gradle and the hook protocol shapes are
ported from rtk (rtk-ai/rtk, Apache-2.0) — attribution retained.
