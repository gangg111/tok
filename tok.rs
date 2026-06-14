// tok — universal token-diet proxy for CLI commands. Pure-std Rust port of tok.py.
// Build anywhere: rustc -O -o tok tok.rs   (no cargo, no crates, no glibc assumptions)
//
// Same filters as tok.py; native binary exists for one reason: startup latency.
// No SQLite, no config parsing, no telemetry at startup — unlike rtk.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Raw output of the most recent child command — used by the panic guard:
/// if a filter ever crashes, the user still gets the full raw output
/// (never block the user; same philosophy as rtk's fallback pattern).
static LAST_RAW: Mutex<String> = Mutex::new(String::new());
static LAST_CODE: Mutex<i32> = Mutex::new(0);

const VERSION: &str = "0.3.0";
const HEAD_KEEP: usize = 12;
const TAIL_KEEP: usize = 18;
const LS_GROUP_MIN: usize = 200;
const LS_DIR_KEEP: usize = 40;

fn home() -> String {
    env::var("HOME").or_else(|_| env::var("USERPROFILE")).unwrap_or_else(|_| ".".into())
}

fn cache_dir() -> String {
    env::var("TOK_CACHE").unwrap_or_else(|_| format!("{}/.cache/tok", home()))
}

fn max_lines_default() -> usize {
    env::var("TOK_MAX_LINES").ok().and_then(|v| v.parse().ok()).unwrap_or(60)
}

// ── text helpers ─────────────────────────────────────────────────────

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            match it.peek() {
                Some('[') => {
                    it.next();
                    while let Some(&d) = it.peek() {
                        it.next();
                        if d.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    it.next();
                    for d in it.by_ref() {
                        if d == '\u{7}' {
                            break;
                        }
                    }
                }
                Some('(') | Some(')') => {
                    it.next();
                    it.next();
                }
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn resolve_cr(s: &str) -> String {
    s.split('\n')
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

fn contains_word(hay_lower: &str, word: &str) -> bool {
    let hb = hay_lower.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay_lower[start..].find(word) {
        let p = start + pos;
        let before_ok = p == 0 || {
            let b = hb[p - 1] as char;
            !(b.is_ascii_alphanumeric() || b == '_')
        };
        let after = p + word.len();
        let after_ok = after >= hb.len() || {
            let a = hb[after] as char;
            !(a.is_ascii_alphanumeric() || a == '_')
        };
        if before_ok && after_ok {
            return true;
        }
        start = p + 1;
    }
    false
}

const IMP_WORDS: &[&str] = &[
    "error", "failed", "failure", "fatal", "panic", "exception", "traceback", "abort",
    "denied", "refused", "cannot", "unable", "missing", "not found", "undefined",
    "unresolved", "conflict", "rejected", "timeout", "segfault", "killed",
];

fn is_important(line: &str) -> bool {
    if line.starts_with("E ") || line.contains("error[") {
        return true;
    }
    if line.trim_start().starts_with('^') {
        return true;
    }
    let low = line.to_lowercase();
    IMP_WORDS.iter().any(|w| contains_word(&low, w))
}

fn has_progress_fraction(lt: &str) -> bool {
    // digits '/' digits anywhere
    let b = lt.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'/' && i > 0 && i + 1 < b.len()
            && b[i - 1].is_ascii_digit() && b[i + 1].is_ascii_digit() {
            return true;
        }
    }
    false
}

fn is_noise(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if t.len() > 1 && t.ends_with('%')
        && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit() || c == '.') {
        return true;
    }
    if t.len() >= 5 && t.starts_with('[') && t.ends_with(']')
        && t[1..t.len() - 1].chars().all(|c| matches!(c, '=' | '-' | '>' | '#' | '.' | ' ')) {
        return true;
    }
    if t.chars().all(|c| c == '.' || ('\u{2807}'..='\u{28ff}').contains(&c)) {
        return true;
    }
    if let Some(c) = t.chars().next() {
        if ('\u{2800}'..='\u{28ff}').contains(&c) {
            return true;
        }
    }
    let lt = t.to_lowercase();
    const STARTS: &[&str] = &[
        "downloading", "resolving", "fetching", "receiving", "unpacking", "extracting",
        "compressing", "counting", "enumerating", "progress", "building wheel", "collecting",
    ];
    if STARTS.iter().any(|s| lt.starts_with(s))
        && (lt.contains('%') || lt.contains("kb") || lt.contains("mb") || lt.contains("kib")
            || lt.contains("mib") || lt.contains("eta") || lt.contains("objects")
            || has_progress_fraction(&lt)) {
        return true;
    }
    if let Some(r) = lt.strip_prefix("remote:") {
        let r = r.trim_start();
        if ["counting", "compressing", "total", "enumerating"].iter().any(|s| r.starts_with(s)) {
            return true;
        }
    }
    // CI runner section markers (GitLab etc.)
    if lt.starts_with("section_start:") || lt.starts_with("section_end:") {
        return true;
    }
    false
}

fn is_framework_frame(line: &str) -> bool {
    const FW: &[&str] = &[
        "at org.junit", "at java.", "at jdk.", "at sun.", "at kotlin",
        "at org.gradle", "at org.opentest4j", "at org.apache.maven",
        "at org.jetbrains", "at scala.", "at android.", "at com.sun.",
    ];
    let t = line.trim_start();
    FW.iter().any(|p| t.starts_with(p))
}

/// Collapse long runs of stack frames ("\tat pkg.Class.m(File.java:12)"):
/// keep up to 2 non-framework frames (the user's own code — the part that
/// matters), or the first frame if all are framework noise.
fn collapse_frames(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    let flush = |run: &mut Vec<&str>, out: &mut Vec<String>| {
        if run.len() <= 2 {
            out.extend(run.iter().map(|s| s.to_string()));
        } else {
            let interesting: Vec<&&str> =
                run.iter().filter(|l| !is_framework_frame(l)).collect();
            let keep: Vec<String> = if interesting.is_empty() {
                vec![run[0].to_string()]
            } else {
                interesting.iter().take(2).map(|s| s.to_string()).collect()
            };
            let dropped = run.len() - keep.len();
            out.extend(keep);
            if dropped > 0 {
                out.push(format!("\t... +{} frames", dropped));
            }
        }
        run.clear();
    };
    for ln in text.split('\n') {
        let t = ln.trim_start();
        if t.starts_with("at ") && t.contains('(') {
            run.push(ln);
        } else {
            flush(&mut run, &mut out);
            out.push(ln.to_string());
        }
    }
    flush(&mut run, &mut out);
    out.join("\n")
}

fn norm_line(line: &str) -> String {
    let t = line.trim();
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_hexdigit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_hexdigit() {
                j += 1;
            }
            let boundary = (i == 0 || !chars[i - 1].is_alphanumeric())
                && (j >= chars.len() || !chars[j].is_alphanumeric());
            if j - i >= 7 && boundary {
                out.push('H');
                i = j;
                continue;
            }
            if c.is_ascii_digit() {
                let mut k = i;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || matches!(chars[k], '.' | ',' | ':' | '%' | '/')) {
                    k += 1;
                }
                out.push('N');
                i = k;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn human(n: u64) -> String {
    let mut f = n as f64;
    for u in ["B", "K", "M", "G"] {
        if f < 1024.0 || u == "G" {
            if u == "B" {
                return format!("{}B", n);
            }
            return format!("{:.1}{}", f, u).replace(".0", "");
        }
        f /= 1024.0;
    }
    String::from("?")
}

// ── generic adaptive filter ──────────────────────────────────────────

fn generic_filter(raw: &str, code: i32, max_lines: usize) -> String {
    let text = collapse_frames(&resolve_cr(&strip_ansi(raw)));
    let mut kept: Vec<(String, usize)> = Vec::new();
    let mut blank = false;
    for ln in text.split('\n') {
        let s = ln.trim_end();
        if s.trim().is_empty() {
            blank = true;
            continue;
        }
        if is_noise(s) && !is_important(s) {
            continue;
        }
        if let Some(last) = kept.last_mut() {
            if !last.0.is_empty() && norm_line(&last.0) == norm_line(s) {
                last.1 += 1;
                continue;
            }
        }
        if blank && !kept.is_empty() {
            kept.push((String::new(), 1));
        }
        blank = false;
        kept.push((s.to_string(), 1));
    }
    let mut out_lines: Vec<String> = kept
        .into_iter()
        .map(|(l, n)| {
            if l.is_empty() {
                String::new()
            } else if n > 1 {
                format!("{}  (x{})", l, n)
            } else {
                l
            }
        })
        .collect();

    if out_lines.len() > max_lines && out_lines.len() > HEAD_KEEP + TAIL_KEEP {
        let head: Vec<String> = out_lines[..HEAD_KEEP].to_vec();
        let tail: Vec<String> = out_lines[out_lines.len() - TAIL_KEEP..].to_vec();
        let middle = &out_lines[HEAD_KEEP..out_lines.len() - TAIL_KEEP];
        let mut important: Vec<String> =
            middle.iter().filter(|l| is_important(l)).cloned().collect();
        let omitted = middle.len() - important.len();
        if important.len() > max_lines {
            let extra = important.len() - max_lines;
            important.truncate(max_lines);
            important.push(format!("... +{} more error/warning lines [tok full]", extra));
        }
        let mut v = head;
        v.extend(important);
        if omitted > 0 {
            v.push(format!("... {} lines omitted [tok full]", omitted));
        }
        v.extend(tail);
        out_lines = v;
    }
    let result = out_lines.join("\n").trim_matches('\n').to_string();
    if result.is_empty() {
        return if code == 0 {
            "ok".into()
        } else {
            format!("exit {} (no output) [tok full]", code)
        };
    }
    if code != 0 {
        return format!("{}\nexit {} [tok full]", result, code);
    }
    result
}

// ── named rule filters (ported from tok.py / rtk TOML DSL) ───────────

/// Lines stripped even when they look "important" — they duplicate facts
/// that other kept lines already carry (e.g. gradle failure boilerplate
/// repeating what `e:` lines and `BUILD FAILED` already say).
fn rules_force_strip(name: &str, line: &str) -> bool {
    match name {
        "gradle" => {
            const P: &[&str] = &[
                "FAILURE: Build failed with an exception",
                "Execution failed for task",
                "> A failure occurred while executing",
                "> Compilation error.",
                "> There were failing tests",
            ];
            P.iter().any(|p| line.trim_start().starts_with(p) || line.starts_with(p))
        }
        "citrace" => {
            line.starts_with("Uploading artifacts") || line.starts_with("Downloading artifacts")
        }
        _ => false,
    }
}

fn rules_strip(name: &str, line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    match name {
        "gradle" => {
            if line.starts_with("> Task :") {
                let rest = &line[8..];
                if line.ends_with("UP-TO-DATE") || line.ends_with("NO-SOURCE")
                    || line.ends_with("FROM-CACHE") || line.ends_with("SKIPPED")
                    || !rest.contains(' ') {
                    return true;
                }
            }
            const P: &[&str] = &[
                "> Configuring project", "> Configure project",
                "> Resolving dependencies", "> Transform ",
                "Starting a Gradle Daemon", "Daemon will be stopped",
                "See the profiling report", "Watched directory hierarchies",
                "To honour the JVM settings", "Calculating task graph",
                "Configuration cache entry", "Reusing configuration cache",
                "Deprecated Gradle features were used", "You can use '--warning-mode all'",
                "For more on this, please refer to https://docs.gradle.org",
            ];
            if P.iter().any(|p| line.starts_with(p)) {
                return true;
            }
            if (line.starts_with("Download") && line.contains("http"))
                || line.contains("problems were found storing the configuration cache") {
                return true;
            }
            const FAIL_BOILERPLATE: &[&str] = &[
                "* What went wrong:", "* Try:", "* Get more help",
                "> Run with --", "There were failing tests",
            ];
            if FAIL_BOILERPLATE.iter().any(|p| line.starts_with(p))
                || line.contains("See the report at:")
                || line.contains(" PASSED") {
                return true;
            }
            t.len() >= 3 && t.starts_with('<') && t.ends_with('>')
                && t[1..t.len() - 1].chars().all(|c| c == '-')
        }
        "pytest" => {
            const P: &[&str] = &["platform ", "cachedir", "rootdir", "plugins",
                "configfile", "collecting", "collected"];
            if P.iter().any(|p| line.starts_with(p)) || line.contains("PASSED") {
                return true;
            }
            if line.starts_with('=') && line.contains("test session starts") {
                return true;
            }
            if line.starts_with('-') && line.contains("live log") {
                return true;
            }
            let core = t.trim_end_matches(|c: char| matches!(c, '[' | ']' | '%' | ' ')
                || c.is_ascii_digit());
            !core.is_empty() && core.chars().all(|c| matches!(c, '.' | 's' | 'x' | 'F'))
                && t.starts_with(core.chars().next().unwrap())
        }
        "npm" => {
            const P: &[&str] = &["npm notice", "npm timing", "npm http"];
            P.iter().any(|p| line.starts_with(p))
                || (line.starts_with("added ") && line.contains("packages in"))
                || t.starts_with("up to date") || t.starts_with("audited")
                || line.contains("packages are looking for funding")
                || t.starts_with("run `npm fund`")
        }
        "pip" => {
            const P: &[&str] = &["Downloading", "Collecting", "Using cached",
                "Found existing", "Uninstalling", "Attempting uninstall",
                "Preparing metadata", "Installing build dependencies",
                "Getting requirements", "Building wheels", "Created wheel",
                "Stored in directory"];
            P.iter().any(|p| t.starts_with(p))
                || line.starts_with("Requirement already satisfied")
                || line.contains("[notice] A new release of pip")
                || line.contains("[notice] To update")
        }
        "citrace" => {
            const INFRA: &[&str] = &[
                "Running with gitlab-runner", "Using Docker executor",
                "Using docker image", "Pulling docker image", "Preparing the",
                "Preparing environment", "Running on runner-", "Getting source from Git",
                "Fetching changes", "Initialized empty Git repository",
                "Created fresh repository", "Checking out", "Skipping Git submodules",
                "Restoring cache", "Saving cache", "Checking cache", "Creating cache",
                "Cleaning up project directory", "Uploading artifacts",
                "Downloading artifacts",
            ];
            INFRA.iter().any(|p| line.starts_with(p))
                || t.starts_with("on runner-")
                || line.contains("via runner-host")
                || rules_strip("npm", line)
        }
        "maven" => {
            if line.contains("-- in ") && line.contains("Tests run:") && !line.contains("FAIL") {
                return true; // per-class passing summary
            }
            if line.starts_with("[INFO]") {
                return !(line.contains("BUILD") || line.contains("Total time")
                    || (line.contains("Tests run:") && !line.contains("-- in ")));
            }
            false
        }
        "ffmpeg" => {
            const LIBS: &[&str] = &["libavutil", "libavcodec", "libavformat", "libavdevice",
                "libavfilter", "libswscale", "libswresample", "libpostproc"];
            const META: &[&str] = &["TSSE", "encoder", "major_brand", "minor_version",
                "compatible_brands", "handler_name", "vendor_id"];
            t.starts_with("configuration:") || t.starts_with("built with")
                || LIBS.iter().any(|l| t.starts_with(l))
                || line.starts_with("Press [q]") || t == "Metadata:"
                || (META.iter().any(|m| t.starts_with(m)) && t.contains(':'))
                || line.starts_with("Stream mapping:")
                || (t.starts_with("Stream #") && t.contains(" -> "))
                || ((line.starts_with("[in#") || line.starts_with("[out#"))
                    && line.contains("] video:"))
        }
        _ => false,
    }
}

fn rules_meta(name: &str) -> (usize, &'static str) {
    match name {
        "gradle" => (50, "gradle: ok"),
        "maven" => (50, "maven: ok"),
        "citrace" => (80, "ci: ok"),
        "pytest" => (60, "pytest: ok"),
        "npm" => (40, "npm: ok"),
        "pip" => (40, "pip: ok"),
        "ffmpeg" => (30, "ffmpeg: ok"),
        _ => (60, "ok"),
    }
}

/// "33 actionable tasks: 16 executed, 17 up-to-date" → "tasks:33 executed:16 up-to-date:17"
fn condense_gradle_tasks(line: &str) -> Option<String> {
    let idx = line.find(" actionable task")?;
    let n: String = line[..idx].chars().filter(|c| c.is_ascii_digit()).collect();
    if n.is_empty() {
        return None;
    }
    let rest = &line[line.find(':')? + 1..];
    let mut parts: Vec<String> = Vec::new();
    for seg in rest.split(',') {
        let seg = seg.trim();
        if let Some(sp) = seg.find(' ') {
            let (num, label) = seg.split_at(sp);
            if num.chars().all(|c| c.is_ascii_digit()) {
                parts.push(format!("{}:{}", label.trim(), num));
                continue;
            }
        }
        parts.push(seg.to_string());
    }
    Some(format!("tasks:{} {}", n, parts.join(" ")))
}

fn rules_filter(name: &str, raw: &str, code: i32) -> String {
    let (max_lines, on_empty) = rules_meta(name);
    let text = resolve_cr(&strip_ansi(raw));
    let lines: Vec<String> = text
        .split('\n')
        .map(|l| l.trim_end())
        .filter(|s| !rules_force_strip(name, s))
        .filter(|s| !(rules_strip(name, s) && !is_important(s)))
        .map(|s| {
            if name == "gradle" {
                condense_gradle_tasks(s).unwrap_or_else(|| s.to_string())
            } else {
                s.to_string()
            }
        })
        .collect();
    let joined = lines.join("\n");
    let joined = joined.trim_matches('\n');
    if joined.trim().is_empty() {
        return if code == 0 {
            on_empty.into()
        } else {
            format!("exit {} [tok full]", code)
        };
    }
    generic_filter(joined, code, max_lines)
}

// ── execution plumbing ───────────────────────────────────────────────

fn run_raw(cmd: &[String]) -> (String, i32) {
    let (s, code) = match Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::inherit()).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            (s, o.status.code().unwrap_or(1))
        }
        Err(_) => (format!("{}: command not found", cmd[0]), 127),
    };
    if let Ok(mut g) = LAST_RAW.lock() {
        *g = s.clone();
    }
    if let Ok(mut g) = LAST_CODE.lock() {
        *g = code;
    }
    (s, code)
}

