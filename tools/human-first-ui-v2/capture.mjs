import { chromium } from 'playwright';
import fs from 'node:fs/promises';

const audit = {
  schema: 1,
  generated_at: new Date().toISOString(),
  head_sha: process.env.GITHUB_SHA ?? null,
  checks: [],
  screenshots: [],
};
const pageErrors = new WeakMap();

function invariant(condition, name, details = {}) {
  if (!condition) {
    throw new Error(`render invariant failed: ${name} :: ${JSON.stringify(details)}`);
  }
  audit.checks.push({ name, ...details });
}

async function assertVisible(page, selector, state) {
  const visible = await page.locator(selector).isVisible();
  invariant(visible, `${state}:visible:${selector}`);
}

async function assertHidden(page, selector, state) {
  const visible = await page.locator(selector).isVisible();
  invariant(!visible, `${state}:hidden:${selector}`);
}

async function assertDocumentLocale(page, locale, state) {
  const actual = await page.evaluate(() => document.documentElement.lang);
  invariant(actual === locale, `${state}:document-locale`, { expected: locale, actual });
}

async function assertNoHorizontalOverflow(page, state) {
  const geometry = await page.evaluate(() => ({
    innerWidth: window.innerWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  invariant(
    geometry.scrollWidth <= geometry.innerWidth + 1,
    `${state}:no-horizontal-overflow`,
    geometry,
  );
}

async function assertVisibleButtonsNamed(page, state) {
  const unnamed = await page.locator('button:visible').evaluateAll((buttons) =>
    buttons
      .filter((button) => {
        const aria = button.getAttribute('aria-label')?.trim();
        const title = button.getAttribute('title')?.trim();
        const text = button.textContent?.trim();
        return !aria && !title && !text;
      })
      .map((button) => button.outerHTML.slice(0, 180))
  );
  invariant(unnamed.length === 0, `${state}:visible-buttons-named`, { unnamed });
}

async function assertNoPageErrors(page, state) {
  const errors = pageErrors.get(page) ?? [];
  invariant(errors.length === 0, `${state}:no-page-errors`, { errors });
}

async function assertPrimaryControlsInViewport(page, state) {
  const result = await page.locator('.top-pill, .floating-rail').evaluateAll((nodes) => {
    const width = window.innerWidth;
    const height = window.innerHeight;
    return nodes
      .filter((node) => {
        const style = getComputedStyle(node);
        return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) > 0;
      })
      .map((node) => {
        const rect = node.getBoundingClientRect();
        return {
          className: node.className,
          left: rect.left,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
          withinViewport:
            rect.right > 0 &&
            rect.bottom > 0 &&
            rect.left < width &&
            rect.top < height &&
            rect.width > 0 &&
            rect.height > 0,
        };
      });
  });
  invariant(
    result.length > 0 && result.every((entry) => entry.withinViewport),
    `${state}:primary-controls-in-viewport`,
    { result },
  );
}

async function readStoredPreferences(page) {
  return page.evaluate(() => {
    const raw = localStorage.getItem('localview.preferences.v2');
    return raw ? JSON.parse(raw) : null;
  });
}

async function shot(page, filename, state = filename) {
  await assertNoHorizontalOverflow(page, state);
  await assertVisibleButtonsNamed(page, state);
  await assertNoPageErrors(page, state);
  await page.screenshot({ path: `human-first-ui-v2-render/${filename}`, fullPage: true });
  audit.screenshots.push({ filename, state });
}

const now = new Date().toISOString();
const dashboard = {
  health: { version: '0.2.0', status: 'healthy', paused: false, sessions: 1 },
  sessions: [{
    id: '11111111-1111-4111-8111-111111111111',
    endpoint: { host: '127.0.0.1', port: 5173, scheme: 'http' },
    classification: {
      kind: 'frontend_dev_server',
      confidence: 0.99,
      framework: 'React + Vite',
      title: 'Nolane Studio',
      hmr_detected: true,
      evidence: ['vite']
    },
    project: {
      key: 'nolane-studio',
      display_name: 'Nolane Studio',
      cwd: '/workspace/nolane-studio',
      git_root: '/workspace/nolane-studio',
      pid: 4217,
      command: 'npm run dev'
    },
    status: 'active',
    first_seen: now,
    last_seen: now,
    preview_visible: true
  }],
  engine: { native: 'Tauri / WRY', tier3: 'Chromium on demand' },
  capabilities: ['semantic','layout','network','console','visual'],
  workspace_surface: { compiled: false, default_mode: 'iframe', reason: 'render audit' }
};

const live = {
  observer: [
    { seq: 1, captured_at: now, kind: 'semantic_snapshot', route: '/', payload: { nodes: 148, interactive: 23, viewport: [1440,900] } },
    { seq: 2, captured_at: now, kind: 'focus', reference: 'button#deploy', route: '/', payload: { role: 'button', name: 'Deploy', source: 'src/components/DeployButton.tsx:42' } },
    { seq: 3, captured_at: now, kind: 'console', route: '/', payload: { level: 'warn', message: 'Deprecated theme token' } },
    { seq: 4, captured_at: now, kind: 'network', route: '/', payload: { method: 'GET', status: 200, duration: 42.6, url: '/api/projects' } }
  ],
  action_results: []
};

const liveNoFocus = {
  ...live,
  observer: live.observer.filter((event) => event.kind !== 'focus')
};

const liveMeasure = {
  ...live,
  observer: live.observer.map((event) =>
    event.kind === 'focus'
      ? { ...event, reference: '@e1a2b3c4' }
      : event
  ),
};

const liveMeasureB = {
  ...liveMeasure,
  observer: liveMeasure.observer.map((event) =>
    event.kind === 'focus'
      ? { ...event, reference: '@e5d6e7f8' }
      : event
  ),
};

const dashboardNoTarget = {
  ...dashboard,
  health: { ...dashboard.health, sessions: 0 },
  sessions: []
};

const dashboardDisconnected = {
  ...dashboard,
  sessions: dashboard.sessions.map((session) => ({ ...session, status: 'disconnected' }))
};

const dashboardLongTarget = {
  ...dashboard,
  sessions: dashboard.sessions.map((session) => ({
    ...session,
    classification: {
      ...session.classification,
      title: 'Extremely long local development target title '.repeat(8).trim(),
    },
    project: {
      ...session.project,
      display_name: 'Extremely long LocalView project name '.repeat(10).trim(),
    },
  })),
};

const liveEmpty = { observer: [], action_results: [] };

function init(
  page,
  locale = 'en',
  overrides = {},
  liveState = live,
  dashboardState = dashboard,
  rawPreferences = null,
  storageFault = false,
  failedCommands = [],
  captureDelayMs = 0,
  measureDelayMs = 0,
) {
  return page.addInitScript(({ dashboardState, liveState, locale, overrides, rawPreferences, storageFault, failedCommands, captureDelayMs, measureDelayMs }) => {
    if (storageFault) {
      Storage.prototype.getItem = () => {
        throw new DOMException('storage disabled by render audit', 'SecurityError');
      };
      Storage.prototype.setItem = () => {
        throw new DOMException('storage disabled by render audit', 'SecurityError');
      };
    }
    const validPreferences = JSON.stringify({
      version: 2,
      locale,
      showTargetBar: true,
      showToolRail: true,
      rememberChromePositions: true,
      targetBarPosition: null,
      toolRailPosition: null,
      annotationPersistence: 'session',
      notifications: 'important',
      autoOpen: 'first_session',
      reducedMotion: 'system',
      density: 'comfortable',
      accent: 'muted-moss',
      ...overrides
    });
    if (!storageFault) {
      localStorage.setItem('localview.preferences.v2', rawPreferences ?? validPreferences);
    }
    window.__LOCALVIEW_AUDIT_INVOKES__ = [];
    window.__LOCALVIEW_AUDIT_LIVE_STATE__ = structuredClone(liveState);
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {
        invoke: async (cmd, args = {}) => {
          window.__LOCALVIEW_AUDIT_INVOKES__.push({ cmd, args: structuredClone(args ?? {}) });
          if (failedCommands.includes(cmd)) {
            throw new Error('forced audit failure for ' + cmd);
          }
          if (cmd === 'dashboard_state') return dashboardState;
          if (cmd === 'live_session_state') {
            return structuredClone(window.__LOCALVIEW_AUDIT_LIVE_STATE__);
          }
          if (cmd === 'measure_current_selection') {
            if (measureDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, measureDelayMs));
            }
            return {
              reference: args.reference,
              rect: { x: 24.0, y: 132.5, width: 128.4, height: 40.0 },
              document_rect: { x: 24.0, y: 332.5, width: 128.4, height: 40.0 },
              viewport_css_width: 1440,
              viewport_css_height: 900,
              route: 'http://127.0.0.1:5173/',
              measured_at_unix_ms: Date.now(),
            };
          }
          if (cmd === 'capture_current_viewport') {
            if (captureDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, captureDelayMs));
            }
            return {
              artifact_id: 'artifact-v21-audit',
              evidence_id: 'evidence-v21-0123456789abcdef',
              deduplicated: false,
              backend: 'audit-native',
              route: 'http://127.0.0.1:5173/',
              viewport: { css_width: 1440, css_height: 900, device_scale_factor: 1 },
              pixel_width: 1440,
              pixel_height: 900,
              revision: null,
              captured_at_unix_ms: Date.now(),
              target: 'viewport',
              region: null,
            };
          }
          if (['pause_runtime','resume_runtime','open_preview','workspace_surface_open','workspace_surface_set_bounds','workspace_surface_navigate','workspace_surface_close'].includes(cmd)) return null;
          throw new Error('audit stub missing ' + cmd);
        },
        transformCallback: (callback) => {
          const id = Math.floor(Math.random() * 1000000000);
          window['_' + id] = callback;
          return id;
        },
        convertFileSrc: (path) => path
      }
    });
  }, { dashboardState, liveState, locale, overrides, rawPreferences, storageFault, failedCommands, captureDelayMs, measureDelayMs });
}

