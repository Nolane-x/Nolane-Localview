from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"{label}: expected exactly one occurrence, found {text.count(old)}")
    return text.replace(old, new, 1)


instrumentation_path = Path("crates/instrumentation/src/lib.rs")
text = instrumentation_path.read_text()
start_marker = '            r#"  const VISUAL_FREEZE_LEASE_MS = 8000;'
end_marker = '  window.__LOCALVIEW__ = Object.freeze({"#,'
start = text.find(start_marker)
if start < 0:
    raise SystemExit("instrumentation freeze block start marker not found")
end = text.find(end_marker, start)
if end < 0:
    raise SystemExit("instrumentation freeze block end marker not found")
end += len(end_marker)

replacement = '''            r#"  const VIEWPORT_VISUAL_FREEZE_LEASE_MS = 8000;
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

  window.__LOCALVIEW__ = Object.freeze({"#,'''
text = text[:start] + replacement + text[end:]
text = replace_once(
    text,
    '            "    snapshot,\\n    freezeVisuals,\\n    restoreVisuals,\\n    inspect(reference)",',
    '            "    snapshot,\\n    freezeVisuals,\\n    restoreVisuals,\\n    captureScrollTo,\\n    captureTileProbe,\\n    inspect(reference)",',
    "instrumentation export",
)
instrumentation_path.write_text(text)


desktop_path = Path("apps/desktop/src-tauri/src/lib.rs")
desktop = desktop_path.read_text()
block_start_marker = "      case 'freeze_visuals': {"
block_end_marker = "      case 'restore_visuals':"
block_start = desktop.find(block_start_marker)
if block_start < 0:
    raise SystemExit("desktop freeze action block start not found")
block_end = desktop.find(block_end_marker, block_start)
if block_end < 0:
    raise SystemExit("desktop restore action marker not found")

new_block = '''      case 'freeze_visuals': {
        const requestedLeaseMs = Number(queued.private_capture?.visual_freeze_lease_ms);
        const leaseMs = Number.isFinite(requestedLeaseMs) ? requestedLeaseMs : 8000;
        const frozen = await window.__LOCALVIEW__?.freezeVisuals?.(queued.id, leaseMs) ?? null;
        if (!frozen) throw new Error('visual_freeze_ack_missing');
        try {
          const geometry = privateMaskGeometry(queued.private_capture?.mask_selectors || []);
          return { ...frozen, ...geometry };
        } catch (error) {
          try { window.__LOCALVIEW__?.restoreVisuals?.(queued.id); } catch (_) {}
          throw error;
        }
      }
      case 'capture_scroll_to': {
        const scrolled = await window.__LOCALVIEW__?.captureScrollTo?.(action.token, action.y) ?? null;
        if (!scrolled) throw new Error('capture_scroll_ack_missing');
        return scrolled;
      }
      case 'capture_tile_probe': {
        const probe = await window.__LOCALVIEW__?.captureTileProbe?.(action.token) ?? null;
        if (!probe) throw new Error('capture_tile_probe_ack_missing');
        const geometry = privateMaskGeometry(queued.private_capture?.mask_selectors || []);
        return { ...probe, ...geometry };
      }
'''
desktop = desktop[:block_start] + new_block + desktop[block_end:]
desktop_path.write_text(desktop)


live_bridge_path = Path("crates/live-bridge/src/lib.rs")
live_bridge = live_bridge_path.read_text()
live_bridge = replace_once(
    live_bridge,
    '''    let visible_fixed_or_sticky = result
        .payload
        .get("visible_fixed_or_sticky")
        .and_then(Value::as_u64);''',
    '''    let visible_fixed_or_sticky = result
        .payload
        .get("visible_fixed_or_sticky")
        .and_then(Value::as_bool);''',
    "tile probe boolean metadata",
)
live_bridge = replace_once(
    live_bridge,
    '''        && positional_elements_scanned.is_some_and(|value| value <= MAX_POSITIONAL_SCAN_ELEMENTS)
        && visible_fixed_or_sticky.is_some_and(|value| {
            value <= positional_elements_scanned.unwrap_or_default()
                && value <= MAX_POSITIONAL_SCAN_ELEMENTS
        });''',
    '''        && positional_elements_scanned.is_some_and(|value| value <= MAX_POSITIONAL_SCAN_ELEMENTS)
        && visible_fixed_or_sticky.is_some();''',
    "tile probe boolean validation",
)
live_bridge_path.write_text(live_bridge)
