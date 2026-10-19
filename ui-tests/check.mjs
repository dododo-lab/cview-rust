// Browser layout checks use synthetic IPC data. They do not validate live services.
import assert from 'node:assert/strict';
import http from 'node:http';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { dirname, extname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { setup } from './fixtures.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../app-ui');
const output = resolve(here, 'results');
const settings = JSON.parse(await readFile(resolve(here, 'settings.json'), 'utf8'));
const types = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.png': 'image/png' };
const server = http.createServer(async (req, res) => {
  try {
    const path = new URL(req.url, 'http://localhost').pathname;
    if (path.startsWith('/fixture/')) {
      const n = Number(path.match(/\d+/)?.[0] || 0);
      if (path.endsWith('.mp4')) { res.writeHead(404); res.end(); return; }
      const hue = (n * 49 + 180) % 360;
      res.setHeader('Content-Type', 'image/svg+xml');
      res.end(`<svg xmlns="http://www.w3.org/2000/svg" width="800" height="450"><defs><linearGradient id="a" x2="1" y2="1"><stop stop-color="hsl(${hue} 36% 22%)"/><stop offset="1" stop-color="hsl(${hue + 45} 45% 8%)"/></linearGradient></defs><path fill="url(#a)" d="M0 0h800v450H0z"/><circle fill="hsl(${hue + 25} 40% 38%)" opacity=".5" cx="560" cy="180" r="180"/><path d="M0 360L240 110 530 450H0" fill="hsl(${hue} 45% 32%)"/><text x="42" y="400" fill="#fff" font-family="sans-serif" font-size="34">CView Studio · ${n + 1}</text></svg>`);
      return;
    }
    const file = resolve(root, `.${path === '/' ? '/index.html' : path}`);
    if (!file.startsWith(`${root}/`)) throw new Error('Invalid path');
    res.setHeader('Content-Type', types[extname(file)] || 'application/octet-stream');
    res.end(await readFile(file));
  } catch {
    res.writeHead(404); res.end();
  }
});
await new Promise((done) => server.listen(0, '127.0.0.1', done));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({
  headless: true,
  ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH } : {}),
}).catch(async (error) => {
  await new Promise((done) => server.close(done));
  throw error;
});
const reports = [];
const failures = [];
await mkdir(output, { recursive: true });

async function open({ theme = 'dark', width = 1280, height = 900, query = '', empty = false, loggedIn = true, colorScheme = theme, richLibrary = false } = {}) {
  const page = await browser.newPage({ viewport: { width, height }, reducedMotion: 'reduce', colorScheme: colorScheme === 'system' ? 'dark' : colorScheme });
  page.designErrors = [];
  page.on('pageerror', (error) => page.designErrors.push(error.message));
  await page.addInitScript(setup, { settings, theme, empty, loggedIn, richLibrary });
  await page.route('**/*', (route) => route.request().url().startsWith(`${origin}/`) ? route.continue() : route.abort());
  await page.goto(`${origin}/${query}`);
  await page.waitForSelector('.nav-item', { state: 'attached' });
  await page.locator('#splash').waitFor({ state: 'detached' });
  await page.addScriptTag({ path: resolve(here, 'node_modules/axe-core/axe.min.js') });
  return page;
}

async function navigate(page, route, params = {}) {
  await page.evaluate(({ route, params }) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route, params } })), { route, params });
  await page.waitForTimeout(100);
}

