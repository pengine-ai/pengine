//! CLI reply envelope and output sinks.
//!
//! Handlers produce [`CliReply`] values; rendering (ANSI, Markdown fences,
//! chunking) belongs to sinks. This keeps handlers transport-agnostic and
//! lets a single change to `TelegramSink` affect every reply at once.

use serde::Serialize;
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

/// What kind of block the body represents. Sinks decide the concrete
/// rendering (ANSI color, code fence language, etc.).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReplyKind {
    /// Plain prose.
    Text,
    /// Pre-formatted code / command output. `lang` is the fence hint.
    CodeBlock { lang: String },
    /// Unified diff, pre-formatted by the producing tool (e.g. `git diff`).
    Diff,
    /// Log stream chunk; rendered as a bash code block.
    Log,
    /// Error message; rendered red on terminals, plain on Telegram.
    Error,
}

/// One user-visible unit of output. Handlers return these; sinks render them.
#[derive(Debug, Clone, Serialize)]
pub struct CliReply {
    #[serde(flatten)]
    pub kind: ReplyKind,
    pub body: String,
}

impl CliReply {
    pub fn text(body: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::Text,
            body: body.into(),
        }
    }

    pub fn error(body: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::Error,
            body: body.into(),
        }
    }

    pub fn code(lang: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::CodeBlock { lang: lang.into() },
            body: body.into(),
        }
    }

    pub fn diff(body: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::Diff,
            body: body.into(),
        }
    }

    pub fn log(body: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::Log,
            body: body.into(),
        }
    }
}

/// Versioned JSON envelope so scripts can pin against `v`.
#[derive(Debug, Clone, Serialize)]
pub struct JsonEnvelope<'a> {
    pub v: u32,
    pub reply: &'a CliReply,
}

/// Rendering target. Implementers must be thread-safe for later FanOut usage.
pub trait OutputSink: Send + Sync {
    fn render(&self, reply: &CliReply);
}

/// Default: ANSI colors on TTYs, plain text otherwise. Prompt lines ("user@pengine:~$")
/// are written by the caller before invoking `render`, not by the sink itself.
pub struct TerminalSink {
    color: bool,
}

impl TerminalSink {
    pub fn new() -> Self {
        Self {
            color: is_terminal_stdout(),
        }
    }

    #[cfg(test)]
    pub fn plain() -> Self {
        Self { color: false }
    }
}

impl Default for TerminalSink {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputSink for TerminalSink {
    fn render(&self, reply: &CliReply) {
        match &reply.kind {
            ReplyKind::Text => println!("{}", reply.body),
            ReplyKind::Error => {
                if self.color {
                    eprintln!("\x1b[31m{}\x1b[0m", reply.body);
                } else {
                    eprintln!("{}", reply.body);
                }
            }
            ReplyKind::CodeBlock { .. } | ReplyKind::Log => {
                // Print raw — terminal rendering speaks for itself without fences.
                println!("{}", reply.body);
            }
            ReplyKind::Diff => {
                if self.color {
                    print_diff_with_ansi(&reply.body);
                } else {
                    println!("{}", reply.body);
                }
            }
        }
    }
}

/// Emit `{"v":1, "kind": "...", "body": "..."}` one reply per line.
pub struct JsonSink;

impl OutputSink for JsonSink {
    fn render(&self, reply: &CliReply) {
        let env = JsonEnvelope { v: 1, reply };
        match serde_json::to_string(&env) {
            Ok(line) => println!("{line}"),
            Err(e) => eprintln!("{{\"v\":1,\"kind\":\"error\",\"body\":\"json encode: {e}\"}}"),
        }
    }
}

fn print_diff_with_ansi(body: &str) {
    for line in body.lines() {
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
            println!("\x1b[1;36m{line}\x1b[0m"); // cyan, bold for headers
        } else if line.starts_with('+') {
            println!("\x1b[32m{line}\x1b[0m"); // green
        } else if line.starts_with('-') {
            println!("\x1b[31m{line}\x1b[0m"); // red
        } else {
            println!("{line}");
        }
    }
}

