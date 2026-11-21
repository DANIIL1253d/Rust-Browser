"""Test Cloudflare's official dummy widget in an isolated browser profile.

This checks compatibility, not production challenge completion. No real sitekeys,
accounts, clearance cookies or ordinary browser profiles are used.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

PAGE = r'''<!doctype html><meta charset="utf-8"><title>Turnstile compatibility fixture</title>
<div id="widget"></div><script>
const result = {errors: [], capabilities: {cookieEnabled: navigator.cookieEnabled,
  wasm: typeof WebAssembly === 'object', subtleCrypto: !!crypto.subtle}};
let finished = false;
function finish(status, code) {
  if (finished) return; finished = true;
  result.status = status; result.code = code || null;
  result.sameUserAgent = navigator.userAgent === EXPECTED_USER_AGENT;
  result.cookieRoundtrip = document.cookie.includes('compat_js=owned-test-value');
  result.storageRoundtrip = localStorage.getItem('compat') === 'owned-test-value';
  fetch('/report', {method:'POST', body: JSON.stringify(result)});
}
addEventListener('error', e => { if (e.message) result.errors.push(e.message.slice(0, 200)); });
document.cookie = 'compat_js=owned-test-value; Path=/; SameSite=Lax';
localStorage.setItem('compat', 'owned-test-value');
function onTurnstileLoaded() {
  result.apiLoaded = true;
  turnstile.render('#widget', {sitekey:'1x00000000000000000000AA',
    callback: token => { result.dummyToken = token === 'XXXX.DUMMY.TOKEN.XXXX'; finish('callback'); },
    'error-callback': code => finish('widget-error', String(code)), retry:'never'});
}
setTimeout(() => finish('timeout'), 45000);
</script><script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit&amp;onload=onTurnstileLoaded"
async defer onerror="finish('script-load-error')"></script>'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    app, work = args.app.resolve(), args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    done, observed = threading.Event(), {}

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = PAGE.replace('EXPECTED_USER_AGENT', json.dumps(self.headers.get('User-Agent', ''))).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Set-Cookie', 'compat_http=owned-test-value; HttpOnly; Path=/; SameSite=Lax')
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            size = int(self.headers.get('Content-Length', '0'))
            if self.path != '/report' or not 0 < size <= 16384:
                self.send_error(400)
                return
            observed.update(json.loads(self.rfile.read(size)))
            observed['httpOnlyRoundtrip'] = 'compat_http=owned-test-value' in self.headers.get('Cookie', '')
            self.send_response(204)
            self.end_headers()
            done.set()

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = os.environ.copy()
    env['RUST_BROWSER_PROFILE'] = tempfile.mkdtemp(prefix='synthetic-', dir=work)
    env['GST_REGISTRY'] = str(Path(env['RUST_BROWSER_PROFILE']) / 'gst-registry.bin')
    for key in ('GST_PLUGIN_PATH', 'GST_PLUGIN_PATH_1_0', 'GST_PLUGIN_SYSTEM_PATH_1_0'):
        env[key] = ''
    system = os.environ.get('SystemRoot') or os.environ['SYSTEMROOT']
    for key in list(env):
        if key.lower() == 'path':
            del env[key]
    env['PATH'] = os.pathsep.join([str(Path(system) / 'System32'), system])
    try:
        with (work / 'engine.log').open('w', encoding='utf-8') as log:
            process = subprocess.Popen([str(app), '--headless', '--pref', 'network_browser_route_mode=direct',
                                        f'http://127.0.0.1:{server.server_port}/'],
                                       cwd=app.parent, env=env, stdout=log, stderr=log,
                                       creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                if not done.wait(75):
                    observed['runnerError'] = 'No fixture report within 75 seconds'
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=15)
    finally:
        server.shutdown()
        (work / 'results.json').write_text(json.dumps(observed, indent=2), encoding='utf-8')
    print(json.dumps(observed, indent=2))
    assert observed.get('sameUserAgent') and observed.get('cookieRoundtrip') and observed.get('httpOnlyRoundtrip') and observed.get('storageRoundtrip'), 'Browser compatibility prerequisites failed'
    assert observed.get('status') == 'callback' and observed.get('dummyToken'), 'Official dummy widget did not complete'


if __name__ == '__main__':
    main()
