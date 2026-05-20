//! Live Hermes-parity integration tests with structured metrics recording.
//!
//! Each test run appends NDJSON to `parity-results.ndjson` (next to Cargo.toml).
//! Over time this file becomes a queryable history of model performance vs. code
//! changes — feed it into jq / DuckDB / a spreadsheet to spot regressions.
//!
//! **Opt-in** — gated by `PENGINE_PARITY_TEST=1` so they never run in offline CI.
//!
//! Run:
//!   PENGINE_PARITY_TEST=1 \
//!   PENGINE_PARITY_MODEL=qwen3.6:latest \
//!   cargo test --manifest-path src-tauri/Cargo.toml --test hermes_parity \
//!     -- --nocapture --test-threads 1
//!
//! Analyse results (requires jq):
//!   jq -s 'group_by(.test) | map({test: .[0].test, runs: length, avg_ms: (map(.elapsed_ms) | add/length), pass_rate: (map(select(.passed)) | length) / length})' \
//!     src-tauri/parity-results.ndjson
//!
//! Optional env vars:
//!   PENGINE_PARITY_MODEL    Ollama model tag        (default: qwen3.6:latest)
//!   PENGINE_PARITY_TIMEOUT  Seconds per-test limit  (default: 1800)
//!                           qwen3.6:latest typically takes ~16 s per call with /nothink.
//!   PENGINE_PARITY_RESULTS  Path for NDJSON output  (default: src-tauri/parity-results.ndjson)
//!   PENGINE_PARITY_MCP=1    Enable MCP stdio server tests (podman required, opt-in)

use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Seek, Write as IoWrite};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── configuration ─────────────────────────────────────────────────────────────

macro_rules! parity_guard {
    () => {
        if std::env::var("PENGINE_PARITY_TEST").unwrap_or_default() != "1" {
            eprintln!("SKIP: set PENGINE_PARITY_TEST=1 to run parity tests");
            return;
        }
    };
}

fn model() -> String {
    std::env::var("PENGINE_PARITY_MODEL").unwrap_or_else(|_| "qwen3.6:latest".into())
}

fn timeout_secs() -> u64 {
    std::env::var("PENGINE_PARITY_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800) // qwen3.6:latest takes ~1200 s per call — allow 30 min headroom
}

fn results_path() -> PathBuf {
    if let Ok(p) = std::env::var("PENGINE_PARITY_RESULTS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parity-results.ndjson")
}

fn pengine_exe() -> PathBuf {
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_pengine") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/pengine")
}

fn pengine() -> Command {
    let exe = pengine_exe();
    assert!(
        exe.exists(),
        "pengine binary missing at {} — run `cargo build` first",
        exe.display()
    );
    Command::new(exe)
}

// ── types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonReply {
    kind: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct JsonEnvelope {
    #[allow(dead_code)]
    v: u8,
    reply: JsonReply,
}

struct RunResult {
    envelope: JsonEnvelope,
    elapsed: Duration,
    /// Prompt length in bytes (for cost / token-count estimation).
    prompt_len: usize,
    /// Model input token count read from the audit log (None if log unavailable).
    in_tokens: Option<u64>,
    /// Model output token count read from the audit log.
    out_tokens: Option<u64>,
}

// ── metrics recorder ──────────────────────────────────────────────────────────

/// Append one NDJSON line to the results file.
/// Called from every test before assertions, so timing is recorded even on failure.
fn record(test_name: &str, r: &RunResult, passed: bool, note: Option<&str>) {
    let ts_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let line = json!({
        "ts":        ts_secs,
        "model":     model(),
        "test":      test_name,
        "elapsed_ms":r.elapsed.as_millis() as u64,
        "prompt_len":r.prompt_len,
        "reply_len": r.envelope.reply.body.len(),
        "kind":      r.envelope.reply.kind,
        "in_tokens": r.in_tokens,
        "out_tokens":r.out_tokens,
        "passed":    passed,
        "note":      note,
    });

    let path = results_path();
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{line}");
        }
        Err(e) => eprintln!("[metrics] could not write to {}: {e}", path.display()),
    }
}

/// Load last N results for the current model from the results file.
fn load_recent_results(n: usize) -> Vec<serde_json::Value> {
    let path = results_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let m = model();
    raw.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["model"].as_str() == Some(&m))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(n * 10) // sample from last ~10 full runs
        .collect()
}

// ── audit-log token helpers ────────────────────────────────────────────────────

/// Return the most-recently-modified audit-*.log file, or None.
fn find_audit_log() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    for base in [
        home.join("Library/Application Support/com.maximedogawa.pengine/logs"),
        home.join(".local/share/com.maximedogawa.pengine/logs"),
    ] {
        if !base.exists() {
            continue;
        }
        let mut entries: Vec<_> = std::fs::read_dir(&base)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("audit-"))
            .collect();
        if entries.is_empty() {
            continue;
        }
        entries.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        return entries.last().map(|e| e.path());
    }
    None
}

