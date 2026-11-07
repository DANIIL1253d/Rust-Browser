# Rust Browser Implementation Plan

> Execution: inline in this chat. User explicitly asked to start writing.

Goal: a runnable Windows Rust browser on Servo with Brave-like chrome and optional
Happ-compatible subscription routing. Spec: BROWSER-DESIGN.md.

- [x] Browser core: profile load/save, bookmarks, bounded history and ordered sessions;
  parse supported subscription formats and generate sing-box configuration. Write
  focused failing behavior tests, implement, run cargo test for this standalone crate.
- [x] Desktop integration: tabs above navigation, dark theme, bookmark/menu panels,
  local new-tab, restore session; connection import UI and child process lifecycle.
- [x] Windows build/package: local build dependencies, locked Servo build, local
  sing-box config validation, launch and capture actual UI; document proof and gaps.

Review focus: corrupt profile preservation; Unicode titles; duplicate bookmarks;
unsupported subscription transports; secret redaction; connection failure must not
claim remote success or silently switch to direct while routing is enabled.

Follow-ups implemented: borderless Windows tab strip with caption buttons and a
permanent drag area; one browser menu; appearance settings; XHTTP via Xray;
separate native password-manager window with CSV import/export and search;
unslop design pass. Proof and runtime gaps: BUILD-REPORT.md.

7 November follow-up: fixed future-timer repaint busy loop (red/green regression),
cached chrome style, released closed-tab favicon textures, reduced unconditional
polling, kept the new-tab button visible, fixed caption focus feedback, added
optional glass/animations and read-only Brave bookmark/history import. Latest
desktop build is packaged and running. Brave history remains locked while Brave
is open; GitHub source upload awaits CLI authentication. Media compatibility and
bounded memory across arbitrary workloads remain open.

YouTube diagnosis: reproduced the full engine's SendRequest failure and traced it
to HTTP/2 REQUEST_HEADER_FIELDS_TOO_LARGE. The 48 KiB local TLS regression fails
before the fix and passes with a bounded 256 KiB decoded-header budget. All four
offline networking tests and the opt-in live direct/auto/proxy YouTube check pass.
The browser-owned subscription proxy also returned HTTP 200 independently.
Owner deferred Brave history import; saved routing is now auto.
