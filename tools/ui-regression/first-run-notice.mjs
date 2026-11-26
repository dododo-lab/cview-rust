// 처음 실행 고지·도움말 프로젝트 항목 회귀 검사(notice.js, app.js MENU_ACTIONS, app-sheets.js 정보 시트, settings-about.js).
// app-ui 를 그대로 띄우고 모의 IPC(mock-app.mjs)에 고지 동의 기록(general.noticeAcceptedVersion·notice_accept)과 app_quit 을 더한다.
// 동의 기록은 localStorage 에 흉내 내 다시 열어도 남는다. 실제 설정·서비스는 건드리지 않는다.
// 저장소 루트에서: NODE_PATH=<playwright 설치 위치> node tools/ui-regression/first-run-notice.mjs [chromium|webkit]
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.argv[2] || process.env.CVIEW_UI_BROWSER || 'chromium';
const browser = await pw[engine].launch();
const NOTICE_SRC = fs.readFileSync(path.resolve('app-ui/js/notice.js'), 'utf8');
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const WIN_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Edg/131.0';

// 모의 IPC 위에 — 고지 동의 기록(다시 열어도 남게 localStorage)·종료·실패 주입. welcomed=false 면 환영 안내도 띄운다.
function noticeMock({ welcomed }) {
  const KEY = '__mock.notice';
  const state = () => { try { return JSON.parse(localStorage.getItem(KEY)) || { v: 0, at: null }; } catch { return { v: 0, at: null }; } };
  if (!welcomed) { try { localStorage.removeItem('cview.welcomed.v1'); } catch { /* 무시 */ } }
  window.__quit = 0;
  window.__fail = {};
  const inner = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (cmd === 'settings_get') {
      const r = await inner(cmd, args);
      const st = state();
      return { ...r, general: { ...(r.general || {}), noticeAcceptedVersion: st.v, noticeAcceptedAt: st.at } };
    }
    if (cmd === 'notice_accept') {
      window.__calls.push(cmd);
      (window.__noticeArgs ||= []).push(args);
      if (window.__fail.accept) throw new Error('저장 실패');
      localStorage.setItem(KEY, JSON.stringify({ v: args.version, at: Date.UTC(2026, 10, 26) }));
      return window.__TAURI__.core.invoke('settings_get');
    }
    if (cmd === 'app_quit') {
      window.__calls.push(cmd);
      if (window.__fail.quit) throw new Error('종료 실패');
      window.__quit += 1;
      return null;
    }
    if (cmd === 'open_external') { window.__calls.push(cmd); (window.__opened ||= []).push(args.url); return null; }
    return inner(cmd, args);
  };
}

/** 앱을 연다. notice — 저장된 동의 버전(없으면 0). bump — 화면의 NOTICE_VERSION 을 이 값으로 바꿔 서빙(문구 개정 흉내). */
async function open({ ua = MAC_UA, theme = 'dark', notice = 0, welcomed = true, bump = null, ctx = null, url = '', events = false, waitPage = true } = {}) {
  ctx ||= await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: theme, userAgent: ua });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  if (bump) {
    await page.route('**/js/notice.js', (r) => r.fulfill({ contentType: 'text/javascript', body: NOTICE_SRC.replace(/export const NOTICE_VERSION = \d+;/, `export const NOTICE_VERSION = ${bump};`) }));
  }
  await page.addInitScript(mockTauri, [base, theme, 12]);
  if (notice) await page.addInitScript((v) => { try { localStorage.setItem('__mock.notice', JSON.stringify({ v, at: Date.UTC(2026, 10, 1) })); } catch { /* 무시 */ } }, notice);
  await page.addInitScript(noticeMock, { welcomed });
  // 이벤트 흉내 — 검사가 window.__ev[이름] 처리기를 직접 부른다(트레이·메뉴 막대 동작 등).
  if (events) await page.addInitScript(() => { window.__ev = {}; window.__TAURI__.event.listen = async (n, cb) => { (window.__ev[n] ||= []).push(cb); return () => {}; }; });
  await page.goto(`${base}${url}`);
  if (waitPage) await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}

