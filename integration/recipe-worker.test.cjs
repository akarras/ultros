const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const load = () => import(pathToFileURL(path.resolve(__dirname, '../ultros/static/recipe-worker-client.mjs')));

function fixture(PlannerQueue, deadlineMs = 1000) {
  const workers = [];
  const queue = new PlannerQueue({
    assets: () => ({ js: '/pkg/build/ultros.js', wasm: '/pkg/build/ultros.wasm' }),
    deadlineMs,
    spawn: () => {
      const worker = { messages: [], terminated: false,
        postMessage(message) { this.messages.push(message); },
        terminate() { this.terminated = true; },
        answer(result) { this.onmessage({data:{id:this.messages.at(-1).id,result}}); },
      };
      workers.push(worker);
      return worker;
    },
  });
  return { queue, workers };
}

test('running work stays bounded and only the latest queued input is published', async () => {
  const { PlannerQueue } = await load();
  const { queue, workers } = fixture(PlannerQueue);
  const first = queue.submit('first');
  const firstRejected = assert.rejects(first, /superseded/);
  const second = queue.submit('second');
  const secondRejected = assert.rejects(second, /superseded/);
  const latest = queue.submit('latest');
  assert.equal(workers.length, 1);
  assert.equal(workers[0].messages.length, 1);
  assert.equal(queue.pending.payload, 'latest');
  workers[0].answer('stale result');
  assert.equal(workers[0].messages.length, 2);
  assert.equal(workers[0].messages[1].payload, 'latest');
  workers[0].answer('current result');
  assert.equal(await latest, 'current result');
  await Promise.all([firstRejected, secondRejected]);
  queue.dispose();
});

test('worker failures, deadlines, and disposal reject work and allow retry', async () => {
  const { PlannerQueue } = await load();
  const { queue, workers } = fixture(PlannerQueue, 10);
  const failed = queue.submit('failed');
  workers[0].onerror({preventDefault(){}});
  await assert.rejects(failed, /failed/);
  assert.equal(workers[0].terminated, true);
  const timed = queue.submit('stalled');
  await assert.rejects(timed, /timed out/);
  assert.equal(workers[1].terminated, true);
  const retry = queue.submit('retry');
  workers[2].answer('ok');
  assert.equal(await retry, 'ok');
  const active = queue.submit('active');
  const activeRejected = assert.rejects(active, /superseded/);
  const pending = queue.submit('pending');
  queue.dispose();
  await activeRejected;
  await assert.rejects(pending, /disposed/);
  assert.equal(workers[2].terminated, true);
  await assert.rejects(queue.submit('later'), /disposed/);
});

test('shares compiled runtime and falls back when a browser cannot clone modules', async () => {
  const { PlannerQueue } = await load();
  const compiled = { sentinel: 'compiled module' };
  const sent = [];
  const worker = { terminate(){}, postMessage(message) {
    sent.push(message);
    if (message.compiled) throw Object.assign(new Error('not cloneable'), {name:'DataCloneError'});
  }};
  const queue = new PlannerQueue({compiled,assets:()=>({js:'/pkg/ultros.js',wasm:'/pkg/ultros.wasm'}),spawn:()=>worker});
  const result = queue.submit('plan');
  assert.equal(sent.length,2);
  assert.equal(sent[0].compiled,compiled);
  assert.equal(sent[1].compiled,undefined);
  worker.onmessage({data:{id:sent[1].id,result:'ok'}});
  assert.equal(await result,'ok');
  queue.dispose();
});
