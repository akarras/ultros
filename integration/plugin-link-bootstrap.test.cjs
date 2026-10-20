"use strict";
const {test}=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs");
const vm=require("node:vm");
const source=fs.readFileSync(require("node:path").join(__dirname,"../ultros-frontend/ultros-app/src/plugin-link-bootstrap.js"),"utf8");
const token=Buffer.alloc(32,17).toString("base64url");

function boot(hash,{ok=true,status=204,fail=false,path="/plugin/verify"}={}) {
  const calls=[];const events={};
  const location={pathname:path,search:"?lang=en",hash,reload(){calls.push({reload:true});}};
  const window={};
  const context={window,location,AbortController,setTimeout,clearTimeout,
    addEventListener(name,handler){events[name]=handler;},
    history:{state:null,replaceState(_state,_title,url){calls.push({replace:url});location.hash="";}},
    async fetch(url,options){calls.push({url,options});if(fail)throw new Error("network failure");return {ok,status};}};
  vm.runInNewContext(source,context);
  return {window,location,calls,events};
}

test("fragment is erased before the credential is posted, with no credential in HTTP URLs",async()=>{
  const page=boot("#pair="+token);
  assert.equal(page.calls[0].replace,"/plugin/verify?lang=en");
  assert.equal(page.location.hash,"");
  const request=page.calls[1];
  assert.equal(request.url,"/api/v1/plugin/browser/link");
  assert.equal(request.options.method,"POST");
  assert.equal(request.options.credentials,"same-origin");
  assert.equal(request.options.redirect,"error");
  assert.equal(new URLSearchParams(request.options.body).get("token"),token);
  assert.equal(await page.window.__ULTROS_PLUGIN_LINK_CAPTURE__,null);
  assert(!JSON.stringify(page.calls.map(({options,...request})=>request)).includes(token));
});

test("phone entry and unrelated anchors do not capture credentials",()=>{
  for(const hash of ["","#install","#troubleshooting"]){const page=boot(hash);assert.equal(page.calls.length,0);}
  assert.equal(boot("#pair="+token,{path:"/plugin"}).calls.length,0);
});

test("invalid or failed links are cleared and return a safe error",async()=>{
  for(const options of [{ok:false,status:400},{fail:true}]){
    const page=boot("#pair="+token,options);
    const message=await page.window.__ULTROS_PLUGIN_LINK_CAPTURE__;
    assert.equal(page.location.hash,"");
    assert.equal(typeof message,"string");assert(!message.includes(token));
  }
  const malformed=boot("#pair=ABCD-EFGH",{ok:false,status:400});
  assert.equal(new URLSearchParams(malformed.calls[1].options.body).get("token"),"","an invalid fragment still clears an older pending cookie server-side");
  await malformed.window.__ULTROS_PLUGIN_LINK_CAPTURE__;
});

test("opening a fresh link on an existing page waits for capture before reloading",async()=>{
  const page=boot("");page.location.hash="#pair="+token;
  page.events.hashchange();
  assert(!page.calls.some(call=>call.reload));
  await page.window.__ULTROS_PLUGIN_LINK_CAPTURE__;
  await Promise.resolve();
  assert(page.calls.some(call=>call.reload));
  assert.equal(page.location.hash,"");
});
