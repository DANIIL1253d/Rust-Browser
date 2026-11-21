"""Check actual package INFO/DEBUG logs with synthetic loopback credentials only."""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'browser-session'))
from win_process import OwnedJob

MARKERS = ('PRIVATE_PATH_MARKER', 'QUERY_VALUE_MARKER', 'REQUEST_COOKIE_MARKER',
           'AUTH_VALUE_MARKER', 'CUSTOM_REQUEST_MARKER', 'RESPONSE_COOKIE_MARKER',
           'CUSTOM_HEADER_MARKER', 'IMAGE_QUERY_MARKER', 'CORS_REQUEST_METHOD_MARKER',
           'CORS_ALLOWED_METHOD_MARKER', 'x-cors-header-marker', 'CORS_HEADER_VALUE_MARKER',
           'WS_URL_MARKER', 'WS_CLIENT_TEXT_MARKER', 'WS_SERVER_TEXT_MARKER',
           'WS_CLIENT_BINARY_MARKER', 'WS_SERVER_BINARY_MARKER',
           'WS_CLIENT_CLOSE_MARKER', 'WS_SERVER_CLOSE_MARKER', 'WS_PROTOCOL_MARKER',
           'WS_RESPONSE_COOKIE_MARKER', 'WS_HANDSHAKE_HEADER_MARKER')


