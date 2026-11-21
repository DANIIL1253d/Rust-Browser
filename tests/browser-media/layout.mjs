import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [app, work, clip] = process.argv.slice(2,5).map(p => path.resolve(p));
const seconds=Number(process.argv[5]||8);
assert.ok(Number.isFinite(seconds) && seconds>=8 && seconds<=180,'Probe duration must be 8..180 seconds');
fs.mkdirSync(work, {recursive:true});
const bytes = fs.readFileSync(clip);
const server = http.createServer((req,res) => {
  if(req.url === '/clip') {
    res.writeHead(200, {'Content-Type':'video/webm','Content-Length':bytes.length});
    res.end(bytes); return;
  }
  res.writeHead(200, {'Content-Type':'text/html'});
  res.end(`<!doctype html><title>Owned video layout fixture</title>
    <style>body{margin:0}main{display:grid;grid-template-columns:repeat(4,1fr)}article{padding:8px;border:1px solid #aaa}video{width:320px;height:180px}</style>
    <video muted loop src="/clip"></video><main>${'<article><b>Static fixture card</b><p>Unchanging page content.</p></article>'.repeat(1000)}</main>`);
});
await new Promise(r => server.listen(0,'127.0.0.1',r));
const freePort = http.createServer(); await new Promise(r=>freePort.listen(0,'127.0.0.1',r));
const port=freePort.address().port; await new Promise(r=>freePort.close(r));
const profile=fs.mkdtempSync(path.join(work,'synthetic-'));
const session=path.join(profile,'session.json');
fs.writeFileSync(session,JSON.stringify({session:[`http://127.0.0.1:${server.address().port}/`],active_tab:0}));
const env={...process.env,RUST_BROWSER_PROFILE:profile,GST_REGISTRY:path.join(profile,'registry.bin'),GST_PLUGIN_SYSTEM_PATH_1_0:'',GST_PLUGIN_PATH_1_0:'',GST_PLUGIN_PATH:'',RUST_LOG:'warn'};
for(const key of Object.keys(env))if(key.toLowerCase()==='path')delete env[key];
env.PATH=[path.join(process.env.SystemRoot,'System32'),process.env.SystemRoot].join(path.delimiter);
const logPath=path.join(work,'engine.log'); const log=fs.openSync(logPath,'w');
const child=spawn(app,['--headless','--session-fixture',session,'--webdriver',String(port),'-p','1'],{cwd:path.dirname(app),env,stdio:['ignore',log,log],windowsHide:true});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
let sid,result={};
async function request(url,body) {
  const r=await fetch(`http://127.0.0.1:${port}${url}`,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
  const value=(await r.json()).value;if(!r.ok)throw Error(JSON.stringify(value));return value;
}
async function until(fn) {
  const end=Date.now()+45000;
  while(Date.now()<end){if(child.exitCode!==null)throw Error('Engine exited: '+child.exitCode);try{const value=await fn();if(value)return value;}catch{}await delay(200);}
  throw Error('Fixture timed out');
}
const execute=script=>request(`/session/${sid}/execute/sync`,{script,args:[]});
function counts() {
  const blocks=fs.readFileSync(logPath,'utf8').split('_category_');
  const all=blocks.filter(block=>block.includes('\n_url_\t_blocked layout queries_')).at(-1)?.split('\n')||[]; const rows={};
  for(const line of all) {
    const m=/^([A-Z][A-Za-z0-9]+)\s+.*?\s+(\d+)\s*$/.exec(line);
    if(m) {
      rows[m[1]]=(rows[m[1]]||0)+Number(m[2]);
      const mean=Number(line.trim().split(/\s+/).at(-5));
      assert.ok(Number.isFinite(mean),'Invalid profiler timing');
      const key=m[1]+'Milliseconds';rows[key]=(rows[key]||0)+mean*Number(m[2]);
    }
  }
  return rows;
}
try {
  await until(()=>request('/status')); sid=(await request('/session',{capabilities:{}})).sessionId;
  await until(()=>execute('return document.querySelector("video")?.readyState >= 2'));
  await delay(2000); result.before=counts();
  await execute('document.querySelector("video").play();return true');
  await delay(seconds*1000);
  result.video=await execute('const v=document.querySelector("video"); const c=document.createElement("canvas");c.width=8;c.height=8;const x=c.getContext("2d");x.drawImage(v,0,0,8,8);const decoded=Array.from(x.getImageData(0,0,8,8).data).some((v,i)=>i%4!==3&&v>0);v.pause();return {time:v.currentTime,width:v.videoWidth,error:v.error?.code||null,decoded};');
  await delay(1500); result.after=counts();
  result.extra=Object.fromEntries(Object.entries(result.after).map(([k,v])=>[k,v-(result.before[k]||0)]));
  console.log(JSON.stringify(result,null,2));
  assert.ok(result.video.decoded && result.video.width>0 && !result.video.error,'No decoded video pixels');
  const rebuilds=result.extra.Layout;
  assert.ok(Number.isInteger(rebuilds),'Layout profiler category missing');
  result.seconds=seconds;
  assert.ok(rebuilds<=20,`Steady video ran page layout ${rebuilds} times in ${seconds} seconds`);
} catch(e) {result.runnerError=String(e);throw e;}
finally {
  fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(result,null,2));
  if(sid)try{await fetch(`http://127.0.0.1:${port}/session/${sid}`,{method:'DELETE',signal:AbortSignal.timeout(3000)});}catch{}
  child.kill();fs.closeSync(log);server.close();
}
