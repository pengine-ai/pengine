//! User-message phrases that mean “search the open web” for Brave gating.
//!
//! All **keyword-only** Brave rules live here and are registered in
//! [`crate::modules::keywords::all_keyword_groups`]. [`super::service::allow_brave_web_search_for_message`]
//! calls [`brave_search_allowed_by_keywords`] first; skills add their own gates via frontmatter.

use crate::shared::keywords::{normalize, KeywordGroup, MatchMode};

const EXPLICIT_WEB_SEARCH_EN: &[&str] = &[
    "search the internet",
    "search the web",
    "web search",
    "duckduckgo",
];

const EXPLICIT_WEB_SEARCH_DE: &[&str] = &[
    "suche im internet",
    "suche im internt",
    "suche mir im internet",
    "such mir im internet",
    "such mir im web",
    "such mal im internet",
    "im internet suchen",
    "im web suchen",
    "finde mir im internet",
    "seachr",
    "internetrecherche",
    "recherche im internet",
    "online recherchieren",
    "google mal",
];

/// Substrings that count as an explicit request to search the public web
/// (exposes billed `brave_web_search` when no skill match is needed).
pub const EXPLICIT_WEB_SEARCH: KeywordGroup = KeywordGroup {
    id: "skills.explicit_web_search",
    description: "User asked to search the open web; allow billed brave_web_search for this turn.",
    mode: MatchMode::Substring,
    phrases_by_lang: &[
        ("en", EXPLICIT_WEB_SEARCH_EN),
        ("de", EXPLICIT_WEB_SEARCH_DE),
    ],
};

/// After `suche nach`, require an explicit web intent nearby (same idea as phrase list; kept
/// separate because `suche nach` alone matches too many German sentences).
const SUCHE_NACH_WEB_CONTEXT: &[&str] =
    &["internet", "online", "im web", "bei google", "duckduckgo"];

/// True when the message matches catalogued **search keywords** (phrase group + `suche nach` rule).
/// Does not evaluate skills; see [`super::service::allow_brave_web_search_for_message`].
pub fn brave_search_allowed_by_keywords(user_message: &str) -> bool {
    if EXPLICIT_WEB_SEARCH.matches(user_message) {
        return true;
    }
    let u = normalize(user_message);
    u.contains("suche nach") && SUCHE_NACH_WEB_CONTEXT.iter().any(|t| u.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_internet_phrases_match() {
        assert!(brave_search_allowed_by_keywords(
            "bitte suche im Internet nach X"
        ));
        assert!(brave_search_allowed_by_keywords(
            "such mir im internet rezepte"
        ));
    }

    #[test]
    fn english_phrases_match() {
        assert!(brave_search_allowed_by_keywords(
            "search the internet for penguins"
        ));
    }

    #[test]
    fn gameinformer_news_does_not_enable_brave_via_keywords() {
        assert!(!brave_search_allowed_by_keywords("gameinformer news"));
    }

    #[test]
    fn non_web_lookup_does_not_match() {
        assert!(!brave_search_allowed_by_keywords(
            "Suche Informationen im Österreich GV über X."
        ));
    }

    #[test]
    fn suche_nach_requires_web_context() {
        assert!(!brave_search_allowed_by_keywords(
            "suche nach something vague"
        ));
        assert!(brave_search_allowed_by_keywords(
            "suche nach topic im internet"
        ));
    }

    #[test]
    fn explicit_group_matches_same_as_brave_keywords_helper() {
        assert_eq!(
            EXPLICIT_WEB_SEARCH.matches("search the web for x"),
            brave_search_allowed_by_keywords("search the web for x")
        );
    }
}
