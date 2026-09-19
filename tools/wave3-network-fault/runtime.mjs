import http from 'node:http';
import fs from 'node:fs';
import { chromium } from 'playwright';

const bootstrap = fs.readFileSync('/tmp/localview-network-fault-bootstrap.js', 'utf8');
const hits = new Map();
const server = http.createServer((req, res) => {
  const url = new URL(req.url || '/', 'http://127.0.0.1');
  hits.set(url.pathname, (hits.get(url.pathname) || 0) + 1);
  res.writeHead(200, { 'content-type': 'text/plain', 'cache-control': 'no-store' });
  res.end(`origin:${url.pathname}`);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const address = server.address();
if (!address || typeof address === 'string') throw new Error('fixture address unavailable');
const origin = `http://127.0.0.1:${address.port}`;

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
await page.addInitScript({ content: bootstrap });

let externalRouteHits = 0;
await page.route('http://nonloopback.test/**', async (route) => {
  externalRouteHits += 1;
  await route.fulfill({
    status: 200,
    contentType: 'text/plain',
    headers: {
      'access-control-allow-origin': '*',
      'cache-control': 'no-store',
    },
    body: 'external-http-pass-through',
  });
});

const invariant = (condition, message) => {
  if (!condition) throw new Error(message);
};

try {
  await page.goto(`${origin}/`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => Boolean(window.__LOCALVIEW__?.networkFaultState));

  const token = 'network_fault_token_01';
  const plan = {
    fingerprint: '0123456789abcdef',
    lease_ms: 1200,
    rules: [
      { id: 'fetch-fail', transport: 'fetch', method: 'get', path: '/api/fetch-fail', effect: { kind: 'fail' }, max_hits: 1 },
      { id: 'fetch-delay', transport: 'fetch', method: 'get', path: '/api/fetch-delay', effect: { kind: 'delay', milliseconds: 120 }, max_hits: 1 },
      { id: 'fetch-mock', transport: 'fetch', method: 'get', path: '/api/fetch-mock', effect: { kind: 'mock_status', status: 503 }, max_hits: 1 },
      { id: 'xhr-fail', transport: 'xhr', method: 'get', path: '/api/xhr-fail', effect: { kind: 'fail' }, max_hits: 1 },
      { id: 'xhr-delay', transport: 'xhr', method: 'get', path: '/api/xhr-delay', effect: { kind: 'delay', milliseconds: 100 }, max_hits: 1 },
      { id: 'xhr-mock', transport: 'xhr', method: 'get', path: '/api/xhr-mock', effect: { kind: 'mock_status', status: 418 }, max_hits: 1 },
      { id: 'exhaust', transport: 'both', method: 'get', path: '/api/exhaust', effect: { kind: 'fail' }, max_hits: 1 }
    ]
  };

  const install = await page.evaluate(({ token, plan }) => {
    const receipt = window.__LOCALVIEW__.installNetworkFaultPlan(token, plan);
    return { ...receipt, state: window.__LOCALVIEW__.networkFaultState() };
  }, { token, plan });
  invariant(install.installed === true, 'plan must install');
  invariant(install.state.active === true && install.state.rule_count === 7, 'installed state mismatch');

  const pass = await page.evaluate(async () => {
    const response = await fetch('/api/pass');
    return { status: response.status, text: await response.text() };
  });
  invariant(pass.status === 200 && pass.text === 'origin:/api/pass', 'unmatched fetch must pass through');

  const fetchFail = await page.evaluate(async () => {
    try {
      await fetch('/api/fetch-fail');
      return { failed: false };
    } catch (error) {
      return { failed: true, name: error?.name, message: String(error?.message || error) };
    }
  });
  invariant(fetchFail.failed && fetchFail.name === 'TypeError', 'fetch fail must reject like a network error');
  invariant((hits.get('/api/fetch-fail') || 0) === 0, 'fetch fail must not reach origin');

  const fetchDelay = await page.evaluate(async () => {
    const start = performance.now();
    const response = await fetch('/api/fetch-delay');
    return { status: response.status, elapsed: performance.now() - start };
  });
  invariant(fetchDelay.status === 200 && fetchDelay.elapsed >= 90, 'fetch delay must delay then reach origin');

  const fetchMock = await page.evaluate(async () => {
    const response = await fetch('/api/fetch-mock');
    return { status: response.status, text: await response.text() };
  });
  invariant(fetchMock.status === 503 && fetchMock.text === '', 'fetch mock must synthesize empty status response');
  invariant((hits.get('/api/fetch-mock') || 0) === 0, 'fetch mock must not reach origin');

  const xhr = async (path) => page.evaluate((path) => new Promise((resolve) => {
    const request = new XMLHttpRequest();
    const start = performance.now();
    request.open('GET', path);
    const finish = (event) => resolve({
      event,
      status: request.status,
      text: request.responseText,
      elapsed: performance.now() - start,
    });
    request.addEventListener('load', () => finish('load'), { once: true });
    request.addEventListener('error', () => finish('error'), { once: true });
    request.send();
  }), path);

  const xhrFail = await xhr('/api/xhr-fail');
  invariant(xhrFail.event === 'error' && xhrFail.status === 0, 'XHR fail must emit network-style error');
  invariant((hits.get('/api/xhr-fail') || 0) === 0, 'XHR fail must not reach origin');

  const xhrDelay = await xhr('/api/xhr-delay');
  invariant(xhrDelay.event === 'load' && xhrDelay.status === 200 && xhrDelay.elapsed >= 75, 'XHR delay must delay real send');

  const xhrMock = await xhr('/api/xhr-mock');
  invariant(xhrMock.event === 'load' && xhrMock.status === 418 && xhrMock.text === '', 'XHR mock must synthesize status');
  invariant((hits.get('/api/xhr-mock') || 0) === 0, 'XHR mock must not reach origin');

  const exhausted = await page.evaluate(async () => {
    let firstFailed = false;
    try { await fetch('/api/exhaust'); } catch (_) { firstFailed = true; }
    const second = await fetch('/api/exhaust');
    return { firstFailed, secondStatus: second.status, secondText: await second.text() };
  });
  invariant(exhausted.firstFailed, 'first hit must consume bounded fault rule');
  invariant(exhausted.secondStatus === 200 && exhausted.secondText === 'origin:/api/exhaust', 'exhausted rule must pass through');
  invariant((hits.get('/api/exhaust') || 0) === 1, 'only post-exhaustion request must reach origin');

  const externalBypass = await page.evaluate(async () => {
    const response = await fetch('http://nonloopback.test/api/external');
    return { status: response.status, text: await response.text() };
  });
  invariant(
    externalBypass.status === 200 && externalBypass.text === 'external-http-pass-through',
    'HTTP non-loopback request must bypass fault selection',
  );
  invariant(externalRouteHits === 1, 'HTTP non-loopback request must reach the underlying browser request path');

  const proof = await page.evaluate(() => ({
    events: window.__LOCALVIEW__.peek(128).filter((event) => event.type === 'network'),
    state: window.__LOCALVIEW__.networkFaultState(),
    inflight: window.__LOCALVIEW__.snapshot().readiness.inflightRequests,
  }));
  invariant(proof.state.total_hits === 7, `expected 7 consumed hits, got ${proof.state.total_hits}`);
  invariant(proof.inflight === 0, `in-flight accounting leaked: ${proof.inflight}`);
  for (const id of ['fetch-fail', 'fetch-delay', 'fetch-mock', 'xhr-fail', 'xhr-delay', 'xhr-mock', 'exhaust']) {
    invariant(proof.events.some((event) => event.faultInjected === true && event.faultRuleId === id), `missing fault metadata for ${id}`);
  }
  const injected = Object.fromEntries(
    proof.events
      .filter((event) => event.faultInjected === true && event.faultRuleId)
      .map((event) => [event.faultRuleId, event]),
  );
  invariant(injected['fetch-delay']?.faultDelayMs === 120, 'fetch delay metadata must retain configured milliseconds');
  invariant(injected['xhr-delay']?.faultDelayMs === 100, 'XHR delay metadata must retain configured milliseconds');
  invariant(injected['fetch-mock']?.faultStatus === 503, 'fetch mock metadata must retain configured status');
  invariant(injected['xhr-mock']?.faultStatus === 418, 'XHR mock metadata must retain configured status');

  await page.waitForTimeout(1250);
  const expired = await page.evaluate(() => window.__LOCALVIEW__.networkFaultState());
  invariant(expired.active === false, 'lease must expire without caller cleanup');

  const clearProof = await page.evaluate(() => {
    const token = 'network_fault_token_02';
    const receipt = window.__LOCALVIEW__.installNetworkFaultPlan(token, {
      fingerprint: 'fedcba9876543210',
      lease_ms: 5000,
      rules: [{ id: 'clear-proof', transport: 'fetch', method: 'get', path: '/api/clear-proof', effect: { kind: 'fail' }, max_hits: 1 }]
    });
    const before = window.__LOCALVIEW__.networkFaultState();
    const cleared = window.__LOCALVIEW__.clearNetworkFaultPlan(token);
    const after = window.__LOCALVIEW__.networkFaultState();
    return { receipt, before, cleared, after };
  });
  invariant(clearProof.before.active === true, 'second lease must install');
  invariant(clearProof.cleared.cleared === true && clearProof.after.active === false, 'clear must remove exact lease');

  const restored = await page.evaluate(async () => {
    const response = await fetch('/api/clear-proof');
    return { status: response.status, text: await response.text(), inflight: window.__LOCALVIEW__.snapshot().readiness.inflightRequests };
  });
  invariant(restored.status === 200 && restored.text === 'origin:/api/clear-proof', 'clear must restore pass-through');
  invariant(restored.inflight === 0, 'clear proof must leave no in-flight debt');

  process.stdout.write(JSON.stringify({
    ok: true,
    checks: 29,
    networkEvents: proof.events.length,
    finalInflight: restored.inflight,
    externalHttpPassThrough: externalRouteHits,
  }) + '\n');
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
