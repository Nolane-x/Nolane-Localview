#![forbid(unsafe_code)]

/// Local-only Wave 6 accessibility + interaction instrumentation.
/// The host injects a bundled/local axe object through installAxe; this code
/// never downloads executable code from a CDN or arbitrary runtime URL.
pub fn wave6_bootstrap_script() -> &'static str {
    WAVE6_SCRIPT
}

const WAVE6_SCRIPT: &str = r##"
(() => {
  if (window.__LOCALVIEW_WAVE6__) return;
  const base = window.__LOCALVIEW__;
  if (!base?.refFor || !base?.snapshot) return;

  const MAX_AXE_FINDINGS = 128;
  const MAX_SCAN_ELEMENTS = 4096;
  const MAX_FOCUS_TRANSITIONS = 64;
  const MAX_DISCOVERY_TARGETS = 32;
  const MAX_OVERLAY_MARKERS = 64;
  const stableRefPattern = /^@e[0-9a-f]+$/i;
  let axeRuntime = null;
  let journey = null;
  let feedbackProbe = null;

  const bounded = (value, max) => String(value ?? '').slice(0, max);
  const validStableRef = value => typeof value === 'string' && stableRefPattern.test(value);

  const canonicalRoute = () => {
    const url = new URL(location.href);
    return url.origin + url.pathname;
  };

  const resolveStableRef = reference => {
    if (!validStableRef(reference)) return null;
    let scanned = 0;
    for (const element of document.querySelectorAll('*')) {
      scanned += 1;
      if (scanned > MAX_SCAN_ELEMENTS) return null;
      if (element.closest?.('[data-localview-owned]')) continue;
      if (base.refFor(element) === reference) return element;
    }
    return null;
  };

  const collectSignature = (root, output) => {
    if (!root || output.length >= 600) return;
    const reference = validStableRef(root.ref) ? root.ref : '';
    const role = bounded(root.role, 48);
    const tag = bounded(root.tag, 32);
    const states = root.states && typeof root.states === 'object'
      ? Object.keys(root.states).sort().slice(0, 16)
          .map(key => bounded(key, 32) + ':' + String(root.states[key] === true || root.states[key] === 'true'))
          .join(',')
      : '';
    output.push(reference + '|' + role + '|' + tag + '|' + states);
    for (const child of Array.isArray(root.children) ? root.children : []) {
      if (output.length >= 600) break;
      collectSignature(child, output);
    }
  };

  const fnv1a = text => {
    let hash = 0x811c9dc5;
    for (let i = 0; i < text.length; i += 1) {
      hash ^= text.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return hash.toString(16).padStart(8, '0');
  };

  const stateIdentity = documentGeneration => {
    const generation = Number(documentGeneration);
    if (!Number.isSafeInteger(generation) || generation < 0) {
      throw new Error('wave6_document_generation_invalid');
    }
    const snapshot = base.snapshot();
    const signature = [];
    collectSignature(snapshot?.semantic_tree, signature);
    return Object.freeze({
      route: canonicalRoute(),
      document_generation: generation,
      semantic_fingerprint: fnv1a(signature.join('\\n')),
      viewport: [Math.max(0, window.innerWidth | 0), Math.max(0, window.innerHeight | 0)],
    });
  };

  const stateMatches = (expected, actual) => (
    !!expected && !!actual &&
    expected.route === actual.route &&
    Number(expected.document_generation) === Number(actual.document_generation) &&
    expected.semantic_fingerprint === actual.semantic_fingerprint &&
    Array.isArray(expected.viewport) && Array.isArray(actual.viewport) &&
    expected.viewport[0] === actual.viewport[0] &&
    expected.viewport[1] === actual.viewport[1]
  );

  const localDeterministicChecks = () => {
    const snapshot = base.snapshot();
    const findings = [];
    const visit = node => {
      if (!node || findings.length >= MAX_AXE_FINDINGS) return;
      const reference = validStableRef(node.ref) ? node.ref : null;
      const role = String(node.role || '');
      if (reference && node.interactive === true && String(node.name || '').trim() === '') {
        findings.push({ code: 'interactive_name_missing', reference, evidence_kind: 'local_deterministic', deterministic: true });
      }
      if (reference && role === 'img' && String(node.name || '').trim() === '') {
        findings.push({ code: 'image_name_missing', reference, evidence_kind: 'local_deterministic', deterministic: true });
      }
      for (const child of Array.isArray(node.children) ? node.children : []) visit(child);
    };
    visit(snapshot?.semantic_tree);
    return findings;
  };

  const installAxe = candidate => {
    if (!candidate || typeof candidate.run !== 'function') return false;
    axeRuntime = candidate;
    return true;
  };

  const exactAxeTargetRef = node => {
    const target = node?.target;
    if (!Array.isArray(target) || target.length !== 1 || typeof target[0] !== 'string') return null;
    const selector = target[0];
    if (selector.length > 512) return null;
    let matches;
    try { matches = document.querySelectorAll(selector); } catch (_) { return null; }
    if (matches.length !== 1) return null;
    const element = matches[0];
    if (element.closest?.('[data-localview-owned]')) return null;
    const reference = base.refFor(element);
    return validStableRef(reference) ? reference : null;
  };

  const runAccessibilityScan = async ({ documentGeneration, maxFindings = MAX_AXE_FINDINGS } = {}) => {
    if (!axeRuntime) throw new Error('wave6_axe_not_installed_local');
    const before = stateIdentity(documentGeneration);
    const cap = Math.max(1, Math.min(Number(maxFindings) || 0, MAX_AXE_FINDINGS));
    const result = await axeRuntime.run(document, { resultTypes: ['violations'], selectors: true });
    const after = stateIdentity(documentGeneration);
    if (!stateMatches(before, after)) {
      return { status: 'inconclusive', reason: 'state_drift', state: after, local_findings: [], axe_findings: [] };
    }
    const axeFindings = [];
    for (const violation of Array.isArray(result?.violations) ? result.violations : []) {
      for (const node of Array.isArray(violation.nodes) ? violation.nodes : []) {
        if (axeFindings.length >= cap) break;
        const reference = exactAxeTargetRef(node);
        const rule = bounded(violation.id, 96).replace(/[^a-z0-9_.-]/gi, '');
        axeFindings.push({
          code: rule,
          rule_id: rule,
          impact: bounded(violation.impact, 24) || null,
          help: bounded(violation.help, 240),
          reference,
          target_resolution: reference ? 'stable_ref' : 'unresolved',
          evidence_kind: 'axe_rule',
          deterministic: false,
        });
      }
      if (axeFindings.length >= cap) break;
    }
    return {
      status: 'complete',
      state: after,
      local_findings: localDeterministicChecks(),
      axe_findings: axeFindings,
      truncated: axeFindings.length >= cap,
      accessibility_complete: false,
    };
  };

  const focusableCandidates = () => {
    const output = [];
    let scanned = 0;
    for (const element of document.querySelectorAll('a[href],button,input,select,textarea,[tabindex],[contenteditable="true"]')) {
      scanned += 1;
      if (scanned > 512 || output.length >= 256) break;
      if (element.closest?.('[data-localview-owned]')) continue;
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      const hidden = element.hidden || style.display === 'none' || style.visibility === 'hidden' || rect.width <= 0 || rect.height <= 0;
      const disabled = element.matches?.(':disabled') || element.getAttribute('aria-disabled') === 'true';
      const tabindex = Number(element.tabIndex);
      if (hidden || disabled || !Number.isFinite(tabindex) || tabindex < 0) continue;
      const reference = base.refFor(element);
      if (validStableRef(reference)) output.push(reference);
    }
    return Array.from(new Set(output));
  };

  const focusObservation = (documentGeneration, direction) => {
    const element = document.activeElement;
    const body = element === document.body || element === document.documentElement || !element;
    const reference = body ? null : base.refFor(element);
    const rect = !body ? element.getBoundingClientRect() : null;
    const style = !body ? getComputedStyle(element) : null;
    const hiddenOrOffscreen = !body && (
      element.hidden || style.display === 'none' || style.visibility === 'hidden' ||
      rect.width <= 0 || rect.height <= 0 || rect.bottom <= 0 || rect.right <= 0 ||
      rect.top >= innerHeight || rect.left >= innerWidth
    );
    return {
      transition_index: journey ? journey.transitions.length + 1 : 0,
      reference: validStableRef(reference) ? reference : null,
      route: canonicalRoute(),
      document_generation: Number(documentGeneration),
      tabindex: body ? null : Number(element.tabIndex),
      hidden_or_offscreen: !!hiddenOrOffscreen,
      is_body_or_document: body,
      direction: direction === 'shift_tab' ? 'shift_tab' : direction === 'initial' ? 'initial' : 'tab',
    };
  };

  const createOverlay = () => {
    const root = document.createElement('div');
    root.setAttribute('data-localview-owned', 'wave6-focus-path');
    root.setAttribute('aria-hidden', 'true');
    root.style.cssText = 'position:fixed;inset:0;z-index:2147483646;pointer-events:none;contain:strict;overflow:hidden;margin:0;padding:0;border:0';
    (document.documentElement || document.body).appendChild(root);
    return root;
  };

  const syncOverlayFreeze = () => {
    if (!journey?.overlay) return;
    const frozen = document.documentElement?.hasAttribute('data-localview-visual-freeze');
    journey.overlay.style.visibility = frozen ? 'hidden' : 'visible';
  };

  const renderFocusOverlay = () => {
    if (!journey?.overlay) return;
    journey.overlay.replaceChildren();
    const observations = [journey.initial, ...journey.transitions].slice(0, MAX_OVERLAY_MARKERS);
    observations.forEach((observation, index) => {
      const element = observation.reference ? resolveStableRef(observation.reference) : null;
      if (!element) return;
      const rect = element.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const marker = document.createElement('div');
      marker.setAttribute('data-localview-owned', 'wave6-focus-path-marker');
      marker.setAttribute('aria-hidden', 'true');
      marker.dataset.reference = observation.reference;
      if (journey.problemRefs.has(observation.reference)) marker.dataset.problem = 'true';
      marker.style.cssText =
        'position:fixed;left:' + Math.max(0, rect.left) + 'px;top:' + Math.max(0, rect.top) +
        'px;width:' + Math.max(1, Math.min(rect.width, innerWidth)) + 'px;height:' +
        Math.max(1, Math.min(rect.height, innerHeight)) +
        'px;box-sizing:border-box;border:2px solid currentColor;pointer-events:none;contain:layout paint style;font:11px/1 sans-serif';
      const badge = document.createElement('span');
      badge.setAttribute('data-localview-owned', 'wave6-focus-path-badge');
      badge.textContent = String(index);
      badge.style.cssText = 'position:absolute;left:0;top:0;transform:translateY(-100%);padding:1px 3px;background:Canvas;color:CanvasText;pointer-events:none';
      marker.appendChild(badge);
      journey.overlay.appendChild(marker);
    });
    syncOverlayFreeze();
  };

  const cleanupJourney = reason => {
    if (!journey) return null;
    const current = journey;
    journey = null;
    current.abort.abort();
    current.freezeObserver.disconnect();
    clearTimeout(current.deadlineTimer);
    current.overlay.remove();
    return {
      status: reason ? 'cancelled' : 'complete',
      reason: reason || null,
      initial: current.initial,
      transitions: current.transitions.slice(0, MAX_FOCUS_TRANSITIONS),
      candidates: current.candidates.slice(0, 256),
      unvisited_candidates: current.candidates.filter(reference => !current.visited.has(reference)).slice(0, 256),
      unreachable: current.closedCycle
        ? current.candidates.filter(reference => !current.visited.has(reference)).slice(0, 256)
        : [],
      cycle_observed: current.closedCycle,
      problem_refs: Array.from(current.problemRefs).slice(0, 64),
    };
  };

  const beginKeyboardJourney = ({ documentGeneration, maxTransitions = MAX_FOCUS_TRANSITIONS, deadlineMs = 8000 } = {}) => {
    if (journey) cleanupJourney('replaced');
    const cap = Math.max(1, Math.min(Number(maxTransitions) || 0, MAX_FOCUS_TRANSITIONS));
    const abort = new AbortController();
    const overlay = createOverlay();
    journey = {
      generation: Number(documentGeneration),
      route: canonicalRoute(),
      maxTransitions: cap,
      transitions: [],
      candidates: focusableCandidates(),
      visited: new Set(),
      problemRefs: new Set(),
      closedCycle: false,
      abort,
      overlay,
      freezeObserver: new MutationObserver(syncOverlayFreeze),
      deadlineTimer: 0,
      initial: null,
    };
    journey.initial = focusObservation(documentGeneration, 'initial');
    if (journey.initial.reference) journey.visited.add(journey.initial.reference);
    const boundedDeadlineMs = Math.max(100, Math.min(Number(deadlineMs) || 8000, 30000));
    journey.deadlineTimer = setTimeout(() => {
      if (journey) cleanupJourney('deadline');
    }, boundedDeadlineMs);
    let pendingTabDirection = null;
    document.addEventListener('keydown', event => {
      if (!journey) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopImmediatePropagation();
        cleanupJourney('escape');
        return;
      }
      if (event.key === 'Tab') {
        pendingTabDirection = event.shiftKey ? 'shift_tab' : 'tab';
      }
    }, { capture: true, signal: abort.signal });
    document.addEventListener('focusin', () => {
      if (!journey || !pendingTabDirection) return;
      const direction = pendingTabDirection;
      pendingTabDirection = null;
      recordKeyboardFocus({ documentGeneration: journey.generation, direction });
    }, { capture: true, signal: abort.signal });
    document.addEventListener('keyup', event => {
      if (!journey || event.key !== 'Tab' || !pendingTabDirection) return;
      const direction = pendingTabDirection;
      pendingTabDirection = null;
      queueMicrotask(() => {
        if (journey) recordKeyboardFocus({ documentGeneration: journey.generation, direction });
      });
    }, { capture: true, signal: abort.signal });
    journey.freezeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-localview-visual-freeze'] });
    renderFocusOverlay();
    return {
      status: 'armed',
      route: journey.route,
      document_generation: journey.generation,
      max_transitions: cap,
      deadline_ms: boundedDeadlineMs,
      initial: journey.initial,
    };
  };

  const recordKeyboardFocus = ({ documentGeneration, direction = 'tab' } = {}) => {
    if (!journey) return { status: 'failed', reason: 'journey_not_active' };
    if (canonicalRoute() !== journey.route) {
      return { status: 'failed', reason: 'route_drift', receipt: cleanupJourney('route_drift') };
    }
    if (Number(documentGeneration) !== journey.generation) {
      return { status: 'failed', reason: 'document_generation_drift', receipt: cleanupJourney('document_generation_drift') };
    }
    if (journey.transitions.length >= journey.maxTransitions) {
      return { status: 'failed', reason: 'transition_cap', receipt: cleanupJourney('transition_cap') };
    }
    const observation = focusObservation(documentGeneration, direction);
    journey.transitions.push(observation);
    if (observation.reference) {
      if (journey.visited.has(observation.reference)) journey.closedCycle = true;
      journey.visited.add(observation.reference);
    }
    renderFocusOverlay();
    return { status: 'recorded', observation };
  };

  const markFocusProblem = reference => {
    if (!journey || !validStableRef(reference)) return false;
    journey.problemRefs.add(reference);
    renderFocusOverlay();
    return true;
  };

  const finishKeyboardJourney = () => journey ? cleanupJourney(null) : { status: 'failed', reason: 'journey_not_active' };
  const cancelKeyboardJourney = (reason = 'cancelled') => cleanupJourney(bounded(reason, 64));

  const intersectRect = (a, b) => {
    const left = Math.max(a.left, b.left);
    const top = Math.max(a.top, b.top);
    const right = Math.min(a.right, b.right);
    const bottom = Math.min(a.bottom, b.bottom);
    if (right <= left || bottom <= top) return null;
    return { left, top, right, bottom, width: right - left, height: bottom - top };
  };

  const serialRect = rect => rect ? ({
    x: Number(rect.left ?? rect.x), y: Number(rect.top ?? rect.y),
    width: Number(rect.width), height: Number(rect.height),
  }) : null;

  const effectiveHitbox = reference => {
    const element = resolveStableRef(reference);
    if (!element) return { status: 'unresolved', reference };
    const nominal = element.getBoundingClientRect();
    let clipped = { left: nominal.left, top: nominal.top, right: nominal.right, bottom: nominal.bottom, width: nominal.width, height: nominal.height };
    clipped = intersectRect(clipped, { left: 0, top: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight });
    let ancestor = element.parentElement;
    let depth = 0;
    while (clipped && ancestor && depth < 32) {
      depth += 1;
      const style = getComputedStyle(ancestor);
      if (/(hidden|clip|auto|scroll)/.test(String(style.overflow) + String(style.overflowX) + String(style.overflowY))) {
        const rect = ancestor.getBoundingClientRect();
        clipped = intersectRect(clipped, { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height });
      }
      ancestor = ancestor.parentElement;
    }
    const pointerEvents = getComputedStyle(element).pointerEvents !== 'none';
    const samples = [];
    if (clipped && clipped.width > 0 && clipped.height > 0) {
      const points = [
        [clipped.left + clipped.width / 2, clipped.top + clipped.height / 2],
        [clipped.left + 1, clipped.top + 1], [clipped.right - 1, clipped.top + 1],
        [clipped.left + 1, clipped.bottom - 1], [clipped.right - 1, clipped.bottom - 1],
      ];
      for (const point of points) {
        const stack = document.elementsFromPoint(point[0], point[1]).filter(node => !node.closest?.('[data-localview-owned]'));
        const top = stack[0] || null;
        const delivered = !!top && (top === element || element.contains(top));
        const blocker = !delivered && top ? base.refFor(top) : null;
        samples.push({ delivered, blocker_ref: validStableRef(blocker) ? blocker : null });
      }
    }
    const deliveredCount = samples.filter(sample => sample.delivered).length;
    return {
      status: 'complete',
      reference,
      authority: 'browser_hit_test',
      nominal: serialRect(nominal),
      effective: serialRect(clipped),
      pointer_events: pointerEvents,
      delivery_observed: samples.length ? deliveredCount > 0 : false,
      fully_blocked: samples.length ? deliveredCount === 0 : true,
      occluders: Array.from(new Set(samples.map(sample => sample.blocker_ref).filter(validStableRef))).slice(0, 16),
      sampled_points: samples.length,
    };
  };

  const accessibilityState = element => element ? ({
    disabled: element.matches?.(':disabled') || element.getAttribute('aria-disabled') === 'true',
    selected: element.getAttribute('aria-selected') === 'true',
    expanded: element.getAttribute('aria-expanded') === 'true',
    pressed: element.getAttribute('aria-pressed') === 'true',
    checked: element.matches?.(':checked') || element.getAttribute('aria-checked') === 'true',
  }) : null;

  const classifySafety = reference => {
    const element = resolveStableRef(reference);
    if (!element) return 'unresolved';
    if (element.matches('[type="submit"],[formaction]')) return 'destructive_or_unknown';
    if (element.matches('[readonly],[aria-readonly="true"]')) return 'read_only';
    return 'unknown';
  };

  const safeDiscoveryTargets = (maxTargets = MAX_DISCOVERY_TARGETS) => {
    const cap = Math.max(1, Math.min(Number(maxTargets) || 0, MAX_DISCOVERY_TARGETS));
    const targets = [];
    let scanned = 0;
    for (const element of document.querySelectorAll('a[href],button,input,select,textarea,[tabindex]')) {
      scanned += 1;
      if (scanned > 256 || targets.length >= cap) break;
      if (element.closest?.('[data-localview-owned]')) continue;
      const reference = base.refFor(element);
      if (!validStableRef(reference)) continue;
      const safety = classifySafety(reference);
      targets.push({
        reference,
        safety,
        probe_allowed: safety === 'read_only',
      });
    }
    return targets;
  };

  const beginFeedbackProbe = ({ reference, documentGeneration, safety, deadlineMs = 800, delayedThresholdMs = 250 } = {}) => {
    if (feedbackProbe) { feedbackProbe.observer.disconnect(); feedbackProbe = null; }
    const pageClassification = classifySafety(reference);
    if (safety !== 'explicitly_safe' && safety !== 'read_only') {
      return { status: 'skipped', reason: 'trusted_safety_required', safety: pageClassification };
    }
    const element = resolveStableRef(reference);
    if (!element) return { status: 'inconclusive', reason: 'stable_ref_invalid' };
    const beforeState = stateIdentity(documentGeneration);
    const focusRef = base.refFor(document.activeElement);
    const beforeFocus = validStableRef(focusRef) ? focusRef : null;
    const beforeResources = performance.getEntriesByType?.('resource')?.length || 0;
    const beforeA11y = accessibilityState(element);
    const observerState = { semantic: false, layout: false };
    const observer = new MutationObserver(records => {
      if (records.length) observerState.semantic = true;
      if (records.some(record => record.type === 'attributes' && ['style', 'class', 'hidden'].includes(record.attributeName))) observerState.layout = true;
    });
    observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true });
    feedbackProbe = {
      reference, generation: Number(documentGeneration), startedAt: performance.now(),
      deadlineMs: Math.max(1, Math.min(Number(deadlineMs) || 800, 5000)),
      delayedThresholdMs: Math.max(0, Math.min(Number(delayedThresholdMs) || 250, 5000)),
      safety, beforeState, beforeFocus, beforeResources, beforeA11y, observer, observerState,
    };
    return { status: 'armed', safety, state: beforeState };
  };

  const finishFeedbackProbe = ({ documentGeneration } = {}) => {
    if (!feedbackProbe) return { status: 'inconclusive', reason: 'probe_not_active' };
    const probe = feedbackProbe;
    feedbackProbe = null;
    probe.observer.disconnect();
    const elapsedMs = Math.max(0, Math.round(performance.now() - probe.startedAt));
    if (Number(documentGeneration) !== probe.generation) return { status: 'inconclusive', reason: 'document_generation_drift', elapsed_ms: elapsedMs };
    const element = resolveStableRef(probe.reference);
    if (!element) return { status: 'inconclusive', reason: 'stable_ref_invalid', elapsed_ms: elapsedMs };
    const afterState = stateIdentity(documentGeneration);
    const currentFocusRef = base.refFor(document.activeElement);
    const afterFocus = validStableRef(currentFocusRef) ? currentFocusRef : null;
    const afterResources = performance.getEntriesByType?.('resource')?.length || 0;
    const afterA11y = accessibilityState(element);
    const signals = {
      focus_changed: probe.beforeFocus !== afterFocus,
      semantic_changed: probe.observerState.semantic || probe.beforeState.semantic_fingerprint !== afterState.semantic_fingerprint,
      route_changed: probe.beforeState.route !== afterState.route,
      network_activity: afterResources > probe.beforeResources,
      layout_changed: probe.observerState.layout,
      accessible_state_changed: JSON.stringify(probe.beforeA11y) !== JSON.stringify(afterA11y),
    };
    const any = Object.values(signals).some(Boolean);
    const verdict = elapsedMs > probe.deadlineMs ? 'inconclusive'
      : any && elapsedMs > probe.delayedThresholdMs ? 'delayed_feedback'
      : any ? 'observed_feedback' : 'no_observed_feedback';
    return { status: 'complete', verdict, elapsed_ms: elapsedMs, safety: probe.safety, signals, before_state: probe.beforeState, after_state: afterState };
  };

  const validateReplayStep = ({ target, expectedState, documentGeneration } = {}) => {
    const actualState = stateIdentity(documentGeneration);
    if (!stateMatches(expectedState, actualState)) return { status: 'failed', reason: 'state_mismatch', actual_state: actualState };
    if (!resolveStableRef(target)) return { status: 'failed', reason: 'stable_ref_invalid', actual_state: actualState };
    return { status: 'ready', target, actual_state: actualState, safety: classifySafety(target) };
  };

  window.__LOCALVIEW_WAVE6__ = Object.freeze({
    version: '0.1.0',
    installAxe, runAccessibilityScan, localDeterministicChecks, stateIdentity,
    beginKeyboardJourney, recordKeyboardFocus, markFocusProblem,
    finishKeyboardJourney, cancelKeyboardJourney, effectiveHitbox,
    classifySafety, safeDiscoveryTargets, beginFeedbackProbe,
    finishFeedbackProbe, validateReplayStep,
  });
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_never_contains_remote_axe_loader() {
        let script = wave6_bootstrap_script();
        assert!(!script.contains("cdn.jsdelivr"));
        assert!(!script.contains("unpkg.com"));
        assert!(!script.contains("https://cdnjs"));
        assert!(script.contains("wave6_axe_not_installed_local"));
    }

    #[test]
    fn bootstrap_has_hard_caps_and_privacy_safe_receipts() {
        let script = wave6_bootstrap_script();
        assert!(script.contains("MAX_AXE_FINDINGS = 128"));
        assert!(script.contains("MAX_FOCUS_TRANSITIONS = 64"));
        assert!(script.contains("MAX_SCAN_ELEMENTS = 4096"));
        for forbidden in [
            "innerHTML",
            "element.value",
            "target.value",
            "input.value",
            "textarea.value",
            "valueAsDate",
            "valueAsNumber",
            "getAttribute('value')",
            "getAttribute(\"value\")",
            "document.cookie",
            "localStorage",
            "sessionStorage",
        ] {
            assert!(
                !script.contains(forbidden),
                "Wave 6 bootstrap must not read sensitive page data via {forbidden}"
            );
        }
    }
}
