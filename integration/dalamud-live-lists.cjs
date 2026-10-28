"use strict";
const assert=require("node:assert/strict");
const {once}=require("node:events");
const WebSocket=require("ws");
const {headers}=require("./dalamud-client.cjs");

async function within(promise,ms,label) {
  let timer;
  try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(label)),ms);})]);}
  finally{clearTimeout(timer);}
}
async function connect(url,access) {
  const socket=new WebSocket(url,{headers:{...headers,Authorization:"Bearer "+access}});
  const messages=[];let invalid;
  socket.on("error",()=>{});
  socket.on("message",data=>{try{assert(data.length<=512);messages.push(JSON.parse(data.toString()));}catch(error){invalid=error;}});
  const closed=new Promise(resolve=>socket.once("close",resolve));
  try{await within(once(socket,"open"),10000,"List WebSocket did not open");}
  catch(error){socket.terminate();throw error;}
  return {socket,closed,async next(id){
    await within((async()=>{while(!messages.length){if(invalid)throw invalid;if(socket.readyState!==WebSocket.OPEN)throw new Error("List WebSocket closed before a wake");await new Promise(resolve=>setTimeout(resolve,10));}})(),10000,"List change did not reach its WebSocket");
    assert.deepEqual(messages.shift(),{event:"changed",list_id:id});
  }};
}
async function rejected(url,extra,expected) {
  const socket=new WebSocket(url,{headers:{...headers,...extra}});socket.on("error",()=>{});
  try {
    const status=await within(new Promise((resolve,reject)=>{
      socket.once("unexpected-response",(_request,response)=>{response.resume();resolve(response.statusCode);});
      socket.once("open",()=>reject(new Error("An unauthorized list WebSocket opened")));
      socket.once("error",reject);
    }),10000,"List WebSocket rejection did not finish");
    assert(expected.includes(status),`Unexpected list stream status ${status}`);
  }finally{socket.terminate();}
}

module.exports=async function liveLists({api,page,cookies,access,world}) {
  const base=process.env.BASE_URL||"http://127.0.0.1:8080";
  assert(["127.0.0.1","localhost","[::1]"].includes(new URL(base).hostname),"use a disposable loopback server");
  const browserHeaders={Cookie:cookies,Origin:new URL(base).origin};
  const name="Game realtime fixture "+crypto.randomUUID();let id;let wire;
  const url=list=>{const value=new URL(`/api/v1/plugin/lists/${list}/stream`,base);value.protocol=value.protocol==="https:"?"wss:":"ws:";return value.href;};
  try {
    assert.equal((await api("POST","/api/v1/list/create",{name,wdr_filter:{World:world}},undefined,browserHeaders)).status,200);
    const lists=await api("GET","/api/v1/list",undefined,undefined,browserHeaders);
    id=lists.data.find(entry=>entry.list.name===name)?.list.id;assert(id,"fixture list was not created");
    const sync=()=>api("POST",`/api/v1/plugin/lists/${id}/sync`,{version:"",update:null},access);
    const initial=await sync();assert.equal(initial.status,200);
    await rejected(url(id),{Cookie:cookies},[401]);
    await rejected(url(2147483647),{Authorization:"Bearer "+access},[409]);
    wire=await connect(url(id),access);await wire.next(id);
    const add=async item=>{
      assert.equal((await api("POST",`/api/v1/list/${id}/add/item`,{id:0,item_id:item,list_id:id,hq:false,quantity:1,acquired:0},undefined,browserHeaders)).status,200);
    };
    await add(8);await wire.next(id);
    const changed=await sync();assert.equal(changed.status,200);assert.notEqual(changed.data.version,initial.data.version,"a wake did not expose the committed document update");
    wire.socket.close();await within(wire.closed,5000,"List WebSocket did not close");
    await add(9);
    wire=await connect(url(id),access);await wire.next(id);
    const caughtUp=await sync();assert.equal(caughtUp.status,200);assert.notEqual(caughtUp.data.version,changed.data.version,"reconnect did not catch up edits made while disconnected");
    assert.equal((await api("DELETE",`/api/v1/list/${id}/delete`,undefined,undefined,browserHeaders)).status,200);
    await within(wire.closed,10000,"A deleted list retained its live subscription");
    assert.equal((await sync()).status,409,"a deleted list remained readable to the game client");
    id=null;
    // The page remains usable after the native-only subscription is retired.
    assert(await page.title());
    console.log("PASS: bearer-only game list WebSocket, committed browser edits, reconnect catch-up and immediate retirement after list deletion.");
  }finally{
    wire?.socket.terminate();
    if(id)await api("DELETE",`/api/v1/list/${id}/delete`,undefined,undefined,browserHeaders);
  }
};