const NOTICE = '.sheet-scrim.notice-scrim';
const shown = (page) => page.evaluate((s) => Boolean(document.querySelector(s)), NOTICE);
const waitNotice = (page) => page.waitForSelector(`${NOTICE} .sheet.notice`);
const calls = (page, cmd) => page.evaluate((c) => window.__calls.filter((x) => x === c).length, cmd);
const menu = (page, id) => page.evaluate((x) => window.dispatchEvent(new CustomEvent('cview:menu', { detail: x })), id);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failed = 0;
async function check(name, fn) {
  try { await fn(); console.log(`PASS [${engine}] ${name}`); } catch (e) { failed++; console.log(`FAIL [${engine}] ${name}: ${e.message}`); }
}

await check('N01 처음 실행 — 고지가 뜨고 macOS 는 "이 앱은" 으로 시작, 대화 상자 이름·설명·초점·뒤 화면 inert', async () => {
  const { ctx, page, errors } = await open();
  await waitNotice(page);
  const info = await page.evaluate((s) => {
    const sheet = document.querySelector(`${s} .sheet`);
    return {
      role: sheet.getAttribute('role'), modal: sheet.getAttribute('aria-modal'),
      label: document.getElementById(sheet.getAttribute('aria-labelledby'))?.textContent,
      desc: document.getElementById(sheet.getAttribute('aria-describedby'))?.textContent,
      lead: sheet.querySelector('[data-notice-lead]').textContent,
      focusIn: sheet.contains(document.activeElement),
      inert: [...document.body.children].filter((e) => !e.matches(s)).every((e) => e.inert),
      welcome: Boolean(document.querySelector('.welcome-scrim')),
      text: sheet.innerText,
    };
  }, NOTICE);
  assert.equal(info.role, 'dialog');
  assert.equal(info.modal, 'true');
  assert.equal(info.label, 'CView 이용 안내');
  assert.ok(info.lead.startsWith('이 앱은 '), info.lead);
  assert.equal(info.desc, info.lead, '설명은 첫 문장');
  assert.ok(info.focusIn, '초점이 고지 안');
  assert.ok(info.inert, '뒤 화면은 inert');
  assert.equal(info.welcome, false, '환영 안내는 고지 뒤에');
  for (const w of ['비영리', '비공식', 'NAVER', '치지직', '오픈소스', 'GPL-3.0-only', '보증', '상표', 'do@dododo.app', 'github.com/dododo-lab/cview-rust']) assert.ok(info.text.includes(w), w);
  assert.doesNotMatch(info.text, /이 프로그램은/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('N02 Esc·바깥 누름·앱 단축키로는 닫히지 않고, Tab 은 고지 안을 돈다', async () => {
  const { ctx, page } = await open();
  await waitNotice(page);
  await page.keyboard.press('Escape');
  await page.mouse.click(10, 10);
  await page.keyboard.press(engine === 'webkit' ? 'Meta+k' : 'Control+k');
  await sleep(250);
  assert.ok(await shown(page), 'Esc·바깥 누름 뒤에도 떠 있다');
  assert.equal(await page.evaluate(() => Boolean(document.querySelector('.palette-scrim:not([hidden])')?.isConnected && getComputedStyle(document.querySelector('.palette-scrim')).display !== 'none')), false, '명령 팔레트가 열리지 않는다');
  assert.equal(await calls(page, 'notice_accept') + await calls(page, 'app_quit'), 0);
  const seen = new Set();
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press('Tab');
    const at = await page.evaluate((s) => { const a = document.activeElement; return { inside: Boolean(a.closest(s)), id: a.dataset.notice || a.dataset.noticeLink || a.className }; }, NOTICE);
    assert.ok(at.inside, `Tab ${i + 1} 번째도 고지 안`);
    seen.add(at.id);
  }
  for (const id of ['contact', 'repo', 'decline', 'accept']) assert.ok(seen.has(id), `Tab 이 ${id} 를 지난다`);
  // 메뉴 막대 동작도 고지 뒤에서 돌지 않는다.
  await menu(page, 'settings');
  await sleep(150);
  assert.equal(await page.evaluate(() => document.querySelector('[data-page="settings"]')?.classList.contains('active') || false), false, '설정으로 가지 않는다');
  await ctx.close();
});

await check('N03 동의 — 버전과 함께 기록하고 닫히며, 이어서 환영 안내가 뜬다', async () => {
  const { ctx, page, errors } = await open({ welcomed: false });
  await waitNotice(page);
  assert.equal(await page.evaluate(() => Boolean(document.querySelector('.welcome-scrim'))), false);
  await page.click(`${NOTICE} [data-notice="accept"]`);
  await page.waitForSelector(NOTICE, { state: 'detached' });
  assert.deepEqual(await page.evaluate(() => window.__noticeArgs), [{ version: 1 }]);
  await page.waitForSelector('.welcome-scrim .sheet.welcome');
  assert.match(await page.textContent('.welcome-scrim'), /오픈소스/, '환영 안내 첫 단계도 오픈소스를 밝힌다');
  assert.equal(await page.evaluate(() => [...document.body.children].some((e) => e.inert)), false, '동의 뒤 inert 해제');
  assert.equal(await calls(page, 'app_quit'), 0);
  assert.deepEqual(errors, []);
  // 다시 열면(같은 저장소) 묻지 않는다.
  const again = await open({ ctx });
  await sleep(1200);
  assert.equal(await shown(again.page), false, '동의한 뒤에는 다시 뜨지 않는다');
  assert.equal(await calls(again.page, 'notice_accept'), 0);
  await ctx.close();
});

await check('N04 문구 버전이 오르면(저장 1 < 화면 2) 다시 묻고, 2 로 기록한다', async () => {
  const { ctx, page } = await open({ notice: 1, bump: 2 });
  await waitNotice(page);
  await page.click(`${NOTICE} [data-notice="accept"]`);
  await page.waitForSelector(NOTICE, { state: 'detached' });
  assert.deepEqual(await page.evaluate(() => window.__noticeArgs), [{ version: 2 }]);
  await ctx.close();
  // 같은 버전이면 묻지 않는다.
  const same = await open({ notice: 2, bump: 2 });
  await sleep(1200);
  assert.equal(await shown(same.page), false);
  await same.ctx.close();
});

await check('N05 동의하지 않음 — 앱을 끝낸다(종료 실패면 고지를 남기고 알린다)', async () => {
  const { ctx, page } = await open();
  await waitNotice(page);
  await page.evaluate(() => { window.__fail.quit = true; });
  await page.click(`${NOTICE} [data-notice="decline"]`);
  await page.waitForFunction(() => /종료하지 못했습니다/.test(document.body.innerText));
  assert.ok(await shown(page), '종료하지 못하면 고지가 남는다');
  await page.evaluate(() => { window.__fail.quit = false; });
  await page.click(`${NOTICE} [data-notice="decline"]`);
  await page.waitForFunction(() => window.__quit === 1);
  assert.equal(await calls(page, 'notice_accept'), 0, '동의를 기록하지 않는다');
  await ctx.close();
});

await check('N06 Windows — "이 프로그램은" 으로 시작하고 종료 안내도 "프로그램"', async () => {
  const { ctx, page } = await open({ ua: WIN_UA });
  await waitNotice(page);
  assert.equal(await page.evaluate(() => document.documentElement.dataset.platform), 'windows');
  const lead = await page.textContent(`${NOTICE} [data-notice-lead]`);
  assert.ok(lead.startsWith('이 프로그램은 '), lead);
  assert.match(await page.textContent(`${NOTICE} .notice-ask`), /프로그램을 종료합니다/);
  assert.doesNotMatch(await page.textContent(NOTICE), /이 앱은/);
  await page.click(`${NOTICE} [data-notice="decline"]`);
  await page.waitForFunction(() => window.__quit === 1);
  await ctx.close();
});

await check('N07 저장 실패 — 이번 실행은 이어 가고 다음 실행에 다시 묻는다고 알린다', async () => {
  const { ctx, page } = await open();
  await waitNotice(page);
  await page.evaluate(() => { window.__fail.accept = true; });
  await page.click(`${NOTICE} [data-notice="accept"]`);
  await page.waitForSelector(NOTICE, { state: 'detached' });
  await page.waitForFunction(() => /다음 실행 때 다시 묻습니다/.test(document.body.innerText));
  await ctx.close();
});

await check('N08 도움말 메뉴 — 이용 고지 다시 보기(동의한 날·Esc 로 닫힘)·라이선스·문의 메일·저장소', async () => {
  const { ctx, page, errors } = await open({ notice: 1 });
  await sleep(600);
  assert.equal(await shown(page), false);
  await menu(page, 'notice');
  await page.waitForSelector('.sheet-scrim.app-sheet .notice-review');
  const t = await page.innerText('.sheet-scrim.app-sheet');
  assert.ok(t.includes('이 앱은 '), t);
  assert.match(t, /2026년 11월 1일에 동의했습니다/);
  assert.equal(await page.evaluate(() => document.activeElement.closest('.sheet-scrim.app-sheet') !== null), true, '초점이 시트 안');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });

  await menu(page, 'licenses');
  await page.waitForSelector('.sheet-scrim.app-sheet .licenses-sheet');
  const lt = await page.innerText('.sheet-scrim.app-sheet');
  for (const w of ['GNU GPL v3.0 (GPL-3.0-only)', 'hls.js', 'Apache-2.0', 'Tauri']) assert.ok(lt.includes(w), w);
  await page.click('.sheet-scrim.app-sheet [data-notice-link="license"]');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });

  await menu(page, 'contact');
  await menu(page, 'github-repo');
  await page.waitForFunction(() => (window.__opened || []).length >= 3);
  assert.deepEqual(await page.evaluate(() => window.__opened), [
    'https://github.com/dododo-lab/cview-rust/blob/main/LICENSE',
    'mailto:do@dododo.app',
    'https://github.com/dododo-lab/cview-rust',
  ]);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('N09 정보 시트·설정 › 정보 — 오픈소스·라이선스·문의·저장소를 밝힌다', async () => {
  const { ctx, page } = await open({ notice: 1 });
  await menu(page, 'about');
  await page.waitForSelector('.sheet-scrim.app-sheet .about-sheet-links');
  const t = await page.innerText('.sheet-scrim.app-sheet');
  for (const w of ['오픈소스 프로젝트', '비공식', 'GPL-3.0-only', '문의하기 (do@dododo.app)', 'GitHub 저장소', '오픈소스 라이선스', '이용 고지']) assert.ok(t.includes(w), w);
  await page.click('.sheet-scrim.app-sheet [data-about-menu="licenses"]');
  await page.waitForSelector('.sheet-scrim.app-sheet .licenses-sheet');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });

  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: {} } })));
  const S = '[data-page="settings"]';
  await page.waitForSelector(`${S} .settings-tabs`);
  await page.waitForTimeout(300);
  await page.click(`${S} .settings-tabs [data-group="about"]`);
  await page.waitForSelector(`${S} .settings-groups.about-tab [data-about="licenses"]`);
  const st = await page.innerText(`${S} .settings-groups`);
  for (const w of ['오픈소스 라이선스', '이용 고지', '문의하기', 'do@dododo.app', '오픈소스(GPL-3.0)']) assert.ok(st.includes(w), w);
  await page.click(`${S} [data-about="contact"]`);
  await page.waitForFunction(() => (window.__opened || []).includes('mailto:do@dododo.app'));
  await page.click(`${S} [data-about="notice"]`);
  await page.waitForSelector('.sheet-scrim.app-sheet .notice-review');
  await ctx.close();
});