fn save_raw(cmd: &[String], raw: &str, code: i32) {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(format!("{}/last.txt", dir), raw);
    // rolling history (last 20 raw outputs) for `tok full [n]`
    let rdir = format!("{}/raw", dir);
    let _ = fs::create_dir_all(&rdir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let label: String = cmd.first().map(|s| basename(s)).unwrap_or("cmd")
        .chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
    let _ = fs::write(format!("{}/{}_{}.txt", rdir, ts, label), raw);
    if let Ok(rd) = fs::read_dir(&rdir) {
        let mut names: Vec<String> = rd.flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        while names.len() > 20 {
            let old = names.remove(0);
            let _ = fs::remove_file(format!("{}/{}", rdir, old));
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = fs::write(
        format!("{}/last.json", dir),
        format!("{{\"cmd\": {}, \"exit\": {}, \"ts\": {}}}",
                json_str_list(cmd), code, ts),
    );
}

/// Persist the current agent session id seen by a hook, so `track` can tag
/// each command and `gain` can answer "this session" vs all-time. Tries the
/// Claude/Codex key first, then Antigravity's conversationId.
fn record_session(input: &str) {
    let sid = extract_string_field(input, "\"session_id\"")
        .or_else(|| extract_string_field(input, "\"conversationId\""))
        .unwrap_or_default();
    if sid.is_empty() {
        return;
    }
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(format!("{}/session", dir), sid);
}

fn current_session() -> String {
    fs::read_to_string(format!("{}/session", cache_dir()))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn track(cmd: &[String], raw: &str, filtered: &str) {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{{\"cmd\": \"{}\", \"ts\": {}, \"sid\": \"{}\", \"in\": {}, \"out\": {}}}\n",
        esc_json(cmd.first().map(|s| s.as_str()).unwrap_or("?")), ts,
        esc_json(&current_session()),
        raw.split_whitespace().count(), filtered.split_whitespace().count());
    let path = format!("{}/stats.jsonl", dir);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
    // bounded footprint: keep stats under ~256 KB (rtk's SQLite grows unbounded)
    if let Ok(md) = fs::metadata(&path) {
        if md.len() > 262_144 {
            if let Ok(data) = fs::read_to_string(&path) {
                let lines: Vec<&str> = data.lines().collect();
                let keep = &lines[lines.len() / 2..];
                let _ = fs::write(&path, format!("{}\n", keep.join("\n")));
            }
        }
    }
}

// ── session dedup: same command + same output = a few tokens ─────────

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn fmt_ago(secs: u64) -> String {
    if secs < 90 {
        format!("{}s", secs)
    } else if secs < 5400 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

/// Multiset line-diff vs previous output; None if too different to be useful.
fn render_diff(prev: &str, new: &str) -> Option<String> {
    let old_l: Vec<&str> = prev.lines().collect();
    let new_l: Vec<&str> = new.lines().collect();
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for l in &old_l {
        *counts.entry(*l).or_insert(0) -= 1;
    }
    for l in &new_l {
        *counts.entry(*l).or_insert(0) += 1;
    }
    let mut added: Vec<&str> = Vec::new();
    let mut removed: Vec<&str> = Vec::new();
    for l in &new_l {
        if counts.get(*l).map(|c| *c > 0).unwrap_or(false) {
            *counts.get_mut(*l).unwrap() -= 1;
            if !l.trim().is_empty() {
                added.push(l);
            }
        }
    }
    for l in &old_l {
        if counts.get(*l).map(|c| *c < 0).unwrap_or(false) {
            *counts.get_mut(*l).unwrap() += 1;
            if !l.trim().is_empty() {
                removed.push(l);
            }
        }
    }
    let changed = added.len() + removed.len();
    if changed == 0 || changed > 40
        || changed * 10 > (new_l.len().max(old_l.len())) * 4 {
        return None; // >40% changed — diff not useful
    }
    let mut out: Vec<String> = Vec::new();
    for l in removed.iter().take(20) {
        out.push(format!("- {}", strip_ansi(l).trim_end()));
    }
    for l in added.iter().take(20) {
        out.push(format!("+ {}", strip_ansi(l).trim_end()));
    }
    Some(out.join("\n"))
}

/// If this exact command (in this cwd) ran before: identical output →
/// "unchanged"; small change → line diff. Returns the replacement display.
fn session_dedup(cmd: &[String], raw: &str, code: i32, filtered: &str) -> Option<String> {
    if env::var("TOK_NO_DEDUP").map(|v| v == "1").unwrap_or(false) {
        return None;
    }
    if raw.len() < 256 {
        return None;
    }
    let cwd = env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = format!("{:016x}", fnv(&format!("{}|{}", cmd.join(" "), cwd)));
    let dir = format!("{}/state", cache_dir());
    let _ = fs::create_dir_all(&dir);
    let path = format!("{}/{}.txt", dir, key);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let prev = fs::read_to_string(&path).ok();
    let _ = fs::write(&path, format!("{}\n{}", now, raw));
    let (ts_line, prev_raw) = prev.as_ref()?.split_once('\n')?;
    let ago = fmt_ago(now.saturating_sub(ts_line.parse().ok()?));
    let ftoks = filtered.split_whitespace().count();
    if prev_raw == raw {
        let mut msg = format!("unchanged since last run ({} ago) [tok full]", ago);
        if code != 0 {
            msg.push_str(&format!("\nexit {}", code));
        }
        if msg.split_whitespace().count() < ftoks {
            return Some(msg);
        }
        return None;
    }
    let diff = render_diff(prev_raw, raw)?;
    let mut msg = format!("changes vs run {} ago [tok full]:\n{}", ago, diff);
    if code != 0 {
        msg.push_str(&format!("\nexit {}", code));
    }
    if msg.split_whitespace().count() < ftoks {
        Some(msg)
    } else {
        None
    }
}

fn esc_json(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            _ => o.push(c),
        }
    }
    o
}

fn json_str_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("\"{}\"", esc_json(s))).collect();
    format!("[{}]", inner.join(", "))
}

// ── specialized handlers ─────────────────────────────────────────────

fn cap_list(items: &[String], keep: usize) -> String {
    if items.len() > keep {
        format!("{},...+{}", items[..keep].join(","), items.len() - keep)
    } else {
        items.join(",")
    }
}

fn ls_ext_groups(files: &[(String, u64)]) -> Vec<String> {
    let mut groups: HashMap<String, (usize, u64)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (name, sz) in files {
        let ext = match name.rsplit_once('.') {
            Some((stem, e)) if !stem.is_empty() && !e.is_empty() => e.to_ascii_lowercase(),
            _ => String::from("noext"),
        };
        if !groups.contains_key(&ext) {
            order.push(ext.clone());
        }
        let g = groups.entry(ext).or_insert((0, 0));
        g.0 += 1;
        g.1 += *sz;
    }
    order.sort_by(|a, b| groups[b].0.cmp(&groups[a].0).then_with(|| a.cmp(b)));
    order
        .iter()
        .map(|e| {
            let (n, sz) = groups[e];
            if sz >= 1024 {
                format!("{}:{}:{}", e, n, human(sz))
            } else {
                format!("{}:{}", e, n)
            }
        })
        .collect()
}

fn h_ls(args: &[String]) -> (String, i32) {
    let show_hidden = args.iter().any(|a| matches!(a.as_str(), "-a" | "-la" | "-al" | "-lah" | "-A"));
    let mut paths: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    if paths.is_empty() {
        paths.push(".".into());
    }
    let mut out: Vec<String> = Vec::new();
    for p in &paths {
        let p = if let Some(rest) = p.strip_prefix("~/") {
            format!("{}/{}", home(), rest)
        } else {
            p.clone()
        };
        let md = fs::metadata(&p);
        match md {
            Ok(m) if !m.is_dir() => {
                out.push(format!("{} {}", p, human(m.len())));
                continue;
            }
            Err(_) => {
                out.push(format!("ls: {}: not found", p));
                continue;
            }
            _ => {}
        }
        let mut entries: Vec<_> = match fs::read_dir(&p) {
            Ok(rd) => rd.flatten().collect(),
            Err(e) => {
                out.push(format!("ls: {}", e));
                continue;
            }
        };
        entries.sort_by_key(|e| e.file_name());
        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<(String, u64)> = Vec::new();
        let mut total: u64 = 0;
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let ft = e.file_type();
            if ft.as_ref().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(format!("{}/", name));
            } else {
                let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
                total += sz;
                files.push((name, sz));
            }
        }
        out.push(format!("{}: {} dirs, {} files, {}", p, dirs.len(), files.len(), human(total)));
        if !dirs.is_empty() {
            out.push(format!("  {}", cap_list(&dirs, LS_DIR_KEEP)));
        }
        // huge dir: a per-file listing would dwarf the savings — group by extension
        let grouped = files.len() > LS_GROUP_MIN;
        let items: Vec<String> = if grouped {
            ls_ext_groups(&files)
        } else {
            files
                .iter()
                .map(|(n, sz)| if *sz >= 1024 { format!("{}:{}", n, human(*sz)) } else { n.clone() })
                .collect()
        };
        let mut line = String::from(if grouped { "  by ext: " } else { "  " });
        for f in &items {
            if line.len() + f.len() > 96 {
                out.push(line.trim_end_matches(',').to_string());
                line = String::from("  ");
            }
            line.push_str(f);
            line.push(',');
        }
        if !line.trim_matches([' ', ','].as_ref()).is_empty() {
            out.push(line.trim_end_matches(',').to_string());
        }
    }
    (out.join("\n"), 0)
}