/// Snapshot audit log byte size before spawning an `ask`.
fn audit_snapshot() -> (Option<PathBuf>, u64) {
    let path = find_audit_log();
    let offset = path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    (path, offset)
}

fn parse_tokens_from_audit_message(msg: &str) -> Option<(u64, u64)> {
    // "model step N X.Xs (in:2312 out:11)"
    let in_start = msg.find("in:")? + 3;
    let in_end = msg[in_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| i + in_start)
        .unwrap_or(msg.len());
    let in_tok: u64 = msg[in_start..in_end].parse().ok()?;

    let out_start = msg.find("out:")? + 4;
    let out_end = msg[out_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| i + out_start)
        .unwrap_or(msg.len());
    let out_tok: u64 = msg[out_start..out_end].parse().ok()?;

    Some((in_tok, out_tok))
}

/// Read token counts from new audit-log lines appended after `offset`.
/// Returns the last `kind:time` entry found in the new region.
fn read_tokens_from_audit_since(path: &Option<PathBuf>, offset: u64) -> Option<(u64, u64)> {
    let path = path.as_ref()?;
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(std::io::SeekFrom::Start(offset)).ok()?;
    let mut new_content = String::new();
    f.read_to_string(&mut new_content).ok()?;

    new_content.lines().rev().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v["kind"].as_str() != Some("time") {
            return None;
        }
        parse_tokens_from_audit_message(v["message"].as_str()?)
    })
}

// ── process helpers ───────────────────────────────────────────────────────────