// 글자 대비 — 고지 본문 글자와 그 칸 바탕(불투명)의 대비가 4.5 이상(라이트·다크).
for (const theme of ['dark', 'light']) {
  await check(`N10 ${theme} 테마 — 고지 글자 대비 4.5 이상`, async () => {
    const { ctx, page } = await open({ theme });
    await waitNotice(page);
    const ratios = await page.evaluate((s) => {
      const rgb = (c) => (c.match(/[\d.]+/g) || []).map(Number);
      const lum = ([r, g, b]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
      const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
      const box = document.querySelector(`${s} .notice-list > div`);
      const bg = rgb(getComputedStyle(box).backgroundColor);
      return ['dt', 'dd'].map((t) => ratio(rgb(getComputedStyle(box.querySelector(t)).color), bg));
    }, NOTICE);
    for (const r of ratios) assert.ok(r >= 4.5, `대비 ${r.toFixed(2)}`);
    await ctx.close();
  });
}

await check('N11 이 기능 이전 백엔드(noticeAcceptedVersion 없음)에서는 묻지 않는다', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, userAgent: MAC_UA });
  const page = await ctx.newPage();
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await sleep(1200);
  assert.equal(await shown(page), false);
  await ctx.close();
});

// ── 9차 — 고지와 다른 시작 흐름 ──
const fire = (page, name, payload) => page.evaluate(([n, p]) => (window.__ev[n] || []).forEach((cb) => cb({ payload: p })), [name, payload]);