fn h_find(cmd: &[String]) -> (String, i32) {
    let (raw, code) = run_raw(cmd);
    let text = strip_ansi(&raw);
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for ln in text.split('\n') {
        if ln.trim().is_empty() {
            continue;
        }
        if ln.starts_with("find:") {
            errs.push(ln.to_string());
            continue;
        }
        let trimmed = ln.trim_end_matches('/');
        let (d, b) = match trimmed.rfind('/') {
            Some(i) => (&trimmed[..i], &trimmed[i + 1..]),
            None => (".", trimmed),
        };
        let d = if d.is_empty() { "." } else { d };
        if !groups.contains_key(d) {
            groups.insert(d.to_string(), Vec::new());
            order.push(d.to_string());
        }
        groups.get_mut(d).unwrap().push(b.to_string());
    }
    let mut out: Vec<String> = Vec::new();
    for d in &order {
        let names = &groups[d];
        let shown: Vec<&str> = names.iter().take(40).map(|s| s.as_str()).collect();
        let extra = if names.len() > 40 {
            format!(",...+{}", names.len() - 40)
        } else {
            String::new()
        };
        out.push(format!("{}/: {}{}", d, shown.join(","), extra));
    }
    let total: usize = groups.values().map(|v| v.len()).sum();
    if total > 0 {
        out.insert(0, format!("{} results in {} dirs", total, groups.len()));
    }
    out.extend(errs.into_iter().take(5));
    (generic_filter(&out.join("\n"), code, 80), code)
}

