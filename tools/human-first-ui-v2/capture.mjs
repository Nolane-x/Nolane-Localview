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
) {
  return page.addInitScript(({ dashboardState, liveState, locale, overrides, rawPreferences, storageFault }) => {
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
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {
        invoke: async (cmd) => {
          if (cmd === 'dashboard_state') return dashboardState;
          if (cmd === 'live_session_state') return liveState;
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
  }, { dashboardState, liveState, locale, overrides, rawPreferences, storageFault });
}

async function pageFor(
  browser,
  viewport,
  locale = 'en',
  overrides = {},
  liveState = live,
  dashboardState = dashboard,
  rawPreferences = null,
  storageFault = false
) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(String(error)));
  await init(page, locale, overrides, liveState, dashboardState, rawPreferences, storageFault);
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
}));
invariant(inspectorActions.count >= 5 && inspectorActions.disabled === inspectorActions.count, 'en-inspector:unwired-actions-fail-closed', inspectorActions);
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

await fs.writeFile(
  'human-first-ui-v2-render/audit.json',
  JSON.stringify(audit, null, 2) + '\\n',
  'utf8'
);
await browser.close();
console.log(`captured ${audit.screenshots.length} human-first UI V2 screenshots with ${audit.checks.length} executable checks`);
