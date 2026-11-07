# Rust Browser 0.1 — 2026-11-06

The user requested a Rust browser using Servo, with a familiar Brave-like interface,
and access from Russia using an existing Happ-compatible subscription without opening
a separate VPN app. Start implementation now; use Windows and dark mode as defaults.

Use Servo 0.7.0 and its existing servoshell for real rendering, input, tabs, dialogs,
and navigation. Keep upstream MPL notices. Change the desktop chrome to horizontal
tabs above the navigation bar, neutral dark surfaces, rounded address bar, warm
orange accent, bookmarks, history and a restrained local new-tab page. Working name:
Rust Browser. This is a first development version, with Servo's compatibility limits.

Persist bookmarks, history and the current session in an independent browser profile.
Never read the user's Brave profile. Restore tabs in their existing order; reject
corrupt profile data without overwriting it. Browser-core owns profile and subscription
parsing. The shell owns UI and process lifetime.

Use a bundled official sing-box executable as a browser-only local HTTP CONNECT proxy.
No system proxy or TUN settings are changed. Import HTTPS subscription contents or
individual VLESS, VMess, Trojan or Shadowsocks URLs. Support ordinary base64 lists.
Explicitly reject unsupported transports instead of silently changing them. Store
connection secrets with Windows DPAPI; never print subscription URLs or node secrets.
The core listens on loopback, starts with the browser, and stops when it exits.
Connection status is about the local process until a real remote check succeeds.
Auto mode attempts direct DNS/TCP/TLS for three seconds before proxy fallback, with
ten-minute per-domain/port caching and explicit direct/proxy domain overrides.
WebSockets use the same policy, including TLS establishment before sending a handshake.
Route changes replace idle/pooled HTTP clients for subsequent requests.
All sites cannot be guaranteed; network restrictions and Servo features both matter.

The user subsequently requested a password manager. Store credentials in a separate
DPAPI-encrypted vault. Add/edit/delete locally; capture filled form values only by an
explicit button, then require Save. Fill only by explicit action, with an exact
scheme/host/port match checked both in Rust and at JavaScript execution time. Do not
submit forms. No automatic password interception or cloud synchronization in v0.1.

Approaches: a new embedding would require reimplementing input and rendering plumbing;
an external Servo window would not be an integrated browser. Adapt the official shell.
Do not claim Brave Shields, Chromium extensions or full Brave compatibility.

Validation: focused Rust tests for real profile behavior and subscription conversion;
compile the actual Servo shell; run sing-box's own config checker against generated
fixtures; launch the Windows app and inspect a rendered screenshot when available.

2026-11-06 follow-up: remove the native Windows title bar, place window controls
on the tab strip, and make its empty space draggable. One menu contains network,
passwords, history, bookmarks and settings. Persist light/dark chrome, four accent
colors, compact navigation, bookmark visibility, startup restoration and address-bar
search provider. Add XHTTP via the official Xray core, retaining the same loopback
HTTP proxy and job-object lifetime. Validate generated XHTTP configurations with
Xray itself. A user's real subscription remains unverified until tested locally.
