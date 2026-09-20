import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { chromium } from 'playwright';

const bootstrap = await fs.readFile('/tmp/localview-point-select-bootstrap.js', 'utf8');
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1000, height: 720 } });

await page.route('http://127.0.0.1:41755/**', async (route) => {
  await route.fulfill({
    status: 200,
    contentType: 'text/html',
    body: `<!doctype html>
<html>
<head><meta charset="utf-8"><style>
  body { margin: 0; padding: 40px; font-family: sans-serif; }
  #action { width: 240px; height: 80px; }
  #nested { display: inline-block; width: 140px; height: 36px; }
  #secret { display: block; margin-top: 40px; width: 220px; height: 32px; }
  #pointer-pass { position:absolute; left:40px; top:40px; width:240px; height:80px; pointer-events:none; }
</style></head>
<body>
  <button id="action"><span id="nested">Nested target</span></button>
  <div id="pointer-pass"></div>
  <input id="secret" type="password" value="super-private-value" placeholder="Password">
  <svg id="svg" width="120" height="80" style="display:block;margin-top:30px"><circle id="circle" cx="40" cy="40" r="24"></circle></svg>
</body>
</html>`,
  });
});

await page.goto('http://127.0.0.1:41755/fixture');
await page.addScriptTag({ content: bootstrap });
await page.evaluate(() => {
  window.__proofClicks = 0;
  window.__proofKeys = 0;
  document.querySelector('#action').addEventListener('click', () => { window.__proofClicks += 1; });
  document.addEventListener('keydown', () => { window.__proofKeys += 1; });
});

const currentRoute = () => page.evaluate(() => {
  const route = new URL(location.href);
  route.search = '';
  route.hash = '';
  return route.toString();
});

async function begin(token) {
  const route = await currentRoute();
  const armed = await page.evaluate(({ token, route }) => window.__LOCALVIEW__.beginPointSelect({
    requestToken: token,
    route,
  }), { token, route });
  assert.equal(armed, true, `${token} should arm point selection`);
}

async function completions() {
  return page.evaluate(() => window.__LOCALVIEW__.takePointSelectCompletions(4));
}

async function center(selector) {
  const box = await page.locator(selector).boundingBox();
  assert.ok(box, `${selector} should have a box`);
  return { x: box.x + box.width / 2, y: box.y + box.height / 2, box };
}

// Hover highlight is bounded, LocalView-owned and absent from semantic content.
await begin('point-hover');
const nested = await center('#nested');
await page.mouse.move(nested.x, nested.y);
const overlay = page.locator('[data-localview-owned="point-select"]');
await overlay.waitFor({ state: 'attached' });
const overlayProof = await overlay.evaluate((node) => {
  const rect = node.getBoundingClientRect();
  return {
    display: getComputedStyle(node).display,
    visibility: getComputedStyle(node).visibility,
    pointerEvents: getComputedStyle(node).pointerEvents,
    rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
  };
});
assert.equal(overlayProof.display, 'block');
assert.equal(overlayProof.visibility, 'visible');
assert.equal(overlayProof.pointerEvents, 'none');
assert.ok(overlayProof.rect.width > 0 && overlayProof.rect.height > 0);
assert.ok(overlayProof.rect.x >= 0 && overlayProof.rect.y >= 0);
assert.ok(overlayProof.rect.x + overlayProof.rect.width <= 1000);
assert.ok(overlayProof.rect.y + overlayProof.rect.height <= 720);

const semanticOwnsOverlay = await page.evaluate(() => {
  const overlayNode = document.querySelector('[data-localview-owned="point-select"]');
  const overlayRef = window.__LOCALVIEW__.refFor(overlayNode);
  const snapshot = window.__LOCALVIEW__.snapshot();
  const refs = [];
  const visit = (node) => {
    if (!node) return;
    refs.push(node.ref);
    for (const child of node.children || []) visit(child);
  };
  visit(snapshot.semantic_tree);
  return refs.includes(overlayRef);
});
assert.equal(semanticOwnsOverlay, false, 'LocalView overlay must not become semantic app content');

// Visual capture freeze hides the transient overlay, then restores it.
await page.evaluate(async () => {
  await window.__LOCALVIEW__.freezeVisuals('point-proof-freeze', 8000);
});
await page.waitForTimeout(20);
assert.equal(await overlay.evaluate((node) => getComputedStyle(node).visibility), 'hidden');
await page.evaluate(() => window.__LOCALVIEW__.restoreVisuals('point-proof-freeze'));
await page.waitForTimeout(20);
assert.equal(await overlay.evaluate((node) => getComputedStyle(node).visibility), 'visible');

