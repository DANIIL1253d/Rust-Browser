# Experimental native MSE contract

The browser keeps `dom_media_source_enabled=false`. The general experimental-web-platform flag does not enable it. Use an explicit preference only in isolated tests.

The runner creates a new synthetic profile, serves generated media over loopback, uses headless WebDriver, and writes `results.json` and `engine.log` below the chosen work directory. It does not read an existing profile.

```powershell
node tests/browser-mse/engine.mjs PATH_TO_EXE PATH_TO_WORK
node tests/browser-mse/engine.mjs PATH_TO_EXE PATH_TO_WORK --enabled --fixtures=PATH_TO_GENERATED_FIXTURES
```

The default command requires `MediaSource` to remain unavailable. The enabled contract requires actual native behavior and collects failures for every case. Use `--case=closed-state`, `csp`, `append-av`, `playback-seek`, `detach`, `reattach-events`, `timestamp-offset`, `malformed-data`, `remove`, or `abort` to investigate one case. Fixture filenames are `video.webm` (VP8) and `audio.webm` (Opus).

`append-av` intentionally checks buffered ranges immediately after `updateend`. An empty range at this point is a conformance failure, even if ranges become available after playback starts. `playback-seek` independently checks decoded progress and a real seek; the offset/removal cases start playback before inspecting the later native store.

GStreamer 1.28.8 does not implement coded-frame removal and its abort path does not perform the required parser cancellation/events. Its SEGMENTS mode also accepts nonzero timestamp offsets without shifting the decoded range; the adapter rejects these offsets. Unsupported operations return `NotSupportedError`. A full enabled contract is expected to remain red until native operations work; do not replace these tests with assertions that accept successful no-ops. Negative offsets, SEQUENCE offset playback, `changeType`, and per-SourceBuffer track lists are also incomplete. Same-global object URLs are implemented; cross-global object URL resolution and worker construction are not.

DOM task callbacks use operation IDs and attachment generations. Tests cover detaching during a pending append and reattaching without old buffers. Navigation cleanup clears native callbacks before cancelling the document task sources; the panic-unwind script failsafe remains an upstream special teardown path.