async function pageFor(
  browser,
  viewport,
  locale = 'en',
  overrides = {},
  liveState = live,
  dashboardState = dashboard,
  rawPreferences = null,
  storageFault = false,
  failedCommands = [],
  captureDelayMs = 0,
  measureDelayMs = 0
) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(String(error)));
  await init(page, locale, overrides, liveState, dashboardState, rawPreferences, storageFault, failedCommands, captureDelayMs, measureDelayMs);
  await page.goto('http://127.0.0.1:1420/', { waitUntil: 'networkidle' });
  await page.waitForTimeout(500);
  return page;
}

await fs.mkdir('human-first-ui-v2-render', { recursive: true });
const browser = await chromium.launch({ headless: true });

let page = await pageFor(browser, { width: 1440, height: 900 });
await assertVisible(page, '.top-pill', 'en-overview');
await assertVisible(page, '.floating-rail', 'en-overview');
await assertDocumentLocale(page, 'en', 'en-overview');
await shot(page, '01-en-overview.png');
await page.keyboard.press('i');
await page.waitForTimeout(150);
await assertVisible(page, '.panel-inspect', 'en-inspector');
const inspectorText = await page.locator('.panel-inspect').innerText();
invariant(!/Semantic Snapshot|Project identity|X-Ray pipeline/.test(inspectorText), 'en-inspector:no-machine-diagnostics');
const inspectorActions = await page.locator('.quick-action-grid button').evaluateAll((buttons) => ({
  count: buttons.length,
  disabled: buttons.filter((button) => button.disabled).length,
  captureEnabled: !buttons.find((button) => button.classList.contains('capture-action'))?.disabled,
}));
invariant(
  inspectorActions.count >= 5 &&
    inspectorActions.disabled === inspectorActions.count - 1 &&
    inspectorActions.captureEnabled,
  'en-inspector:only-trusted-capture-enabled',
  inspectorActions,
);
await shot(page, '02-en-inspector.png');
await page.keyboard.press('Escape');
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await assertVisible(page, '.panel-settings', 'en-settings');
await assertDocumentLocale(page, 'en', 'en-settings');
await shot(page, '03-en-settings.png');
await page.getByLabel('Show target bar').setChecked(false);
await assertHidden(page, '.top-pill', 'settings-target-hidden');
await page.getByLabel('Show tool rail').setChecked(false);
await assertHidden(page, '.floating-rail', 'settings-rail-hidden');
await page.getByRole('button', { name: 'Reset workspace' }).click();
await assertVisible(page, '.top-pill', 'settings-reset-recovered');
await assertVisible(page, '.floating-rail', 'settings-reset-recovered');
const settingsRecoveredPreferences = await readStoredPreferences(page);
invariant(settingsRecoveredPreferences?.showTargetBar === true, 'settings-reset-recovered:target-visible');
invariant(settingsRecoveredPreferences?.showToolRail === true, 'settings-reset-recovered:rail-visible');
await shot(page, '20-settings-reset-recovered.png', 'settings-reset-recovered');
await page.keyboard.press('Escape');
await page.keyboard.press('m');
await page.waitForTimeout(150);
await assertVisible(page, '.panel-advanced', 'en-advanced');
const advancedText = await page.locator('.panel-advanced').innerText();
invariant(advancedText.includes('Project identity'), 'en-advanced:diagnostics-present');
await shot(page, '04-en-advanced.png');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'vi');
await assertDocumentLocale(page, 'vi', 'vi-settings');
const viChromeLabel = await page.locator('.chrome-layer').getAttribute('aria-label');
invariant(viChromeLabel === 'Điều khiển LocalView', 'vi-accessibility:chrome-label', { viChromeLabel });
const viToolRailLabel = await page.locator('.floating-rail').getAttribute('aria-label');
invariant(viToolRailLabel === 'Công cụ LocalView', 'vi-accessibility:tool-rail-label', { viToolRailLabel });
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
const viSettingsPanelLabel = await page.locator('.panel-settings').getAttribute('aria-label');
invariant(viSettingsPanelLabel === 'Cài đặt', 'vi-accessibility:settings-panel-label', { viSettingsPanelLabel });
const viSettingsEyebrow = await page.locator('.panel-settings .panel-header > div > span').innerText();
invariant(viSettingsEyebrow === 'Tùy chọn', 'vi-accessibility:settings-eyebrow', { viSettingsEyebrow });
await shot(page, '05-vi-settings.png');
await page.locator('.panel-settings select').selectOption('en');
await assertDocumentLocale(page, 'en', 'settings-live-locale-switch-en');
let liveLocalePreferences = await readStoredPreferences(page);
invariant(liveLocalePreferences?.locale === 'en', 'settings-live-locale-switch-en:persisted');
await page.locator('.panel-settings select').selectOption('vi');
await assertDocumentLocale(page, 'vi', 'settings-live-locale-switch-vi');
liveLocalePreferences = await readStoredPreferences(page);
invariant(liveLocalePreferences?.locale === 'vi', 'settings-live-locale-switch-vi:persisted');
await shot(page, '21-settings-live-locale-switch.png', 'settings-live-locale-switch-vi');
await page.keyboard.press('Escape');
await page.keyboard.press('i');
await page.waitForTimeout(150);
const viInspectorPanelLabel = await page.locator('.panel-inspect').getAttribute('aria-label');
invariant(viInspectorPanelLabel === 'Kiểm tra', 'vi-accessibility:inspector-panel-label', { viInspectorPanelLabel });
const viInspectorActionsLabel = await page.locator('.quick-action-grid').getAttribute('aria-label');
invariant(viInspectorActionsLabel === 'Thao tác kiểm tra', 'vi-accessibility:inspector-actions-label', { viInspectorActionsLabel });
const viInspectorEyebrow = await page.locator('.panel-inspect .panel-header > div > span').innerText();
invariant(viInspectorEyebrow === 'Công cụ', 'vi-accessibility:inspector-eyebrow', { viInspectorEyebrow });
await shot(page, '24-vi-inspector-accessibility.png', 'vi-inspector-accessibility');
await page.keyboard.press('Escape');
await page.keyboard.press('c');
await page.waitForTimeout(150);
const viConsoleText = await page.locator('.panel-console').innerText();
invariant(viConsoleText.includes('Trực tiếp'), 'vi-console:localized-live', { viConsoleText });
invariant(viConsoleText.includes('1 sự kiện'), 'vi-console:localized-event-count', { viConsoleText });
await shot(page, '25-vi-console.png', 'vi-console');
await page.keyboard.press('Escape');
await page.keyboard.press('n');
await page.waitForTimeout(150);
const viNetworkText = await page.locator('.panel-network').innerText();
invariant(viNetworkText.includes('MỤC TIÊU'), 'vi-network:localized-target', { viNetworkText });
invariant(viNetworkText.includes('YÊU CẦU'), 'vi-network:localized-requests', { viNetworkText });
invariant(viNetworkText.includes('LỖI'), 'vi-network:localized-failures', { viNetworkText });
await shot(page, '26-vi-network.png', 'vi-network');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'zh-CN');
await assertDocumentLocale(page, 'zh-CN', 'zh-cn-settings');
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await shot(page, '06-zh-cn-settings.png');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', { showTargetBar: false });
await assertHidden(page, '.top-pill', 'target-bar-hidden');
await shot(page, '07-target-bar-hidden.png');
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(150);
await assertVisible(page, '.top-pill', 'target-bar-restored');
await shot(page, '08-target-bar-restored.png');
await page.close();

