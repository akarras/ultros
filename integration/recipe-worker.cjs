// Real WASM/worker boundary probe, separate from market/route fixtures.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');

async function staticFixture() {
  // Exercise a freshly built runtime without Postgres, Discord, or hydration.
  const pkg = path.resolve(__dirname, '../target/site/pkg');
  assert.ok(fs.existsSync(path.join(pkg,'ultros.js')), 'run cargo leptos build --frontend-only first');
  const staticRoot = path.resolve(__dirname, '../ultros/static');
  const server = http.createServer((req,res) => {
    const url = new URL(req.url,'http://localhost');
    if (url.pathname === '/') {
      res.setHeader('Content-Type','text/html');
      return res.end(`<link rel="modulepreload" href="/pkg/probe/ultros.js"><script type="module">import init from '/pkg/probe/ultros.js'; await init({module_or_path:'/pkg/probe/ultros.wasm'});</script>`);
    }
    const root = url.pathname.startsWith('/pkg/probe/') ? pkg : staticRoot;
    const prefix = root === pkg ? '/pkg/probe/' : '/static/';
    const file = path.resolve(root,decodeURIComponent(url.pathname.slice(prefix.length)));
    if (!url.pathname.startsWith(prefix) || !file.startsWith(root+path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      res.writeHead(404);return res.end();
    }
    const ext = path.extname(file);
    res.setHeader('Content-Type',ext === '.wasm' ? 'application/wasm' : ext === '.js' || ext === '.mjs' ? 'application/javascript' : 'application/octet-stream');
    fs.createReadStream(file).pipe(res);
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {base:`http://127.0.0.1:${server.address().port}`,close:()=>new Promise(resolve=>server.close(resolve))};
}

async function main() {
  const fixture = process.env.BASE_URL ? null : await staticFixture();
  const base = process.env.BASE_URL || fixture.base;
  let browser;
  try {
    const { default: puppeteer } = await import('puppeteer');
    browser = await puppeteer.launch({headless:true,args:['--no-sandbox']});
    const page = await browser.newPage();
    page.setDefaultTimeout(90000);
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.goto(base, {waitUntil:'networkidle2'});
    await page.waitForFunction(() => [...performance.getEntriesByType('resource')].some(e=>e.name.endsWith('.wasm')));
    const result = await page.evaluate(async () => {
      const {PlannerQueue,runtimeAssets} = await import('/static/recipe-worker-client.mjs');
      const assets = runtimeAssets();
      const runtime = await import(assets.js);
      const request = (worlds, items, needed, listings) => ({
        materials:Array.from({length:items},(_,i)=>({item:i+1,needed,owned:0,crafts:0,surplus:0,depth:0,recipe:null})),
        offers:Object.fromEntries(Array.from({length:items},(_,i)=>{
          const item=i+1;
          return [item,Array.from({length:worlds},(_,w)=>Array.from({length:listings},(_,n)=>({
            id:item*100000+(w+1)*1000+n,world:w+1,
            quantity:1+(n*17+item*3+(w+1)*7)%99,price:100+((w+1)*137+item*251+n*31)%5000,
          }))).flat()];
        })),
        vendors:{},context:{home:1,datacenters:Object.fromEntries(Array.from({length:worlds},(_,i)=>[i+1,Math.floor(i/8)])),weights:{world_hop:2000,dc_hop:10000},locked:{},unavailable:[]},
        route:null,visits:null,finished:null,craft_materials:null,buy_output:false,
      });
      const queue = new PlannerQueue();
      const warm = JSON.stringify(request(2,2,5,2));
      if (await queue.submit(warm) !== runtime.recipe_plan(warm)) throw new Error('Worker warmup differs from direct engine');
      const payload = JSON.stringify(request(32,24,100,20));
      let running = true, frames = 0, last = performance.now(), maxGap = 0;
      const frame = now => {frames++;maxGap=Math.max(maxGap,now-last);last=now;if(running)requestAnimationFrame(frame);};
      requestAnimationFrame(frame);
      const start = performance.now();
      const workerResult = await queue.submit(payload);
      const duration = performance.now()-start;
      running = false;
      await new Promise(resolve=>requestAnimationFrame(resolve));
      queue.dispose();
      // Compare only after the responsiveness measurement: this intentionally
      // runs the reference calculation synchronously on the browser thread.
      const expected = runtime.recipe_plan(payload);
      return {duration,frames,maxGap,equal:expected===workerResult,cards:JSON.parse(workerResult).cards.plans.length};
    });
    assert.equal(result.equal,true,'worker and direct computation must agree');
    assert.ok(result.cards>0,'synthetic market must produce plans');
    assert.ok(result.frames>=2,`browser must keep painting during search: ${JSON.stringify(result)}`);
    assert.ok(result.maxGap<250,`worker search must not freeze the browser: ${JSON.stringify(result)}`);
    assert.deepEqual(errors,[]);
    console.log(JSON.stringify(result));
  } finally {await browser?.close();await fixture?.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