fn h_grep(cmd: &[String]) -> (String, i32) {
    let (raw, code) = run_raw(cmd);
    let text = strip_ansi(&raw);
    let mut groups: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut misc: Vec<String> = Vec::new();
    for ln in text.split('\n') {
        if ln.trim().is_empty() {
            continue;
        }
        // parse file:line:content
        let mut parsed = None;
        if let Some(c1) = ln.find(':') {
            if let Some(c2_rel) = ln[c1 + 1..].find(':') {
                let c2 = c1 + 1 + c2_rel;
                let num = &ln[c1 + 1..c2];
                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) && c1 <= 200 {
                    parsed = Some((ln[..c1].to_string(), num.to_string(),
                                   ln[c2 + 1..].trim().to_string()));
                }
            }
        }
        match parsed {
            Some((f, num, content)) => {
                let content: String = content.chars().take(90).collect();
                if !groups.contains_key(&f) {
                    groups.insert(f.clone(), Vec::new());
                    order.push(f.clone());
                }
                groups.get_mut(&f).unwrap().push((num, content));
            }
            None => misc.push(ln.to_string()),
        }
    }
    // global grouping by content
    let mut by_content: HashMap<String, Vec<(String, Vec<String>)>> = HashMap::new();
    let mut corder: Vec<String> = Vec::new();
    let mut total_hits = 0usize;
    for f in &order {
        for (num, c) in &groups[f] {
            total_hits += 1;
            let key: String = c.chars().take(70).collect();
            let entry = by_content.entry(key.clone()).or_insert_with(|| {
                corder.push(key.clone());
                Vec::new()
            });
            match entry.iter_mut().find(|(ef, _)| ef == f) {
                Some((_, nums)) => nums.push(num.clone()),
                None => entry.push((f.clone(), vec![num.clone()])),
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    for c in corder.iter().take(25) {
        let locs = &by_content[c];
        let mut loc_s: Vec<String> = locs.iter().take(30)
            .map(|(f, nums)| {
                let ns: Vec<&str> = nums.iter().take(8).map(|s| s.as_str()).collect();
                format!("{}:{}", f, ns.join(","))
            })
            .collect();
        if locs.len() > 30 {
            loc_s.push(format!("...+{} files", locs.len() - 30));
        }
        out.push(format!("{} <- {}", c, loc_s.join(" ")));
    }
    if corder.len() > 25 {
        out.push(format!("...+{} distinct matches [tok full]", corder.len() - 25));
    }
    out.extend(misc.into_iter().take(10));
    if !order.is_empty() {
        out.insert(0, format!("{} matches in {} files", total_hits, order.len()));
    }
    let text_out = out.join("\n");
    let lines: Vec<&str> = text_out.split('\n').collect();
    let final_out = if lines.len() > 100 {
        format!("{}\n... +{} lines [tok full]",
                lines[..100].join("\n"), lines.len() - 100)
    } else {
        text_out
    };
    if final_out.trim().is_empty() {
        if code == 1 {
            return ("no matches".into(), code);
        }
        return (generic_filter(&raw, code, max_lines_default()), code);
    }
    (final_out, code)
}

/// Collapse long path lists by 2-level directory prefix:
/// 111 x "M target/debug/..." becomes "target/debug/... x111".
fn group_paths(items: &[String]) -> Vec<String> {
    if items.len() <= 8 {
        return items.to_vec();
    }
    let mut groups: Vec<(String, Vec<&String>)> = Vec::new();
    for it in items {
        let path = it.rsplit(' ').next().unwrap_or(it);
        let segs: Vec<&str> = path.split('/').collect();
        let key = if segs.len() > 2 {
            format!("{}/{}", segs[0], segs[1])
        } else {
            it.clone()
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(it),
            None => groups.push((key, vec![it])),
        }
    }
    let mut out: Vec<String> = Vec::new();
    for (key, members) in groups {
        if members.len() <= 2 {
            out.extend(members.into_iter().cloned());
        } else {
            out.push(format!("{}/... x{}", key, members.len()));
        }
    }
    out
}

fn h_git(cmd: &[String]) -> (String, i32) {
    let sub = cmd[1..].iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("");
    match sub {
        "status" => {
            let args: Vec<String> = ["git", "status", "--porcelain=v1", "-b"]
                .iter().map(|s| s.to_string()).collect();
            let (raw, code) = run_raw(&args);
            if code != 0 {
                return (generic_filter(&raw, code, max_lines_default()), code);
            }
            let lines: Vec<&str> = raw.trim().split('\n').filter(|l| !l.is_empty()).collect();
            let branch = lines.first()
                .and_then(|l| l.strip_prefix("## "))
                .unwrap_or("?");
            let mut staged: Vec<String> = Vec::new();
            let mut modified: Vec<String> = Vec::new();
            let mut untracked: Vec<String> = Vec::new();
            let mut other: Vec<String> = Vec::new();
            for ln in lines.iter().skip(1) {
                if ln.len() < 4 {
                    continue;
                }
                let st = &ln[..2];
                let path = &ln[3..];
                if st == "??" {
                    untracked.push(path.to_string());
                } else if "MADRC".contains(&st[..1]) {
                    staged.push(format!("{} {}", st.trim(), path));
                } else if "MD".contains(&st[1..2]) {
                    modified.push(format!("{} {}", st.trim(), path));
                } else {
                    other.push(ln.to_string());
                }
            }
            let mut out = vec![branch.to_string()];
            let mut block = |label: &str, items: &[String]| {
                if !items.is_empty() {
                    let items = group_paths(items);
                    let head: Vec<&str> = items.iter().take(15).map(|s| s.as_str()).collect();
                    let more = if items.len() > 15 {
                        format!("  ...+{}", items.len() - 15)
                    } else {
                        String::new()
                    };
                    out.push(format!("{}({}): {}{}", label, items.len(), head.join("  "), more));
                }
            };
            block("staged", &staged);
            block("changed", &modified);
            block("untracked", &untracked);
            block("other", &other);
            if out.len() == 1 {
                out.push("clean".into());
            }
            (out.join("\n"), 0)
        }
        "log" if !cmd.iter().any(|a| a.starts_with("--format") || a.starts_with("--pretty")) => {
            let mut n = String::from("20");
            let mut paths: Vec<String> = Vec::new();
            let mut skip = false;
            for (i, a) in cmd.iter().enumerate().skip(2) {
                if skip {
                    skip = false;
                    continue;
                }
                if a == "-n" || a == "--max-count" {
                    if i + 1 < cmd.len() {
                        n = cmd[i + 1].clone();
                        skip = true;
                    }
                    continue;
                }
                if a.len() > 1 && a.starts_with('-')
                    && a[1..].chars().all(|c| c.is_ascii_digit()) {
                    n = a[1..].to_string();
                } else if !a.starts_with('-') {
                    paths.push(a.clone());
                }
            }
            let mut args: Vec<String> = ["git", "log", "--oneline", "--no-color",
                "--no-decorate", "-n"].iter().map(|s| s.to_string()).collect();
            args.push(n);
            args.extend(paths);
            let (raw, code) = run_raw(&args);
            (generic_filter(&raw, code, 80), code)
        }
        "diff" => {
            let mut args: Vec<String> = vec!["git".into(), "-c".into(), "color.ui=false".into()];
            args.extend(cmd[1..].iter().cloned());
            let (raw, code) = run_raw(&args);
            if cmd.iter().any(|a| a == "--stat" || a == "--name-only" || a == "--name-status") {
                return (generic_filter(&raw, code, 80), code);
            }
            let mut kept: Vec<String> = Vec::new();
            for ln in strip_ansi(&raw).split('\n') {
                if ln.starts_with("diff --git") {
                    let path = ln.rsplit(" b/").next().unwrap_or(ln);
                    kept.push(format!("* {}", path));
                } else if ln.starts_with("+++") || ln.starts_with("---")
                    || ln.starts_with("index ") {
                    continue;
                } else if ln.starts_with("@@") {
                    let parts: Vec<&str> = ln.splitn(3, "@@").collect();
                    if parts.len() >= 2 && !parts[1].trim().is_empty() {
                        kept.push(format!("@@{}@@", parts[1]));
                    } else {
                        kept.push(ln.to_string());
                    }
                } else if ln.starts_with('+') || ln.starts_with('-') {
                    kept.push(ln.to_string());
                }
            }
            let out = if kept.len() > 200 {
                format!("{}\n... diff truncated ({} more lines) [tok full]",
                        kept[..200].join("\n"), kept.len() - 200)
            } else {
                kept.join("\n")
            };
            if out.trim().is_empty() {
                ("no diff".into(), code)
            } else {
                (out, code)
            }
        }
        "push" | "pull" | "fetch" | "add" | "commit" | "checkout" | "switch"
        | "merge" | "rebase" => {
            let (raw, code) = run_raw(cmd);
            if code == 0 {
                let br_out = Command::new("git")
                    .args(["rev-parse", "--abbrev-ref", "HEAD"])
                    .output();
                let br = br_out
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                let text = strip_ansi(&raw);
                let essentials: Vec<&str> = text.split('\n')
                    .filter(|l| {
                        let low = l.to_lowercase();
                        is_important(l) || low.contains("up to date") || low.contains("up-to-date")
                            || low.contains("fast-forward") || low.contains("fast forward")
                            || low.contains("files changed") || low.contains("file changed")
                            || low.contains("insertion") || low.contains("deletion")
                            || l.contains("->")
                    })
                    .take(6)
                    .collect();
                let mut msg = format!("ok {} {}", sub, br);
                if !essentials.is_empty() {
                    msg.push('\n');
                    msg.push_str(&essentials.join("\n"));
                }
                (msg, 0)
            } else {
                (generic_filter(&raw, code, max_lines_default()), code)
            }
        }
        _ => {
            let (raw, code) = run_raw(cmd);
            (generic_filter(&raw, code, max_lines_default()), code)
        }
    }
}

/// On a fully green `cargo test`, collapse the per-suite `test result:` lines
/// into one total; any failure keeps the detailed output untouched.
fn cargo_test_summary(kept: &[String], code: i32) -> Option<String> {
    if code != 0 {
        return None;
    }
    let (mut suites, mut passed, mut failed, mut ignored) = (0usize, 0usize, 0usize, 0usize);
    let mut secs = 0.0f64;
    for ln in kept {
        let t = ln.trim_start();
        let rest = match t.strip_prefix("test result: ok. ") {
            Some(r) => r,
            None => {
                if t.starts_with("test result:") {
                    return None;
                }
                continue;
            }
        };
        suites += 1;
        for part in rest.split(';') {
            let p = part.trim();
            let mut it = p.split_whitespace();
            if let (Some(num), Some(word)) = (it.next(), it.next()) {
                if let Ok(n) = num.parse::<usize>() {
                    match word {
                        "passed" => passed += n,
                        "failed" => failed += n,
                        "ignored" => ignored += n,
                        _ => {}
                    }
                }
            }
            if let Some(i) = p.find("finished in ") {
                if let Ok(s) = p[i + 12..].trim_end_matches('s').parse::<f64>() {
                    secs += s;
                }
            }
        }
    }
    if suites == 0 || failed > 0 {
        return None;
    }
    let mut s = format!("cargo test: {} passed", passed);
    if ignored > 0 {
        s.push_str(&format!(", {} ignored", ignored));
    }
    s.push_str(&format!(
        " ({} suite{}, {:.2}s)",
        suites,
        if suites == 1 { "" } else { "s" },
        secs
    ));
    Some(s)
}

fn h_cargo(cmd: &[String]) -> (String, i32) {
    let (raw, code) = run_raw(cmd);
    let text = resolve_cr(&strip_ansi(&raw));
    let mut kept: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut warn = 0usize;
    for ln in text.split('\n') {
        let s = ln.trim_end();
        if s.starts_with("error: could not compile") {
            in_block = false;
            continue;
        }
        let is_msg_start = (s.starts_with("error") || s.starts_with("warning"))
            && (s.as_bytes().get(5) == Some(&b'[') || s.as_bytes().get(5) == Some(&b':')
                || s.as_bytes().get(7) == Some(&b'[') || s.as_bytes().get(7) == Some(&b':'));
        if is_msg_start {
            if s.starts_with("warning") && !s.contains("generated") {
                warn += 1;
                in_block = false;
                continue;
            }
            in_block = true;
            kept.push(s.to_string());
            continue;
        }
        if in_block {
            if s.trim().is_empty() {
                in_block = false;
            } else {
                kept.push(s.to_string());
            }
            continue;
        }
        let tr = s.trim_start();
        if ["Compiling", "Downloaded", "Downloading", "Updating", "Locking",
            "Checking", "Documenting", "Fresh", "Adding"]
            .iter().any(|p| tr.starts_with(p)) {
            continue;
        }
        if ["Finished", "Running", "Doc-tests", "test result"]
            .iter().any(|p| tr.starts_with(p))
            || (s.starts_with("test ") && (s.contains("FAILED") || s.contains("panicked")))
            || s.contains("FAILED") || s.starts_with("failures:") {
            kept.push(s.to_string());
        }
    }
    if let Some(sum) = cargo_test_summary(&kept, code) {
        kept.retain(|ln| {
            let t = ln.trim_start();
            !(t.starts_with("test result:") || t.starts_with("Running")
                || t.starts_with("Doc-tests") || t.starts_with("Finished"))
        });
        kept.insert(0, sum);
    }
    if warn > 0 {
        kept.push(format!("({} warnings hidden)", warn));
    }
    let out = kept.join("\n").trim_matches('\n').to_string();
    if out.is_empty() && code == 0 {
        return ("ok".into(), 0);
    }
    (generic_filter(&out, code, 80), code)
}

// ── dispatch ─────────────────────────────────────────────────────────

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// Compact file read: blank-squeeze + head/tail cap. Errors and content
/// stay verbatim — this is for reading code, not logs.
fn h_read(args: &[String]) -> (String, i32) {
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if paths.is_empty() {
        return ("tok read: no file".into(), 2);
    }
    let mut out: Vec<String> = Vec::new();
    let mut code = 0;
    for p in paths {
        match fs::read_to_string(p) {
            Ok(data) => {
                let mut lines: Vec<&str> = Vec::new();
                let mut blank = false;
                for l in data.lines() {
                    if l.trim().is_empty() {
                        if !blank && !lines.is_empty() {
                            lines.push("");
                        }
                        blank = true;
                    } else {
                        blank = false;
                        lines.push(l.trim_end());
                    }
                }
                let n = lines.len();
                out.push(format!("== {} ({} lines, {})", p, data.lines().count(),
                                 human(data.len() as u64)));
                if n > 300 {
                    out.extend(lines[..220].iter().map(|s| s.to_string()));
                    out.push(format!("... {} lines omitted [tok full] ...", n - 270));
                    out.extend(lines[n - 50..].iter().map(|s| s.to_string()));
                } else {
                    out.extend(lines.iter().map(|s| s.to_string()));
                }
            }
            Err(e) => {
                out.push(format!("tok read {}: {}", p, e));
                code = 1;
            }
        }
    }
    (out.join("\n"), code)
}

/// Scan recent Claude Code transcripts for Bash commands that bypassed tok.
fn discover() -> i32 {
    let base = format!("{}/.claude/projects", home());
    let mut files: Vec<(std::time::SystemTime, String)> = Vec::new();
    if let Ok(projects) = fs::read_dir(&base) {
        for proj in projects.flatten() {
            if let Ok(rd) = fs::read_dir(proj.path()) {
                for f in rd.flatten() {
                    let path = f.path().to_string_lossy().into_owned();
                    if path.ends_with(".jsonl") {
                        if let Ok(md) = f.metadata() {
                            if let Ok(t) = md.modified() {
                                files.push((t, path));
                            }
                        }
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let mut missed: HashMap<String, usize> = HashMap::new();
    let mut routed = 0usize;
    let mut total = 0usize;
    for (_, path) in files.iter().take(5) {
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for line in data.lines() {
            if !line.contains("\"command\"") || !line.contains("Bash") {
                continue;
            }
            if let Some((_, _, cmd)) = claude_style_command(line) {
                total += 1;
                let first = basename(cmd.split_whitespace().next().unwrap_or(""));
                if first == "tok" || first == "rtk" {
                    routed += 1;
                } else if REWRITABLE.contains(&first) {
                    *missed.entry(first.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    println!("tok discover (5 newest transcripts): {} bash calls, {} routed via tok/rtk",
             total, routed);
    let mut m: Vec<(String, usize)> = missed.into_iter().collect();
    m.sort_by(|a, b| b.1.cmp(&a.1));
    if m.is_empty() {
        println!("no missed opportunities found");
    } else {
        println!("missed (eligible but unproxied):");
        for (cmd, n) in m.iter().take(10) {
            println!("  {} x{}", cmd, n);
        }
        println!("hint: tok init -g installs the auto-rewrite hook");
    }
    0
}

fn dispatch(cmd: &[String]) -> i32 {
    // Filter crash must never block the user: fall back to raw output.
    let cmd_owned: Vec<String> = cmd.to_vec();
    let result = std::panic::catch_unwind(move || dispatch_inner(&cmd_owned));
    match result {
        Ok(code) => code,
        Err(_) => {
            let raw = LAST_RAW.lock().map(|g| g.clone()).unwrap_or_default();
            let code = LAST_CODE.lock().map(|g| *g).unwrap_or(1);
            eprintln!("[tok] filter error — passing raw output through");
            print!("{}", raw);
            code
        }
    }
}

fn dispatch_inner(cmd: &[String]) -> i32 {
    let mut name = basename(&cmd[0]).to_string();
    if cmd[0].ends_with("gradlew") {
        name = "gradlew".into();
    }
    let mut last_raw = String::new();
    let (filtered, code) = match name.as_str() {
        "ls" => h_ls(&cmd[1..]),
        "cat" | "read" => h_read(&cmd[1..]),
        "find" => h_find(cmd),
        "grep" | "rg" | "egrep" => h_grep(cmd),
        "git" => h_git(cmd),
        "cargo" => h_cargo(cmd),
        "gradle" | "gradlew" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("gradle", &raw, code), code)
        }
        "pytest" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("pytest", &raw, code), code)
        }
        "npm" | "pnpm" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("npm", &raw, code), code)
        }
        "pip" | "pip3" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("pip", &raw, code), code)
        }
        "mvn" | "maven" | "mvnw" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("maven", &raw, code), code)
        }
        "glab" if cmd.iter().any(|a| a == "trace") => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("citrace", &raw, code), code)
        }
        "ffmpeg" | "ffprobe" => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (rules_filter("ffmpeg", &raw, code), code)
        }
        _ => {
            let (raw, code) = run_raw(cmd);
            last_raw = raw.clone();
            (generic_filter(&raw, code, max_lines_default()), code)
        }
    };
    let global_raw = LAST_RAW.lock().map(|g| g.clone()).unwrap_or_default();
    let raw_owned: String = if !last_raw.is_empty() {
        last_raw
    } else if !global_raw.is_empty() {
        global_raw
    } else {
        filtered.clone()
    };
    save_raw(cmd, &raw_owned, code);
    track(cmd, &raw_owned, &filtered);
    let display = session_dedup(cmd, &raw_owned, code, &filtered).unwrap_or(filtered);
    println!("{}", display);
    code
}

