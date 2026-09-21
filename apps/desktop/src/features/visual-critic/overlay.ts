export type CriticEvidenceClass = 'deterministic' | 'heuristic' | 'subjective';

export interface CriticSourceHintView {
  file: string;
  line?: number;
  column?: number;
  authority: 'observed_runtime_hint' | 'exact_declaration_position';
}

export interface CriticFindingView {
  id: string;
  code: string;
  class: CriticEvidenceClass;
  confidence: number;
  affectedRefs: string[];
  evidenceSummary: string;
  measuredDeviation?: number;
  sourceHint?: CriticSourceHintView;
  classificationReason: string;
}

export interface CriticRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CriticOverlayInput {
  findings: CriticFindingView[];
  rectByRef: ReadonlyMap<string, CriticRect>;
  selectedFindingId?: string;
  evidenceCaptureActive: boolean;
}

export interface CriticOverlayController {
  update(input: CriticOverlayInput): void;
  destroy(): void;
}

const MAX_OVERLAY_FINDINGS = 128;
const MAX_REFS_PER_FINDING = 24;

function finiteRect(rect: CriticRect): boolean {
  return Number.isFinite(rect.x)
    && Number.isFinite(rect.y)
    && Number.isFinite(rect.width)
    && Number.isFinite(rect.height)
    && rect.width > 0
    && rect.height > 0;
}

function percentConfidence(confidence: number): string {
  const bounded = Math.max(0, Math.min(1, confidence));
  return `${Math.round(bounded * 100)}%`;
}

function sourceLabel(source: CriticSourceHintView | undefined): string | undefined {
  if (!source) return undefined;
  const line = source.line === undefined ? '' : `:${source.line}`;
  const column = source.column === undefined ? '' : `:${source.column}`;
  const authority = source.authority === 'exact_declaration_position' ? 'verified declaration' : 'runtime hint';
  return `${source.file}${line}${column} · ${authority}`;
}

/**
 * Self-contained Wave 7 overlay for the LocalView chrome layer.
 *
 * It never mounts inside the inspected application WebView, uses fixed-position
 * paint-only highlights, and returns no UI while evidence capture is active.
 * The integrator owns coordinate conversion from managed-preview CSS pixels to
 * desktop chrome coordinates.
 */
export function mountVisualCriticOverlay(
  host: HTMLElement,
  onSelectFinding: (findingId: string) => void,
): CriticOverlayController {
  const root = document.createElement('section');
  root.dataset.localviewVisualCritic = 'overlay';
  root.setAttribute('aria-label', 'Visual critic findings');
  Object.assign(root.style, {
    position: 'fixed',
    inset: '0',
    zIndex: '40',
    pointerEvents: 'none',
    contain: 'strict',
  });
  host.append(root);

  const update = (input: CriticOverlayInput): void => {
    root.replaceChildren();
    root.hidden = input.evidenceCaptureActive;
    if (input.evidenceCaptureActive) return;

    const findings = input.findings.slice(0, MAX_OVERLAY_FINDINGS);
    for (const finding of findings) {
      for (const reference of finding.affectedRefs.slice(0, MAX_REFS_PER_FINDING)) {
        const rect = input.rectByRef.get(reference);
        if (!rect || !finiteRect(rect)) continue;
        const highlight = document.createElement('div');
        highlight.dataset.findingId = finding.id;
        highlight.dataset.evidenceClass = finding.class;
        highlight.setAttribute('aria-hidden', 'true');
        Object.assign(highlight.style, {
          position: 'fixed',
          left: `${rect.x}px`,
          top: `${rect.y}px`,
          width: `${rect.width}px`,
          height: `${rect.height}px`,
          boxSizing: 'border-box',
          outline: input.selectedFindingId === finding.id ? '3px solid currentColor' : '2px solid currentColor',
          outlineOffset: '2px',
          pointerEvents: 'none',
        });
        root.append(highlight);
      }
    }

    const panel = document.createElement('div');
    panel.dataset.localviewVisualCritic = 'finding-list';
    Object.assign(panel.style, {
      position: 'fixed',
      right: '16px',
      top: '72px',
      width: 'min(360px, calc(100vw - 32px))',
      maxHeight: 'min(560px, calc(100vh - 96px))',
      overflow: 'auto',
      pointerEvents: 'auto',
      contain: 'content',
    });

    for (const finding of findings) {
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.findingId = finding.id;
      button.dataset.evidenceClass = finding.class;
      button.setAttribute('aria-pressed', String(input.selectedFindingId === finding.id));
      button.addEventListener('click', () => onSelectFinding(finding.id));

      const title = document.createElement('strong');
      title.textContent = `${finding.code} · ${finding.class} · ${percentConfidence(finding.confidence)}`;
      const summary = document.createElement('span');
      summary.textContent = finding.evidenceSummary;
      const reason = document.createElement('small');
      reason.textContent = finding.classificationReason;
      button.append(title, document.createElement('br'), summary, document.createElement('br'), reason);

      if (finding.measuredDeviation !== undefined && Number.isFinite(finding.measuredDeviation)) {
        const deviation = document.createElement('small');
        deviation.textContent = ` · deviation ${finding.measuredDeviation.toFixed(2)}`;
        button.append(document.createElement('br'), deviation);
      }
      const source = sourceLabel(finding.sourceHint);
      if (source) {
        const sourceNode = document.createElement('small');
        sourceNode.textContent = source;
        button.append(document.createElement('br'), sourceNode);
      }
      panel.append(button);
    }
    root.append(panel);
  };

  return {
    update,
    destroy: () => root.remove(),
  };
}