fn is_terminal_stdout() -> bool {
    // Avoids pulling a new crate; the file-descriptor check is enough for color gating.
    #[cfg(unix)]
    unsafe {
        // SAFETY: `isatty` takes a raw fd and has no memory effects.
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
        isatty(1) == 1
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Where a reply is being printed. Controls prefix/continuation layout.
#[derive(Debug, Clone, Copy)]
pub enum RenderStyle {
    /// One-shot: print as-is.
    Plain,
    /// Interactive REPL: `  ⎿  ` first-line prefix, 5-space continuation.
    ReplIndent,
}

const REPL_FIRST_PREFIX: &str = "  \x1b[2m⎿\x1b[0m  ";
const REPL_FIRST_PREFIX_PLAIN: &str = "  ⎿  ";
const REPL_CONT_PREFIX: &str = "     ";

/// Central rendering helper. Handles diff-fence splitting for `Text` replies
/// in REPL mode so ` ```diff ``` ` blocks get coloured inline.
pub fn render_reply(sink: &dyn OutputSink, reply: &CliReply, style: RenderStyle) {
    match style {
        RenderStyle::Plain => sink.render(reply),
        RenderStyle::ReplIndent => render_reply_indented(sink, reply),
    }
}

fn render_reply_indented(sink: &dyn OutputSink, reply: &CliReply) {
    match &reply.kind {
        ReplyKind::Text => {
            let blocks = split_text_into_blocks(&reply.body);
            for (i, part) in blocks.iter().enumerate() {
                let prefix = if i == 0 {
                    FirstPrefix::Repl
                } else {
                    FirstPrefix::None
                };
                render_with_prefix(sink, part, prefix);
            }
        }
        _ => render_with_prefix(sink, reply, FirstPrefix::Repl),
    }
}

#[derive(Clone, Copy)]
enum FirstPrefix {
    /// Indent the first line with `  ⎿  ` (or plain equivalent if no TTY).
    Repl,
    /// No first-line prefix; still indent continuation lines (for the 2nd+ block in a split reply).
    None,
}

fn render_with_prefix(sink: &dyn OutputSink, reply: &CliReply, first: FirstPrefix) {
    let color = is_terminal_stdout();
    let (first_prefix, cont_prefix) = match first {
        FirstPrefix::Repl => {
            if color {
                (REPL_FIRST_PREFIX, REPL_CONT_PREFIX)
            } else {
                (REPL_FIRST_PREFIX_PLAIN, REPL_CONT_PREFIX)
            }
        }
        FirstPrefix::None => (REPL_CONT_PREFIX, REPL_CONT_PREFIX),
    };

    // We can't route through OutputSink::render directly because it owns the
    // `println!` newline placement; rebuild the body with indentation and
    // hand that to the sink instead.
    let indented = indent_body(&reply.body, first_prefix, cont_prefix);
    let shaped = CliReply {
        kind: reply.kind.clone(),
        body: indented,
    };
    sink.render(&shaped);
}

fn indent_body(body: &str, first_prefix: &str, cont_prefix: &str) -> String {
    if body.is_empty() {
        return first_prefix.to_string();
    }
    let mut out = String::new();
    for (i, line) in body.lines().enumerate() {
        if i == 0 {
            out.push_str(first_prefix);
        } else {
            out.push('\n');
            out.push_str(cont_prefix);
        }
        out.push_str(line);
    }
    out
}

/// Pull `` ```diff\n…\n``` `` blocks out of a text body. Surrounding text
/// stays as `Text` replies. Missing closers or no fences → single `Text`.
pub fn split_text_into_blocks(body: &str) -> Vec<CliReply> {
    const OPEN: &str = "```diff\n";
    const CLOSE: &str = "\n```";
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open_idx) = rest.find(OPEN) {
        let before = &rest[..open_idx];
        let trimmed_before = before.trim_matches('\n');
        if !trimmed_before.is_empty() {
            out.push(CliReply::text(trimmed_before.to_string()));
        }
        let after_open = &rest[open_idx + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(close_idx) => {
                let inner = &after_open[..close_idx];
                out.push(CliReply::diff(inner.to_string()));
                rest = &after_open[close_idx + CLOSE.len()..];
            }
            None => {
                // unterminated fence — keep whatever came after open as diff
                out.push(CliReply::diff(after_open.to_string()));
                rest = "";
                break;
            }
        }
    }
    let tail = rest.trim_matches('\n');
    if !tail.is_empty() {
        out.push(CliReply::text(tail.to_string()));
    }
    if out.is_empty() {
        out.push(CliReply::text(body.to_string()));
    }
    out
}

/// Human elapsed string: `850ms`, `4.8s`, `4m 48s`.
pub fn fmt_elapsed(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        return format!("{secs:.1}s");
    }
    let total = d.as_secs();
    let m = total / 60;
    let s = total % 60;
    format!("{m}m {s}s")
}

