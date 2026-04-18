//! Brave gating from **skills** (frontmatter). Keyword-only rules are tested in
//! `modules/skills/keywords.rs`.

mod common;

use pengine_lib::modules::skills::service::{
    allow_brave_web_search_for_message, write_custom_skill,
};
use tempfile::tempdir;

#[test]
fn brave_allowed_when_skill_substring_matches_umlaut_user_text() {
    let tmp = tempdir().unwrap();
    let store = tmp.path().join("connection.json");
    let md = "---\nname: t\ndescription: d\ntags: [toolong]\nrequires: [brave_web_search]\nbrave_allow_substrings: [oesterreich]\n---\n\nbody\n";
    write_custom_skill(&store, "bravegate-a", md, None).unwrap();
    assert!(allow_brave_web_search_for_message(
        &store,
        "Kurz zu Österreich und Pension"
    ));
}

#[test]
fn brave_allowed_when_skill_requires_and_substring_matches() {
    let tmp = tempdir().unwrap();
    let store = tmp.path().join("connection.json");
    let md = "---\nname: t\ndescription: d\ntags: [gov]\nrequires: [brave_web_search]\nbrave_allow_substrings: [widgets]\n---\n\nbody\n";
    write_custom_skill(&store, "bravegate-b", md, None).unwrap();
    assert!(!allow_brave_web_search_for_message(&store, "hello world"));
    assert!(allow_brave_web_search_for_message(
        &store,
        "tell me about widgets"
    ));
}

#[test]
fn brave_not_enabled_by_generic_news_tag() {
    let tmp = tempdir().unwrap();
    let store = tmp.path().join("connection.json");
    let md = "---\nname: t\ndescription: d\ntags: [news, gaming]\nrequires: [brave_web_search]\n---\n\nbody\n";
    write_custom_skill(&store, "bravegate-c", md, None).unwrap();
    assert!(!allow_brave_web_search_for_message(
        &store,
        "gameinformer news"
    ));
}

#[test]
fn brave_blocked_for_portal_skill_slug_without_admin_keywords() {
    let tmp = tempdir().unwrap();
    let store = tmp.path().join("connection.json");
    let md = "---\nname: t\ndescription: d\ntags: []\nrequires: [brave_web_search]\nbrave_allow_substrings: [oesterreich]\n---\n\nbody\n";
    write_custom_skill(&store, "austria-gv-data", md, None).unwrap();
    assert!(!allow_brave_web_search_for_message(&store, "hello random"));
    assert!(allow_brave_web_search_for_message(
        &store,
        "Infos von oesterreich.gv"
    ));
}
