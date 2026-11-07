/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::{Deserialize, Serialize};
use url::Url;

/// Transport status for the main document, not a reputation or mixed-content verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConnectionSecurity {
    Secure,
    Unverified,
    Insecure,
    Local,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectionInfo {
    pub url: Url,
    pub security: ConnectionSecurity,
    pub protocol: Option<String>,
}

impl ConnectionInfo {
    /// `verified_tls` comes from the response's handshake/verification result.
    /// Absence of TLS evidence must never be interpreted as a secure HTTPS load.
    pub fn from_response(url: Url, verified_tls: Option<bool>, failed: bool) -> Self {
        let security = if failed {
            ConnectionSecurity::Failed
        } else {
            match url.scheme() {
                "https" => match verified_tls {
                    Some(true) => ConnectionSecurity::Secure,
                    Some(false) => ConnectionSecurity::Unverified,
                    None => ConnectionSecurity::Unknown,
                },
                "http" => ConnectionSecurity::Insecure,
                "servo" | "about" | "file" => ConnectionSecurity::Local,
                _ => ConnectionSecurity::Unknown,
            }
        };
        Self {
            url,
            security,
            protocol: None,
        }
    }
}

/// Keep transport evidence tied to a document identity across pending loads and BFCache.
/// Old/cancelled responses cannot replace evidence for the selected document.
pub struct ConnectionTracker<K> {
    selected: Option<K>,
    evidence: std::collections::VecDeque<(K, ConnectionInfo)>,
}

impl<K> Default for ConnectionTracker<K> {
    fn default() -> Self {
        Self {
            selected: None,
            evidence: Default::default(),
        }
    }
}

impl<K: Copy + PartialEq> ConnectionTracker<K> {
    pub fn select(&mut self, document: K) {
        self.selected = Some(document);
    }
    pub fn record(&mut self, document: K, info: ConnectionInfo) {
        self.evidence.retain(|(id, _)| *id != document);
        self.evidence.push_back((document, info));
        while self.evidence.len() > 64 {
            self.evidence.pop_front();
        }
    }
    pub fn current(&self) -> Option<&ConnectionInfo> {
        let selected = self.selected?;
        self.evidence
            .iter()
            .find(|(id, _)| *id == selected)
            .map(|(_, info)| info)
    }
    pub fn clear_current(&mut self) {
        self.selected = None;
    }
}