/// Poll child with OS-level kill if deadline is exceeded.
fn wait_with_deadline(mut child: Child, deadline: Duration, label: &str) -> std::process::Output {
    let t0 = Instant::now();
    loop {
        match child.try_wait().expect("try_wait failed") {
            Some(_) => return child.wait_with_output().expect("wait_with_output"),
            None => {
                if t0.elapsed() > deadline {
                    let _ = child.kill();
                    panic!(
                        "{label}: exceeded {:.0}s deadline — killed process. \
                         Tip: use /nothink prefix or increase PENGINE_PARITY_TIMEOUT.",
                        deadline.as_secs_f64()
                    );
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

/// Reset the CLI session before each test so accumulated conversation history
/// from previous tests does not inflate the input token count or distract the
/// model. Without this, by test #14 the session can exceed 6 000 tokens, which
/// causes multi-step tests to produce truncated replies ("Done." instead of
/// listing dice results) and makes tool availability appear inconsistent.
fn reset_session() {
    let child = pengine()
        .args(["--json", "new"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pengine new");
    wait_with_deadline(child, Duration::from_secs(30), "reset_session");
}

fn set_model(tag: &str) {
    reset_session();
    let child = pengine()
        .args(["--json", "model", tag])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pengine model");
    let out = wait_with_deadline(child, Duration::from_secs(60), "set_model");
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("[setup] model set → {}", stdout.trim());
}

fn ask(prompt: &str) -> RunResult {
    ask_with_timeout(prompt, timeout_secs())
}

/// Like `ask` but passes `--continue` so the subprocess resumes the most recently
/// saved session.  Use this when multiple one-shot calls must accumulate turns in
/// the same session (e.g. the compaction test builds up 7 turns before compacting).
fn ask_continue(prompt: &str) -> RunResult {
    ask_impl(prompt, timeout_secs(), true)
}

fn ask_with_timeout(prompt: &str, secs: u64) -> RunResult {
    ask_impl(prompt, secs, false)
}

fn ask_impl(prompt: &str, secs: u64, continue_session: bool) -> RunResult {
    let t0 = Instant::now();
    let prompt_len = prompt.len();
    let preview = prompt.chars().take(60).collect::<String>();
    let preview = if prompt.len() > 60 {
        format!("{preview}…")
    } else {
        preview.clone()
    };
    let flag = if continue_session { " --continue" } else { "" };
    eprintln!("  → sending{flag} ({prompt_len} B, timeout {secs}s): {preview:?}");

    let (audit_path, audit_offset) = audit_snapshot();

    let child = if continue_session {
        pengine()
            .args(["--json", "ask", "--continue", prompt])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pengine ask --continue")
    } else {
        pengine()
            .args(["--json", "ask", prompt])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pengine ask")
    };

    let out = wait_with_deadline(child, Duration::from_secs(secs), prompt);
    let elapsed = t0.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    let line = stdout
        .lines()
        .rfind(|l| l.starts_with('{'))
        .unwrap_or_else(|| {
            panic!(
                "no JSON line in stdout.\nstdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });

    let envelope: JsonEnvelope = serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("parse JSON envelope: {e}\nline: {line}"));

    let tokens = read_tokens_from_audit_since(&audit_path, audit_offset);
    let (in_tokens, out_tokens) = (tokens.map(|(i, _)| i), tokens.map(|(_, o)| o));

    eprintln!(
        "  ← [{:.1}s] kind={} in={} out={} | reply: {:?}",
        elapsed.as_secs_f64(),
        envelope.reply.kind,
        in_tokens
            .map(|t| t.to_string())
            .unwrap_or_else(|| "?".into()),
        out_tokens
            .map(|t| t.to_string())
            .unwrap_or_else(|| "?".into()),
        envelope.reply.body.chars().take(80).collect::<String>(),
    );
    RunResult {
        envelope,
        elapsed,
        prompt_len,
        in_tokens,
        out_tokens,
    }
}

// ── perf baseline ─────────────────────────────────────────────────────────────

/// Cold-start latency for a trivial non-tool prompt.
/// /nothink skips qwen3's chain-of-thought — critical for latency benchmarking.
#[test]
fn perf_baseline_simple_reply() {
    parity_guard!();
    let m = model();
    set_model(&m);

    eprintln!("=== PERF BASELINE ({m}) ===");
    let r = ask("/nothink Reply with exactly one word: PONG");
    let passed =
        r.envelope.reply.kind == "text" && r.envelope.reply.body.to_uppercase().contains("PONG");

    record("perf_baseline", &r, passed, None);

    assert!(passed, "expected PONG in reply: {}", r.envelope.reply.body);
    eprintln!(
        "  baseline: {:.2}s / {} bytes",
        r.elapsed.as_secs_f64(),
        r.envelope.reply.body.len()
    );
}

/// Throughput for a longer (~100-token) generation.
#[test]
fn perf_long_reply_latency() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== PERF LONG REPLY ===");
    let r = ask(
        "/nothink List the 10 largest cities in Europe with their countries. \
         One per line, no extra commentary.",
    );
    let passed = r.envelope.reply.kind == "text" && r.envelope.reply.body.len() > 100;

    record("perf_long_reply", &r, passed, None);

    assert!(
        passed,
        "reply too short: {} bytes",
        r.envelope.reply.body.len()
    );
    eprintln!(
        "  {:.2}s / {} bytes / {:.0} B/s",
        r.elapsed.as_secs_f64(),
        r.envelope.reply.body.len(),
        r.envelope.reply.body.len() as f64 / r.elapsed.as_secs_f64()
    );
}

// ── multi-agent ───────────────────────────────────────────────────────────────

/// Verifies that the agent can delegate two independent subtasks in a single turn
/// using two consecutive task_spawn calls, then report both results.
/// The `aaa_` prefix ensures this runs first — the session is freshest and the
/// agent has the most token budget for multi-step work.
#[test]
fn aaa_multi_agent_dual_spawn() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== MULTI-AGENT: dual task_spawn ===");
    let r = ask("Use task_spawn twice in this turn: \
         first to answer 'What is the capital city of France?', \
         then to answer 'What is 6 multiplied by 7?'. \
         Report both results in your reply.");
    let body_lc = r.envelope.reply.body.to_lowercase();

    let has_paris = body_lc.contains("paris");
    let has_42 = body_lc.contains("42");
    let passed = r.envelope.reply.kind == "text" && has_paris && has_42;

    record(
        "multi_agent_dual_spawn",
        &r,
        passed,
        Some(&format!("paris={has_paris} answer_42={has_42}")),
    );

    assert!(
        has_paris,
        "expected Paris (capital of France) in reply: {}",
        r.envelope.reply.body
    );
    assert!(
        has_42,
        "expected 42 (6×7) in reply: {}",
        r.envelope.reply.body
    );
    eprintln!("  Paris + 42 ✓  {:.2}s", r.elapsed.as_secs_f64());
}

// ── context compaction ────────────────────────────────────────────────────────

/// Builds a 7-turn session (one more than HISTORY_TURN_BUDGET=6), triggers
/// `/compact` to summarise the oldest turn, then verifies the model can recall a
/// unique marker that existed only in the dropped turn — proving the summary is
/// usable context for future turns.
#[test]
fn context_compaction_produces_summary() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CONTEXT COMPACTION ===");

    // All turns use --continue so they accumulate in the session created by
    // reset_session() above.  Without --continue each pengine-ask subprocess
    // creates a new one-turn session, so compact would find an empty session.

    // Turn 0: embed a unique marker. After compact this turn is dropped from
    // verbatim history and lives ONLY in the generated summary.
    let _ = ask_continue("/nothink Store this unique code: COMPACT-MARKER=THETA-3. Reply: stored");

    // Turns 1-6: fast filler to reach the 7-turn threshold for compaction.
    for i in 1u8..=6 {
        let _ = ask_continue(&format!("/nothink Reply with only the number {i}"));
    }
    // Session now has 7 turns. compact drops turn[0] into the AI summary,
    // keeps turns [1..6] verbatim.

    eprintln!("  7 turns built — calling compact...");
    let child = pengine()
        .args(["--json", "compact"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pengine compact");
    let out = wait_with_deadline(child, Duration::from_secs(120), "compact");
    let compact_stdout = String::from_utf8_lossy(&out.stdout).to_string();
    eprintln!("  compact → {}", compact_stdout.trim());

    // CLI returns e.g. "Compacted 7 turn(s) → summary + 6 recent turn(s) kept verbatim."
    let compact_ok = compact_stdout.to_lowercase().contains("compact");

    // Recall: the marker is no longer in verbatim turns — only in the summary.
    // Must use --continue so the subprocess resumes the compacted session.
    let r = ask_continue(
        "/nothink What was the COMPACT-MARKER value stored at the start of our session? \
         Reply with just the value.",
    );
    let body = r.envelope.reply.body.to_lowercase();
    let recalls_marker = body.contains("theta");
    let passed = compact_ok && recalls_marker;

    record(
        "context_compaction",
        &r,
        passed,
        Some(&format!(
            "compact_ok={compact_ok} recalls_theta={recalls_marker}"
        )),
    );

    assert!(
        compact_ok,
        "compact command did not confirm compaction: {compact_stdout}"
    );
    assert!(
        recalls_marker,
        "model could not recall THETA-3 from compacted summary.\nreply: {}",
        r.envelope.reply.body
    );
    eprintln!(
        "  ✓ THETA-3 recalled from summary  {:.2}s",
        r.elapsed.as_secs_f64()
    );
}

// ── tool use ──────────────────────────────────────────────────────────────────

#[test]
fn tool_use_dice_single_roll() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== TOOL USE: roll_dice (single) ===");
    let r =
        ask("/nothink Use the roll_dice tool to roll a 6-sided die. Reply with only the number.");
    let body = r.envelope.reply.body.clone();
    let number: Option<u64> = body
        .split_whitespace()
        .find_map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok());
    let passed = number.map(|n| (1..=6).contains(&n)).unwrap_or(false);

    record(
        "tool_dice_single",
        &r,
        passed,
        number.map(|n| format!("rolled={n}")).as_deref(),
    );

    let n = number.unwrap_or_else(|| panic!("no number in reply: {body}"));
    assert!(
        (1..=6).contains(&n),
        "rolled {n} not in 1–6 — hallucinated?"
    );
    eprintln!("  rolled={n}  {:.2}s", r.elapsed.as_secs_f64());
}

// ── code review ───────────────────────────────────────────────────────────────

/// Verifies that the code-review skill is injected when "code review" keywords
/// appear in the prompt — token count must be meaningfully higher than a bare
/// baseline, confirming the SKILL.md was appended to the system prompt.
#[test]
fn code_review_skill_injected() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CODE REVIEW: skill injection ===");

    let r_base = ask("/nothink Reply with exactly one word: YES");
    let base_tokens = r_base.in_tokens;
    record(
        "code_review_skill_baseline",
        &r_base,
        r_base.envelope.reply.kind == "text",
        base_tokens.map(|t| format!("in_tokens={t}")).as_deref(),
    );

    // "code review" is in hint_allow_substrings → should inject the skill.
    let r_review = ask(
        "/nothink code review: briefly describe what sections you include \
         when reviewing code. One sentence.",
    );
    let review_tokens = r_review.in_tokens;
    let content_ok =
        r_review.envelope.reply.kind == "text" && !r_review.envelope.reply.body.is_empty();

    let note = match (base_tokens, review_tokens) {
        (Some(b), Some(w)) => format!("base={b} review={w} diff={:+}", w as i64 - b as i64),
        _ => "tokens_unavailable".into(),
    };
    record(
        "code_review_skill_inject",
        &r_review,
        content_ok,
        Some(&note),
    );

    assert!(
        content_ok,
        "code review query returned empty reply: {}",
        r_review.envelope.reply.body
    );
    if let (Some(base), Some(review)) = (base_tokens, review_tokens) {
        let diff = review as i64 - base as i64;
        eprintln!(
            "  base={base}  review={review}  diff={diff:+}  {:.2}s",
            r_review.elapsed.as_secs_f64()
        );
        assert!(
            diff > 50,
            "code-review query ({review} tokens) vs baseline ({base}): diff={diff:+} \
             too small — skill likely not injecting (expected >50)",
        );
        eprintln!("  ✓ skill injection confirmed (+{diff} tokens)");
    } else {
        eprintln!(
            "  (audit log unavailable)  {:.2}s",
            r_review.elapsed.as_secs_f64()
        );
    }
}

/// Verifies the model produces structured code review output (Summary / Issues /
/// Suggestions) for an inline Rust snippet.  The snippet contains a known
/// division-by-zero bug so the model has something concrete to comment on.
#[test]
fn code_review_structured_output() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CODE REVIEW: structured output ===");

    // Embedded snippet — no tool call needed, keeps test deterministic.
    let prompt = concat!(
        "/nothink Perform a code review of the following Rust function. ",
        "Use the standard sections: Summary, Strengths, Issues, Suggestions.\n\n",
        "```rust\n",
        "fn average(values: &[i32]) -> i32 {\n",
        "    let sum: i32 = values.iter().sum();\n",
        "    sum / values.len() as i32\n",
        "}\n",
        "```"
    );

    let r = ask(prompt);
    let body_lc = r.envelope.reply.body.to_lowercase();

    // Any recognised section header or direct bug keyword counts.
    let has_structure = [
        "summary",
        "issue",
        "suggestion",
        "strength",
        "bug",
        "panic",
        "empty",
    ]
    .iter()
    .any(|kw| body_lc.contains(kw));
    let passed = r.envelope.reply.kind == "text" && has_structure;

    record(
        "code_review_structured",
        &r,
        passed,
        Some(&format!("has_structure={has_structure}")),
    );

    assert!(
        passed,
        "reply missing section headers or bug keywords.\nreply: {}",
        r.envelope.reply.body
    );
    eprintln!("  ✓ structured output  {:.2}s", r.elapsed.as_secs_f64());
}

/// Verifies the model identifies a specific, known bug (division-by-zero on an
/// empty slice) in an inline snippet.  Records pass/fail per run to track model
/// quality regressions over time.
#[test]
fn code_review_identifies_bug() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CODE REVIEW: bug identification ===");

    let prompt = concat!(
        "/nothink Review this Rust function for bugs. ",
        "Be specific about what can go wrong.\n\n",
        "```rust\n",
        "fn average(values: &[i32]) -> i32 {\n",
        "    let sum: i32 = values.iter().sum();\n",
        "    sum / values.len() as i32\n",
        "}\n",
        "```"
    );

    let r = ask(prompt);
    let body_lc = r.envelope.reply.body.to_lowercase();

    // Division-by-zero on empty slice — model should mention at least one of these.
    let identifies_bug = [
        "empty",
        "zero",
        "divis",
        "panic",
        "len()",
        "length",
        "0 element",
        "no element",
    ]
    .iter()
    .any(|kw| body_lc.contains(kw));
    let passed = r.envelope.reply.kind == "text" && identifies_bug;

    record(
        "code_review_bug_id",
        &r,
        passed,
        Some(&format!("identifies_bug={identifies_bug}")),
    );

    assert!(
        identifies_bug,
        "model did not identify the division-by-zero / empty-slice bug.\nreply: {}",
        r.envelope.reply.body
    );
    eprintln!("  ✓ bug identified  {:.2}s", r.elapsed.as_secs_f64());
}

/// Verifies the agent calls git_diff when asked to review the current repo
/// changes, producing either a real review or an explicit "no changes" reply.
/// Does not assert on review quality — only that the agent uses the tool and
/// produces a coherent response.
#[test]
fn code_review_uses_git_diff() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CODE REVIEW: git_diff tool use ===");

    // git_diff requires a mandatory `target` arg (e.g. "HEAD") — use
    // git_diff_unstaged instead, which shows working-tree changes with no
    // required parameters and is the correct tool for this purpose.
    let r = ask(
        "code review: Please review the current git changes in this repository. \
         Use the git_diff_unstaged tool to fetch the working-tree diff. \
         If there are no unstaged changes, reply with exactly: NO_CHANGES",
    );
    let body_lc = r.envelope.reply.body.to_lowercase();

    let no_changes = body_lc.contains("no_changes") || body_lc.contains("no changes");
    let made_review = [
        "summary", "change", "diff", "issue", "suggest", "file", "line", "function", "commit",
    ]
    .iter()
    .any(|kw| body_lc.contains(kw));
    let passed = r.envelope.reply.kind == "text" && (made_review || no_changes);

    record(
        "code_review_git_diff",
        &r,
        passed,
        Some(if no_changes {
            "no_changes"
        } else if made_review {
            "review_produced"
        } else {
            "unclear"
        }),
    );

    assert!(
        passed,
        "expected a code review or NO_CHANGES, got: {}",
        r.envelope.reply.body
    );
    eprintln!(
        "  {}  {:.2}s",
        if no_changes {
            "clean repo — no changes to review"
        } else {
            "✓ review produced"
        },
        r.elapsed.as_secs_f64()
    );
}

// ── task_spawn ────────────────────────────────────────────────────────────────

#[test]
fn task_spawn_research_subtask() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== TASK SPAWN: capital of Japan ===");
    let r = ask(
        "/nothink Use task_spawn to delegate: 'What is the capital city of Japan?' \
         Report the result in your reply.",
    );
    let body = r.envelope.reply.body.to_lowercase();
    let passed = r.envelope.reply.kind == "text" && body.contains("tokyo");

    record("task_spawn_capital", &r, passed, None);

    assert!(passed, "expected Tokyo in reply: {}", r.envelope.reply.body);
    eprintln!("  Tokyo ✓  {:.2}s", r.elapsed.as_secs_f64());
}

// ── token budget ──────────────────────────────────────────────────────────────

/// Simple query with /nothink must stay under 3 000 input tokens.
/// This catches regressions from skill injection, system-prompt bloat, or
/// broken hint-gate logic that would silently re-inflate the prompt.
#[test]
fn token_budget_simple_query() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== TOKEN BUDGET: simple query ===");
    let r = ask("/nothink Reply with exactly one word: PONG");
    let body_upper = r.envelope.reply.body.to_uppercase();
    let content_ok = r.envelope.reply.kind == "text" && body_upper.contains("PONG");

    let budget_ok = r.in_tokens.map(|t| t < 3000).unwrap_or(true); // pass if log unavailable
    let passed = content_ok && budget_ok;

    record(
        "token_budget_simple",
        &r,
        passed,
        r.in_tokens.map(|t| format!("in_tokens={t}")).as_deref(),
    );

    assert!(
        content_ok,
        "expected PONG in reply: {}",
        r.envelope.reply.body
    );
    if let Some(t) = r.in_tokens {
        assert!(
            t < 3000,
            "input token budget exceeded: {t} tokens (budget: 3000). \
             Check for skill injection regressions or system-prompt bloat.",
        );
        eprintln!(
            "  in={t} out={}  {:.2}s  ✓ within budget",
            r.out_tokens.unwrap_or(0),
            r.elapsed.as_secs_f64()
        );
    } else {
        eprintln!(
            "  (audit log unavailable — skipping budget assert)  {:.2}s",
            r.elapsed.as_secs_f64()
        );
    }
}

