# Changelog

All notable changes to **tok** are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/); tok follows
[Semantic Versioning](https://semver.org/).

## [0.3.2] — 2026-06-21

### Fixed
- **Windows doubled-CR data loss.** `resolve_cr` stripped only one trailing `\r`,
  so a line ending in `\r\r\n` (emitted when a tool's `\r\n` passes through a
  layer that re-adds `\r` — common on Windows) collapsed to an empty final frame:
  the whole line was lost and short outputs came back as bare `ok`. It now strips
  **all** trailing CRs while still keeping the final frame of a genuine `\r`-redraw.
  Fixed in both `tok.rs` and `tok.py`.
- **Non-BMP characters dropped from rewritten commands.** The hand-rolled JSON
  parser (`parse_json_string`, Rust) decoded `\uXXXX` per code unit, so a non-BMP
  char sent as an ASCII-escaped UTF-16 **surrogate pair** (`🚀` → 🚀,
  emoji and other astral chars) was silently dropped — corrupting e.g.
  `git commit -m "🚀 release"`. Surrogate pairs are now combined. (Raw UTF-8 input
  was already fine; `tok.py` uses `json` and was unaffected.)

### Added
- **PowerShell hook coverage on Windows.** `tok init` now installs a PreToolUse
  matcher for the `PowerShell` tool in addition to `Bash`, so Claude Code on
  Windows (which drives commands through a PowerShell tool) routes them through
  tok too instead of bypassing it. `tok hook claude` was already tool-agnostic.

## [0.3.1] — 2026-06-14

### Fixed
- **Windows output loss (CRLF) — the big one.** `resolve_cr` treated the `\r` of
  a `\r\n` line ending as a progress-bar overwrite and discarded everything
  before it, so any command that prints CRLF (cmd.exe, PowerShell, `where`,
  `winget`, most native Windows tools) came back **empty → tok reported `ok`**,
  silently dropping real output. On Windows — where Codex drives commands through
  PowerShell — this made the agent lose context. `resolve_cr` is now CRLF-aware:
  a trailing `\r` is stripped as a line terminator, and only a `\r` with content
  after it (a redrawn progress bar) collapses to its final frame. Reproduced and
  fixed on 0.3.0; thanks to a contributor's Windows testing for the report.



Multi-agent integration, per-session savings, and safe rewriting of command chains.

### Added
- **OpenAI Codex hook** — `tok hook codex` covers the Codex CLI, desktop and
  IDE through one `~/.codex/config.toml` layer (Claude-style PreToolUse contract).
- **Google Antigravity hook** — `tok hook antigravity` (alias `agy`) covers the
  agy CLI and the desktop via `~/.gemini/config/hooks.json`; rewrites the command
  at `toolCall.args.CommandLine` through Antigravity's `overwrite` reply.
- **Per-session token savings** — `tok gain` now prints a `this session` line
  (keyed on the agent session id the hook records) above the `all-time` total, so
  an agent can answer "how many tokens did you save this session?" with a number.
- **Command-chain rewriting** — the hook now prefixes `tok ` to each safe segment
  of a top-level `&&`/`||`/`;` chain (e.g. `cd src && cargo build` →
  `cd src && tok cargo build`). A quote-aware scanner leaves `cd`/`ssh`/env-prefix
  segments raw and bails the whole command to raw on anything it can't rewrite
  without risk (pipes, redirects, `$()`, backticks, subshells, background `&`,
  unbalanced quotes).
- **`tok init` self-report instruction** — alongside the hook, init appends a short
  "when asked how many tokens you saved, run `tok gain`" note to `CLAUDE.md`,
  idempotently (a `<!-- tok:gain -->` marker, so re-running never duplicates it).
- **Per-agent install docs** — a "Hook setup per agent" table and snippets in the
  README (Claude Code, Codex, Antigravity, Gemini CLI, Copilot, Cursor).

### Changed
- **`tok init` installs globally by default** (`~/.claude`), since tok is a
  machine-wide tool; `--local` / `--project` scopes it to the current repo, and
  `-g` still works as an explicit global flag.
- Version aligned across the two implementations (the Python fallback was 0.1.0).

### Fixed
- **Antigravity overwrite** is applied only when paired with an explicit
  `{"decision":"allow"}` — a bare `overwrite` was silently ignored by agy 1.x
  (diagnosed with a live round-trip).
- **Chain rewriting never changes a command's behavior** — segments are
  reassembled verbatim with bare operators, so a backslash-escaped trailing space
  (`-m hi\ `) survives byte-for-byte and a malformed `;;` stays a syntax error
  instead of being "repaired" into two valid `;` (which could have activated a
  dead `;; sudo rm`). Both were caught by adversarial review before release.
- **tok.py** now emits UTF-8 on Windows `cp1250` consoles — `tok gain`'s `→` no
  longer raises `UnicodeEncodeError`.

## [0.2.0] — 2026-06-12

Initial public release: a single-file, **zero-crate** Rust proxy (with a Python
fallback) that filters command output before it reaches an LLM's context. Built
to beat [rtk](https://github.com/rtk-ai/rtk) and benchmarked against it —
see [BENCHMARK.md](BENCHMARK.md).

### Added
- Universal adaptive generic filter for ANY command: ANSI/progress strip,
  similar-line dedup with counts, error-preserving middle-out truncation,
  stack-frame collapse that keeps user frames.
- Specialized handlers: `ls` (native scandir), `find`/`grep` (grouping), `git`
  (status/log/diff/push…), `cargo`, `gradle`/`gradlew`, `maven`, `pytest`,
  `npm`/`pnpm`, `pip`, `ffmpeg`, GitLab CI traces.
- Session dedup: a repeated command with unchanged output returns
  `unchanged since last run`; a small change returns just the line diff.
- `tok full` (full raw output of the last 20 runs), `tok gain`, `tok discover`,
  `tok pipe`, `tok run`, `tok proxy`, `tok read`.
- PreToolUse rewrite hooks for Claude Code, Gemini CLI, GitHub Copilot
  (VS Code + CLI) and Cursor.
- Panic guard: a filter crash falls back to raw output — the user is never blocked.
- Python fallback (`tok.py`, 3.8+, zero dependencies) for hosts without a compiler.
- CI: build + tests on ubuntu / macos / windows.

### Changed
- `ls` on huge directories groups files by extension and caps the directory line
  (`C:\Windows\System32`: ~112 kB → ~1 kB).
- `cargo test` green runs collapse to a single rtk-style tally
  (`cargo test: N passed (M suites, T.TTs)`).
- Documentation translated to English; benchmark tables rendered as SVG charts;
  a Windows-PC benchmark added.
- CI actions bumped to Node 24 (`checkout@v5`, `upload-artifact@v7`).
