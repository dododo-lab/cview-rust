// 설정 › 정보 탭 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 업데이트 서버·클립보드·Finder·브라우저는 건드리지 않는다).
// SA01 탭·머리말 — '정보'가 맨 끝 탭(End 키로도 간다), 아이콘(장식)·이름·'버전 X.Y.Z (빌드 N)'·한 줄 소개.
// SA02 업데이트 줄 — 'CView 0.1.103' + [업데이트 확인]. 결과(최신·새 버전·실패)를 상태 글(role=status)로, 새 버전이면 설치 단추.
//      개요 탭에는 업데이트 줄이 더 없다(한 곳에만).
// SA03 '자동으로 업데이트 확인' 토글 — general.autoCheckUpdates 를 읽고 쓴다. 일반 탭에서 옮겨 와 설정 전체에 한 줄뿐.
// SA04 '0.1.103의 새로운 기능' [보기…] — 릴리스 노트 시트(제목·게시일·노트·GitHub에서 보기), 실패면 다시 시도, 노트 없음 안내.
//      Esc 로 닫으면 초점이 [보기…]로 돌아온다.
// SA05 피드백·링크 — [작성…]은 저장소 이슈 양식, 아래 띠 링크(웹사이트·변경 내역·소스 코드)는 기본 브라우저로(웹뷰는 그대로).
//      지어낸 메일 주소·제작자 이름·개인정보 처리방침 링크가 없다.
// SA06 진단 보고서 — 설명에 빼는 개인 정보(쿠키·토큰·계정·채팅·팔로잉)를 적고, [보고서 복사]는 백엔드 글을 그대로 클립보드에.
//      ClipboardItem(Promise) → writeText 순으로 시도, 둘 다 안 되면 글을 고른 채 시트로. 누른 단추는 초점을 잃지 않는다.
// SA07 사용 데이터 — macOS 'Finder에서 보기' · Windows '탐색기에서 보기', 앱 데이터 폴더(dataDir)를 연다.
// SA08 검색 — 정보 탭 줄도 찾고, 검색 중엔 머리말·링크 띠를 그리지 않는다.
// SA09 axe(WCAG 2.1 A·AA + best-practice, 글자 대비 포함) — 정보 탭 라이트·다크와 두 시트.
// SA10 키보드 — Tab 이 정보 탭의 단추·토글·링크를 차례로 지나고, 초점마다 보이는 표시(외곽선·고리)가 있다.
// SA11 배치 — 1000×660(최소 창)·1280×900 × 라이트·다크: 가로 넘침 없음, 단추가 카드 밖으로 안 나감, 머리말 가운데.
// 저장소 루트에서: node tools/ui-regression/settings-about.mjs   (Playwright·axe-core 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 화면 PNG 를 남긴다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const browser = await engine.launch();
const results = [];
const ONLY = (process.env.CVIEW_SA_ONLY || '').split(',').filter(Boolean); // 예: CVIEW_SA_ONLY=SA04,SA06
async function check(id, fn) {
  if (ONLY.length && !ONLY.some((o) => id.startsWith(o))) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 8).join('\n') }); }
}

const DATA_DIR = '/Users/someone/Library/Application Support/app.cview.rust';
const REPORT = 'CView 진단 보고서\n만든 시각: 2026-11-14 20:00:00 +09:00\n\n[앱]\n버전: 0.1.103 (빌드 103)\n운영체제: macOS 26.3 (aarch64)\n';
const NOTES_PAGE = 'https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.103';

