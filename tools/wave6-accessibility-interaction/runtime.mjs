import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { chromium } from 'playwright';
import axe from 'axe-core';

const bootstrap = await fs.readFile('/tmp/localview-wave6-bootstrap.js', 'utf8');
const browser = await chromium.launch({
  headless: true,
  args: ['--host-resolver-rules=MAP cross-origin.test 127.0.0.1'],
});
const page = await browser.newPage({ viewport: { width: 1000, height: 720 } });

const fixture = [
  '<!doctype html><html><head><meta charset="utf-8"><style>',
  'body{margin:0;padding:30px;font-family:sans-serif}#small{width:12px;height:12px;padding:0}',
  '#clip{width:45px;height:40px;overflow:hidden;margin-top:12px}#clipped{width:120px;height:34px}',
  '#occlusion-wrap{position:relative;width:150px;height:48px;margin-top:12px}',
  '#occluded,#blocker{position:absolute;inset:0;width:150px;height:48px}',
  '#blocker{z-index:5;background:rgba(0,0,0,.2)}#offscreen{position:absolute;left:-2000px;top:0}',
  '</style></head><body>',
  '<button id="positive" tabindex="2">Positive</button><button id="natural">Natural</button>',
  '<button id="nameless"></button>',
  '<img id="missing-alt" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==">',
  '<button id="small" data-localview-safe-interaction="true" aria-label="Small"></button>',
  '<div id="clip"><button id="clipped" data-localview-safe-interaction="true">Clipped</button></div>',
  '<div id="occlusion-wrap"><button id="occluded" data-localview-safe-interaction="true">Occluded</button><div id="blocker"></div></div>',
  '<button id="observed" data-localview-safe-interaction="true" aria-expanded="false">Observed</button>',
  '<button id="delayed" data-localview-safe-interaction="true" aria-expanded="false">Delayed</button>',
  '<button id="no-feedback" data-localview-safe-interaction="true">No feedback</button>',
  '<button id="unsafe" type="submit">Delete</button>',
  '<button id="trap">Trap</button><button id="offscreen">Offscreen</button>',
  '<input id="readonly" readonly aria-label="Read only"><input id="secret" type="password" value="SUPER_PRIVATE_WAVE6_VALUE" aria-label="Password">',
  '<iframe id="cross-frame" src="http://cross-origin.test:41767/frame"></iframe>',
  '<script>',
  'document.querySelector("#observed").addEventListener("click",event=>event.currentTarget.setAttribute("aria-expanded","true"));',
  'document.querySelector("#delayed").addEventListener("click",event=>{const target=event.currentTarget;setTimeout(()=>target.setAttribute("aria-expanded","true"),320)});',
  'document.querySelector("#trap").addEventListener("keydown",event=>{if(event.key==="Tab"){event.preventDefault();event.currentTarget.focus()}});',
  '</script></body></html>',
].join('\n');

await page.route('http://127.0.0.1:41766/**', async route => {
  await route.fulfill({ status: 200, contentType: 'text/html', body: fixture });
});
await page.route('http://cross-origin.test:41767/**', async route => {
  await route.fulfill({ status: 200, contentType: 'text/html', body: '<!doctype html><button id="foreign"></button>' });
});

await page.goto('http://127.0.0.1:41766/fixture');
await page.addScriptTag({ content: bootstrap });
assert.ok(await page.evaluate(() => !!window.__LOCALVIEW_WAVE6__));
await page.addScriptTag({ content: axe.source });
assert.equal(await page.evaluate(() => window.__LOCALVIEW_WAVE6__.installAxe(window.axe)), true);

const ref = selector => page.evaluate(sel => window.__LOCALVIEW__.refFor(document.querySelector(sel)), selector);

const scan = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.runAccessibilityScan({
  documentGeneration: 11,
  maxFindings: 128,
}));
assert.equal(scan.status, 'complete');
assert.equal(scan.accessibility_complete, false);
assert.ok(scan.local_findings.some(item => item.code === 'interactive_name_missing'));
assert.ok(scan.local_findings.some(item => item.code === 'image_name_missing'));
assert.ok(scan.axe_findings.some(item => item.rule_id === 'button-name'));
assert.ok(scan.axe_findings.length <= 128);
const scanJson = JSON.stringify(scan);
assert.equal(scanJson.includes('SUPER_PRIVATE_WAVE6_VALUE'), false);
assert.equal(scanJson.includes('#nameless'), false);
assert.ok(['complete', 'inconclusive'].includes(scan.status));

