use rust_browser_core::Profile;

#[test]
fn brave_bookmarks_merge_folders_without_duplicates_or_internal_urls() {
    let mut profile = Profile::default();
    profile.toggle_bookmark("https://example.org/", "Existing");
    let json = br#"{"roots":{"bookmark_bar":{"type":"folder","children":[{"type":"url","url":"https://example.org/","name":"Duplicate"},{"type":"folder","children":[{"type":"url","url":"https://new.example/","name":"New"},{"type":"url","url":"brave://settings/","name":"Settings"}]}]}}}"#;
    assert_eq!(profile.import_brave_bookmarks(json).unwrap(), 1);
    assert_eq!(profile.bookmarks.len(), 2);
    assert_eq!(profile.bookmarks[0].title, "Existing");
    assert!(profile.import_brave_bookmarks(b"not json").is_err());
    assert_eq!(profile.bookmarks.len(), 2);
}
