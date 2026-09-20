import http from 'node:http';
import fs from 'node:fs/promises';
import { chromium } from 'playwright';

const bootstrap = await fs.readFile('/tmp/localview-css-declaration-bootstrap.js', 'utf8');

const listen = (handler) => new Promise((resolve, reject) => {
  const server = http.createServer(handler);
  server.on('error', reject);
  server.listen(0, '127.0.0.1', () => resolve(server));
});
const address = (server) => {
  const value = server.address();
  if (!value || typeof value === 'string') throw new Error('server_address_missing');
  return value.port;
};
const close = (server) => new Promise((resolve) => server.close(resolve));

const opaqueServer = await listen((req, res) => {
  res.writeHead(200, { 'content-type': 'text/css; charset=utf-8' });
  res.end('.card { border-top-width: 7px; }');
});
const opaquePort = address(opaqueServer);

const appServer = await listen((req, res) => {
  const url = new URL(req.url || '/', 'http://127.0.0.1');
  if (url.pathname === '/app.css') {
    res.writeHead(200, { 'content-type': 'text/css; charset=utf-8' });
    res.end([
      '.card { color: rgb(1, 2, 3); display: block; }',
      '@media (min-width: 1px) { .card { position: relative; } }',
    ].join('\n'));
    return;
  }
  res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
  res.end(`<!doctype html>
<html>
<head>
  <link rel="stylesheet" href="/app.css?token=css-secret#css-fragment">
  <link rel="stylesheet" href="http://127.0.0.1:${opaquePort}/opaque.css">
</head>
<body>
  <button id="target" class="card" style="opacity: 0.8">Save</button>
</body>
</html>`);
});
const appPort = address(appServer);

let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
  await page.addInitScript({ content: bootstrap });
  await page.goto(`http://127.0.0.1:${appPort}/`, { waitUntil: 'networkidle' });

  const proof = await page.evaluate(() => {
    const element = document.getElementById('target');
    if (!element) throw new Error('target_missing');
    const api = window.__LOCALVIEW__;
    if (!api?.refFor || !api?.inspectCss) throw new Error('localview_css_api_missing');
    const reference = api.refFor(element);
    return api.inspectCss(reference);
  });

  if (!proof || typeof proof !== 'object') throw new Error('css_proof_missing');
  if (!String(proof.reference || '').startsWith('@e')) throw new Error('reference_missing');
  if (proof.computed?.color !== 'rgb(1, 2, 3)') {
    throw new Error(`computed_color_mismatch:${JSON.stringify(proof.computed)}`);
  }
  if (proof.computed?.position !== 'relative') {
    throw new Error(`conditional_computed_style_missing:${JSON.stringify(proof.computed)}`);
  }
  if (!proof.conditional_rules_omitted) throw new Error('conditional_omission_not_disclosed');
  if (!(proof.opaque_stylesheets >= 1)) {
    throw new Error(`opaque_stylesheet_not_counted:${JSON.stringify(proof)}`);
  }

  const declarations = Array.isArray(proof.declarations) ? proof.declarations : [];
  const inlineOpacity = declarations.find((item) =>
    item.origin === 'inline' && item.property === 'opacity' && item.value === '0.8'
  );
  if (!inlineOpacity) throw new Error(`inline_declaration_missing:${JSON.stringify(declarations)}`);

  const stylesheetColor = declarations.find((item) =>
    item.origin === 'author_stylesheet'
    && item.property === 'color'
    && item.value === 'rgb(1, 2, 3)'
    && item.selector === '.card'
    && item.stylesheet_path === '/app.css'
  );
  if (!stylesheetColor) {
    throw new Error(`stylesheet_declaration_missing:${JSON.stringify(declarations)}`);
  }

  if (declarations.some((item) => item.property === 'position')) {
    throw new Error('conditional_rule_was_presented_as_top_level_evidence');
  }
  const serialized = JSON.stringify(proof);
  for (const forbidden of ['css-secret', 'css-fragment', 'opaque.css?']) {
    if (serialized.includes(forbidden)) throw new Error(`retention_leak:${forbidden}`);
  }
  if (Object.keys(proof.computed || {}).some((property) => property.startsWith('--'))) {
    throw new Error('custom_property_enumerated');
  }

  console.log(JSON.stringify({
    reference: proof.reference,
    declarations: declarations.length,
    opaque_stylesheets: proof.opaque_stylesheets,
    conditional_rules_omitted: proof.conditional_rules_omitted,
  }));
} finally {
  if (browser) await browser.close();
  await close(appServer);
  await close(opaqueServer);
}
