# Next improvements

Priorities selected on 7 November 2026 from this owner's requests and primary
browser documentation. This is a backlog, not a list of implemented features.
Keep each change independently reviewable and preserve existing profile data.

1. **Site and video compatibility.** Inspect the pinned Servo media backend and
   actual YouTube playback requirements. First produce a local HTML5 video/audio
   test; then test a public YouTube video, seeking and playback controls. Page
   loading is already fixed, but that does not establish media playback.
2. **Memory-saving tabs.** Add explicit background-tab unloading and lazy session
   restoration before automatic policies. Show which tabs need a reload; protect
   active tabs and do not silently discard form edits or media. Measure identical
   1/5/10-tab sessions, including child processes, before/after. Brave's
   [Memory Saver](https://support.brave.app/hc/en-us/articles/13383683902733-How-do-I-use-the-Memory-and-Energy-Saver-features-in-Brave)
   demonstrates deactivation/reload and site exceptions; copying that behavior
   requires corresponding state handling in our shell.
3. **Vertical tabs and workspaces.** An optional sidebar, pinned tabs and named
   groups should retain the same navigation and keyboard access. Start with
   grouping, persistence and predictable close/reopen behavior. Zen's
   [workspaces](https://docs.zen-browser.app/user-manual/workspaces) are a useful
   reference. Grouping alone must not be presented as isolated account containers;
   separate cookie/storage partitions require engine support and tests.
4. **Focus mode.** Hide chrome only through an explicit toggle, retain an obvious
   reveal gesture and a keyboard escape route. Zen's
   [compact mode](https://docs.zen-browser.app/user-manual/compact-mode) documents
   edge reveal and persistent toolbar toggles. Our current compact setting only
   reduces navigation height and is not equivalent to this feature.
5. **Everyday completeness.** Downloads with cancellation/progress, find-in-page,
   site permissions, per-site zoom and session/crash recovery. Implement engine
   operations rather than decorative controls that cannot perform them.

Acceptance for each item: focused regression where meaningful, real desktop
build, isolated engine or native runtime check, updated usage documentation and
delivery evidence. Respect reduced-motion/appearance choices, keep network mode
optional, and keep source private until the owner explicitly changes that.
