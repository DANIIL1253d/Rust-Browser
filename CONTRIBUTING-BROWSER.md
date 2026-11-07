# Contributing to Rust Browser

Keep changes small and state the user-visible result, verification and remaining
limits. The upstream Servo baseline is v0.7.0 at
`aac43a3f31a259f04a574f5ec4e959c943ad7cc7`; retain upstream notices and history references.

Run the browser-core tests after profile, password or subscription changes. Run
the standalone UI scheduling tests after event-loop changes. Networking changes
also require the local HTTP CONNECT/WebSocket integration tests. Desktop changes
must compile the real shell and be checked in the actual app.

Never commit a browser profile, cookies, authentication files, subscription keys,
password exports, local screenshots of personal browsing, or runtime logs. Use
synthetic fixtures. Generated binaries and downloaded network engines stay outside
Git; their exact versions, licenses and archive hashes are documented in `runtime`.

Do not claim Chromium/Brave/Zen compatibility from a successful Rust build. Report
runtime and device-specific evidence separately. Repository visibility remains
private until its owner explicitly requests publication.
