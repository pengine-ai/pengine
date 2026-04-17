//! Small text helpers shared across modules.

/// Remove reasoning scaffolding from model content.
///
/// qwen3 and similar reasoning models emit chain-of-thought wrapped in
/// `<think>…</think>` tags inside `message.content`. Three shapes in the wild:
///
/// 1. Paired (`<think>r</think>answer`) — strip the pair.
/// 2. Closer-only (`reasoning…</think>answer`) — the Ollama chat template
///    injected the opening tag before generation, so the client sees only
///    the closer. Drop through the last `</think>`.
/// 3. Unclosed opener (`<think>partial`) — drop everything from the opener
///    onwards so partial reasoning never leaks.
///
/// Limitation: an answer that contains a literal `</think>` (e.g. the model
/// explaining reasoning-mode syntax) will be cut through that token. This is
/// accepted because reasoning models never emit the tag for any other purpose.
pub fn strip_think(s: &str) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";

    // Pass 1 — paired blocks. Preserves content between multiple blocks.
    let mut out = String::new();
    let mut rest = s;
    while let Some((before, after_open)) = rest.split_once(OPEN) {
        out.push_str(before);
        match after_open.split_once(CLOSE) {
            Some((_, tail)) => rest = tail,
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);

    // Pass 2 — closer-only residue. Nothing to do if pass 1 consumed it.
    if let Some((_, tail)) = out.rsplit_once(CLOSE) {
        out = tail.to_string();
    }

    out.trim().to_string()
}

/// Strip ANSI escape sequences (CSI + OSC) and collapse runs of blank lines.
///
/// Tool fetches like `wttr.in` return colored terminal output; the ANSI bytes
/// waste prompt tokens but carry no signal for the model. Also trims trailing
/// whitespace per line and squashes 3+ blank lines to a single blank line.
pub fn compact_tool_output(s: &str) -> String {
    let stripped = strip_ansi(s);
    let mut out = String::with_capacity(stripped.len());
    let mut blank_run = 0usize;
    for line in stripped.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Remove ANSI CSI (`ESC[…m` and similar) and OSC (`ESC]…BEL/ST`) sequences.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for esc in chars.by_ref() {
                    // CSI final byte is in 0x40..=0x7E
                    if ('\x40'..='\x7e').contains(&esc) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC is terminated by BEL (0x07) or ESC\.
                while let Some(esc) = chars.next() {
                    if esc == '\x07' {
                        break;
                    }
                    if esc == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) | None => {}
        }
    }
    out
}

/// Cap `text` at `max_chars` (counted as Unicode chars, not bytes) and append
/// a short truncation marker so the model knows content was elided. Used when
/// feeding tool output back into the chat history — raw fetch bodies are
/// often 5–10 kB but the model only needs the first screen to answer.
///
/// Returns the original `String` unchanged when it is already within budget.
pub fn truncate_for_model(text: &str, max_chars: usize) -> String {
    const MARKER: &str = "\n…[truncated]";
    let mut iter = text.char_indices();
    let Some((cut, _)) = iter.nth(max_chars) else {
        return text.to_string();
    };
    let mut out = String::with_capacity(cut + MARKER.len());
    out.push_str(&text[..cut]);
    out.push_str(MARKER);
    out
}

