#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentationConfig {
    pub max_events: usize,
    pub max_interactive_nodes: usize,
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
            include_console: true,
            include_network: true,
            include_performance: true,
            include_scroll: true,
        }
    }
}

pub fn bootstrap_script(config: &InstrumentationConfig) -> String {
    let config = serde_json::to_string(config).expect("instrumentation config is serializable");
    SCRIPT.replace("__LOCALVIEW_CONFIG__", &config)
}

const SCRIPT: &str = r#"
(() => {
  if (window.__LOCALVIEW__) return;

  const config = __LOCALVIEW_CONFIG__;
  const events = [];
  const refs = new WeakMap();
  let sequence = 0;
  let snapshotVersion = 0;
  let mutationFlushQueued = false;
  const changedRefs = new Set();

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
      route: location.pathname + location.search + location.hash,
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
    HEADER: 'banner', FOOTER: 'contentinfo', FORM: 'form'
  })[el.tagName] || null;

  const nameOf = (el) => {
    const labelled = el.getAttribute?.('aria-label');
    if (labelled) return redact(labelled).slice(0, 180);
    const labelledBy = el.getAttribute?.('aria-labelledby');
    if (labelledBy) {
      const text = labelledBy.split(/\s+/).map(id => document.getElementById(id)?.textContent || '').join(' ').trim();
      if (text) return redact(text).slice(0, 180);
    }
    if ('value' in el && ['BUTTON', 'INPUT'].includes(el.tagName) && el.value) return redact(el.value).slice(0, 180);
    return redact(el.innerText || el.alt || el.title || '').replace(/\s+/g, ' ').trim().slice(0, 180) || null;
  };

  const ancestry = (el) => {
    const parts = [];
    let cursor = el;
    for (let depth = 0; cursor && depth < 5; depth++, cursor = cursor.parentElement) {
      let part = cursor.tagName?.toLowerCase() || 'node';
      if (cursor.id) part += '#' + cursor.id;
      const testId = cursor.getAttribute?.('data-testid');
      if (testId) part += '[testid=' + testId + ']';
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

  const interactiveSelector = [
    'a[href]', 'button', 'input', 'select', 'textarea', 'summary',
    '[role="button"]', '[role="link"]', '[role="textbox"]', '[tabindex]'
  ].join(',');

  const semanticNode = (el) => ({
    ref: refFor(el),
    tag: el.tagName.toLowerCase(),
    role: roleOf(el),
    name: nameOf(el),
    rect: rectOf(el),
    disabled: !!el.disabled || el.getAttribute('aria-disabled') === 'true',
    hidden: el.hidden || getComputedStyle(el).visibility === 'hidden' || getComputedStyle(el).display === 'none',
    focused: document.activeElement === el,
  });

  const snapshot = () => {
    snapshotVersion += 1;
    const interactive = Array.from(document.querySelectorAll(interactiveSelector))
      .slice(0, config.max_interactive_nodes)
      .map(semanticNode);
    return {
      version: snapshotVersion,
      route: location.pathname + location.search + location.hash,
      title: document.title,
      readyState: document.readyState,
      viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
      scroll: { x: scrollX, y: scrollY },
      activeRef: refFor(document.activeElement),
      interactive,
    };
  };

  const flushMutations = () => {
    mutationFlushQueued = false;
    if (!changedRefs.size) return;
    push('dom_changed', { refs: Array.from(changedRefs).slice(0, 256) });
    changedRefs.clear();
  };

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
  }).observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  });

  const announceRoute = (source) => push('route_changed', {
    source,
    href: safeUrl(location.href),
  });

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
      try {
        const response = await originalFetch(...args);
        push('network', {
          transport: 'fetch',
          method,
          url,
          status: response.status,
          ok: response.ok,
          duration: Math.round((performance.now() - started) * 10) / 10,
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
        });
        throw error;
      }
    };

    const xhrMeta = new WeakMap();
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function(method, url, ...rest) {
      xhrMeta.set(this, { method: String(method || 'GET').toUpperCase(), url: safeUrl(url), started: 0 });
      return originalOpen.call(this, method, url, ...rest);
    };
    XMLHttpRequest.prototype.send = function(...args) {
      const meta = xhrMeta.get(this) || { method: 'GET', url: '', started: 0 };
      meta.started = performance.now();
      xhrMeta.set(this, meta);
      this.addEventListener('loadend', () => {
        push('network', {
          transport: 'xhr',
          method: meta.method,
          url: meta.url,
          status: Number.isFinite(this.status) ? this.status : null,
          ok: this.status >= 200 && this.status < 400,
          duration: Math.round((performance.now() - meta.started) * 10) / 10,
        });
      }, { once: true });
      return originalSend.apply(this, args);
    };
  }

  if (config.include_performance && 'PerformanceObserver' in window) {
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          push('long_task', { duration: entry.duration, startTime: entry.startTime });
        }
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
    drain(max = 256) {
      const count = Math.max(0, Math.min(Number(max) || 0, events.length));
      return events.splice(0, count);
    },
    peek(max = 64) { return events.slice(-Math.max(0, Number(max) || 0)); },
    refFor,
  });

  push('instrumentation_ready', { href: safeUrl(location.href) });
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
    fn defaults_capture_metadata_without_bodies() {
        let script = bootstrap_script(&InstrumentationConfig::default());
        assert!(script.contains("include_network"));
        assert!(!script.contains("response.text()"));
        assert!(!script.contains("response.json()"));
    }
}
