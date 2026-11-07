use rust_browser_core::passwords::{Vault, fill_script, origin};
#[test]
fn editing_site_and_username_replaces_original_without_stale_password() {
    let mut vault = Vault::default();
    vault
        .upsert("https://old.example/", "old-user", "old-password")
        .unwrap();
    vault
        .replace(
            "https://old.example",
            "old-user",
            "https://new.example/login",
            "new-user",
            "new-password",
        )
        .unwrap();
    assert_eq!(vault.entries.len(), 1);
    assert!(vault.matching("https://old.example/").is_empty());
    assert_eq!(vault.entries[0].origin, "https://new.example");
    assert_eq!(vault.entries[0].username, "new-user");
}
#[test]
fn passwords_match_exact_origin_including_scheme_and_port() {
    let mut vault = Vault::default();
    vault
        .upsert(
            "https://accounts.example/login",
            "alice",
            "example-test-password",
        )
        .unwrap();
    assert_eq!(vault.matching("https://accounts.example/profile"), vec![0]);
    for url in [
        "https://evil.accounts.example/",
        "https://accounts.example.evil/",
        "http://accounts.example/",
        "https://accounts.example:8443/",
    ] {
        assert!(vault.matching(url).is_empty());
    }
    assert!(fill_script("https://other.example/", &vault.entries[0]).is_err());
    assert!(origin("javascript:alert(1)").is_err());
}
#[test]
fn updating_one_login_preserves_other_logins() {
    let mut vault = Vault::default();
    vault
        .upsert("https://accounts.example/", "alice", "first")
        .unwrap();
    vault
        .upsert("https://accounts.example/", "bob", "other")
        .unwrap();
    vault
        .upsert("https://accounts.example/login", "alice", "updated")
        .unwrap();
    assert_eq!(vault.entries.len(), 2);
    assert_eq!(vault.entries[0].password, "updated");
    assert_eq!(vault.entries[1].username, "bob");
}
#[cfg(windows)]
#[test]
fn vault_is_dpapi_encrypted_and_corruption_is_preserved() {
    let path = std::env::temp_dir().join(format!("browser-vault-test-{}.dat", std::process::id()));
    let mut vault = Vault::default();
    vault
        .upsert("https://accounts.example/", "alice", "test-secret-9xy")
        .unwrap();
    vault.save(&path).unwrap();
    let encrypted = std::fs::read(&path).unwrap();
    assert!(
        !encrypted
            .windows(b"test-secret-9xy".len())
            .any(|w| w == b"test-secret-9xy")
    );
    assert_eq!(
        Vault::load(&path).unwrap().entries[0].password,
        "test-secret-9xy"
    );
    std::fs::write(&path, b"corrupt vault").unwrap();
    assert!(Vault::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt vault");
    std::fs::remove_file(path).unwrap();
}