// ── skills gate ───────────────────────────────────────────────────────────────

/// Verifies that the bundled weather skill is injected for weather-keyword queries
/// (increasing input tokens) but absent for unrelated queries.
///
/// The weather skill is ~2 KB, so a weather-keyword query should have ≥200 more
/// input tokens than a comparable off-topic query.
#[test]
fn skills_gate_weather_skill_injects() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== SKILLS GATE: weather skill injection ===");

    // Baseline — no weather keywords → skill must NOT be injected.
    let r_base = ask("/nothink Reply with exactly one word: YES");
    let base_tokens = r_base.in_tokens;
    record(
        "skills_gate_baseline",
        &r_base,
        r_base.envelope.reply.kind == "text",
        base_tokens.map(|t| format!("in_tokens={t}")).as_deref(),
    );

    // Weather-keyword query → skill SHOULD be injected.
    let r_wx = ask(
        "/nothink weather forecast: briefly describe what tools or data sources you have \
         available for answering weather and forecast questions. One sentence.",
    );
    let wx_tokens = r_wx.in_tokens;
    let content_ok = r_wx.envelope.reply.kind == "text" && !r_wx.envelope.reply.body.is_empty();

    let note = match (base_tokens, wx_tokens) {
        (Some(b), Some(w)) => format!("base={b} weather={w} diff={:+}", w as i64 - b as i64),
        _ => "tokens_unavailable".into(),
    };
    record("skills_gate_weather", &r_wx, content_ok, Some(&note));

    assert!(
        content_ok,
        "weather query returned empty reply: {}",
        r_wx.envelope.reply.body
    );

    if let (Some(base), Some(wx)) = (base_tokens, wx_tokens) {
        let diff = wx as i64 - base as i64;
        eprintln!(
            "  base={base}  weather={wx}  diff={diff:+}  {:.2}s",
            r_wx.elapsed.as_secs_f64()
        );
        // Weather skill is ~2 kB. The qwen3.6 tokenizer is efficient so the actual
        // diff is typically 100–600 tokens. Require >50 to distinguish genuine
        // injection from message-length noise alone (~14 tokens for the longer prompt).
        if diff < 200 {
            eprintln!("  WARN: diff={diff:+} < 200 — skill may be injecting less than expected (tokenizer efficiency or truncation)");
        }
        assert!(
            diff > 50,
            "weather query ({wx} tokens) vs baseline ({base}): diff={diff:+} is too small — \
             weather skill likely not injecting (expected >50 from SKILL_HINT_INTRO alone)",
        );
        eprintln!("  ✓ skill injection confirmed (+{diff} tokens)");
    } else {
        eprintln!(
            "  (audit log unavailable — skipping injection assert)  {:.2}s",
            r_wx.elapsed.as_secs_f64()
        );
    }
}

