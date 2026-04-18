//! Cross-module view of every [`KeywordGroup`] the agent matches against.
//!
//! Each feature module still owns its own groups; this file is a catalog for
//! tooling (the dashboard overview, tests that enforce no phrase collisions).

use crate::modules::memory;
use crate::modules::ollama;
use crate::modules::skills;
use crate::shared::keywords::KeywordGroup;

pub fn all_keyword_groups() -> Vec<&'static KeywordGroup> {
    vec![
        &memory::SESSION_START,
        &memory::SESSION_END,
        &memory::DIARY_START,
        &memory::DIARY_END,
        &ollama::keywords::THINK_ON,
        &skills::keywords::EXPLICIT_WEB_SEARCH,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn group_ids_are_unique() {
        let mut seen = HashMap::new();
        for g in all_keyword_groups() {
            assert!(
                seen.insert(g.id, ()).is_none(),
                "duplicate keyword group id: {}",
                g.id
            );
        }
    }

    #[test]
    fn every_group_has_at_least_english_phrases() {
        for g in all_keyword_groups() {
            let has_en = g
                .phrases_by_lang
                .iter()
                .any(|(lang, ps)| *lang == "en" && !ps.is_empty());
            assert!(has_en, "group {} is missing English phrases", g.id);
        }
    }
}
