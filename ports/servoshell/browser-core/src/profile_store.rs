//! Skip unchanged profile writes while retaining an explicit final shutdown flush.
use crate::Profile;
use std::path::PathBuf;

pub struct ProfileStore {
    path: PathBuf,
    saved: Option<Profile>,
}

impl ProfileStore {
    /// `saved` is the profile successfully loaded from this path, if it existed.
    pub fn new(path: PathBuf, saved: Option<Profile>) -> Self {
        Self { path, saved }
    }

    /// Return whether the file was written. Failed saves remain pending for retry.
    pub fn flush(&mut self, profile: &Profile) -> Result<bool, String> {
        if self.saved.as_ref() == Some(profile) {
            return Ok(false);
        }
        profile.save(&self.path)?;
        self.saved = Some(profile.clone());
        Ok(true)
    }

    /// Adopt a profile persisted by the importer, which also creates its own backup.
    pub fn accept_saved(&mut self, profile: &Profile) {
        self.saved = Some(profile.clone());
    }
}
