# Roadmap

- [x] Integrated Servo desktop shell, tabs, local new tab and persistent profile
- [x] Direct / automatic / proxy routing for HTTP, HTTPS and WebSockets
- [x] Subscription imports and XHTTP using a separate Xray core
- [x] Windows DPAPI password vault, native manager, CSV import and export
- [x] Configurable theme, accent, compact navigation, optional translucent panels
- [x] Regression test for delayed repaints causing idle CPU usage
- [x] Read-only import of Brave bookmarks and recent history
- [x] Unicode tab search, bounded persistent recently closed tabs and Ctrl+Shift+T
- [x] Main-document connection indicator using TLS evidence and document identity
- [x] Lazy saved-session tabs, deferred activation and isolated 1/5/10-tab memory comparison
- [x] Migrate all 3,797 eligible Brave history URLs on the owner's PC
- [x] GitHub plugin access to the owner's private repository; pinned upstream overlay build
- [x] Receive YouTube/control HTTP 200 through the owner's locally configured browser proxy
- [x] Fix large HTTP/2 response headers rejecting the YouTube page
- [ ] Broader site compatibility and media backend, including YouTube playback
- [ ] Memory-saving tab unloading with meaningful multi-site benchmarks
- [ ] Vertical/pinned tabs and workspaces, with keyboard navigation and persistence
- [ ] Downloads, find-in-page, site permissions and per-site zoom
- [ ] Reproducible signed release installer and updater
- [ ] Public release after explicit owner approval

500 MB is a measured target for a light browsing session, not a guarantee for
arbitrary websites or unlimited active tabs. Authenticated Brave sessions are not
part of the supported importer.
