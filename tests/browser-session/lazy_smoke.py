"""Real isolated Servo/WebDriver session tests and synthetic eager/lazy comparison.

python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-check --benchmark
Only loopback fixture pages are used. --session-fixture disables persistent engine
config/storage and never loads the browser's normal profile or vault.
"""
import argparse
from collections import Counter
import http.server
import json
import os
from pathlib import Path
import socket
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from win_process import sample

parser = argparse.ArgumentParser()
parser.add_argument("--app", type=Path, required=True)
parser.add_argument("--work", type=Path, required=True)
parser.add_argument("--benchmark", action="store_true")
args = parser.parse_args()
args.work.mkdir(parents=True, exist_ok=True)
app = args.app.resolve()
hits = Counter()
hits_lock = threading.Lock()


class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if not self.path.startswith("/page/"):
            self.send_error(404)
            return
        index = int(self.path.rsplit("/", 1)[1])
        with hits_lock:
            hits[index] += 1
        # An explicit synthetic retained allocation, not a representative website.
        body = (f"<!doctype html><title>Fixture {index}</title><h1>Page {index}</h1>"
                f"<script>window.__payload = new Uint8Array(24*1024*1024);"
                f"window.__payload.fill({index + 1}); window.__ready = true;</script>").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
origin = f"http://127.0.0.1:{server.server_port}"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_until(test, timeout=30):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = test()
            if value:
                return value
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.1)
    raise AssertionError("Fixture condition timed out")


def request(base, path, body=None, method=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    with opener.open(req, timeout=15) as response:
        value = json.load(response)["value"]
    if isinstance(value, dict) and "error" in value:
        raise AssertionError(value)
    return value


def page_counts():
    with hits_lock:
        return dict(hits)


results = []


def run_case(tabs, eager=False, immediate=None, replacement=None, functional=False):
    name = f"{tabs}-{'eager' if eager else 'lazy'}" + ("-immediate" if immediate is not None else "") + ("-replacement" if replacement is not None else "")
    with hits_lock:
        hits.clear()
    port = free_port()
    base = f"http://127.0.0.1:{port}"
    selected = tabs // 2
    fixture = args.work / (name + ".json")
    fixture.write_text(json.dumps({"session": [f"{origin}/page/{i}" for i in range(tabs)],
                                   "active_tab": selected, "eager": eager,
                                   "activate_immediately": immediate,
                                   "navigate_immediately": f"{origin}/page/{replacement}" if replacement is not None else None}), encoding="utf-8")
    log_path = args.work / (name + ".log")
    with tempfile.TemporaryDirectory(prefix="profile-", dir=args.work) as profile, log_path.open("wb") as log:
        env = os.environ.copy()
        env["RUST_BROWSER_PROFILE"] = profile
        env["RUST_LOG"] = "warn,servoshell::desktop::app=info,servoshell::window=debug,servoshell::running_app_state=debug"
        env["SERVO_TRACING"] = env["RUST_LOG"]
        started = time.monotonic()
        proc = subprocess.Popen([str(app), "--headless", "--session-fixture", str(fixture.resolve()),
                                 "--webdriver", str(port)], cwd=app.parent, env=env,
                                stdout=log, stderr=log)
        sid = None
        try:
            wait_until(lambda: request(base, "/status"))
            sid = request(base, "/session", {"capabilities": {}})["sessionId"]
            path = "/session/" + sid
            handles = request(base, path + "/window/handles")
            assert len(handles) == tabs, handles
            active = replacement if replacement is not None else (immediate if immediate is not None else selected)
            expected_initial = {selected, active}
            wait_until(lambda: request(base, path + "/execute/sync", {"script": "return window.__ready === true", "args": []}))
            wait_until(lambda: len(page_counts()) == (tabs if eager else len(expected_initial)))
            initial = page_counts()
            assert eager or set(initial) == expected_initial, initial
            wait_until(lambda: request(base, path + "/url") == f"{origin}/page/{active}")
            wait_until(lambda: request(base, path + "/execute/sync", {"script": "return window.__ready === true", "args": []}))
            current_url = request(base, path + "/url")
            assert current_url == f"{origin}/page/{active}", current_url
            active_handle = request(base, path + "/window")
            startup_ready = time.monotonic() - started
            if eager:
                for handle in handles:
                    request(base, path + "/window", {"handle": handle})
                    wait_until(lambda: request(base, path + "/execute/sync", {"script": "return window.__ready === true", "args": []}))
                request(base, path + "/window", {"handle": active_handle})
            if functional:
                # W3C window handles have arbitrary order. An index in this list
                # is not a tab/session index; identify pages by their actual URL.
                activated = False
                for handle in handles:
                    if handle == active_handle:
                        continue
                    request(base, path + "/window", {"handle": handle})
                    wait_until(lambda: request(base, path + "/execute/sync", {"script": "return window.__ready === true", "args": []}))
                    target_url = request(base, path + "/url")
                    target = int(target_url.rsplit("/", 1)[1])
                    assert target_url == f"{origin}/page/{target}", target_url
                    if target not in initial:
                        assert set(page_counts()) == set(initial) | {target}, page_counts()
                        activated = True
                        break
                assert activated, "No deferred page was activated"
                request(base, path + "/window", {"handle": active_handle})
                assert request(base, path + "/url") == current_url
                time.sleep(0.3)
                assert all(count == 1 for count in page_counts().values()), page_counts()
            time.sleep(1.5)
            before_time = time.monotonic()
            before = sample(proc.pid)
            measurements = []
            for _ in range(15):
                time.sleep(0.2)
                measurements.append(sample(proc.pid))
            elapsed = time.monotonic() - before_time
            cpu = max(0, measurements[-1]["cpu_seconds"] - before["cpu_seconds"]) / elapsed / os.cpu_count() * 100
            record = {"case": name, "tabs": tabs, "eager": eager, "initial_loaded_pages": len(initial),
                      "page_requests": page_counts(), "startup_to_ready_seconds": round(startup_ready, 3),
                      "rss_mib_median": round(statistics.median(m["rss_mib"] for m in measurements), 2),
                      "private_mib_median": round(statistics.median(m["private_mib"] for m in measurements), 2),
                      "idle_cpu_percent_all_cores": round(cpu, 4), "logical_processors": os.cpu_count(),
                      "processes": measurements[-1]["processes"], "functional_activation_checked": functional}
            results.append(record)
            print(json.dumps(record), flush=True)
        finally:
            if proc.poll() is None:
                proc.terminate()  # Only this owned, isolated headless test process.
            proc.wait(timeout=10)
        text = log_path.read_text(encoding="utf-8", errors="replace")
        assert "Session fixture early snapshot valid: true" in text, text[-2000:]
        assert "Session fixture storage: persistent config=false, temporary=true" in text, text[-2000:]
        assert not any((Path(profile) / p).exists() for p in ("profile.json", "passwords.dat", "connections.dat"))


try:
    run_case(3, functional=True)
    run_case(3, immediate=0, functional=True)
    run_case(3, replacement=100, functional=True)
    if args.benchmark:
        for count in (1, 5, 10):
            run_case(count, eager=True)
            run_case(count)
finally:
    server.shutdown()
    (args.work / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