page = await pageFor(browser, { width: 390, height: 844 });
await shot(page, '09-mobile-overview.png');
await page.keyboard.press('i');
await page.waitForTimeout(150);
await shot(page, '10-mobile-inspector.png');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', {}, liveNoFocus);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await shot(page, '11-en-inspector-no-selection.png');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', { showToolRail: false });
await assertHidden(page, '.floating-rail', 'tool-rail-hidden');
await shot(page, '12-tool-rail-hidden.png');
await page.keyboard.press('Control+k');
await page.waitForTimeout(150);
await page.getByRole('button', { name: 'Show tool rail' }).click();
await page.keyboard.press('Escape');
await page.waitForTimeout(150);
await assertVisible(page, '.floating-rail', 'tool-rail-restored');
await shot(page, '13-tool-rail-restored.png');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'vi',
  {},
  liveEmpty,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await assertVisible(page, '.attach-notice', 'vi-live-inspection-unavailable');
const viAttachText = await page.locator('.attach-notice').innerText();
invariant(viAttachText.includes('Kiểm tra trực tiếp chưa được kết nối'), 'vi-live-inspection-unavailable:localized-title', { viAttachText });
invariant(viAttachText.includes('Mở bản xem trước cô lập'), 'vi-live-inspection-unavailable:localized-guidance', { viAttachText });
await shot(page, '27-vi-live-inspection-unavailable.png', 'vi-live-inspection-unavailable');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveEmpty,
  dashboardNoTarget
);
await assertVisible(page, '.workspace-empty', 'no-target');
await shot(page, '14-no-target.png');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'vi',
  {},
  liveEmpty,
  dashboardNoTarget
);
await assertVisible(page, '.workspace-empty', 'vi-no-target');
await assertDocumentLocale(page, 'vi', 'vi-no-target');
const viNoTargetText = await page.locator('.workspace-empty').innerText();
invariant(viNoTargetText.includes('Chưa phát hiện ứng dụng'), 'vi-no-target:localized-title', { viNoTargetText });
invariant(viNoTargetText.includes('Chạy dev server để bắt đầu'), 'vi-no-target:localized-guidance', { viNoTargetText });
await shot(page, '22-vi-no-target.png', 'vi-no-target');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'vi',
  {},
  live,
  dashboardDisconnected
);
await assertVisible(page, '.disconnect-shade', 'vi-disconnected');
await assertDocumentLocale(page, 'vi', 'vi-disconnected');
const viDisconnectedText = await page.locator('.disconnect-shade').innerText();
invariant(
  viDisconnectedText.includes('Máy chủ phát triển đã ngắt kết nối'),
  'vi-disconnected:localized-title',
  { viDisconnectedText },
);
invariant(
  viDisconnectedText.includes('LocalView đang giữ phiên này trong khi chờ máy chủ phát triển kết nối lại.'),
  'vi-disconnected:localized-guidance',
  { viDisconnectedText },
);
await shot(page, '23-vi-disconnected.png', 'vi-disconnected');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 });
await page.keyboard.press('a');
await page.waitForTimeout(150);
await assertVisible(page, '.panel-ai', 'ai-unavailable');
const aiText = await page.locator('.panel-ai').innerText();
invariant(aiText.includes('AI provider not connected'), 'ai-unavailable:explicit-provider-state');
await shot(page, '15-ai-unavailable.png');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  '{ malformed json'
);
await assertDocumentLocale(page, 'en', 'malformed-preferences-recovered');
await assertVisible(page, '.top-pill', 'malformed-preferences-recovered');
await assertVisible(page, '.floating-rail', 'malformed-preferences-recovered');
await shot(page, '16-malformed-preferences-recovered.png', 'malformed-preferences-recovered');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', { reducedMotion: 'reduce' });
await assertVisible(page, '.is-reduced-motion', 'explicit-reduced-motion');
await page.keyboard.press('i');
await page.waitForTimeout(50);
await assertVisible(page, '.panel-inspect', 'explicit-reduced-motion');
const reducedMotionDuration = await page.locator('.panel-inspect').evaluate((element) =>
  getComputedStyle(element).animationDuration
);
invariant(
  Number.parseFloat(reducedMotionDuration) <= 0.001,
  'explicit-reduced-motion:animation-collapsed',
  { animationDuration: reducedMotionDuration },
);
await shot(page, '17-explicit-reduced-motion.png', 'explicit-reduced-motion');
await page.close();