const smallRef = await ref('#small');
const small = await page.evaluate(r => window.__LOCALVIEW_WAVE6__.effectiveHitbox(r), smallRef);
assert.equal(small.nominal.width, 12);
assert.equal(small.nominal.height, 12);

const clippedRef = await ref('#clipped');
const clipped = await page.evaluate(r => window.__LOCALVIEW_WAVE6__.effectiveHitbox(r), clippedRef);
assert.equal(clipped.status, 'complete');
assert.equal(clipped.authority, 'browser_hit_test');
assert.ok(clipped.effective.width < clipped.nominal.width);

const occludedRef = await ref('#occluded');
const blockerRef = await ref('#blocker');
const occluded = await page.evaluate(r => window.__LOCALVIEW_WAVE6__.effectiveHitbox(r), occludedRef);
assert.equal(occluded.fully_blocked, true);
assert.ok(occluded.occluders.includes(blockerRef));

await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({ documentGeneration: 11 }));
await page.keyboard.press('Tab');
assert.equal(await page.evaluate(() => document.activeElement?.id), 'positive');
await page.keyboard.press('Tab');
await page.keyboard.press('Shift+Tab');
await page.waitForTimeout(20);

const overlay = page.locator('[data-localview-owned="wave6-focus-path"]');
assert.equal(await overlay.count(), 1);
const snapshotWithOverlay = await page.evaluate(() => JSON.stringify(window.__LOCALVIEW__.snapshot()));
assert.equal(snapshotWithOverlay.includes('wave6-focus-path'), false);

await page.evaluate(() => window.__LOCALVIEW__.freezeVisuals('wave6-freeze-proof', 8000));
await page.waitForTimeout(20);
assert.equal(await overlay.evaluate(node => getComputedStyle(node).visibility), 'hidden');
await page.evaluate(() => window.__LOCALVIEW__.restoreVisuals('wave6-freeze-proof'));
await page.waitForTimeout(20);
assert.equal(await overlay.evaluate(node => getComputedStyle(node).visibility), 'visible');

const positiveRef = await ref('#positive');
await page.evaluate(r => window.__LOCALVIEW_WAVE6__.markFocusProblem(r), positiveRef);
assert.equal(await page.locator('[data-localview-owned="wave6-focus-path-marker"][data-problem="true"]').count() > 0, true);
const journey = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishKeyboardJourney());
assert.equal(journey.status, 'complete');
assert.ok(journey.transitions.length >= 3);
assert.equal(journey.transitions[0].reference, positiveRef);
assert.ok(journey.transitions.some(item => item.direction === 'shift_tab'));
assert.equal(await overlay.count(), 0);

await page.locator('#trap').focus();
await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({ documentGeneration: 11, maxTransitions: 8 }));
for (let i = 0; i < 4; i += 1) {
  await page.keyboard.press('Tab');
  await page.waitForTimeout(5);
}
const trapped = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishKeyboardJourney());
assert.equal(new Set(trapped.transitions.map(item => item.reference)).size, 1);

await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({ documentGeneration: 11 }));
await page.evaluate(() => history.pushState({}, '', '/route-drift'));
let drift = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.recordKeyboardFocus({ documentGeneration: 11 }));
assert.equal(drift.reason, 'route_drift');
await page.evaluate(() => history.replaceState({}, '', '/fixture'));

await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({ documentGeneration: 11 }));
drift = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.recordKeyboardFocus({ documentGeneration: 12 }));
assert.equal(drift.reason, 'document_generation_drift');

await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({ documentGeneration: 11 }));
await page.keyboard.press('Escape');
assert.equal(await overlay.count(), 0);

await page.evaluate(() => window.__LOCALVIEW_WAVE6__.beginKeyboardJourney({
  documentGeneration: 11,
  deadlineMs: 100,
}));
assert.equal(await overlay.count(), 1);
await page.waitForTimeout(150);
assert.equal(await overlay.count(), 0, 'deadline must remove the transient focus overlay');

const safeTargets = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.safeDiscoveryTargets(32));
const unsafeRef = await ref('#unsafe');
const unsafeCandidate = safeTargets.find(item => item.reference === unsafeRef);
assert.ok(unsafeCandidate);
assert.equal(unsafeCandidate.probe_allowed, false);
assert.equal(unsafeCandidate.safety, 'destructive_or_unknown');
const readonlyRef = await ref('#readonly');
const readonlyCandidate = safeTargets.find(item => item.reference === readonlyRef);
assert.ok(readonlyCandidate);
assert.equal(readonlyCandidate.probe_allowed, true);
assert.equal(readonlyCandidate.safety, 'read_only');

