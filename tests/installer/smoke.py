"""Install, render one loopback page headlessly, and uninstall in owned scratch.

Requires Windows and an EXE produced by Build-Installer.ps1. The installer has
its own /TESTINSTALL=1 AppId and never creates shortcuts or launches the UI.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
import winreg
import zipfile


parser = argparse.ArgumentParser()
parser.add_argument("--installer", type=Path, required=True)
parser.add_argument("--archive", type=Path, required=True)
parser.add_argument("--work", type=Path, required=True)
args = parser.parse_args()
installer = args.installer.resolve(strict=True)
archive = args.archive.resolve(strict=True)
work = args.work.resolve()
work.mkdir(parents=True, exist_ok=True)
run = Path(tempfile.mkdtemp(prefix="installer-smoke-", dir=work)).resolve()
installed = run / "installed"
assert installed.is_relative_to(work) and installed != work
profile = run / "profile"
profile.mkdir()
marker = profile / "preserve-on-uninstall.txt"
marker.write_text("isolated test profile", encoding="utf-8")
key_base = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\"
smoke_key = key_base + "RustBrowser.InstallerSmoke_is1"
normal_key = key_base + "RustBrowser.6BC79C9E-00E1-491B-895B-CC89ED1F6B00_is1"


def registry_values(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
            return sorted(winreg.EnumValue(key, i) for i in range(winreg.QueryInfoKey(key)[1]))
    except FileNotFoundError:
        return None


def shell_path(name, fallback):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders") as key:
            return Path(os.path.expandvars(winreg.QueryValueEx(key, name)[0]))
    except FileNotFoundError:
        return fallback


def shortcut_state():
    desktop = shell_path("Desktop", Path(os.environ["USERPROFILE"]) / "Desktop")
    programs = shell_path("Programs", Path(os.environ["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs")
    paths = [desktop / "Rust Browser.lnk", programs / "Rust Browser/Rust Browser.lnk"]
    return [(str(p), (p.stat().st_size, p.stat().st_mtime_ns) if p.exists() else None) for p in paths]


startup = subprocess.STARTUPINFO()
startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
startup.wShowWindow = subprocess.SW_HIDE


def execute(command, **kwargs):
    return subprocess.run(command, startupinfo=startup, creationflags=subprocess.CREATE_NO_WINDOW,
                          capture_output=True, timeout=120, **kwargs)


assert registry_values(smoke_key) is None, "An existing smoke installation needs review before another test."
normal_before = registry_values(normal_key)
shortcuts_before = shortcut_state()
result = {"run": str(run), "installed_files": 0, "headless_exit": None, "uninstalled": False}
uninstaller = None
expected = {}
try:
    setup = execute([str(installer), "/TESTINSTALL=1", "/VERYSILENT", "/SUPPRESSMSGBOXES",
                     "/NORESTART", "/NOICONS", f"/DIR={installed}", f"/LOG={run / 'install.log'}"])
    assert setup.returncode == 0, f"Installer exit {setup.returncode}"
    uninstallers = list(installed.glob("unins*.exe"))
    assert len(uninstallers) == 1, "Standard uninstaller missing"
    uninstaller = uninstallers[0]
    assert registry_values(smoke_key) is not None, "Separate smoke uninstall registration missing"
    assert registry_values(normal_key) == normal_before, "Normal uninstall registration changed"
    assert shortcut_state() == shortcuts_before, "Smoke installation changed normal shortcuts"

    with zipfile.ZipFile(archive) as package:
        names = package.namelist()
        manifest_name = next(name for name in names if name.endswith("/release-manifest.json"))
        prefix = manifest_name.removesuffix("release-manifest.json")
        manifest = json.loads(package.read(manifest_name))
        expected = dict(manifest["sha256"])
        expected["release-manifest.json"] = hashlib.sha256(package.read(manifest_name)).hexdigest()
        for relative, digest in expected.items():
            target = installed / relative
            assert target.is_relative_to(installed) and target.is_file(), relative
            assert hashlib.sha256(target.read_bytes()).hexdigest() == digest.lower(), relative
        actual = {p.relative_to(installed).as_posix() for p in installed.rglob("*") if p.is_file()
                  and not re.fullmatch(r"unins\d+\.(exe|dat|msg)", p.name, re.IGNORECASE)}
        assert actual == set(expected), f"Unexpected payload files: {actual ^ set(expected)}"
        result["installed_files"] = len(expected)

    class Fixture(http.server.BaseHTTPRequestHandler):
        requests = 0

        def do_GET(self):
            Fixture.requests += 1
            body = b"<!doctype html><title>Installer smoke</title><body style='background:#134047;color:white'><h1>Installed Rust Browser</h1><p>Loopback fixture rendered.</p>"
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    fixture = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=fixture.serve_forever, daemon=True).start()
    try:
        app = installed / "app/RustBrowser.exe"
        image = run / "installed-browser.png"
        env = os.environ.copy()
        env["RUST_BROWSER_PROFILE"] = str(profile)
        browser = execute([str(app), "--headless", "--temporary-storage", "--config-dir", str(run / "engine"),
                           "--exit", "--output", str(image), f"http://127.0.0.1:{fixture.server_port}/"],
                          cwd=app.parent, env=env)
        (run / "browser.log").write_bytes(browser.stdout + browser.stderr)
        result["headless_exit"] = browser.returncode
        assert browser.returncode == 0 and Fixture.requests > 0, result
        assert image.is_file() and image.stat().st_size > 1000 and image.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), "Headless rendering failed"
    finally:
        fixture.shutdown()
        fixture.server_close()
finally:
    if uninstaller is not None and uninstaller.is_file():
        uninstall = execute([str(uninstaller), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", f"/LOG={run / 'uninstall.log'}"])
        assert uninstall.returncode == 0, f"Uninstaller exit {uninstall.returncode}"
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and (uninstaller.exists() or registry_values(smoke_key) is not None):
            time.sleep(0.1)
        assert not any((installed / relative).exists() for relative in expected), "Uninstaller left a payload file"
        assert registry_values(smoke_key) is None, "Uninstaller left its registration"
        assert marker.read_text(encoding="utf-8") == "isolated test profile", "Uninstaller changed the separate profile"
        assert registry_values(normal_key) == normal_before, "Normal uninstall registration changed"
        assert shortcut_state() == shortcuts_before, "Normal shortcuts changed"
        result["uninstalled"] = True
    (run / "results.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result), flush=True)