/** 정보 탭이 부르는 명령만 흉내 낸다(나머지는 mock-app 그대로). 부른 명령·인자는 window.__about.calls. 클립보드도 가짜로. */
function installAboutMocks([report, dataDir, notesPage]) {
  // 환영 안내를 띄우지 않는다 — WebKit 에서는 열린 뒤에 늦게 떠서 지워도 다시 나타나 누르기를 가로챘다(키는 onboarding.js).
  try { localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ }
  const cfg = window.__about = { update: 'latest', notes: 'ok', report: 'ok', clip: 'ok', general: { autoCheckUpdates: true }, calls: [], clipped: [] };
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a = {}) => {
    cfg.calls.push([c, a]);
    if (c === 'app_info') return { name: 'CView', version: '0.1.103', build: '103', os: 'macos', arch: 'aarch64', dataDir };
    if (c === 'update_check') {
      if (cfg.update === 'fail') throw new Error('업데이트 서버에 연결하지 못했습니다');
      if (cfg.update === 'newer') return { current: '0.1.103', latest: '0.1.104', newer: true, url: 'https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.104', downloadUrl: 'https://github.com/x/CView_0.1.104_arm64.dmg', downloadName: 'CView_0.1.104_arm64.dmg', autoInstall: true, signed: true };
      return { current: '0.1.103', latest: '0.1.103', newer: false, url: notesPage, autoInstall: false, signed: false };
    }
    if (c === 'release_notes') {
      if (cfg.notes === 'fail') throw new Error('업데이트 서버에 연결하지 못했습니다');
      if (cfg.notes === 'none') return { version: '0.1.103', notes: null, publishedAt: null, pageUrl: 'https://github.com/dododo-lab/cview-rust/releases' };
      return { version: '0.1.103', notes: 'CView 0.1.103 (빌드 103)\n\n- 홈 맨 위 "방송 중인 팔로잉"을 새로 바꿨습니다.', publishedAt: '2026-11-14T09:00:10Z', pageUrl: notesPage };
    }
    if (c === 'diagnostic_report') { if (cfg.report === 'fail') throw new Error('보고서 실패'); return report; }
    if (c === 'open_external' || c === 'reveal_path') return null;
    if (c === 'settings_get') { const s = await inv(c, a); return { ...s, general: { ...cfg.general } }; }
    if (c === 'settings_update') {
      if (a.domain === 'general') cfg.general = { ...cfg.general, ...a.patch };
      const s = await inv(c, a);
      return { ...s, general: { ...cfg.general } };
    }
    return inv(c, a);
  };
  const clip = {
    async write(items) {
      if (cfg.clip !== 'ok') throw new Error('NotAllowedError');
      const blob = await items[0].getType('text/plain');
      cfg.clipped.push(['write', await blob.text()]);
    },
    async writeText(t) {
      if (cfg.clip === 'none') throw new Error('NotAllowedError');
      cfg.clipped.push(['writeText', t]);
    },
  };
  Object.defineProperty(Navigator.prototype, 'clipboard', { get: () => clip, configurable: true });
}

const WIN_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0';

/** 모의 IPC 로 앱을 열고 설정 › 정보 탭으로. */
async function open({ theme = 'light', width = 1280, height = 900, windows = false, tab = true } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, ...(windows ? { userAgent: WIN_UA } : {}) });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(installAboutMocks, [REPORT, DATA_DIR, NOTES_PAGE]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"] .settings-tabs');
  await page.waitForTimeout(300); // 화면 등장 전환(page-in)이 끝난 뒤
  if (tab) await aboutTab(page);
  return { ctx, page, errors };
}
async function aboutTab(page) {
  await page.click('[data-page="settings"] .settings-tabs [data-group="about"]');
  await page.waitForSelector('[data-page="settings"] .settings-groups.about-tab .about-hero');
  await page.waitForFunction(() => /빌드/.test(document.querySelector('.about-version')?.textContent || ''));
}
const S = '[data-page="settings"]';
const calls = (page, cmd) => page.evaluate((c) => window.__about.calls.filter(([x]) => x === c).map(([, a]) => a), cmd);
const setMock = (page, patch) => page.evaluate((p) => Object.assign(window.__about, p), patch);
const sheetOpen = (page) => page.waitForSelector('.sheet-scrim.app-sheet .sheet', { state: 'visible' });
const settleSheet = (page) => page.waitForTimeout(450); // 시트 등장 전환이 끝난 뒤(대비·위치를 잴 때)

