//! CLI reply envelope and output sinks.
//!
//! Handlers produce [`CliReply`] values; rendering (ANSI, Markdown fences,
//! chunking) belongs to sinks. This keeps handlers transport-agnostic and
//! lets a single change to `TelegramSink` affect every reply at once.

use regex::Regex;
use serde::Serialize;
use std::io::{IsTerminal, Write};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};
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

    /// When true, [`render_with_prefix`] may print unified diffs line-by-line with ANSI
    /// (so `+`/`-` detection still works alongside REPL indentation). JSON sinks keep `false`.
    fn prefer_line_oriented_diff(&self) -> bool {
        false
    }
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
    fn prefer_line_oriented_diff(&self) -> bool {
        self.color
    }

    fn render(&self, reply: &CliReply) {
        match &reply.kind {
            ReplyKind::Text => println!("{}", reply.body),
            ReplyKind::Error => {
                if self.color {
                    eprintln!();
                    eprintln!(
                        "  \x1b[1m\x1b[38;5;203m⚠\x1b[0m \x1b[1m\x1b[38;5;203mSomething went wrong\x1b[0m"
                    );
                    for line in reply.body.lines() {
                        eprintln!("\x1b[38;5;203m  {}\x1b[0m", line);
                    }
                } else {
                    eprintln!("Error: {}", reply.body);
                }
            }
            ReplyKind::CodeBlock { lang } => {
                let _ = lang;
                println!("{}", reply.body);
            }
            ReplyKind::Log => {
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

/// Dark-terminal palette (truecolor + bold). Background strips improve scanability on
/// charcoal backgrounds without relying on low-contrast default 16-color green/red.
const DIFF_RESET: &str = "\x1b[0m";
const DIFF_CTX: &str = "\x1b[38;2;139;148;158m"; // muted slate (context)
const DIFF_META: &str = "\x1b[38;2;180;190;200m"; // diff --git, index, rename…
const DIFF_FILE_HDR: &str = "\x1b[1;38;2;121;192;255m"; // +++ / ---
const DIFF_HUNK: &str = "\x1b[1;38;2;242;204;96m"; // @@ hunk headers
const DIFF_ADD_FG: &str = "\x1b[38;2;80;250;123m";
const DIFF_ADD_BG: &str = "\x1b[48;2;20;45;28m";
const DIFF_DEL_FG: &str = "\x1b[38;2;255;123;123m";
const DIFF_DEL_BG: &str = "\x1b[48;2;45;22;24m";

fn style_diff_line(line: &str) -> String {
    if line.starts_with("+++") || line.starts_with("---") {
        return format!("{DIFF_FILE_HDR}{line}{DIFF_RESET}");
    }
    if line.starts_with("@@") {
        return format!("{DIFF_HUNK}{line}{DIFF_RESET}");
    }
    if line.starts_with('+') {
        return format!("{DIFF_ADD_BG}{DIFF_ADD_FG}{line}{DIFF_RESET}");
    }
    if line.starts_with('-') {
        return format!("{DIFF_DEL_BG}{DIFF_DEL_FG}{line}{DIFF_RESET}");
    }
    if line.starts_with("diff --git ")
        || line.starts_with("index ")
        || line.starts_with("new file mode ")
        || line.starts_with("deleted file mode ")
        || line.starts_with("similarity index ")
        || line.starts_with("rename from ")
        || line.starts_with("rename to ")
        || line.starts_with("Binary files ")
    {
        return format!("{DIFF_META}{line}{DIFF_RESET}");
    }
    if line.is_empty() {
        return String::new();
    }
    format!("{DIFF_CTX}{line}{DIFF_RESET}")
}

fn print_diff_with_ansi(body: &str) {
    for line in body.lines() {
        println!("{}", style_diff_line(line));
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

pub(super) const REPL_FIRST_PREFIX: &str = "  \x1b[2m⎿\x1b[0m  ";
pub(super) const REPL_FIRST_PREFIX_PLAIN: &str = "  ⎿  ";
pub(super) const REPL_CONT_PREFIX: &str = "     ";

/// Width of dim rules framing each REPL reply (app-like section separators).
const REPL_REPLY_RULE_WIDTH: usize = 52;

fn repl_reply_rule_bar(left: char, mid: char, right: char) {
    let dash = REPL_REPLY_RULE_WIDTH.saturating_sub(2);
    let mid_str: String = std::iter::repeat_n(mid, dash).collect();
    println!("  \x1b[38;5;242m{}{}{}\x1b[0m", left, mid_str, right);
}

/// Soft panel header/footer so replies feel scoped like an in-app card (TTY only).
pub(super) fn repl_reply_section_open() {
    if !is_terminal_stdout() {
        return;
    }
    println!();
    repl_reply_rule_bar('╭', '─', '╮');
}

pub(super) fn repl_reply_section_close() {
    if !is_terminal_stdout() {
        return;
    }
    repl_reply_rule_bar('╰', '─', '╯');
    println!();
}

/// Short slash-command replies (single short line, no fences) stay compact — no card chrome.
pub(crate) fn repl_reply_use_section_chrome(reply: &CliReply) -> bool {
    match &reply.kind {
        ReplyKind::Text => {
            let t = reply.body.trim();
            if t.contains("```") || t.contains('\n') {
                return true;
            }
            t.chars().count() >= 72
        }
        _ => true,
    }
}

/// Central rendering helper. Handles diff-fence splitting for `Text` replies
/// in REPL mode so ` ```diff ``` ` blocks get coloured inline.
pub fn render_reply(sink: &dyn OutputSink, reply: &CliReply, style: RenderStyle) {
    // Empty text is a no-op — content was already streamed live to the terminal.
    if matches!(reply.kind, ReplyKind::Text) && reply.body.is_empty() {
        return;
    }
    match style {
        RenderStyle::Plain => sink.render(reply),
        RenderStyle::ReplIndent => {
            if repl_reply_use_section_chrome(reply) {
                repl_reply_section_open();
                render_reply_indented(sink, reply);
                repl_reply_section_close();
            } else {
                println!();
                render_reply_indented(sink, reply);
                println!();
            }
        }
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
        ReplyKind::CodeBlock { .. } => render_with_prefix(sink, reply, FirstPrefix::Repl),
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
    if matches!(reply.kind, ReplyKind::Diff) && sink.prefer_line_oriented_diff() {
        render_diff_with_repl_prefix(reply.body.as_str(), first);
        return;
    }

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

/// REPL-style prefixes on each line, with per-line diff ANSI (adds/removes stay highlighted).
fn render_diff_with_repl_prefix(body: &str, first: FirstPrefix) {
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
    for (i, line) in body.lines().enumerate() {
        let p = if i == 0 { first_prefix } else { cont_prefix };
        let body_line = style_diff_line(line);
        println!("{p}{body_line}");
    }
}

static MD_FENCE_RE: OnceLock<Regex> = OnceLock::new();

fn md_fence_regex() -> &'static Regex {
    MD_FENCE_RE.get_or_init(|| {
        Regex::new(r"```\s*([^\n`]*?)\s*\n([\s\S]*?)```").expect("markdown fence regex")
    })
}

/// Pull `` ```lang\n…\n``` `` blocks out of a text body. `lang == diff` (case-insensitive)
/// becomes [`ReplyKind::Diff`]; other languages become [`ReplyKind::CodeBlock`].
/// Unclosed fences are left in the trailing `Text`. No fences → single `Text`.
pub fn split_text_into_blocks(body: &str) -> Vec<CliReply> {
    let mut out = Vec::new();
    let re = md_fence_regex();
    let mut last = 0usize;
    for cap in re.captures_iter(body) {
        let m = cap.get(0).expect("regex capture 0");
        if m.start() > last {
            let chunk = body[last..m.start()].trim_matches('\n');
            if !chunk.is_empty() {
                out.push(CliReply::text(chunk.to_string()));
            }
        }
        let lang = cap.get(1).map(|g| g.as_str().trim()).unwrap_or("");
        let inner = cap
            .get(2)
            .map(|g| g.as_str())
            .unwrap_or("")
            .trim_matches('\n');
        if lang.eq_ignore_ascii_case("diff") {
            out.push(CliReply::diff(inner.to_string()));
        } else {
            out.push(CliReply::code(lang.to_string(), inner.to_string()));
        }
        last = m.end();
    }
    let tail = body[last..].trim_matches('\n');
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
        let token_counter = Arc::new(AtomicU64::new(0));
        let state = Arc::new(AsyncMutex::new(ProgressState {
            label: label.into(),
            last_status: None,
            done: false,
            interjects: Vec::new(),
            tty: animate,
        }));
        let task = if animate {
            let state = state.clone();
            let tc = token_counter.clone();
            Some(tokio::spawn(spinner_loop(state, start, tc)))
        } else {
            None
        };
        ProgressHandle {
            task,
            start,
            state,
            token_counter,
        }
    }
}

pub struct ProgressHandle {
    task: Option<JoinHandle<()>>,
    start: Instant,
    state: Arc<AsyncMutex<ProgressState>>,
    token_counter: Arc<AtomicU64>,
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

    /// Returns a counter the caller can pass to the agent for live output-token display.
    /// The spinner reads it atomically each tick and shows `out:N↑` in the status line.
    pub fn token_counter(&self) -> Arc<AtomicU64> {
        self.token_counter.clone()
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

    /// Signal the spinner to stop. The spinner task will clear its line and
    /// exit within one tick (≤90 ms). Callers should wait ~95 ms before
    /// printing to stdout so the line is clear.
    pub async fn stop_spinner(&self) {
        let mut st = self.state.lock().await;
        st.done = true;
    }
}

/// Visible-character width of the terminal's stderr, capped for safety.
/// Uses TIOCGWINSZ, then falls back to the COLUMNS env var, then 80.
fn term_cols() -> usize {
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Winsize {
            ws_row: u16,
            ws_col: u16,
            ws_xpixel: u16,
            ws_ypixel: u16,
        }
        // SAFETY: ioctl with TIOCGWINSZ only reads from the kernel into our
        // local Winsize struct; no aliasing or other unsafe preconditions.
        unsafe {
            extern "C" {
                fn ioctl(fd: i32, request: u64, ...) -> i32;
            }
            #[cfg(target_os = "macos")]
            const TIOCGWINSZ: u64 = 0x40087468;
            #[cfg(not(target_os = "macos"))]
            const TIOCGWINSZ: u64 = 0x5413;
            let mut ws = Winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if ioctl(2, TIOCGWINSZ, &mut ws as *mut Winsize) == 0 && ws.ws_col > 20 {
                return ws.ws_col as usize;
            }
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n: &usize| n > 20)
        .unwrap_or(80)
}

/// Thousands-separator formatter for the live token counter (no dep on handlers).
fn spinner_fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Build the spinner line, capped to `cols` visible characters so it never
/// wraps. Wrapping is the root cause of the spinner not overwriting in place:
/// `\r\x1b[2K` clears only the current terminal line, so any wrapped
/// continuation from the previous tick stays and accumulates.
///
/// `live_out` is the approximate output-token count so far (0 = hide).
fn build_spinner_line(
    frame: &str,
    label: &str,
    status: Option<&str>,
    elapsed: &str,
    live_out: u64,
    cols: usize,
) -> String {
    // Leave a 1-char margin so the cursor never lands on column 0 of the next
    // line due to an off-by-one in the terminal's auto-wrap logic.
    let budget = cols.saturating_sub(1);

    let tok = if live_out > 0 {
        format!(" · out:{}↑", spinner_fmt_num(live_out))
    } else {
        String::new()
    };

    let base_with_status = match status {
        Some(s) if !s.is_empty() => format!("{frame} {label} · {s} · {elapsed}"),
        _ => format!("{frame} {label} · {elapsed}"),
    };
    let base_plain = format!("{frame} {label} · {elapsed}");

    // Prefer longest line that fits; drop token suffix first, then status.
    let visible: String = {
        let full = format!("{base_with_status}{tok}");
        if full.chars().count() <= budget {
            full
        } else if base_with_status.chars().count() <= budget {
            base_with_status
        } else if base_plain.chars().count() <= budget {
            base_plain
        } else {
            let max = budget.saturating_sub(1);
            format!("{}…", base_plain.chars().take(max).collect::<String>())
        }
    };

    // Blue spinner: bold bright-blue frame + medium-blue label + dim-blue suffix.
    // Split at the first " · " to colour frame+label vs. the trailing detail.
    let colored = if let Some((head, tail)) = visible.split_once(" · ") {
        // head = "⠹ Thonking", tail = "status · 4s" or just "4s"
        format!(
            "\x1b[1;38;2;79;172;255m{head}\x1b[0m \x1b[38;2;99;160;220m·\x1b[0m \x1b[2;38;2;120;170;220m{tail}\x1b[0m"
        )
    } else {
        format!("\x1b[1;38;2;79;172;255m{visible}\x1b[0m")
    };
    format!("\r\x1b[2K{colored}")
}

async fn spinner_loop(
    state: Arc<AsyncMutex<ProgressState>>,
    start: Instant,
    token_counter: Arc<AtomicU64>,
) {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut i: usize = 0;
    // Read terminal width once; it rarely changes mid-turn.
    let cols = term_cols();
    loop {
        // Check done + drain interjects + build line, all under the lock.
        let (line, interjects) = {
            let mut st = state.lock().await;
            if st.done {
                break;
            }
            let interjects = std::mem::take(&mut st.interjects);
            let elapsed = fmt_elapsed(start.elapsed());
            let live_out = token_counter.load(Ordering::Relaxed);
            let line = build_spinner_line(
                FRAMES[i],
                &st.label,
                st.last_status.as_deref(),
                &elapsed,
                live_out,
                cols,
            );
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
    fn split_text_pulls_rust_fence_out() {
        let body = "intro\n```rust\nfn main() {}\n```\ntrailer";
        let parts = split_text_into_blocks(body);
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[0].kind, ReplyKind::Text));
        assert_eq!(parts[0].body, "intro");
        match &parts[1].kind {
            ReplyKind::CodeBlock { lang } => assert_eq!(lang, "rust"),
            _ => panic!("expected code block"),
        }
        assert_eq!(parts[1].body, "fn main() {}");
        assert!(matches!(parts[2].kind, ReplyKind::Text));
        assert_eq!(parts[2].body, "trailer");
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

    #[test]
    fn repl_chrome_skips_short_single_line() {
        assert!(!repl_reply_use_section_chrome(&CliReply::text("ready")));
        assert!(!repl_reply_use_section_chrome(&CliReply::text(
            "short slash reply"
        )));
    }

    #[test]
    fn repl_chrome_frames_long_or_multiline() {
        assert!(repl_reply_use_section_chrome(&CliReply::text("two\nlines")));
        let long = "x".repeat(90);
        assert!(repl_reply_use_section_chrome(&CliReply::text(long)));
        assert!(repl_reply_use_section_chrome(&CliReply::text(
            "```\nok\n```"
        )));
    }

    #[test]
    fn repl_chrome_always_on_code_and_diff() {
        assert!(repl_reply_use_section_chrome(&CliReply::code(
            "bash", "echo hi"
        )));
        assert!(repl_reply_use_section_chrome(&CliReply::diff("+x")));
    }

    #[test]
    fn style_diff_line_tags_add_remove_and_hunk() {
        let add = super::style_diff_line("+foo");
        assert!(add.contains("+foo"));
        assert!(
            add.contains("\x1b[48;2;20;45;28m"),
            "add line uses dark green bg"
        );
        let del = super::style_diff_line("-bar");
        assert!(del.contains("-bar"));
        assert!(
            del.contains("\x1b[48;2;45;22;24m"),
            "del line uses dark red bg"
        );
        let hunk = super::style_diff_line("@@ -1,2 +1,3 @@");
        assert!(hunk.contains("@@"));
        assert!(hunk.contains("\x1b[1;38;2;242;204;96m"));
        let ctx = super::style_diff_line(" context");
        assert!(ctx.contains("context"));
        assert!(ctx.contains("\x1b[38;2;139;148;158m"));
    }

    #[test]
    fn style_diff_line_file_headers_not_confused_with_plus() {
        let hdr = super::style_diff_line("+++ b/src/main.rs");
        assert!(hdr.contains("+++"));
        assert!(
            !hdr.contains("\x1b[48;2;20;45;28m"),
            "+++ must not use add-line bg"
        );
    }
}
