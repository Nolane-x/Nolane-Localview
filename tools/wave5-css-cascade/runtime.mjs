import fs from 'node:fs/promises';
import { chromium } from 'playwright';

const invariant = (condition, message, detail) => {
  if (condition) return;
  throw new Error(`${message}${detail === undefined ? '' : `\n${JSON.stringify(detail, null, 2)}`}`);
};

const bootstrap = await fs.readFile('/tmp/localview-css-cascade-bootstrap.js', 'utf8');
const browser = await chromium.launch({ headless: true });

const findById = (node, id) => {
  if (!node) return null;
  if (node.attributes?.id === id) return node;
  for (const child of node.children || []) {
    const found = findById(child, id);
    if (found) return found;
  }
  return null;
};

const winnerMap = (cascade) =>
  new Map((cascade?.winners || []).map((winner) => [winner.property, winner]));

try {
  const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
  await page.setContent(`
    <!doctype html>
    <html>
      <head>
        <style>
          button.target { display: flex; }
          .target { display: grid; }
          #target {
            display: block;
            color: red !important;
            pointer-events: none !important;
          }
          @media (max-width: 200px) {
            #target { position: absolute; }
          }
          @media (min-width: 300px) {
            .target { position: relative; }
          }
          @supports (display: grid) {
            .target { visibility: visible; }
          }
          .target { z-index: 1; }
          .target { z-index: 2; }
          .target:is(.target) { opacity: 0.5; }
        </style>
      </head>
      <body>
        <button
          id="target"
          class="target"
          style="color: green; pointer-events: auto !important"
        >Save</button>
      </body>
    </html>
  `);
  await page.addScriptTag({ content: bootstrap });

  const first = await page.evaluate(() => window.__LOCALVIEW__.snapshot());
  const target = findById(first.semantic_tree, 'target');
  invariant(target, 'target semantic node missing', first.semantic_tree);
  invariant(target.styleTrace, 'styleTrace missing', target);

  const declarations = target.styleTrace.declarations || [];
  invariant(
    declarations.some((item) => item.property === 'position' && item.value === 'relative'),
    'active media declaration missing',
    declarations,
  );
  invariant(
    !declarations.some((item) => item.property === 'position' && item.value === 'absolute'),
    'inactive media declaration leaked into trace',
    declarations,
  );

  const cascade = target.styleTrace.authorCascade;
  invariant(cascade?.scope === 'supported_author_subset', 'unexpected cascade scope', cascade);
  invariant(cascade.coverage_complete === true, 'supported fixture must have complete coverage', cascade);
  invariant(
    cascade.unresolved_properties.includes('opacity'),
    'functional selector must taint opacity instead of fabricating specificity',
    cascade,
  );

  const winners = winnerMap(cascade);
  invariant(winners.get('display')?.value === 'block', 'ID specificity must win display', winners.get('display'));
  invariant(
    JSON.stringify(winners.get('display')?.specificity) === JSON.stringify([0, 1, 0, 0]),
    'display specificity tuple mismatch',
    winners.get('display'),
  );
  invariant(
    winners.get('color')?.value === 'red' && winners.get('color')?.important === true,
    'stylesheet important must beat inline normal',
    winners.get('color'),
  );
  invariant(
    winners.get('pointer-events')?.value === 'auto'
      && JSON.stringify(winners.get('pointer-events')?.specificity) === JSON.stringify([1, 0, 0, 0]),
    'inline important must beat stylesheet important',
    winners.get('pointer-events'),
  );
  invariant(winners.get('position')?.value === 'relative', 'active media winner missing', winners.get('position'));
  invariant(winners.get('visibility')?.value === 'visible', 'active supports winner missing', winners.get('visibility'));
  invariant(winners.get('z-index')?.value === '2', 'later equal-specificity source order must win', winners.get('z-index'));
  invariant(!winners.has('opacity'), 'unsupported functional selector must not mint opacity winner', winners.get('opacity'));

  const capPage = await browser.newPage({ viewport: { width: 800, height: 600 } });
  const paddingRules = Array.from({ length: 13 }, (_, index) => `.cap { padding-top: ${index + 1}px; }`).join('\n');
  await capPage.setContent(`
    <!doctype html>
    <html>
      <head>
        <style>
          ${paddingRules}
          #cap { display: grid; }
        </style>
      </head>
      <body><button id="cap" class="cap">Cap</button></body>
    </html>
  `);
  await capPage.addScriptTag({ content: bootstrap });
  const capped = await capPage.evaluate(() => window.__LOCALVIEW__.snapshot());
  const capNode = findById(capped.semantic_tree, 'cap');
  invariant(capNode?.styleTrace?.declarations?.length === 12, 'declaration retention cap changed', capNode?.styleTrace);
  invariant(
    !capNode.styleTrace.declarations.some((item) => item.property === 'display'),
    'fixture must place display declaration beyond retention cap',
    capNode.styleTrace.declarations,
  );
  const capWinners = winnerMap(capNode.styleTrace.authorCascade);
  invariant(
    capNode.styleTrace.authorCascade?.coverage_complete === true
      && capWinners.get('display')?.value === 'grid',
    'retention cap must not truncate cascade winner computation',
    capNode.styleTrace.authorCascade,
  );

  await page.evaluate(() => {
    const style = document.createElement('style');
    style.textContent = '@layer localview-test { #target { display: inline; } }';
    document.head.appendChild(style);
  });
  const layered = await page.evaluate(() => window.__LOCALVIEW__.snapshot());
  const layeredTarget = findById(layered.semantic_tree, 'target');
  invariant(
    layeredTarget?.styleTrace?.authorCascade?.coverage_complete === false,
    'cascade layer must invalidate bounded winner authority',
    layeredTarget?.styleTrace?.authorCascade,
  );
  invariant(
    (layeredTarget.styleTrace.authorCascade.winners || []).length === 0,
    'incomplete coverage must never retain winners',
    layeredTarget.styleTrace.authorCascade,
  );

  process.stdout.write(JSON.stringify({
    activeMediaFiltered: true,
    activeSupportsObserved: true,
    importantOrdering: true,
    specificityOrdering: true,
    sourceOrderTieBreak: true,
    unsupportedSelectorFailClosed: true,
    retentionIndependent: true,
    layerFailClosed: true,
  }) + '\n');
} finally {
  await browser.close();
}