const invalidPreferencePayload = '{"version":1,"locale":"xx-ZZ","showTargetBar":"yes","showToolRail":null,"rememberChromePositions":"no","targetBarPosition":{"x":1e309,"y":12},"toolRailPosition":{"x":12,"y":1e309},"annotationPersistence":"forever","notifications":"everything","autoOpen":"magic","reducedMotion":"spin","density":"tiny","accent":"electric-blue"}';
page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  invalidPreferencePayload
);
await assertDocumentLocale(page, 'en', 'invalid-preferences-normalized');
await assertVisible(page, '.top-pill', 'invalid-preferences-normalized');
await assertVisible(page, '.floating-rail', 'invalid-preferences-normalized');
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(100);
await assertHidden(page, '.top-pill', 'invalid-preferences-normalized');
const normalizedPreferences = await readStoredPreferences(page);
invariant(normalizedPreferences?.version === 2, 'invalid-preferences-normalized:version');
invariant(normalizedPreferences?.locale === 'en', 'invalid-preferences-normalized:locale');
invariant(normalizedPreferences?.showTargetBar === false, 'invalid-preferences-normalized:target-toggle-persists');
invariant(normalizedPreferences?.showToolRail === true, 'invalid-preferences-normalized:tool-rail-default');
invariant(normalizedPreferences?.rememberChromePositions === true, 'invalid-preferences-normalized:remember-default');
invariant(normalizedPreferences?.targetBarPosition === null, 'invalid-preferences-normalized:target-position-rejected');
invariant(normalizedPreferences?.toolRailPosition === null, 'invalid-preferences-normalized:rail-position-rejected');
invariant(normalizedPreferences?.annotationPersistence === 'session', 'invalid-preferences-normalized:annotation');
invariant(normalizedPreferences?.notifications === 'important', 'invalid-preferences-normalized:notifications');
invariant(normalizedPreferences?.autoOpen === 'first_session', 'invalid-preferences-normalized:auto-open');
invariant(normalizedPreferences?.reducedMotion === 'system', 'invalid-preferences-normalized:motion');
invariant(normalizedPreferences?.density === 'comfortable', 'invalid-preferences-normalized:density');
invariant(normalizedPreferences?.accent === 'muted-moss', 'invalid-preferences-normalized:accent');
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(100);
await assertVisible(page, '.top-pill', 'invalid-preferences-normalized');
await shot(page, '18-invalid-preferences-normalized.png', 'invalid-preferences-normalized');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  '{"locale":"vi"}'
);
await assertDocumentLocale(page, 'vi', 'partial-preferences-recovered');
await assertVisible(page, '.top-pill', 'partial-preferences-recovered');
await assertVisible(page, '.floating-rail', 'partial-preferences-recovered');
await shot(page, '19-partial-preferences-recovered.png', 'partial-preferences-recovered');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  null,
  true
);
await assertDocumentLocale(page, 'en', 'storage-unavailable');
await assertVisible(page, '.top-pill', 'storage-unavailable');
await assertVisible(page, '.floating-rail', 'storage-unavailable');
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(100);
await assertHidden(page, '.top-pill', 'storage-unavailable-toggle');
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(100);
await assertVisible(page, '.top-pill', 'storage-unavailable-recovered');
await assertPrimaryControlsInViewport(page, 'storage-unavailable-recovered');
await shot(page, '28-storage-unavailable.png', 'storage-unavailable-recovered');
await page.close();

