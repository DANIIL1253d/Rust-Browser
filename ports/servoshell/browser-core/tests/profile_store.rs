use rust_browser_core::profile_store::ProfileStore;
use rust_browser_core::{Profile, SearchEngine};
use std::fs::{self, File, FileTimes};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

struct SyntheticProfile(PathBuf);
impl SyntheticProfile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rust-browser-profile-store-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("profile.json")
    }
}
impl Drop for SyntheticProfile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unchanged_autosaves_keep_profile_bytes_and_modified_time() {
    let directory = SyntheticProfile::new();
    let mut profile = Profile::default();
    profile.visit("https://example.test/", "Synthetic page");
    profile.save(&directory.path()).unwrap();
    // An old timestamp makes an accidental rewrite detectable without a timed sleep.
    let marker = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    File::options()
        .write(true)
        .open(directory.path())
        .unwrap()
        .set_times(FileTimes::new().set_modified(marker))
        .unwrap();
    let before = fs::read(directory.path()).unwrap();
    let modified = fs::metadata(directory.path()).unwrap().modified().unwrap();
    let mut store = ProfileStore::new(directory.path(), Some(profile.clone()));

    for _ in 0..5 {
        assert!(!store.flush(&profile).unwrap());
    }

    assert_eq!(fs::read(directory.path()).unwrap(), before);
    assert_eq!(
        fs::metadata(directory.path()).unwrap().modified().unwrap(),
        modified
    );
    assert!(!directory.0.join("profile.json.tmp").exists());
}

#[test]
fn history_and_settings_changes_are_persisted() {
    let directory = SyntheticProfile::new();
    let mut profile = Profile::default();
    let mut store = ProfileStore::new(directory.path(), None);
    assert!(store.flush(&profile).unwrap());

    profile.visit("https://example.test/visited", "Visited synthetic page");
    assert!(store.flush(&profile).unwrap());
    let loaded = Profile::load(&directory.path()).unwrap();
    assert_eq!(loaded.history[0].url, "https://example.test/visited");

    profile.appearance.search_engine = SearchEngine::Google;
    profile.appearance.compact = true;
    assert!(store.flush(&profile).unwrap());
    let loaded = Profile::load(&directory.path()).unwrap();
    assert!(loaded.appearance.search_engine == SearchEngine::Google);
    assert!(loaded.appearance.compact);
    assert!(!store.flush(&profile).unwrap());
}

#[test]
fn final_shutdown_flush_persists_changes_before_the_next_autosave() {
    let directory = SyntheticProfile::new();
    let mut profile = Profile::default();
    let mut store = ProfileStore::new(directory.path(), None);
    store.flush(&profile).unwrap();

    // Browser shutdown calls the same flush directly, without waiting for its poll timer.
    profile.visit("https://example.test/final", "Final synthetic page");
    profile.appearance.glass = true;
    assert!(store.flush(&profile).unwrap());
    drop(store);

    let loaded = Profile::load(&directory.path()).unwrap();
    assert_eq!(loaded.history[0].url, "https://example.test/final");
    assert!(loaded.appearance.glass);
}

#[test]
fn failed_write_does_not_mark_the_profile_as_saved() {
    let directory = SyntheticProfile::new();
    let blocked_parent = directory.0.join("blocked");
    fs::write(&blocked_parent, b"not a directory").unwrap();
    let path = blocked_parent.join("profile.json");
    let profile = Profile::default();
    let mut store = ProfileStore::new(path.clone(), None);
    assert!(store.flush(&profile).is_err());

    fs::remove_file(&blocked_parent).unwrap();
    assert!(store.flush(&profile).unwrap());
    assert!(path.is_file());
}