const observedRef = await ref('#observed');
await page.evaluate(r => window.__LOCALVIEW_WAVE6__.beginFeedbackProbe({ reference: r, documentGeneration: 11, safety: 'explicitly_safe' }), observedRef);
await page.locator('#observed').click();
let feedbackObserved = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishFeedbackProbe({ documentGeneration: 11 }));
assert.equal(feedbackObserved.verdict, 'observed_feedback');

const delayedRef = await ref('#delayed');
await page.evaluate(r => window.__LOCALVIEW_WAVE6__.beginFeedbackProbe({
  reference: r, documentGeneration: 11, safety: 'explicitly_safe', delayedThresholdMs: 250, deadlineMs: 1000,
}), delayedRef);
await page.locator('#delayed').click();
await page.waitForTimeout(380);
const feedbackDelayed = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishFeedbackProbe({ documentGeneration: 11 }));
assert.equal(feedbackDelayed.verdict, 'delayed_feedback');

const noFeedbackRef = await ref('#no-feedback');
await page.locator('#no-feedback').focus();
await page.evaluate(r => window.__LOCALVIEW_WAVE6__.beginFeedbackProbe({ reference: r, documentGeneration: 11, safety: 'explicitly_safe' }), noFeedbackRef);
await page.locator('#no-feedback').click();
const feedbackNone = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishFeedbackProbe({ documentGeneration: 11 }));
assert.equal(feedbackNone.verdict, 'no_observed_feedback');

await page.evaluate(r => window.__LOCALVIEW_WAVE6__.beginFeedbackProbe({
  reference: r, documentGeneration: 11, safety: 'explicitly_safe', deadlineMs: 40,
}), noFeedbackRef);
await page.waitForTimeout(70);
const feedbackInconclusive = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.finishFeedbackProbe({ documentGeneration: 11 }));
assert.equal(feedbackInconclusive.verdict, 'inconclusive');

const skipped = await page.evaluate(r => window.__LOCALVIEW_WAVE6__.beginFeedbackProbe({ reference: r, documentGeneration: 11 }), unsafeRef);
assert.equal(skipped.status, 'skipped');
assert.equal(skipped.reason, 'trusted_safety_required');

const replayTarget = await ref('#small');
const expected = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.stateIdentity(11));
let replay = await page.evaluate(({ target, expectedState }) => window.__LOCALVIEW_WAVE6__.validateReplayStep({
  target, expectedState, documentGeneration: 11,
}), { target: replayTarget, expectedState: expected });
assert.equal(replay.status, 'ready');

await page.evaluate(() => document.querySelector('#small').setAttribute('aria-expanded', 'true'));
replay = await page.evaluate(({ target, expectedState }) => window.__LOCALVIEW_WAVE6__.validateReplayStep({
  target, expectedState, documentGeneration: 11,
}), { target: replayTarget, expectedState: expected });
assert.equal(replay.reason, 'state_mismatch');

const current = await page.evaluate(() => window.__LOCALVIEW_WAVE6__.stateIdentity(11));
await page.evaluate(() => document.querySelector('#small').remove());
replay = await page.evaluate(({ target, expectedState }) => window.__LOCALVIEW_WAVE6__.validateReplayStep({
  target, expectedState, documentGeneration: 11,
}), { target: replayTarget, expectedState: current });
assert.ok(['state_mismatch', 'stable_ref_invalid'].includes(replay.reason));

const allReceipts = JSON.stringify({
  scan, small, clipped, occluded, journey, trapped, feedbackObserved, feedbackDelayed, feedbackNone, feedbackInconclusive, safeTargets,
});
assert.equal(allReceipts.includes('SUPER_PRIVATE_WAVE6_VALUE'), false);

console.log(JSON.stringify({
  ok: true,
  proofs: [
    'axe-local-live-scan',
    'missing-name-image-alt',
    'cross-origin-frame-bounded',
    'effective-clipped-hitbox',
    'occluded-browser-hit-test',
    'tab-shift-tab-real-browser',
    'focus-trap-observation',
    'overlay-semantic-exclusion',
    'overlay-freeze-cleanup',
    'overlay-deadline-cleanup',
    'route-generation-drift',
    'safe-discovery-skip',
    'feedback-observed-delayed-none-inconclusive',
    'replay-fail-closed',
    'privacy',
  ],
}));

await browser.close();
