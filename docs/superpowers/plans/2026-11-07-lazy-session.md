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
- [ ] Review, package, update reports/checkpoint/wiki, publish through GitHub plugin
  with an expected-head lease, and delete the consumed one-time automation.

Review focus: startup with a non-first active tab; invalid/duplicate URLs; snapshot
or close before deferred activation; activation through keyboard or WebDriver;
short-lived page/about:blank history changes and stale metadata after activation.