await check('SA01 정보 탭이 맨 끝이고 머리말에 아이콘·이름·버전(빌드)·소개가 있다', async () => {
  const { ctx, page, errors } = await open({ tab: false });
  const tabs = await page.$$eval(`${S} .settings-tabs [role="tab"]`, (ts) => ts.map((t) => [t.dataset.group, t.textContent.trim()]));
  assert.deepEqual(tabs.at(-1), ['about', '정보'], `탭 ${JSON.stringify(tabs)}`);
  await page.focus(`${S} .settings-tabs [aria-selected="true"]`);
  await page.keyboard.press('End');
  await page.waitForSelector(`${S} .settings-groups.about-tab .about-hero`);
  const st = await page.evaluate(() => ({
    sel: document.querySelector('.settings-tabs [aria-selected="true"]')?.dataset.group,
    focus: document.activeElement?.dataset.group,
  }));
  assert.deepEqual(st, { sel: 'about', focus: 'about' }, 'End 로 정보 탭 선택·초점');
  await page.waitForFunction(() => /빌드/.test(document.querySelector('.about-version')?.textContent || ''));
  const hero = await page.evaluate(() => {
    const h = document.querySelector('.about-hero');
    const imgs = [...h.querySelectorAll('img')];
    return {
      name: h.querySelector('.about-name').textContent.trim(), tag: h.querySelector('.about-name').tagName,
      version: h.querySelector('.about-version').textContent.trim(), tagline: h.querySelector('.about-tagline').textContent.trim(),
      iconHidden: h.querySelector('.about-icon').getAttribute('aria-hidden'), alts: imgs.map((i) => i.getAttribute('alt')),
      shown: imgs.filter((i) => i.offsetWidth > 0 && i.complete && i.naturalWidth > 0).length,
    };
  });
  assert.equal(hero.name, 'CView');
  assert.equal(hero.tag, 'H2');
  assert.equal(hero.version, '버전 0.1.103 (빌드 103)');
  assert.ok(hero.tagline.length > 4, '한 줄 소개');
  assert.equal(hero.iconHidden, 'true', '아이콘은 장식');
  assert.deepEqual(hero.alts, ['', '']);
  assert.equal(hero.shown, 1, '테마에 맞는 로고 하나만 보인다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('SA02 업데이트 줄 — 앱 이름·버전, 확인 결과(최신·새 버전·실패), 개요 탭엔 더 없음', async () => {
  const { ctx, page, errors } = await open();
  const row = `${S} .update-row`;
  await page.waitForFunction((r) => document.querySelector(r)?.querySelector('.update-version')?.textContent === 'CView 0.1.103', row);
  assert.equal(await page.getAttribute(`${row} .update-status`, 'role'), 'status');
  const status = () => page.textContent(`${row} .update-status`);
  await page.click(`${row} [data-sys="update"]`);
  await page.waitForFunction((r) => /최신 버전입니다/.test(document.querySelector(`${r} .update-status`).textContent), row);
  assert.equal(await status(), '최신 버전입니다 (0.1.103)');
  assert.ok(await page.isHidden(`${row} [data-sys="update-install"]`));
  await setMock(page, { update: 'newer' });
  await page.click(`${row} [data-sys="update"]`);
  await page.waitForSelector(`${row} [data-sys="update-install"]:not([hidden])`);
  assert.match(await status(), /새 버전 0\.1\.104/);
  assert.ok(await page.isVisible(`${row} [data-sys="update-page"]`), '릴리스 노트 단추');
  await setMock(page, { update: 'fail' });
  await page.click(`${row} [data-sys="update"]`);
  await page.waitForFunction((r) => /확인하지 못했습니다/.test(document.querySelector(`${r} .update-status`).textContent), row);
  assert.match(await status(), /^확인하지 못했습니다 — 업데이트 서버에 연결하지 못했습니다/);
  // 개요 탭 — 업데이트 줄은 정보 탭 한 곳에만
  await page.click(`${S} .settings-tabs [data-group="overview"]`);
  await page.waitForSelector(`${S} .settings-group[data-g="overview"]`);
  assert.equal(await page.locator(`${S} .update-row`).count(), 0);
  assert.deepEqual(errors.filter((e) => !/업데이트 서버에 연결하지 못했습니다/.test(e)), []);
  await ctx.close();
});

await check('SA03 자동으로 업데이트 확인 토글 — 실제 설정(general.autoCheckUpdates)을 읽고 쓰며 설정 전체에 한 줄', async () => {
  const { ctx, page } = await open();
  const input = `${S} .settings-group[data-g="about"] input[data-domain="general"][data-key="autoCheckUpdates"]`;
  assert.ok(await page.isChecked(input), '켜짐을 읽는다');
  const label = await page.evaluate((s) => document.querySelector(s).closest('.setting').querySelector('.setting-label').firstChild.textContent, input);
  assert.equal(label, '자동으로 업데이트 확인');
  await page.click(`${input} + span`);
  await page.waitForFunction(() => window.__about.calls.some(([c, a]) => c === 'settings_update' && a.domain === 'general'));
  assert.deepEqual((await calls(page, 'settings_update')).at(-1), { domain: 'general', patch: { autoCheckUpdates: false } });
  // 다른 탭에 갔다 와도 꺼진 채
  await page.click(`${S} .settings-tabs [data-group="general"]`);
  assert.equal(await page.locator(`${S} [data-key="autoCheckUpdates"]`).count(), 0, '일반 탭에는 없다');
  await aboutTab(page);
  assert.equal(await page.isChecked(input), false);
  // 검색 — 설정 전체에서 한 줄
  await page.fill(`${S} .settings-search`, '자동으로 업데이트');
  await page.waitForTimeout(100);
  assert.equal(await page.locator(`${S} .setting:not([hidden]) [data-key="autoCheckUpdates"]`).count(), 1);
  await ctx.close();
});

await check('SA04 새로운 기능 — 릴리스 노트 시트, GitHub에서 보기, 실패·다시 시도, 노트 없음, Esc 뒤 초점 복귀', async () => {
  const { ctx, page, errors } = await open();
  const btn = `${S} [data-about="notes"]`;
  assert.equal(await page.textContent(`${S} [data-about-text="notes"]`), '0.1.103의 새로운 기능');
  assert.equal(await page.getAttribute(btn, 'aria-haspopup'), 'dialog');
  await page.focus(btn);
  await page.keyboard.press('Enter');
  await sheetOpen(page);
  await page.waitForSelector('.notes-sheet pre.release-notes');
  const sheet = await page.evaluate(() => {
    const d = document.querySelector('.sheet[role="dialog"]');
    return { name: document.getElementById(d.getAttribute('aria-labelledby'))?.textContent, notes: d.querySelector('pre').textContent, tab: d.querySelector('pre').getAttribute('tabindex'), date: d.querySelector('.update-body p')?.textContent };
  });
  assert.equal(sheet.name, '0.1.103의 새로운 기능');
  assert.match(sheet.notes, /방송 중인 팔로잉/);
  assert.equal(sheet.tab, '0', '스크롤되는 노트는 키보드로도 닿는다');
  assert.match(sheet.date, /2026.*게시/);
  await page.click('.sheet [data-notes="page"]');
  assert.deepEqual((await calls(page, 'open_external')).at(-1), { url: NOTES_PAGE });
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.about), 'notes', 'Esc 뒤 초점이 [보기…]로');
  // 실패 → 다시 시도
  await setMock(page, { notes: 'fail' });
  await page.click(btn);
  await page.waitForSelector('.sheet [data-notes="retry"]');
  assert.match(await page.textContent('.sheet .update-body'), /릴리스 노트를 받지 못했습니다/);
  await setMock(page, { notes: 'ok' });
  await page.click('.sheet [data-notes="retry"]');
  await page.waitForSelector('.notes-sheet pre.release-notes');
  // 누른 [다시 시도]가 사라져도 초점은 시트 안에 남아 Esc 가 닿는다
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('.sheet'))), '다시 시도 뒤 초점이 시트 안');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
  // 노트 없음(개발 빌드) → 안내 + 릴리스 목록
  await setMock(page, { notes: 'none' });
  await page.click(btn);
  await page.waitForFunction(() => /릴리스 노트가 없습니다/.test(document.querySelector('.sheet .update-body')?.textContent || ''));
  await page.click('.sheet [data-notes="page"]');
  assert.deepEqual((await calls(page, 'open_external')).at(-1), { url: 'https://github.com/dododo-lab/cview-rust/releases' });
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('SA05 피드백·아래 띠 링크 — 공개된 주소를 기본 브라우저로, 지어낸 연락처·제작자 없음', async () => {
  const { ctx, page } = await open();
  await page.click(`${S} [data-about="feedback"]`);
  assert.deepEqual((await calls(page, 'open_external')).at(-1), { url: 'https://github.com/dododo-lab/cview-rust/issues/new/choose' });
  const before = page.url();
  const links = await page.$$eval(`${S} .about-links a`, (as) => as.map((a) => [a.textContent.trim(), a.getAttribute('href')]));
  assert.deepEqual(links, [
    ['웹사이트', 'https://cv.dododo.app/'],
    ['변경 내역', 'https://github.com/dododo-lab/cview-rust/releases'],
    ['소스 코드', 'https://github.com/dododo-lab/cview-rust'],
  ]);
  for (const [text, href] of links) {
    await page.click(`${S} .about-links a:text-is("${text}")`);
    assert.deepEqual((await calls(page, 'open_external')).at(-1), { url: href }, text);
  }
  assert.equal(page.url(), before, '웹뷰는 다른 주소로 가지 않는다');
  assert.equal(await page.getAttribute(`${S} .about-links`, 'aria-label'), 'CView 링크');
  const text = await page.textContent(`${S} .settings-groups.about-tab`);
  assert.doesNotMatch(text, /@[\w-]+\.\w/, '메일 주소를 지어내지 않는다');
  assert.doesNotMatch(text, /제작|개인정보 처리방침|개인정보 보호/, '확인 안 된 제작자·정책 링크 없음');
  assert.match(text, /비공식/);
  await ctx.close();
});