// ── Claude Code hook ─────────────────────────────────────────────────

const REWRITABLE: &[&str] = &[
    "git", "cargo", "gradle", "gradlew", "pytest", "npm", "pnpm", "pip", "pip3",
    "ffmpeg", "ffprobe", "ls", "find", "grep", "rg", "egrep", "make", "go",
    "docker", "kubectl", "node", "python", "python3", "javac", "gcc", "clang",
    "tsc", "eslint", "ruff", "mypy", "pkg", "apt", "du", "df", "wc", "tree", "gh",
    "cat", "glab", "mvn",
];

/// Extract the byte range of the `"tool_input": { ... }` object
/// (transcript files use `"input"` instead).
fn tool_input_range(j: &str) -> Option<(usize, usize)> {
    let key = j.find("\"tool_input\"").or_else(|| j.find("\"input\""))?;
    let open = j[key..].find('{')? + key;
    let b = j.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &c) in b.iter().enumerate().skip(open) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Within a JSON object substring, find the value-start quote of "command".
fn command_value_quote(obj: &str) -> Option<usize> {
    let k = obj.find("\"command\"")?;
    let after = &obj[k + 9..];
    let colon = after.find(':')?;
    let mut idx = k + 9 + colon + 1;
    let b = obj.as_bytes();
    while idx < b.len() && (b[idx] as char).is_whitespace() {
        idx += 1;
    }
    if idx < b.len() && b[idx] == b'"' {
        Some(idx)
    } else {
        None
    }
}

/// Brace-matched object that follows `key` (e.g. "\"toolCall\""). Generic
/// sibling of tool_input_range for envelopes that nest the command elsewhere.
fn object_after_key(j: &str, key: &str) -> Option<(usize, usize)> {
    let k = j.find(key)?;
    let open = j[k..].find('{')? + k;
    let b = j.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for (i, &c) in b.iter().enumerate().skip(open) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Opening-quote position of the string value for `key` inside `obj`. Generic
/// sibling of command_value_quote (which is hardwired to "command").
fn quoted_value_pos(obj: &str, key: &str) -> Option<usize> {
    let k = obj.find(key)?;
    let after = &obj[k + key.len()..];
    let colon = after.find(':')?;
    let mut idx = k + key.len() + colon + 1;
    let b = obj.as_bytes();
    while idx < b.len() && (b[idx] as char).is_whitespace() {
        idx += 1;
    }
    if idx < b.len() && b[idx] == b'"' {
        Some(idx)
    } else {
        None
    }
}

fn parse_json_string(s: &str) -> Option<String> {
    // s starts at the opening quote
    let mut it = s.chars();
    if it.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let h: String = it.by_ref().take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&h, 16) {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                        }
                    }
                }
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}

/// Should a single simple segment (no top-level shell operators — the caller
/// guarantees this) get a `tok ` prefix? Classifies by first word only.
fn seg_should_prefix(seg: &str) -> bool {
    let first_tok = seg.split_whitespace().next().unwrap_or("");
    let first = basename(first_tok);
    if first.is_empty()
        || ["tok", "rtk", "cd", "export", "source", "vim", "nano", "ssh"].contains(&first)
    {
        return false;
    }
    let rewrite_all = env::var("TOK_REWRITE_ALL").map(|v| v == "1").unwrap_or(false);
    rewrite_all || REWRITABLE.contains(&first) || first_tok.ends_with("gradlew")
}

