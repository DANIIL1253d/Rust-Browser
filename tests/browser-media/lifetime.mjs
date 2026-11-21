import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [app, work, video] = process.argv.slice(2).map(p => path.resolve(p));
fs.mkdirSync(work, {recursive:true});
const bytes = fs.readFileSync(video);
const script = `
(async () => {
  const result = {stage:'start', forcedGcCount:0};
  window.__lifetimeProgress = result;
  const weak = await (async () => {
    const v = document.createElement('video'); v.muted = true; v.loop = true; v.src = '/video';
    await v.play(); return new WeakRef(v);
  })();
  result.stage = 'allocation-churn';
  let retained = [];
  if(typeof TestUtils?.gc !== 'function') throw new Error('Engine GC test API unavailable');
  for(let i=0;i<20;i++) {
    retained.push(Array.from({length:20000}, (_,j) => ({a:j+i, b:String(j), c:j%7})));
    if(retained.length>3) retained.shift();
    await new Promise(resolve => setTimeout(resolve, 20));
    await TestUtils.gc(); result.forcedGcCount++;
  }
  retained = null;
  await new Promise(resolve => setTimeout(resolve, 500));
  const v = weak.deref();
  result.alive = !!v;
  if(v) {
    result.before = v.currentTime;
    await new Promise(resolve => setTimeout(resolve, 1200));
    result.after = v.currentTime;
    result.width = v.videoWidth;
    result.error = v.error ? v.error.code : null;
    const c = document.createElement('canvas'); c.width=8; c.height=8;
    const x = c.getContext('2d'); x.drawImage(v,0,0,8,8);
    const pixels = Array.from(x.getImageData(0,0,8,8).data);
    result.decodedPixels = pixels.some((v,i) => i%4!==3 && v>0);
    v.pause(); v.removeAttribute('src'); v.load();
  }
  result.stage = 'iframe-teardown'; result.iframeTeardowns = 0;
  for(let i=0;i<10;i++) {
    const f = document.createElement('iframe');
    const loaded = new Promise(resolve => {f.onload = resolve;});
    f.src = '/frame'; document.body.append(f); await loaded;
    const inner = f.contentDocument.querySelector('video');
    inner.muted = true; await inner.play();
    await new Promise(resolve => setTimeout(resolve, 50));
    await TestUtils.gc();
    f.remove(); await TestUtils.gc();
    result.iframeTeardowns++;
  }
  result.stage = 'complete'; window.__lifetimeResult = result;
})().catch(e => {window.__lifetimeResult = {error:String(e),stage:window.__lifetimeProgress?.stage};});`;
const server = http.createServer((req,res) => {
  if(req.url === '/frame') {res.writeHead(200,{'Content-Type':'text/html'});res.end('<!doctype html><video src="/video" muted></video>');return;}
  if(req.url === '/video') {
    let start=0,end=bytes.length-1,status=200;
    const m=/^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
    if(m) { start=Number(m[1]); end=Math.min(end,m[2]?Number(m[2]):end); status=206; }
    if(start>end) {res.writeHead(416);res.end();return;}
    res.writeHead(status,{'Content-Type':'video/webm','Content-Length':end-start+1,'Accept-Ranges':'bytes',...(status===206?{'Content-Range':`bytes ${start}-${end}/${bytes.length}`}:{})});
    res.end(bytes.subarray(start,end+1)); return;
  }
  res.writeHead(200,{'Content-Type':'text/html'});res.end('<!doctype html><title>Owned media lifetime fixture</title><script>'+script+'</script>');
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const probe=http.createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
const profile=fs.mkdtempSync(path.join(work,'synthetic-'));
const session=path.join(profile,'session.json');fs.writeFileSync(session,JSON.stringify({session:[`http://127.0.0.1:${server.address().port}/`],active_tab:0}));
const env={...process.env,RUST_BROWSER_PROFILE:profile,GST_REGISTRY:path.join(profile,'gst-registry.bin'),GST_PLUGIN_SYSTEM_PATH_1_0:'',GST_PLUGIN_PATH_1_0:'',GST_PLUGIN_PATH:'',RUST_LOG:'warn'};
const system=process.env.SystemRoot||process.env.SYSTEMROOT;
for(const key of Object.keys(env))if(key.toLowerCase()==='path')delete env[key];
env.PATH=[path.join(system,'System32'),system].join(path.delimiter);
const log=fs.openSync(path.join(work,'engine.log'),'w');
const child=spawn(app,['--headless','--session-fixture',session,'--webdriver',String(port),'--pref','dom_testutils_enabled=true'],{cwd:path.dirname(app),env,stdio:['ignore',log,log],windowsHide:true});
let sid,result;
async function request(url,body) {const r=await fetch(`http://127.0.0.1:${port}`+url,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});const v=(await r.json()).value;if(!r.ok)throw Error(JSON.stringify(v));return v;}
async function until(fn) {const end=Date.now()+100000;while(Date.now()<end){if(child.exitCode!==null)throw Error('Engine exited: '+child.exitCode);try{const v=await fn();if(v)return v;}catch(e){if(child.exitCode!==null)throw e;}await new Promise(r=>setTimeout(r,200));}throw Error('Fixture timed out');}
try {
  await until(()=>request('/status'));sid=(await request('/session',{capabilities:{}})).sessionId;
  result=await until(()=>request('/session/'+sid+'/execute/sync',{script:'return window.__lifetimeResult || null',args:[]}));
  console.log(JSON.stringify(result,null,2));
  assert.ok(!result.error,result.error); assert.equal(result.forcedGcCount,20,'Engine GC stress did not complete');
  assert.equal(result.iframeTeardowns,10,'Iframe media teardown stress did not complete');
  assert.equal(result.alive,true,'Playing detached media was collected');assert.ok(result.width>0 && result.error===null && result.after!==result.before,'Detached media stopped decoding');
  assert.ok(result.decodedPixels,'No decoded video pixels');
} catch(e) { result={...(result||{}),runnerError:String(e),exitCode:child.exitCode};throw e; }
finally {fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(result,null,2));if(sid){try{await fetch(`http://127.0.0.1:${port}/session/${sid}`,{method:'DELETE',signal:AbortSignal.timeout(5000)});}catch{}}child.kill();fs.closeSync(log);server.close();}