/// Split `text` into chunks no longer than `budget` characters, preferring
/// newline boundaries so paragraphs stay intact. Used to respect Telegram's
/// 4096-character per-message limit.
pub fn split_by_chars(text: &str, budget: usize) -> Vec<String> {
    assert!(budget > 0, "budget must be positive");

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;

    let flush = |current: &mut String, current_chars: &mut usize, chunks: &mut Vec<String>| {
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
            *current_chars = 0;
        }
    };

    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if line_chars > budget {
            flush(&mut current, &mut current_chars, &mut chunks);
            let mut buf = String::new();
            let mut buf_chars = 0usize;
            for ch in line.chars() {
                if buf_chars + 1 > budget {
                    chunks.push(std::mem::take(&mut buf));
                    buf_chars = 0;
                }
                buf.push(ch);
                buf_chars += 1;
            }
            if !buf.is_empty() {
                chunks.push(buf);
            }
            continue;
        }
        if current_chars + line_chars > budget {
            flush(&mut current, &mut current_chars, &mut chunks);
        }
        current.push_str(line);
        current_chars += line_chars;
    }
    flush(&mut current, &mut current_chars, &mut chunks);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_think_removes_single_block() {
        let s = "<think>reasoning</think>Hello";
        assert_eq!(strip_think(s), "Hello");
    }

    #[test]
    fn strip_think_removes_multiple_blocks() {
        let s = "a<think>r1</think> b <think>r2</think>c";
        assert_eq!(strip_think(s), "a b c");
    }

    #[test]
    fn strip_think_drops_unclosed_block() {
        let s = "text before<think>unfinished reasoning";
        assert_eq!(strip_think(s), "text before");
    }

    #[test]
    fn strip_think_passes_through_when_absent() {
        assert_eq!(strip_think("plain reply"), "plain reply");
    }

    #[test]
    fn strip_think_handles_multiline_block() {
        let s = "pre\n<think>line1\nline2\n</think>\npost";
        assert_eq!(strip_think(s), "pre\n\npost");
    }

    #[test]
    fn strip_think_handles_closer_only_reasoning() {
        // Ollama's chat template injects the opener before streaming starts,
        // so only the closer reaches the client.
        let s = "Okay, let me think about this...</think>\n\nThe answer is 42.";
        assert_eq!(strip_think(s), "The answer is 42.");
    }

    #[test]
    fn strip_think_closer_only_with_multiline_preamble() {
        let s = "Reasoning line 1.\nReasoning line 2.\n</think>Here is the reply.";
        assert_eq!(strip_think(s), "Here is the reply.");
    }

    #[test]
    fn strip_think_cuts_through_literal_closer_in_answer() {
        // Documented limitation: an answer containing a literal `</think>`
        // is cut through it. Acceptable — reasoning models never emit the
        // tag for any other purpose.
        let s = "The `</think>` tag terminates a reasoning block.";
        assert_eq!(strip_think(s), "` tag terminates a reasoning block.");
    }

    #[test]
    fn compact_tool_output_strips_ansi_csi() {
        let s = "\x1b[31mred\x1b[0m normal";
        assert_eq!(compact_tool_output(s), "red normal");
    }

    #[test]
    fn compact_tool_output_strips_osc_bel_terminator() {
        let s = "\x1b]0;title\x07visible";
        assert_eq!(compact_tool_output(s), "visible");
    }

    #[test]
    fn compact_tool_output_collapses_blank_runs_and_trailing_spaces() {
        let s = "line1   \n\n\n\nline2\n   \nline3";
        assert_eq!(compact_tool_output(s), "line1\n\nline2\n\nline3");
    }

    #[test]
    fn compact_tool_output_passes_through_clean_text() {
        let s = "plain\ntext";
        assert_eq!(compact_tool_output(s), "plain\ntext");
    }

    #[test]
    fn truncate_for_model_passes_through_short_input() {
        assert_eq!(truncate_for_model("hello", 100), "hello");
    }

    #[test]
    fn truncate_for_model_cuts_at_char_boundary_and_marks() {
        let out = truncate_for_model("αβγδεζ", 3);
        assert_eq!(out, "αβγ\n…[truncated]");
    }

    #[test]
    fn truncate_for_model_exact_budget_is_untouched() {
        let out = truncate_for_model("abc", 3);
        assert_eq!(out, "abc");
    }

    #[test]
    fn split_by_chars_short_text_stays_whole() {
        let chunks = split_by_chars("hello world", 100);
        assert_eq!(chunks, vec!["hello world"]);
    }

    #[test]
    fn split_by_chars_breaks_on_newline() {
        let text = "aaaa\nbbbb\ncccc";
        let chunks = split_by_chars(text, 5);
        assert_eq!(chunks, vec!["aaaa\n", "bbbb\n", "cccc"]);
    }

    #[test]
    fn split_by_chars_splits_long_line_in_half() {
        let chunks = split_by_chars("abcdefghij", 4);
        assert_eq!(chunks, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn split_by_chars_respects_unicode_char_count() {
        let chunks = split_by_chars("αβγδ", 2);
        assert_eq!(chunks, vec!["αβ", "γδ"]);
    }
}
