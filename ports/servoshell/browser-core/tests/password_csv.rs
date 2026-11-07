use rust_browser_core::passwords::{Vault, export_csv, parse_csv};

#[test]
fn brave_and_firefox_csv_preserve_quotes_newlines_and_unicode() {
    let brave = "name,url,username,password,note\r\nExample,https://example.org/login,юзер,\"a,\"\"b\"\"\nline\",ignored\r\n";
    let entries = parse_csv(brave.as_bytes()).unwrap();
    assert_eq!(entries[0].origin, "https://example.org");
    assert_eq!(entries[0].username, "юзер");
    assert_eq!(entries[0].password, "a,\"b\"\nline");
    let firefox = "\u{feff}\"url\",\"username\",\"password\",\"httpRealm\"\n\"https://other.example:8443/path\",\"mail@example.org\",\"secret\",\"\"\n";
    assert_eq!(
        parse_csv(firefox.as_bytes()).unwrap()[0].origin,
        "https://other.example:8443"
    );
    let round_trip = parse_csv(&export_csv(&entries).unwrap()).unwrap();
    assert_eq!(round_trip[0].password, entries[0].password);
}

#[test]
fn import_never_overwrites_existing_login_and_invalid_input_does_not_leak() {
    let mut vault = Vault::default();
    vault.upsert("https://example.org", "me", "old").unwrap();
    let entries = parse_csv(
        b"url,username,password\nhttps://example.org,me,new\nhttps://new.example,other,next\n",
    )
    .unwrap();
    let (added, skipped) = vault.import_entries(entries);
    assert_eq!((added, skipped), (1, 1));
    assert_eq!(vault.entries[0].password, "old");
    let error = parse_csv(b"url,username,password\nsecret-invalid-url,private-user,secret-value\n")
        .err()
        .unwrap();
    assert!(!error.contains("secret"));
    assert!(!error.contains("private-user"));
    assert!(parse_csv(b"wrong,headers\nprivate,secret\n").is_err());
}

#[test]
fn conflicting_path_specific_credentials_are_rejected_before_import() {
    let csv = b"url,username,password\nhttps://example.org/one,me,first\nhttps://example.org/two,me,second\n";
    let error = parse_csv(csv)
        .err()
        .expect("conflicting passwords must not be lost silently");
    assert!(!error.contains("first"));
    assert!(!error.contains("second"));
    let identical = b"url,username,password\nhttps://example.org/one,me,same\nhttps://example.org/two,me,same\n";
    assert_eq!(parse_csv(identical).unwrap().len(), 1);
}
