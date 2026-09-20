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
    pub include_hmr: bool,
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
            include_hmr: true,
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
            r#"  const VIEWPORT_VISUAL_FREEZE_LEASE_MS = 8000;
  const FULL_PAGE_VISUAL_FREEZE_LEASE_MS = 30000;
  const MAX_POSITIONAL_SCAN_ELEMENTS = 4096;
  let visualFreezeLease = null;

  const documentGeometry = () => {
    const root = document.documentElement;
    const body = document.body;
    return {
      viewport_css_width: Number(window.innerWidth || root?.clientWidth || 0),
      viewport_css_height: Number(window.innerHeight || root?.clientHeight || 0),
      document_css_width: Math.max(
        Number(root?.scrollWidth || 0),
        Number(root?.clientWidth || 0),
        Number(body?.scrollWidth || 0),
        Number(body?.clientWidth || 0)
      ),
      document_css_height: Math.max(
        Number(root?.scrollHeight || 0),
        Number(root?.clientHeight || 0),
        Number(body?.scrollHeight || 0),
        Number(body?.clientHeight || 0)
      ),
    };
  };

  const restoreVisuals = (token) => {
    token = String(token || '');
    const lease = visualFreezeLease;
    if (!lease || lease.token !== token) throw new Error('visual_freeze_token_mismatch');

    clearTimeout(lease.timer);
    window.scrollTo({
      left: lease.originalScrollX,
      top: lease.originalScrollY,
      behavior: 'auto',
    });
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

  const freezeVisuals = async (token, leaseMs = VIEWPORT_VISUAL_FREEZE_LEASE_MS) => {
    token = String(token || '');
    if (!token) throw new Error('visual_freeze_token_required');
    if (leaseMs !== VIEWPORT_VISUAL_FREEZE_LEASE_MS &&
        leaseMs !== FULL_PAGE_VISUAL_FREEZE_LEASE_MS) {
      throw new Error('visual_freeze_lease_invalid');
    }
    if (visualFreezeLease) {
      const lease = visualFreezeLease;
      if (lease.token !== token) throw new Error('visual_freeze_already_active');
      return {
        paused_animations: lease.pausedAnimations,
        web_animations_supported: lease.webAnimationsSupported,
        scroll_x: lease.originalScrollX,
        scroll_y: lease.originalScrollY,
        document_css_width: lease.documentGeometry.document_css_width,
        document_css_height: lease.documentGeometry.document_css_height,
      };
    }

    const root = document.documentElement;
    if (!root) throw new Error('visual_freeze_root_unavailable');
    const geometry = documentGeometry();
    if (!Number.isFinite(geometry.viewport_css_width) || geometry.viewport_css_width <= 0 ||
        !Number.isFinite(geometry.viewport_css_height) || geometry.viewport_css_height <= 0 ||
        !Number.isFinite(geometry.document_css_width) || geometry.document_css_width <= 0 ||
        !Number.isFinite(geometry.document_css_height) || geometry.document_css_height <= 0) {
      throw new Error('visual_freeze_geometry_invalid');
    }
    const originalScrollX = Number(window.scrollX || 0);
    const originalScrollY = Number(window.scrollY || 0);
    if (!Number.isFinite(originalScrollX) || originalScrollX < 0 ||
        !Number.isFinite(originalScrollY) || originalScrollY < 0) {
      throw new Error('visual_freeze_scroll_invalid');
    }

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
      originalScrollX,
      originalScrollY,
      documentGeometry: geometry,
      timer: 0,
    };
    visualFreezeLease = lease;
    lease.timer = setTimeout(() => {
      if (visualFreezeLease?.token !== token) return;
      try { restoreVisuals(token); } catch (_) {}
    }, leaseMs);

    await new Promise(resolve => requestAnimationFrame(() => resolve()));
    if (visualFreezeLease?.token !== token) throw new Error('visual_freeze_lease_lost');
    return {
      paused_animations: pausedAnimations,
      web_animations_supported: webAnimationsSupported,
      scroll_x: originalScrollX,
      scroll_y: originalScrollY,
      document_css_width: geometry.document_css_width,
      document_css_height: geometry.document_css_height,
    };
  };

  const captureScrollTo = async (token, y) => {
    token = String(token || '');
    const lease = visualFreezeLease;
    if (!lease || lease.token !== token) throw new Error('visual_freeze_token_mismatch');
    y = Number(y);
    if (!Number.isFinite(y) || y < 0) throw new Error('full_page_scroll_invalid');

    const geometry = documentGeometry();
    const maxScrollY = Math.max(0, geometry.document_css_height - geometry.viewport_css_height);
    if (y > maxScrollY + 0.5) throw new Error('full_page_scroll_out_of_bounds');
    const targetY = Math.min(y, maxScrollY);
    window.scrollTo({
      left: lease.originalScrollX,
      top: targetY,
      behavior: 'auto',
    });
    await new Promise(resolve => requestAnimationFrame(() => resolve()));
    await new Promise(resolve => requestAnimationFrame(() => resolve()));
    if (visualFreezeLease?.token !== token) throw new Error('visual_freeze_lease_lost');
    const settledGeometry = documentGeometry();
    return {
      requested_y: y,
      actual_x: Number(window.scrollX || 0),
      actual_y: Number(window.scrollY || 0),
      ...settledGeometry,
    };
  };

  const captureTileProbe = async (token) => {
    token = String(token || '');
    const lease = visualFreezeLease;
    if (!lease || lease.token !== token) throw new Error('visual_freeze_token_mismatch');

    const geometry = documentGeometry();
    const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_ELEMENT);
    let positionalElementsScanned = 0;
    let visibleFixedOrSticky = false;
    let node = walker.currentNode;
    while (node) {
      positionalElementsScanned += 1;
      if (positionalElementsScanned > MAX_POSITIONAL_SCAN_ELEMENTS) {
        throw new Error('full_page_positional_scan_budget_exceeded');
      }
      const style = getComputedStyle(node);
      if (style.position === 'fixed' || style.position === 'sticky') {
        const rect = node.getBoundingClientRect();
        const visible = rect.width > 0 && rect.height > 0 &&
          style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0' &&
          rect.bottom > 0 && rect.right > 0 && rect.top < innerHeight && rect.left < innerWidth;
        if (visible) {
          visibleFixedOrSticky = true;
          break;
        }
      }
      node = walker.nextNode();
    }
    if (visualFreezeLease?.token !== token) throw new Error('visual_freeze_lease_lost');
    return {
      scroll_x: Number(window.scrollX || 0),
      scroll_y: Number(window.scrollY || 0),
      ...geometry,
      visible_fixed_or_sticky: visibleFixedOrSticky,
      positional_elements_scanned: positionalElementsScanned,
    };
  };

  window.__LOCALVIEW__ = Object.freeze({"#,
        )
        .replace(
            "    snapshot,\n    inspect(reference)",
            "    snapshot,\n    freezeVisuals,\n    restoreVisuals,\n    captureScrollTo,\n    captureTileProbe,\n    inspect(reference)",
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
  let networkFaultLease = null;
  const changedRefs = new Set();
  const localViewOwnedElements = new WeakSet();
  const pointSelectCompletions = [];
  let pointSelectState = null;

  const isLocalViewOwned = (element) => !!element && localViewOwnedElements.has(element);

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

  const NETWORK_FAULT_MAX_RULES = 16;
  const NETWORK_FAULT_MAX_PATH_BYTES = 256;
  const NETWORK_FAULT_MIN_LEASE_MS = 100;
  const NETWORK_FAULT_MAX_LEASE_MS = 30000;
  const NETWORK_FAULT_MAX_DELAY_MS = 5000;
  const NETWORK_FAULT_MAX_HITS = 64;
  const NETWORK_FAULT_METHODS = new Set(['get', 'head', 'post', 'put', 'patch', 'delete', 'options']);
  const NETWORK_FAULT_TRANSPORTS = new Set(['fetch', 'xhr', 'both']);

  const normalizeNetworkFaultTarget = (rawUrl) => {
    let faultUrl;
    try {
      faultUrl = new URL(String(rawUrl || ''), location.href);
    } catch (_) {
      return null;
    }
    if (faultUrl.protocol !== 'http:' && faultUrl.protocol !== 'https:') return null;
    const hostname = String(faultUrl.hostname || '').toLowerCase();
    const ipv4Parts = hostname.startsWith('127.') ? hostname.split('.') : [];
    const ipv4Loopback = ipv4Parts.length === 4 && ipv4Parts.every(part => {
      if (!/^\d{1,3}$/.test(part)) return false;
      const value = Number(part);
      return Number.isInteger(value) && value >= 0 && value <= 255;
    });
    const loopback = hostname === 'localhost'
      || hostname.endsWith('.localhost')
      || ipv4Loopback
      || hostname === '::1'
      || hostname === '[::1]';
    if (!loopback) return null;
    return { path: faultUrl.pathname || '/' };
  };

  const validateNetworkFaultPlan = (plan) => {
    if (!config.include_network) throw new Error('network_fault_instrumentation_disabled');
    if (!plan || typeof plan !== 'object') throw new Error('network_fault_invalid_plan');
    const fingerprint = String(plan.fingerprint || '');
    if (!/^[0-9a-f]{16}$/.test(fingerprint)) throw new Error('network_fault_invalid_fingerprint');
    const leaseMs = Number(plan.lease_ms);
    if (!Number.isInteger(leaseMs)
        || leaseMs < NETWORK_FAULT_MIN_LEASE_MS
        || leaseMs > NETWORK_FAULT_MAX_LEASE_MS) {
      throw new Error('network_fault_invalid_lease');
    }
    if (!Array.isArray(plan.rules)
        || plan.rules.length === 0
        || plan.rules.length > NETWORK_FAULT_MAX_RULES) {
      throw new Error('network_fault_invalid_rules');
    }

    const seen = new Set();
    const rules = plan.rules.map(input => {
      if (!input || typeof input !== 'object') throw new Error('network_fault_invalid_rule');
      const id = String(input.id || '');
      if (!/^[A-Za-z0-9_-]{1,64}$/.test(id)) throw new Error('network_fault_invalid_rule_id');
      const transport = String(input.transport || '').toLowerCase();
      if (!NETWORK_FAULT_TRANSPORTS.has(transport)) throw new Error('network_fault_invalid_transport');
      const method = String(input.method || '').toLowerCase();
      if (!NETWORK_FAULT_METHODS.has(method)) throw new Error('network_fault_invalid_method');
      const path = String(input.path || '');
      const pathBytes = new TextEncoder().encode(path).length;
      if (!path.startsWith('/')
          || path.startsWith('//')
          || path.includes('?')
          || path.includes('#')
          || /[\u0000-\u001f\u007f]/.test(path)
          || pathBytes === 0
          || pathBytes > NETWORK_FAULT_MAX_PATH_BYTES) {
        throw new Error('network_fault_invalid_path');
      }
      const maxHits = Number(input.max_hits);
      if (!Number.isInteger(maxHits) || maxHits < 1 || maxHits > NETWORK_FAULT_MAX_HITS) {
        throw new Error('network_fault_invalid_hit_budget');
      }
      const effectInput = input.effect;
      if (!effectInput || typeof effectInput !== 'object') throw new Error('network_fault_invalid_effect');
      const kind = String(effectInput.kind || '');
      let effect;
      if (kind === 'fail') {
        effect = { kind: 'fail' };
      } else if (kind === 'delay') {
        const milliseconds = Number(effectInput.milliseconds);
        if (!Number.isInteger(milliseconds)
            || milliseconds < 1
            || milliseconds > NETWORK_FAULT_MAX_DELAY_MS) {
          throw new Error('network_fault_invalid_delay');
        }
        effect = { kind: 'delay', milliseconds };
      } else if (kind === 'mock_status') {
        const status = Number(effectInput.status);
        if (!Number.isInteger(status) || status < 200 || status > 599) {
          throw new Error('network_fault_invalid_status');
        }
        effect = { kind: 'mock_status', status };
      } else {
        throw new Error('network_fault_invalid_effect');
      }

      const selector = [transport, method, path].join('|');
      if (seen.has(selector)) throw new Error('network_fault_duplicate_selector');
      seen.add(selector);
      return { id, transport, method, path, effect, max_hits: maxHits, hits: 0 };
    });

    return { fingerprint, lease_ms: leaseMs, rules };
  };

  const activeNetworkFaultLease = () => {
    if (!networkFaultLease) return null;
    if (performance.now() >= networkFaultLease.expiresAt) {
      networkFaultLease = null;
      return null;
    }
    return networkFaultLease;
  };

  const installNetworkFaultPlan = (token, plan) => {
    token = String(token || '');
    if (!/^[A-Za-z0-9_-]{16,128}$/.test(token)) throw new Error('network_fault_invalid_token');
    if (!normalizeNetworkFaultTarget(location.href)) throw new Error('network_fault_route_not_loopback');
    const canonical = validateNetworkFaultPlan(plan);
    networkFaultLease = {
      token,
      fingerprint: canonical.fingerprint,
      expiresAt: performance.now() + plan.lease_ms,
      rules: canonical.rules,
    };
    return {
      installed: true,
      fingerprint: canonical.fingerprint,
      rule_count: canonical.rules.length,
      expires_in_ms: canonical.lease_ms,
    };
  };

  const clearNetworkFaultPlan = (token) => {
    token = String(token || '');
    const lease = activeNetworkFaultLease();
    if (!lease) return { cleared: true, active: false };
    if (lease.token !== token) throw new Error('network_fault_token_mismatch');
    const fingerprint = lease.fingerprint;
    networkFaultLease = null;
    return { cleared: true, active: false, fingerprint };
  };

  const networkFaultState = () => {
    const lease = activeNetworkFaultLease();
    if (!lease) return { active: false, fingerprint: null, rule_count: 0, total_hits: 0 };
    return {
      active: true,
      fingerprint: lease.fingerprint,
      rule_count: lease.rules.length,
      total_hits: lease.rules.reduce((sum, rule) => sum + rule.hits, 0),
      remaining_ms: Math.max(0, Math.round(lease.expiresAt - performance.now())),
    };
  };

  const selectNetworkFaultRule = (requestedTransport, method, rawUrl, normalizedTarget = null) => {
    const lease = activeNetworkFaultLease();
    if (!lease) return null;
    const target = normalizedTarget || normalizeNetworkFaultTarget(rawUrl);
    if (!target) return null;
    const normalizedMethod = String(method || 'GET').toLowerCase();
    for (const rule of lease.rules) {
      const transport = rule.transport;
      if (transport !== 'both' && transport !== requestedTransport) continue;
      if (rule.method !== normalizedMethod || rule.path !== target.path) continue;
      if (rule.hits >= rule.max_hits) continue;
      rule.hits += 1;
      return rule;
    }
    return null;
  };

  const setSyntheticXhrValue = (xhr, name, value) => {
    try {
      Object.defineProperty(xhr, name, { configurable: true, value });
    } catch (_) {}
  };

  const clearSyntheticXhrValues = (xhr) => {
    for (const name of ['status', 'statusText', 'readyState', 'response', 'responseText', 'responseURL']) {
      try { delete xhr[name]; } catch (_) {}
    }
  };

  const completeSyntheticXhrFault = (xhr, meta, rule, success) => {
    const faultCompleted = Boolean(meta.faultCompleted);
    if (faultCompleted) return;
    meta.faultCompleted = true;
    meta.faultPending = false;
    if (meta.active) {
      meta.active = false;
      finishNetworkRequest();
    }

    const status = success && rule.effect.kind === 'mock_status' ? rule.effect.status : 0;
    Object.defineProperty(xhr, 'status', { configurable: true, value: status });
    Object.defineProperty(xhr, 'readyState', { configurable: true, value: 4 });
    setSyntheticXhrValue(xhr, 'statusText', '');
    setSyntheticXhrValue(xhr, 'response', '');
    setSyntheticXhrValue(xhr, 'responseText', '');
    setSyntheticXhrValue(xhr, 'responseURL', '');

    push('network', {
      transport: 'xhr',
      method: meta.method,
      url: meta.url,
      status: success ? status : null,
      ok: success && status >= 200 && status < 400,
      duration: Math.round((performance.now() - meta.started) * 10) / 10,
      error: success ? null : 'LocalView injected network failure',
      faultInjected: Boolean(rule),
      faultRuleId: rule?.id || null,
      faultEffect: rule?.effect?.kind || null,
      faultDelayMs: rule?.effect?.kind === 'delay' ? rule.effect.milliseconds : null,
      faultStatus: rule?.effect?.kind === 'mock_status' ? rule.effect.status : null,
    });

    xhr.dispatchEvent(new Event('readystatechange'));
    xhr.dispatchEvent(new Event(success ? 'load' : 'error'));
    xhr.dispatchEvent(new Event('loadend'));
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

  const pointSelectCanonicalRoute = () => {
    try {
      const route = new URL(location.href);
      route.search = '';
      route.hash = '';
      return route.toString();
    } catch (_) {
      return '';
    }
  };

  const validPointSelectToken = (token) =>
    typeof token === 'string' &&
    token.length > 0 &&
    token.length <= 128 &&
    /^[A-Za-z0-9._:-]+$/.test(token);

  const validStableReference = (reference) =>
    typeof reference === 'string' &&
    reference.length <= 64 &&
    /^@e[0-9a-f]+$/i.test(reference);

  const queuePointSelectCompletion = (completion) => {
    pointSelectCompletions.push(Object.freeze({
      requestToken: completion.requestToken,
      route: completion.route,
      status: completion.status,
      reference: completion.reference || null,
      reason: completion.reason || null,
    }));
    while (pointSelectCompletions.length > 4) pointSelectCompletions.shift();
  };

  const cleanupPointSelect = () => {
    const state = pointSelectState;
    if (!state) return null;
    pointSelectState = null;
    for (const [type, listener] of state.listeners) {
      removeEventListener(type, listener, true);
    }
    try { state.freezeObserver?.disconnect(); } catch (_) {}
    try { state.overlay?.remove(); } catch (_) {}
    return state;
  };

  const finishPointSelect = (status, reason = null, reference = null) => {
    const state = cleanupPointSelect();
    if (!state) return false;
    queuePointSelectCompletion({
      requestToken: state.requestToken,
      route: pointSelectCanonicalRoute(),
      status,
      reference: validStableReference(reference) ? reference : null,
      reason,
    });
    return true;
  };

  const refreshPointSelectOverlayVisibility = (state) => {
    if (!state?.overlay?.isConnected) return;
    const frozen = document.documentElement?.hasAttribute('data-localview-visual-freeze');
    state.overlay.style.visibility = frozen ? 'hidden' : 'visible';
  };

  const updatePointSelectHighlight = (state, target) => {
    if (!state || pointSelectState !== state || !target || isLocalViewOwned(target)) return false;
    if (!target.isConnected) return false;
    const rect = target.getBoundingClientRect();
    const left = Math.max(0, Math.min(innerWidth, Number(rect.left)));
    const top = Math.max(0, Math.min(innerHeight, Number(rect.top)));
    const right = Math.max(0, Math.min(innerWidth, Number(rect.right)));
    const bottom = Math.max(0, Math.min(innerHeight, Number(rect.bottom)));
    if (![left, top, right, bottom].every(Number.isFinite) || right <= left || bottom <= top) {
      state.hoveredElement = null;
      state.hoveredReference = null;
      state.overlay.style.display = 'none';
      return false;
    }
    const reference = refFor(target);
    if (!validStableReference(reference)) {
      state.hoveredElement = null;
      state.hoveredReference = null;
      state.overlay.style.display = 'none';
      return false;
    }
    state.hoveredElement = target;
    state.hoveredReference = reference;
    state.overlay.style.display = 'block';
    state.overlay.style.left = `${left}px`;
    state.overlay.style.top = `${top}px`;
    state.overlay.style.width = `${right - left}px`;
    state.overlay.style.height = `${bottom - top}px`;
    refreshPointSelectOverlayVisibility(state);
    return true;
  };

  const pointTargetAt = (x, y) => {
    const clientX = Number(x);
    const clientY = Number(y);
    if (!Number.isFinite(clientX) || !Number.isFinite(clientY)) return null;
    if (clientX < 0 || clientY < 0 || clientX >= innerWidth || clientY >= innerHeight) return null;
    const target = document.elementFromPoint(clientX, clientY);
    if (!target || target.nodeType !== Node.ELEMENT_NODE || isLocalViewOwned(target)) return null;
    return target;
  };

  const beginPointSelect = (request) => {
    const requestToken = String(request?.requestToken || '');
    const expectedRoute = String(request?.route || '');
    if (!validPointSelectToken(requestToken)) return false;
    if (!expectedRoute || expectedRoute !== pointSelectCanonicalRoute()) return false;

    if (pointSelectState) finishPointSelect('cancelled', 'cancelled');

    const root = document.documentElement;
    if (!root) return false;
    const overlay = document.createElement('div');
    overlay.setAttribute('data-localview-owned', 'point-select');
    overlay.setAttribute('aria-hidden', 'true');
    overlay.style.cssText = [
      'position:fixed',
      'display:none',
      'pointer-events:none',
      'box-sizing:border-box',
      'border:2px solid currentColor',
      'outline:1px solid rgba(255,255,255,.9)',
      'background:rgba(127,127,127,.12)',
      'color:#2f81f7',
      'z-index:2147483646',
      'margin:0',
      'padding:0',
    ].join(';');
    localViewOwnedElements.add(overlay);
    (document.body || root).appendChild(overlay);

    const state = {
      requestToken,
      expectedRoute,
      overlay,
      hoveredElement: null,
      hoveredReference: null,
      pointerDownElement: null,
      pendingFailure: null,
      listeners: [],
      freezeObserver: null,
    };
    pointSelectState = state;

    const suppress = (event) => {
      if (pointSelectState !== state) return;
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation?.();
    };

    const onPointerMove = (event) => {
      if (pointSelectState !== state) return;
      const target = pointTargetAt(event.clientX, event.clientY);
      if (!target) {
        state.hoveredElement = null;
        state.hoveredReference = null;
        state.overlay.style.display = 'none';
        return;
      }
      updatePointSelectHighlight(state, target);
    };

    const onPointerDown = (event) => {
      if (pointSelectState !== state) return;
      suppress(event);
      if (event.button !== 0) {
        state.pointerDownElement = null;
        return;
      }
      const target = pointTargetAt(event.clientX, event.clientY);
      const hovered = state.hoveredElement;
      if (!target) {
        state.pointerDownElement = null;
        state.pendingFailure = 'target_unavailable';
        return;
      }
      if (hovered && (!hovered.isConnected || target !== hovered)) {
        state.pointerDownElement = target;
        state.pendingFailure = 'target_changed';
        return;
      }
      state.pendingFailure = null;
      state.pointerDownElement = target;
      updatePointSelectHighlight(state, target);
    };

    const onClick = (event) => {
      if (pointSelectState !== state) return;
      suppress(event);
      const pendingFailure = state.pendingFailure;
      const downTarget = state.pointerDownElement;
      state.pointerDownElement = null;
      state.pendingFailure = null;
      if (pendingFailure) {
        finishPointSelect('failed', pendingFailure);
        return;
      }
      if (!downTarget || !downTarget.isConnected) {
        finishPointSelect('failed', 'target_unavailable');
        return;
      }
      const freshTarget = pointTargetAt(event.clientX, event.clientY);
      if (!freshTarget || freshTarget !== downTarget || freshTarget !== state.hoveredElement) {
        finishPointSelect('failed', 'target_changed');
        return;
      }
      const reference = refFor(freshTarget);
      if (!validStableReference(reference) || !resolveRefForPointSelect(freshTarget, reference)) {
        finishPointSelect('failed', 'invalid_reference');
        return;
      }
      finishPointSelect('selected', null, reference);
    };

    const onKeyDown = (event) => {
      if (pointSelectState !== state || event.key !== 'Escape') return;
      suppress(event);
      finishPointSelect('cancelled', 'escape');
    };

    const listenerSpecs = [
      ['pointermove', onPointerMove],
      ['pointerdown', onPointerDown],
      ['mousedown', suppress],
      ['mouseup', suppress],
      ['click', onClick],
      ['auxclick', suppress],
      ['contextmenu', suppress],
      ['keydown', onKeyDown],
    ];
    for (const [type, listener] of listenerSpecs) {
      addEventListener(type, listener, true);
      state.listeners.push([type, listener]);
    }

    if (typeof MutationObserver === 'function') {
      state.freezeObserver = new MutationObserver(() => refreshPointSelectOverlayVisibility(state));
      state.freezeObserver.observe(root, {
        attributes: true,
        attributeFilter: ['data-localview-visual-freeze'],
      });
    }
    refreshPointSelectOverlayVisibility(state);
    return true;
  };

  const resolveRefForPointSelect = (element, reference) =>
    !!element?.isConnected && refFor(element) === reference;

  const cancelPointSelect = (requestToken, reason = 'cancelled') => {
    if (!pointSelectState || pointSelectState.requestToken !== String(requestToken || '')) return false;
    return finishPointSelect('cancelled', reason === 'escape' ? 'escape' : 'cancelled');
  };

  const takePointSelectCompletions = (max = 4) => {
    const count = Math.max(0, Math.min(Number(max) || 0, 4, pointSelectCompletions.length));
    return pointSelectCompletions.splice(0, count);
  };

  const failPointSelectForRouteDrift = () => {
    if (pointSelectState) finishPointSelect('failed', 'route_changed');
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

  const MAX_FRAMEWORK_OWNERSHIP_PROBES = 256;
  const MAX_REACT_HOST_KEYS = 64;
  const MAX_REACT_FIBER_DEPTH = 32;
  const MAX_REACT_COMPONENT_BYTES = 96;
  const MAX_REACT_SOURCE_FILE_BYTES = 260;
  const MAX_REACT_SOURCE_LINE = 1000000;
  const MAX_REACT_SOURCE_COLUMN = 10000001;
  const MAX_REACT_DEBUG_STACK_BYTES = 16384;
  const MAX_REACT_DEBUG_STACK_LINES = 24;
  const REACT_FIBER_PREFIXES = ['__reactFiber$', '__reactInternalInstance$'];
  const REACT_PROPS_PREFIX = '__reactProps$';
  const MAX_SVELTE_COMPONENT_BYTES = 96;
  const MAX_SVELTE_SOURCE_FILE_BYTES = 260;
  const MAX_SVELTE_SOURCE_LINE = 1000000;
  const MAX_SVELTE_SOURCE_COLUMN = 10000000;
  const MAX_VUE_COMPONENT_BYTES = 96;
  const MAX_VUE_SOURCE_FILE_BYTES = 260;
  const MAX_VUE_ABSOLUTE_SOURCE_FILE_BYTES = 1024;

  const boundedUtf8String = (value, maxBytes) => {
    if (typeof value !== 'string') return null;
    const normalized = redact(value).trim();
    if (!normalized || /[\u0000-\u001f\u007f]/.test(normalized)) return null;
    if (new TextEncoder().encode(normalized).length > maxBytes) return null;
    return normalized;
  };

  const boundedRelativeSourceFile = (value, maxBytes) => {
    const raw = boundedUtf8String(value, maxBytes);
    if (!raw) return null;
    const file = raw.replace(/\\/g, '/');
    if (file.startsWith('/')
        || /^[A-Za-z]:\//.test(file)
        || /^[A-Za-z][A-Za-z0-9+.-]*:/.test(file)
        || /[%?#:]/.test(file)) {
      return null;
    }

    const segments = [];
    for (const segment of file.split('/')) {
      if (!segment || segment === '.') continue;
      if (segment === '..') return null;
      segments.push(segment);
    }
    const normalized = segments.join('/');
    if (!normalized || new TextEncoder().encode(normalized).length > maxBytes) return null;
    return normalized;
  };

  const boundedVueSourceFile = (value) => {
    const raw = boundedUtf8String(value, MAX_VUE_ABSOLUTE_SOURCE_FILE_BYTES);
    if (!raw) return null;
    const file = raw.replace(/\\/g, '/');
    if (file.startsWith('//') || /[%?#]/.test(file)) return null;

    const driveAbsolute = /^[A-Za-z]:\//.test(file);
    const posixAbsolute = file.startsWith('/');
    if (driveAbsolute || posixAbsolute) return file;
    if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(file)) return null;
    return boundedRelativeSourceFile(file, MAX_VUE_SOURCE_FILE_BYTES);
  };

  const reactComponentName = (type) => {
    const candidates = [type, type?.render, type?.type].slice(0, 3);
    for (const candidate of candidates) {
      if (!candidate || (typeof candidate !== 'function' && typeof candidate !== 'object')) continue;
      const name = boundedUtf8String(
        candidate.displayName || candidate.name,
        MAX_REACT_COMPONENT_BYTES
      );
      if (name) return name;
    }
    return null;
  };

  const reactDebugStackSource = (debugStack) => {
    let raw;
    try {
      raw = typeof debugStack === 'string' ? debugStack : debugStack?.stack;
    } catch (_) {
      return null;
    }
    if (typeof raw !== 'string'
        || new TextEncoder().encode(raw).length > MAX_REACT_DEBUG_STACK_BYTES) {
      return null;
    }

    const lines = raw.split('\n').slice(0, MAX_REACT_DEBUG_STACK_LINES);
    for (const line of lines) {
      if (!line
          || /node_modules|react(?:-dom)?|jsx-dev-runtime|jsxDEV|createElement|vite\/dist|\/@vite\//i.test(line)) {
        continue;
      }
      const match = line.match(/(https?:\/\/[^\s()]+):(\d+):(\d+)\)?$/);
      if (!match) continue;

      let url;
      try {
        url = new URL(match[1]);
      } catch (_) {
        continue;
      }
      if (url.origin !== location.origin
          || url.pathname.startsWith('/@fs/')
          || url.pathname.startsWith('//')
          || url.pathname.includes('%')
          || url.pathname.split('/').includes('..')) {
        continue;
      }

      const file = boundedUtf8String(
        url.pathname.replace(/^\/+/, ''),
        MAX_REACT_SOURCE_FILE_BYTES
      );
      const sourceLine = Number(match[2]);
      const sourceColumn = Number(match[3]);
      if (!file
          || !Number.isInteger(sourceLine)
          || sourceLine < 1
          || sourceLine > MAX_REACT_SOURCE_LINE
          || !Number.isInteger(sourceColumn)
          || sourceColumn < 1
          || sourceColumn > MAX_REACT_SOURCE_COLUMN) {
        continue;
      }
      return {
        file,
        line: sourceLine,
        column: sourceColumn,
        signal: 'debug_stack',
      };
    }
    return null;
  };

  const reactDebugSource = (fiber) => {
    const debugSource = fiber?._debugSource;
    if (debugSource && typeof debugSource === 'object') {
      const file = boundedUtf8String(debugSource.fileName, MAX_REACT_SOURCE_FILE_BYTES);
      const sourceLine = Number(debugSource.lineNumber);
      const sourceColumn = debugSource.columnNumber == null ? null : Number(debugSource.columnNumber);
      if (file
          && Number.isInteger(sourceLine)
          && sourceLine >= 1
          && sourceLine <= MAX_REACT_SOURCE_LINE
          && (sourceColumn === null
            || (Number.isInteger(sourceColumn)
              && sourceColumn >= 1
              && sourceColumn <= MAX_REACT_SOURCE_COLUMN))) {
        return {
          file,
          line: sourceLine,
          column: sourceColumn,
          signal: 'debug_source',
        };
      }
    }
    return reactDebugStackSource(fiber?._debugStack);
  };

  const reactSourceHint = (el, ownershipBudget) => {
    if (!ownershipBudget || ownershipBudget.remaining <= 0) return null;

    let keys;
    try {
      keys = Object.getOwnPropertyNames(el).slice(0, MAX_REACT_HOST_KEYS);
    } catch (_) {
      return null;
    }
    const key = keys.find((candidate) =>
      REACT_FIBER_PREFIXES.some((prefix) => candidate.startsWith(prefix))
    );
    if (!key) return null;
    ownershipBudget.remaining -= 1;

    const prefix = REACT_FIBER_PREFIXES.find((candidate) => key.startsWith(candidate));
    const suffix = prefix ? key.slice(prefix.length) : '';
    if (!suffix || !keys.includes(`${REACT_PROPS_PREFIX}${suffix}`)) return null;

    let descriptor;
    try {
      descriptor = Object.getOwnPropertyDescriptor(el, key);
    } catch (_) {
      return null;
    }
    const fiber = descriptor && Object.prototype.hasOwnProperty.call(descriptor, 'value')
      ? descriptor.value
      : null;
    if (!fiber || typeof fiber !== 'object' || fiber.stateNode !== el) return null;

    const hostSource = reactDebugSource(fiber);
    let cursor = fiber.return;
    for (let depth = 0; cursor && depth < MAX_REACT_FIBER_DEPTH; depth += 1, cursor = cursor.return) {
      if (typeof cursor !== 'object') break;
      const component = reactComponentName(cursor.type);
      if (!component) continue;
      const source = hostSource || reactDebugSource(cursor);
      if (!source) continue;

      return {
        origin: 'react-dev-fiber',
        file: source.file,
        line: source.line,
        column: source.column,
        component,
        signal: source.signal,
      };
    }
    return null;
  };

  const ownDataDescriptor = (object, key) => {
    if (!object || (typeof object !== 'object' && typeof object !== 'function')) return null;
    try {
      const descriptor = Object.getOwnPropertyDescriptor(object, key);
      return descriptor && Object.prototype.hasOwnProperty.call(descriptor, 'value')
        ? descriptor
        : null;
    } catch (_) {
      return null;
    }
  };

  const svelteSourceHint = (el, ownershipBudget) => {
    if (!ownershipBudget || ownershipBudget.remaining <= 0) return null;

    const descriptor = ownDataDescriptor(el, '__svelte_meta');
    if (!descriptor) return null;
    ownershipBudget.remaining -= 1;

    const meta = descriptor.value;
    const locDescriptor = ownDataDescriptor(meta, 'loc');
    if (!locDescriptor) return null;
    const loc = locDescriptor.value;

    const fileDescriptor = ownDataDescriptor(loc, 'file');
    const lineDescriptor = ownDataDescriptor(loc, 'line');
    const columnDescriptor = ownDataDescriptor(loc, 'column');
    if (!fileDescriptor || !lineDescriptor || !columnDescriptor) return null;

    const file = boundedRelativeSourceFile(fileDescriptor.value, MAX_SVELTE_SOURCE_FILE_BYTES);
    if (!file || !file.endsWith('.svelte')) return null;

    const line = Number(lineDescriptor.value);
    const column = Number(columnDescriptor.value);
    if (!Number.isInteger(line)
        || line < 1
        || line > MAX_SVELTE_SOURCE_LINE
        || !Number.isInteger(column)
        || column < 0
        || column > MAX_SVELTE_SOURCE_COLUMN) {
      return null;
    }

    const basename = file.split('/').pop();
    const component = basename && basename.endsWith('.svelte')
      ? boundedUtf8String(basename.slice(0, -'.svelte'.length), MAX_SVELTE_COMPONENT_BYTES)
      : null;
    if (!component) return null;

    return {
      origin: 'svelte-dev-meta',
      file,
      line,
      column,
      component,
      signal: 'element_meta',
    };
  };

  const vueSourceHint = (el, ownershipBudget) => {
    if (!ownershipBudget || ownershipBudget.remaining <= 0) return null;

    const instanceDescriptor = ownDataDescriptor(el, '__vueParentComponent');
    if (!instanceDescriptor) return null;
    ownershipBudget.remaining -= 1;

    const instance = instanceDescriptor.value;
    const typeDescriptor = ownDataDescriptor(instance, 'type');
    if (!typeDescriptor) return null;
    const componentType = typeDescriptor.value;
    const fileDescriptor = ownDataDescriptor(componentType, '__file');
    if (!fileDescriptor) return null;

    const file = boundedVueSourceFile(fileDescriptor.value);
    if (!file || !file.endsWith('.vue')) return null;

    const basename = file.split('/').pop();
    const component = basename && basename.endsWith('.vue')
      ? boundedUtf8String(basename.slice(0, -'.vue'.length), MAX_VUE_COMPONENT_BYTES)
      : null;
    if (!component) return null;

    return {
      origin: 'vue-dev-instance',
      file,
      component,
      signal: 'element_parent_component',
    };
  };

  const sourceHint = (el, ownershipBudget) => {
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
    const react = reactSourceHint(el, ownershipBudget);
    if (react) return react;
    const svelte = svelteSourceHint(el, ownershipBudget);
    if (svelte) return svelte;
    return vueSourceHint(el, ownershipBudget);
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

  const CSS_TRACE_PROPERTIES = [
    'display', 'position', 'overflow-x', 'overflow-y', 'box-sizing', 'z-index',
    'flex-direction', 'flex-wrap', 'justify-content', 'align-items', 'gap', 'row-gap', 'column-gap',
    'grid-template-columns', 'grid-template-rows',
    'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
    'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
    'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width',
    'font-size', 'font-weight', 'font-family', 'line-height', 'color', 'background-color',
    'opacity', 'pointer-events', 'visibility'
  ];
  const CSS_AUTHOR_CASCADE_PROPERTIES = [
    'display', 'position', 'box-sizing', 'z-index',
    'opacity', 'pointer-events', 'visibility', 'color'
  ];
  const CSS_AUTHOR_CASCADE_PROPERTY_SET = new Set(CSS_AUTHOR_CASCADE_PROPERTIES);
  const MAX_CSS_TRACE_STYLESHEETS = 96;
  const MAX_CSS_TRACE_RULES = 512;
  const MAX_CSS_TRACE_DECLARATIONS = 12;
  const MAX_CSS_SELECTOR_BYTES = 256;
  const MAX_CSS_SELECTOR_ARMS = 32;
  const MAX_CSS_CASCADE_DEPTH = 8;
  const MAX_CSS_VALUE_BYTES = 256;
  const MAX_CSS_SOURCE_FILE_BYTES = 260;
  const MAX_CSS_SPECIFICITY_UNIT = 255;

  const boundedCssValue = (value) => {
    if (typeof value !== 'string') return null;
    const withoutUrls = value.replace(/url\([^)]*\)/gi, 'url(<redacted>)');
    return boundedUtf8String(withoutUrls, MAX_CSS_VALUE_BYTES);
  };

  const stylesheetSourceFile = (sheet) => {
    try {
      if (!sheet?.href) return null;
      const url = new URL(sheet.href, location.href);
      if (url.origin !== location.origin
          || url.pathname.startsWith('/@fs/')
          || url.pathname.startsWith('//')
          || url.pathname.includes('%')) {
        return null;
      }
      return boundedRelativeSourceFile(
        url.pathname.replace(/^\/+/, ''),
        MAX_CSS_SOURCE_FILE_BYTES
      );
    } catch (_) {
      return null;
    }
  };

  const splitSelectorList = (selectorText) => {
    const selector = boundedUtf8String(selectorText, MAX_CSS_SELECTOR_BYTES);
    if (!selector) return null;
    const arms = [];
    let start = 0;
    let squareDepth = 0;
    let parenDepth = 0;
    let quote = null;
    let escaped = false;

    for (let index = 0; index < selector.length; index += 1) {
      const char = selector[index];
      if (escaped) {
        escaped = false;
        continue;
      }
      if (char === '\\') {
        escaped = true;
        continue;
      }
      if (quote) {
        if (char === quote) quote = null;
        continue;
      }
      if (char === '"' || char === "'") {
        quote = char;
        continue;
      }
      if (char === '[') {
        squareDepth += 1;
        continue;
      }
      if (char === ']') {
        squareDepth -= 1;
        if (squareDepth < 0) return null;
        continue;
      }
      if (char === '(') {
        parenDepth += 1;
        continue;
      }
      if (char === ')') {
        parenDepth -= 1;
        if (parenDepth < 0) return null;
        continue;
      }
      if (char === ',' && squareDepth === 0 && parenDepth === 0) {
        const arm = selector.slice(start, index).trim();
        if (!arm) return null;
        arms.push(arm);
        if (arms.length > MAX_CSS_SELECTOR_ARMS) return null;
        start = index + 1;
      }
    }

    if (escaped || quote || squareDepth !== 0 || parenDepth !== 0) return null;
    const finalArm = selector.slice(start).trim();
    if (!finalArm) return null;
    arms.push(finalArm);
    return arms.length <= MAX_CSS_SELECTOR_ARMS ? arms : null;
  };

  const selectorSpecificity = (selector) => {
    const raw = boundedUtf8String(selector, MAX_CSS_SELECTOR_BYTES);
    if (!raw
        || raw.includes('\\')
        || raw.includes('|')
        || raw.includes('(')
        || raw.includes('::')
        || /[^\x00-\x7f]/.test(raw)) {
      return null;
    }

    const isIdentStart = (char) => !!char && /[A-Za-z_-]/.test(char);
    const isIdent = (char) => !!char && /[A-Za-z0-9_-]/.test(char);
    const consumeIdent = (offset) => {
      if (!isIdentStart(raw[offset])) return -1;
      let cursor = offset + 1;
      while (cursor < raw.length && isIdent(raw[cursor])) cursor += 1;
      return cursor;
    };

    let ids = 0;
    let classes = 0;
    let types = 0;
    let index = 0;
    let compoundStart = true;

    while (index < raw.length) {
      const char = raw[index];
      if (/\s/.test(char)) {
        while (index < raw.length && /\s/.test(raw[index])) index += 1;
        compoundStart = true;
        continue;
      }
      if (char === '>' || char === '+' || char === '~') {
        index += 1;
        compoundStart = true;
        continue;
      }
      if (char === '*') {
        if (!compoundStart) return null;
        compoundStart = false;
        index += 1;
        continue;
      }
      if (char === '#' || char === '.') {
        const next = consumeIdent(index + 1);
        if (next < 0) return null;
        if (char === '#') ids += 1;
        else classes += 1;
        if (ids > MAX_CSS_SPECIFICITY_UNIT || classes > MAX_CSS_SPECIFICITY_UNIT) return null;
        compoundStart = false;
        index = next;
        continue;
      }
      if (char === '[') {
        let cursor = index + 1;
        let quote = null;
        while (cursor < raw.length) {
          const nested = raw[cursor];
          if (quote) {
            if (nested === quote) quote = null;
            cursor += 1;
            continue;
          }
          if (nested === '"' || nested === "'") {
            quote = nested;
            cursor += 1;
            continue;
          }
          if (nested === ']') break;
          cursor += 1;
        }
        if (cursor >= raw.length || quote) return null;
        classes += 1;
        if (classes > MAX_CSS_SPECIFICITY_UNIT) return null;
        compoundStart = false;
        index = cursor + 1;
        continue;
      }
      if (char === ':') {
        if (raw[index + 1] === ':') return null;
        const next = consumeIdent(index + 1);
        if (next < 0) return null;
        classes += 1;
        if (classes > MAX_CSS_SPECIFICITY_UNIT) return null;
        compoundStart = false;
        index = next;
        continue;
      }
      if (compoundStart && isIdentStart(char)) {
        const next = consumeIdent(index);
        if (next < 0) return null;
        types += 1;
        if (types > MAX_CSS_SPECIFICITY_UNIT) return null;
        compoundStart = false;
        index = next;
        continue;
      }
      return null;
    }

    return [0, ids, classes, types];
  };

  const compareSpecificity = (left, right) => {
    for (let index = 0; index < 4; index += 1) {
      if (left[index] !== right[index]) return left[index] - right[index];
    }
    return 0;
  };

  const matchingSelectorSpecificity = (selectorText, el) => {
    const arms = splitSelectorList(selectorText);
    if (!arms) {
      let matched = false;
      try {
        matched = el.matches(selectorText);
      } catch (_) {}
      return { matched, specificity: null, unsupportedMatching: matched };
    }

    let matched = false;
    let unsupportedMatching = false;
    let best = null;
    for (const arm of arms) {
      let armMatches = false;
      try {
        armMatches = el.matches(arm);
      } catch (_) {}
      if (!armMatches) continue;
      matched = true;
      const specificity = selectorSpecificity(arm);
      if (!specificity) {
        unsupportedMatching = true;
        continue;
      }
      if (!best || compareSpecificity(specificity, best) > 0) best = specificity;
    }
    return { matched, specificity: best, unsupportedMatching };
  };

  const cssDeclarationTrace = (el) => {
    const declarations = [];
    const unresolvedProperties = new Set();
    const winners = new Map();
    let cascadeCoverageComplete = true;
    let visitedRules = 0;
    let sourceOrder = 0;

    const pushDeclaration = (sourceKind, file, selector, property, value, important) => {
      if (declarations.length >= MAX_CSS_TRACE_DECLARATIONS) return;
      const boundedProperty = boundedUtf8String(property, 64);
      const boundedValue = boundedCssValue(value);
      const boundedSelector = selector == null
        ? null
        : boundedUtf8String(selector, MAX_CSS_SELECTOR_BYTES);
      if (!boundedProperty || !boundedValue || (selector != null && !boundedSelector)) return;
      declarations.push({
        source_kind: sourceKind,
        stylesheet_path: file,
        selector: boundedSelector,
        property: boundedProperty,
        value: boundedValue,
        important: !!important,
      });
    };

    const markAllCascadePropertiesUnresolved = () => {
      for (const property of CSS_AUTHOR_CASCADE_PROPERTIES) unresolvedProperties.add(property);
    };

    const markRulePropertiesUnresolved = (style) => {
      if (!style) return;
      if (style.getPropertyValue('all')) markAllCascadePropertiesUnresolved();
      for (const property of CSS_AUTHOR_CASCADE_PROPERTIES) {
        if (style.getPropertyValue(property)) unresolvedProperties.add(property);
      }
    };

    const betterCandidate = (candidate, current) => {
      if (!current) return true;
      if (candidate.important !== current.important) return candidate.important;
      const specificity = compareSpecificity(candidate.specificity, current.specificity);
      if (specificity !== 0) return specificity > 0;
      return candidate.source_order > current.source_order;
    };

    const considerCascadeCandidate = (
      sourceKind,
      file,
      selector,
      property,
      value,
      important,
      specificity,
      order
    ) => {
      if (!CSS_AUTHOR_CASCADE_PROPERTY_SET.has(property)) return;
      const boundedValue = boundedCssValue(value);
      if (!boundedValue || boundedValue === 'revert' || boundedValue === 'revert-layer') {
        unresolvedProperties.add(property);
        return;
      }
      const candidate = {
        source_kind: sourceKind,
        stylesheet_path: file,
        selector,
        property,
        value: boundedValue,
        important: !!important,
        specificity,
        source_order: order,
      };
      const current = winners.get(property);
      if (betterCandidate(candidate, current)) winners.set(property, candidate);
    };

    const inlineStyle = el?.style;
    if (inlineStyle) {
      if (inlineStyle.getPropertyValue('all')) markAllCascadePropertiesUnresolved();
      for (const property of CSS_TRACE_PROPERTIES) {
        const value = inlineStyle.getPropertyValue(property);
        if (!value) continue;
        pushDeclaration(
          'inline_element',
          null,
          null,
          property,
          value,
          inlineStyle.getPropertyPriority(property) === 'important'
        );
      }
      for (const property of CSS_AUTHOR_CASCADE_PROPERTIES) {
        const value = inlineStyle.getPropertyValue(property);
        if (!value) continue;
        considerCascadeCandidate(
          'inline_element',
          null,
          null,
          property,
          value,
          inlineStyle.getPropertyPriority(property) === 'important',
          [1, 0, 0, 0],
          0
        );
      }
    }

    let adoptedCount = 0;
    try {
      adoptedCount = Number(document.adoptedStyleSheets?.length || 0);
    } catch (_) {
      adoptedCount = 1;
    }
    if (adoptedCount > 0) cascadeCoverageComplete = false;

    let allSheets = [];
    try {
      allSheets = Array.from(document.styleSheets || []);
    } catch (_) {
      cascadeCoverageComplete = false;
    }
    if (allSheets.length > MAX_CSS_TRACE_STYLESHEETS) cascadeCoverageComplete = false;

    const walkRules = (rules, sourceKind, sourceFile, depth) => {
      if (depth > MAX_CSS_CASCADE_DEPTH) {
        cascadeCoverageComplete = false;
        return false;
      }
      for (const rule of rules) {
        if (visitedRules >= MAX_CSS_TRACE_RULES) {
          cascadeCoverageComplete = false;
          return false;
        }
        visitedRules += 1;
        if (!rule) continue;

        const constructorName = String(rule?.constructor?.name || '');
        if (constructorName === 'CSSImportRule'
            || constructorName === 'CSSLayerBlockRule'
            || constructorName === 'CSSLayerStatementRule'
            || constructorName === 'CSSContainerRule'
            || constructorName === 'CSSScopeRule'
            || constructorName === 'CSSStartingStyleRule') {
          cascadeCoverageComplete = false;
          continue;
        }

        if (rule.selectorText && rule.style) {
          sourceOrder += 1;
          const selector = boundedUtf8String(rule.selectorText, MAX_CSS_SELECTOR_BYTES);
          if (!selector) {
            cascadeCoverageComplete = false;
            continue;
          }

          const matching = matchingSelectorSpecificity(selector, el);
          if (matching.matched) {
            if (matching.unsupportedMatching || !matching.specificity) {
              markRulePropertiesUnresolved(rule.style);
            } else {
              if (rule.style.getPropertyValue('all')) markAllCascadePropertiesUnresolved();
              for (const property of CSS_AUTHOR_CASCADE_PROPERTIES) {
                const value = rule.style.getPropertyValue(property);
                if (!value) continue;
                considerCascadeCandidate(
                  sourceKind,
                  sourceFile,
                  selector,
                  property,
                  value,
                  rule.style.getPropertyPriority(property) === 'important',
                  matching.specificity,
                  sourceOrder
                );
              }
            }

            for (const property of CSS_TRACE_PROPERTIES) {
              const value = rule.style.getPropertyValue(property);
              if (!value) continue;
              pushDeclaration(
                sourceKind,
                sourceFile,
                selector,
                property,
                value,
                rule.style.getPropertyPriority(property) === 'important'
              );
            }
          }

          let nestedStyleRules = [];
          try {
            nestedStyleRules = rule.cssRules ? Array.from(rule.cssRules) : [];
          } catch (_) {
            cascadeCoverageComplete = false;
          }
          if (nestedStyleRules.length > 0) cascadeCoverageComplete = false;
          continue;
        }

        let nested = [];
        try {
          nested = rule.cssRules ? Array.from(rule.cssRules) : [];
        } catch (_) {
          cascadeCoverageComplete = false;
          continue;
        }

        if (constructorName === 'CSSMediaRule') {
          let active = false;
          try {
            active = window.matchMedia(rule.conditionText).matches;
          } catch (_) {
            cascadeCoverageComplete = false;
            continue;
          }
          if (active && !walkRules(nested, sourceKind, sourceFile, depth + 1)) return false;
          continue;
        }

        if (constructorName === 'CSSSupportsRule') {
          let active = false;
          try {
            active = typeof CSS?.supports === 'function' && CSS.supports(rule.conditionText);
          } catch (_) {
            cascadeCoverageComplete = false;
            continue;
          }
          if (active && !walkRules(nested, sourceKind, sourceFile, depth + 1)) return false;
          continue;
        }

        if (constructorName === 'CSSKeyframesRule') continue;
        if (nested.length > 0) cascadeCoverageComplete = false;
      }
      return true;
    };

    for (const sheet of allSheets.slice(0, MAX_CSS_TRACE_STYLESHEETS)) {
      if (sheet?.disabled) continue;
      let sheetMedia = '';
      try {
        sheetMedia = String(sheet?.media?.mediaText || '').trim();
      } catch (_) {
        cascadeCoverageComplete = false;
        continue;
      }
      if (sheetMedia && sheetMedia !== 'all') {
        let active = false;
        try {
          active = window.matchMedia(sheetMedia).matches;
        } catch (_) {
          cascadeCoverageComplete = false;
          continue;
        }
        if (!active) continue;
      }

      let rules;
      try {
        rules = Array.from(sheet.cssRules || []);
      } catch (_) {
        cascadeCoverageComplete = false;
        continue;
      }
      const sourceFile = stylesheetSourceFile(sheet);
      if (sheet?.href && !sourceFile) {
        cascadeCoverageComplete = false;
        continue;
      }
      const sourceKind = sourceFile ? 'same_origin_stylesheet' : 'inline_stylesheet';
      if (!walkRules(rules, sourceKind, sourceFile, 0)) break;
    }

    const unresolved = Array.from(unresolvedProperties)
      .filter((property) => CSS_AUTHOR_CASCADE_PROPERTY_SET.has(property))
      .sort();
    const authorWinners = cascadeCoverageComplete
      ? Array.from(winners.values())
          .filter((candidate) => !unresolvedProperties.has(candidate.property))
          .sort((left, right) => left.property.localeCompare(right.property))
      : [];

    return {
      declarations,
      authorCascade: {
        scope: 'supported_author_subset',
        coverage_complete: cascadeCoverageComplete,
        unresolved_properties: unresolved,
        winners: authorWinners,
      },
    };
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
        !isLocalViewOwned(candidate) &&
        candidate !== el && !el.contains(candidate) && !candidate.contains?.(el)
      );
      if (blocker) {
        occluded = true;
        occludedBy = refFor(blocker);
      }
    }

    return { inViewport, clipped, occluded, occludedBy, sampled };
  };

  const compactSemanticNode = (el, includeStyle, occlusionBudget, ownershipBudget) => {
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
      sourceHint: sourceHint(el, ownershipBudget),
      attributes: safeAttributes(el),
      style: includeStyle ? computedStylePacket(el) : null,
      styleTrace: includeStyle ? cssDeclarationTrace(el) : null,
    };
  };

  const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'META', 'LINK', 'HEAD']);

  const semanticTree = (occlusionBudget, ownershipBudget) => {
    const root = document.body || document.documentElement;
    if (!root) return null;
    let nodes = 0;
    let styled = 0;

    const visit = (el, depth) => {
      if (!el || el.nodeType !== Node.ELEMENT_NODE || isLocalViewOwned(el) || SKIP_TAGS.has(el.tagName)) return null;
      if (nodes >= config.max_semantic_nodes || depth > config.max_tree_depth) return null;
      nodes += 1;
      const includeStyle = styled < config.max_style_nodes && (isInteractive(el) || depth <= 3);
      if (includeStyle) styled += 1;
      const node = compactSemanticNode(el, includeStyle, occlusionBudget, ownershipBudget);
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

  const interactiveSnapshot = (occlusionBudget, ownershipBudget) => Array.from(document.querySelectorAll(interactiveSelector))
    .filter((el) => !isLocalViewOwned(el))
    .slice(0, config.max_interactive_nodes)
    .map((el) => compactSemanticNode(el, false, occlusionBudget, ownershipBudget));

  const snapshot = () => {
    snapshotVersion += 1;
    const occlusionBudget = { remaining: Math.max(0, Number(config.max_occlusion_samples) || 0) };
    const ownershipBudget = { remaining: MAX_FRAMEWORK_OWNERSHIP_PROBES };
    const semantic_tree = semanticTree(occlusionBudget, ownershipBudget);
    const packet = {
      version: snapshotVersion,
      route: safeUrl(location.href),
      title: redact(document.title).slice(0, 240),
      readyState: document.readyState,
      viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
      scroll: { x: scrollX, y: scrollY },
      activeRef: refFor(document.activeElement),
      semantic_tree,
      interactive: interactiveSnapshot(occlusionBudget, ownershipBudget),
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
      if (isLocalViewOwned(element)) continue;
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
      node: compactSemanticNode(el, true, { remaining: 1 }, { remaining: 1 }),
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
    failPointSelectForRouteDrift();
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

  if (config.include_hmr && 'WebSocket' in window) {
    const NativeWebSocket = window.WebSocket;
    const MAX_HMR_MESSAGE_BYTES = 256 * 1024;
    const MAX_HMR_UPDATE_COUNT = 256;

    const hmrProtocols = (value) => {
      if (typeof value === 'string') return [value.toLowerCase()];
      if (Array.isArray(value)) {
        return value
          .filter((item) => typeof item === 'string')
          .map((item) => item.toLowerCase())
          .slice(0, 16);
      }
      return [];
    };

    const isLoopbackHmrHost = (hostname) => {
      const host = String(hostname || '').toLowerCase();
      return host === 'localhost'
        || host === '::1'
        || host === '[::1]'
        || /^127(?:\.\d{1,3}){3}$/.test(host);
    };

    const hmrFrameworkForSocket = (rawUrl, protocols) => {
      let parsed;
      try {
        parsed = new URL(String(rawUrl || ''), location.href);
      } catch (_) {
        return null;
      }
      if (!isLoopbackHmrHost(parsed.hostname)) return null;

      if (hmrProtocols(protocols).includes('vite-hmr')) return 'vite';

      const path = parsed.pathname.toLowerCase();

      if (
        path === '/_next/hmr'
        || path === '/_next/webpack-hmr'
        || path.endsWith('/_next/hmr')
        || path.endsWith('/_next/webpack-hmr')
      ) return 'next';

      if (path.includes('/sockjs-node') || path.includes('webpack-hmr')) return 'webpack';
      return null;
    };

    const canonicalHmrPhase = (value) => String(value || '')
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '');

    const classifyHmrMessage = (framework, data) => {
      if (typeof data !== 'string' || data.length > MAX_HMR_MESSAGE_BYTES) return null;
      try {
        if (new TextEncoder().encode(data).byteLength > MAX_HMR_MESSAGE_BYTES) return null;
      } catch (_) {
        return null;
      }

      let value;
      try {
        value = JSON.parse(data);
      } catch (_) {
        return null;
      }
      if (!value || typeof value !== 'object' || Array.isArray(value)) return null;

      if (framework === 'vite') {
        const type = String(value.type || '').toLowerCase();
        if (type === 'update') {
          return {
            phase: 'update',
            updateCount: Math.min(
              Array.isArray(value.updates) ? value.updates.length : 0,
              MAX_HMR_UPDATE_COUNT,
            ),
          };
        }
        if (type === 'full-reload') return { phase: 'full_reload' };
        if (type === 'prune') return { phase: 'prune' };
        if (type === 'error') return { phase: 'error' };
        return null;
      }

      const rawPhase = canonicalHmrPhase(value.action || value.type || value.event);
      if (!rawPhase) return null;

      if (framework === 'next') {
        const nextPhases = {
          building: 'building',
          built: 'built',
          sync: 'sync',
          reload: 'reload',
          reload_page: 'reload',
          reloadpage: 'reload',
          server_component_change: 'server_component_change',
          server_component_changes: 'server_component_change',
          servercomponentchange: 'server_component_change',
          servercomponentchanges: 'server_component_change',
          error: 'error',
          errors: 'error',
        };
        const phase = nextPhases[rawPhase];
        return phase ? { phase } : null;
      }

      if (framework === 'webpack') {
        const webpackPhases = {
          invalid: 'invalid',
          hash: 'hash',
          ok: 'ok',
          still_ok: 'still_ok',
          warnings: 'warnings',
          errors: 'errors',
          static_changed: 'static_changed',
        };
        const phase = webpackPhases[rawPhase];
        return phase ? { phase } : null;
      }

      return null;
    };

    const observeHmrSocket = (socket, args) => {
      const framework = hmrFrameworkForSocket(args?.[0], args?.[1]);
      if (!framework) return;

      socket.addEventListener('message', (event) => {
        try {
          const signal = classifyHmrMessage(framework, event.data);
          if (signal) push('hmr', { framework, ...signal });
        } catch (_) {}
      });
    };

    const ObservedWebSocket = new Proxy(NativeWebSocket, {
      construct(target, args, newTarget) {
        const socket = Reflect.construct(target, args, newTarget);
        try { observeHmrSocket(socket, args); } catch (_) {}
        return socket;
      },
    });

    try { window.WebSocket = ObservedWebSocket; } catch (_) {}
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
      const rawUrl = request?.url || request;
      const url = safeUrl(rawUrl);
      const rule = selectNetworkFaultRule('fetch', method, rawUrl);
      const started = performance.now();
      beginNetworkRequest();
      try {
        if (rule && rule.effect.kind === 'fail') {
          throw new TypeError('LocalView injected network failure');
        }
        if (rule && rule.effect.kind === 'delay') {
          await new Promise(resolve => setTimeout(resolve, rule.effect.milliseconds));
        }
        const response = rule && rule.effect.kind === 'mock_status'
          ? new Response(null, { status: rule.effect.status })
          : await originalFetch(...args);
        push('network', {
          transport: 'fetch',
          method,
          url,
          status: response.status,
          ok: response.ok,
          duration: Math.round((performance.now() - started) * 10) / 10,
          faultInjected: Boolean(rule),
          faultRuleId: rule?.id || null,
          faultEffect: rule?.effect?.kind || null,
          faultDelayMs: rule?.effect?.kind === 'delay' ? rule.effect.milliseconds : null,
          faultStatus: rule?.effect?.kind === 'mock_status' ? rule.effect.status : null,
        });
        return response;
      } catch (error) {
        push('network', {
          transport: 'fetch',
          method,
          url,
          status: null,
          ok: false,
          duration: Math.round((performance.now() - started) * 10) / 10,
          error: redact(error?.message || error),
          faultInjected: Boolean(rule),
          faultRuleId: rule?.id || null,
          faultEffect: rule?.effect?.kind || null,
          faultDelayMs: rule?.effect?.kind === 'delay' ? rule.effect.milliseconds : null,
          faultStatus: rule?.effect?.kind === 'mock_status' ? rule.effect.status : null,
        });
        throw error;
      } finally {
        finishNetworkRequest();
      }
    };

    const xhrMeta = new WeakMap();
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function(method, url, ...rest) {
      clearSyntheticXhrValues(this);
      const faultTarget = normalizeNetworkFaultTarget(url);
      xhrMeta.set(this, {
        method: String(method || 'GET').toUpperCase(),
        url: safeUrl(url),
        faultTarget,
        started: 0,
        active: false,
        faultPending: false,
        faultCompleted: false,
      });
      return originalOpen.call(this, method, url, ...rest);
    };
    XMLHttpRequest.prototype.send = function(...args) {
      const meta = xhrMeta.get(this) || {
        method: 'GET',
        url: '',
        faultTarget: null,
        started: 0,
        active: false,
        faultPending: false,
        faultCompleted: false,
      };
      const startedHere = !meta.active;
      let onLoadEnd = null;
      if (meta.active || meta.faultPending) {
        throw new DOMException('XMLHttpRequest send already active', 'InvalidStateError');
      }
      const rule = selectNetworkFaultRule('xhr', meta.method, '', meta.faultTarget);
      if (startedHere) {
        meta.started = performance.now();
        meta.active = true;
        meta.faultPending = Boolean(rule);
        meta.faultCompleted = false;
        beginNetworkRequest();
        xhrMeta.set(this, meta);
        onLoadEnd = () => {
          const faultCompleted = Boolean(meta.faultCompleted);
          if (faultCompleted) return;
          meta.faultCompleted = true;
          meta.faultPending = false;
          if (meta.active) {
            meta.active = false;
            finishNetworkRequest();
          }
          push('network', {
            transport: 'xhr',
            method: meta.method,
            url: meta.url,
            status: Number.isFinite(this.status) ? this.status : null,
            ok: this.status >= 200 && this.status < 400,
            duration: Math.round((performance.now() - meta.started) * 10) / 10,
            faultInjected: Boolean(rule),
            faultRuleId: rule?.id || null,
            faultEffect: rule?.effect?.kind || null,
            faultDelayMs: rule?.effect?.kind === 'delay' ? rule.effect.milliseconds : null,
            faultStatus: rule?.effect?.kind === 'mock_status' ? rule.effect.status : null,
          });
        };
        if (!rule || rule.effect.kind === 'delay') {
          this.addEventListener('loadend', onLoadEnd, { once: true });
        }
      }

      if (rule && rule.effect.kind === 'fail') {
        queueMicrotask(() => completeSyntheticXhrFault(this, meta, rule, false));
        return undefined;
      }
      if (rule && rule.effect.kind === 'mock_status') {
        queueMicrotask(() => completeSyntheticXhrFault(this, meta, rule, true));
        return undefined;
      }
      if (rule && rule.effect.kind === 'delay') {
        setTimeout(() => {
          if (meta.faultCompleted) return;
          meta.faultPending = false;
          try {
            originalSend.apply(this, args);
          } catch (_) {
            if (onLoadEnd) this.removeEventListener('loadend', onLoadEnd);
            completeSyntheticXhrFault(this, meta, rule, false);
          }
        }, rule.effect.milliseconds);
        return undefined;
      }

      try {
        return originalSend.apply(this, args);
      } catch (error) {
        if (startedHere && meta.active) {
          meta.active = false;
          finishNetworkRequest();
        }
        if (startedHere && onLoadEnd) {
          this.removeEventListener('loadend', onLoadEnd);
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
    beginPointSelect,
    cancelPointSelect,
    takePointSelectCompletions,
    inspect(reference) { return inspect(reference); },
    installNetworkFaultPlan,
    clearNetworkFaultPlan,
    networkFaultState,
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
    fn defaults_enable_bounded_privacy_safe_hmr_transport_observation() {
        let script = bootstrap_script(&InstrumentationConfig::default());
        assert!(script.contains("\"include_hmr\":true"));
        assert!(script.contains("new Proxy(NativeWebSocket"));
        assert!(script.contains("'vite-hmr'"));
        assert!(script.contains("'/_next/hmr'"));
        assert!(script.contains("'/_next/webpack-hmr'"));
        assert!(script.contains("'/sockjs-node'"));
        assert!(script.contains("MAX_HMR_MESSAGE_BYTES = 256 * 1024"));
        assert!(
            script.contains("new TextEncoder().encode(data).byteLength > MAX_HMR_MESSAGE_BYTES")
        );
        assert!(script.contains("MAX_HMR_UPDATE_COUNT = 256"));
        assert!(script.contains("isLoopbackHmrHost"));
        assert!(script.contains("!isLoopbackHmrHost(parsed.hostname)"));
        assert!(script.contains("host === 'localhost'"));
        assert!(script.contains("/^127(?:\\.\\d{1,3}){3}$/"));
        assert!(script.contains("push('hmr', { framework, ...signal })"));
        assert!(script.contains("Array.isArray(value.updates) ? value.updates.length : 0"));
        assert!(!script.contains("push('hmr', { data: event.data"));
        assert!(!script.contains("path === '/ws'"));
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