/// Quote-aware rewrite: prefix `tok ` to each safe simple segment of a
/// top-level `&&`/`||`/`;` chain, leaving operators and non-rewritable segments
/// (cd, sudo, …) intact. Returns None — meaning "leave the whole command raw,
/// exactly as today" — on ANYTHING we can't handle with certainty: pipes,
/// redirects, command substitution `$(`/backtick, subshells `()`, background
/// `&`, newlines, or unbalanced quotes. This makes the rewrite strictly safe
/// (worst case = no change) and strictly additive for savings.
fn rewrite_command(cmd: &str) -> Option<String> {
    let b = cmd.as_bytes();
    let mut segs: Vec<&str> = Vec::new();
    let mut ops: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\'' => {
                i += 1;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                if i >= b.len() {
                    return None; // unbalanced single quote
                }
                i += 1;
            }
            b'"' => {
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
                if i >= b.len() {
                    return None; // unbalanced double quote
                }
                i += 1;
            }
            b'\\' => i += 2, // escape next char (outside quotes)
            b'`' => return None,
            b'$' if b.get(i + 1) == Some(&b'(') => return None,
            b'(' | b')' | b'<' | b'>' | b'\n' => return None,
            b'&' => {
                if b.get(i + 1) == Some(&b'&') {
                    segs.push(&cmd[start..i]);
                    ops.push("&&");
                    i += 2;
                    start = i;
                } else {
                    return None; // background &
                }
            }
            b'|' => {
                if b.get(i + 1) == Some(&b'|') {
                    segs.push(&cmd[start..i]);
                    ops.push("||");
                    i += 2;
                    start = i;
                } else {
                    return None; // pipe
                }
            }
            b';' => {
                segs.push(&cmd[start..i]);
                ops.push(";");
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    segs.push(&cmd[start..]);
    let mut any = false;
    let mut out: Vec<String> = Vec::with_capacity(segs.len());
    for s in &segs {
        if !s.trim().is_empty() && seg_should_prefix(s) {
            // Insert "tok " before the first non-whitespace byte and keep the
            // segment otherwise VERBATIM — never trim, so a backslash-escaped
            // trailing space (`-m hi\ `) and tabs survive unchanged.
            let lead = s.len() - s.trim_start().len();
            out.push(format!("{}tok {}", &s[..lead], &s[lead..]));
            any = true;
        } else {
            out.push((*s).to_string()); // verbatim
        }
    }
    if !any {
        return None;
    }
    // Bare operators between verbatim segments reconstruct the original spacing
    // exactly — and keep a malformed `;;`/dangling-`&&` a syntax error rather
    // than silently "fixing" it into a working (and possibly destructive) one.
    let mut res = String::new();
    for (idx, s) in out.iter().enumerate() {
        if idx > 0 {
            res.push_str(ops[idx - 1]);
        }
        res.push_str(s);
    }
    Some(res)
}

/// Index just past the closing quote of a JSON string that starts at `start`
/// (which must be an opening `"`). Used to splice a new command value into an
/// existing tool_input without disturbing sibling fields.
fn json_string_end(s: &str, start: usize) -> Option<usize> {
    let b = s.as_bytes();
    if b.get(start) != Some(&b'"') {
        return None;
    }
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn read_stdin_trimmed() -> String {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    input.trim_start_matches('\u{feff}').trim().to_string()
}

/// Find a top-level-ish string field value, e.g. key = "\"tool_name\"".
fn extract_string_field(j: &str, key: &str) -> Option<String> {
    let k = j.find(key)?;
    let after = &j[k + key.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    parse_json_string(rest)
}

/// Locate command inside tool_input and return (object_text, quote_pos, command).
fn claude_style_command(input: &str) -> Option<(String, usize, String)> {
    let (s, e) = tool_input_range(input)?;
    let ti = input[s..e].to_string();
    let q = command_value_quote(&ti)?;
    let cmd = parse_json_string(&ti[q..])?;
    if cmd.is_empty() {
        return None;
    }
    Some((ti, q, cmd))
}

fn hook_claude() -> i32 {
    let input = read_stdin_trimmed();
    if input.is_empty() {
        return 0;
    }
    record_session(&input);
    let (ti, qpos, cmd) = match claude_style_command(&input) {
        Some(t) => t,
        None => return 0,
    };
    let new = match rewrite_command(&cmd) {
        Some(n) => n,
        None => return 0,
    };
    let end = match json_string_end(&ti, qpos) {
        Some(e) => e,
        None => return 0,
    };
    // replace just the command value, preserving every other tool_input field
    let updated = format!("{}\"{}\"{}", &ti[..qpos], esc_json(&new), &ti[end..]);
    println!(
        "{{\"hookSpecificOutput\": {{\"hookEventName\": \"PreToolUse\", \
         \"permissionDecision\": \"allow\", \
         \"permissionDecisionReason\": \"tok auto-rewrite\", \
         \"updatedInput\": {}}}}}",
        updated
    );
    0
}

/// OpenAI Codex hook (CLI, desktop and IDE — all share the ~/.codex config
/// layer, so one entrypoint covers every surface). Codex ships a Claude-style
/// PreToolUse contract: stdin carries `tool_name` + `tool_input.command`
/// (alongside a richer envelope — turn_id, cwd, tool_use_id… — that we ignore),
/// and the allow+rewrite reply is the same
/// `hookSpecificOutput.permissionDecision` / `updatedInput` shape. The contract
/// is byte-identical to Claude's, so we reuse that logic verbatim rather than
/// duplicate it — if Codex ever diverges, fork here. The config-side matcher
/// (`^Bash$`) gates which tools reach us, exactly like Claude's matcher.
fn hook_codex() -> i32 {
    hook_claude()
}

/// Google Antigravity hook (agy CLI + desktop; one global
/// ~/.gemini/config/hooks.json covers both). Different envelope from Claude:
/// the shell command lives at toolCall.args.CommandLine, and a rewrite is
/// returned by echoing the whole toolCall back under "overwrite" with
/// CommandLine swapped — preserving name, Cwd and the rest of args (string
/// injection, like the Claude path). The reply MUST pair "overwrite" with an
/// explicit {"decision":"allow"} — empirically agy 1.x silently ignores a bare
/// overwrite and runs the original command (verified by a live round-trip).
/// Wire this under toolPermission=always-proceed; request-review drops the
/// overwrite (a separate upstream bug).
fn hook_antigravity() -> i32 {
    let input = read_stdin_trimmed();
    if input.is_empty() {
        return 0;
    }
    record_session(&input);
    let (s, e) = match object_after_key(&input, "\"toolCall\"") {
        Some(r) => r,
        None => return 0,
    };
    let tc = &input[s..e];
    let q = match quoted_value_pos(tc, "\"CommandLine\"") {
        Some(q) => q,
        None => return 0,
    };
    let cmd = match parse_json_string(&tc[q..]) {
        Some(c) => c,
        None => return 0,
    };
    let new = match rewrite_command(&cmd) {
        Some(n) => n,
        None => return 0,
    };
    let qend = match json_string_end(tc, q) {
        Some(e) => e,
        None => return 0,
    };
    let new_tc = format!("{}\"{}\"{}", &tc[..q], esc_json(&new), &tc[qend..]);
    println!("{{\"decision\": \"allow\", \"overwrite\": {}}}", new_tc);
    0
}

/// Gemini CLI BeforeTool hook (tool_name == "run_shell_command").
fn hook_gemini() -> i32 {
    let input = read_stdin_trimmed();
    if input.is_empty() {
        println!("{{\"decision\": \"allow\"}}");
        return 0;
    }
    record_session(&input);
    let tool = extract_string_field(&input, "\"tool_name\"").unwrap_or_default();
    if tool != "run_shell_command" {
        println!("{{\"decision\": \"allow\"}}");
        return 0;
    }
    match claude_style_command(&input).and_then(|(_, _, cmd)| rewrite_command(&cmd)) {
        Some(new) => {
            println!(
                "{{\"decision\": \"allow\", \"hookSpecificOutput\": \
                 {{\"tool_input\": {{\"command\": \"{}\"}}}}}}",
                esc_json(&new)
            );
        }
        None => println!("{{\"decision\": \"allow\"}}"),
    }
    0
}

/// Cursor Agent hook: {} = no change; allow + updated_input on rewrite.
fn hook_cursor() -> i32 {
    let input = read_stdin_trimmed();
    record_session(&input);
    match claude_style_command(&input).and_then(|(_, _, cmd)| rewrite_command(&cmd)) {
        Some(new) => {
            println!(
                "{{\"continue\": true, \"permission\": \"allow\", \
                 \"updated_input\": {{\"command\": \"{}\"}}}}",
                esc_json(&new)
            );
        }
        None => println!("{{}}"),
    }
    0
}

/// Copilot: VS Code Chat (snake_case, like Claude) or CLI (camelCase,
/// toolArgs is a JSON-encoded string; respond with modifiedArgs object).
fn hook_copilot() -> i32 {
    let input = read_stdin_trimmed();
    if input.is_empty() {
        return 0;
    }
    record_session(&input);
    if input.contains("\"tool_name\"") {
        let tool = extract_string_field(&input, "\"tool_name\"").unwrap_or_default();
        if !matches!(tool.as_str(), "runTerminalCommand" | "Bash" | "bash") {
            return 0;
        }
        if let Some(new) = claude_style_command(&input).and_then(|(_, _, c)| rewrite_command(&c)) {
            println!(
                "{{\"hookSpecificOutput\": {{\"hookEventName\": \"PreToolUse\", \
                 \"permissionDecision\": \"allow\", \
                 \"permissionDecisionReason\": \"tok auto-rewrite\", \
                 \"updatedInput\": {{\"command\": \"{}\"}}}}}}",
                esc_json(&new)
            );
        }
        return 0;
    }
    let tool = extract_string_field(&input, "\"toolName\"").unwrap_or_default();
    if tool != "bash" {
        return 0;
    }
    let args_obj = match extract_string_field(&input, "\"toolArgs\"") {
        Some(a) => a, // decoded JSON object text
        None => return 0,
    };
    let qpos = match command_value_quote(&args_obj) {
        Some(q) => q,
        None => return 0,
    };
    let cmd = match parse_json_string(&args_obj[qpos..]) {
        Some(c) => c,
        None => return 0,
    };
    let new = match rewrite_command(&cmd) {
        Some(n) => n,
        None => return 0,
    };
    let qend = match json_string_end(&args_obj, qpos) {
        Some(e) => e,
        None => return 0,
    };
    let modified = format!("{}\"{}\"{}", &args_obj[..qpos], esc_json(&new), &args_obj[qend..]);
    println!(
        "{{\"permissionDecision\": \"allow\", \
         \"permissionDecisionReason\": \"tok auto-rewrite\", \
         \"modifiedArgs\": {}}}",
        modified
    );
    0
}

// ── meta commands ────────────────────────────────────────────────────

fn gain() -> i32 {
    let p = format!("{}/stats.jsonl", cache_dir());
    let data = match fs::read_to_string(&p) {
        Ok(d) => d,
        Err(_) => {
            println!("no stats yet");
            return 0;
        }
    };
    let cur = current_session();
    let (mut tin, mut tout, mut n) = (0u64, 0u64, 0u64);
    let (mut sin, mut sout, mut sn) = (0u64, 0u64, 0u64);
    for ln in data.lines() {
        let get = |key: &str| -> Option<u64> {
            let k = ln.find(key)?;
            let rest = &ln[k + key.len()..];
            let digits: String = rest.chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse().ok()
        };
        if let (Some(i), Some(o)) = (get("\"in\":"), get("\"out\":")) {
            tin += i;
            tout += o;
            n += 1;
            if !cur.is_empty() && extract_string_field(ln, "\"sid\"").as_deref() == Some(cur.as_str()) {
                sin += i;
                sout += o;
                sn += 1;
            }
        }
    }
    let pct = |inp: u64, outp: u64| if inp > 0 { 100.0 * (1.0 - outp as f64 / inp as f64) } else { 0.0 };
    if !cur.is_empty() && sn > 0 {
        println!("this session: {} commands, {} → {} tokens ({:.1}% saved)", sn, sin, sout, pct(sin, sout));
    }
    println!("all-time: {} commands, {} → {} tokens ({:.1}% saved)", n, tin, tout, pct(tin, tout));
    0
}

const TOK_GAIN_INSTRUCTION: &str = "<!-- tok:gain -->
## tok token savings
When asked how many tokens tok (or you) saved — this session, today, or in
total — run `tok gain` and report the number: the \"this session\" line for the
current run, \"all-time\" for the cumulative total. Don't estimate or ask what
was meant — run it and answer.
<!-- /tok:gain -->
";

/// Append the gain self-report instruction to CLAUDE.md so a freshly-installed
/// agent answers "tokens saved" reflexively. Global (~/.claude) by default —
/// tok is a machine-wide tool; `--local`/`--project` targets the project file.
/// Idempotent via a sentinel marker — shared with tok.py so the Rust→Python
/// init delegation never double-writes.
fn install_instruction(flags: &[String]) {
    let local = flags.iter().any(|a| a == "--local" || a == "--project");
    let path = if local {
        "CLAUDE.md".to_string()
    } else {
        format!("{}/.claude/CLAUDE.md", home())
    };
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.contains("<!-- tok:gain -->") {
        return;
    }
    if let Some(dir) = std::path::Path::new(&path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = fs::create_dir_all(dir);
        }
    }
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(format!("{}{}", sep, TOK_GAIN_INSTRUCTION).as_bytes());
        println!("tok gain instruction → {}", path);
    }
}

fn usage() {
    println!("tok {} — universal token-diet proxy (native)", VERSION);
    println!("  tok <command> [args...]   run command through the best filter");
    println!("  tok run -- <command> ...  force the generic filter");
    println!("  tok proxy <command> ...   raw passthrough");
    println!("  tok pipe [name]           filter stdin (gradle|pytest|npm|pip|ffmpeg|generic)");
    println!("  tok full                  print full raw output of the last run");
    println!("  tok gain                  cumulative token savings");
    println!("  tok init [--local]        install hook + gain instruction (global by default)");
    println!("  tok hook claude|codex|antigravity|gemini|copilot|cursor   hook entrypoint (JSON on stdin)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_test_success_aggregates_suites() {
        let kept: Vec<String> = vec![
            "     Running unittests src/lib.rs (target/debug/deps/x-abc)".into(),
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s".into(),
            "     Running tests/basic.rs (target/debug/deps/basic-def)".into(),
            "test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s".into(),
            "   Doc-tests x".into(),
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s".into(),
        ];
        let s = cargo_test_summary(&kept, 0).unwrap();
        assert_eq!(s, "cargo test: 5 passed, 1 ignored (3 suites, 0.03s)");
        // failures (or any non-zero exit) must keep the detailed output
        assert!(cargo_test_summary(&kept, 101).is_none());
        let failed: Vec<String> = vec![
            "test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s".into(),
        ];
        assert!(cargo_test_summary(&failed, 0).is_none());
    }

    #[test]
    fn cap_list_truncates_with_count() {
        let items: Vec<String> = (0..44).map(|i| format!("d{}/", i)).collect();
        let s = cap_list(&items, 40);
        assert!(s.ends_with("d39/,...+4"), "s = {}", s);
        assert!(!s.contains("d40/"));
        assert_eq!(cap_list(&items[..3], 40), "d0/,d1/,d2/");
    }

    #[test]
    fn ls_ext_groups_collapse_huge_dirs() {
        let mut files: Vec<(String, u64)> = (0..300).map(|i| (format!("lib{}.dll", i), 2048)).collect();
        files.extend((0..50).map(|i| (format!("tool{}.exe", i), 512)));
        files.push((String::from("README"), 100));
        files.push((String::from(".hidden"), 100));
        let g = ls_ext_groups(&files);
        assert_eq!(g[0], "dll:300:600K");
        assert_eq!(g[1], "exe:50:25K");
        assert!(g.contains(&String::from("noext:2")), "g = {:?}", g);
    }

    #[test]
    fn generic_dedups_similar_lines_with_counts() {
        let raw = "copied file 1 of 30\ncopied file 2 of 30\ncopied file 3 of 30\ndone";
        let out = generic_filter(raw, 0, 60);
        assert!(out.contains("(x3)"), "out = {}", out);
        assert!(out.contains("done"));
    }

    #[test]
    fn generic_keeps_errors_beyond_cap() {
        let mut lines: Vec<String> = (0..200).map(|i| format!("{} info", "x".repeat(i + 1))).collect();
        lines[100] = "ERROR: disk on fire".into();
        let out = generic_filter(&lines.join("\n"), 0, 60);
        assert!(out.contains("ERROR: disk on fire"));
        assert!(out.contains("lines omitted"));
    }

    #[test]
    fn generic_empty_ok_and_exit_code() {
        assert_eq!(generic_filter("", 0, 60), "ok");
        assert!(generic_filter("", 3, 60).contains("exit 3"));
        assert!(generic_filter("boom", 2, 60).contains("exit 2"));
    }

    #[test]
    fn gradle_rtk_toml_vector() {
        // rtk's own gradle.toml test input — tok keeps the failure info
        let input = "> Configuring project :app\n> Task :app:compileJava UP-TO-DATE\n\
                     > Task :app:compileKotlin UP-TO-DATE\n> Task :app:test\n\n\
                     3 tests completed, 1 failed\n\nBUILD FAILED in 12s";
        let out = rules_filter("gradle", input, 0);
        assert!(out.contains("3 tests completed, 1 failed"));
        assert!(out.contains("BUILD FAILED in 12s"));
        assert!(!out.contains("UP-TO-DATE"));
        assert!(!out.contains("Configuring"));
    }

    #[test]
    fn gradle_real_configure_and_condense() {
        let input = "> Configure project :app\nBUILD SUCCESSFUL in 24s\n\
                     33 actionable tasks: 16 executed, 17 up-to-date";
        let out = rules_filter("gradle", input, 0);
        assert!(!out.contains("Configure project"));
        assert!(out.contains("BUILD SUCCESSFUL in 24s"));
        assert!(out.contains("tasks:33 executed:16 up-to-date:17"));
    }

    #[test]
    fn maven_keeps_errors_strips_info_noise() {
        let input = "[INFO] Scanning for projects...\n[INFO] Building X 1.0\n\
            [INFO] Tests run: 1, Failures: 0, Errors: 0 -- in org.x.PassTest\n\
            [ERROR] Tests run: 1, Failures: 1, Errors: 0 <<< FAILURE! -- in org.x.FailTest\n\
            [INFO] BUILD FAILURE\n[INFO] Total time:  2.193 s";
        let out = rules_filter("maven", input, 0);
        assert!(out.contains("FailTest"));
        assert!(out.contains("BUILD FAILURE"));
        assert!(out.contains("Total time"));
        assert!(!out.contains("Scanning for projects"));
        assert!(!out.contains("PassTest"));
    }

    #[test]
    fn frames_collapse_keeps_two() {
        let input = "Boom\n\tat a.B.c(B.java:1)\n\tat d.E.f(E.java:2)\n\
                     \tat g.H.i(H.java:3)\n\tat j.K.l(K.java:4)\nCaused by: X";
        let out = collapse_frames(input);
        assert!(out.contains("B.java:1"));
        assert!(out.contains("E.java:2"));
        assert!(!out.contains("H.java:3"));
        assert!(out.contains("+2 frames"));
        assert!(out.contains("Caused by: X"));
    }

    #[test]
    fn frames_prefer_user_code_over_framework() {
        let input = "AssertionError\n\
                     \tat org.junit.Assert.fail(Assert.java:89)\n\
                     \tat org.junit.Assert.assertEquals(Assert.java:197)\n\
                     \tat com.example.myapp.CalculatorTest.testSubtraction(CalculatorTest.kt:25)\n\
                     \tat java.base/jdk.internal.reflect.Native(Native.java:62)";
        let out = collapse_frames(input);
        assert!(out.contains("CalculatorTest.kt:25"));
        assert!(!out.contains("Assert.java:89"));
        assert!(out.contains("+3 frames"));
    }

    #[test]
    fn norm_and_word_boundaries() {
        assert_eq!(norm_line("time=00:01:02 size=42KiB"), norm_line("time=09:09:09 size=7KiB"));
        assert!(is_important("ERROR: x"));
        assert!(!is_important("terrorism statistics"));
        assert!(is_noise("section_start:123:step[0K"));
        assert!(is_noise("Receiving objects:  45% (10/22)"));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(800), "800B");
        assert_eq!(human(25 * 1024 + 512), "25.5K");
        assert_eq!(human(2 * 1024 * 1024), "2M");
    }

    #[test]
    fn hook_claude_replaces_command_preserving_fields() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"git status","description":"d"}}"#;
        let (ti, q, cmd) = claude_style_command(input).unwrap();
        assert_eq!(cmd, "git status");
        let new = rewrite_command(&cmd).unwrap();
        assert_eq!(new, "tok git status");
        let end = json_string_end(&ti, q).unwrap();
        let updated = format!("{}\"{}\"{}", &ti[..q], esc_json(&new), &ti[end..]);
        assert!(updated.contains(r#""command":"tok git status""#));
        assert!(updated.contains(r#""description":"d""#)); // sibling preserved
    }

    #[test]
    fn rewrite_command_safety_and_chains() {
        // unsafe constructs → leave the whole command raw (None == today's behavior)
        assert!(rewrite_command("cat x | grep y").is_none());
        assert!(rewrite_command("cargo build > log").is_none());
        assert!(rewrite_command("git diff $(git merge-base a b)").is_none());
        assert!(rewrite_command("ls `pwd`").is_none());
        assert!(rewrite_command("server &").is_none());
        assert!(rewrite_command("echo \"unbalanced").is_none());
        // never-rewrite / non-rewritable single command → None
        assert!(rewrite_command("rtk git status").is_none());
        assert!(rewrite_command("cd /tmp").is_none());
        assert!(rewrite_command("echo hi").is_none());
        // simple rewritable → prefixed
        assert_eq!(rewrite_command("./gradlew assembleDebug").unwrap(), "tok ./gradlew assembleDebug");
        assert_eq!(rewrite_command("ffmpeg -i a.wav b.mp3").unwrap(), "tok ffmpeg -i a.wav b.mp3");
        // &&/||/; chains: prefix safe segments, keep operators + non-rewritable raw
        assert_eq!(rewrite_command("cd src && cargo build").unwrap(), "cd src && tok cargo build");
        assert_eq!(rewrite_command("git add -A && git commit -m x").unwrap(),
                   "tok git add -A && tok git commit -m x");
        assert_eq!(rewrite_command("cargo test || true").unwrap(), "tok cargo test || true");
        assert_eq!(rewrite_command("git fetch ; git status").unwrap(), "tok git fetch ; tok git status");
        // operator inside quotes must NOT split the command
        assert_eq!(rewrite_command(r#"git commit -m "a && b""#).unwrap(),
                   r#"tok git commit -m "a && b""#);
        // a pipe anywhere in a chain → bail entirely (raw)
        assert!(rewrite_command("cargo build && cat x | grep y").is_none());
        // regression (adversarial review): a backslash-escaped trailing space is
        // part of the argument and must survive verbatim — no trimming.
        assert_eq!(rewrite_command("git commit -m hi\\ ").unwrap(), "tok git commit -m hi\\ ");
        // regression: `;;` is a bash syntax error and must STAY one, not get
        // "repaired" into two valid `;` (which could activate a dead command).
        assert_eq!(rewrite_command("git status;;cargo build").unwrap(),
                   "tok git status;;tok cargo build");
    }

    #[test]
    fn antigravity_overwrites_commandline_preserving_args() {
        // Antigravity's envelope nests the command at toolCall.args.CommandLine
        // and expects the whole toolCall echoed back under "overwrite". Cwd and
        // any sibling args must survive the rewrite untouched.
        let input = r#"{"toolCall":{"name":"run_command","args":{"CommandLine":"git status","Cwd":"/repo"}},"workspacePaths":[],"transcriptPath":""}"#;
        let (s, e) = object_after_key(input, "\"toolCall\"").unwrap();
        let tc = &input[s..e];
        let q = quoted_value_pos(tc, "\"CommandLine\"").unwrap();
        let cmd = parse_json_string(&tc[q..]).unwrap();
        assert_eq!(cmd, "git status");
        let new = rewrite_command(&cmd).unwrap();
        let qend = json_string_end(tc, q).unwrap();
        let new_tc = format!("{}\"{}\"{}", &tc[..q], esc_json(&new), &tc[qend..]);
        let out = format!("{{\"decision\": \"allow\", \"overwrite\": {}}}", new_tc);
        assert!(out.contains(r#""CommandLine":"tok git status""#));
        assert!(out.contains(r#""Cwd":"/repo""#)); // sibling arg preserved
        assert!(out.starts_with(r#"{"decision": "allow", "overwrite": {"name":"run_command""#));
    }

    #[test]
    fn codex_envelope_rewrites_through_extra_fields() {
        // Codex sends a richer PreToolUse envelope than Claude (turn_id, cwd,
        // tool_use_id…). The command must still be located at tool_input.command
        // and rewritten cleanly — tool_use_id must not be mistaken for tool_input.
        let input = r#"{"session_id":"s","cwd":"/repo","hook_event_name":"PreToolUse","turn_id":"t1","tool_name":"Bash","tool_use_id":"call_42","tool_input":{"command":"cargo build"}}"#;
        let (ti, q, cmd) = claude_style_command(input).unwrap();
        assert_eq!(cmd, "cargo build");
        let new = rewrite_command(&cmd).unwrap();
        let end = json_string_end(&ti, q).unwrap();
        let updated = format!("{}\"{}\"{}", &ti[..q], esc_json(&new), &ti[end..]);
        assert!(updated.contains(r#""command":"tok cargo build""#));
    }

    #[test]
    fn copilot_cli_toolargs_decoding() {
        let input = r#"{"toolName":"bash","toolArgs":"{\"command\":\"git status\",\"timeout\":5}"}"#;
        let args_obj = extract_string_field(input, "\"toolArgs\"").unwrap();
        assert!(args_obj.contains(r#""command":"git status""#));
        let q = command_value_quote(&args_obj).unwrap();
        let cmd = parse_json_string(&args_obj[q..]).unwrap();
        assert_eq!(cmd, "git status");
        let modified = format!("{}tok {}", &args_obj[..q + 1], &args_obj[q + 1..]);
        assert!(modified.contains(r#""command":"tok git status""#));
        assert!(modified.contains(r#""timeout":5"#));
    }

    #[test]
    fn grep_like_norm_does_not_merge_distinct_text() {
        assert_ne!(norm_line("fn alpha()"), norm_line("fn beta()"));
    }
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    let code = match argv.first().map(|s| s.as_str()) {
        None | Some("-h") | Some("--help") => {
            usage();
            0
        }
        Some("--version") => {
            println!("tok {}", VERSION);
            0
        }
        Some("full") => {
            let arg = argv.get(1).map(|s| s.as_str());
            let rdir = format!("{}/raw", cache_dir());
            let mut names: Vec<String> = fs::read_dir(&rdir)
                .map(|rd| rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            names.sort();
            match arg {
                Some("list") => {
                    for (i, n) in names.iter().rev().enumerate() {
                        println!("{}: {}", i + 1, n);
                    }
                    if names.is_empty() {
                        println!("no cached output");
                    }
                }
                Some(n) if n.parse::<usize>().is_ok() => {
                    let k: usize = n.parse().unwrap();
                    if k >= 1 && k <= names.len() {
                        let f = &names[names.len() - k];
                        match fs::read_to_string(format!("{}/{}", rdir, f)) {
                            Ok(d) => print!("{}", d),
                            Err(_) => println!("no cached output"),
                        }
                    } else {
                        println!("no cached output #{} (have {})", k, names.len());
                    }
                }
                _ => match fs::read_to_string(format!("{}/last.txt", cache_dir())) {
                    Ok(d) => print!("{}", d),
                    Err(_) => println!("no cached output"),
                },
            }
            0
        }
        Some("gain") => gain(),
        Some("hook") => match argv.get(1).map(|s| s.as_str()) {
            Some("gemini") => hook_gemini(),
            Some("cursor") => hook_cursor(),
            Some("copilot") => hook_copilot(),
            Some("codex") => hook_codex(),
            Some("antigravity") | Some("agy") => hook_antigravity(),
            _ => hook_claude(),
        },
        Some("init") => {
            install_instruction(&argv[1..]);
            let py = format!("{}/tok/tok.py", home());
            let st = Command::new("python3").arg(&py).arg("init").args(&argv[1..]).status();
            match st {
                Ok(s) => s.code().unwrap_or(1),
                Err(_) => {
                    println!("tok init needs python3 + {} (or add the hook manually:", py);
                    println!("  settings.json → hooks.PreToolUse += {{\"matcher\": \"Bash\", \
                              \"hooks\": [{{\"type\": \"command\", \"command\": \"tok hook claude\"}}]}})");
                    1
                }
            }
        }
        Some("pipe") => {
            let mut raw = String::new();
            let _ = std::io::stdin().read_to_string(&mut raw);
            let name = argv.get(1).map(|s| s.as_str()).unwrap_or("");
            if ["gradle", "maven", "pytest", "npm", "pip", "ffmpeg", "citrace"].contains(&name) {
                println!("{}", rules_filter(name, &raw, 0));
            } else {
                println!("{}", generic_filter(&raw, 0, max_lines_default()));
            }
            0
        }
        Some("proxy") => {
            if argv.len() < 2 {
                println!("tok proxy: no command");
                2
            } else {
                let (raw, code) = run_raw(&argv[1..]);
                print!("{}", raw);
                code
            }
        }
        Some("run") => {
            let mut cmd: &[String] = &argv[1..];
            if cmd.first().map(|s| s.as_str()) == Some("--") {
                cmd = &cmd[1..];
            }
            if cmd.is_empty() {
                println!("tok run: no command");
                2
            } else {
                let (raw, code) = run_raw(cmd);
                save_raw(cmd, &raw, code);
                let filtered = generic_filter(&raw, code, max_lines_default());
                track(cmd, &raw, &filtered);
                let display = session_dedup(cmd, &raw, code, &filtered).unwrap_or(filtered);
                println!("{}", display);
                code
            }
        }
        Some("read") => {
            let (out, code) = h_read(&argv[1..]);
            println!("{}", out);
            code
        }
        Some("discover") => discover(),
        Some(_) => dispatch(&argv),
    };
    std::process::exit(code);
}
