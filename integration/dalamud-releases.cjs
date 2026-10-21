"use strict";
const assert = require("node:assert/strict");
const {createHash} = require("node:crypto");
const {headers} = require("./dalamud-client.cjs");
const base = process.env.BASE_URL || "http://localhost:8080";
assert(["localhost","127.0.0.1","[::1]"].includes(new URL(base).hostname),"Use an isolated loopback fixture");
const url = path => new URL("/api/v1/plugin/"+path,base);
async function main() {
  const capabilities = await (await fetch(url("capabilities"))).json();
  assert.equal(capabilities.protocol,1);
  assert.match(capabilities.minimum_client_version,/^\d+\.\d+\.\d+\.\d+$/);
  for (const unsupported of [{},{...headers,"X-Ultros-Client-Version":"0.2.0.0"},
    {...headers,"X-Ultros-Client-Version":"01.0.0.0"},{...headers,"X-Ultros-Protocol-Version":"2"}]) {
    const response = await fetch(url("pairings"),{method:"POST",headers:{...unsupported,"Content-Type":"application/json"},body:JSON.stringify({label:"Unsupported client fixture",scopes:["lists:read"]})});
    assert.equal(response.status,426);
    assert.equal((await response.json()).error,"client_update_required");
    assert.match(response.headers.get("cache-control"),/no-store/);
  }
  assert.equal((await fetch(url("session"),{headers})).status,401,"Compatible version labels are not authentication");
  assert.equal((await fetch(url("session/revoke"),{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({refresh_token:"invalid-fixture-credential"})})).status,401,"Old clients can still attempt credential revocation");
  const repository = await fetch(url("repository.json"));
  assert.equal(repository.status,200);assert.match(repository.headers.get("cache-control"),/^public/);
  const entries = await repository.json();
  const release = await fetch(url("release"));
  if (!entries.length) {
    assert.equal(release.status,404);assert.equal(capabilities.latest_client_version,null);
    assert.equal((await fetch(url("download"),{redirect:"manual"})).status,404);
    assert(!process.env.EXPECT_PLUGIN_RELEASE,"Expected a published plugin package");
    console.log("PASS: minimum/protocol gates, independent authentication, revocation and empty release repository.");
    return;
  }
  assert.equal(entries.length,1);
  const entry=entries[0], metadata=await release.json();
  assert.equal(entry.InternalName,"Ultros");assert.equal(entry.Name,"Ultros");assert.equal(entry.DalamudApiLevel,15);
  assert.equal(entry.AssemblyVersion,metadata.version);assert.equal(capabilities.latest_client_version,metadata.version);
  assert.equal(entry.DownloadLinkInstall,entry.DownloadLinkUpdate);
  assert.equal(new URL(entry.DownloadLinkInstall).origin,new URL(base).origin);
  const response=await fetch(entry.DownloadLinkInstall);
  assert.equal(response.status,200);assert.match(response.headers.get("cache-control"),/immutable/);
  assert.equal(response.headers.get("content-type"),"application/zip");
  const bytes=Buffer.from(await response.arrayBuffer());
  assert.equal(bytes.length,metadata.size);assert.equal(createHash("sha256").update(bytes).digest("hex"),metadata.sha256);
  assert.equal(bytes.readUInt32LE(0),0x04034b50,"Download is a ZIP archive");
  const etag=response.headers.get("etag");assert(etag);
  assert.equal((await fetch(entry.DownloadLinkInstall,{headers:{"If-None-Match":etag}})).status,304);
  const range=await fetch(entry.DownloadLinkInstall,{headers:{Range:"bytes=0-3"}});
  assert.equal(range.status,206);assert.equal((await range.arrayBuffer()).byteLength,4);
  const latest=await fetch(url("download"),{redirect:"manual"});assert.equal(latest.status,307);
  assert.equal(new URL(latest.headers.get("location"),base).href,entry.DownloadLinkInstall);
  console.log("PASS: version/protocol rejection, independent auth, old-client revocation, repository metadata, exact ZIP checksum/size, immutable caching, conditional and range downloads.");
}
main().catch(error=>{console.error(error);process.exitCode=1;});