page = await pageFor(
  browser,
  { width: 320, height: 568 },
  'en',
  {},
  live,
  dashboardLongTarget
);
await assertVisible(page, '.top-pill', 'narrow-long-target');
await assertVisible(page, '.floating-rail', 'narrow-long-target');
await assertPrimaryControlsInViewport(page, 'narrow-long-target');
await page.keyboard.press('c');
await page.waitForTimeout(150);
await assertVisible(page, '.panel-console', 'narrow-long-target-console');
await assertPrimaryControlsInViewport(page, 'narrow-long-target-console');
await shot(page, '29-narrow-long-target-console.png', 'narrow-long-target-console');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  null,
  false,
  ['open_preview']
);
await assertVisible(page, '.top-pill', 'runtime-action-failure-isolated');
await assertVisible(page, '.floating-rail', 'runtime-action-failure-isolated');
await page.getByLabel('Open preview').click();
await page.waitForTimeout(100);
await assertVisible(page, '.runtime-toast', 'runtime-action-failure-isolated');
const isolatedRuntimeText = await page.locator('.runtime-toast').innerText();
invariant(
  isolatedRuntimeText.includes('Runtime unavailable'),
  'runtime-action-failure-isolated:humanized-error',
  { isolatedRuntimeText },
);
invariant(
  !isolatedRuntimeText.includes('forced audit failure'),
  'runtime-action-failure-isolated:no-raw-error',
  { isolatedRuntimeText },
);
await assertPrimaryControlsInViewport(page, 'runtime-action-failure-isolated');
await shot(page, '30-runtime-action-failure-isolated.png', 'runtime-action-failure-isolated');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 });
await page.keyboard.press('i');
await page.waitForTimeout(150);
await assertVisible(page, '.capture-action', 'trusted-capture-success');
await page.locator('.capture-action').click();
await page.waitForTimeout(100);
await assertVisible(page, '.capture-status.success', 'trusted-capture-success');
const trustedCaptureSuccessText = await page.locator('.capture-status.success').innerText();
invariant(
  trustedCaptureSuccessText.includes('Captured current viewport'),
  'trusted-capture-success:human-copy',
  { trustedCaptureSuccessText },
);
invariant(
  trustedCaptureSuccessText.includes('1440×900'),
  'trusted-capture-success:pixel-metadata',
  { trustedCaptureSuccessText },
);
const trustedCaptureInvokes = await page.evaluate(() => window.__LOCALVIEW_AUDIT_INVOKES__);
const trustedCaptureCall = trustedCaptureInvokes.find((entry) => entry.cmd === 'capture_current_viewport');
invariant(!!trustedCaptureCall, 'trusted-capture-success:command-invoked', { trustedCaptureInvokes });
invariant(
  trustedCaptureCall?.args?.sessionId === dashboard.sessions[0].id,
  'trusted-capture-success:session-authority',
  { args: trustedCaptureCall?.args },
);
invariant(
  !trustedCaptureCall?.args || !Object.prototype.hasOwnProperty.call(trustedCaptureCall.args, 'viewport'),
  'trusted-capture-success:no-caller-viewport',
  { args: trustedCaptureCall?.args },
);
await shot(page, '31-trusted-capture-success.png', 'trusted-capture-success');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  live,
  dashboard,
  null,
  false,
  ['capture_current_viewport']
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.capture-action').click();
await page.waitForTimeout(100);
await assertVisible(page, '.capture-status.failure', 'trusted-capture-failure');
const trustedCaptureFailureText = await page.locator('.capture-status.failure').innerText();
invariant(
  trustedCaptureFailureText.includes('Could not capture current viewport'),
  'trusted-capture-failure:human-copy',
  { trustedCaptureFailureText },
);
invariant(
  !trustedCaptureFailureText.includes('forced audit failure'),
  'trusted-capture-failure:no-raw-error',
  { trustedCaptureFailureText },
);
await shot(page, '32-trusted-capture-failure.png', 'trusted-capture-failure');
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'vi');
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.capture-action').click();
await page.waitForTimeout(100);
await assertVisible(page, '.capture-status.success', 'vi-trusted-capture-success');
const viTrustedCaptureText = await page.locator('.capture-status.success').innerText();
invariant(
  viTrustedCaptureText.includes('Đã chụp khung nhìn hiện tại'),
  'vi-trusted-capture-success:localized',
  { viTrustedCaptureText },
);
await shot(page, '33-vi-trusted-capture-success.png', 'vi-trusted-capture-success');
await page.close();

