"""Verify graceful-restart persistence using only synthetic loopback storage.

python tests/browser-storage/smoke.py --app <RustBrowser.exe> --work <scratch>
No Brave or ordinary Rust Browser profile is opened. HttpOnly cookies are checked
by the fixture server, never by credential extraction or copying profile stores.
"""
import argparse
import ctypes
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    app, work = args.app.resolve(), args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    ctypes.windll.kernel32.SetErrorMode(0x8003)
    observed = {}

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            phase = "write" if self.path.startswith("/write") else "read"
            if phase == "read":
                observed["persistent_httponly_cookie"] = "fixture_persist=owned-test-value" in self.headers.get("Cookie", "")
            script = "localStorage.setItem('fixture-storage','owned-test-value');" if phase == "write" else ""
            body = ("<!doctype html><title>Storage fixture</title><body><script>" + script +
                    "fetch('/report', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({phase:'" + phase +
                    "',localStorage:localStorage.getItem('fixture-storage')==='owned-test-value',indexedDB:typeof indexedDB==='object'})})" +
                    ".then(()=>document.body.textContent='Complete');</script>").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            if phase == "write":
                self.send_header("Set-Cookie", "fixture_persist=owned-test-value; Max-Age=86400; Path=/; HttpOnly; SameSite=Lax")
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if size > 4096:
                self.send_error(413)
                return
            record = json.loads(self.rfile.read(size))
            observed[record["phase"]] = record
            self.send_response(204)
            self.end_headers()

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="storage-owned-", dir=work) as directory:
            profile = Path(directory)
            env = os.environ.copy()
            env["RUST_BROWSER_PROFILE"] = str(profile)
            env["GST_REGISTRY"] = str(profile / "gst-registry.bin")
            env["GST_PLUGIN_SYSTEM_PATH_1_0"] = ""
            env["GST_PLUGIN_PATH_1_0"] = ""
            env["GST_PLUGIN_PATH"] = ""
            system = Path(os.environ["SystemRoot"])
            env["PATH"] = os.pathsep.join([str(system / "System32"), str(system)])
            for phase in ["write", "read"]:
                command = [str(app), "--headless", "--config-dir", str(profile / "engine"), "--exit", f"http://127.0.0.1:{server.server_port}/{phase}"]
                run = subprocess.run(command, cwd=app.parent, env=env, capture_output=True, timeout=50)
                (work / (phase + ".log")).write_bytes(run.stdout + run.stderr)
                assert run.returncode == 0, {"phase": phase, "exit": run.returncode}
                assert phase in observed, {"phase": phase, "reported": observed}
            assert observed["read"]["localStorage"], observed
            assert observed["persistent_httponly_cookie"], observed
            assert not any((profile / p).exists() for p in ["profile.json", "passwords.dat", "connections.dat"])
            print(json.dumps(observed), flush=True)
    finally:
        server.shutdown()
        (work / "results.json").write_text(json.dumps(observed, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
