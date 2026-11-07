# Rust Browser — community improvements, 7 November 2026

The later 17:30 continuation added lazy session restoration, measured reduced
startup memory and repaired selection/navigation races. Current package and
validation: [SESSION-REPORT.md](SESSION-REPORT.md). The sections below record the
earlier daytime delivery; its binary hash and test counts are historical.

The final desktop build includes tab tools, the profile-only Brave importer,
connection evidence and truncated-response/cache fixes. The security integration
build completed in 2m 48s; the final networking increment completed in 29.95s.
The binary was packaged in app/RustBrowser.exe. All five isolated engine cases
passed on that exact package. SHA-256 is recorded in BUILD-MANIFEST.json.
The previous YouTube HTTP/2 page-loading fix remains included.

## Brave migration

The source Default profile was available read-only. All 3,797 eligible HTTP(S)
history URLs were imported; the destination has 3,798 unique history entries
including existing browser data. A set comparison found zero missing source
URLs. No personal URLs or titles were printed in development logs. Source
bookmarks contained zero eligible entries. A timestamped backup of the plain
Rust Browser profile was saved in its own local profile directory first.

The importer does not copy cookies, authenticated sessions, passwords, payment
data or extensions. Passwords use the manager's explicit CSV workflow; Chromium
extensions are not supported by this Servo build. This is not a full-profile or
active-account transfer. Brave was not modified. A final set comparison still found zero missing source
URLs; the destination subsequently contained 3,799 entries including new browser
history added since the initial migration.

## Features selected from user feedback

Primary sources, dated requests and decisions: [COMMUNITY-IMPROVEMENTS.md](COMMUNITY-IMPROVEMENTS.md).

- Ctrl+Shift+A and the menu open a searchable list of this window's tabs.
  Search matches words across title and URL, including Unicode case conversion.
  Enter activates the first match; Escape closes the panel.
- Ctrl+Shift+T and the menu reopen the last closed web tab. The 20-entry stack is
  stored locally and survives a restart. Internal blank pages are excluded.
- The new-tab button was already outside the horizontal scroll area, matching
  the overflow issue reported by Zen users.
- History capacity is now bounded at 10,000 instead of 500, allowing this owner's
  complete visible Brave web history to fit.

19 core tests passed, including three new regression tests for history capacity,
recent-tab persistence/bounds and Unicode tab matching. Eight standalone UI and
connection-lifecycle tests passed, as did eight offline network regressions. The
core and UI suites also passed from the generated pinned-Servo workspace.
Menu/shortcut interaction still needs a native interactive smoke check;
compile/core results are not that proof. Full YouTube playback and a 500 MB
maximum remain unverified.

## Connection indicator

The missing-glyph marker is replaced with vector lock, unlocked-lock, warning
and information icons. Clicking shows connection details for the committed main
document, rather than the editable address text. HTTPS alone is insufficient:
the lock requires response TLS evidence. Certificate exceptions or the test-only
ignore flag explicitly downgrade the status. HTTP, local pages, pending/unknown
state and failed loads have separate explanations.

Security evidence crosses the script/embedder boundary with PipelineId and is
selected by the committed history pipeline. A bounded tracker prevents an old
response or same-origin BFCache document from inheriting another page's lock.
An EOF error replaces stored evidence before parser initialization, so a short
truncated response cannot restore a previous protected status. Security-only
changes request a repaint. The indicator does not assess site reputation or
report a full mixed-content audit; those limits are stated in its details.

The packaged-engine test reproduced an upstream body-transfer error being
reported as successful completion. The loader now forwards non-decompression I/O
failures too, marks incomplete cache bodies aborted, and preserves errors or
cancellation for waiting cache consumers. Two regressions failed before this
change and passed after it. The five isolated engine cases passed after the
transfer-error fix; the final delivered package passed the same five cases again, including live
verified HTTPS. No profile, vault or subscription files were created in the
isolated headless profiles.

The packaged-engine fixture check is tests/browser-security/smoke.py. It runs
HTTP, rejected/bypassed TLS and a small truncated TLS response, with separate
temporary profiles, plus an optional live verified HTTPS page. It also detects
accidental creation of the normal profile/vault files during headless testing.

## GitHub source delivery

The owner created the private repository
[DANIIL1253d/Rust-Browser](https://github.com/DANIIL1253d/Rust-Browser) and granted
the plugin access. The plugin verified private visibility and push/admin access.
README was initialized through the plugin; source publication uses the same
plugin's blob/tree/commit/ref APIs. All 89 overlay files were published in
commit [829fc0b](https://github.com/DANIIL1253d/Rust-Browser/commit/829fc0bc1329c6e05881361c3a44ccd4197b000c).
Key source, build, artwork and workflow files were fetched back successfully.
The branch update used a lease, and visibility remains private. Hosted CI is
not claimed passing; local test/build and packaged-engine evidence is above.

The repository versions all browser modifications, licenses, artwork, build tools
and CI. The large upstream Servo/WPT corpus is not vendored: Initialize-ServoSource.ps1
downloads the exact Servo revision aac43a3f31a259f04a574f5ec4e959c943ad7cc7, selects
the build sources, verifies the revision and applies BROWSER-OVERLAY.json to a
generated workspace. This clean-checkout path was exercised against the official
public source, followed by passing core and UI tests.

The earlier complete source snapshot is retained locally. No personal profiles,
password exports, cookie databases, subscription keys, runtime logs or downloaded
network-engine executables are included in the published source payload.

## Next iteration

The owner requested autonomous improvement and a one-time continuation at 17:30
Moscow time on 7 November. That continuation ran; its schedule was deleted through
automation_update. No repeat was created. The ordered backlog and acceptance criteria are in
NEXT-IMPROVEMENTS.md: media compatibility, memory-saving/lazy tabs, optional
vertical tabs/workspaces, focus mode and essential browser operations.