await check('N12 고지가 떠 있으면 메뉴 막대·트레이의 화면 이동·멀티라이브 추가·업데이트 확인을 하지 않는다', async () => {
  const { ctx, page } = await open({ events: true });
  await waitNotice(page);
  const route0 = await page.evaluate(() => [...document.querySelectorAll('[data-page]')].find((p) => !p.hidden)?.dataset.page);
  await fire(page, 'tray://control', { kind: 'queue-multi', channelIds: ['ch1', 'ch2'] });
  await fire(page, 'tray://control', { kind: 'navigate', route: 'search' });
  await fire(page, 'tray://control', { kind: 'check-update' });
  await fire(page, 'tray://control', { kind: 'show-now-playing' });
  await sleep(400);
  const after = await page.evaluate(() => ({
    route: [...document.querySelectorAll('[data-page]')].find((p) => !p.hidden)?.dataset.page,
    sheets: document.querySelectorAll('.sheet-scrim:not(.notice-scrim)').length,
    multiAdd: window.__calls.filter((c) => c === 'multi_live_add').length,
  }));
  assert.equal(after.route, route0, '화면이 바뀌지 않는다');
  assert.equal(after.sheets, 0, '업데이트 시트가 고지 위에 뜨지 않는다');
  assert.equal(after.multiAdd, 0, '멀티라이브에 넣지 않는다');
  assert.ok(await shown(page));
  // 동의한 뒤에는 그대로 듣는다
  await page.click(`${NOTICE} [data-notice="accept"]`);
  await page.waitForFunction((s) => !document.querySelector(s), NOTICE);
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await fire(page, 'tray://control', { kind: 'navigate', route: 'search' });
  await page.waitForFunction(() => !document.querySelector('[data-page="search"]').hidden);
  await ctx.close();
});

