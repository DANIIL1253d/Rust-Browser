"""Long, controlled Windows headless comparison on the same local fixture.

python tests/browser-performance/compare.py --app <candidate.exe> --baseline <old.exe>
    --reference <brave.exe> --work <scratch> --seconds 300

Only owned synthetic profiles/processes are used. CPU includes the process tree;
working-set sums can count shared pages twice. This is not a native UI or real
website benchmark. Run after compilers stop, on the same machine/power settings.
"""
import argparse
import ctypes
import hashlib
import http.server
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "browser-session"))
from win_process import sample, OwnedJob


HTML = """<!doctype html><meta charset=utf-8><title>Controlled browser workload</title>
<style>body{font:16px sans-serif;background:#fafafa}main{display:grid;grid-template-columns:repeat(10,1fr);gap:4px}span{padding:4px;border:1px solid #ddd}</style><main></main>
<script>
const run=new URL(location.href).searchParams.get('run');const root=document.querySelector('main');
for(let i=0;i<500;i++){const node=document.createElement('span');node.textContent='Fixture '+i;root.appendChild(node)}
const nodes=[...root.children];let frames=0,writes=0,maxFrame=0,last=performance.now(),ready=last,stopped=false;
function frame(now){frames++;maxFrame=Math.max(maxFrame,now-last);last=now;
 if(now-ready<30000){for(let i=0;i<100;i++){const n=nodes[(writes+i)%nodes.length];n.textContent='Frame '+frames+' item '+i;n.style.transform='translateY('+frames%3+'px)'}writes+=100;requestAnimationFrame(frame)}else stopped=true}
requestAnimationFrame(frame);
setInterval(()=>fetch('/report?run='+encodeURIComponent(run),{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({frames,writes,maxFrame,stopped,elapsed:(performance.now()-ready)/1000})}).catch(()=>{}),1000);
</script>"""


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=300)
    args = parser.parse_args()
    if args.seconds < 60:
        parser.error("Use at least60seconds (default300)")
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    reports = {}
    lock = threading.Lock()

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = HTML.encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            run = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query).get("run", [""])[0]
            size = int(self.headers.get("Content-Length", "0"))
            if size > 4096:
                self.send_error(413)
                return
            record = json.loads(self.rfile.read(size))
            with lock:
                reports[run] = record
            self.send_response(204)
            self.end_headers()

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    cases = []
    if args.baseline:
        cases.append(("baseline", args.baseline.resolve(), False))
    cases.append(("candidate", args.app.resolve(), False))
    if args.reference:
        cases.append(("reference", args.reference.resolve(), True))
    results = []
    ctypes.windll.kernel32.SetErrorMode(0x8003)
    try:
        for name, binary, chromium in cases:
            with tempfile.TemporaryDirectory(prefix=name + "-", dir=work) as directory:
                profile = Path(directory)
                url = f"http://127.0.0.1:{server.server_port}/?run={name}"
                env = os.environ.copy()
                env["RUST_BROWSER_PROFILE"] = str(profile)
                env["GST_PLUGIN_SYSTEM_PATH_1_0"] = ""
                env["GST_REGISTRY"] = str(profile / "gst-registry.bin")
                if chromium:
                    command = [str(binary), "--headless=new", "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-background-networking", "--disable-component-update", "--user-data-dir=" + str(profile / "reference"), "--remote-debugging-port=0", url]
                else:
                    fixture = profile / "session.json"
                    fixture.write_text(json.dumps({"session": [url], "active_tab": 0}), encoding="utf-8")
                    command = [str(binary), "--headless", "--session-fixture", str(fixture)]
                with (work / (name + ".log")).open("wb") as log:
                    started = time.monotonic()
                    job = OwnedJob()
                    proc = subprocess.Popen(command, cwd=binary.parent, env=env, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
                    try:
                        job.assign(proc)
                        deadline = time.monotonic() + 45
                        while time.monotonic() < deadline:
                            with lock:
                                ready = reports.get(name)
                            if ready:
                                break
                            if proc.poll() is not None:
                                raise AssertionError(f"{name} exited before fixture ready: {proc.returncode}")
                            time.sleep(0.1)
                        else:
                            raise AssertionError(f"{name} fixture did not report readiness")
                        startup = time.monotonic() - started
                        measures = []
                        for _ in range(args.seconds):
                            measures.append({"elapsed": round(time.monotonic() - started, 3), **sample(proc.pid)})
                            time.sleep(1)
                        measures.append({"elapsed": round(time.monotonic() - started, 3), **sample(proc.pid)})
                        elapsed = measures[-1]["elapsed"] - measures[0]["elapsed"]
                        idle = [m for m in measures if m["elapsed"] >= 40]
                        idle_cpu = max(0, idle[-1]["cpu_seconds"] - idle[0]["cpu_seconds"]) / (idle[-1]["elapsed"] - idle[0]["elapsed"]) / os.cpu_count() * 100
                        with lock:
                            final = dict(reports[name])
                        assert final["elapsed"] >= args.seconds - 5 and final["frames"] > 1 and final["stopped"], final
                        result = {"case": name, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "seconds": elapsed, "startup_to_first_report_seconds": round(startup, 3), "logical_processors": os.cpu_count(), "total_cpu_percent_all_cores": round((measures[-1]["cpu_seconds"] - measures[0]["cpu_seconds"]) / elapsed / os.cpu_count() * 100, 4), "idle_cpu_percent_all_cores": round(idle_cpu, 4), "rss_mib_median": round(statistics.median(m["rss_mib"] for m in measures), 2), "rss_mib_peak": round(max(m["rss_mib"] for m in measures), 2), "private_mib_final": round(measures[-1]["private_mib"], 2), "fixture": final}
                        results.append(result)
                        (work / (name + "-samples.json")).write_text(json.dumps(measures, indent=2), encoding="utf-8")
                        print(json.dumps(result), flush=True)
                    finally:
                        job.close()
                        if proc.poll() is None:
                            proc.terminate()  # Also handles a failed job assignment.
                        proc.wait(timeout=15)
    finally:
        server.shutdown()
        (work / "results.json").write_text(json.dumps({"kind": "controlled-headless-loopback", "results": results}, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
