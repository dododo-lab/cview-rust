// 지우기·초기화 확인 규칙(P3-4) UI 회귀 검사 — docs/UI_CONFIRM_RULES.md 의 규칙대로 묻는지 본다.
// CR01 규칙 한곳: 두 번 누르기를 화면마다 따로 짜지 않고 confirm.js 하나로 쓴다.
// CR02 두 번 누르기(최근 검색어 모두 지우기): 첫 누름은 안 지움·글자/낭독기 알림, 더블클릭(연타)은 확인이 아님, 두 번째에 지움.
// CR03 두 번 누르기 해제: 3초가 지나면·Esc·단추 밖 누름·초점 이탈이면 풀리고 글자가 돌아온다. 누르고 있는 Enter 반복은 확인이 아님.
// CR04 두 번 누르기(다운로드 끝난 항목 지우기·OAuth 자격 증명 지우기)도 같은 도우미.
// CR05 모달(보관함 › 최근 시청 기록 지우기): 통계 › 데이터 관리와 같은 모달 — 취소·Esc·바깥은 안 지움, 초점 가둠·되돌림,
//      백업 후 지움, 백업 실패면 안 지움, 백업을 끄면 바로 지움.
// CR06 모달(설정 전체 초기화): 취소는 안 바꿈, 내보낸 뒤 초기화, 내보내기 실패면 초기화 안 함, 내보내기를 끄면 바로 초기화.
// CR07 되돌리기 토스트(북마크·멀티라이브 프리셋 한 항목): 바로 숨기고 안 지움, 되돌리면 다시 보임, 시간이 지나면 지움.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/confirm-rule.mjs   (Playwright 가 필요하다. CVIEW_UI_SHOTS=<폴더> 면 스크린샷)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, go, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// mock-app 의 모의 IPC 위에 지우기 명령을 더하고, 부른 명령과 인자를 window.__log 에 남긴다. window.__fail[cmd] 면 그 명령은 실패한다.
function extraMock() {
  const inv = window.__TAURI__.core.invoke;
  const ago = (min) => new Date(Date.now() - min * 60000).toISOString();
  const health = { available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [{ label: '시청 기록', rows: 3 }], backupDir: '/tmp/b', storePath: '/tmp/s' };
  window.__log = [];
  window.__fail = {};
  const more = {
    recent_channels_clear: () => null,
    data_backup_now: () => health,
    data_cleanup: () => health,
    settings_reset: () => inv('settings_get'),
    settings_export: () => '{"player":{}}',
    save_file: ({ fileName }) => `/tmp/dl/${fileName}`,
    oauth_status: () => ({ connected: false, hasCredentials: true, clientIdHint: 'ab12', redirectUri: 'http://127.0.0.1:1/cb' }),
    oauth_clear_credentials: () => null,
    download_list: () => ({ folder: '/tmp/dl', jobs: [], history: [{ id: 'vod-500', title: '받은 다시보기', channelName: '채널', finishedAt: ago(60), completed: true, fileSize: 1e8, filePath: '/tmp/dl/a.mp4', fileStatus: 'ok' }] }),
    download_clear: () => ({ folder: '/tmp/dl', jobs: [], history: [] }),
    preset_list: () => [{ id: 'p1', name: '저녁 구성', kind: 'live', channels: ['ch1', 'ch2'] }, { id: 'p2', name: '주말 구성', kind: 'live', channels: ['ch3'] }],
    preset_delete: () => null,
    bookmark_delete: () => null,
  };
  window.__TAURI__.core.invoke = async (c, a) => {
    if (c !== 'ui_heartbeat') window.__log.push({ cmd: c, args: a || {} });
    if (window.__fail[c]) throw new Error(window.__fail[c]);
    return more[c] ? more[c](a || {}) : inv(c, a);
  };
  try { localStorage.setItem('cview.welcomed.v1', String(Date.now())); localStorage.setItem('cview.recentSearches', JSON.stringify(['마인크래프트', '침착맨'])); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark' });
const page = await ctx.newPage();
page.setDefaultTimeout(8000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(mockTauri, [base, 'dark']);
await page.addInitScript(extraMock);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
// 첫 화면(홈)이 자리 잡은 뒤에 옮겨 다닌다 — 그 전에 옮기면 시작 경로가 다시 홈으로 돌려놓는다
await page.waitForSelector('[data-page="home"]:not([hidden])').catch(() => {});
await page.waitForTimeout(500);

const calls = (cmd) => page.evaluate((c) => window.__log.filter((x) => x.cmd === c), cmd);
const order = (...cmds) => page.evaluate((cs) => window.__log.map((x) => x.cmd).filter((c) => cs.includes(c)), cmds);
const resetLog = () => page.evaluate(() => { window.__log.length = 0; window.__fail = {}; });
const settle = (ms = 150) => page.waitForTimeout(ms);
const GUARD = 500; // confirm.js ARM_GUARD_MS(400) 보다 조금 길게
const shot = async (name) => { if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${name}.png`) }); } };
const recentSaved = () => page.evaluate(() => JSON.parse(localStorage.getItem('cview.recentSearches') || '[]').length);
const live = () => page.evaluate(() => document.getElementById('confirm-live')?.textContent || '');
const armedOf = (sel) => page.$eval(sel, (b) => ({ armed: b.dataset.armed || '', text: b.textContent.trim() }));
const modalOpen = () => page.evaluate(() => Boolean(document.querySelector('.sheet-scrim:not(.leaving) [role="alertdialog"]')));
const SEARCH_CLEAR = '[data-page="search"] [data-recent-clear]';

/** 검색 첫 화면(최근 검색어 2개)을 다시 그린다 — 검색했다가 입력란을 비우면 처음 화면으로 돌아온다. */
async function openSearch() {
  await page.evaluate(() => localStorage.setItem('cview.recentSearches', JSON.stringify(['마인크래프트', '침착맨'])));
  await go(page, 'search');
  const input = '[data-page="search"] input[type=search]';
  await page.fill(input, '침착맨');
  await page.press(input, 'Enter');
  await settle(200);
  await page.fill(input, '');
  await page.waitForSelector(SEARCH_CLEAR);
  assert.equal(await recentSaved(), 2);
}

await check('CR01 one shared helper — no hand-rolled two-press code left', async () => {
  const dir = path.resolve('app-ui/js');
  assert.ok(fs.existsSync(path.join(dir, 'confirm.js')), 'confirm.js 가 없다');
  const offenders = fs.readdirSync(dir).filter((f) => f.endsWith('.js') && f !== 'confirm.js')
    .filter((f) => /dataset\.armed|data-armed/.test(fs.readFileSync(path.join(dir, f), 'utf8')));
  assert.deepEqual(offenders, [], `화면마다 따로 짠 두 번 누르기: ${offenders.join(', ')}`);
  const users = fs.readdirSync(dir).filter((f) => /from '\.\/confirm\.js'/.test(fs.readFileSync(path.join(dir, f), 'utf8')));
  for (const f of ['pages.js', 'downloads.js', 'settings.js', 'library.js', 'stats.js', 'multilive.js']) assert.ok(users.includes(f), `${f} 가 confirm.js 를 쓰지 않는다`);
  assert.ok(fs.existsSync(path.resolve('docs/UI_CONFIRM_RULES.md')), '규칙 문서가 없다');
});

await check('CR02 two-press: first press arms, double-click is not a confirm, second press clears', async () => {
  await openSearch();
  await page.click(SEARCH_CLEAR);
  await settle(60);
  assert.equal(await recentSaved(), 2, '첫 누름에 지웠다');
  const a = await armedOf(SEARCH_CLEAR);
  assert.equal(a.armed, '1');
  assert.equal(a.text, '한 번 더 누르면 모두 지웁니다');
  assert.match(await live(), /최근 검색어.*3초 안에 한 번 더 누르면 모두 지웁니다/);
  assert.equal(await page.$eval('#confirm-live', (x) => x.getAttribute('aria-live')), 'assertive');
  await shot('cr02-armed');
  await page.waitForTimeout(GUARD);
  await page.click(SEARCH_CLEAR);
  await settle();
  assert.equal(await recentSaved(), 0, '두 번째 누름에 지우지 않았다');
  assert.equal(await live(), '', '확인 뒤에도 알림이 남았다');
  // 더블클릭(연타) — 첫 누름이 무장하고 곧바로 온 둘째 누름은 무시한다
  await openSearch();
  await page.dblclick(SEARCH_CLEAR);
  await settle(60);
  assert.equal(await recentSaved(), 2, '더블클릭 한 번에 지워졌다');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '1', '더블클릭 뒤 무장이 풀렸다');
  await page.keyboard.press('Escape');
});

await check('CR03 two-press disarms on timeout, Esc, outside press, focus leaving; held Enter does not confirm', async () => {
  await openSearch();
  // 시간 끝
  await page.click(SEARCH_CLEAR);
  await page.waitForTimeout(3300);
  assert.deepEqual(await armedOf(SEARCH_CLEAR), { armed: '', text: '모두 지우기' }, '3초 뒤 풀리지 않았다');
  assert.equal(await live(), '');
  await page.click(SEARCH_CLEAR); // 풀린 뒤 누름은 다시 무장(지우지 않음)
  await settle(60);
  assert.equal(await recentSaved(), 2, '시간이 지난 뒤 누름에 지웠다');
  // Esc — 초점이 단추에 있을 때
  await page.keyboard.press('Escape');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '', 'Esc 로 풀리지 않았다');
  // Esc — 초점이 단추 밖(WKWebView 는 누른 단추에 초점을 주지 않는다)
  await page.click(SEARCH_CLEAR);
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Escape');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '', '단추 밖 Esc 로 풀리지 않았다');
  // 단추 밖 누름
  await page.click(SEARCH_CLEAR);
  await page.mouse.click(640, 880);
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '', '바깥을 눌러도 풀리지 않았다');
  // 초점 이탈(키보드)
  await page.focus(SEARCH_CLEAR);
  await page.keyboard.press('Enter');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '1', 'Enter 로 무장하지 않았다');
  await page.keyboard.press('Tab');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '', 'Tab 으로 떠나도 풀리지 않았다');
  // 누르고 있는 Enter — 반복 입력은 확인이 아니다
  await page.focus(SEARCH_CLEAR);
  await page.keyboard.down('Enter');
  await page.waitForTimeout(GUARD);
  await page.keyboard.down('Enter'); // repeat
  await page.keyboard.down('Enter'); // repeat
  await page.keyboard.up('Enter');
  await settle(60);
  assert.equal(await recentSaved(), 2, '누르고 있는 Enter 로 지워졌다');
  assert.equal((await armedOf(SEARCH_CLEAR)).armed, '1');
  await page.keyboard.press('Escape');
  assert.equal(await recentSaved(), 2);
});

await check('CR04 downloads "clear finished" and OAuth "clear" use the same two-press', async () => {
  await resetLog();
  await go(page, 'library', { tab: 'downloads' });
  await page.waitForSelector('[data-dl="clear"]');
  await page.click('[data-dl="clear"]');
  await settle();
  assert.equal((await calls('download_clear')).length, 0, '첫 누름에 다운로드 기록을 지웠다');
  assert.equal((await armedOf('[data-dl="clear"]')).text, '한 번 더 누르면 모두 지웁니다');
  assert.match(await live(), /끝난 다운로드 기록/);
  await page.waitForTimeout(GUARD);
  await page.click('[data-dl="clear"]');
  await settle();
  const dc = await calls('download_clear');
  assert.equal(dc.length, 1);
  assert.equal(dc[0].args.ids, null);
  // 한 줄 ✕ 는 확인 없이(파일은 남고 목록에서만 빠진다)
  await go(page, 'settings');
  await page.waitForSelector('[data-oauth="clear"]');
  await page.click('[data-oauth="clear"]');
  await settle();
  assert.equal((await calls('oauth_clear_credentials')).length, 0, '첫 누름에 자격 증명을 지웠다');
  assert.equal((await armedOf('[data-oauth="clear"]')).armed, '1');
  await page.waitForTimeout(GUARD);
  await page.click('[data-oauth="clear"]');
  await settle();
  assert.equal((await calls('oauth_clear_credentials')).length, 1);
});

const LIB_CLEAR = '[data-page="library"] [data-lib="recent-clear"]';
await check('CR05 library "clear recent" asks with the same modal as stats › data, backs up first', async () => {
  await resetLog();
  await go(page, 'library', { tab: 'recent' });
  await page.waitForSelector(LIB_CLEAR);
  assert.equal(await page.getAttribute(LIB_CLEAR, 'aria-haspopup'), 'dialog');
  assert.match(await page.textContent(LIB_CLEAR), /…$/, '모달을 여는 단추 글자 끝에 … 가 없다');
  await page.click(LIB_CLEAR);
  await page.waitForSelector('.cleanup-confirm [role="alertdialog"]');
  await page.waitForTimeout(400);
  await shot('cr05-library-modal');
  assert.equal((await calls('recent_channels_clear')).length, 0, '첫 누름에 지웠다');
  const info = await page.evaluate(() => {
    const d = document.querySelector('.cleanup-confirm [role="alertdialog"]');
    return { modal: d.getAttribute('aria-modal'), title: document.getElementById(d.getAttribute('aria-labelledby'))?.textContent,
      desc: Boolean(document.getElementById(d.getAttribute('aria-describedby'))), cancel: document.activeElement?.matches('[data-cancel]'),
      backup: d.querySelector('input[type="checkbox"]')?.checked };
  });
  assert.deepEqual(info, { modal: 'true', title: '최근 시청 목록을 비울까요?', desc: true, cancel: true, backup: true });
  // 초점 가둠 — Tab·⇧Tab 이 모달 안에서 돈다
  for (let i = 0; i < 4; i += 1) await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('.cleanup-confirm'))), true, 'Tab 이 모달 밖으로 나갔다');
  await page.keyboard.press('Escape');
  await settle(300);
  assert.equal(await modalOpen(), false);
  assert.equal((await calls('recent_channels_clear')).length, 0, 'Esc 뒤 지웠다');
  assert.equal(await page.evaluate((s) => document.activeElement === document.querySelector(s), LIB_CLEAR), true, '닫은 뒤 초점이 단추로 돌아오지 않았다');
  // 취소 단추·바깥 클릭
  await page.click(LIB_CLEAR);
  await page.click('.cleanup-confirm [data-cancel]');
  await settle(300);
  await page.click(LIB_CLEAR);
  await page.waitForSelector('.cleanup-confirm [role="alertdialog"]');
  await page.mouse.click(8, 8);
  await settle(300);
  assert.equal(await modalOpen(), false);
  assert.equal((await calls('recent_channels_clear')).length, 0, '취소·바깥 클릭 뒤 지웠다');
  // 백업 실패면 지우지 않는다
  await page.evaluate(() => { window.__fail.data_backup_now = 'disk full'; });
  await page.click(LIB_CLEAR);
  await page.click('.cleanup-confirm button[type="submit"]');
  await settle(300);
  assert.equal((await calls('recent_channels_clear')).length, 0, '백업 실패인데 지웠다');
  assert.match(await page.evaluate(() => document.getElementById('toast-stack')?.textContent || ''), /백업하지 못해 지우지 않았습니다/);
  await page.evaluate(() => { window.__fail = {}; });
  // 확인 — 백업한 뒤 지운다
  await page.waitForSelector(`${LIB_CLEAR}:not([disabled])`);
  await page.click(LIB_CLEAR);
  await page.click('.cleanup-confirm button[type="submit"]');
  await settle(300);
  assert.deepEqual(await order('data_backup_now', 'recent_channels_clear'), ['data_backup_now', 'data_backup_now', 'recent_channels_clear']);
  // 백업을 끄면 바로 지운다
  await resetLog();
  await page.waitForSelector(LIB_CLEAR);
  await page.click(LIB_CLEAR);
  await page.click('.cleanup-confirm .confirm-check');
  await page.click('.cleanup-confirm button[type="submit"]');
  await settle(300);
  assert.deepEqual(await order('data_backup_now', 'recent_channels_clear'), ['recent_channels_clear']);
  // 같은 데이터(통계 › 데이터 관리의 최근 시청 목록 비우기)는 같은 모달
  await go(page, 'stats', { tab: 'data' });
  await page.waitForSelector('[data-stats="cleanup"][data-kind="recent"]');
  await page.click('[data-stats="cleanup"][data-kind="recent"]');
  await page.waitForSelector('.cleanup-confirm [role="alertdialog"]');
  assert.equal(await page.textContent('.cleanup-confirm h2'), '최근 시청 목록을 비울까요?');
  await page.keyboard.press('Escape');
  await settle(300);
  assert.equal((await calls('data_cleanup')).length, 0);
});

const RESET = '[data-reset-all]';
await check('CR06 settings reset-all asks with a modal and exports first', async () => {
  await resetLog();
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"] [data-group="advanced"]');
  await page.click('[data-page="settings"] [data-group="advanced"]');
  await page.waitForSelector(RESET);
  assert.equal(await page.getAttribute(RESET, 'aria-haspopup'), 'dialog');
  assert.equal((await page.textContent(RESET)).trim(), '초기화…');
  await page.click(RESET);
  await page.waitForSelector('.reset-confirm [role="alertdialog"]');
  await page.waitForTimeout(400);
  await shot('cr06-reset-modal');
  assert.equal((await calls('settings_reset')).length, 0, '첫 누름에 초기화했다');
  const info = await page.evaluate(() => {
    const d = document.querySelector('.reset-confirm [role="alertdialog"]');
    return { title: document.getElementById(d.getAttribute('aria-labelledby'))?.textContent, facts: [...d.querySelectorAll('dt')].map((x) => x.textContent),
      cancel: document.activeElement?.matches('[data-cancel]'), exportOn: d.querySelector('input[type="checkbox"]')?.checked, ok: d.querySelector('button[type="submit"]').textContent };
  });
  assert.deepEqual(info, { title: '모든 설정을 초기화할까요?', facts: ['바뀌는 것', '그대로', '복구'], cancel: true, exportOn: true, ok: '초기화' });
  await page.keyboard.press('Escape');
  await settle(300);
  assert.equal((await calls('settings_reset')).length, 0, 'Esc 뒤 초기화했다');
  // 두 번 눌러도(예전 방식) 모달이 열릴 뿐 초기화하지 않는다
  await page.click(RESET);
  await page.click('.reset-confirm [data-cancel]');
  await settle(300);
  assert.equal((await calls('settings_reset')).length, 0, '취소 뒤 초기화했다');
  // 내보내기 실패면 초기화하지 않는다
  await page.evaluate(() => { window.__fail.save_file = 'no space'; });
  await page.click(RESET);
  await page.click('.reset-confirm button[type="submit"]');
  await settle(300);
  assert.equal((await calls('settings_reset')).length, 0, '내보내기 실패인데 초기화했다');
  await page.evaluate(() => { window.__fail = {}; });
  // 확인 — 내보낸 뒤 초기화
  await page.click(RESET);
  await page.click('.reset-confirm button[type="submit"]');
  await settle(400);
  assert.deepEqual(await order('save_file', 'settings_reset'), ['save_file', 'save_file', 'settings_reset']);
  assert.equal((await calls('settings_reset'))[0].args.domain, 'all');
  // 내보내기를 끄면 바로 초기화
  await resetLog();
  await page.click('[data-page="settings"] [data-group="advanced"]');
  await page.waitForSelector(RESET);
  await page.click(RESET);
  await page.click('.reset-confirm .confirm-check');
  await page.click('.reset-confirm button[type="submit"]');
  await settle(400);
  assert.deepEqual(await order('settings_export', 'save_file', 'settings_reset'), ['settings_reset']);
});

await check('CR07 single-item delete: hide + undo toast, then delete (bookmark, multilive preset)', async () => {
  await resetLog();
  await go(page, 'library', { tab: 'bookmarks' });
  await page.waitForSelector('[data-lib="bookmark-delete"]');
  await page.click('[data-lib="bookmark-delete"]');
  await settle();
  assert.equal(await page.locator('[data-bookmark-id="b1"]:not([hidden])').count(), 0, '바로 숨기지 않았다');
  assert.equal((await calls('bookmark_delete')).length, 0, '되돌리기 시간 전에 지웠다');
  await page.locator('.toast-action', { hasText: '되돌리기' }).last().click();
  await settle();
  assert.equal(await page.locator('[data-bookmark-id="b1"]:not([hidden])').count(), 1, '되돌렸는데 다시 보이지 않는다');
  // 프리셋 — 시트를 연다(멀티라이브 메뉴의 '프리셋 저장·불러오기'와 같은 함수)
  await page.evaluate(async () => {
    const { MultiLivePage } = await import('/js/multilive.js');
    MultiLivePage.prototype.showPresets.call({ state: { sessions: [] }, restoreSessions: async () => {} });
  });
  await page.waitForSelector('.preset-row[data-preset-id="p1"] [data-preset-del]');
  await page.click('.preset-row[data-preset-id="p1"] [data-preset-del]');
  await settle();
  assert.equal(await page.locator('.preset-row[data-preset-id="p1"]:not([hidden])').count(), 0, '프리셋을 바로 숨기지 않았다');
  assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('.sheet'))), true, '지운 뒤 초점이 시트 밖으로 빠졌다');
  assert.equal((await calls('preset_delete')).length, 0, '되돌리기 시간 전에 프리셋을 지웠다');
  await page.locator('.toast-action', { hasText: '되돌리기' }).last().click();
  await settle();
  assert.equal(await page.locator('.preset-row[data-preset-id="p1"]:not([hidden])').count(), 1, '되돌린 프리셋이 다시 보이지 않는다');
  // 이번에는 둘 다 지우고 기다린다
  await page.click('.preset-row[data-preset-id="p1"] [data-preset-del]');
  await page.keyboard.press('Escape');
  await go(page, 'library', { tab: 'bookmarks' });
  await page.waitForSelector('[data-bookmark-id="b1"]:not([hidden]) [data-lib="bookmark-delete"]');
  await page.click('[data-lib="bookmark-delete"]');
  await page.waitForTimeout(5600);
  const bd = await calls('bookmark_delete');
  const pd = await calls('preset_delete');
  assert.deepEqual(bd.map((x) => x.args.id), ['b1']);
  assert.deepEqual(pd.map((x) => x.args.id), ['p1']);
});

await check('CR08 no page errors', async () => {
  assert.deepEqual(errors, []);
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  if (!r.ok) failed += 1;
}
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
