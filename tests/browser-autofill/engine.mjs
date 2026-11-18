// Real Servo DOM regression. Only synthetic loopback pages and fake credentials.
// node tests/browser-autofill/engine.mjs app/RustBrowser.exe .build/autofill-check
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const app = path.resolve(process.argv[2] || path.join(root, 'app/RustBrowser.exe'));
const work = path.resolve(process.argv[3] || path.join(root, '.build/autofill-check'));
fs.mkdirSync(work, {recursive:true});
const profile = fs.mkdtempSync(path.join(work, 'synthetic-'));
const server = http.createServer((req,res) => {
  res.writeHead(200, {'Content-Type':'text/html; charset=utf-8', 'Cache-Control':'no-store'});
  res.end('<!doctype html><title>Autofill synthetic fixture</title><body><main></main>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const portProbe = http.createServer();
await new Promise(resolve => portProbe.listen(0, '127.0.0.1', resolve));
const port = portProbe.address().port;
await new Promise(resolve => portProbe.close(resolve));
const base = `http://127.0.0.1:${port}`;
const log = fs.openSync(path.join(work, 'engine.log'), 'w');
const child = spawn(app, ['--headless','--temporary-storage','--config-dir',path.join(profile,'engine'),'--webdriver',String(port),origin+'/form'], {
  cwd:path.dirname(app), env:{...process.env,RUST_BROWSER_PROFILE:profile}, stdio:['ignore',log,log], windowsHide:true,
});
let session;
async function request(route, body, method = body === undefined ? 'GET' : 'POST') {
  const response = await fetch(base+route, {method, headers:{'Content-Type':'application/json'}, body:body === undefined ? undefined : JSON.stringify(body)});
  const result = await response.json();
  if (!response.ok || result.value?.error) throw new Error(JSON.stringify(result.value));
  return result.value;
}
async function eventually(fn) {
  const end=Date.now()+30000;
  while(Date.now()<end) { try { const value=await fn(); if(value)return value; } catch {} await new Promise(r=>setTimeout(r,100)); }
  throw new Error('Synthetic engine condition timed out');
}
function script(args) {
  const file=path.join(root,'ports/servoshell/browser-core/src/autofill.js');
  if(fs.existsSync(file)) return fs.readFileSync(file,'utf8')+'('+JSON.stringify(args)+')';
  // Before the fix, execute the actual Rust raw-string template to demonstrate red.
  const source=fs.readFileSync(path.join(root,'ports/servoshell/browser-core/src/passwords.rs'),'utf8');
  const raw=source.slice(source.indexOf('pub fn fill_script')).match(/r#"([\s\S]*?)"#/)[1];
  return raw.replaceAll('{{','{').replaceAll('}}','}').replace('{args}',JSON.stringify([args.origin,args.username,args.password]));
}
const results=[];
try {
  await eventually(()=>request('/status'));
  session=(await request('/session',{capabilities:{}})).sessionId;
  const prefix='/session/'+session;
  const js=code=>request(prefix+'/execute/sync',{script:code,args:[]});
  await eventually(()=>js('return document.readyState==="complete"'));
  async function form(html) { await js('document.body.innerHTML='+JSON.stringify(html)+'; window.__submits=0; document.addEventListener("submit", e=>{window.__submits++;e.preventDefault()});'); }
  async function fill() { return js('return '+script({mode:'fill',origin,username:'fake-alice@example.org',password:'FAKE-password-9xy'})); }
  async function check(name, html, expected) {
    await form(html); assert.equal(await fill(),true,name+' must fill an eligible field');
    assert.deepEqual(await js('return Array.from(document.querySelectorAll("input")).map(e=>e.value)'),expected,name);
    assert.equal(await js('return window.__submits'),0,'must never submit'); results.push(name);
  }
  await check('email-only', '<form><input id="identifierId" type="email" autocomplete="username"><button>Next</button></form>', ['fake-alice@example.org']);
  await check('password-only', '<form><input type="password" autocomplete="current-password"></form>', ['FAKE-password-9xy']);
  await check('otp-password-skipped', '<form><input type="password" name="otp"><input type="password" autocomplete="current-password"></form>', ['', 'FAKE-password-9xy']);
  await check('combined', '<form><input name="username"><input type="password"></form>', ['fake-alice@example.org','FAKE-password-9xy']);
  await check('unsafe-fields-ignored', '<form><input type="hidden"><input type="email" disabled><input type="text" readonly><input autocomplete="one-time-code"><input type="password" autocomplete="new-password"><input type="email" autocomplete="username"><input type="password" autocomplete="current-password"></form>', ['', '', '', '', '', 'fake-alice@example.org','FAKE-password-9xy']);
  await form('<form><input autocomplete="one-time-code"><input type="password" autocomplete="new-password"></form>');
  assert.equal(await fill(),false,'OTP and signup forms must not receive secrets'); results.push('otp-signup-blocked');
  await form('<input type="email" id="otp">');
  assert.equal(await fill(),false,'OTP names without autocomplete must not receive usernames'); results.push('otp-name-blocked');
  await form('<form><input type="email" autocomplete="username"><input type="password" autocomplete="new-password"></form>');
  assert.equal(await fill(),false,'registration email must not be treated as a login'); results.push('signup-email-blocked');
  await form('<form><input type="email" autocomplete="username" id="react"><input type="password"></form>');
  await js('window.__inputs=0;window.__changes=0;window.__intercept=0; var e=document.getElementById("react"); e.addEventListener("input",()=>window.__inputs++);e.addEventListener("change",()=>window.__changes++); Object.defineProperty(e,"value",{set:()=>window.__intercept++,get:()=>"framework override"})');
  assert.equal(await fill(),true);
  assert.deepEqual(await js('return [Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").get.call(document.getElementById("react")),window.__inputs,window.__changes,window.__intercept,window.__submits]'), ['fake-alice@example.org',1,1,0,0]); results.push('native-setter-events-no-submit');
  await form('<input type="email" autocomplete="username" id="login">');
  await js('document.querySelector("input").focus()');
  const probe=JSON.parse(await js('return '+script({mode:'discover',origin,token:'synthetic-binding',focused:true})));
  assert.equal(probe.origin,origin); assert.equal(probe.token,'synthetic-binding');
  assert.ok(!JSON.stringify(probe).includes('FAKE-password')); results.push('discovery-no-credentials');
  assert.equal(await js('return document.querySelector("input").value'),'','discovery must not fill');
  const bound={mode:'fill',origin,username:'fake-alice@example.org',password:'FAKE-password-9xy',token:probe.token};
  await js('document.querySelector("input").remove(); document.body.innerHTML="<input type=email>"');
  assert.equal(await js('return '+script(bound)),false,'replaced target must reject fill'); results.push('stale-target-blocked');
  const focusForm='<form><input id="bound-login" type="email" autocomplete="username"><input type="password" autocomplete="current-password"></form><input id="search" type="search"><div id="editable" contenteditable="true" tabindex="0">Other text</div><button id="other-button" type="button">Other action</button>';
  for (const focusId of ['search','editable','other-button']) {
    await form(focusForm);
    await js('document.getElementById("bound-login").focus()');
    const focusProbe=JSON.parse(await js('return '+script({mode:'discover',origin,token:'focus-'+focusId,focused:true})));
    assert.equal(focusProbe.role,'username','must bind the initially focused login');
    await js('document.getElementById('+JSON.stringify(focusId)+').focus()');
    assert.equal(await js('return document.activeElement.id'),focusId,'test must move actual DOM focus');
    assert.equal(await js('return '+script({...bound,token:focusProbe.token})),false,'old native suggestion must reject focus on '+focusId);
    assert.deepEqual(await js('return Array.from(document.querySelectorAll("input")).map(e=>e.value)'),['','',''],'stale suggestion must not fill any field');
    results.push('stale-focus-'+focusId+'-blocked');
  }
  await form(focusForm);
  await js('document.getElementById("bound-login").focus()');
  const unchangedProbe=JSON.parse(await js('return '+script({mode:'discover',origin,token:'unchanged-focus',focused:true})));
  assert.equal(await js('return '+script({...bound,token:unchangedProbe.token})),true,'unchanged DOM focus must still fill after native account selection');
  assert.deepEqual(await js('return Array.from(document.querySelectorAll("input")).map(e=>e.value)'),['fake-alice@example.org','FAKE-password-9xy','']);
  assert.equal(await js('return window.__submits'),0); results.push('unchanged-focus-fills');
  await form(focusForm);
  await js('document.getElementById("search").focus()');
  const managerProbe=JSON.parse(await js('return '+script({mode:'discover',origin,token:'explicit-manager',focused:false})));
  assert.equal(managerProbe.role,'password','explicit manager must find the login form without login focus');
  assert.equal(await js('return '+script({...bound,token:managerProbe.token})),true,'explicit manager fallback must remain available');
  assert.deepEqual(await js('return Array.from(document.querySelectorAll("input")).map(e=>e.value)'),['fake-alice@example.org','FAKE-password-9xy','']);
  assert.equal(await js('return window.__submits'),0); results.push('explicit-manager-fallback-fills');
  await form('<input type="email" autocomplete="username">');
  assert.equal(await js('return '+script({...bound,origin:'https://accounts.example.org'})),false,'origin mismatch'); results.push('origin-blocked');
  await request(prefix+'/url',{url:origin+'/form'});
  await form('<input type="email" autocomplete="username">');
  assert.equal(await js('return '+script(bound)),false,'same-origin replacement document must reject an old binding'); results.push('stale-document-blocked');
  await js('document.body.innerHTML="<iframe src=about:blank></iframe>";');
  assert.equal(await js('return '+script({mode:'discover',origin,token:'iframe',focused:true})), '', 'iframe focus not eligible'); results.push('iframe-blocked');
  console.log(JSON.stringify({passed:results},null,2));
} finally {
  fs.writeFileSync(path.join(work,'results.json'),JSON.stringify({passed:results},null,2));
  if(session) { try {await request('/session/'+session,undefined,'DELETE');} catch{} }
  child.kill(); fs.closeSync(log); server.close();
}