await check('SA06 진단 보고서 — 빼는 개인 정보 안내, 복사(ClipboardItem → writeText → 시트), 내용 보기, 초점 유지', async () => {
  const { ctx, page, errors } = await open();
  const help = await page.evaluate(() => document.querySelector('[data-about="report-copy"]').closest('.setting').querySelector('small').textContent);
  for (const w of ['쿠키', '토큰', '계정', '채팅', '팔로잉']) assert.ok(help.includes(w), `설명에 ${w}`);
  const copy = `${S} [data-about="report-copy"]`;
  await page.focus(copy);
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.__about.clipped.length === 1);
  assert.deepEqual(await page.evaluate(() => window.__about.clipped[0]), ['write', REPORT], 'ClipboardItem 으로 백엔드 글 그대로');
  await page.waitForSelector('.toast-stack :text("진단 보고서를 복사했습니다")');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.about), 'report-copy', '누른 단추에 초점이 남는다');
  assert.equal(await page.getAttribute(copy, 'aria-disabled'), null, '끝나면 다시 누를 수 있다');
  // ClipboardItem 이 막히면 writeText
  await setMock(page, { clip: 'write-fail' });
  await page.click(copy);
  await page.waitForFunction(() => window.__about.clipped.length === 2);
  assert.deepEqual(await page.evaluate(() => window.__about.clipped[1]), ['writeText', REPORT]);
  // 둘 다 막히면 글을 고른 채 시트로
  await setMock(page, { clip: 'none' });
  await page.click(copy);
  await sheetOpen(page);
  await page.waitForSelector('.report-sheet .report-text');
  const sheet = await page.evaluate(() => {
    const t = document.querySelector('.report-text');
    return { value: t.value, focused: document.activeElement === t, sel: [t.selectionStart, t.selectionEnd], ro: t.readOnly, note: document.querySelector('.report-note').textContent };
  });
  assert.equal(sheet.value, REPORT);
  assert.ok(sheet.focused && sheet.ro, '읽기 전용 글에 초점');
  assert.deepEqual(sheet.sel, [0, REPORT.length], '모두 골라 둔다');
  assert.match(sheet.note, /클립보드에 넣지 못했습니다/);
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
  // 내용 보기 → 시트의 [복사]
  await setMock(page, { clip: 'ok' });
  await page.click(`${S} [data-about="report-view"]`);
  await page.waitForSelector('.report-sheet .report-text');
  assert.match(await page.textContent('.report-note'), /쿠키·토큰/);
  await page.click('.sheet [data-report-copy]');
  await page.waitForFunction(() => window.__about.clipped.length === 3);
  assert.equal((await page.evaluate(() => window.__about.clipped[2]))[1], REPORT);
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
  // 보고서를 못 만들면 알린다(빈 글을 복사하지 않는다)
  await setMock(page, { report: 'fail' });
  await page.click(copy);
  await page.waitForSelector('.toast-stack :text("진단 보고서를 만들지 못했습니다")');
  assert.equal(await page.evaluate(() => window.__about.clipped.length), 3);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('SA07 사용 데이터 — macOS Finder에서 보기 · Windows 탐색기에서 보기, 앱 데이터 폴더를 연다', async () => {
  for (const windows of [false, true]) {
    const { ctx, page } = await open({ windows });
    const b = `${S} [data-about="data"]`;
    const want = windows ? '탐색기에서 보기' : 'Finder에서 보기';
    assert.equal((await page.textContent(b)).trim(), want);
    assert.equal(await page.getAttribute(b, 'aria-label'), `사용 데이터 폴더를 ${want}`);
    await page.click(b);
    await page.waitForFunction(() => window.__about.calls.some(([c]) => c === 'reveal_path'));
    assert.deepEqual((await calls(page, 'reveal_path')).at(-1), { path: DATA_DIR });
    await ctx.close();
  }
});