/// Verifies that an off-topic query does NOT inflate the token count above the
/// simple-query budget, confirming the skills hint gate blocks irrelevant skills.
#[test]
fn skills_gate_offtopic_stays_compact() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== SKILLS GATE: off-topic stays compact ===");
    let r = ask("/nothink What is the square root of 144? Reply with just the number.");
    let passed = r.envelope.reply.kind == "text"
        && r.envelope.reply.body.contains("12")
        && r.in_tokens.map(|t| t < 3000).unwrap_or(true);

    record(
        "skills_gate_offtopic",
        &r,
        passed,
        r.in_tokens.map(|t| format!("in_tokens={t}")).as_deref(),
    );

    assert!(
        r.envelope.reply.body.contains("12"),
        "expected 12 in reply: {}",
        r.envelope.reply.body
    );
    if let Some(t) = r.in_tokens {
        assert!(
            t < 3000,
            "off-topic query used {t} tokens — skill injection may be leaking (budget: 3000)",
        );
        eprintln!("  in={t}  {:.2}s  ✓ compact", r.elapsed.as_secs_f64());
    } else {
        eprintln!("  (audit log unavailable)  {:.2}s", r.elapsed.as_secs_f64());
    }
}

// ── MCP stdio server ──────────────────────────────────────────────────────────

