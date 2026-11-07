#[path = "../../../components/shared/embedder/connection.rs"]
mod connection;
#[path = "../../../ports/servoshell/desktop/repaint.rs"]
mod repaint;
#[path = "../../../ports/servoshell/desktop/tab_shortcuts.rs"]
mod tab_shortcuts;

#[cfg(test)]
mod connection_tests {
    use super::connection::{ConnectionInfo, ConnectionSecurity as Security};
    use url::Url;

    #[test]
    fn https_scheme_alone_is_not_verification() {
        let url = Url::parse("https://example.org/").unwrap();
        assert_eq!(
            ConnectionInfo::from_response(url.clone(), None, false).security,
            Security::Unknown
        );
        assert_eq!(
            ConnectionInfo::from_response(url.clone(), Some(true), false).security,
            Security::Secure
        );
        assert_eq!(
            ConnectionInfo::from_response(url, Some(false), false).security,
            Security::Unverified
        );
    }

    #[test]
    fn failed_https_never_inherits_a_lock() {
        let url = Url::parse("https://example.org/").unwrap();
        assert_eq!(
            ConnectionInfo::from_response(url, Some(true), true).security,
            Security::Failed
        );
    }

    #[test]
    fn http_and_local_documents_have_distinct_states() {
        for (url, expected) in [
            ("http://example.org/", Security::Insecure),
            ("servo:newtab", Security::Local),
            ("file:///example.html", Security::Local),
            ("data:text/html,hello", Security::Unknown),
        ] {
            assert_eq!(
                ConnectionInfo::from_response(Url::parse(url).unwrap(), None, false).security,
                expected
            );
        }
    }

    #[test]
    fn connection_evidence_follows_document_identity_not_same_url() {
        use super::connection::ConnectionTracker;
        let url = Url::parse("https://example.org/").unwrap();
        let mut tracker = ConnectionTracker::<u64>::default();
        tracker.record(
            1,
            ConnectionInfo::from_response(url.clone(), Some(false), false),
        );
        tracker.select(1);
        assert_eq!(tracker.current().unwrap().security, Security::Unverified);
        tracker.record(
            2,
            ConnectionInfo::from_response(url.clone(), Some(true), false),
        );
        assert_eq!(tracker.current().unwrap().security, Security::Unverified);
        tracker.select(2);
        assert_eq!(tracker.current().unwrap().security, Security::Secure);
        tracker.record(1, ConnectionInfo::from_response(url, None, true));
        assert_eq!(tracker.current().unwrap().security, Security::Secure);
        tracker.select(1); // BFCache/history traversal, even at the identical URL.
        assert_eq!(tracker.current().unwrap().security, Security::Failed);
        tracker.clear_current();
        assert!(tracker.current().is_none());
    }

    #[test]
    fn forgotten_history_evidence_is_unknown_and_storage_is_bounded() {
        use super::connection::ConnectionTracker;
        let mut tracker = ConnectionTracker::<u64>::default();
        for id in 0..100 {
            tracker.record(
                id,
                ConnectionInfo::from_response(
                    Url::parse("https://example.org/").unwrap(),
                    Some(true),
                    false,
                ),
            );
        }
        tracker.select(0);
        assert!(tracker.current().is_none());
        tracker.select(99);
        assert_eq!(tracker.current().unwrap().security, Security::Secure);
    }
}