/// Live progress indicator written to stderr. No-op when stderr is not a TTY.
///
/// Lifecycle:
/// - [`Progress::start`] spawns the spinner task and returns a handle.
/// - [`ProgressHandle::status_sender`] hands out a cheap clone for updating the
///   live status suffix from other tasks (e.g. a log forwarder).
/// - [`ProgressHandle::finish`] flips the done flag, waits for the spinner to
///   clear its line, and returns the elapsed time.
pub struct Progress;

impl Progress {
    pub fn start(label: impl Into<String>) -> ProgressHandle {
        let start = Instant::now();
        let animate = std::io::stderr().is_terminal();
        let state = Arc::new(AsyncMutex::new(ProgressState {
            label: label.into(),
            last_status: None,
            done: false,
            interjects: Vec::new(),
            tty: animate,
        }));
        let task = if animate {
            let state = state.clone();
            Some(tokio::spawn(spinner_loop(state, start)))
        } else {
            None
        };
        ProgressHandle { task, start, state }
    }
}

pub struct ProgressHandle {
    task: Option<JoinHandle<()>>,
    start: Instant,
    state: Arc<AsyncMutex<ProgressState>>,
}

pub struct ProgressStatus {
    state: Arc<AsyncMutex<ProgressState>>,
}

struct ProgressState {
    label: String,
    last_status: Option<String>,
    done: bool,
    /// Lines to print **above** the spinner on the next tick. Consumers enqueue
    /// with [`ProgressStatus::interject`]; the spinner task drains them.
    interjects: Vec<String>,
    /// When false, [`ProgressStatus::interject`] is a no-op so non-TTY callers
    /// don't leak memory into an unread queue.
    tty: bool,
}

impl ProgressHandle {
    pub fn status_sender(&self) -> ProgressStatus {
        ProgressStatus {
            state: self.state.clone(),
        }
    }

    pub async fn finish(self) -> Duration {
        {
            let mut s = self.state.lock().await;
            s.done = true;
        }
        if let Some(t) = self.task {
            let _ = t.await;
        }
        self.start.elapsed()
    }
}

impl ProgressStatus {
    pub async fn set(&self, s: impl Into<String>) {
        let mut st = self.state.lock().await;
        st.last_status = Some(s.into());
    }

    /// Queue a line to print above the spinner on the next tick.
    /// No-op when the spinner wasn't started (no TTY).
    pub async fn interject(&self, line: impl Into<String>) {
        let mut st = self.state.lock().await;
        if !st.tty {
            return;
        }
        st.interjects.push(line.into());
    }
}