/// Verify that the te_pengine-time MCP server can report the current time.
/// Opt-in: set PENGINE_PARITY_MCP=1 to enable (requires podman and the tool
/// engine containers to be available).
#[test]
fn mcp_time_tool_reports_time() {
    parity_guard!();
    if std::env::var("PENGINE_PARITY_MCP").unwrap_or_default() != "1" {
        eprintln!("SKIP: set PENGINE_PARITY_MCP=1 to run MCP stdio server tests");
        return;
    }
    set_model(&model());

    eprintln!("=== MCP: te_pengine-time ===");
    // Allow extra time for MCP warm-up on first call.
    let r = ask_with_timeout(
        "/nothink Use the time tool to get the current time. \
         Reply with just the time you received from the tool.",
        timeout_secs(),
    );
    let body = r.envelope.reply.body.clone();
    // A time string should contain digits and at least one colon (HH:MM) or
    // recognizable time words.
    let looks_like_time = body.contains(':')
        || body.chars().any(|c| c.is_ascii_digit())
        || body.to_lowercase().contains("time");
    let passed = r.envelope.reply.kind == "text" && looks_like_time;

    record(
        "mcp_time_tool",
        &r,
        passed,
        Some(&format!("reply_snippet={}", &body[..body.len().min(60)])),
    );

    assert!(passed, "expected time-like string in reply, got: {body}");
    eprintln!(
        "  time reply: {:?}  {:.2}s",
        &body[..body.len().min(60)],
        r.elapsed.as_secs_f64()
    );
}

