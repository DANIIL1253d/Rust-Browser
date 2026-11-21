//! Pure state shared by the native media popup and its regression tests.
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Playback {
    #[default]
    Unknown,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Session {
    pub(crate) document: u64,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) playback: Playback,
    pub(crate) position: Option<(f64, f64)>,
}

pub(crate) enum Update {
    Metadata {
        title: String,
        artist: String,
        album: String,
    },
    Playback(Playback),
    Position {
        position: f64,
        duration: f64,
    },
    Ended,
}

pub(crate) struct MediaControls<Id> {
    documents: HashMap<Id, u64>,
    sessions: HashMap<Id, Session>,
}

impl<Id> Default for MediaControls<Id> {
    fn default() -> Self {
        Self {
            documents: HashMap::new(),
            sessions: HashMap::new(),
        }
    }
}

impl<Id: Copy + Eq + Hash> MediaControls<Id> {
    /// Same-document history changes preserve media; replacement documents do not.
    pub(crate) fn document_changed(&mut self, id: Id, document: u64) {
        if self.documents.insert(id, document) != Some(document) {
            self.sessions.remove(&id);
        }
    }

    /// Also blocks queued updates until the next document is committed.
    pub(crate) fn remove(&mut self, id: Id) {
        self.documents.remove(&id);
        self.sessions.remove(&id);
    }

    pub(crate) fn session(&self, id: Id, document: u64) -> Option<&Session> {
        self.sessions
            .get(&id)
            .filter(|session| session.document == document)
    }

    pub(crate) fn update(&mut self, id: Id, document: u64, update: Update) -> bool {
        if self.documents.get(&id) != Some(&document) {
            return false;
        }
        if matches!(update, Update::Ended) {
            return self.sessions.remove(&id).is_some();
        }
        let session = self.sessions.entry(id).or_insert_with(|| Session {
            document,
            ..Session::default()
        });
        match update {
            Update::Metadata {
                title,
                artist,
                album,
            } => {
                session.title = title;
                session.artist = artist;
                session.album = album;
            },
            Update::Playback(playback) => session.playback = playback,
            Update::Position { position, duration } => {
                session.position = (position.is_finite()
                    && duration.is_finite()
                    && duration > 0.0
                    && position >= 0.0
                    && position <= duration)
                    .then_some((position, duration));
            },
            Update::Ended => unreachable!(),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_and_metadata_belong_to_their_own_tab() {
        let mut controls = MediaControls::default();
        controls.document_changed(1, 10);
        controls.document_changed(2, 20);
        controls.update(1, 10, Update::Playback(Playback::Playing));
        controls.update(2, 20, Update::Playback(Playback::Paused));
        controls.update(
            2,
            20,
            Update::Metadata {
                title: "Second track".into(),
                artist: "Artist".into(),
                album: "Album".into(),
            },
        );
        assert_eq!(controls.session(1, 10).unwrap().playback, Playback::Playing);
        assert_eq!(controls.session(1, 10).unwrap().title, "");
        let second = controls.session(2, 20).unwrap();
        assert_eq!(
            (&*second.title, &*second.artist, &*second.album),
            ("Second track", "Artist", "Album")
        );
        assert_eq!(second.playback, Playback::Paused);
    }

    #[test]
    fn replacing_a_document_rejects_old_updates_even_at_the_same_url() {
        let mut controls = MediaControls::default();
        controls.document_changed(1, 10);
        controls.update(1, 10, Update::Playback(Playback::Playing));
        controls.document_changed(1, 11);
        assert!(controls.session(1, 10).is_none());
        assert!(!controls.update(1, 10, Update::Playback(Playback::Playing)));
        assert!(controls.session(1, 11).is_none());
        assert!(controls.update(1, 11, Update::Playback(Playback::Paused)));
    }

    #[test]
    fn same_document_history_preserves_media() {
        let mut controls = MediaControls::default();
        controls.document_changed(1, 10);
        controls.update(1, 10, Update::Playback(Playback::Playing));
        controls.document_changed(1, 10);
        assert_eq!(controls.session(1, 10).unwrap().playback, Playback::Playing);
    }

    #[test]
    fn navigation_and_close_block_delayed_events() {
        let mut controls = MediaControls::default();
        controls.document_changed(1, 10);
        controls.update(1, 10, Update::Playback(Playback::Playing));
        controls.remove(1);
        assert!(!controls.update(1, 10, Update::Playback(Playback::Paused)));
        assert!(controls.session(1, 10).is_none());
        controls.document_changed(1, 11);
        assert!(!controls.update(1, 10, Update::Ended));
    }

    #[test]
    fn ended_session_is_removed_without_removing_other_tabs() {
        let mut controls = MediaControls::default();
        for id in [1, 2] {
            controls.document_changed(id, id);
            controls.update(id, id, Update::Playback(Playback::Playing));
        }
        assert!(controls.update(1, 1, Update::Ended));
        assert!(controls.session(1, 1).is_none());
        assert!(controls.session(2, 2).is_some());
    }

    #[test]
    fn invalid_or_cleared_position_does_not_leave_a_stale_time() {
        let mut controls = MediaControls::default();
        controls.document_changed(1, 1);
        controls.update(
            1,
            1,
            Update::Position {
                position: 3.0,
                duration: 10.0,
            },
        );
        assert_eq!(controls.session(1, 1).unwrap().position, Some((3.0, 10.0)));
        for (position, duration) in [
            (f64::NAN, 10.0),
            (3.0, f64::INFINITY),
            (-1.0, 10.0),
            (11.0, 10.0),
            (0.0, 0.0),
        ] {
            controls.update(1, 1, Update::Position { position, duration });
            assert!(controls.session(1, 1).unwrap().position.is_none());
        }
    }
}
