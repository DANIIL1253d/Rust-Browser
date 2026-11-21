"""Exercise real media decoding in an isolated browser, never the user's profile.

python tests/browser-media/smoke.py --app <RustBrowser.exe> --work <scratch>
Optional --video <fixture.webm|mp4> also checks decoded video frames and seeking.
The WAV fixture is generated locally. Results and logs stay under --work.
"""
import argparse
import ctypes
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import wave


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--video", type=Path)
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    app = args.app.resolve()
    ctypes.windll.kernel32.SetErrorMode(0x8003)
    with wave.open(str(work / "silence.wav"), "wb") as wav:
        wav.setparams((1, 2, 22050, 0, "NONE", "not compressed"))
        wav.writeframes(bytes(22050 * 2 * 4))
    media = {"/silence.wav": ((work / "silence.wav").read_bytes(), "audio/wav")}
    if args.video:
        media["/video"] = (args.video.read_bytes(), "video/mp4" if args.video.suffix == ".mp4" else "video/webm")

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/":
                body = b'<!doctype html><title>Media fixture</title><audio id="audio" src="/silence.wav" preload="auto"></audio><video id="video" src="/video" muted preload="auto" width="320" height="240"></video>'
                mime = "text/html; charset=utf-8"
            elif self.path in media:
                body, mime = media[self.path]
            else:
                self.send_error(404)
                return
            start, end, status = 0, len(body) - 1, 200
            requested = self.headers.get("Range", "")
            if requested.startswith("bytes="):
                first, last = requested[6:].split("-", 1)
                start = int(first or 0)
                end = min(int(last) if last else end, end)
                if start > end:
                    self.send_response(416)
                    self.end_headers()
                    return
                status = 206
            self.send_response(status)
            self.send_header("Content-Type", mime)
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            if status == 206:
                self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
            self.end_headers()
            self.wfile.write(body[start:end + 1])

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(path, body=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(base + path, data=data, headers={"Content-Type": "application/json"})
        with opener.open(req, timeout=15) as response:
            value = json.load(response)["value"]
        if isinstance(value, dict) and isinstance(value.get("error"), str):
            raise AssertionError(value)
        return value

    def wait_until(test, timeout=20):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            try:
                last = test()
                if last:
                    return last
            except OSError:
                pass
            time.sleep(0.1)
        raise AssertionError(f"Media condition timed out: {last!r}")

    results = []
    fixture = work / "session.json"
    fixture.write_text(json.dumps({"session": [f"http://127.0.0.1:{server.server_port}/"], "active_tab": 0}), encoding="utf-8")
    try:
        with tempfile.TemporaryDirectory(prefix="profile-", dir=work) as profile, (work / "browser.log").open("wb") as log:
            env = os.environ.copy()
            env["RUST_BROWSER_PROFILE"] = profile
            env["RUST_LOG"] = "warn"
            # Do not discover or cache machine-wide GStreamer plugins during this test.
            env["GST_REGISTRY"] = str(Path(profile) / "gst-registry.bin")
            env["GST_PLUGIN_SYSTEM_PATH_1_0"] = ""
            env["GST_PLUGIN_PATH_1_0"] = ""
            env["GST_PLUGIN_PATH"] = ""
            # A missing bundled DLL must not be satisfied by the developer SDK.
            system = Path(os.environ["SystemRoot"])
            env["PATH"] = os.pathsep.join([str(system / "System32"), str(system)])
            proc = subprocess.Popen([str(app), "--headless", "--session-fixture", str(fixture), "--webdriver", str(port)], cwd=app.parent, env=env, stdout=log, stderr=log)
            try:
                wait_until(lambda: request("/status"))
                sid = request("/session", {"capabilities": {}})["sessionId"]
                path = "/session/" + sid

                def js(script):
                    return request(path + "/execute/sync", {"script": script, "args": []})

                wait_until(lambda: js("return !!document.getElementById('audio')"))
                for name in (["audio", "video"] if args.video else ["audio"]):
                    caps = js("return {wav:document.createElement('audio').canPlayType('audio/wav'),mp4:document.createElement('video').canPlayType('video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\"'),webm:document.createElement('video').canPlayType('video/webm; codecs=\"vp9, opus\"'),mse:typeof MediaSource==='function'}")
                    js(f"window.m=document.getElementById('{name}');m.muted=true;window.playError=null;m.play().catch(e=>window.playError=e.name);return true")

                    def state():
                        return js("return {time:m.currentTime,ready:m.readyState,paused:m.paused,error:m.error&&m.error.code,playError:window.playError,width:m.videoWidth||0,height:m.videoHeight||0}")

                    deadline = time.monotonic() + 20
                    current = state()
                    while time.monotonic() < deadline and not current["error"] and not current["playError"] and current["time"] < 0.2:
                        time.sleep(0.1)
                        current = state()
                    record = {"case": name, "capabilities": caps, "playback": current}
                    results.append(record)
                    print(json.dumps(record), flush=True)
                    assert not current["error"] and not current["playError"] and current["ready"] >= 2 and current["time"] >= 0.2, record
                    if name == "video":
                        assert current["width"] > 0 and current["height"] > 0, record
                        pixel = js("const c=document.createElement('canvas');c.width=2;c.height=2;const x=c.getContext('2d');x.drawImage(m,0,0,2,2);return Array.from(x.getImageData(0,0,2,2).data)")
                        assert any(pixel[i] for i in range(len(pixel)) if i % 4 != 3), pixel
                        record["decoded_pixels"] = pixel
                    js("m.pause();m.currentTime=1;return true")
                    wait_until(lambda: abs(state()["time"] - 1) < 0.15)
                    before = state()["time"]
                    time.sleep(0.5)
                    assert abs(state()["time"] - before) < 0.15
                    record["seek_and_pause"] = True
                assert not any((Path(profile) / p).exists() for p in ("profile.json", "passwords.dat", "connections.dat"))
            finally:
                if proc.poll() is None:
                    proc.terminate()
                proc.wait(timeout=10)
    finally:
        server.shutdown()
        (work / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
