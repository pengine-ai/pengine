//! Small text helpers shared across modules.
//!
//! ## Reasoning vs user-visible text (defense in depth)
//!
//! Major hosted APIs (e.g. OpenAI reasoning models) keep chain-of-thought off the user-visible
//! channel entirely. Ollama does the same when thinking mode is enabled: traces go in
//! `message.thinking` and the host must drop that field before history or UI (see
//! `extract_message` in `ollama/service.rs`). Models still sometimes emit planning in
//! `message.content`; we strip tags, optional `<pengine_reply>`, JSON `reply`, heuristics,
//! and (when safe) constrain generations with a JSON schema so only `reply` reaches users.

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

/// First lines of the system prompt so the format rule sits in the KV-cache prefix.
pub const PENGINE_OUTPUT_CONTRACT_LEAD: &str = "[Pengine — output format; host parses this]\n\
1) User-visible text: ONLY inside one <pengine_reply>...</pengine_reply> block (any language, markdown OK).\n\
2) Private planning / English scratch: ONLY inside <pengine_plan>...</pengine_plan> (optional; host deletes it).\n\
3) No user-facing sentences outside <pengine_reply>. After tool results, write the answer immediately in <pengine_reply>.\n\
Example: <pengine_plan>scan forecast</pengine_plan><pengine_reply>Morgen in X: kurz und freundlich.</pengine_reply>\n\n";

/// Injected as an extra system message immediately before the post-tool model call only.
pub const PENGINE_POST_TOOL_REMINDER: &str = "\
You have tool output. Respond in the user's language. REQUIRED: put ONLY the user-visible answer inside \
<pengine_reply>...</pengine_reply>. Put any English or meta reasoning ONLY inside <pengine_plan>...</pengine_plan>. \
Do not narrate tool usage, skills, or planning in plain text; no sentences outside those tags. \
If several `fetch` results are present, some may show robots.txt or User-Agent blocks — still use any successful excerpts; do not tell the user that nothing could be retrieved when other blocks contain usable text.";

fn looks_like_english_scratchpad(s: &str) -> bool {
    s.contains("Okay, let's")
        || s.contains("Okay, let me")
        || s.contains("The user asked")
        || s.contains("The user is asking")
        || s.contains("Wait, the user")
        || s.contains("the user's query")
        || s.contains("Let me check")
        || s.contains("I need to check")
        || s.contains("First, I need")
        || s.contains("according to the skill")
        || s.contains("The instructions say")
        || s.matches("Wait,").count() >= 2
}

/// German / mixed prompts often produce German meta ("Zunächst muss …") without the English cues above.
fn looks_like_german_scratchpad(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("der nutzer fragt")
        || l.contains("die nutzerin fragt")
        || l.contains("zunächst muss")
        || l.contains("zuerst muss ich")
        || l.contains("ich sollte jetzt")
        || (l.contains(" laut skill") || l.contains("laut der skill"))
}

fn looks_like_scratchpad_meta(s: &str) -> bool {
    looks_like_english_scratchpad(s) || looks_like_german_scratchpad(s)
}

fn paragraph_opens_like_meta(p: &str) -> bool {
    let head: String = p.chars().take(120).collect::<String>().to_lowercase();
    head.starts_with("okay,")
        || head.starts_with("wait,")
        || head.starts_with("let me ")
        || head.starts_with("first,")
        || head.starts_with("hmm,")
        || head.contains("the user asked")
        || head.contains("the user is asking")
        || head.contains("der nutzer fragt")
        || head.contains("die nutzerin fragt")
}

fn last_non_meta_paragraph(s: &str) -> Option<String> {
    for block in s.rsplit("\n\n") {
        let t = block.trim();
        if t.len() < 60 {
            continue;
        }
        if paragraph_opens_like_meta(t) {
            continue;
        }
        let open: String = t.chars().take(220).collect::<String>().to_lowercase();
        if open.contains("the user is")
            || open.contains("i should now")
            || open.contains("according to the search")
            || open.contains("tool response")
        {
            continue;
        }
        return Some(t.to_string());
    }
    None
}

