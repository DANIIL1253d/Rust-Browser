# Rust Browser — verification, 7 November 2026

Latest daytime checkpoint: [COMMUNITY-REPORT.md](COMMUNITY-REPORT.md) covers the
Brave history migration, tab tools, connection security and private GitHub project. The measurements
and earlier limitations below describe the preceding nightly build.

This report describes the local Windows development build, not a public release.
Servo baseline: `aac43a3f31a259f04a574f5ec4e959c943ad7cc7` (v0.7.0).

## Delivered and run

The latest desktop build with the YouTube fix completed successfully in 3m 03s
and was packaged as `app/RustBrowser.exe`. The preceding UI/performance build was
also launched with the owner's existing profile and responded to Windows. The
latest binary was validated with the full headless engine. The original Brave application was not modified
or stopped. A backup of Rust Browser's plain settings/profile JSON was saved
beside that profile before the update; no authentication files were copied.

Binary SHA-256:
`1A09AAD48E41376FC1B4E006FBC9983F72A92389670FD234091F4F9D9C34AA95`.

Compiled features: bundled, clipboard, js_jit, multiprocess, webgl, webcrypto.
Rust stable 1.99.1, MSVC 14.44, Windows SDK, CMake 4.4.4, Ninja 1.13.2,
libclang 18.1.1; development build with debug information and incremental
compilation disabled. The media backend and WebGPU are not enabled.

The framebuffer capture [browser-preview.png](browser-preview.png) is an actual
rendered new tab using an isolated synthetic QA profile. It contains no imported
personal browsing data. This capture precedes only closed-tab favicon cleanup
and the final caption-button focus/press feedback adjustments.

## Behavior and tests

21 offline Rust tests passed: 16 core tests, one UI scheduling regression and four
network routing tests. An additional explicitly run live check passed against
YouTube in direct, auto and proxy modes. Core coverage includes profile behavior, subscription
conversion, exact-origin DPAPI vault behavior, CSV conflicts and Brave bookmark
merging. Routing tests cover direct failure to proxy, proxy mode ignoring
NO_PROXY and replacing pools, WebSocket pre-handshake TLS fallback, and large
valid HTTP/2 response headers over a local TLS connection.

The YouTube failure was reproduced in the full headless engine, with its original
`client error (SendRequest)` page. Restricted diagnostic logging identified
`REQUEST_HEADER_FIELDS_TOO_LARGE` followed by HTTP/2 PROTOCOL_ERROR. The decoded
response-header budget is now explicitly bounded at 256 KiB. A synthetic 48 KiB
header reproduced the failure before the change and passed afterwards. The live
Rust connector then returned HTTP 200 over HTTP/2 in all three routing modes.
This addresses page loading; it does not prove video playback.

The packaged fixed engine subsequently loaded and rendered the YouTube home page
in an isolated headless profile. [Actual capture](youtube-page-check.png). The
previous SendRequest error page is gone. There was one secondary TLS resource
error in the direct-only check; complete site compatibility is not claimed.

The repaint regression was observed failing before the fix and passing after it.
The bug treated a future egui repaint timer as an immediate redraw request. The
shell now schedules that deadline through the native event loop instead of
continuously drawing. Style setup is cached, closed-tab favicon textures are
released, and polling wakes the GUI only when the visible state changes.

Xray checked generated TLS and Reality configuration fixtures. A local XHTTP
client/server test passed all four modes: packet-up, stream-up, stream-one, auto.
Three sing-box configuration fixtures passed its native checker. These synthetic
checks do not validate the owner's actual subscription or an overseas route.

Reproduce the Rust checks through Build-Browser.ps1 with -CoreTests, -UiTests,
and -NetworkTests, passing the local Python executable using -PythonPath.

## Performance observed on this PC

| Build/workload | Interval | CPU, normalized over 12 logical cores | Working set | Private committed memory |
| --- | ---: | ---: | ---: | ---: |
| Previous build, owner's existing pages | 41.7 s | 4.48% | 253.3 MiB | 366.3 MiB |
| Fixed build, idle isolated new tab | 64.4 s | 0.022% | 144.3 MiB | 214.8 MiB |

These are process measurements without trimming the working set. Workloads differ,
so the memory numbers are not a controlled percentage comparison. They demonstrate
low idle usage; they do not establish a 500 MB maximum for heavy sites, video,
many live tabs, or network child processes. Tab unloading remains roadmap work.

## Interface changes

- Borderless main window; caption controls share the tab strip.
- The permanent logo also provides a drag region; the new-tab button sits outside
  horizontal tab scrolling. Minimum main-window size is 640 × 420.
- Heavier Segoe UI Semibold chrome; centered URL text; no bookmark bar.
- One menu opens network, settings and the separate native password manager.
- Settings have close/Done/Escape handling; floating panels retain pointer input.
- Original geometric shield icon replaces R in chrome, new tab and Windows assets.
- Optional translucent panels and short UI transitions, each configurable.
  The glass option does not implement physical refraction or a desktop blur.
- Page-load errors offer retry and a route to network/subscription settings.

## Brave import and remaining work

The read-only importer was invoked against the local Default profile. It contains
zero eligible web bookmarks. Brave is still open and holds an exclusive lock on
its history database, so history was not imported. Close Brave and use
Settings → Import Brave bookmarks and history to retry. Up to 500 recent URLs
are supported; bookmark folders are flattened and existing bookmarks retained.

Cookies, authenticated sessions, extensions and site settings are not transferred.
Passwords use the separate manager's explicit CSV import workflow. Active logins
cannot be reported as migrated.

Initially the subscription was not connected. The owner subsequently imported it,
and a public YouTube request through the browser's own loopback proxy returned
HTTP 200. A control request to example.com also returned HTTP 200. No subscription
keys or decrypted configuration were read into the development logs. The current
saved route mode is auto: direct connections first, subscription on a failed
connection, with explicit domain exceptions. Full YouTube playback is still not
verified. Successful routing does not add media support to this Servo build;
enabling and validating a media backend is still needed.

GitHub connector account DANIIL1253d was identified, but it cannot create a
repository. The separate GitHub browser session required login, and the CLI
device authorization expired without completion. No repository has been created
or pushed. A clean complete source snapshot, project README, credits, roadmap
and Windows core/UI CI workflow are prepared in a local source commit. It must remain private
when uploaded; the Servo upstream remote is not a publication destination.