page = await pageFor(
  browser,
  { width: 390, height: 844 },
  'en',
  {},
  liveEmpty,
  dashboardNoTarget
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await assertVisible(page, '.capture-action', 'no-target-capture-disabled');
const noTargetCaptureDisabled = await page.locator('.capture-action').isDisabled();
invariant(noTargetCaptureDisabled, 'no-target-capture-disabled:semantic-disabled');
const noTargetInvokes = await page.evaluate(() => window.__LOCALVIEW_AUDIT_INVOKES__);
invariant(
  !noTargetInvokes.some((entry) => entry.cmd === 'capture_current_viewport'),
  'no-target-capture-disabled:not-invoked',
  { noTargetInvokes },
);
await shot(page, '34-no-target-capture-disabled.png', 'no-target-capture-disabled');
await page.close();

page = await pageFor(
  browser,
  { width: 390, height: 844 },
  'en',
  {},
  live,
  dashboard,
  null,
  false,
  [],
  350
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
const inFlightCapture = page.locator('.capture-action');
await inFlightCapture.click();
await page.waitForTimeout(60);
const captureBusy = await inFlightCapture.getAttribute('aria-busy');
const capturingText = await inFlightCapture.innerText();
invariant(captureBusy === 'true', 'trusted-capture-in-progress:aria-busy', { captureBusy });
invariant(capturingText.includes('Capturing'), 'trusted-capture-in-progress:label', { capturingText });
const inFlightInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'capture_current_viewport')
);
invariant(inFlightInvokes.length === 1, 'trusted-capture-in-progress:single-request', { inFlightInvokes });
await shot(page, '35-trusted-capture-in-progress.png', 'trusted-capture-in-progress');
await page.waitForTimeout(400);
await assertVisible(page, '.capture-status.success', 'trusted-capture-in-progress-completes');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
const measureReadyButton = page.locator('.measure-action');
invariant(
  !(await measureReadyButton.isDisabled()),
  'trusted-measure-ready:stable-reference-enabled'
);
const measureReadyInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(measureReadyInvokes.length === 0, 'trusted-measure-ready:not-invoked');
await shot(page, '36-trusted-measure-ready.png', 'trusted-measure-ready');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(120);
await assertVisible(page, '.measure-status.success', 'trusted-measure-success');
const trustedMeasureSuccessText = await page.locator('.measure-status.success').innerText();
invariant(
  trustedMeasureSuccessText.includes('Measured 128.4 × 40 CSS px'),
  'trusted-measure-success:geometry',
  { trustedMeasureSuccessText }
);
invariant(
  trustedMeasureSuccessText.includes('Position x 24 · y 132.5'),
  'trusted-measure-success:position',
  { trustedMeasureSuccessText }
);
const measureSuccessInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(measureSuccessInvokes.length === 1, 'trusted-measure-success:measure-single-request', { measureSuccessInvokes });
const measureArgs = measureSuccessInvokes[0]?.args ?? {};
invariant(
  Object.keys(measureArgs).sort().join(',') === 'reference,sessionId'
    && measureArgs.reference === '@e1a2b3c4',
  'trusted-measure-success:authority-payload',
  { measureArgs }
);
const callerGeometryFields = ['x', 'y', 'width', 'height', 'viewport', 'route']
  .filter((field) => field in measureArgs);