await check('N13 동의하기 전에는 보조 창(채팅 창 등)이 열리지 않고 스스로 닫힌다', async () => {
  const { ctx, page } = await open({ url: '?w=chat&channelId=ch1', waitPage: false });
  await page.waitForFunction(() => window.__calls.includes('window_close_self'), null, { timeout: 5000 }).catch(() => {});
  const r = await page.evaluate(() => ({ closed: window.__calls.includes('window_close_self'), chat: window.__calls.filter((c) => c.startsWith('chat_connect')).length }));
  assert.ok(r.closed, 'window_close_self 를 부른다');
  assert.equal(r.chat, 0, '채팅을 연결하지 않는다');
  await ctx.close();
  // 동의한 뒤에는 그대로 연다
  const b = await open({ url: '?w=chat&channelId=ch1', notice: 1, waitPage: false });
  await sleep(1200);
  assert.equal(await b.page.evaluate(() => window.__calls.includes('window_close_self')), false);
  await b.ctx.close();
});

await check('N14 Windows — 고지가 떠 있어도 창 단추(최소화·최대화·닫기)는 누를 수 있다', async () => {
  const { ctx, page } = await open({ ua: WIN_UA });
  await waitNotice(page);
  const r = await page.evaluate(() => {
    const bar = document.querySelector('.win-controls');
    return { has: Boolean(bar), inert: bar ? bar.inert : null, others: [...document.body.children].filter((e) => !e.matches('.win-controls, .notice-scrim')).every((e) => e.inert) };
  });
  assert.ok(r.has, '창 단추가 있다');
  assert.equal(r.inert, false, '창 단추는 inert 가 아니다');
  assert.ok(r.others, '나머지 화면은 inert');
  await ctx.close();
});

await check('N15 고지를 물어야 하면 홈 데이터를 기다리지 않고 고지를 띄운다(시작 화면이 먼저 걷혀 홈을 누를 틈이 없다)', async () => {
  const src = fs.readFileSync(path.resolve('app-ui/js/app.js'), 'utf8');
  assert.match(src, /const askNotice = needsNotice\(settings && settings\.general\);\s*\n\s*if \(!askNotice\) await loadHomeBehindSplash\(\);/);
  assert.match(src, /if \(askNotice\) await showNotice\(\);/);
});

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