/// If the model inlined a labeled answer section, keep only that tail (exact-case markers common in DE output).
fn strip_after_inline_answer_label(s: &str) -> Option<String> {
    for marker in [
        "**Antwort:**",
        "**Antwort**",
        "Antwort:\n\n",
        "\nAntwort:\n",
        "**Zusammenfassung:**",
        "**Zusammenfassung**",
    ] {
        if let Some(idx) = s.find(marker) {
            let tail =
                s[idx + marker.len()..].trim_start_matches(|c: char| c == ':' || c.is_whitespace());
            if tail.len() >= 12 {
                return Some(tail.to_string());
            }
        }
    }
    None
}

/// When the model ignores `<pengine_reply>` but dumps English chain-of-thought, keep the tail that
/// usually starts the real answer. Used only as a last resort after tag parsing fails.
fn strip_plain_scratchpad_fallback(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() || !looks_like_scratchpad_meta(s) {
        return s.to_string();
    }

    if let Some(tail) = strip_after_inline_answer_label(s) {
        return tail;
    }

    let markers = [
        "\n\nMorgen (",
        "\nMorgen (",
        "\n\nMorgen in ",
        "\nMorgen in ",
        "\n\n**Morgen",
        "\n**Morgen",
        "\n\nHeute in ",
        "\nHeute in ",
        "\n\nTomorrow in ",
        "\nTomorrow in ",
        "\n\nThe answer is ",
        "\n\n### Antwort",
        "\n\nZusammenfassung:",
        "\nZusammenfassung:",
        "\n\n**Zusammenfassung",
        "\n\nKurzfassung:",
        "\nKurzfassung:",
        "\n\nDamit:",
        "\n\nFazit:",
        "\n\nZusammenfassend",
        "\n\nKurz gesagt,",
        "\nKurz gesagt,",
    ];
    let mut best: Option<usize> = None;
    for m in markers {
        if let Some(i) = s.rfind(m) {
            if i >= 40 {
                best = Some(best.map_or(i, |j| j.max(i)));
            }
        }
    }
    if let Some(i) = s.rfind("Morgen in ") {
        if i >= 80 {
            best = Some(best.map_or(i, |j| j.max(i)));
        }
    }
    if let Some(i) = s.rfind("Morgen (") {
        if i >= 80 {
            best = Some(best.map_or(i, |j| j.max(i)));
        }
    }

    if let Some(i) = best {
        return s[i..].trim_start().to_string();
    }

    if let Some(p) = last_non_meta_paragraph(s) {
        return p;
    }

    // Meta-only or no recoverable user-facing block — better empty than leaking CoT to Telegram/UI.
    String::new()
}

