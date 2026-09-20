import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';

const bootstrap = await fs.readFile('/tmp/localview-vue-ownership-bootstrap.js', 'utf8');
const fixtureDir = path.resolve('.localview-wave5-vue-ownership-fixture');

const invariant = (condition, message, detail) => {
  if (!condition) {
    throw new Error(`${message}${detail === undefined ? '' : ` :: ${JSON.stringify(detail)}`}`);
  }
};

await fs.rm(fixtureDir, { recursive: true, force: true });
await fs.mkdir(path.join(fixtureDir, 'src'), { recursive: true });

await fs.writeFile(
  path.join(fixtureDir, 'index.html'),
  '<!doctype html><html><body><div id="app"></div><script type="module" src="/main.js"></script></body></html>',
);
await fs.writeFile(
  path.join(fixtureDir, 'src', 'VueCard.vue'),
  `<script setup>
defineProps({
  secret: { type: String, required: true }
})
</script>

<template>
  <button id="vue-target">Save</button>
</template>
`,
);
await fs.writeFile(
  path.join(fixtureDir, 'main.js'),
  `import { createApp } from 'vue';
import VueCard from './src/VueCard.vue';

createApp(VueCard, { secret: 'MUST-NOT-LEAK-VUE-PROP' }).mount('#app');
`,
);

