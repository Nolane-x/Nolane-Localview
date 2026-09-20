import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';
import { createServer } from 'vite';

const bootstrap = await fs.readFile('/tmp/localview-react-ownership-bootstrap.js', 'utf8');
const fixtureDir = path.resolve('.localview-wave5-react-ownership-fixture');

const invariant = (condition, message, detail) => {
  if (!condition) {
    throw new Error(`${message}${detail === undefined ? '' : ` :: ${JSON.stringify(detail)}`}`);
  }
};

const findById = (root, id) => {
  if (!root) return null;
  if (root.attributes?.id === id) return root;
  for (const child of root.children || []) {
    const found = findById(child, id);
    if (found) return found;
  }
  return null;
};

await fs.rm(fixtureDir, { recursive: true, force: true });
await fs.mkdir(fixtureDir, { recursive: true });

await fs.writeFile(
  path.join(fixtureDir, 'index.html'),
  '<!doctype html><html><body><div id="root"></div></body></html><script type="module" src="/main.jsx"></script>',
);

await fs.writeFile(
  path.join(fixtureDir, 'main.jsx'),
  `import React from 'react';
import { createRoot } from 'react-dom/client';

function SettingsCard({ secret }) {
  return React.createElement('button', { id: 'react-target' }, 'Save');
}
SettingsCard.displayName = 'SettingsCard';

function App() {
  return React.createElement(SettingsCard, { secret: 'MUST-NOT-LEAK-REACT-PROP' });
}

createRoot(document.getElementById('root')).render(React.createElement(App));
`,
);

const vite = await createServer({
  root: fixtureDir,
  logLevel: 'error',
  server: {
    host: '127.0.0.1',
    port: 0,
    strictPort: false,
  },
});
await vite.listen();

const address = vite.httpServer?.address();
if (!address || typeof address === 'string') {
  throw new Error('Vite fixture address unavailable');
}
const origin = `http://127.0.0.1:${address.port}`;

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
await page.addInitScript({ content: bootstrap });

try {
  await page.goto(origin, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() =>
    Boolean(window.__LOCALVIEW__?.snapshot && document.getElementById('react-target'))
  );

  const result = await page.evaluate(() => {
    const flattenById = (root, id) => {
      if (!root) return null;
      if (root.attributes?.id === id) return root;
      for (const child of root.children || []) {
        const found = flattenById(child, id);
        if (found) return found;
      }
      return null;
    };

    const snapshot = window.__LOCALVIEW__.snapshot();
    const target = flattenById(snapshot.semantic_tree, 'react-target');
    const element = document.getElementById('react-target');
    const fiberKey = Object.getOwnPropertyNames(element || {})
      .find((key) => key.startsWith('__reactFiber$') || key.startsWith('__reactInternalInstance$'));
    const fiber = fiberKey && element ? element[fiberKey] : null;
    const parent = fiber?.return || null;

    return {
      hint: target?.sourceHint ?? null,
      serialized: JSON.stringify(snapshot),
      diagnostic: {
        fiberKey: fiberKey || null,
        hostStateNodeMatches: Boolean(fiber && fiber.stateNode === element),
        hostDebugSource: fiber?._debugSource ?? null,
        hostDebugStack: String(fiber?._debugStack?.stack || '').slice(0, 2000),
        parentName: parent?.type?.displayName || parent?.type?.name || null,
        parentDebugSource: parent?._debugSource ?? null,
        parentDebugStack: String(parent?._debugStack?.stack || '').slice(0, 2000),
      },
    };
  });

  invariant(result.hint, 'React ownership hint missing', result.diagnostic);
  invariant(result.hint.origin === 'react-dev-fiber', 'unexpected React ownership origin', result);
  invariant(result.hint.component === 'SettingsCard', 'unexpected React component identity', result.hint);
  invariant(
    result.hint.signal === 'debug_stack' || result.hint.signal === 'debug_source',
    'React ownership must expose a bounded development source signal',
    result.hint,
  );
  invariant(typeof result.hint.file === 'string' && result.hint.file.length > 0, 'React source file missing', result.hint);
  invariant(Number.isInteger(result.hint.line) && result.hint.line > 0, 'React source line invalid', result.hint);
  invariant(!result.hint.file.includes('node_modules'), 'React ownership selected a dependency frame', result.hint);
  invariant(!result.hint.file.includes('/@fs/'), 'React ownership exposed Vite absolute-file transport', result.hint);
  invariant(!result.serialized.includes('MUST-NOT-LEAK-REACT-PROP'), 'React props leaked into semantic snapshot');

  const explicit = await page.evaluate(() => {
    const walk = (root, id) => {
      if (!root) return null;
      if (root.attributes?.id === id) return root;
      for (const child of root.children || []) {
        const found = walk(child, id);
        if (found) return found;
      }
      return null;
    };
    const target = document.getElementById('react-target');
    target.setAttribute('data-component-source', 'src/ExplicitCard.tsx:41:3');
    const snapshot = window.__LOCALVIEW__.snapshot();
    return walk(snapshot.semantic_tree, 'react-target')?.sourceHint ?? null;
  });
  invariant(explicit?.origin === 'data-component-source', 'explicit source must outrank React ownership', explicit);
  invariant(explicit?.file === 'src/ExplicitCard.tsx', 'explicit source file mismatch', explicit);
  invariant(explicit?.line === 41 && explicit?.column === 3, 'explicit source coordinates mismatch', explicit);

  const nonReact = await page.evaluate(() => {
    const walk = (root, id) => {
      if (!root) return null;
      if (root.attributes?.id === id) return root;
      for (const child of root.children || []) {
        const found = walk(child, id);
        if (found) return found;
      }
      return null;
    };
    const plain = document.createElement('div');
    plain.id = 'plain-target';
    plain.textContent = 'Plain DOM';
    document.body.appendChild(plain);

    const fake = document.createElement('div');
    fake.id = 'fake-react-target';
    fake.textContent = 'Fake React shape';
    Object.defineProperty(fake, '__reactFiber$fake', {
      configurable: true,
      value: {
        stateNode: fake,
        return: {
          type: function FakeComponent() {},
          _debugSource: {
            fileName: 'src/Fake.tsx',
            lineNumber: 1,
            columnNumber: 1,
          },
        },
      },
    });
    document.body.appendChild(fake);

    const snapshot = window.__LOCALVIEW__.snapshot();
    return {
      plain: walk(snapshot.semantic_tree, 'plain-target')?.sourceHint ?? null,
      fake: walk(snapshot.semantic_tree, 'fake-react-target')?.sourceHint ?? null,
    };
  });

  invariant(nonReact.plain === null, 'plain DOM node fabricated React ownership', nonReact);
  invariant(nonReact.fake === null, 'unpaired React-shaped property fabricated ownership', nonReact);

  process.stdout.write(JSON.stringify({
    ok: true,
    reactSignal: result.hint.signal,
    component: result.hint.component,
    file: result.hint.file,
    explicitPrecedence: true,
    plainDomIgnored: true,
    fakeFiberRejected: true,
    propsPrivate: true,
  }) + '\n');
} finally {
  await browser.close();
  await vite.close();
  await fs.rm(fixtureDir, { recursive: true, force: true });
}
