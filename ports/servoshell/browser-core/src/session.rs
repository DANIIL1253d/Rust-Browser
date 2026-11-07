//! Saved-session planning and intended page metadata for not-yet-loaded tabs.
use crate::{Page, Profile};
use std::{collections::HashMap, hash::Hash};
use url::Url;

#[derive(Clone)]
pub struct SessionPlan {
    pub pages: Vec<Page>,
    pub active: usize,
}

impl SessionPlan {
    pub fn from_profile(profile: &Profile) -> Option<Self> {
        let selected = profile
            .active_tab
            .min(profile.session.len().saturating_sub(1));
        let titles: HashMap<_, _> = profile
            .history
            .iter()
            .rev()
            .map(|p| (p.url.as_str(), p.title.as_str()))
            .collect();
        let kept: Vec<_> = profile
            .session
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                let url = Url::parse(text).ok()?;
                let supported = matches!(url.scheme(), "http" | "https" | "file")
                    || (url.scheme() == "servo" && url.path() == "newtab")
                    || (url.scheme() == "about" && url.path() == "blank");
                supported.then(|| {
                    (
                        index,
                        Page {
                            title: titles
                                .get(text.as_str())
                                .copied()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(text)
                                .to_string(),
                            url: url.to_string(),
                        },
                    )
                })
            })
            .collect();
        if kept.is_empty() {
            return None;
        }
        let active = kept
            .iter()
            .position(|(index, _)| *index == selected)
            .unwrap_or_else(|| {
                kept.iter()
                    .rposition(|(index, _)| *index < selected)
                    .unwrap_or(0)
            });
        Some(Self {
            pages: kept.into_iter().map(|(_, page)| page).collect(),
            active,
        })
    }
}

/// Separate metadata keeps blank placeholder documents out of session/history UI.
pub struct DeferredTabs<K> {
    pages: HashMap<K, (Page, LoadPhase)>,
}
enum LoadPhase {
    Deferred { ready: bool },
    Requested,
    Loading,
    Replacing { loading_blank: bool },
}
impl<K> Default for DeferredTabs<K> {
    fn default() -> Self {
        Self {
            pages: HashMap::new(),
        }
    }
}
impl<K: Copy + Eq + Hash> DeferredTabs<K> {
    pub fn insert(&mut self, id: K, page: Page) {
        self.pages
            .insert(id, (page, LoadPhase::Deferred { ready: false }));
    }
    pub fn get(&self, id: K) -> Option<&Page> {
        self.pages.get(&id).map(|(page, _)| page)
    }
    pub fn take(&mut self, id: K) -> Option<Page> {
        self.pages.remove(&id).map(|(page, _)| page)
    }
    pub fn is_deferred(&self, id: K) -> bool {
        self.pages
            .get(&id)
            .is_some_and(|(_, phase)| matches!(phase, LoadPhase::Deferred { .. }))
    }
    pub fn track_loading(&mut self, id: K, page: Page) {
        self.pages.insert(id, (page, LoadPhase::Loading));
    }
    pub fn begin_activation(&mut self, id: K) -> Option<Page> {
        let (page, phase) = self.pages.get_mut(&id)?;
        match phase {
            LoadPhase::Deferred { ready: true } => {
                *phase = LoadPhase::Loading;
                Some(page.clone())
            },
            LoadPhase::Deferred { ready: false } => {
                *phase = LoadPhase::Requested;
                None
            },
            LoadPhase::Requested | LoadPhase::Loading | LoadPhase::Replacing { .. } => None,
        }
    }
    pub fn did_navigate(&mut self, id: K, url: &str) -> Option<Page> {
        let (page, phase) = self.pages.get_mut(&id)?;
        match phase {
            LoadPhase::Deferred { ready } if url == "about:blank" => *ready = true,
            LoadPhase::Requested if url == "about:blank" => {
                *phase = LoadPhase::Loading;
                return Some(page.clone());
            },
            LoadPhase::Replacing { loading_blank } if url != "about:blank" || *loading_blank => {
                *phase = LoadPhase::Loading;
                return Some(page.clone());
            },
            LoadPhase::Loading if url != "about:blank" || page.url == "about:blank" => {
                self.pages.remove(&id);
            },
            _ => {},
        }
        None
    }
    pub fn retarget_waiting(&mut self, id: K, page: Page) -> bool {
        let Some((target, phase)) = self.pages.get_mut(&id) else {
            return false;
        };
        match phase {
            LoadPhase::Deferred { ready: false } | LoadPhase::Requested => {
                *phase = LoadPhase::Requested
            },
            LoadPhase::Loading => {
                *phase = LoadPhase::Replacing {
                    loading_blank: target.url == "about:blank",
                }
            },
            LoadPhase::Replacing { .. } => {},
            LoadPhase::Deferred { ready: true } => return false,
        }
        *target = page;
        true
    }
    pub fn len(&self) -> usize {
        self.pages.len()
    }
}

impl Profile {
    /// Persist exactly the visible page list; an uncommitted empty URL cannot skew selection.
    pub fn remember_session(&mut self, tabs: impl IntoIterator<Item = (bool, Page)>) {
        let pages: Vec<_> = tabs
            .into_iter()
            .filter(|(_, page)| !page.url.is_empty())
            .collect();
        if pages.is_empty() {
            return;
        }
        self.active_tab = pages
            .iter()
            .position(|(active, _)| *active)
            .unwrap_or_else(|| self.active_tab.min(pages.len() - 1));
        self.session = pages.into_iter().map(|(_, page)| page.url).collect();
    }
}