/// Drop paired XML/HTML-style blocks iteratively.
fn strip_tag_pair(s: &str, open: &str, close: &str) -> String {
    if open.is_empty() || close.is_empty() {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some((before, after_open)) = rest.split_once(open) {
        out.push_str(before);
        match after_open.split_once(close) {
            Some((_, tail)) => rest = tail,
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_all_named_blocks(s: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = s.to_string();
    loop {
        let next = strip_tag_pair(&out, &open, &close);
        if next == out {
            return next.trim().to_string();
        }
        out = next;
    }
}

/// Inner text of the last complete `<tag>…</tag>` pair (models sometimes emit a draft then a final block).
fn last_tag_inner(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut last = None;
    let mut search = s;
    while let Some(start) = search.find(&open) {
        let after_open = &search[start + open.len()..];
        if let Some(end) = after_open.find(&close) {
            last = Some(after_open[..end].trim().to_string());
            search = &after_open[end + close.len()..];
        } else {
            break;
        }
    }
    last
}

fn parse_json_reply_field(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content.trim()).ok()?;
    Some(v.get("reply")?.as_str()?.trim().to_string())
}

/// Normalize `message.content` from Ollama: template reasoning tags, optional
/// `<pengine_reply>`, or JSON `{"reply":"…"}` when `json_object_reply` is true.
pub fn normalize_assistant_message_content(raw: &str, json_object_reply: bool) -> String {
    if json_object_reply {
        if let Some(reply) = parse_json_reply_field(raw) {
            return block_if_still_meta_scratchpad(reply);
        }
    }

    let s = strip_think(raw);
    let s = strip_tag_pair(&s, concat!("<", "think", ">"), concat!("</", "think", ">"));

    // Explicit host contract wins; do not second-guess tagged user text.
    if let Some(inner) = last_tag_inner(&s, "pengine_reply") {
        return inner.trim().to_string();
    }

    // Some reasoning models wrap only the final reply in `<answer>…</answer>`.
    if let Some(inner) = last_tag_inner(&s, "answer") {
        return block_if_still_meta_scratchpad(inner);
    }

    let without_plan = strip_all_named_blocks(&s, "pengine_plan");
    let t = without_plan.trim().to_string();
    let out = strip_plain_scratchpad_fallback(&t);
    block_if_still_meta_scratchpad(out)
}

/// If the model slipped planning into the only channel we show, drop it rather than leak CoT.
fn block_if_still_meta_scratchpad(s: String) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    if looks_like_scratchpad_meta(t) {
        return String::new();
    }
    t.to_string()
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
    fn normalize_assistant_prefers_pengine_reply() {
        let s = "<pengine_plan>notes</pengine_plan><pengine_reply>Hello</pengine_reply>";
        assert_eq!(normalize_assistant_message_content(s, false), "Hello");
    }

    #[test]
    fn normalize_assistant_last_reply_block_wins() {
        let s = "<pengine_reply>draft</pengine_reply> x <pengine_reply>final</pengine_reply>";
        assert_eq!(normalize_assistant_message_content(s, false), "final");
    }

    #[test]
    fn normalize_assistant_strips_plan_only_when_no_reply_tag() {
        let s = "<pengine_plan>secret</pengine_plan>\nvisible";
        assert_eq!(normalize_assistant_message_content(s, false), "visible");
    }

    #[test]
    fn normalize_assistant_json_reply_mode() {
        let s = r#"{"reply":"Done."}"#;
        assert_eq!(normalize_assistant_message_content(s, true), "Done.");
    }

    #[test]
    fn normalize_assistant_think_then_reply_tag() {
        let s = "<think>x</think><pengine_reply>ok</pengine_reply>";
        assert_eq!(normalize_assistant_message_content(s, false), "ok");
    }

    #[test]
    fn normalize_assistant_extracts_answer_tag_when_no_pengine_reply() {
        let s = "<think>r</think><answer>Only this.</answer>";
        assert_eq!(normalize_assistant_message_content(s, false), "Only this.");
    }

    #[test]
    fn normalize_assistant_json_reply_still_meta_gets_cleared() {
        let s = r#"{"reply":"Okay, let's see. The user asked for X."}"#;
        assert_eq!(normalize_assistant_message_content(s, true), "");
    }

    #[test]
    fn normalize_assistant_fallback_strips_english_scratchpad_before_morgen() {
        let pad = "Okay, let's see. The user asked for weather.\n\n";
        let answer = "Morgen in Breitenau: sonnig.";
        let combined = format!("{pad}{answer}");
        assert_eq!(
            normalize_assistant_message_content(&combined, false),
            answer
        );
    }

    #[test]
    fn normalize_assistant_fallback_strips_after_antwort_label() {
        let pad = "Okay, let's see. The user is asking about divorce.\n\n";
        let answer = "**Antwort:** Ja, das ist möglich.";
        let combined = format!("{pad}{answer}");
        assert_eq!(
            normalize_assistant_message_content(&combined, false),
            "Ja, das ist möglich."
        );
    }

    #[test]
    fn normalize_assistant_fallback_meta_only_returns_empty() {
        let s = "Okay, let's see. The user is asking about X. First, I need to check. Wait, the skill says. So I should.";
        assert_eq!(normalize_assistant_message_content(s, false), "");
    }

    #[test]
    fn normalize_assistant_fallback_german_meta_then_paragraph() {
        let pad = "Zunächst muss ich die Frage prüfen.\n\n";
        let answer = "Kurz: In Österreich gilt aus den Familienberatungsstellen-Infos Folgendes für Ihren Fall und die Beratungsbestätigung.";
        let combined = format!("{pad}{answer}");
        assert_eq!(
            normalize_assistant_message_content(&combined, false),
            answer.trim()
        );
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
