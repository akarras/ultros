'use strict';
// Local QA instrumentation only; request headers, bodies and query strings are never logged.
const path = require('node:path');
const repoRoot = path.resolve(__dirname, '../..');
const puppeteerEntry = require.resolve(path.join(repoRoot, 'integration/node_modules/puppeteer'));
// Puppeteer's current CommonJS import is an immutable ESM namespace. Replace
// the CommonJS cache facade rather than assigning to that namespace silently.
const puppeteer = {...require(puppeteerEntry)};
const originalLaunch = puppeteer.launch.bind(puppeteer);
const localOrigin = new URL(process.env.BASE_URL || 'http://localhost:55438').origin;
const shouldBlock = process.env.QA_ISOLATE_EXTERNAL === '1';
puppeteer.launch = async function (...args) {
  console.log('QA_BROWSER_TRACE ' + JSON.stringify({isolateExternal:shouldBlock}));
  const browser = await originalLaunch(...args);
  const originalNewPage = browser.newPage.bind(browser);
  const instrumentedPages = new WeakSet();
  const instrumentedContexts = new WeakSet();
  const instrumentPage = async page => {
    if(instrumentedPages.has(page))return page;
    instrumentedPages.add(page);
    const isolatedResource = url => ['www.googletagmanager.com','www.google-analytics.com'].includes(url.hostname)
      || (url.hostname==='cdn.discordapp.com' && /^\/embed\/avatars\/\d+\.png$/.test(url.pathname));
    page.on('requestfailed', request => {
      const url = new URL(request.url());
      if (!/^https?:$/.test(url.protocol)) return;
      console.log('QA_FAILED_REQUEST ' + JSON.stringify({origin:url.origin,path:url.pathname,
        type:request.resourceType(),error:request.failure()?.errorText,firstParty:url.origin===localOrigin}));
    });
    if (shouldBlock) {
      // CDP blocking avoids changing any fixture's request-interception handler.
      // Limit this to analytics and the public default-avatar path observed in
      // the bounded DNS probes and context-aware sync trace.
      const session = await page.createCDPSession();
      await session.send('Network.enable');
      await session.send('Network.setBlockedURLs', {urls:[
        '*://www.googletagmanager.com/*', '*://www.google-analytics.com/*',
        '*://cdn.discordapp.com/embed/avatars/*'
      ]});
      console.log('QA_EXTERNAL_BLOCK_POLICY ' + JSON.stringify({analyticsHosts:['www.googletagmanager.com','www.google-analytics.com'],defaultAvatars:'https://cdn.discordapp.com/embed/avatars/*'}));
      // Some probes assert on every console error, including intentional browser
      // blocks. Exclude only errors whose console location names an observed
      // isolated resource; unknown locations and every first-party error pass
      // through to the original strict assertions.
      const originalOn = page.on.bind(page), originalOff = page.off.bind(page);
      const consoleListeners = new WeakMap();
      page.on = function(event, listener) {
        if(event !== 'console') return originalOn(event, listener);
        const wrapped = message => {
          let url;
          try { url = new URL(message.location().url); } catch {}
          if(message.type()==='error' && url && isolatedResource(url)) {
            console.log('QA_EXTERNAL_CONSOLE ' + JSON.stringify({origin:url.origin,path:url.pathname,error:message.text()}));
            return;
          }
          return listener(message);
        };
        consoleListeners.set(listener, wrapped);
        return originalOn(event, wrapped);
      };
      page.off = function(event, listener) {
        return originalOff(event, event==='console' ? consoleListeners.get(listener)||listener : listener);
      };
    }
    return page;
  };
  const instrumentContext = context => {
    if(instrumentedContexts.has(context))return context;
    instrumentedContexts.add(context);
    const newPage = context.newPage.bind(context);
    context.newPage = async (...pageArgs) => instrumentPage(await newPage(...pageArgs));
    return context;
  };
  instrumentContext(browser.defaultBrowserContext());
  const createContext = browser.createBrowserContext.bind(browser);
  browser.createBrowserContext = async (...contextArgs) => instrumentContext(await createContext(...contextArgs));
  browser.newPage = async (...pageArgs) => instrumentPage(await originalNewPage(...pageArgs));
  return browser;
};
require.cache[puppeteerEntry].exports = puppeteer;
if(require(puppeteerEntry).launch !== puppeteer.launch)throw new Error('QA browser instrumentation was not installed.');
