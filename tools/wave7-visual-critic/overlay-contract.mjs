import fs from 'node:fs';

const source = fs.readFileSync(new URL('../../apps/desktop/src/features/visual-critic/overlay.ts', import.meta.url), 'utf8');
const mustContain = [
  "evidenceCaptureActive",
  "root.hidden = input.evidenceCaptureActive",
  "pointerEvents: 'none'",
  "onSelectFinding(finding.id)",
  "dataset.localviewVisualCritic",
  "exact_declaration_position",
  "observed_runtime_hint",
];
for (const fragment of mustContain) {
  if (!source.includes(fragment)) throw new Error(`missing overlay contract fragment: ${fragment}`);
}
for (const forbidden of ['innerHTML', 'document.querySelector(', 'iframe.contentDocument', 'contentWindow.document']) {
  if (source.includes(forbidden)) throw new Error(`forbidden target-DOM coupling: ${forbidden}`);
}
console.log('wave7 visual critic overlay contract: ok');