async fn spinner_loop(state: Arc<AsyncMutex<ProgressState>>, start: Instant) {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut i: usize = 0;
    loop {
        // Check done + drain interjects + build line, all under the lock.
        let (line, interjects) = {
            let mut st = state.lock().await;
            if st.done {
                break;
            }
            let interjects = std::mem::take(&mut st.interjects);
            let elapsed = fmt_elapsed(start.elapsed());
            let line = match st.last_status.as_deref() {
                Some(status) if !status.is_empty() => format!(
                    "\r\x1b[2K\x1b[2m{} {} · {} · {}\x1b[0m",
                    FRAMES[i], st.label, status, elapsed
                ),
                _ => format!(
                    "\r\x1b[2K\x1b[2m{} {} · {}\x1b[0m",
                    FRAMES[i], st.label, elapsed
                ),
            };
            (line, interjects)
        };
        // `StderrLock` is `!Send`; scope all writes so nothing crosses `.await`.
        write_interjects_above_spinner(&interjects);
        write_line_to_stderr(&line);
        tokio::time::sleep(Duration::from_millis(90)).await;
        i = (i + 1) % FRAMES.len();
    }
    // Final drain — pick up anything queued after `done` flipped.
    let leftover = {
        let mut st = state.lock().await;
        std::mem::take(&mut st.interjects)
    };
    write_interjects_above_spinner(&leftover);
    write_line_to_stderr("\r\x1b[2K");
}

fn write_line_to_stderr(s: &str) {
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(s.as_bytes());
    let _ = err.flush();
}

fn write_interjects_above_spinner(lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let mut err = std::io::stderr().lock();
    // Erase the current spinner line, print each interject with its own
    // newline; next spinner tick redraws itself below.
    let _ = err.write_all(b"\r\x1b[2K");
    for l in lines {
        let _ = err.write_all(l.as_bytes());
        let _ = err.write_all(b"\n");
    }
    let _ = err.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_envelope_is_versioned() {
        let reply = CliReply::text("hi");
        let env = JsonEnvelope {
            v: 1,
            reply: &reply,
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.starts_with("{\"v\":1,"));
        assert!(s.contains("\"kind\":\"text\""));
        assert!(s.contains("\"body\":\"hi\""));
    }

    #[test]
    fn code_block_carries_lang() {
        let r = CliReply::code("bash", "ls -la");
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"kind\":\"code_block\""));
        assert!(s.contains("\"lang\":\"bash\""));
    }

    #[test]
    fn split_text_pulls_diff_fence_out() {
        let body = "before text\n```diff\n+a\n-b\n```\nafter text";
        let parts = split_text_into_blocks(body);
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[0].kind, ReplyKind::Text));
        assert_eq!(parts[0].body, "before text");
        assert!(matches!(parts[1].kind, ReplyKind::Diff));
        assert_eq!(parts[1].body, "+a\n-b");
        assert!(matches!(parts[2].kind, ReplyKind::Text));
        assert_eq!(parts[2].body, "after text");
    }

    #[test]
    fn split_text_passes_plain_through() {
        let parts = split_text_into_blocks("just prose, no fences");
        assert_eq!(parts.len(), 1);
        assert!(matches!(parts[0].kind, ReplyKind::Text));
    }

    #[test]
    fn indent_body_prefixes_first_and_continuation() {
        let out = indent_body("one\ntwo\nthree", "  ⎿  ", "     ");
        assert_eq!(out, "  ⎿  one\n     two\n     three");
    }

    #[test]
    fn fmt_elapsed_bucketizes_correctly() {
        assert_eq!(fmt_elapsed(Duration::from_millis(5)), "5ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(999)), "999ms");
        assert!(fmt_elapsed(Duration::from_secs(4)).ends_with('s'));
        assert_eq!(fmt_elapsed(Duration::from_secs(65)), "1m 5s");
        assert_eq!(fmt_elapsed(Duration::from_secs(288)), "4m 48s");
    }
}
