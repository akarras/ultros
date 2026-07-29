// Proves the #6831 residual mechanism: serve prod's REAL SSR page to prod's
// REAL wasm, but cut the HTML off mid-body (simulating the stalled/truncated
// SSR stream observed in hydrate-timing.cjs, where panicking loads hydrated
// with bodyKids=2). Everything else (pkg/, static/, api/) forwards to prod.
//
//   node truncation-proxy.mjs            # then load http://127.0.0.1:8799<path>
//   TRUNCATE=off node truncation-proxy.mjs   # control: serve the page intact
import http from 'node:http';

const UPSTREAM = 'https://ultros.app';
const PORT = Number(process.env.PORT || 8799);
const TRUNCATE = (process.env.TRUNCATE || 'on') === 'on';

const server = http.createServer(async (req, res) => {
  const url = UPSTREAM + req.url;
  let upstream;
  try {
    upstream = await fetch(url, {
      headers: {
        'user-agent': req.headers['user-agent'] || 'Mozilla/5.0',
        accept: req.headers.accept || '*/*',
        'accept-encoding': 'identity',
        // MUST forward cookies: the SSR render is cookie-dependent (HIDE_ADS,
        // theme, home world, locale). Dropping them makes prod render a page
        // the client would never produce, which is itself a hydration mismatch
        // and silently confounds the experiment.
        ...(req.headers.cookie ? { cookie: req.headers.cookie } : {}),
      },
      redirect: 'follow',
    });
  } catch (e) {
    res.writeHead(502).end(String(e));
    return;
  }

  const ct = upstream.headers.get('content-type') || 'application/octet-stream';
  const headers = { 'content-type': ct, 'cache-control': 'no-store' };
  // Strip the policies that would block a localhost-origin copy of the page.
  // (content-security-policy / x-frame-options are simply not copied.)

  const isDoc = ct.includes('text/html');
  if (!isDoc) {
    const buf = Buffer.from(await upstream.arrayBuffer());
    res.writeHead(upstream.status, headers).end(buf);
    return;
  }

  let html = await upstream.text();
  if (TRUNCATE) {
    // Cut immediately after the 2nd top-level <div> under <body> closes —
    // exactly the shape the panicking prod loads showed (bodyKids=2, lastKid=DIV).
    const bodyStart = html.indexOf('<body');
    const bodyOpenEnd = html.indexOf('>', bodyStart) + 1;
    let i = bodyOpenEnd;
    let depth = 0;
    let closedTopLevel = 0;
    const tagRe = /<(\/?)(\w+)[^>]*?(\/?)>/g;
    tagRe.lastIndex = bodyOpenEnd;
    let m;
    const VOID = new Set(['br', 'img', 'input', 'meta', 'link', 'hr', 'source', 'path', 'circle', 'use']);
    while ((m = tagRe.exec(html))) {
      const [full, slash, name, selfClose] = m;
      if (VOID.has(name.toLowerCase()) || selfClose) continue;
      if (slash) {
        depth--;
        if (depth === 0) {
          closedTopLevel++;
          i = m.index + full.length;
          if (closedTopLevel === 2) break;
        }
      } else {
        depth++;
      }
    }
    html = html.slice(0, i); // abrupt end: no </body>, no </html>, no resource scripts
  }
  headers['content-type'] = 'text/html; charset=utf-8';
  res.writeHead(200, headers).end(html);
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`truncation-proxy on http://127.0.0.1:${PORT} TRUNCATE=${TRUNCATE ? 'on' : 'off'}`);
});
