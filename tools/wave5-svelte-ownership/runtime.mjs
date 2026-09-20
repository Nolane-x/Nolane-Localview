import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { compile } from 'svelte/compiler';

const bootstrap = await fs.readFile('/tmp/localview-svelte-ownership-bootstrap.js', 'utf8');
const fixtureDir = path.resolve('.localview-wave5-svelte-ownership-fixture');

const invariant = (condition, message, detail) => {
  if (!condition) {
    throw new Error(`${message}${detail === undefined ? '' : ` :: ${JSON.stringify(detail)}`}`);
  }
};

const walkById = (root, id) => {
  if (!root) return null;
  if (root.attributes?.id === id) return root;
  for (const child of root.children || []) {
    const found = walkById(child, id);
    if (found) return found;
  }
  return null;
};

await fs.rm(fixtureDir, { recursive: true, force: true });
await fs.mkdir(fixtureDir, { recursive: true });

const componentSource = `<script>
  let { secret } = $props();
</script>

<button id="svelte-target">Save</button>
`;
const compiled = compile(componentSource, {
  filename: 'src/SvelteCard.svelte',
  dev: true,
  generate: 'client',
});

await fs.writeFile(
  path.join(fixtureDir, 'index.html'),
  '<!doctype html><html><body><div id="root"></div></body></html><script type="module" src="/main.js"></script>',
);
await fs.writeFile(path.join(fixtureDir, 'SvelteCard.js'), compiled.js.code);
await fs.writeFile(
  path.join(fixtureDir, 'main.js'),
  `import { mount } from 'svelte';
import SvelteCard from './SvelteCard.js';

mount(SvelteCard, {
  target: document.getElementById('root'),
  props: { secret: 'MUST-NOT-LEAK-SVELTE-PROP' },
});
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
    Boolean(window.__LOCALVIEW__?.snapshot && document.getElementById('svelte-target'))
  );

  const result = await page.evaluate(() => {
    const walk = (root, id) => {
      if (!root) return null;
      if (root.attributes?.id === id) return root;
      for (const child of root.children || []) {
        const found = walk(child, id);
        if (found) return found;
      }
      return null;
    };

    const element = document.getElementById('svelte-target');
    const descriptor = element
      ? Object.getOwnPropertyDescriptor(element, '__svelte_meta')
      : null;
    const snapshot = window.__LOCALVIEW__.snapshot();
    const target = walk(snapshot.semantic_tree, 'svelte-target');

    return {
      hint: target?.sourceHint ?? null,
      serialized: JSON.stringify(snapshot),
      diagnostic: {
        ownMeta: Boolean(descriptor && Object.prototype.hasOwnProperty.call(descriptor, 'value')),
        loc: descriptor?.value?.loc ?? null,
        parentIsNull: descriptor?.value?.parent === null,
      },
    };
  });

  invariant(result.diagnostic.ownMeta, 'real Svelte runtime did not attach own metadata', result.diagnostic);
  invariant(result.hint, 'Svelte ownership hint missing', result.diagnostic);
  invariant(result.hint.origin === 'svelte-dev-meta', 'unexpected Svelte ownership origin', result.hint);
  invariant(result.hint.file === 'src/SvelteCard.svelte', 'unexpected Svelte source file', result.hint);
  invariant(result.hint.component === 'SvelteCard', 'unexpected Svelte component identity', result.hint);
  invariant(result.hint.signal === 'element_meta', 'unexpected Svelte ownership signal', result.hint);
  invariant(Number.isInteger(result.hint.line) && result.hint.line > 0, 'Svelte source line invalid', result.hint);
  invariant(Number.isInteger(result.hint.column) && result.hint.column >= 0, 'Svelte source column invalid', result.hint);
  invariant(!result.serialized.includes('MUST-NOT-LEAK-SVELTE-PROP'), 'Svelte prop leaked into semantic snapshot');

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
    const target = document.getElementById('svelte-target');
    target.setAttribute('data-component-source', 'src/ExplicitCard.svelte:41:3');
    const snapshot = window.__LOCALVIEW__.snapshot();
    return walk(snapshot.semantic_tree, 'svelte-target')?.sourceHint ?? null;
  });
  invariant(explicit?.origin === 'data-component-source', 'explicit source must outrank Svelte ownership', explicit);
  invariant(explicit?.file === 'src/ExplicitCard.svelte', 'explicit source file mismatch', explicit);
  invariant(explicit?.line === 41 && explicit?.column === 3, 'explicit source coordinates mismatch', explicit);

  const adversarial = await page.evaluate(() => {
    const walk = (root, id) => {
      if (!root) return null;
      if (root.attributes?.id === id) return root;
      for (const child of root.children || []) {
        const found = walk(child, id);
        if (found) return found;
      }
      return null;
    };

    const root = document.body;
    const plain = document.createElement('div');
    plain.id = 'plain-target';
    root.appendChild(plain);

    let getterCalls = 0;
    const accessor = document.createElement('div');
    accessor.id = 'svelte-accessor-target';
    Object.defineProperty(accessor, '__svelte_meta', {
      configurable: true,
      get() {
        getterCalls += 1;
        return {
          loc: { file: 'src/Accessor.svelte', line: 1, column: 0 },
        };
      },
    });
    root.appendChild(accessor);

    const absolute = document.createElement('div');
    absolute.id = 'svelte-absolute-target';
    Object.defineProperty(absolute, '__svelte_meta', {
      configurable: true,
      value: {
        parent: null,
        loc: { file: '/private/Absolute.svelte', line: 1, column: 0 },
      },
    });
    root.appendChild(absolute);

    const traversal = document.createElement('div');
    traversal.id = 'svelte-traversal-target';
    Object.defineProperty(traversal, '__svelte_meta', {
      configurable: true,
      value: {
        parent: null,
        loc: { file: 'src/../Traversal.svelte', line: 1, column: 0 },
      },
    });
    root.appendChild(traversal);

    const wrongExtension = document.createElement('div');
    wrongExtension.id = 'svelte-extension-target';
    Object.defineProperty(wrongExtension, '__svelte_meta', {
      configurable: true,
      value: {
        parent: null,
        loc: { file: 'src/NotAComponent.ts', line: 1, column: 0 },
      },
    });
    root.appendChild(wrongExtension);

    const snapshot = window.__LOCALVIEW__.snapshot();
    return {
      plain: walk(snapshot.semantic_tree, 'plain-target')?.sourceHint ?? null,
      accessor: walk(snapshot.semantic_tree, 'svelte-accessor-target')?.sourceHint ?? null,
      absolute: walk(snapshot.semantic_tree, 'svelte-absolute-target')?.sourceHint ?? null,
      traversal: walk(snapshot.semantic_tree, 'svelte-traversal-target')?.sourceHint ?? null,
      wrongExtension: walk(snapshot.semantic_tree, 'svelte-extension-target')?.sourceHint ?? null,
      getterCalls,
    };
  });

  invariant(adversarial.plain === null, 'plain DOM fabricated Svelte ownership', adversarial);
  invariant(adversarial.accessor === null, 'accessor-backed Svelte marker fabricated ownership', adversarial);
  invariant(adversarial.getterCalls === 0, 'LocalView invoked a Svelte metadata getter', adversarial);
  invariant(adversarial.absolute === null, 'absolute Svelte source escaped privacy boundary', adversarial);
  invariant(adversarial.traversal === null, 'traversal Svelte source escaped privacy boundary', adversarial);
  invariant(adversarial.wrongExtension === null, 'non-Svelte file fabricated ownership', adversarial);

  process.stdout.write(JSON.stringify({
    ok: true,
    component: result.hint.component,
    file: result.hint.file,
    line: result.hint.line,
    column: result.hint.column,
    rootParentNullAccepted: result.diagnostic.parentIsNull,
    explicitPrecedence: true,
    propsPrivate: true,
    accessorNotInvoked: true,
    unsafePathsRejected: true,
  }) + '\n');
} finally {
  await browser.close();
  await vite.close();
  await fs.rm(fixtureDir, { recursive: true, force: true });
}