async function check(page, name) {
  await page.waitForTimeout(50);
  const violations = await page.evaluate(async () => {
    const result = await axe.run(document, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] },
      // Captions require real media; fixture videos intentionally have no source.
      rules: { 'video-caption': { enabled: false } },
    });
    return result.violations.map((v) => ({ id: v.id, nodes: v.nodes.map((n) => ({ target: n.target, reason: n.failureSummary })) }));
  });
  const geometry = await page.evaluate(() => {
    const issues = [];
    for (const el of document.querySelectorAll('.page:not([hidden])')) {
      if (el.scrollWidth > el.clientWidth + 2) issues.push(`Page overflow: ${el.dataset.page} ${el.scrollWidth}/${el.clientWidth}`);
    }
    for (const el of document.querySelectorAll('.btn > svg')) {
      const r = el.getBoundingClientRect();
      if (r.width > 24 || r.height > 24) issues.push('Unconstrained button icon');
    }
    for (const el of document.querySelectorAll('.page:not([hidden]) .page-title')) {
      const r = el.getBoundingClientRect();
      if (r.height > parseFloat(getComputedStyle(el).lineHeight) * 1.5) issues.push(`Wrapped page title: ${el.textContent}`);
    }
    const account = document.querySelector('.account');
    if (account?.getClientRects().length && account.getBoundingClientRect().bottom > innerHeight + 2) issues.push('Sidebar account clipped');
    for (const pane of document.querySelectorAll('.ml-pane:not([hidden])')) {
      const bounds = pane.getBoundingClientRect();
      for (const button of pane.querySelectorAll('.ml-pane-top button, .ml-pane-bar button')) {
        const r = button.getBoundingClientRect();
        if (r.width && (r.left < bounds.left - 1 || r.right > bounds.right + 1)) issues.push(`Clipped pane control: ${button.title || button.dataset.mp}`);
      }
    }
    return issues;
  });
  const result = { name, violations, geometry, errors: [...page.designErrors] };
  console.log(`${name}: ${violations.length} accessibility issues, ${geometry.length} layout issues, ${page.designErrors.length} script errors`);
  reports.push(result);
  await page.screenshot({ path: resolve(output, `${name}.png`) });
  if (violations.length || geometry.length || page.designErrors.length) failures.push(result);
}

