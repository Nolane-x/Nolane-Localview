import fs from 'node:fs';
import http from 'node:http';
import { chromium } from 'playwright';
import { WebSocketServer } from 'ws';

const bootstrap = fs.readFileSync('/tmp/localview-hmr-bootstrap.js', 'utf8');

const server = http.createServer((req, res) => {
  res.writeHead(200, {
    'content-type': 'text/html; charset=utf-8',
    'cache-control': 'no-store',
  });
  res.end('<!doctype html><html><body><main>LocalView HMR proof</main></body></html>');
});

const wss = new WebSocketServer({ noServer: true });
server.on('upgrade', (request, socket, head) => {
  wss.handleUpgrade(request, socket, head, (ws) => {
    wss.emit('connection', ws, request);
  });
});

const connections = [];
wss.on('connection', (ws, request) => {
  const protocol = request.headers['sec-websocket-protocol'] || '';
  const url = new URL(request.url || '/', 'http://127.0.0.1');
  connections.push({ ws, protocol, pathname: url.pathname });
});

await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const address = server.address();
if (!address || typeof address === 'string') throw new Error('fixture address unavailable');
const httpOrigin = `http://127.0.0.1:${address.port}`;
const wsOrigin = `ws://127.0.0.1:${address.port}`;

const invariant = (condition, message, detail) => {
  if (!condition) {
    throw new Error(`${message}${detail === undefined ? '' : ` :: ${JSON.stringify(detail)}`}`);
  }
};

const waitForConnection = async (predicate, timeoutMs = 2000) => {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const found = connections.find(predicate);
    if (found) return found;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error('timed out waiting for websocket fixture connection');
};

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
await page.addInitScript({ content: bootstrap });

try {
  await page.goto(httpOrigin, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => Boolean(window.__LOCALVIEW__?.peek));

  const token = 'localview-hmr-token-must-not-be-retained';
  await page.evaluate(({ wsOrigin, token }) => {
    window.__viteProof = new WebSocket(
      `${wsOrigin}/hmr?token=${encodeURIComponent(token)}`,
      'vite-hmr',
    );
    return new Promise((resolve, reject) => {
      window.__viteProof.addEventListener('open', resolve, { once: true });
      window.__viteProof.addEventListener('error', reject, { once: true });
    });
  }, { wsOrigin, token });

  const vite = await waitForConnection((entry) => String(entry.protocol).includes('vite-hmr'));
  vite.ws.send(JSON.stringify({
    type: 'update',
    updates: [{
      type: 'js-update',
      path: '/src/private/SecretPanel.tsx',
      acceptedPath: '/src/private/SecretPanel.tsx',
      timestamp: Date.now(),
    }],
  }));

  await page.waitForFunction(() =>
    window.__LOCALVIEW__.peek(128).some((event) => event.type === 'hmr')
  );

  let hmrEvents = await page.evaluate(() =>
    window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'hmr')
  );

  invariant(hmrEvents.length === 1, 'expected exactly one Vite HMR event', hmrEvents);
  invariant(hmrEvents[0].framework === 'vite', 'expected Vite framework', hmrEvents[0]);
  invariant(hmrEvents[0].phase === 'update', 'expected update phase', hmrEvents[0]);
  invariant(hmrEvents[0].updateCount === 1, 'expected bounded update count', hmrEvents[0]);

  const serialized = JSON.stringify(hmrEvents);
  invariant(!serialized.includes('SecretPanel'), 'module path leaked into HMR observation', serialized);
  invariant(!serialized.includes(token), 'WebSocket token leaked into HMR observation', serialized);
  invariant(!serialized.includes('acceptedPath'), 'raw HMR payload leaked into observation', serialized);

  await page.evaluate((wsOrigin) => {
    window.__genericProof = new WebSocket(`${wsOrigin}/ws`);
    return new Promise((resolve, reject) => {
      window.__genericProof.addEventListener('open', resolve, { once: true });
      window.__genericProof.addEventListener('error', reject, { once: true });
    });
  }, wsOrigin);

  const generic = await waitForConnection(
    (entry) => entry.pathname === '/ws' && !String(entry.protocol).includes('vite-hmr')
  );
  generic.ws.send(JSON.stringify({ type: 'update', secret: 'generic-app-message' }));
  await page.waitForTimeout(80);

  hmrEvents = await page.evaluate(() =>
    window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'hmr')
  );
  invariant(hmrEvents.length === 1, 'generic application socket must not emit HMR telemetry', hmrEvents);

  vite.ws.send('{broken-json');
  vite.ws.send('x'.repeat(256 * 1024 + 1));
  await page.waitForTimeout(80);
  hmrEvents = await page.evaluate(() =>
    window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'hmr')
  );
  invariant(hmrEvents.length === 1, 'malformed/oversized HMR frames must be ignored', hmrEvents);

  vite.ws.send(JSON.stringify({ type: 'full-reload', path: '/src/private/full.tsx' }));
  await page.waitForFunction(() =>
    window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'hmr').length === 2
  );
  hmrEvents = await page.evaluate(() =>
    window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'hmr')
  );
  invariant(hmrEvents[1].phase === 'full_reload', 'full reload phase mismatch', hmrEvents[1]);
  invariant(!JSON.stringify(hmrEvents).includes('/src/private/'), 'full-reload path leaked', hmrEvents);

  const socketBehavior = await page.evaluate(() => ({
    viteReadyState: window.__viteProof.readyState,
    genericReadyState: window.__genericProof.readyState,
    webSocketType: typeof WebSocket,
    viteIsNativeInstance: window.__viteProof instanceof WebSocket,
  }));
  invariant(socketBehavior.webSocketType === 'function', 'WebSocket constructor was broken', socketBehavior);
  invariant(socketBehavior.viteIsNativeInstance === true, 'observed socket lost WebSocket identity', socketBehavior);

  process.stdout.write(JSON.stringify({
    ok: true,
    hmrEvents: hmrEvents.map(({ framework, phase, updateCount = null }) => ({
      framework,
      phase,
      updateCount,
    })),
    genericSocketIgnored: true,
    rawPayloadPrivate: true,
  }) + '\n');
} finally {
  for (const entry of connections) {
    try { entry.ws.close(); } catch (_) {}
  }
  await browser.close();
  await new Promise((resolve) => wss.close(resolve));
  await new Promise((resolve) => server.close(resolve));
}