await check('SA08 검색 — 정보 탭 줄도 찾고, 검색 중엔 머리말·링크 띠가 없다', async () => {
  const { ctx, page } = await open();
  await page.fill(`${S} .settings-search`, '진단');
  await page.waitForTimeout(100);
  const st = await page.evaluate(() => ({
    rows: [...document.querySelectorAll('[data-page="settings"] .setting:not([hidden])')].filter((r) => r.offsetParent).map((r) => r.querySelector('.setting-label')?.firstChild?.textContent.trim()),
    hero: Boolean(document.querySelector('.about-hero')), foot: Boolean(document.querySelector('.about-foot')),
    lit: document.querySelectorAll('.settings-tabs .seg.active').length, oneCol: document.querySelector('.settings-groups').classList.contains('about-tab'),
  }));
  assert.ok(st.rows.includes('진단 보고서'), JSON.stringify(st.rows));
  assert.deepEqual([st.hero, st.foot, st.lit, st.oneCol], [false, false, 0, false]);
  await page.fill(`${S} .settings-search`, '');
  await page.waitForSelector(`${S} .about-hero`);
  await ctx.close();
});

for (const theme of ['light', 'dark']) {
  await check(`SA09 ${theme} axe — 정보 탭과 두 시트에 위반 없음(글자 대비 포함)`, async () => {
    const { ctx, page } = await open({ theme });
    await page.waitForTimeout(500);
    await page.evaluate(AXE);
    const run = (sel) => page.evaluate(async (s) => {
      const r = await window.axe.run(document.querySelector(s), { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
      return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
    }, sel);
    assert.deepEqual(await run(`${S} .settings-groups.about-tab`), [], '정보 탭');
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `about-${theme}-${WEBKIT ? 'webkit' : 'chromium'}.png`) });
    await page.click(`${S} [data-about="notes"]`);
    await page.waitForSelector('.notes-sheet pre.release-notes');
    await settleSheet(page);
    assert.deepEqual(await run('.sheet-scrim.app-sheet'), [], '노트 시트');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
    await page.click(`${S} [data-about="report-view"]`);
    await page.waitForSelector('.report-sheet .report-text');
    await settleSheet(page);
    assert.deepEqual(await run('.sheet-scrim.app-sheet'), [], '보고서 시트');
    await ctx.close();
  });
}

