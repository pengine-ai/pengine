//! User-message keyword groups that control AI-model behavior.
//!
//! To translate: fill in the phrase array for the target language. Phrases are
//! lowercase, ASCII-quoted, and matched as substrings (the cue may appear
//! inside a real question).

use crate::shared::keywords::{KeywordGroup, MatchMode};

const THINK_ON_EN: &[&str] = &[
    "think hard",
    "take your time",
    "reason carefully",
    "be thorough",
    "think step by step",
    "think deeply",
    "deep think",
];

const THINK_ON_DE: &[&str] = &[
    "denk gründlich nach",
    "denke gründlich nach",
    "lass dir zeit",
    "überlege sorgfältig",
    "überleg sorgfältig",
    "denk scharf nach",
    "denk genau nach",
    "denk gut nach",
];

const THINK_ON_FR: &[&str] = &[
    "réfléchis bien",
    "prends ton temps",
    "réfléchis soigneusement",
    "réfléchis en profondeur",
];

const THINK_ON_ES: &[&str] = &[
    "piensa bien",
    "tómate tu tiempo",
    "piensa detenidamente",
    "piensa a fondo",
];

const THINK_ON_JA: &[&str] = &["じっくり考えて", "よく考えて", "深く考えて"];

/// Substrings that enable the model's thinking/reasoning mode for this turn.
/// The default is thinking-off (faster, cheaper); this group re-enables it
/// when the user explicitly asks for careful reasoning.
pub const THINK_ON: KeywordGroup = KeywordGroup {
    id: "ai.think_on",
    description: "Enable qwen3-style thinking mode for this turn when the user \
asks for careful reasoning.",
    mode: MatchMode::Substring,
    phrases_by_lang: &[
        ("en", THINK_ON_EN),
        ("de", THINK_ON_DE),
        ("fr", THINK_ON_FR),
        ("es", THINK_ON_ES),
        ("ja", THINK_ON_JA),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_cues_match() {
        assert!(THINK_ON.matches("think hard about this problem"));
        assert!(THINK_ON.matches("please take your time with the answer"));
        assert!(THINK_ON.matches("REASON CAREFULLY please"));
    }

    #[test]
    fn german_cues_match() {
        assert!(THINK_ON.matches("Denk gründlich nach über diese Frage"));
        assert!(THINK_ON.matches("lass dir Zeit damit"));
    }

    #[test]
    fn unrelated_text_does_not_match() {
        assert!(!THINK_ON.matches("what is the weather today"));
        assert!(!THINK_ON.matches("thinker's guide"));
    }
}
