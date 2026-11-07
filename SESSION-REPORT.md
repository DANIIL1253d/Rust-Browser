# Lazy session restoration — 7 November 2026, 17:30 continuation

Saved background pages now wait until selection before making their page requests.
The selected saved page loads immediately, including when it was in the middle
of the tab strip. Normal active pages are never automatically discarded.

## Behavior

Tab order, duplicates, intended URLs and saved titles survive startup. A clock
marks a deferred page; search, the address bar, session snapshots and recently
closed tabs use its intended URL rather than the blank placeholder document.
Mouse, keyboard, search and WebDriver selection share the activation path.
Closing the selected tab activates its successor through that same path.

The shell waits for the initial blank browsing context to commit before sending
a deferred load. It retains the latest typed navigation if another initial load
is still committing, then sends it after that commit; the intermediate load may
still have to finish. Losing window focus preserves the saved selected index.
The connection control remains pending until the selected document has real
connection evidence; deferred HTTPS text cannot produce a protected lock.

This is not complete tab unloading. Each saved tab still has a blank Servo
WebView, and memory grows when pages are selected. Automatic live-tab discard,
form-state restoration, media protection rules and full per-tab navigation history
are not implemented by this change.

## Packaged-engine verification

The final desktop increment compiled successfully in 16.81 seconds with MSVC,
stable Rust, jobs=2 and the same development features as the preceding package.
The bundle is app/RustBrowser.exe; its SHA-256 is in BUILD-MANIFEST.json.

31 core tests passed, including 12 session regressions, and 8 standalone UI /
connection-lifecycle tests passed. The pre-existing 8 network regressions remain
unchanged from the daytime passing run. Two review findings (unfocused selection
and a second navigation during an initial load) failed before their fixes and
passed afterward. The earlier activation-before-blank race also has a failing /
passing regression and a real-engine case.

Nine isolated headless WebDriver cases passed on the final executable:

- Three functional cases: ordinary deferred activation, activation before the
  initial blank commit, and replacing an initial in-flight URL. Each checks page
  request counts, final committed URL, return to the selected page without a
  second request, and the early session snapshot before commits.
- Six comparison cases: eager and lazy restore with 1, 5 and 10 tabs.

All five connection cases also passed on the final app bundle: HTTP, rejected
TLS, explicitly bypassed TLS, a truncated TLS response and live verified HTTPS
at example.org. No normal profile/vault/subscription file was created. The session
fixture disables persistent engine configuration before preference loading and
uses temporary storage. The transport fixture now explicitly supplies its own
temporary engine config directory and temporary storage as well.

Native interactive menus and keyboard interaction were not launched in this run;
headless activation and compilation do not establish that visual interaction.
YouTube playback, DRM, extensions and a hard 500 MB maximum remain unverified.

## Controlled synthetic memory comparison

Each loopback page retains and fills a 24 MiB JavaScript Uint8Array. A fresh owned
headless process is used for each case. Eager mode loads all saved pages; normal
lazy mode loads just the selected page. Both use identical URLs and the same
executable. Eager pages are checked ready before measurement. The functional
cases are separate and excluded from this table.

| Saved tabs | Loaded pages eager / lazy | Median working set, MiB eager / lazy | Median private commit, MiB eager / lazy |
| --- | --- | --- | --- |
| 1 | 1 / 1 | 128.53 / 128.65 | 88.79 / 89.05 |
| 5 | 5 / 1 | 243.46 / 145.40 | 231.84 / 133.30 |
| 10 | 10 / 1 | 383.07 / 161.11 | 404.50 / 180.30 |

For this ten-tab workload, working set was about 58% lower; private commit about
55% lower. This demonstrates avoided background page allocations, not a universal
percentage improvement or a real-website memory cap. Empty WebViews still have
a cost. These are development/headless measurements, not a release GUI benchmark.

Each measurement uses 15 samples at 0.2-second intervals after a 1.5-second settle.
Win32 process-tree enumeration includes descendants; all observed cases had one
process. Summed working sets can count shared pages twice when child processes
exist; private commit is recorded separately. CPU is normalized by 12 logical
processors. The six comparison cases recorded 0–0.043% over the short three-second
interval; that resolution is not a sustained idle-CPU guarantee. Startup timings
are also included in the raw results, but no startup speed claim is made.

Reproduce after packaging:

```powershell
python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-check --benchmark
python tests/browser-security/smoke.py --app app/RustBrowser.exe --live
```

Raw synthetic measurements: [results JSON](tests/browser-session/results-2026-11-07.json).
The test runner uses only loopback pages and its own process; it never loads the
owner's browser profile. The normal application retains the user's route and
appearance settings. No Brave import was repeated.

## References and continuation

Source was published through the GitHub plugin in private commit
[acf8d7f](https://github.com/DANIIL1253d/Rust-Browser/commit/acf8d7ff1f38516b51219b4874727824d1408eba).
The 96-file overlay manifest, new module, report, README and logo were fetched
back after the leased branch update. The updated core suite also passed after
applying the overlay to the generated official pinned-Servo workspace.

The historical feature request is [Mozilla Bugzilla #710994](https://bugzilla.mozilla.org/show_bug.cgi?id=710994).
Mozilla's [session-store events documentation](https://firefox-source-docs.mozilla.org/browser/components/sessionstore/docs/events.html)
describes selection-driven restoration. This is a behavior reference, not copied
Firefox code or a claim that the engine architectures are equivalent.
Process metrics use Microsoft's [PROCESS_MEMORY_COUNTERS_EX](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)
and [PROCESSENTRY32W](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/ns-tlhelp32-processentry32w)
definitions.

The consumed rust-browser-17-30 automation was deleted through the app tool.
No new repeats were scheduled. Continue from CONTINUE.md; next priorities remain
media compatibility and explicit safe tab unloading, one measured change at a time.
