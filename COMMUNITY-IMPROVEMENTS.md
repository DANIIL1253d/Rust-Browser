# Community-driven improvements — 7 November 2026

Scope: preserve the Rust/Servo engine and existing interface. Implement useful,
bounded changes; no claim of market leadership or Chromium compatibility.

## Sources and decisions

| User feedback | Source | Decision |
| --- | --- | --- |
| Hidden tabs become difficult to find when the tab-search control disappears or moves | [Brave Community, July–September 2025](https://community.brave.app/t/down-arrow-at-the-end-of-horizontal-tab-list-is-gone/631155) | Add a searchable list in the existing menu and Ctrl+Shift+A; no extra permanent toolbar button |
| Accidental closure and crashes make previously open tabs difficult to recover | [Mozilla Connect, October 2023](https://connect.mozilla.org/t5/ideas/restore-recently-closed-tabs/idi-p/37024) | Persist a bounded list of 20 closed web tabs; restore with menu or Ctrl+Shift+T |
| New-tab button disappears when the tab list overflows | [Zen issue #2835](https://github.com/zen-browser/desktop/issues/2835) | Already fixed: keep + outside the horizontal scroll region |

These dated individual reports identify concrete problems, not a representative
market survey. Future candidates: tab unloading, downloads, page search and
stronger crash/session recovery. Media compatibility remains a separate engine task.

## Implementation plan

- [x] Add regression for retaining more than 500 history entries, then raise the
  bounded history capacity to 10,000 so this owner's complete web history fits.
- [x] Add tested recent-tab storage and Unicode title/URL matching to browser-core.
- [x] Integrate restore/search into the shell, shortcuts and existing menu.
- [x] Add a profile-only --import-brave CLI path: read Bookmarks and History,
  preserve existing data, save a candidate atomically, never load the vault.
- [x] Compile, run core/UI checks, package, import while the browser is closed,
  and report counts without printing personal addresses or titles.

The owner created the private repository and granted the GitHub plugin access.
Logo and preview links were repaired. The source overlay and pinned Servo
bootstrap are ready; current delivery evidence is in PRIVATE-REPOSITORY.md.

Cookies, authenticated sessions and Chromium extensions are outside the current
importer's capabilities; passwords use its explicit native CSV workflow. Do not
copy auth files or decrypted material into source, logs, deliverables or the wiki.
