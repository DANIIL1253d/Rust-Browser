use rust_browser_core::{Page, Profile};

#[test]
fn history_retains_a_complete_small_browser_migration() {
    let mut profile = Profile::default();
    for n in 0..4000 {
        profile.visit(&format!("https://example.org/{n}"), "History");
    }
    assert_eq!(profile.history.len(), 4000);
}

#[test]
fn closed_tabs_are_lifo_bounded_and_survive_serialization() {
    let mut profile = Profile::default();
    profile.remember_closed("servo:newtab", "Blank");
    assert!(profile.recently_closed.is_empty());
    for n in 0..25 {
        profile.remember_closed(&format!("https://example.org/{n}"), "Страница");
    }
    let mut loaded: Profile =
        serde_json::from_slice(&serde_json::to_vec(&profile).unwrap()).unwrap();
    assert_eq!(loaded.recently_closed.len(), 20);
    assert_eq!(loaded.take_closed().unwrap().url, "https://example.org/24");
    assert_eq!(loaded.take_closed().unwrap().url, "https://example.org/23");
}

#[test]
fn tab_search_matches_all_words_across_title_and_url_without_case() {
    let page = Page {
        title: "Работа на Rust".into(),
        url: "https://example.org/docs".into(),
    };
    assert!(page.matches("РАБОТА docs"));
    assert!(page.matches("  "));
    assert!(!page.matches("Работа missing"));
}