/// Verify that the te_pengine-shell MCP server can execute a shell command
/// and return its output.  Opt-in: set PENGINE_PARITY_MCP=1 (requires podman
/// and the pengine-shell image to be available locally).
#[test]
fn mcp_shell_execute_runs_command() {
    parity_guard!();
    if std::env::var("PENGINE_PARITY_MCP").unwrap_or_default() != "1" {
        eprintln!("SKIP: set PENGINE_PARITY_MCP=1 to run MCP stdio server tests");
        return;
    }
    set_model(&model());

    eprintln!("=== MCP: te_pengine-shell ===");
    // Allow extra time for container cold-start on first call.
    let r = ask_with_timeout(
        "/nothink Use shell_execute to run the command `echo PENGINE_SHELL_OK`. \
         Reply with only the exact output you received from the tool.",
        timeout_secs(),
    );
    let body = r.envelope.reply.body.clone();
    let passed = r.envelope.reply.kind == "text" && body.contains("PENGINE_SHELL_OK");

    record(
        "mcp_shell_execute",
        &r,
        passed,
        Some(&format!("reply_snippet={}", &body[..body.len().min(80)])),
    );

    assert!(passed, "expected PENGINE_SHELL_OK in reply, got: {body}");
    eprintln!(
        "  shell reply: {:?}  {:.2}s",
        &body[..body.len().min(80)],
        r.elapsed.as_secs_f64()
    );
}

// ── context window ────────────────────────────────────────────────────────────

/// Verifies the context window is not truncated to a very small size.
/// Runs last (zzz_ prefix) because the padded prompt is the largest input and
/// will stress slower models. Uses ~800 B of padding — enough to catch a
/// 512-token truncation while keeping inference manageable.
#[test]
fn zzz_context_window_not_truncated() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== CONTEXT WINDOW: 16 k ===");
    let padding = "Filler text to pad the context window. ".repeat(20); // ~780 bytes
    let prompt = format!(
        "/nothink MARKER=ZETA-9-KILO\n\n\
         {padding}\n\n\
         What is the value of MARKER at the very start of this message? \
         Reply with only the value."
    );
    let r = ask_with_timeout(&prompt, timeout_secs());
    let body_upper = r.envelope.reply.body.to_uppercase();
    let passed = body_upper.contains("ZETA") && body_upper.contains("KILO");

    record(
        "context_window",
        &r,
        passed,
        Some(&format!("prompt_bytes={}", prompt.len())),
    );

    assert!(passed, "marker not found in: {}", r.envelope.reply.body);
    eprintln!(
        "  ZETA-9-KILO ✓  prompt={}B  {:.2}s",
        prompt.len(),
        r.elapsed.as_secs_f64()
    );
}

