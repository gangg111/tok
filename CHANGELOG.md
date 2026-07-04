# Changelog

All notable changes to **tok** are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/); tok follows
[Semantic Versioning](https://semver.org/).

## [0.3.5] — 2026-07-04

### Fixed
- **Hook rewrote a NESTED `command`, not the real one.** The hand-rolled JSON
  splice located the command field with a plain substring search for
  `"command"`, so a `tool_input` that nested an object with its own `command`
  key — `{"opts":{"command":"git log"},"command":"git status"}` — had the
  *nested* value rewritten (→ `"opts":{"command":"tok git log"}`) while the
  real top-level command was left un-proxied. The finder is now depth- and
  string-aware: `command` is matched only as a **top-level** key of
  `tool_input`, and the Antigravity path (`toolCall.args.CommandLine`, one
  level down) matches the first genuine *key* rather than any substring — a
  sibling whose string *value* happens to be the word `command` can no longer
  win. Found by an adversarial hook-envelope fuzz (13 crafted cases), proven
  live before/after. (`tok.py` parses with `json`, so it was never affected.)

## [0.3.4] — 2026-07-04

Heavy-load audit: a 36-check battery of adversarial real-command tests plus a
3-dimension code review. Every finding below was reproduced live before the
fix and re-verified after it.

### Fixed
- **Windows `.cmd`/`.bat` shims failed with exit 127.** Rust's `Command`
  resolves only `.exe` on PATH, so `tok npm --version` died with
  "npm: command not found" while npm worked natively — the hook was turning
  WORKING npm/npx/tsc/eslint commands into failures. `run_raw` now probes PATH
  for `.cmd`/`.bat` on NotFound and respawns. Other spawn errors (permission
  denied, bad exe format) now report `spawn failed: …` with exit 126 instead of
  the misleading "command not found". (`tok.py`: `shutil.which` probe + broader
  `OSError` handling.)
- **Token bombs: line LENGTH was never capped.** A single 10 MB minified/base64
  line sailed through `cat`, the generic filter, grep's unparsed lines and the
  dedup diff untouched (measured: 10,485,847 bytes reaching the agent). All
  line-emitting paths now cap at `TOK_MAX_LINE_CHARS` (default 2000) with a
  `...(+N chars) [tok full]` marker: `generic_filter`, `h_read`, `h_grep` misc,
  `render_diff`. 10 MB line → 2,112 bytes, fully recoverable via `tok full`.
- **`strip_ansi` swallowed ALL output after an OSC sequence terminated by ST
  (`ESC \`)** — hyperlinks (OSC 8), window titles, CI logs. Everything after
  the sequence (ERROR lines included) silently vanished. OSC now terminates on
  BEL *or* ST, and both OSC/CSI scans are capped so malformed input can't eat
  the stream. (`tok.py`: `ANSI_RE` updated.)
- **`tok cat` hard-failed on non-UTF-8 files.** PowerShell 5.1 `>`/`Out-File`
  write UTF-16LE, so agents lost the ability to read such logs entirely
  ("stream did not contain valid UTF-8"). `h_read` now decodes UTF-16 by BOM,
  BOM-less UTF-16LE by NUL-density (checked BEFORE the UTF-8 attempt — ASCII
  UTF-16LE is *valid* UTF-8 full of NULs), and falls back to lossy UTF-8.
- **`tok full` after `tok cat` returned the filtered view, not the file** —
  the `[tok full]` markers lied. `h_read` now publishes the full decoded
  content as the run's raw.
- **Unbounded capture → alloc-abort with total output loss.** A multi-GB
  output drove tok past several GB RSS (the panic guard cannot catch an
  allocation abort). Capture is now head+tail capped at `TOK_MAX_RAW_MB`
  (default 64) with an omission marker; early errors and final summaries both
  survive. One redundant full-size clone in dispatch removed.
- **`state/` dedup dir grew without bound** (full raw per key, never pruned —
  one 5 MB file per unique command, forever). Raw >4 MB now stores a small
  hash sentinel (38 B, "unchanged" detection still works); the dir is pruned
  to 200 newest entries.
- **Dedup claimed "unchanged since last run" across agent sessions** — a fresh
  session's first command could be told its (never-seen) output was unchanged
  from yesterday. The key now mixes the agent session id, entries older than
  4 h are ignored, and NUL-joined argv stops `grep "a b"`/`grep a b` aliasing.
- **grep/rg on Windows paths never parsed** (`C:\x\y.rs:12:…` — the drive colon
  was taken as the separator), dumping whole result sets into an unparsed
  bucket that was silently truncated at 10 lines. Drive prefixes are now
  skipped, unparsed lines are length-capped, and truncation prints
  `...+N more lines [tok full]`.
- **`ls` by-extension summary exploded on rotated logs** (`app.log.1…N` minted
  one "extension" per file: 3000 files → 20 KB). Numeric suffixes are stripped
  (grouped under the true extension) and the group list is capped at 30 with a
  `+N more exts` tail. 3000 rotated logs → 2 lines.
- **Cache races:** `last.txt`/ring/state/stats now written via tmp+rename
  (a concurrent `tok full` can no longer read a half-written file), ring
  filenames carry the pid (same-millisecond runs no longer overwrite each
  other), stats trim is atomic and lazier (512 KB). (`tok.py`: stats trim
  ported; `tok full <n>` now says the fallback has no ring instead of silently
  printing the wrong run.)
- **Late output of detached grandchildren was dropped and unrecoverable** —
  a worker backgrounded by its wrapper that printed an error 0.5 s after the
  wrapper exited lost that error everywhere. The post-exit quiet window is now
  1 s (hard cap unchanged), so such last words land in the capture.

## [0.3.3] — 2026-06-21

### Fixed
- **Hang on commands that spawn a persistent daemon (Gradle).** `run_raw` used
  `Command::output()`, which reads stdout/stderr to EOF. A command whose
  grandchild keeps a pipe write-end open — the **Gradle daemon** is the classic
  case (`tok gradlew … ` in the default daemon mode) — never reaches EOF, so tok
  hung **forever**. Two-part fix, verified against a real Gradle 9.3.1 daemon
  build and a synthetic repro:
  - drain stdout/stderr on threads, wait on the *direct* child, then take the
    captured output once it goes quiet (hard-capped) — so an orphaned daemon
    can't block tok; output and exit code are preserved in full.
  - on Windows, flip tok's own stdout/stderr to **non-inheritable** across the
    spawn (`SetHandleInformation`), so the grandchild daemon can't inherit the
    pipe our parent (the agent) reads and hold it open after we exit — which had
    hung the *calling* agent too. (`tok.py`: `Popen` + drain thread; `close_fds`
    already covers the inheritance side.)

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
