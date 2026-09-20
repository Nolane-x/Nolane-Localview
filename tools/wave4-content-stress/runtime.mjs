import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { chromium } from 'playwright';

const bootstrap = await fs.readFile('/tmp/localview-content-stress-bootstrap.js', 'utf8');
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 700 } });

await page.route('http://127.0.0.1:41756/**', async (route) => {
  await route.fulfill({
    status: 200,
    contentType: 'text/html',
    body: `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><style>
  body { font-family: sans-serif; margin: 20px; }
  #row { display:flex; gap:12px; width:420px; }
  #label { white-space:nowrap; display:inline-block; }
  #private { white-space:nowrap; }
</style></head>
<body>
  <div id="row"><button id="action"><span id="label">Save project</span></button><button>Cancel</button></div>
  <p id="body-copy">A short sentence for layout testing.</p>
  <div id="private" data-localview-private>private-secret-should-not-change</div>
  <input id="password" type="password" value="password-secret">
  <code id="code">const x = 1;</code>
</body></html>`,
  });
});

await page.goto('http://127.0.0.1:41756/fixture');
await page.addScriptTag({ content: bootstrap });

const canonicalRoute = await page.evaluate(() => {
  const url = new URL(location.href);
  url.search = '';
  url.hash = '';
  return url.toString();
});

const baseline = await page.evaluate(() => ({
  html: document.documentElement.outerHTML,
  label: document.querySelector('#label').textContent,
  body: document.querySelector('#body-copy').textContent,
  privateText: document.querySelector('#private').textContent,
  password: document.querySelector('#password').value,
  code: document.querySelector('#code').textContent,
  lang: document.documentElement.getAttribute('lang'),
  dir: document.documentElement.getAttribute('dir'),
}));

for (const profile of ['expanded_130', 'expanded_180', 'dense_cjk', 'rtl_pseudo']) {
  const token = `proof-${profile}`;
  const applied = await page.evaluate(({ token, profile, canonicalRoute }) =>
    window.__LOCALVIEW__.beginContentStress({
      requestToken: token,
      route: canonicalRoute,
      profile,
    }), { token, profile, canonicalRoute });
  assert.equal(applied, true, `${profile} should apply`);

  const completion = await page.evaluate(() => window.__LOCALVIEW__.takeContentStressCompletions(8));
  const appliedReceipt = completion.find((entry) => entry.status === 'applied');
  assert.ok(appliedReceipt, `${profile} applied receipt`);
  assert.equal(appliedReceipt.profile, profile);
  assert.ok(appliedReceipt.mutatedNodes >= 2);
  const encoded = JSON.stringify(appliedReceipt);
  for (const secret of ['Save project', 'private-secret-should-not-change', 'password-secret']) {
    assert.equal(encoded.includes(secret), false, `receipt leaked ${secret}`);
  }

  const stressed = await page.evaluate(() => ({
    label: document.querySelector('#label').textContent,
    body: document.querySelector('#body-copy').textContent,
    privateText: document.querySelector('#private').textContent,
    password: document.querySelector('#password').value,
    code: document.querySelector('#code').textContent,
    lang: document.documentElement.getAttribute('lang'),
    dir: document.documentElement.getAttribute('dir'),
  }));
  assert.notEqual(stressed.label, baseline.label);
  assert.notEqual(stressed.body, baseline.body);
  assert.equal(stressed.privateText, baseline.privateText);
  assert.equal(stressed.password, baseline.password);
  assert.equal(stressed.code, baseline.code);
  if (profile === 'rtl_pseudo') assert.equal(stressed.dir, 'rtl');

  const restored = await page.evaluate((token) =>
    window.__LOCALVIEW__.restoreContentStress(token), token);
  assert.equal(restored, true, `${profile} should restore`);
  const restoredCompletions = await page.evaluate(() => window.__LOCALVIEW__.takeContentStressCompletions(8));
  assert.ok(restoredCompletions.some((entry) => entry.status === 'restored'));

  const after = await page.evaluate(() => ({
    label: document.querySelector('#label').textContent,
    body: document.querySelector('#body-copy').textContent,
    privateText: document.querySelector('#private').textContent,
    password: document.querySelector('#password').value,
    code: document.querySelector('#code').textContent,
    lang: document.documentElement.getAttribute('lang'),
    dir: document.documentElement.getAttribute('dir'),
  }));
  assert.deepEqual(after, {
    label: baseline.label,
    body: baseline.body,
    privateText: baseline.privateText,
    password: baseline.password,
    code: baseline.code,
    lang: baseline.lang,
    dir: baseline.dir,
  });
}

// App-owned mutation during the lease must never be clobbered by restore.
const conflictToken = 'proof-conflict';
assert.equal(await page.evaluate(({ conflictToken, canonicalRoute }) =>
  window.__LOCALVIEW__.beginContentStress({
    requestToken: conflictToken,
    route: canonicalRoute,
    profile: 'expanded_180',
  }), { conflictToken, canonicalRoute }), true);
await page.evaluate(() => window.__LOCALVIEW__.takeContentStressCompletions(8));
await page.evaluate(() => {
  document.querySelector('#label').firstChild.nodeValue = 'app-owned-new-value';
});
const conflictRestore = await page.evaluate((token) =>
  window.__LOCALVIEW__.restoreContentStress(token), conflictToken);
assert.equal(conflictRestore, false);
const conflictCompletions = await page.evaluate(() => window.__LOCALVIEW__.takeContentStressCompletions(8));
const conflict = conflictCompletions.find((entry) => entry.status === 'restore_conflict');
assert.ok(conflict);
assert.ok(conflict.conflictNodes >= 1);
assert.equal(await page.locator('#label').textContent(), 'app-owned-new-value');

console.log(JSON.stringify({
  ok: true,
  proofs: [
    'four-bounded-profiles',
    'private-input-code-skip',
    'no-raw-text-in-receipt',
    'lang-dir-restore',
    'exact-text-restore',
    'app-mutation-conflict-fail-closed',
  ],
}));

await browser.close();
