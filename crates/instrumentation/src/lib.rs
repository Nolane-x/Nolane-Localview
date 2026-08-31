#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentationConfig {
    pub max_events: usize,
    pub max_interactive_nodes: usize,
    pub max_semantic_nodes: usize,
    pub max_tree_depth: usize,
    pub max_style_nodes: usize,
    pub max_geometry_nodes: usize,
    pub max_occlusion_samples: usize,
    pub include_console: bool,
    pub include_network: bool,
    pub include_performance: bool,
    pub include_scroll: bool,
}

impl Default for InstrumentationConfig {
    fn default() -> Self {
        Self {
            max_events: 1_024,
            max_interactive_nodes: 1_000,
            max_semantic_nodes: 600,
            max_tree_depth: 12,
            max_style_nodes: 192,
            max_geometry_nodes: 384,
            max_occlusion_samples: 128,
            include_console: true,
            include_network: true,
            include_performance: true,
            include_scroll: true,
        }
    }
}

pub fn bootstrap_script(config: &InstrumentationConfig) -> String {
    let config = serde_json::to_string(config).expect("instrumentation config is serializable");
    SCRIPT
        .replace("__LOCALVIEW_CONFIG__", &config)
        .replace(
            "  const snapshot = () => {",
            r#"  const readinessPacket = () => {
    const images = Array.from(document.images || []);
    return {
      fonts: document.fonts?.status || 'unsupported',
      pendingImages: images.reduce((count, image) => count + (image.complete ? 0 : 1), 0),
      totalImages: images.length,
      inflightRequests: config.include_network ? inflightNetworkRequests : null,
    };
  };

  const snapshot = () => {"#,
        )
        .replace(
            "      readyState: document.readyState,",
            "      readyState: document.readyState,\n      readiness: readinessPacket(),",
        )
        .replace(
            "  window.__LOCALVIEW__ = Object.freeze({",
            r#"  const VISUAL_FREEZE_LEASE_MS = 8000;
  let visualFreezeLease = null;

  const restoreVisuals = (token) => {
    token = String(token || '');
    const lease = visualFreezeLease;
    if (!lease || lease.token !== token) throw new Error('visual_freeze_token_mismatch');

    clearTimeout(lease.timer);
    if (lease.style?.isConnected) lease.style.remove();
    const root = document.documentElement;
    if (root?.getAttribute('data-localview-visual-freeze') === token) {
      root.removeAttribute('data-localview-visual-freeze');
    }
    for (const record of lease.animations) {
      if (!record.resume) continue;
      try { record.animation.play(); } catch (_) {}
    }
    visualFreezeLease = null;
    return { restored: true };
  };

  const freezeVisuals = async (token) => {
    token = String(token || '');
    if (!token) throw new Error('visual_freeze_token_required');
    if (visualFreezeLease) {
      const lease = visualFreezeLease;
      if (lease.token !== token) throw new Error('visual_freeze_already_active');
      return {
        paused_animations: lease.pausedAnimations,
        web_animations_supported: lease.webAnimationsSupported,
      };
    }

    const root = document.documentElement;
    if (!root) throw new Error('visual_freeze_root_unavailable');
    const webAnimationsSupported = typeof document.getAnimations === 'function';
    const animations = [];
    let pausedAnimations = 0;
    if (webAnimationsSupported) {
      for (const animation of Array.from(document.getAnimations()).slice(0, 2048)) {
        const playState = animation.playState;
        const resume = playState === 'running' || playState === 'pending';
        if (resume) {
          try {
            animation.pause();
            pausedAnimations += 1;
          } catch (_) {}
        }
        animations.push({ animation, resume });
      }
    }

    const style = document.createElement('style');
    style.setAttribute('data-localview-visual-freeze', '');
    style.textContent = `
html[data-localview-visual-freeze],
html[data-localview-visual-freeze] *,
html[data-localview-visual-freeze] *::before,
html[data-localview-visual-freeze] *::after {
  animation-play-state: paused !important;
  transition-duration: 0s !important;
  transition-delay: 0s !important;
  caret-color: transparent !important;
  scroll-behavior: auto !important;
}`;
    root.setAttribute('data-localview-visual-freeze', token);
    (document.head || root).appendChild(style);

    const lease = {
      token,
      style,
      animations,
      pausedAnimations,
      webAnimationsSupported,
      timer: 0,
    };
    visualFreezeLease = lease;
    lease.timer = setTimeout(() => {
      if (visualFreezeLease?.token !== token) return;
      try { restoreVisuals(token); } catch (_) {}
    }, VISUAL_FREEZE_LEASE_MS);

    await new Promise(resolve => requestAnimationFrame(() => resolve()));
    if (visualFreezeLease?.token !== token) throw new Error('visual_freeze_lease_lost');
    return {
      paused_animations: pausedAnimations,
      web_animations_supported: webAnimationsSupported,
    };
  };

  window.__LOCALVIEW__ = Object.freeze({"#,
        )
        .replace(
            "    snapshot,\n    inspect(reference)",
            "    snapshot,\n    freezeVisuals,\n    restoreVisuals,\n    inspect(reference)",
        )
}

const SCRIPT: &str = r#"
(() => {
  if (window.__LOCALVIEW__) return;

  const config = __LOCALVIEW_CONFIG__;
  const events = [];
  const refs = new WeakMap();
  const elementsByRef = new Map();
  const geometryBaseline = new Map();
  let sequence = 0;
  let snapshotVersion = 0;
  let mutationFlushQueued = false;
  let geometryFlushQueued = false;
  let routeSnapshotTimer = 0;
  let lastSnapshot = null;
  let inflightNetworkRequests = 0;
  const changedRefs = new Set();

  const beginNetworkRequest = () => {
    if (inflightNetworkRequests < Number.MAX_SAFE_INTEGER) inflightNetworkRequests += 1;
  };
  const finishNetworkRequest = () => {
    inflightNetworkRequests = Math.max(0, inflightNetworkRequests - 1);
  };

  const redact = (value) => String(value ?? '')
    .replace(/(authorization\s*[:=]\s*(?:bearer\s+)?)[^\s,;]+/ig, '$1[REDACTED]')
    .replace(/((?:api[_-]?key|token|password|secret)\s*[:=]\s*)[^\s,;]+/ig, '$1[REDACTED]')
    .slice(0, 1500);

  const safeUrl = (value) => {
    try {
      const url = new URL(String(value ?? ''), location.href);
      const sensitive = /token|key|secret|password|authorization/i;
      for (const key of Array.from(url.searchParams.keys())) {
        if (sensitive.test(key)) url.searchParams.set(key, '[REDACTED]');
      }
      url.hash = '';
      return redact(url.toString()).slice(0, 1000);
    } catch (_) {
      return redact(value).slice(0, 1000);
    }
  };

  const push = (type, payload = {}) => {
    events.push({
      seq: ++sequence,
      type,
      at: performance.now(),
      route: safeUrl(location.href),
      ...payload,
    });
    if (events.length > config.max_events) {
      events.splice(0, events.length - config.max_events);
    }
  };

  const fnv = (input) => {
    let hash = 0x811c9dc5;
    for (let i = 0; i < input.length; i++) {
      hash ^= input.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193);
    }
    return (hash >>> 0).toString(16);
  };

  const roleOf = (el) => el.getAttribute?.('role') || ({
    A: 'link', BUTTON: 'button', INPUT: 'textbox', SELECT: 'combobox',
    TEXTAREA: 'textbox', IMG: 'img', NAV: 'navigation', MAIN: 'main',
    HEADER: 'banner', FOOTER: 'contentinfo', FORM: 'form', ARTICLE: 'article',
    ASIDE: 'complementary', SECTION: 'region', H1: 'heading', H2: 'heading',
    H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading', DIALOG: 'dialog',
    TABLE: 'table', UL: 'list', OL: 'list', LI: 'listitem'
  })[el.tagName] || null;

  const boundedText = (value, max = 512) => redact(String(value ?? '').slice(0, max))
    .replace(/\s+/g, ' ')
    .trim();

  const textNameAllowed = (el) => {
    const role = roleOf(el);
    return ['A', 'BUTTON', 'SUMMARY', 'LABEL', 'OPTION', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6'].includes(el.tagName)
      || ['button', 'link', 'heading', 'tab', 'menuitem', 'option', 'checkbox', 'radio', 'switch'].includes(role)
      || !!el.isContentEditable;
  };

  const nameOf = (el) => {
    const labelled = el.getAttribute?.('aria-label');
    if (labelled) return boundedText(labelled, 180).slice(0, 180) || null;
    const labelledBy = el.getAttribute?.('aria-labelledby');
    if (labelledBy) {
      const text = labelledBy.split(/\s+/).map(id => document.getElementById(id)?.textContent || '').join(' ').trim();
      if (text) return boundedText(text, 512).slice(0, 180) || null;
    }
    if (el.labels?.length) {
      const text = Array.from(el.labels).map(label => label.textContent || '').join(' ');
      if (text) return boundedText(text, 512).slice(0, 180) || null;
    }
    if (el.tagName === 'BUTTON' && el.value) return boundedText(el.value, 180).slice(0, 180) || null;
    if (el.tagName === 'INPUT') {
      const type = String(el.type || 'text').toLowerCase();
      if (['button', 'submit', 'reset'].includes(type) && el.value) return boundedText(el.value, 180).slice(0, 180) || null;
      const placeholder = el.getAttribute?.('placeholder');
      return placeholder ? boundedText(placeholder, 180).slice(0, 180) || null : null;
    }
    if (['TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable) {
      const placeholder = el.getAttribute?.('placeholder');
      if (placeholder) return boundedText(placeholder, 180).slice(0, 180) || null;
    }
    if (el.tagName === 'IMG' && el.alt) return boundedText(el.alt, 180).slice(0, 180) || null;
    if (textNameAllowed(el)) {
      const text = boundedText(el.textContent, 512);
      if (text) return text.slice(0, 180);
    }
    const title = el.getAttribute?.('title');
    return title ? boundedText(title, 180).slice(0, 180) || null : null;
  };

  const descriptionOf = (el) => {
    const ids = el.getAttribute?.('aria-describedby');
    if (ids) {
      const text = ids.split(/\s+/).map(id => document.getElementById(id)?.textContent || '').join(' ');
      if (text) return boundedText(text, 640).slice(0, 240) || null;
    }
    const title = el.getAttribute?.('title');
    return title ? boundedText(title, 240).slice(0, 240) || null : null;
  };

  const ancestry = (el) => {
    const parts = [];
    let cursor = el;
    for (let depth = 0; cursor && depth < 6; depth++, cursor = cursor.parentElement) {
      let part = cursor.tagName?.toLowerCase() || 'node';
      if (cursor.id) part += '#' + redact(cursor.id).slice(0, 80);
      const testId = cursor.getAttribute?.('data-testid');
      if (testId) part += '[testid=' + redact(testId).slice(0, 80) + ']';
      parts.push(part);
    }
    return parts.reverse().join('>');
  };

  const refFor = (el) => {
    if (!el || el.nodeType !== Node.ELEMENT_NODE) return null;
    if (refs.has(el)) return refs.get(el);
    const signature = [roleOf(el) || '', nameOf(el) || '', el.tagName || '', ancestry(el)].join('|');
    const ref = '@e' + fnv(signature);
    refs.set(el, ref);
    if (elementsByRef.size < config.max_semantic_nodes * 2) elementsByRef.set(ref, el);
    return ref;
  };

  const rectOf = (el) => {
    const r = el.getBoundingClientRect();
    return {
      x: Math.round(r.x * 10) / 10,
      y: Math.round(r.y * 10) / 10,
      width: Math.round(r.width * 10) / 10,
      height: Math.round(r.height * 10) / 10,
    };
  };

  const documentRect = (el) => {
    const r = el.getBoundingClientRect();
    return {
      x: Math.round((r.x + scrollX) * 10) / 10,
      y: Math.round((r.y + scrollY) * 10) / 10,
      width: Math.round(r.width * 10) / 10,
      height: Math.round(r.height * 10) / 10,
    };
  };

  const interactiveSelector = [
    'a[href]', 'button', 'input', 'select', 'textarea', 'summary',
    '[role="button"]', '[role="link"]', '[role="textbox"]', '[tabindex]'
  ].join(',');

  const isInteractive = (el) => {
    try { return el.matches(interactiveSelector); } catch (_) { return false; }
  };

  const boolAttr = (el, name) => {
    const value = el.getAttribute?.(name);
    if (value === null || value === undefined) return null;
    if (value === '' || value === name || value === 'true') return true;
    if (value === 'false') return false;
    return redact(value).slice(0, 80);
  };

  const statePacket = (el, style) => ({
    disabled: !!el.disabled || boolAttr(el, 'aria-disabled') === true,
    checked: typeof el.checked === 'boolean' ? el.checked : boolAttr(el, 'aria-checked'),
    pressed: boolAttr(el, 'aria-pressed'),
    selected: typeof el.selected === 'boolean' ? el.selected : boolAttr(el, 'aria-selected'),
    expanded: boolAttr(el, 'aria-expanded'),
    required: !!el.required || boolAttr(el, 'aria-required') === true,
    readonly: !!el.readOnly || boolAttr(el, 'aria-readonly') === true,
    invalid: boolAttr(el, 'aria-invalid'),
    focused: document.activeElement === el,
    focusable: isInteractive(el) || Number(el.tabIndex) >= 0,
    hidden: !!el.hidden || style.visibility === 'hidden' || style.display === 'none' || style.opacity === '0',
  });

  const safeAttributes = (el) => {
    const output = {};
    const allowed = new Set(['id', 'role', 'type', 'name', 'data-testid', 'aria-label', 'aria-labelledby', 'aria-describedby', 'aria-live', 'aria-current', 'aria-haspopup', 'aria-controls']);
    for (const attr of Array.from(el.attributes || [])) {
      if (allowed.has(attr.name) || attr.name.startsWith('aria-')) {
        if (/value|password|secret|token|key/i.test(attr.name)) continue;
        output[attr.name] = redact(attr.value).slice(0, 160);
      }
    }
    if (el.tagName === 'A' && el.getAttribute('href')) output.href = safeUrl(el.href);
    return output;
  };

  const sourceHint = (el) => {
    for (const attribute of ['data-component-source', 'data-source']) {
      const raw = el.getAttribute?.(attribute);
      if (!raw) continue;
      const value = redact(raw).trim().slice(0, 320);
      if (!value) continue;
      const match = value.match(/^(.*?)(?::(\d+))?(?::(\d+))?$/);
      return {
        origin: attribute,
        file: (match?.[1] || value).slice(0, 260),
        line: match?.[2] ? Number(match[2]) : null,
        column: match?.[3] ? Number(match[3]) : null,
      };
    }
    return null;
  };

  const STYLE_PROPERTIES = [
    'display', 'position', 'overflowX', 'overflowY', 'boxSizing', 'zIndex',
    'flexDirection', 'flexWrap', 'justifyContent', 'alignItems', 'gap', 'rowGap', 'columnGap',
    'gridTemplateColumns', 'gridTemplateRows',
    'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
    'marginTop', 'marginRight', 'marginBottom', 'marginLeft',
    'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
    'fontSize', 'fontWeight', 'fontFamily', 'lineHeight', 'color', 'backgroundColor',
    'opacity', 'pointerEvents', 'visibility'
  ];

  const computedStylePacket = (el) => {
    const style = getComputedStyle(el);
    const packet = {};
    for (const property of STYLE_PROPERTIES) {
      const value = style[property];
      if (value !== undefined && value !== '') packet[property] = redact(value).slice(0, 180);
    }
    return packet;
  };

  const rectIntersects = (a, b) =>
    a.right > b.left && a.left < b.right && a.bottom > b.top && a.top < b.bottom;

  const ancestorClips = (el, rect) => {
    let cursor = el.parentElement;
    for (let depth = 0; cursor && depth < 10; depth++, cursor = cursor.parentElement) {
      const style = getComputedStyle(cursor);
      const clipsX = ['hidden', 'clip', 'auto', 'scroll'].includes(style.overflowX);
      const clipsY = ['hidden', 'clip', 'auto', 'scroll'].includes(style.overflowY);
      if (!clipsX && !clipsY) continue;
      const boundary = cursor.getBoundingClientRect();
      if ((clipsX && (rect.left < boundary.left || rect.right > boundary.right)) ||
          (clipsY && (rect.top < boundary.top || rect.bottom > boundary.bottom))) {
        return true;
      }
    }
    return false;
  };

  const visibilityPacket = (el, style, budget) => {
    const rect = el.getBoundingClientRect();
    const viewport = { left: 0, top: 0, right: innerWidth, bottom: innerHeight };
    const inViewport = rect.width > 0 && rect.height > 0 && rectIntersects(rect, viewport);
    const clipped = inViewport ? ancestorClips(el, rect) : false;
    let occluded = false;
    let occludedBy = null;
    let sampled = false;

    if (inViewport && !el.hidden && style.visibility !== 'hidden' && style.display !== 'none' &&
        style.opacity !== '0' && budget.remaining > 0) {
      budget.remaining -= 1;
      sampled = true;
      const x = Math.max(0, Math.min(innerWidth - 1, rect.left + rect.width / 2));
      const y = Math.max(0, Math.min(innerHeight - 1, rect.top + rect.height / 2));
      const stack = typeof document.elementsFromPoint === 'function' ? document.elementsFromPoint(x, y) : [];
      const blocker = stack.find(candidate =>
        candidate !== el && !el.contains(candidate) && !candidate.contains?.(el)
      );
      if (blocker) {
        occluded = true;
        occludedBy = refFor(blocker);
      }
    }

    return { inViewport, clipped, occluded, occludedBy, sampled };
  };

  const compactSemanticNode = (el, includeStyle, occlusionBudget) => {
    const style = getComputedStyle(el);
    return {
      ref: refFor(el),
      tag: el.tagName.toLowerCase(),
      role: roleOf(el),
      name: nameOf(el),
      description: descriptionOf(el),
      rect: rectOf(el),
      documentRect: documentRect(el),
      interactive: isInteractive(el),
      states: statePacket(el, style),
      visibility: visibilityPacket(el, style, occlusionBudget),
      sourceHint: sourceHint(el),
      attributes: safeAttributes(el),
      style: includeStyle ? computedStylePacket(el) : null,
    };
  };

  const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'META', 'LINK', 'HEAD']);

  const semanticTree = (occlusionBudget) => {
    const root = document.body || document.documentElement;
    if (!root) return null;
    let nodes = 0;
    let styled = 0;

    const visit = (el, depth) => {
      if (!el || el.nodeType !== Node.ELEMENT_NODE || SKIP_TAGS.has(el.tagName)) return null;
      if (nodes >= config.max_semantic_nodes || depth > config.max_tree_depth) return null;
      nodes += 1;
      const includeStyle = styled < config.max_style_nodes && (isInteractive(el) || depth <= 3);
      if (includeStyle) styled += 1;
      const node = compactSemanticNode(el, includeStyle, occlusionBudget);
      node.children = [];
      for (const child of Array.from(el.children || [])) {
        if (nodes >= config.max_semantic_nodes) break;
        const next = visit(child, depth + 1);
        if (next) node.children.push(next);
      }
      return node;
    };

    return visit(root, 0);
  };

  const flattenTree = (root, out = new Map()) => {
    if (!root) return out;
    out.set(root.ref, root);
    for (const child of root.children || []) flattenTree(child, out);
    return out;
  };

  const rectEqual = (a, b) => !!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  const semanticSignature = (node) => JSON.stringify([
    node.tag, node.role, node.name, node.description, node.interactive, node.states,
    node.visibility, node.sourceHint, node.attributes
  ]);

  const snapshotDelta = (before, after) => {
    if (!before) return {
      added_refs: Array.from(flattenTree(after.semantic_tree).keys()).slice(0, config.max_semantic_nodes),
      removed_refs: [],
      changed_refs: [],
      layout_changes: [],
      route_changed: false,
    };
    const oldNodes = flattenTree(before.semantic_tree);
    const newNodes = flattenTree(after.semantic_tree);
    const added_refs = [];
    const removed_refs = [];
    const changed_refs = [];
    const layout_changes = [];

    for (const [ref, node] of newNodes) {
      const old = oldNodes.get(ref);
      if (!old) {
        added_refs.push(ref);
        continue;
      }
      if (semanticSignature(old) !== semanticSignature(node)) changed_refs.push(ref);
      if (!rectEqual(old.documentRect, node.documentRect)) {
        layout_changes.push({ reference: ref, before: old.documentRect, after: node.documentRect });
      }
    }
    for (const ref of oldNodes.keys()) if (!newNodes.has(ref)) removed_refs.push(ref);

    return {
      added_refs: added_refs.slice(0, config.max_semantic_nodes),
      removed_refs: removed_refs.slice(0, config.max_semantic_nodes),
      changed_refs: changed_refs.slice(0, config.max_semantic_nodes),
      layout_changes: layout_changes.slice(0, config.max_geometry_nodes),
      route_changed: before.route !== after.route,
    };
  };

  const interactiveSnapshot = (occlusionBudget) => Array.from(document.querySelectorAll(interactiveSelector))
    .slice(0, config.max_interactive_nodes)
    .map((el) => compactSemanticNode(el, false, occlusionBudget));

  const snapshot = () => {
    snapshotVersion += 1;
    const occlusionBudget = { remaining: Math.max(0, Number(config.max_occlusion_samples) || 0) };
    const semantic_tree = semanticTree(occlusionBudget);
    const packet = {
      version: snapshotVersion,
      route: safeUrl(location.href),
      title: redact(document.title).slice(0, 240),
      readyState: document.readyState,
      viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
      scroll: { x: scrollX, y: scrollY },
      activeRef: refFor(document.activeElement),
      semantic_tree,
      interactive: interactiveSnapshot(occlusionBudget),
      occlusion: {
        max_samples: config.max_occlusion_samples,
        sampled: Math.max(0, Number(config.max_occlusion_samples) || 0) - occlusionBudget.remaining,
      },
    };
    packet.delta = snapshotDelta(lastSnapshot, packet);
    lastSnapshot = packet;
    push('semantic_snapshot', { snapshot: packet });
    return packet;
  };

  const resolveRef = (reference) => {
    if (!reference) return null;
    const cached = elementsByRef.get(reference);
    if (cached?.isConnected) return cached;
    for (const element of Array.from(document.querySelectorAll('*')).slice(0, config.max_semantic_nodes * 2)) {
      if (refFor(element) === reference) return element;
    }
    return null;
  };

  const inspect = (reference) => {
    const el = resolveRef(reference);
    if (!el) return null;
    const parents = [];
    let cursor = el.parentElement;
    for (let depth = 0; cursor && depth < 6; depth++, cursor = cursor.parentElement) {
      parents.push({ ref: refFor(cursor), tag: cursor.tagName.toLowerCase(), role: roleOf(cursor), name: nameOf(cursor) });
    }
    return {
      reference,
      node: compactSemanticNode(el, true, { remaining: 1 }),
      ancestry: parents,
      viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
      route: safeUrl(location.href),
    };
  };

  const sampleGeometry = (reason) => {
    geometryFlushQueued = false;
    const elements = Array.from(document.querySelectorAll(interactiveSelector)).slice(0, config.max_geometry_nodes);
    const current = new Map();
    const layout_changes = [];
    for (const el of elements) {
      const ref = refFor(el);
      if (!ref) continue;
      const rect = documentRect(el);
      current.set(ref, rect);
      const before = geometryBaseline.get(ref);
      if (before && !rectEqual(before, rect)) layout_changes.push({ reference: ref, before, after: rect });
    }
    for (const [ref, before] of geometryBaseline) {
      if (!current.has(ref)) layout_changes.push({ reference: ref, before, after: null });
    }
    geometryBaseline.clear();
    for (const [ref, rect] of current) geometryBaseline.set(ref, rect);
    if (layout_changes.length) push('geometry_changed', {
      reason,
      layout_changes: layout_changes.slice(0, config.max_geometry_nodes),
      truncated: layout_changes.length > config.max_geometry_nodes,
    });
  };

  const scheduleGeometry = (reason) => {
    if (geometryFlushQueued) return;
    geometryFlushQueued = true;
    requestAnimationFrame(() => sampleGeometry(reason));
  };

  const flushMutations = () => {
    mutationFlushQueued = false;
    if (!changedRefs.size) return;
    push('dom_changed', { refs: Array.from(changedRefs).filter(Boolean).slice(0, 256) });
    changedRefs.clear();
    scheduleGeometry('dom_mutation');
  };

  const startDomObservers = () => {
    const root = document.documentElement;
    if (!root) return;
    new MutationObserver((records) => {
      for (const record of records) {
        if (record.target?.nodeType === Node.ELEMENT_NODE) changedRefs.add(refFor(record.target));
        for (const node of record.addedNodes || []) {
          if (node.nodeType === Node.ELEMENT_NODE) changedRefs.add(refFor(node));
        }
      }
      if (!mutationFlushQueued) {
        mutationFlushQueued = true;
        queueMicrotask(flushMutations);
      }
    }).observe(root, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });

    if ('ResizeObserver' in window) {
      const resizeObserver = new ResizeObserver(() => scheduleGeometry('resize_observer'));
      resizeObserver.observe(document.documentElement);
      if (document.body) resizeObserver.observe(document.body);
    }
  };

  const scheduleRouteSnapshot = () => {
    clearTimeout(routeSnapshotTimer);
    routeSnapshotTimer = setTimeout(() => {
      try { snapshot(); } catch (_) {}
    }, 80);
  };

  const announceRoute = (source) => {
    push('route_changed', { source, href: safeUrl(location.href) });
    scheduleRouteSnapshot();
  };

  for (const method of ['pushState', 'replaceState']) {
    const original = history[method];
    history[method] = function(...args) {
      const result = original.apply(this, args);
      queueMicrotask(() => announceRoute(method));
      return result;
    };
  }
  addEventListener('popstate', () => announceRoute('popstate'));
  addEventListener('hashchange', () => announceRoute('hashchange'));
  addEventListener('focusin', (event) => push('focus_changed', { ref: refFor(event.target), focused: true }), true);
  addEventListener('focusout', (event) => push('focus_changed', { ref: refFor(event.target), focused: false }), true);
  addEventListener('resize', () => scheduleGeometry('viewport_resize'), { passive: true });

  if (config.include_scroll) {
    let scrollScheduled = false;
    addEventListener('scroll', () => {
      if (scrollScheduled) return;
      scrollScheduled = true;
      requestAnimationFrame(() => {
        scrollScheduled = false;
        push('scroll_changed', { x: scrollX, y: scrollY });
      });
    }, { passive: true, capture: true });
  }

  if (config.include_console) {
    for (const level of ['warn', 'error']) {
      const original = console[level].bind(console);
      console[level] = (...args) => {
        try {
          push('console', {
            level,
            message: redact(args.map(v => typeof v === 'string' ? v : v?.message || String(v)).join(' ')),
          });
        } catch (_) {}
        return original(...args);
      };
    }
    addEventListener('error', (event) => push('exception', {
      message: redact(event.message),
      source: safeUrl(event.filename),
      line: event.lineno,
      column: event.colno,
    }));
    addEventListener('unhandledrejection', (event) => push('unhandled_rejection', {
      message: redact(event.reason?.message || event.reason),
    }));
  }

  if (config.include_network) {
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const request = args[0];
      const init = args[1] || {};
      const method = String(init.method || request?.method || 'GET').toUpperCase();
      const url = safeUrl(request?.url || request);
      const started = performance.now();
      beginNetworkRequest();
      try {
        const response = await originalFetch(...args);
        finishNetworkRequest();
        push('network', {
          transport: 'fetch', method, url, status: response.status, ok: response.ok,
          duration: Math.round((performance.now() - started) * 10) / 10,
        });
        return response;
      } catch (error) {
        finishNetworkRequest();
        push('network', {
          transport: 'fetch', method, url, status: null, ok: false,
          duration: Math.round((performance.now() - started) * 10) / 10,
          error: redact(error?.message || error),
        });
        throw error;
      }
    };

    const xhrMeta = new WeakMap();
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function(method, url, ...rest) {
      xhrMeta.set(this, { method: String(method || 'GET').toUpperCase(), url: safeUrl(url), started: 0, active: false });
      return originalOpen.call(this, method, url, ...rest);
    };
    XMLHttpRequest.prototype.send = function(...args) {
      const meta = xhrMeta.get(this) || { method: 'GET', url: '', started: 0, active: false };
      const startedHere = !meta.active;
      meta.started = performance.now();
      if (startedHere) {
        meta.active = true;
        beginNetworkRequest();
      }
      xhrMeta.set(this, meta);
      this.addEventListener('loadend', () => {
        if (meta.active) {
          meta.active = false;
          finishNetworkRequest();
        }
        push('network', {
          transport: 'xhr', method: meta.method, url: meta.url,
          status: Number.isFinite(this.status) ? this.status : null,
          ok: this.status >= 200 && this.status < 400,
          duration: Math.round((performance.now() - meta.started) * 10) / 10,
        });
      }, { once: true });
      try {
        return originalSend.apply(this, args);
      } catch (error) {
        if (startedHere && meta.active) {
          meta.active = false;
          finishNetworkRequest();
        }
        throw error;
      }
    };
  }

  if (config.include_performance && 'PerformanceObserver' in window) {
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) push('long_task', { duration: entry.duration, startTime: entry.startTime });
      }).observe({ type: 'longtask', buffered: true });
    } catch (_) {}
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (!entry.hadRecentInput) push('layout_shift', { value: entry.value, startTime: entry.startTime });
        }
      }).observe({ type: 'layout-shift', buffered: true });
    } catch (_) {}
  }

  window.__LOCALVIEW__ = Object.freeze({
    version: '0.2.0',
    snapshot,
    inspect(reference) { return inspect(reference); },
    drain(max = 256) {
      const count = Math.max(0, Math.min(Number(max) || 0, events.length));
      return events.splice(0, count);
    },
    peek(max = 64) { return events.slice(-Math.max(0, Number(max) || 0)); },
    refFor,
  });

  const start = () => {
    startDomObservers();
    try { sampleGeometry('initial'); } catch (_) {}
    try { snapshot(); } catch (_) {}
    push('instrumentation_ready', { href: safeUrl(location.href) });
  };
  if (document.readyState === 'loading') {
    addEventListener('DOMContentLoaded', start, { once: true });
  } else {
    queueMicrotask(start);
  }
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_self_contained_bootstrap() {
        let script = bootstrap_script(&InstrumentationConfig::default());
        assert!(script.contains("window.__LOCALVIEW__"));
        assert!(script.contains("dom_changed"));
        assert!(script.contains("route_changed"));
        assert!(script.contains("push('network'"));
        assert!(!script.contains("__LOCALVIEW_CONFIG__"));
    }

    #[test]
    fn defaults_capture_metadata_without_bodies_or_live_form_values() {
        let script = bootstrap_script(&InstrumentationConfig::default());
        assert!(script.contains("include_network"));
        assert!(!script.contains("response.text()"));
        assert!(!script.contains("response.json()"));
        assert!(!script.contains("['BUTTON', 'INPUT'].includes(el.tagName) && el.value"));
        assert!(script.contains("el.isContentEditable"));
        assert!(script.contains("route: safeUrl(location.href)"));
    }

    #[test]
    fn semantic_defaults_are_bounded() {
        let config = InstrumentationConfig::default();
        assert_eq!(config.max_semantic_nodes, 600);
        assert_eq!(config.max_tree_depth, 12);
        assert_eq!(config.max_style_nodes, 192);
        assert_eq!(config.max_geometry_nodes, 384);
        assert_eq!(config.max_occlusion_samples, 128);
    }
}