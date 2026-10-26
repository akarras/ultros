"use strict";
const fs = require("node:fs");
const path = require("node:path");
const version = fs.readFileSync(path.join(__dirname,"../plugins/dalamud/Directory.Build.props"),"utf8")
  .match(/<UltrosPluginVersion>([^<]+)<\/UltrosPluginVersion>/)?.[1];
if (!version) throw new Error("Missing plugin test client version");
const headers = Object.freeze({"X-Ultros-Client-Version":version,"X-Ultros-Protocol-Version":"1"});
function clientFetch(input, init = {}) {
  const url = new URL(input instanceof Request ? input.url : input);
  const merged = new Headers(input instanceof Request ? input.headers : undefined);
  new Headers(init.headers).forEach((value,key)=>merged.set(key,value));
  if (url.pathname.startsWith("/api/v1/plugin/") && !url.pathname.startsWith("/api/v1/plugin/browser/")) {
    for (const [name,value] of Object.entries(headers)) if (!merged.has(name)) merged.set(name,value);
  }
  return globalThis.fetch(input,{...init,headers:merged});
}
async function enableLabs(page, base) {
  const url = new URL(base);
  if (!["localhost","127.0.0.1","[::1]"].includes(url.hostname)) throw new Error("Labs fixtures require a loopback server");
  await page.setCookie({name:"LABS_DALAMUD",value:"true",url:url.origin+"/",path:"/",sameSite:"Lax"});
}
module.exports = {headers,clientFetch,version,enableLabs};