def log_evidence(log_text, observed):
    leaks = sum(marker in log_text or ('BINARY' in marker and str(list(marker.encode())) in log_text)
                for marker in MARKERS)
    diagnostics = 'net::http_loader]' in log_text
    cors_diagnostics = any('net::http_loader]' in line and 'CORS check:' in line
                           for line in log_text.splitlines())
    websocket_diagnostics = 'net::websocket_loader]' in log_text
    def has_trace(target):
        # Match the logger's level/target envelope, not words in a payload.
        envelope = re.compile(r'^\[[^\]\r\n]*\bTRACE\s+' + re.escape(target) + r'\]')
        return any(envelope.match(line) for line in log_text.splitlines())
    websocket_trace = has_trace('net::websocket_loader')
    fetch_trace = has_trace('net::fetch::methods')
    return dict(observed=observed, leaked_marker_categories=leaks,
                diagnostics_present=diagnostics, cors_diagnostics_present=cors_diagnostics,
                websocket_diagnostics_present=websocket_diagnostics,
                websocket_trace_present=websocket_trace, fetch_trace_present=fetch_trace,
                passed=leaks == 0 and diagnostics and cors_diagnostics and websocket_diagnostics
                    and websocket_trace and fetch_trace
                    and bool(observed) and all(observed.values()))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    app, work = args.app.resolve(), args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    observed = {'authorization': False, 'cookie': False, 'custom_header': False, 'image': False,
                'cors_allowed_preflight': False, 'cors_denied_preflight': False,
                'cors_allowed_request': False, 'cors_denied_blocked': False,
                'ws_server_exchange': False, 'ws_client_exchange': False,
                'ws_client_close': False, 'ws_browser_complete': False,
                'ws_cookie_persisted': False}
    denied_request_seen = False
    lock = threading.Lock()

    class WebSocketFixture(http.server.BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def read_frame(self):
            head = self.rfile.read(2)
            if len(head) != 2 or not head[0] & 0x80 or not head[1] & 0x80:
                raise AssertionError('Expected complete masked synthetic client frame')
            length = head[1] & 127
            if length >= 126:
                raise AssertionError('Synthetic frames must be short')
            mask = self.rfile.read(4)
            body = self.rfile.read(length)
            if len(mask) != 4 or len(body) != length:
                raise AssertionError('Incomplete synthetic frame')
            return head[0] & 15, bytes(value ^ mask[index % 4] for index, value in enumerate(body))

        def send_frame(self, opcode, body):
            assert len(body) < 126
            self.wfile.write(bytes([0x80 | opcode, len(body)]) + body)
            self.wfile.flush()

        def do_GET(self):
            self.connection.settimeout(5)
            self.close_connection = True
            key = self.headers.get('Sec-WebSocket-Key', '')
            if self.headers.get('Sec-WebSocket-Protocol') != 'WS_PROTOCOL_MARKER':
                self.send_error(400)
                return
            accept = base64.b64encode(hashlib.sha1(
                (key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
            self.send_response(101)
            self.send_header('Upgrade', 'websocket')
            self.send_header('Connection', 'Upgrade')
            self.send_header('Sec-WebSocket-Accept', accept)
            self.send_header('Sec-WebSocket-Protocol', 'WS_PROTOCOL_MARKER')
            self.send_header('Set-Cookie', 'ws=WS_RESPONSE_COOKIE_MARKER; Path=/; HttpOnly')
            self.send_header('X-Private', 'WS_HANDSHAKE_HEADER_MARKER')
            self.end_headers()
            text = self.read_frame()
            binary = self.read_frame()
            server_close = self.path.startswith('/server-close?')
            with lock:
                observed['ws_server_exchange' if server_close else 'ws_client_exchange'] = (
                    text == (1, b'WS_CLIENT_TEXT_MARKER')
                    and binary == (2, b'WS_CLIENT_BINARY_MARKER')
                    and 'REQUEST_COOKIE_MARKER' in self.headers.get('Cookie', '')
                    and 'WS_URL_MARKER' in self.path)
            self.send_frame(1, b'WS_SERVER_TEXT_MARKER')
            self.send_frame(2, b'WS_SERVER_BINARY_MARKER')
            if server_close:
                self.send_frame(8, struct.pack('!H', 1000) + b'WS_SERVER_CLOSE_MARKER')
            else:
                opcode, body = self.read_frame()
                with lock:
                    observed['ws_client_close'] = (
                        opcode == 8 and body == struct.pack('!H', 1000) + b'WS_CLIENT_CLOSE_MARKER')
                self.send_frame(8, body)

        def log_message(self, *_):
            pass

    class CorsFixture(http.server.BaseHTTPRequestHandler):
        def do_OPTIONS(self):
            with lock:
                key = {'/allowed': 'cors_allowed_preflight', '/denied': 'cors_denied_preflight'}.get(self.path)
                if key:
                    observed[key] = (
                        self.headers.get('Access-Control-Request-Method') == 'CORS_REQUEST_METHOD_MARKER'
                        and self.headers.get('Access-Control-Request-Headers') == 'x-cors-header-marker')
            self.send_response(204)
            self.send_header('Access-Control-Allow-Origin', '*')
            methods = 'CORS_ALLOWED_METHOD_MARKER'
            if self.path == '/allowed':
                methods += ', CORS_REQUEST_METHOD_MARKER'
            self.send_header('Access-Control-Allow-Methods', methods)
            self.send_header('Access-Control-Allow-Headers', 'x-cors-header-marker')
            self.send_header('Access-Control-Max-Age', '0')
            self.end_headers()

        def do_CORS_REQUEST_METHOD_MARKER(self):
            nonlocal denied_request_seen
            with lock:
                if self.path == '/denied':
                    denied_request_seen = True
                elif self.path == '/allowed':
                    observed['cors_allowed_request'] = (
                        self.headers.get('x-cors-header-marker') == 'CORS_HEADER_VALUE_MARKER')
            self.send_response(200)
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Content-Length', '2')
            self.end_headers()
            self.wfile.write(b'OK')

        def log_message(self, *_):
            pass

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path.startswith('/ws-cookie-check'):
                with lock:
                    observed['ws_cookie_persisted'] = 'WS_RESPONSE_COOKIE_MARKER' in self.headers.get('Cookie', '')
                body, mime = b'OK', 'text/plain'
            elif self.path.startswith('/data'):
                with lock:
                    observed['authorization'] = self.headers.get('Authorization') == 'Bearer AUTH_VALUE_MARKER'
                    observed['cookie'] = 'REQUEST_COOKIE_MARKER' in self.headers.get('Cookie', '')
                    observed['custom_header'] = self.headers.get('X-Private') == 'CUSTOM_REQUEST_MARKER'
                body, mime = b'OK', 'text/plain'
            elif self.path.startswith('/image'):
                with lock:
                    observed['image'] = True
                body = b'<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="red"/></svg>'
                mime = 'image/svg+xml'
            else:
                body = b'''<!doctype html><title>Private diagnostics fixture</title>
                <img src="/image?token=IMAGE_QUERY_MARKER" onload="window.__imageLoaded=true"><script>
                fetch('/data?token=QUERY_VALUE_MARKER', {credentials:'include',headers:{
                    Authorization:'Bearer AUTH_VALUE_MARKER','X-Private':'CUSTOM_REQUEST_MARKER'
                }}).then(r=>r.text()).then(()=>window.__privacyDone=true);
                </script>'''
                body += ('''<script>
                const corsBase = 'http://127.0.0.1:PORT';
                const options = {method:'CORS_REQUEST_METHOD_MARKER',credentials:'omit',
                    headers:{'x-cors-header-marker':'CORS_HEADER_VALUE_MARKER'}};
                Promise.all([
                    fetch(corsBase+'/allowed', options).then(r=>r.text()).then(t=>t==='OK'),
                    fetch(corsBase+'/denied', options).then(()=>false, e=>e instanceof TypeError)
                ]).then(results=>window.__corsDone=results.every(Boolean));
                </script>'''.replace('PORT', str(cors_server.server_port))).encode()
                body += ('''<script>
                function checkWS(clientClose) {
                    return new Promise((resolve, reject)=>{
                        const ws = new WebSocket('ws://127.0.0.1:PORT/' +
                            (clientClose?'client-close':'server-close') + '?token=WS_URL_MARKER',
                            'WS_PROTOCOL_MARKER');
                        ws.binaryType='arraybuffer';
                        let text=false, binary=false;
                        ws.onopen=()=>{
                            ws.send('WS_CLIENT_TEXT_MARKER');
                            ws.send(Uint8Array.from('WS_CLIENT_BINARY_MARKER', c=>c.charCodeAt(0)));
                        };
                        ws.onmessage=e=>{
                            if(typeof e.data==='string') text=e.data==='WS_SERVER_TEXT_MARKER';
                            else binary=String.fromCharCode(...new Uint8Array(e.data))==='WS_SERVER_BINARY_MARKER';
                            if(clientClose && text && binary) ws.close(1000,'WS_CLIENT_CLOSE_MARKER');
                        };
                        ws.onerror=()=>reject(new Error('Synthetic WebSocket failed'));
                        ws.onclose=e=>resolve(text && binary && e.code===1000 &&
                            e.reason===(clientClose?'WS_CLIENT_CLOSE_MARKER':'WS_SERVER_CLOSE_MARKER'));
                    });
                }
                Promise.all([checkWS(false), checkWS(true)])
                    .then(results=>{
                        if(!results.every(Boolean)) return;
                        return fetch('/ws-cookie-check', {credentials:'include'}).then(r=>r.text())
                            .then(text=>window.__wsDone=text==='OK');
                    });
                </script>'''.replace('PORT', str(ws_server.server_port))).encode()
                mime = 'text/html'
            self.send_response(200)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(body)))
            if self.path.startswith('/data'):
                self.send_header('Set-Cookie', 'later=RESPONSE_COOKIE_MARKER; Path=/; HttpOnly')
            else:
                self.send_header('Set-Cookie', 'session=REQUEST_COOKIE_MARKER; Path=/; HttpOnly')
            self.send_header('X-Private', 'CUSTOM_HEADER_MARKER')
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    ws_server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), WebSocketFixture)
    threading.Thread(target=ws_server.serve_forever, daemon=True).start()
    cors_server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), CorsFixture)
    threading.Thread(target=cors_server.serve_forever, daemon=True).start()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(path, body=None):
        req = urllib.request.Request(f'http://127.0.0.1:{port}' + path,
            data=None if body is None else json.dumps(body).encode(),
            headers={'Content-Type': 'application/json'})
        with opener.open(req, timeout=5) as response:
            return json.load(response)['value']

    result = {'kind': 'synthetic-info-debug-trace-log-privacy', 'passed': False}
    try:
        with tempfile.TemporaryDirectory(prefix='privacy-', dir=work) as directory:
            profile = Path(directory)
            fixture = profile / 'session.json'
            fixture.write_text(json.dumps({'session': [f'http://127.0.0.1:{server.server_port}/PRIVATE_PATH_MARKER?token=QUERY_VALUE_MARKER'], 'active_tab': 0}))
            env = os.environ.copy()
            system = Path(os.environ['SystemRoot'])
            for key in list(env):
                if key.lower() == 'path':
                    del env[key]
            env.update(PATH=os.pathsep.join([str(system / 'System32'), str(system)]),
                RUST_BROWSER_PROFILE=str(profile), RUST_LOG='warn,net=trace,servoshell=info',
                SERVO_TRACING='warn,net=trace,servoshell=info',
                GST_REGISTRY=str(profile / 'gst-registry.bin'),
                GST_PLUGIN_PATH_1_0='', GST_PLUGIN_SYSTEM_PATH_1_0='', GST_PLUGIN_PATH='')
            with (work / 'engine.log').open('wb') as log:
                job = OwnedJob()
                proc = subprocess.Popen([str(app), '--headless', '--session-fixture', str(fixture), '--webdriver', str(port)],
                    cwd=app.parent, env=env, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
                try:
                    job.assign(proc)
                    deadline = time.monotonic() + 30
                    sid = None
                    while time.monotonic() < deadline:
                        if proc.poll() is not None:
                            raise AssertionError('Engine exited before fixture completion')
                        try:
                            if sid is None:
                                request('/status')
                                sid = request('/session', {'capabilities': {}})['sessionId']
                                request('/session/' + sid + '/url', {'url': f'http://127.0.0.1:{server.server_port}/PRIVATE_PATH_MARKER?token=QUERY_VALUE_MARKER'})
                            done = request('/session/' + sid + '/execute/sync', {'script': 'return window.__privacyDone===true && window.__imageLoaded===true && window.__corsDone===true && window.__wsDone===true', 'args': []})
                            with lock:
                                observed['cors_denied_blocked'] = done is True and not denied_request_seen
                                observed['ws_browser_complete'] = done is True
                                completed = all(observed.values())
                            if done is True and completed:
                                break
                        except (OSError, KeyError):
                            pass
                        time.sleep(0.1)
                    else:
                        raise AssertionError('Synthetic HTTP/CORS/WebSocket fixture did not complete')
                    assert not any((profile / name).exists() for name in ('profile.json', 'passwords.dat', 'connections.dat'))
                finally:
                    job.close()
                    if proc.poll() is None:
                        proc.terminate()
                    proc.wait(timeout=10)
        log_text = (work / 'engine.log').read_text(encoding='utf-8', errors='replace')
        # Logs remain local; the report identifies categories/counts, never values.
        result.update(log_evidence(log_text, observed))
        assert result['diagnostics_present'], 'Logging must be active; empty logs are not redaction'
        assert result['cors_diagnostics_present'], 'Actual CORS diagnostics must be observed'
        assert result['websocket_diagnostics_present'], 'Actual WebSocket diagnostics must be observed'
        assert result['websocket_trace_present'] and result['fetch_trace_present'], 'Actual WS/fetch TRACE events must be observed'
        assert result['leaked_marker_categories'] == 0, 'Synthetic secret markers found in package diagnostics'
        assert result['passed'], 'Synthetic fixture acceptance is incomplete'
        print(json.dumps(result))
    finally:
        server.shutdown()
        server.server_close()
        cors_server.shutdown()
        cors_server.server_close()
        ws_server.shutdown()
        ws_server.server_close()
        (work / 'results.json').write_text(json.dumps(result, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
