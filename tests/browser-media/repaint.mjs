import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [app,work,fixtures]=process.argv.slice(2).map(p=>path.resolve(p));
fs.mkdirSync(work,{recursive:true});
const clips=Object.fromEntries(['red','blue','green'].map(name=>['/'+name,fs.readFileSync(path.join(fixtures,name+'.webm'))]));
const server=http.createServer((req,res)=>{
  if(clips[req.url]) {res.writeHead(200,{'Content-Type':'video/webm'});res.end(clips[req.url]);return;}
  res.writeHead(200,{'Content-Type':'text/html'});
  res.end('<!doctype html><title>Owned repaint fixture</title><style>body{margin:0}video{width:320px;height:180px}</style><video muted loop src="/red"></video>');
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const freePort=http.createServer();await new Promise(r=>freePort.listen(0,'127.0.0.1',r));
const port=freePort.address().port;await new Promise(r=>freePort.close(r));
const profile=fs.mkdtempSync(path.join(work,'synthetic-'));
const session=path.join(profile,'session.json');
fs.writeFileSync(session,JSON.stringify({session:[`http://127.0.0.1:${server.address().port}/`],active_tab:0}));
const env={...process.env,RUST_BROWSER_PROFILE:profile,GST_REGISTRY:path.join(profile,'registry.bin'),GST_PLUGIN_SYSTEM_PATH_1_0:'',GST_PLUGIN_PATH_1_0:'',GST_PLUGIN_PATH:'',RUST_LOG:'warn'};
for(const key of Object.keys(env))if(key.toLowerCase()==='path')delete env[key];
env.PATH=[path.join(process.env.SystemRoot,'System32'),process.env.SystemRoot].join(path.delimiter);
const log=fs.openSync(path.join(work,'engine.log'),'w');
const child=spawn(app,['--headless','--session-fixture',session,'--webdriver',String(port)],{cwd:path.dirname(app),env,stdio:['ignore',log,log],windowsHide:true});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
let sid,result={stages:[]};
async function request(url,body) {
  const r=await fetch(`http://127.0.0.1:${port}${url}`,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
  const value=(await r.json()).value;if(!r.ok)throw Error(JSON.stringify(value));return value;
}
async function until(fn) {
  const end=Date.now()+30000;
  while(Date.now()<end){if(child.exitCode!==null)throw Error('Engine exited: '+child.exitCode);try{const value=await fn();if(value)return value;}catch{}await delay(100);}
  throw Error('Fixture timed out');
}
const execute=script=>request(`/session/${sid}/execute/sync`,{script,args:[]});
async function screenshotColor(name,channel) {
  const png=await request(`/session/${sid}/screenshot`);
  fs.writeFileSync(path.join(work,name+'.png'),Buffer.from(png,'base64'));
  await execute(`window.__pixel=null;window.__shot=new Image();window.__shot.onload=()=>{const c=document.createElement('canvas');c.width=window.__shot.naturalWidth;c.height=window.__shot.naturalHeight;const x=c.getContext('2d');x.drawImage(window.__shot,0,0);window.__pixel=Array.from(x.getImageData(160,90,1,1).data);};window.__shot.src=${JSON.stringify('data:image/png;base64,'+png)};return true`);
  const pixel=await until(()=>execute('return window.__pixel'));
  assert.ok(pixel[channel]>180 && pixel.filter((_,i)=>i<3 && i!==channel).every(v=>v<60),name+': unexpected rendered pixel '+pixel);
  return pixel;
}
try {
  await until(()=>request('/status'));sid=(await request('/session',{capabilities:{}})).sessionId;
  for(const [name,channel,width] of [['red',0,160],['blue',2,160],['green',1,320]]) {
    if(name!=='red')await execute(`const v=document.querySelector('video');v.src='/${name}';v.load();v.play();return true`);
    else await execute('document.querySelector("video").play();return true');
    await until(()=>execute(`const v=document.querySelector('video');return v.readyState>=2 && v.videoWidth===${width} && v.currentTime>0.15`));
    const playing=await screenshotColor(name+'-playing',channel);
    await execute('document.querySelector("video").pause();return true');await delay(200);
    const paused=await screenshotColor(name+'-paused',channel);
    result.stages.push({name,width,playing,paused});
  }
  await execute('window.__detached=document.querySelector("video");window.__detached.remove();window.__detached.play();return true');
  await delay(300);await execute('document.body.append(window.__detached);return true');
  result.reattached=await screenshotColor('green-reattached',1);
  result.passed=true;console.log(JSON.stringify(result,null,2));
} catch(e) {result.error=String(e);throw e;}
finally {
  fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(result,null,2));
  if(sid)try{await fetch(`http://127.0.0.1:${port}/session/${sid}`,{method:'DELETE',signal:AbortSignal.timeout(3000)});}catch{}
  child.kill();fs.closeSync(log);server.close();
}
