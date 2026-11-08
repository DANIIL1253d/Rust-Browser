# Testing Rust Browser

Use the same Windows MSVC environment as the desktop build. Pass an installed
Python 3.11+ executable to the build script:

```powershell
.\Build-Browser.ps1 -CoreTests -PythonPath C:\Path\To\python.exe
.\Build-Browser.ps1 -UiTests -PythonPath C:\Path\To\python.exe
.\Build-Browser.ps1 -NetworkTests -PythonPath C:\Path\To\python.exe
```

Core tests cover profiles, subscriptions, CSV password behavior, DPAPI and session
planning. UI regressions cover repaint scheduling, shortcuts and connection
evidence. Network tests use local fixtures; the live YouTube case is separately
marked ignored and is not evidence of video playback.

After a real desktop build and packaging, exercise the actual engine:

```powershell
python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-check
python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-benchmark --benchmark
python tests/browser-security/smoke.py --app app/RustBrowser.exe --live
```

Session fixtures use loopback pages and disable persistent engine configuration
and storage. The transport runner uses its own engine configuration and temporary
storage. Neither runner loads the normal profile, password vault or subscription.

The benchmark uses synthetic retained JavaScript allocations, fresh processes
and Win32 process-tree metrics. Working set and private commit are distinct.
Its output belongs in a local build directory; it is not committed. Do not
generalize a short headless sample to all websites, sustained CPU usage or a
fixed memory ceiling. Native UI interaction, clean-machine installation and real
media playback need separate checks.
