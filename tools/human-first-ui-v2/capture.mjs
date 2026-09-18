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

const liveEmpty = { observer: [], action_results: [] };

function init(page, locale = 'en', overrides = {}, liveState = live, dashboardState = dashboard, rawPreferences = null) {
  return page.addInitScript(({ dashboardState, liveState, locale, overrides, rawPreferences }) => {
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
    localStorage.setItem('localview.preferences.v2', rawPreferences ?? validPreferences);
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
  }, { dashboardState, liveState, locale, overrides, rawPreferences });
}

async function pageFor(
  browser,
  viewport,
  locale = 'en',
  overrides = {},
  liveState = live,
  dashboardState = dashboard,
  rawPreferences = null
) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(String(error)));
  await init(page, locale, overrides, liveState, dashboardState, rawPreferences);
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
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await shot(page, '05-vi-settings.png');
await page.getByRole('combobox').selectOption('en');
await assertDocumentLocale(page, 'en', 'settings-live-locale-switch-en');
let liveLocalePreferences = await readStoredPreferences(page);
invariant(liveLocalePreferences?.locale === 'en', 'settings-live-locale-switch-en:persisted');
await page.getByRole('combobox').selectOption('vi');
await assertDocumentLocale(page, 'vi', 'settings-live-locale-switch-vi');
liveLocalePreferences = await readStoredPreferences(page);
invariant(liveLocalePreferences?.locale === 'vi', 'settings-live-locale-switch-vi:persisted');
await shot(page, '21-settings-live-locale-switch.png', 'settings-live-locale-switch-vi');
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
  'en',
  {},
  liveEmpty,
  dashboardNoTarget
);
await assertVisible(page, '.workspace-empty', 'no-target');
await shot(page, '14-no-target.png');
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

await fs.writeFile(
  'human-first-ui-v2-render/audit.json',
  JSON.stringify(audit, null, 2) + '\\n',
  'utf8'
);
await browser.close();
console.log(`captured ${audit.screenshots.length} human-first UI V2 screenshots with ${audit.checks.length} executable checks`);
