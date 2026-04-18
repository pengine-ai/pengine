//! Generic keyword-matching primitives.
//!
//! Feature modules declare their own [`KeywordGroup`]s next to the domain that
//! owns them (e.g. memory-session commands live in `modules/memory`, AI-model
//! control cues in `modules/ollama/keywords.rs`, Brave gating phrases in
//! `modules/skills/keywords.rs`). This module only provides the shape and the
//! matcher — no domain knowledge.
//!
//! Translation guide: each group carries one `(lang, &[phrases])` row per
//! language. To add Spanish phrases, add them to the `("es", &[...])` entry
//! of the relevant group. Empty slices are fine — the group simply has no
//! phrases for that language yet.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchMode {
    /// Full-message match after normalization. Trailing non-alphanumerics are
    /// stripped before comparison. Use for short, standalone commands where
    /// an accidental substring hit would be harmful (`exit`, `quit`, `record`).
    Exact,
    /// Phrase contained anywhere in the normalized message. Use for cues that
    /// ride along with a real request (`think hard about this problem`).
    /// Phrases must be long enough not to collide with common words.
    Substring,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct KeywordGroup {
    pub id: &'static str,
    pub description: &'static str,
    pub mode: MatchMode,
    pub phrases_by_lang: &'static [(&'static str, &'static [&'static str])],
}

/// Lowercase and map curly quotes to ASCII. Phrase lists are authored lower-case
/// and ASCII-quoted; this brings user input into the same shape.
pub fn normalize(msg: &str) -> String {
    let mapped: String = msg
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            c => c,
        })
        .collect();
    mapped.trim().to_lowercase()
}

/// Normalize + strip trailing non-alphanumerics. Used by exact matching and by
/// domain matchers like memory's starfleet-signoff check that run on tokens.
pub fn normalize_exact(msg: &str) -> String {
    normalize(msg)
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

impl KeywordGroup {
    pub fn all_phrases(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.phrases_by_lang
            .iter()
            .flat_map(|(_, ps)| ps.iter().copied())
    }

    pub fn matches(&self, msg: &str) -> bool {
        match self.mode {
            MatchMode::Exact => {
                let n = normalize_exact(msg);
                self.all_phrases().any(|p| n == p)
            }
            MatchMode::Substring => {
                let n = normalize(msg);
                self.all_phrases().any(|p| n.contains(p))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PING: KeywordGroup = KeywordGroup {
        id: "test.ping",
        description: "test",
        mode: MatchMode::Exact,
        phrases_by_lang: &[("en", &["ping"]), ("de", &["pong"])],
    };

    const THINK: KeywordGroup = KeywordGroup {
        id: "test.think",
        description: "test",
        mode: MatchMode::Substring,
        phrases_by_lang: &[("en", &["think hard"])],
    };

    #[test]
    fn exact_matches_full_message_ignoring_case_and_trailing_punct() {
        assert!(PING.matches("ping"));
        assert!(PING.matches("PING"));
        assert!(PING.matches("  ping.  "));
        assert!(PING.matches("pong!"));
        assert!(!PING.matches("ping pong"));
        assert!(!PING.matches("ping me"));
    }

    #[test]
    fn substring_matches_embedded_phrase() {
        assert!(THINK.matches("think hard about this"));
        assert!(THINK.matches("please THINK HARD"));
        assert!(!THINK.matches("thinker"));
        assert!(!THINK.matches("hard think"));
    }

    #[test]
    fn all_phrases_iterates_every_language() {
        let phrases: Vec<&str> = PING.all_phrases().collect();
        assert_eq!(phrases, vec!["ping", "pong"]);
    }
}
