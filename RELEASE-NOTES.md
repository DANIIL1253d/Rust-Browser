# Rust Browser v0.1.0 — first preview

An experimental Windows x64 browser built on Servo v0.7.0. This first preview
uses the development build profile and is not an optimized stable release.

## Included

- Native tabbed interface with an integrated title bar, vector logo and one menu.
- Configurable theme, accent, compact navigation, animations and translucent panels.
- Lazy saved-session restoration, tab search and recently closed tabs.
- Main-document connection details based on TLS evidence and document identity.
- Direct / automatic / subscription routing with domain rules and XHTTP support.
- Separate DPAPI password manager with explicit CSV import and export.
- Read-only Brave bookmark/history importer.
- HTTP/2 large-header and incomplete-response error handling fixes.

## Run

Extract the Windows x64 ZIP and run `Start-Browser.cmd`. Keep the `app` directory
intact. Windows may require the Microsoft Visual C++ 2015–2022 x64 runtime.
The executable is unsigned. A clean Windows installation is not yet validated.

The source for this version is the release tag. It contains browser code, build
tools, tests, licenses and portable documentation. The pinned upstream Servo
source is obtained by `Initialize-ServoSource.ps1` when building. Downloaded
network-core binaries are supplied separately in the application ZIP.

## Known limitations

The media backend, DRM, Chrome extensions and Brave Shields are not implemented.
Loading the YouTube page does not establish video playback. A valid local
subscription is required for sites that need proxy routing; it is not bundled.
Authenticated Brave sessions/cookies are not imported. Passwords use an explicit
local CSV workflow. Lazy restoration does not unload already active pages.
No universal 500 MB maximum is guaranteed.

Profiles, subscriptions, credentials, local reports and benchmark results are
excluded from release assets and the source distribution. Third-party notices
and runtime source URLs are included. See `CONTRIBUTORS.md` for project credits.
Servo dependency notices are in `app/resources/resource_protocol/license.html`;
the additional CSV dependency notice is in `licenses/CSV-MIT.txt`.