const vite = await createServer({
  root: fixtureDir,
  logLevel: 'error',
  plugins: [
    vue({
      // Current plugin-vue development metadata exposes absolute filenames.
      // This proof deliberately asks the real SFC compiler for basename-safe
      // metadata while leaving the Vue runtime itself in development mode.
      isProduction: true,
    }),
  ],
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
    Boolean(window.__LOCALVIEW__?.snapshot && document.getElementById('vue-target'))
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

    const element = document.getElementById('vue-target');
    const instanceDescriptor = element
      ? Object.getOwnPropertyDescriptor(element, '__vueParentComponent')
      : null;
    const instance = instanceDescriptor &&
      Object.prototype.hasOwnProperty.call(instanceDescriptor, 'value')
      ? instanceDescriptor.value
      : null;
    const typeDescriptor = instance
      ? Object.getOwnPropertyDescriptor(instance, 'type')
      : null;
    const type = typeDescriptor &&
      Object.prototype.hasOwnProperty.call(typeDescriptor, 'value')
      ? typeDescriptor.value
      : null;
    const fileDescriptor = type
      ? Object.getOwnPropertyDescriptor(type, '__file')
      : null;

    const snapshot = window.__LOCALVIEW__.snapshot();
    const target = walk(snapshot.semantic_tree, 'vue-target');

    return {
      hint: target?.sourceHint ?? null,
      serialized: JSON.stringify(snapshot),
      diagnostic: {
        ownInstance: Boolean(
          instanceDescriptor &&
          Object.prototype.hasOwnProperty.call(instanceDescriptor, 'value')
        ),
        ownType: Boolean(
          typeDescriptor &&
          Object.prototype.hasOwnProperty.call(typeDescriptor, 'value')
        ),
        ownFile: Boolean(
          fileDescriptor &&
          Object.prototype.hasOwnProperty.call(fileDescriptor, 'value')
        ),
        upstreamFile: fileDescriptor?.value ?? null,
      },
    };
  });

  invariant(result.diagnostic.ownInstance, 'real Vue runtime did not attach own component marker', result.diagnostic);
  invariant(result.diagnostic.ownType, 'real Vue instance did not expose own component type', result.diagnostic);
  invariant(result.diagnostic.ownFile, 'real Vue SFC component type did not expose own __file metadata', result.diagnostic);
  invariant(result.hint, 'Vue ownership hint missing', result);
  invariant(result.hint.origin === 'vue-dev-instance', 'unexpected Vue ownership origin', result.hint);
  invariant(result.hint.file === 'VueCard.vue', 'unexpected bounded Vue source file', result.hint);
  invariant(result.hint.component === 'VueCard', 'unexpected Vue component identity', result.hint);
  invariant(result.hint.signal === 'element_parent_component', 'unexpected Vue signal', result.hint);
  invariant(!Object.prototype.hasOwnProperty.call(result.hint, 'line'), 'Vue foundation fabricated a source line', result.hint);
  invariant(!Object.prototype.hasOwnProperty.call(result.hint, 'column'), 'Vue foundation fabricated a source column', result.hint);
  invariant(!result.serialized.includes('MUST-NOT-LEAK-VUE-PROP'), 'Vue prop leaked into semantic snapshot');

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
    const target = document.getElementById('vue-target');
    target.setAttribute('data-component-source', 'src/ExplicitVue.vue:31:2');
    const snapshot = window.__LOCALVIEW__.snapshot();
    return walk(snapshot.semantic_tree, 'vue-target')?.sourceHint ?? null;
  });
  invariant(explicit?.origin === 'data-component-source', 'explicit source must outrank Vue ownership', explicit);
  invariant(explicit?.file === 'src/ExplicitVue.vue', 'explicit Vue source file mismatch', explicit);
  invariant(explicit?.line === 31 && explicit?.column === 2, 'explicit Vue source coordinates mismatch', explicit);

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
    const make = (id) => {
      const node = document.createElement('div');
      node.id = id;
      root.appendChild(node);
      return node;
    };

    const plain = make('vue-plain-target');

    let instanceGetterCalls = 0;
    const instanceAccessor = make('vue-instance-accessor-target');
    Object.defineProperty(instanceAccessor, '__vueParentComponent', {
      configurable: true,
      get() {
        instanceGetterCalls += 1;
        return { type: { __file: 'src/Accessor.vue' } };
      },
    });

    let typeGetterCalls = 0;
    const typeAccessor = make('vue-type-accessor-target');
    const fakeInstanceWithTypeAccessor = {};
    Object.defineProperty(fakeInstanceWithTypeAccessor, 'type', {
      configurable: true,
      get() {
        typeGetterCalls += 1;
        return { __file: 'src/TypeAccessor.vue' };
      },
    });
    Object.defineProperty(typeAccessor, '__vueParentComponent', {
      configurable: true,
      value: fakeInstanceWithTypeAccessor,
    });

    let fileGetterCalls = 0;
    const fileAccessor = make('vue-file-accessor-target');
    const fakeTypeWithFileAccessor = {};
    Object.defineProperty(fakeTypeWithFileAccessor, '__file', {
      configurable: true,
      get() {
        fileGetterCalls += 1;
        return 'src/FileAccessor.vue';
      },
    });
    Object.defineProperty(fileAccessor, '__vueParentComponent', {
      configurable: true,
      value: { type: fakeTypeWithFileAccessor },
    });

    const fakeWithFile = (id, file) => {
      const node = make(id);
      Object.defineProperty(node, '__vueParentComponent', {
        configurable: true,
        value: { type: { __file: file } },
      });
      return node;
    };
    fakeWithFile('vue-absolute-target', '/private/Absolute.vue');
    fakeWithFile('vue-traversal-target', 'src/../Traversal.vue');
    fakeWithFile('vue-encoded-target', 'src/%2e%2e/Encoded.vue');
    fakeWithFile('vue-extension-target', 'src/NotVue.ts');

    const snapshot = window.__LOCALVIEW__.snapshot();
    return {
      plain: walk(snapshot.semantic_tree, 'vue-plain-target')?.sourceHint ?? null,
      instanceAccessor: walk(snapshot.semantic_tree, 'vue-instance-accessor-target')?.sourceHint ?? null,
      typeAccessor: walk(snapshot.semantic_tree, 'vue-type-accessor-target')?.sourceHint ?? null,
      fileAccessor: walk(snapshot.semantic_tree, 'vue-file-accessor-target')?.sourceHint ?? null,
      absolute: walk(snapshot.semantic_tree, 'vue-absolute-target')?.sourceHint ?? null,
      traversal: walk(snapshot.semantic_tree, 'vue-traversal-target')?.sourceHint ?? null,
      encoded: walk(snapshot.semantic_tree, 'vue-encoded-target')?.sourceHint ?? null,
      wrongExtension: walk(snapshot.semantic_tree, 'vue-extension-target')?.sourceHint ?? null,
      instanceGetterCalls,
      typeGetterCalls,
      fileGetterCalls,
    };
  });

  invariant(adversarial.plain === null, 'plain DOM fabricated Vue ownership', adversarial);
  invariant(adversarial.instanceAccessor === null && adversarial.instanceGetterCalls === 0, 'Vue element accessor was invoked', adversarial);
  invariant(adversarial.typeAccessor === null && adversarial.typeGetterCalls === 0, 'Vue instance.type accessor was invoked', adversarial);
  invariant(adversarial.fileAccessor === null && adversarial.fileGetterCalls === 0, 'Vue type.__file accessor was invoked', adversarial);
  invariant(adversarial.absolute === null, 'absolute Vue path escaped privacy boundary', adversarial);
  invariant(adversarial.traversal === null, 'traversal Vue path escaped privacy boundary', adversarial);
  invariant(adversarial.encoded === null, 'encoded Vue path escaped privacy boundary', adversarial);
  invariant(adversarial.wrongExtension === null, 'non-Vue file fabricated Vue ownership', adversarial);

  process.stdout.write(JSON.stringify({
    ok: true,
    framework: 'vue',
    component: result.hint.component,
    file: result.hint.file,
    upstreamFile: result.diagnostic.upstreamFile,
    explicitPrecedence: true,
    propsPrivate: true,
    noFabricatedCoordinates: true,
    accessorNotInvoked: true,
    unsafePathsRejected: true,
  }) + '\n');
} finally {
  await browser.close();
  await vite.close();
  await fs.rm(fixtureDir, { recursive: true, force: true });
}