// Exact nested target wins; app click is suppressed; stable ref comes from instrumentation.
const nestedRef = await page.evaluate(() => window.__LOCALVIEW__.refFor(document.querySelector('#nested')));
await page.mouse.click(nested.x, nested.y);
let receipt = (await completions()).find((entry) => entry.requestToken === 'point-hover');
assert.ok(receipt, 'selected receipt should exist');
assert.equal(receipt.status, 'selected');
assert.equal(receipt.reference, nestedRef);
assert.match(receipt.reference, /^@e[0-9a-f]+$/i);
assert.equal(await page.evaluate(() => window.__proofClicks), 0, 'selection click must be suppressed');
assert.equal(await page.locator('[data-localview-owned="point-select"]').count(), 0, 'one-shot overlay cleanup');

// Listener cleanup: after completion an ordinary app click works.
await page.mouse.click(nested.x, nested.y);
assert.equal(await page.evaluate(() => window.__proofClicks), 1, 'listeners must not survive completion');

// Escape cancels only while mode is active and cleans up.
await begin('point-escape');
await page.keyboard.press('Escape');
receipt = (await completions()).find((entry) => entry.requestToken === 'point-escape');
assert.ok(receipt);
assert.equal(receipt.status, 'cancelled');
assert.equal(receipt.reason, 'escape');
assert.equal(await page.locator('[data-localview-owned="point-select"]').count(), 0);

// Route drift is terminal and cannot retain the overlay.
await begin('point-route');
await page.evaluate(() => history.pushState({}, '', '/route-b'));
await page.waitForTimeout(0);
receipt = (await completions()).find((entry) => entry.requestToken === 'point-route');
assert.ok(receipt);
assert.equal(receipt.status, 'failed');
assert.equal(receipt.reason, 'route_changed');
assert.equal(await page.locator('[data-localview-owned="point-select"]').count(), 0);

// Removed hover target never yields the removed target's stale reference.
await begin('point-removed');
await page.evaluate(() => {
  const target = document.createElement('button');
  target.id = 'removed-target';
  target.textContent = 'temporary';
  target.style.cssText = 'position:absolute;left:420px;top:80px;width:160px;height:50px';
  document.body.appendChild(target);
});
const removed = await center('#removed-target');
await page.mouse.move(removed.x, removed.y);
const removedRef = await page.evaluate(() => window.__LOCALVIEW__.refFor(document.querySelector('#removed-target')));
await page.evaluate(() => document.querySelector('#removed-target').remove());
await page.mouse.click(removed.x, removed.y);
receipt = (await completions()).find((entry) => entry.requestToken === 'point-removed');
assert.ok(receipt);
assert.notEqual(receipt.reference, removedRef, 'removed target reference must never be selected');
assert.equal(receipt.status, 'failed');

// Sensitive input selection transports no input value or text payload.
await begin('point-private');
const secret = await center('#secret');
await page.mouse.move(secret.x, secret.y);
await page.mouse.click(secret.x, secret.y);
receipt = (await completions()).find((entry) => entry.requestToken === 'point-private');
assert.ok(receipt);
assert.equal(receipt.status, 'selected');
assert.match(receipt.reference, /^@e[0-9a-f]+$/i);
const privateReceiptJson = JSON.stringify(receipt);
assert.equal(privateReceiptJson.includes('super-private-value'), false);
assert.equal(Object.hasOwn(receipt, 'value'), false);
assert.equal(Object.hasOwn(receipt, 'textContent'), false);
assert.equal(Object.hasOwn(receipt, 'innerHTML'), false);

// SVG hit testing resolves the real SVG element without a DOM path heuristic.
await begin('point-svg');
const circle = await center('#circle');
await page.mouse.move(circle.x, circle.y);
const circleRef = await page.evaluate(() => window.__LOCALVIEW__.refFor(document.querySelector('#circle')));
await page.mouse.click(circle.x, circle.y);
receipt = (await completions()).find((entry) => entry.requestToken === 'point-svg');
assert.ok(receipt);
assert.equal(receipt.status, 'selected');
assert.equal(receipt.reference, circleRef);

// Outside selection mode Escape and clicks are no longer globally intercepted.
const keysBefore = await page.evaluate(() => window.__proofKeys);
await page.keyboard.press('Escape');
assert.equal(await page.evaluate(() => window.__proofKeys), keysBefore + 1);

console.log(JSON.stringify({
  ok: true,
  proofs: [
    'hover-highlight',
    'semantic-overlay-exclusion',
    'capture-overlay-suspension',
    'stable-ref-click',
    'click-suppression',
    'one-shot-cleanup',
    'escape-cancel',
    'route-drift',
    'removed-target',
    'private-input-no-value',
    'svg-hit-test',
  ],
}));

await browser.close();