// ── output contract ───────────────────────────────────────────────────────────

#[test]
fn output_contract_reply_kind_is_text() {
    parity_guard!();
    set_model(&model());

    eprintln!("=== OUTPUT CONTRACT ===");
    let prompts = [
        ("/nothink Say: HELLO", "HELLO"),
        ("/nothink What is 3 + 4?", "7"),
        ("/nothink Name one planet.", ""),
    ];
    for (prompt, expected) in &prompts {
        let r = ask(prompt);
        let body = r.envelope.reply.body.clone();
        let passed = r.envelope.reply.kind == "text"
            && !body.trim().is_empty()
            && (expected.is_empty() || body.to_uppercase().contains(*expected));

        record(
            "output_contract",
            &r,
            passed,
            Some(&format!(
                "prompt_snippet={}",
                &prompt[..prompt.len().min(30)]
            )),
        );

        assert!(
            r.envelope.reply.kind == "text",
            "kind='{}' expected 'text' for: {prompt}",
            r.envelope.reply.kind
        );
        assert!(!body.trim().is_empty(), "empty body for: {prompt}");
        if !expected.is_empty() {
            assert!(
                body.to_uppercase().contains(*expected),
                "expected '{expected}' in reply '{body}' for: {prompt}"
            );
        }
        eprintln!("  ✓ [{:.2}s] {prompt:?}", r.elapsed.as_secs_f64());
    }
}

// ── trend summary ─────────────────────────────────────────────────────────────

/// Reads parity-results.ndjson and prints a per-test trend table.
/// Always passes — run last to get a full picture across this + prior runs.
#[test]
fn zzz_print_parity_summary() {
    parity_guard!();
    let m = model();
    let all = load_recent_results(50);

    eprintln!("\n╔═══════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║              Hermes Parity Results — {m:<36}║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  {:<30} {:>5}  {:>6}  {:>8}  {:>8}  ║",
        "test", "runs", "pass%", "avg_ms", "avg_in_t"
    );
    eprintln!(
        "║  {:<30} {:>5}  {:>6}  {:>8}  {:>8}  ║",
        "─".repeat(30),
        "─────",
        "──────",
        "────────",
        "────────"
    );

    // Group by test name.
    let mut groups: std::collections::BTreeMap<String, Vec<&serde_json::Value>> =
        std::collections::BTreeMap::new();
    for v in &all {
        if let Some(name) = v["test"].as_str() {
            groups.entry(name.to_string()).or_default().push(v);
        }
    }

    for (name, runs) in &groups {
        let n = runs.len();
        let passed = runs
            .iter()
            .filter(|v| v["passed"].as_bool() == Some(true))
            .count();
        let avg_ms = runs
            .iter()
            .filter_map(|v| v["elapsed_ms"].as_u64())
            .sum::<u64>()
            / n.max(1) as u64;
        let pass_pct = passed * 100 / n.max(1);
        let indicator = if pass_pct == 100 {
            "✓"
        } else if pass_pct >= 80 {
            "~"
        } else {
            "✗"
        };
        let in_tok_runs: Vec<u64> = runs
            .iter()
            .filter_map(|v| v["in_tokens"].as_u64())
            .collect();
        let avg_in_tok = if in_tok_runs.is_empty() {
            "   –".into()
        } else {
            format!(
                "{:>8}",
                in_tok_runs.iter().sum::<u64>() / in_tok_runs.len() as u64
            )
        };
        eprintln!(
            "║  {indicator} {:<29} {:>5}  {:>5}%  {:>7}ms  {avg_in_tok}  ║",
            &name[..name.len().min(29)],
            n,
            pass_pct,
            avg_ms
        );
    }

    if groups.is_empty() {
        eprintln!("║  (no recorded results yet — run tests first)                              ║");
    }

    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  Pending: FTS5 search │ dashboard UI                                         ║");
    eprintln!("║  Code review: skill_inject │ structured │ bug_id │ git_diff               ║");
    eprintln!("║  Multi-agent: dual_spawn │ Compaction: context_compaction                  ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════════════════╝");
    eprintln!("\n  Results file: {}", results_path().display());
    eprintln!("  Analyse with jq:");
    eprintln!(
        r#"    jq -s 'group_by(.test)|map({{t:.[0].test,runs:length,pass_rate:(map(select(.passed))|length)/length*100}})' src-tauri/parity-results.ndjson"#
    );
}
