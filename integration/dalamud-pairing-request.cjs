"use strict";
const assert=require("node:assert/strict");
const {randomBytes}=require("node:crypto");
const source="127."+[...randomBytes(3)].map(byte=>1+byte%254).join(".");

// Each QA process uses a real, distinct loopback source address so browser
// retries cannot exhaust the player's shared localhost pairing quota. No
// forwarding headers or server-side authorization/rate checks are bypassed.
module.exports=async function pairingRequest(url,body){
  url=new URL(url);
  assert(["localhost","127.0.0.1","[::1]"].includes(url.hostname)&&url.pathname==="/api/v1/plugin/pairings"&&!url.search);
  if(url.hostname==="[::1]")return fetch(url,{method:"POST",redirect:"manual",headers:{"Content-Type":"application/json"},body});
  const payload=Buffer.from(body);
  return new Promise((resolve,reject)=>{
    const request=require(url.protocol==="https:"?"node:https":"node:http").request(url,{method:"POST",family:4,localAddress:source,headers:{"Content-Type":"application/json","Content-Length":payload.length}},response=>{
      const chunks=[];let size=0;
      response.on("data",chunk=>{size+=chunk.length;if(size>65536)response.destroy(new Error("Oversized pairing response"));else chunks.push(chunk);});
      response.on("error",reject);
      response.on("end",()=>resolve(new Response(Buffer.concat(chunks),{status:response.statusCode,headers:Object.fromEntries(Object.entries(response.headers).filter(([,value])=>value!==undefined).map(([key,value])=>[key,Array.isArray(value)?value.join(", "):value]))})));
    });
    request.on("error",reject);request.setTimeout(45000,()=>request.destroy(new Error("Pairing request timed out")));request.end(payload);
  });
};