await check('SA10 키보드 — Tab 이 단추·토글·링크를 차례로 지나고 초점마다 보이는 표시가 있다', async () => {
  const { ctx, page } = await open();
  const LINKS = ['웹사이트', '변경 내역', '소스 코드'];
  const want = await page.evaluate(() => [...document.querySelectorAll('[data-page="settings"] .settings-groups.about-tab :is(button, input, a[href])')]
    .filter((e) => !e.hidden && !e.closest('[hidden]') && !e.disabled).map((e) => e.dataset.sys || e.dataset.about || e.dataset.key || e.textContent.trim()));
  assert.deepEqual(want.slice(-3), LINKS, '링크는 맨 끝');
  const focused = () => page.evaluate(() => {
    const e = document.activeElement;
    const target = e.matches('.toggle input') ? e.nextElementSibling : e;
    const cs = getComputedStyle(target);
    const ring = (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) >= 1) || (cs.boxShadow && cs.boxShadow !== 'none');
    return { id: e.dataset.sys || e.dataset.about || e.dataset.key || e.textContent.trim(), ring, inTab: Boolean(e.closest('.settings-groups.about-tab')) };
  });
  // WebKit 은 기본 설정에서 Tab 이 링크를 건너뛴다(Safari '탭으로 링크 이동' 꺼짐) — 링크는 아래에서 따로 잰다.
  const expect = WEBKIT ? want.filter((w) => !LINKS.includes(w)) : want;
  await page.focus(`${S} .settings-search`);
  const seen = [];
  for (let i = 0; i < expect.length; i++) {
    await page.keyboard.press('Tab');
    const f = await focused();
    assert.ok(f.inTab, `Tab ${i + 1} 이 정보 탭 밖(${f.id})`);
    assert.ok(f.ring, `${f.id} 초점 표시 없음`);
    seen.push(f.id);
  }
  assert.deepEqual(seen, expect);
  for (const text of LINKS) {
    await page.keyboard.press('Shift'); // 마지막 입력이 키보드 — :focus-visible
    await page.focus(`${S} .about-links a:text-is("${text}")`);
    const f = await focused();
    assert.ok(f.ring, `${text} 초점 표시 없음`);
  }
  await ctx.close();
});

