// node tests/browser-mse/engine.mjs app/RustBrowser.exe .build/mse-check --enabled --fixtures=PATH
// Default verifies that the unfinished experimental API stays gated off.
import fs from 'node:fs';import path from 'node:path';import http from 'node:http';import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const app=path.resolve(process.argv[2]||path.join(root,'app/RustBrowser.exe'));
const work=path.resolve(process.argv[3]||path.join(root,'.build/mse-check'));
const enabled=process.argv.includes('--enabled');
const fixtures=process.argv.find(a=>a.startsWith('--fixtures='))?.slice(11);
const selectedCase=process.argv.find(a=>a.startsWith('--case='))?.slice(7);
fs.mkdirSync(work,{recursive:true});const profile=fs.mkdtempSync(path.join(work,'synthetic-'));
const contract=fs.readFileSync(path.join(root,'tests/browser-mse/contract.js'),'utf8');
const cspContract=fs.readFileSync(path.join(root,'tests/browser-mse/csp.js'),'utf8');
const server=http.createServer((req,res)=>{
  if(req.url==='/csp') {
    res.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store','Content-Security-Policy':"media-src 'none'"});
    res.end('<!doctype html><title>MSE CSP</title><body><script>'+cspContract+'</script>');return;
  }
  if(req.url==='/video.webm'||req.url==='/audio.webm') {
    const file=fixtures&&path.join(fixtures,req.url.slice(1));
    if(!file||!fs.existsSync(file)){res.writeHead(404);res.end();return;}
    res.writeHead(200,{'Content-Type':req.url==='/video.webm'?'video/webm':'audio/webm','Cache-Control':'no-store'});fs.createReadStream(file).pipe(res);return;
  }
  res.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store'});
  res.end('<!doctype html><title>Isolated MSE contract</title><body><script>'+ (enabled?contract:'window.__mseResult={disabled:typeof MediaSource==="undefined"};')+'</script>');
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const probe=http.createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
const base=`http://127.0.0.1:${port}`, origin=`http://127.0.0.1:${server.address().port}`;
const log=fs.openSync(path.join(work,'engine.log'),'w');
const fixture=path.join(profile,'session.json');
fs.writeFileSync(fixture,JSON.stringify({session:[origin+(selectedCase?'/?case='+encodeURIComponent(selectedCase):'')],active_tab:0}));
// Resolve native media dependencies from the package, never a developer SDK.
const env={...process.env,RUST_BROWSER_PROFILE:profile,GST_REGISTRY:path.join(profile,'gst-registry.bin'),GST_PLUGIN_SYSTEM_PATH_1_0:'',GST_PLUGIN_PATH_1_0:'',GST_PLUGIN_PATH:''};
const system=process.env.SystemRoot||process.env.SYSTEMROOT;
for(const key of Object.keys(env))if(key.toLowerCase()==='path')delete env[key];
env.PATH=[path.join(system,'System32'),system].join(path.delimiter);
const child=spawn(app,['--headless','--session-fixture',fixture,'--webdriver',String(port),...(enabled&&!process.argv.includes('--baseline')?['--pref','dom_media_source_enabled=true']:[])],{cwd:path.dirname(app),env,stdio:['ignore',log,log],windowsHide:true});
let sid,result,progress;
async function request(url,body,method=body===undefined?'GET':'POST') {
  const response=await fetch(base+url,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});const value=(await response.json()).value;
  if(!response.ok)throw new Error(JSON.stringify(value));return value;
}
async function wait(fn){const end=Date.now()+150000;while(Date.now()<end){if(child.exitCode!==null)throw new Error('Engine exited before fixture completion: '+child.exitCode);try{const v=await fn();if(v)return v;}catch{}await new Promise(r=>setTimeout(r,100));}throw new Error('MSE fixture timed out');}
try {
  await wait(()=>request('/status'));sid=(await request('/session',{capabilities:{}})).sessionId;
  result=await wait(async()=>{
    const snapshot=await request('/session/'+sid+'/execute/sync',{script:'return {result:window.__mseResult||null,progress:window.__mseProgress||null}',args:[]});
    progress=snapshot.progress;return snapshot.result;
  });
  console.log(JSON.stringify(result,null,2));
  assert.ok(!result.error,result.error);if(!enabled)assert.equal(result.disabled,true,'Default MSE gate must remain off');
} finally {
  fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(result||{error:'runner failed',progress},null,2));
  if(sid){try{await request('/session/'+sid,undefined,'DELETE')}catch{}}
  child.kill();fs.closeSync(log);server.close();
}
