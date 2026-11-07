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
