# Lazy session restoration implementation plan

**Goal:** Restore the selected saved page immediately and defer background network
loads until activation, preserving tab order, URLs, titles and close/reopen behavior.

**Architecture:** Keep ordinary Servo WebViews for stable tab identities, using blank
documents for deferred tabs. Store their intended Page separately, consume it once
on activation, and use that metadata for chrome, search, session snapshots and close.
This reduces eager page work; it is not full tab unloading or a hard memory limit.

**Choices:** A pure metadata-only tab model would release more context memory but
requires a larger shell redesign. Automatic unloading could lose live form/media
state. This iteration uses blank background views only for saved-session restore;
normal active tabs are never discarded.

**Constraints:** Preserve Rust/Servo, private repository, current appearance and
network choices. No Brave/profile secrets or previously rejected GUI launch.

- [x] Add core tests for filtering invalid saved URLs, preserving selected index,
  duplicate tabs and one-time deferred activation. Implement SessionPlan/DeferredTabs.
- [x] Integrate startup, activation, labels/search, capture and closed-tab metadata.
  Ensure close-selected and keyboard/WebDriver activation wake deferred pages.
- [x] Add an explicitly headless session-fixture CLI for isolated engine tests and
  controlled eager/lazy benchmarks; never load the normal profile for these tests.
- [x] Compile, check core/UI tests, verify request counts and activation using
  loopback/WebDriver fixtures, measure identical 1/5/10-tab workloads if feasible.
- [x] Review, package, update reports/checkpoint/wiki, publish through GitHub plugin
  with an expected-head lease, and delete the consumed one-time automation.

Review focus: startup with a non-first active tab; invalid/duplicate URLs; snapshot
or close before deferred activation; activation through keyboard or WebDriver;
short-lived page/about:blank history changes and stale metadata after activation.

Result: 31 core + 8 UI/lifecycle tests passed; nine isolated session/benchmark
cases and five packaged transport cases passed. The generated pinned-Servo
overlay also passed core tests. Review findings were reproduced and fixed;
final follow-up found no Critical/Important issue. Private source delivery:
acf8d7ff1f38516b51219b4874727824d1408eba. Codex-Knowledge lint: 49 sources,
41 pages, zero issues. The one-time automation was deleted; no repeat scheduled.
