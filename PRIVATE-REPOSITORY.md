# Private repository handoff

Requested owner: `DANIIL1253d`. Requested repository: `rust-browser`.
Visibility: **private**. Creating it and pushing source are authorized by the
owner. Changing its visibility to public requires a new explicit request.

The owner created [DANIIL1253d/Rust-Browser](https://github.com/DANIIL1253d/Rust-Browser)
and granted the GitHub plugin access. Metadata was checked: visibility private,
admin/push permissions available. README was initialized using the plugin.
The plugin supports contents, blobs, trees, commits and refs; it has no repository
creation tool. Do not store tokens or subscription keys in the source tree.

The project uses BROWSER-OVERLAY.json and Initialize-ServoSource.ps1. All browser
modifications are versioned; the pinned official Servo source is downloaded into
the generated build workspace. A clean-checkout bootstrap was tested. Only
browser-core.yml is included as CI; inherited upstream publishing jobs, bundles,
downloaded engines, profiles, runtime logs and private data are excluded. MPL and
third-party notices are preserved. A local complete source snapshot also remains
available for development. Never push changes to Servo's upstream.

## Verified source delivery — 7 November 2026

All 89 source overlay files were published through the authenticated GitHub
plugin in commit [829fc0b](https://github.com/DANIIL1253d/Rust-Browser/commit/829fc0bc1329c6e05881361c3a44ccd4197b000c).
The main branch was advanced with an expected-head lease and without force.
Private visibility was checked before publication. Remote bootstrap, manifest,
security source, logo, preview and CI files were fetched back successfully.
The logo/preview 404 links are repaired. Local checks passed; a green hosted
GitHub Actions run has not yet been established.