try {
  const suite = process.env.DESIGN_SUITE;
  assert.ok(!suite || ['players', 'extras', 'keyboard'].includes(suite), 'Unknown design suite');
  if (!suite) {
    const routes = ['home', 'following', 'popular', 'categories', 'live', 'multilive', 'multichat', 'library', 'stats', 'settings', 'search'];
    for (const theme of ['dark', 'light']) {
      for (const [width, height] of [[1000, 660], [1280, 900], [1600, 1000]]) {
        const page = await open({ theme, width, height });
        for (const route of routes) {
          await navigate(page, route);
          if (route === 'search') {
            await page.locator('[data-page=search] input[type=search]').fill('방송');
            await page.locator('[data-page=search] form').press('Enter');
          }
          await check(page, `${theme}-${width}-${route}`);
        }
        await page.close();
      }

      const page = await open({ theme });
      for (const group of ['overview', 'watch', 'chat', 'multi', 'general', 'link', 'advanced']) {
        await navigate(page, 'settings');
        await page.locator(`[data-group="${group}"]`).click();
        await check(page, `${theme}-settings-${group}`);
      }
      for (const tab of ['clips', 'vods', 'continue', 'later', 'favorites', 'recent', 'bookmarks', 'downloads']) {
        await navigate(page, 'library', { tab });
        await check(page, `${theme}-library-${tab}`);
      }
      for (const tab of ['overview', 'ranking', 'time', 'history', 'data']) {
        await navigate(page, 'stats', { tab });
        await check(page, `${theme}-stats-${tab}`);
      }
      await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: 'ch-0' } })));
      for (const tab of ['info', 'vod', 'clip']) {
        await page.locator(`[data-page=channel] [data-tab="${tab}"]`).click();
        await check(page, `${theme}-channel-${tab}`);
      }
      await page.locator('[data-page=channel] [data-ch=memo]').click();
      await check(page, `${theme}-memo-dialog`);
      await page.keyboard.press('Escape');
      await navigate(page, 'media', { kind: 'vod', id: 1 });
      await check(page, `${theme}-vod`);
      await navigate(page, 'media', { kind: 'clip', id: 'clip-0' });
      await check(page, `${theme}-clip`);
      await page.close();

      // Native auxiliary windows have different minimum sizes than the main window.
      for (const [kind, width, height] of [['chat', 420, 720], ['stats', 980, 760], ['multichat', 1100, 760], ['player', 960, 600]]) {
        const auxiliary = await open({ theme, width, height, query: `?w=${kind}&channelId=ch-0` });
        await check(auxiliary, `${theme}-window-${kind}`);
        await auxiliary.close();
      }
      const empty = await open({ theme, empty: true, loggedIn: false });
      for (const route of ['home', 'following', 'popular', 'categories', 'live', 'library']) {
        await navigate(empty, route);
        await check(empty, `${theme}-empty-${route}`);
      }
      await empty.close();
    }

  }

  if (!suite || suite === 'players') {
    for (const theme of ['dark', 'light']) {
      for (const [width, height] of [[1000, 660], [1280, 900]]) {
        const player = await open({ theme, width, height });
        await player.evaluate(() => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: 'ch-0' } })));
        await player.locator('#ctl-play').waitFor({ state: 'visible' });
        await player.evaluate(() => {
          const frame = document.querySelector('.player-stage iframe');
          window.__cviewPlayerBridge({ type: 'playStarted', data: {} }, frame.contentWindow);
        });
        await player.locator('#ctl-play').focus();
        assert.equal(await player.locator('.player-controls').evaluate((el) => getComputedStyle(el).opacity), '1');
        await check(player, `${theme}-${width}-active-player`);
        await player.locator('#ctl-quality').click();
        await check(player, `${theme}-${width}-player-quality-menu`);
        await player.close();

        const multi = await open({ theme, width, height });
        await navigate(multi, 'multilive');
        for (const [layout, count] of [['grid', 2], ['grid', 8], ['focus', 4], ['custom', 4]]) {
          await multi.evaluate(({ layout, count }) => {
            const d = window.__design;
            d.emit('multilive://state', {
              sessions: d.lives.slice(0, count).map((live, i) => ({ ...live, id: `session-${i}`, slot: i, profileImageUrl: live.channelImageUrl, isAudible: i === 0, state: { kind: 'playing' } })),
              maxSessions: 8, canAdd: count < 8, layout, focusRatio: .7, focusDirection: 'left', horizontalRatio: .5, verticalRatio: .5, selectedId: 'session-0', multiAudio: false,
            });
          }, { layout, count });
          await multi.locator('.ml-pane:not([hidden]) .ml-pane-close').first().focus();
          await check(multi, `${theme}-${width}-multi-${layout}-${count}`);
        }
        await multi.locator('.ml-pane:not([hidden]) [data-mp=more]').first().click();
        await check(multi, `${theme}-${width}-multi-pane-menu`);
        await multi.close();
      }
    }

  }
  if (!suite || suite === 'extras') {
    for (const theme of ['dark', 'light']) {
      const extras = await open({ theme, width: 1000, height: 660, richLibrary: true });
      for (const tab of ['vods', 'continue', 'later', 'bookmarks', 'downloads']) {
        await navigate(extras, 'library', { tab });
        await check(extras, `${theme}-populated-library-${tab}`);
      }
      await navigate(extras, 'settings');
      await extras.setViewportSize({ width: 1280, height: 900 });
      await extras.waitForTimeout(100);
      await extras.setViewportSize({ width: 1000, height: 660 });
      await extras.waitForTimeout(100);
      assert.equal(await extras.locator('#nav .active').evaluate((el) => {
        const r = el.getBoundingClientRect(), n = el.parentElement.getBoundingClientRect();
        return r.top >= n.top - 1 && r.bottom <= n.bottom + 1;
      }), true, 'Active navigation remains visible after resizing');
      await extras.evaluate(async () => (await import('/js/onboarding.js')).showWelcome({ force: true }));
      for (let step = 0; step < 5; step++) {
        await check(extras, `${theme}-welcome-${step + 1}`);
        await extras.locator('[data-w=next]').click();
      }
      await extras.waitForTimeout(100);
      assert.equal(await extras.locator('[inert]').count(), 0, 'Modal background is restored');
      await extras.close();
    }
  }

  if (!suite || suite === 'keyboard') {
    const keyboard = await open();
    await navigate(keyboard, 'settings');
    await keyboard.locator('[data-group=overview]').focus();
    await keyboard.keyboard.press('ArrowRight');
    await keyboard.waitForTimeout(50);
    assert.equal(await keyboard.locator('[data-group=watch]').getAttribute('aria-selected'), 'true');
    assert.equal(await keyboard.locator('[data-group=watch]').evaluate((el) => el === document.activeElement), true);
    await keyboard.keyboard.press('Home');
    await keyboard.waitForTimeout(50);
    await keyboard.locator('[data-theme-choice=dark]').focus();
    await keyboard.keyboard.press('ArrowRight');
    await keyboard.waitForTimeout(50);
    assert.equal(await keyboard.locator('html').getAttribute('data-theme'), 'light');
    await keyboard.keyboard.press('Control+k');
    await keyboard.locator('.palette input').waitFor({ state: 'visible' });
    await keyboard.keyboard.press('ArrowDown');
    const active = await keyboard.locator('.palette input').getAttribute('aria-activedescendant');
    assert.equal(await keyboard.locator(`#${active}`).getAttribute('aria-selected'), 'true');
    await keyboard.keyboard.press('Tab');
    assert.equal(await keyboard.locator('.palette input').evaluate((el) => el === document.activeElement), true);
    await check(keyboard, 'keyboard-command-palette');
    await keyboard.keyboard.press('Escape');
    await keyboard.waitForTimeout(60);
    assert.equal(await keyboard.locator('[data-theme-choice=light]').evaluate((el) => el === document.activeElement), true);
    await navigate(keyboard, 'popular');
    await keyboard.locator('[data-page=popular] .card').first().focus();
    await keyboard.keyboard.press('Enter');
    await keyboard.waitForTimeout(100);
    assert.equal(await keyboard.evaluate(() => window.__design.calls.filter((x) => x === 'player_start').length), 1);
    await navigate(keyboard, 'popular');
    await keyboard.locator('[data-page=popular] .card').first().focus();
    await keyboard.keyboard.press('Space');
    await keyboard.waitForTimeout(100);
    assert.equal(await keyboard.evaluate(() => window.__design.calls.filter((x) => x === 'player_start').length), 2);
    await navigate(keyboard, 'popular');
    await keyboard.locator('[data-page=popular] .card-avatar').first().focus();
    await keyboard.keyboard.press('Enter');
    await keyboard.waitForTimeout(100);
    assert.equal(await keyboard.locator('[data-page=channel]').isVisible(), true);
    assert.equal(await keyboard.evaluate(() => window.__design.calls.filter((x) => x === 'player_start').length), 2);
    await keyboard.close();

    for (const scheme of ['dark', 'light']) {
      const system = await open({ theme: 'system', colorScheme: scheme });
      const resolved = await system.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--text-tertiary').trim());
      assert.equal(resolved, scheme === 'dark' ? '#a0a5af' : '#606773');
      await check(system, `system-${scheme}`);
      await system.close();
    }
  }
  assert.equal(failures.length, 0, JSON.stringify(failures, null, 2));
  console.log(`Design checks passed: ${reports.length} screen states${suite ? "" : "; keyboard tabs, dialogs, palette and single card activation verified"}.`);
} finally {
  await writeFile(resolve(output, process.env.DESIGN_SUITE ? `${process.env.DESIGN_SUITE}-report.json` : 'report.json'), JSON.stringify({ generatedAt: new Date().toISOString(), screens: reports.length, failures, reports }, null, 2));
  await browser.close();
  await new Promise((done) => server.close(done));
}