invariant(
  callerGeometryFields.length === 0,
  'trusted-measure-success:no-caller-geometry',
  { measureArgs, callerGeometryFields }
);
for (const forbidden of ['x', 'y', 'width', 'height', 'viewport', 'route']) {
  invariant(!(forbidden in measureArgs), `trusted-measure-success:no-caller-${forbidden}`, { measureArgs });
}
await shot(page, '37-trusted-measure-success.png', 'trusted-measure-success');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboard,
  null,
  false,
  ['measure_current_selection']
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(120);
await assertVisible(page, '.measure-status.failure', 'trusted-measure-failure');
const trustedMeasureFailureText = await page.locator('.panel-inspect').innerText();
invariant(
  trustedMeasureFailureText.includes('Could not measure selected element'),
  'trusted-measure-failure:humanized',
  { trustedMeasureFailureText }
);
invariant(
  !trustedMeasureFailureText.includes('forced audit failure'),
  'trusted-measure-failure:measure-no-raw-error',
  { trustedMeasureFailureText }
);
await shot(page, '38-trusted-measure-failure.png', 'trusted-measure-failure');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveNoFocus,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
const noSelectionMeasure = page.locator('.measure-action');
invariant(await noSelectionMeasure.isDisabled(), 'no-selection-measure-disabled:disabled');
const noSelectionMeasureInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(
  noSelectionMeasureInvokes.length === 0,
  'no-selection-measure-disabled:not-invoked',
  { noSelectionMeasureInvokes }
);
await shot(page, '39-no-selection-measure-disabled.png', 'no-selection-measure-disabled');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboard,
  null,
  false,
  [],
  0,
  700
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(80);
const inFlightMeasureButton = page.locator('.measure-action');
invariant(await inFlightMeasureButton.isDisabled(), 'trusted-measure-in-progress:disabled');
invariant(
  (await inFlightMeasureButton.getAttribute('aria-busy')) === 'true',
  'trusted-measure-in-progress:aria-busy'
);
await inFlightMeasureButton.evaluate((button) => button.click());
await page.waitForTimeout(40);
const measureInFlightInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(
  measureInFlightInvokes.length === 1,
  'trusted-measure-in-progress:measure-single-request',
  { measureInFlightInvokes }
);
await shot(page, '40-trusted-measure-in-progress.png', 'trusted-measure-in-progress');
await page.waitForTimeout(700);
await assertVisible(page, '.measure-status.success', 'trusted-measure-in-progress-completes');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'vi',
  {},
  liveMeasure,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(120);
