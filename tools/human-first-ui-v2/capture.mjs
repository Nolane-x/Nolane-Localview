import { chromium } from 'playwright';
import fs from 'node:fs/promises';

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

function init(page, locale = 'en', overrides = {}, liveState = live, dashboardState = dashboard) {
  return page.addInitScript(({ dashboardState, liveState, locale, overrides }) => {
    localStorage.setItem('localview.preferences.v2', JSON.stringify({
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
    }));
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
  }, { dashboardState, liveState, locale, overrides });
}

async function pageFor(
  browser,
  viewport,
  locale = 'en',
  overrides = {},
  liveState = live,
  dashboardState = dashboard
) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
  await init(page, locale, overrides, liveState, dashboardState);
  await page.goto('http://127.0.0.1:1420/', { waitUntil: 'networkidle' });
  await page.waitForTimeout(500);
  return page;
}

await fs.mkdir('human-first-ui-v2-render', { recursive: true });
const browser = await chromium.launch({ headless: true });

let page = await pageFor(browser, { width: 1440, height: 900 });
await page.screenshot({ path: 'human-first-ui-v2-render/01-en-overview.png', fullPage: true });
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/02-en-inspector.png', fullPage: true });
await page.keyboard.press('Escape');
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/03-en-settings.png', fullPage: true });
await page.keyboard.press('Escape');
await page.keyboard.press('m');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/04-en-advanced.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'vi');
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/05-vi-settings.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'zh-CN');
await page.keyboard.press('Control+,');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/06-zh-cn-settings.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', { showTargetBar: false });
await page.screenshot({ path: 'human-first-ui-v2-render/07-target-bar-hidden.png', fullPage: true });
await page.keyboard.press('Control+Shift+T');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/08-target-bar-restored.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 390, height: 844 });
await page.screenshot({ path: 'human-first-ui-v2-render/09-mobile-overview.png', fullPage: true });
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/10-mobile-inspector.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', {}, liveNoFocus);
await page.keyboard.press('i');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/11-en-inspector-no-selection.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 }, 'en', { showToolRail: false });
await page.screenshot({ path: 'human-first-ui-v2-render/12-tool-rail-hidden.png', fullPage: true });
await page.keyboard.press('Control+k');
await page.waitForTimeout(150);
await page.getByRole('button', { name: 'Show tool rail' }).click();
await page.keyboard.press('Escape');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/13-tool-rail-restored.png', fullPage: true });
await page.close();

page = await pageFor(
  browser,
  { width: 1440, height: 900 },
  'en',
  {},
  liveEmpty,
  dashboardNoTarget
);
await page.screenshot({ path: 'human-first-ui-v2-render/14-no-target.png', fullPage: true });
await page.close();

page = await pageFor(browser, { width: 1440, height: 900 });
await page.keyboard.press('a');
await page.waitForTimeout(150);
await page.screenshot({ path: 'human-first-ui-v2-render/15-ai-unavailable.png', fullPage: true });
await page.close();

await browser.close();
console.log('captured 15 human-first UI V2 screenshots');