for (const [width, height] of [[1000, 660], [1280, 900]]) {
  for (const theme of ['light', 'dark']) {
    await check(`SA11 ${width}×${height} ${theme} 배치 — 가로 넘침 없음, 단추가 카드 안, 머리말 가운데`, async () => {
      const { ctx, page } = await open({ width, height, theme });
      await setMock(page, { update: 'newer' });
      await page.click(`${S} [data-sys="update"]`); // 단추가 가장 많은 상태(릴리스 노트·업데이트 설치·업데이트 확인)
      await page.waitForSelector(`${S} [data-sys="update-install"]:not([hidden])`);
      const m = await page.evaluate(() => {
        const out = [];
        const page = document.querySelector('[data-page="settings"]');
        if (page.scrollWidth > page.clientWidth + 1) out.push(`설정 화면 가로 넘침 ${page.scrollWidth}>${page.clientWidth}`);
        if (document.documentElement.scrollWidth > innerWidth + 1) out.push('문서 가로 넘침');
        for (const row of document.querySelectorAll('.settings-groups.about-tab .setting')) {
          const card = row.closest('.settings-group').getBoundingClientRect();
          const label = row.querySelector('.setting-label').getBoundingClientRect();
          for (const b of row.querySelectorAll('button:not([hidden]), .toggle')) {
            const r = b.getBoundingClientRect();
            if (r.right > card.right + 0.5 || r.left < card.left - 0.5) out.push(`${b.textContent.trim() || 'toggle'} 카드 밖`);
            if (r.top < label.bottom - 1 && r.left < label.right - 1 && r.bottom > label.top + 1) out.push(`${b.textContent.trim() || 'toggle'} 글과 겹침`);
          }
          if (row.scrollWidth > row.clientWidth + 1) out.push(`${row.querySelector('.setting-label').textContent.slice(0, 12)} 줄 넘침`);
        }
        const col = document.querySelector('.settings-groups.about-tab').getBoundingClientRect();
        const icon = document.querySelector('.about-icon').getBoundingClientRect();
        if (Math.abs((icon.left + icon.right) / 2 - (col.left + col.right) / 2) > 2) out.push('아이콘이 가운데가 아님');
        const foot = document.querySelector('.about-foot');
        if (foot.scrollWidth > foot.clientWidth + 1) out.push('링크 띠 넘침');
        return out;
      });
      if (SHOTS) {
        await page.screenshot({ path: path.join(SHOTS, `about-${width}-${theme}-${WEBKIT ? 'webkit' : 'chromium'}.png`), fullPage: true });
      }
      assert.deepEqual(m, []);
      await ctx.close();
    });
  }
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) { failed += 1; console.log(`    ${r.error.replace(/\n/g, '\n    ')}`); }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
