"""Exercise real main-document security events using isolated headless profiles.

Run after packaging: python tests/browser-security/smoke.py --app app/RustBrowser.exe
Uses only loopback fixtures, upstream public TEST certificate/key and optional example.org.
No normal browser profile, cookies, passwords or subscription files are read.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import tempfile
import threading
import time

parser = argparse.ArgumentParser()
parser.add_argument("--app", type=Path, required=True)
parser.add_argument("--source", type=Path)
parser.add_argument("--work", type=Path)
parser.add_argument("--live", action="store_true")
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
source = args.source or (root if (root / "components/net/tests").is_dir() else root / ".build/servo-source")
work = args.work or root / ".build/security-smoke"
work.mkdir(parents=True, exist_ok=True)
app = args.app.resolve()


class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/slow":
            time.sleep(5)  # Cross the browser's normal profile-poll interval.
        body = b"<!doctype html><title>Security fixture</title><p>Transport check</p>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", "100000" if self.path == "/truncated" else str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True

    def log_message(self, *_):
        pass


plain = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
secure = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(source / "components/net/tests/self_signed_certificate_for_testing.crt",
                        source / "components/net/tests/privatekey_for_testing.key")
secure.socket = context.wrap_socket(secure.socket, server_side=True)
for server in (plain, secure):
    threading.Thread(target=server.serve_forever, daemon=True).start()

cases = [
    ("http-slow", f"http://127.0.0.1:{plain.server_port}/slow", [], "Insecure"),
    ("tls-rejected", f"https://127.0.0.1:{secure.server_port}/", [], "Failed"),
    ("tls-bypassed", f"https://127.0.0.1:{secure.server_port}/", ["--ignore-certificate-errors"], "Unverified"),
    ("tls-short-eof-error", f"https://127.0.0.1:{secure.server_port}/truncated", ["--ignore-certificate-errors"], "Failed"),
]
if args.live:
    cases.append(("https-verified", "https://example.org/", [], "Secure"))

results = []
try:
    for name, url, extra, expected in cases:
        with tempfile.TemporaryDirectory(prefix=name + "-", dir=work) as profile:
            env = os.environ.copy()
            env["RUST_BROWSER_PROFILE"] = profile
            env["RUST_LOG"] = "warn,servo::webview=debug"
            env["SERVO_TRACING"] = "warn,servo::webview=debug"
            run = subprocess.run([str(app), "--headless", "--temporary-storage", "--config-dir", str((Path(profile) / "engine").resolve()),
                                  "--exit", "--output", str((work / (name + ".png")).resolve()), *extra, url],
                                 cwd=app.parent, env=env, capture_output=True, timeout=50)
            log = (run.stdout + run.stderr).decode("utf-8", errors="replace")
            (work / (name + ".log")).write_text(log, encoding="utf-8")
            states = re.findall(r"Completed document connection security: Some\((\w+)\)", log)
            touched = [p.name for p in Path(profile).iterdir() if p.name in ("profile.json", "passwords.dat", "connections.dat")]
            result = {"case": name, "exit": run.returncode, "expected": expected, "completed": states, "profile_files_created": touched}
            results.append(result)
            print(json.dumps(result), flush=True)
            assert run.returncode == 0 and states and states[-1] == expected and not touched, result
finally:
    plain.shutdown()
    secure.shutdown()
    (work / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