await assertVisible(page, '.measure-status.success', 'vi-trusted-measure-success');
const viMeasureSuccessText = await page.locator('.measure-status.success').innerText();
invariant(
  viMeasureSuccessText.includes('Đã đo 128.4 × 40 CSS px')
    && viMeasureSuccessText.includes('Vị trí x 24 · y 132.5'),
  'vi-trusted-measure-success:localized',
  { viMeasureSuccessText }
);
await shot(page, '41-vi-trusted-measure-success.png', 'vi-trusted-measure-success');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboard,
  null,
  false,
  [],
  0,
  900
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(80);
await page.evaluate((nextLive) => {
  window.__LOCALVIEW_AUDIT_LIVE_STATE__ = structuredClone(nextLive);
}, liveMeasureB);
await page.waitForFunction(
  () => document.querySelector('.inspector-hero strong')?.textContent?.includes('@e5d6e7f8'),
  null,
  { timeout: 1800 }
);
await page.waitForTimeout(1000);
const stalePanelText = await page.locator('.panel-inspect').innerText();
invariant(
  stalePanelText.includes('@e5d6e7f8') && !stalePanelText.includes('Measured 128.4 × 40 CSS px'),
  'trusted-measure-stale-selection:stale-measure-result-discarded',
  { stalePanelText }
);
const staleMeasureInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(
  staleMeasureInvokes.length === 1,
  'trusted-measure-stale-selection:single-original-request',
  { staleMeasureInvokes }
);
await shot(page, '42-trusted-measure-stale-selection.png', 'trusted-measure-stale-selection');
await page.close();

page = await pageFor(
  browser,
  { width: 390, height: 844 },
  'en',
  {},
  liveMeasure,
  dashboard
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(120);
await assertVisible(page, '.measure-status.success', 'narrow-trusted-measure-success');
await assertPrimaryControlsInViewport(page, 'narrow-trusted-measure-success');
const narrowMeasurePanel = await page.locator('.panel-inspect').evaluate((node) => {
  const rect = node.getBoundingClientRect();
  return {
    left: rect.left,
    right: rect.right,
    top: rect.top,
    bottom: rect.bottom,
    viewportWidth: window.innerWidth,
    viewportHeight: window.innerHeight,
  };
});
invariant(
  narrowMeasurePanel.left >= 0
    && narrowMeasurePanel.right <= narrowMeasurePanel.viewportWidth + 1
    && narrowMeasurePanel.top >= 0
    && narrowMeasurePanel.bottom <= narrowMeasurePanel.viewportHeight + 1,
  'narrow-trusted-measure-success:panel-in-viewport',
  { narrowMeasurePanel }
);
await shot(page, '43-narrow-trusted-measure-success.png', 'narrow-trusted-measure-success');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveMeasure,
  dashboardNoTarget
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
const noSessionMeasure = page.locator('.measure-action');
invariant(await noSessionMeasure.isDisabled(), 'no-session-measure-disabled:disabled');
const noSessionMeasureInvokes = await page.evaluate(() =>
  window.__LOCALVIEW_AUDIT_INVOKES__.filter((entry) => entry.cmd === 'measure_current_selection')
);
invariant(
  noSessionMeasureInvokes.length === 0,
  'no-session-measure-disabled:not-invoked',
  { noSessionMeasureInvokes }
);
await shot(page, '44-no-session-measure-disabled.png', 'no-session-measure-disabled');
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'vi',
  {},
  liveMeasure,
  dashboard,
  null,
  false,
  ['measure_current_selection']
);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.locator('.measure-action').click();
await page.waitForTimeout(120);
await assertVisible(page, '.measure-status.failure', 'vi-trusted-measure-failure');
const viMeasureFailureText = await page.locator('.measure-status.failure').innerText();
invariant(
  viMeasureFailureText.includes('Không thể đo phần tử đã chọn'),
  'vi-trusted-measure-failure:localized',
  { viMeasureFailureText }
);
await shot(page, '45-vi-trusted-measure-failure.png', 'vi-trusted-measure-failure');
await page.close();

await fs.writeFile(
  'human-first-ui-v2-render/audit.json',
  JSON.stringify(audit, null, 2) + '\\n',
  'utf8'
);
await browser.close();
console.log(`captured ${audit.screenshots.length} human-first UI V2 screenshots with ${audit.checks.length} executable checks`);
