// 기능 정밀 고도화 9차(2026-11-27) 회귀 검사 — 여러 창 동기화·장시간 누적·오류 문구·키보드·Windows 경로·지난 수정 주변.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass-9.mjs
// (처음 실행 고지와 시작 흐름은 first-run-notice.mjs N12~N15)
// P01 다른 창에서 바꾼 팔로우·즐겨찾기(social://changed)를 이 창도 따르고, 보낸 창(origin)은 거른다.
// P02 다른 창에서 바꾼 재생·일반 설정(settings://player·general)을 이 창도 따른다(후원 알림 위치 등).
// P03 오류 안내 — Rust 한국어 문구(접근 권한·찾을 수 없음·요청이 많아·서버 응답 해석)를 맞는 원인으로 바꾼다.
// P04 맥락 메뉴 키·Shift+F10 으로 카드의 채널 메뉴를 연다(macOS).
// P05 Windows — F5 는 문서를 다시 불러오지 않고 앱 새로고침으로, Ctrl+P 는 막는다.
// P06 이모티콘 캐시 만료(순수 함수)·보관함 '오늘 올라옴'은 달력 날짜로 센다.
// P07 설정 — 효과 없던 '여러 패인 소리' 토글이 없고, 보조 패인 볼륨을 바꿀 수 있다.
// P08 업데이트 확인 시트 — 키보드로 '다시 시도'를 눌러도 초점이 시트에 남아 Esc 로 닫힌다.
// P09 코드 확인 — 멀티라이브 채팅 메뉴 Esc 가 단독 보기를 닫지 않음, 채팅 통계 분당 평균, 다시보기 첫 로드 중 화질 변경,
//     패인 재시작 뒤 화질 다시 읽기, 숨은 화면의 전체 화면 풀기, 멀티라이브 후원 목록 /mute, 인사이트 캐시 비우기.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const WIN_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Edg/131.0';
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}
const src = (f) => fs.readFileSync(path.resolve('app-ui/js', f), 'utf8');

/** 이벤트를 모아 두는 listen + 창 이름(label) — 검사가 window.__ev[이름] 처리기를 직접 부른다. */
function captureEvents(label) {
  window.__ev = {};
  window.__TAURI__.event.listen = async (name, cb) => { (window.__ev[name] ||= []).push(cb); return () => {}; };
  window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ label, listen: (n, cb) => window.__TAURI__.event.listen(n, cb) }) };
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}

async function open({ ua = MAC_UA, extra = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: ua });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(captureEvents, 'main');
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const fire = (page, name, payload) => page.evaluate(([n, p]) => (window.__ev[n] || []).forEach((cb) => cb({ payload: p })), [name, payload]);

try {
  await check('P01 다른 창의 팔로우·즐겨찾기 변경을 이 창도 따르고, 보낸 창 자신의 알림은 거른다', async () => {
    const { ctx, page } = await open();
    await page.waitForFunction(() => (window.__ev['social://changed'] || []).length > 0);
    await page.evaluate(() => { window.__social = []; window.addEventListener('cview:channel-social', (e) => window.__social.push(e.detail)); });
    await fire(page, 'social://changed', { origin: 'player-1', channelId: 'ch1', favorite: false });
    await fire(page, 'social://changed', { origin: 'main', channelId: 'ch2', following: true });
    const got = await page.evaluate(() => window.__social);
    assert.deepEqual(got, [{ channelId: 'ch1', favorite: false }], JSON.stringify(got));
    // 덮어쓰기도 바뀌어, 이 창에서 다시 읽으면 서버·DB 캐시보다 앞선다
    const st = await page.evaluate(async () => (await import('/js/channel-actions.js')).loadChannelSocial('ch1', false));
    assert.equal(st.favorite, false);
    await ctx.close();
  });

  await check('P02 다른 창에서 바꾼 재생·일반 설정을 이 창도 따른다', async () => {
    const { ctx, page } = await open();
    await page.waitForFunction(() => (window.__ev['settings://player'] || []).length > 0);
    await fire(page, 'settings://player', { streamAlertAnchorX: 0.2, streamAlertAnchorY: 0.5, streamAlertScale: 1.5, continuePlaybackInBackground: false });
    const v = await page.evaluate(() => ['--alert-x', '--alert-y', '--alert-scale'].map((k) => document.documentElement.style.getPropertyValue(k)));
    assert.deepEqual(v, ['0.2', '0.5', '1.5']);
    assert.ok((await page.evaluate(() => (window.__ev['settings://general'] || []).length)) > 0, 'general 도 듣는다');
    await ctx.close();
  });

  await check('P03 오류 안내 — Rust 한국어 문구를 맞는 원인으로', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { friendlyError } = await import('/js/cards.js');
      return ['접근 권한이 없습니다', '요청한 정보를 찾을 수 없습니다', '요청이 많아 잠시 막혔습니다 — 5초 뒤 다시 시도해 주세요',
        '요청이 많아 잠시 제한되었습니다', '서버 응답을 읽지 못했습니다', '서버 응답 형식이 예상과 다릅니다', '서버에 문제가 생겼습니다',
        '네트워크에 연결하지 못했습니다', '로그인이 필요합니다'].map((m) => friendlyError(new Error(m)));
    });
    assert.match(r[0], /접근 권한/);
    assert.match(r[1], /찾을 수 없습니다/);
    assert.match(r[2], /요청이 많아/);
    assert.match(r[3], /요청이 많아/);
    assert.match(r[4], /해석하지 못했습니다/);
    assert.match(r[5], /해석하지 못했습니다/);
    assert.match(r[6], /서버가 응답하지 않습니다/);
    assert.match(r[7], /인터넷 연결/);
    assert.match(r[8], /로그인/);
    await ctx.close();
  });

  await check('P04 맥락 메뉴 키·Shift+F10 — 카드의 채널 메뉴를 연다(macOS)', async () => {
    const { ctx, page } = await open();
    await go(page, 'popular');
    const card = page.locator('[data-page="popular"] [data-channel-id] :is(a, button, [tabindex])').first();
    await card.waitFor();
    await card.focus();
    await page.keyboard.press('Shift+F10');
    await page.waitForSelector('.ctx-menu');
    assert.equal(await page.locator('.ctx-menu').count(), 1, '메뉴는 하나');
    assert.match(await page.textContent('.ctx-menu'), /채팅만 열기/);
    assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('ctx-item')), true, '첫 항목에 초점');
    await page.keyboard.press('Escape');
    await page.keyboard.press('ContextMenu');
    await page.waitForSelector('.ctx-menu');
    await ctx.close();
  });

  await check('P05 Windows — F5 는 앱 새로고침으로(문서를 다시 불러오지 않음), Ctrl+P 는 막는다', async () => {
    const { ctx, page } = await open({ ua: WIN_UA });
    await page.evaluate(() => {
      window.__alive = 1;
      window.__menus = [];
      window.addEventListener('cview:menu', (e) => window.__menus.push(e.detail));
      window.__prevented = [];
      window.addEventListener('keydown', (e) => { if (e.key !== 'Control') window.__prevented.push([e.key, e.defaultPrevented]); });
    });
    await page.keyboard.press('F5');
    await page.keyboard.press('Control+p');
    await page.waitForTimeout(400);
    const r = await page.evaluate(() => ({ alive: window.__alive, menus: window.__menus, prevented: window.__prevented }));
    assert.equal(r.alive, 1, '문서를 다시 불러오지 않았다');
    assert.ok(r.menus.includes('refresh'), JSON.stringify(r.menus));
    assert.ok(r.prevented.every(([, p]) => p), JSON.stringify(r.prevented));
    await ctx.close();
  });

  await check('P06 이모티콘 캐시 만료·보관함 \'오늘 올라옴\'은 달력 날짜', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { emoticonCacheFresh } = await import('/js/chat.js');
      const { summarizeSection } = await import('/js/library-overview.js');
      const now = new Date(2026, 10, 27, 9, 0).getTime();
      const fresh = [emoticonCacheFresh({ packs: [], at: now - 60000 }, now), emoticonCacheFresh({ packs: [], at: now - 11 * 60000 }, now), emoticonCacheFresh(null, now)];
      const vods = [
        { videoTitle: 'a', publishDate: new Date(2026, 10, 27, 0, 30).getTime() }, // 오늘
        { videoTitle: 'b', publishDate: new Date(2026, 10, 26, 10, 0).getTime() }, // 어제(24시간 안)
      ];
      const s = summarizeSection('vods', vods, now);
      return { fresh, today: s.metrics.find(([k]) => k === '오늘 올라옴')?.[1] };
    });
    assert.deepEqual(r.fresh, [true, false, false]);
    assert.equal(r.today, '1개');
    await ctx.close();
  });

  await check('P07 설정 — \'여러 패인 소리\' 토글이 없고 보조 패인 볼륨은 늘 바꿀 수 있다', async () => {
    const s = src('settings.js');
    assert.doesNotMatch(s, /k: 'multiAudioEnabled'/);
    const row = s.match(/\{ d: 'multiLive', k: 'secondaryVolume'[^\n]*\}/);
    assert.ok(row, '보조 패인 볼륨 줄');
    assert.doesNotMatch(row[0], /disabled/);
  });

  await check('P08 업데이트 확인 시트 — 키보드로 \'다시 시도\'를 눌러도 초점이 시트에 남고 Esc 로 닫힌다', async () => {
    const { ctx, page } = await open({ extra: { check_for_update: () => { throw new Error('서버에 연결하지 못했습니다'); }, update_check: () => { throw new Error('서버에 연결하지 못했습니다'); } } });
    await page.evaluate(async () => (await import('/js/app-sheets.js')).showUpdateCheck());
    const retry = page.locator('[data-update="retry"]');
    await retry.waitFor();
    await retry.focus();
    await page.keyboard.press('Enter');
    await page.waitForTimeout(300);
    const inSheet = await page.evaluate(() => Boolean(document.activeElement && document.activeElement.closest('.sheet')));
    assert.ok(inSheet, `초점: ${await page.evaluate(() => document.activeElement?.tagName)}`);
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => !document.querySelector('.update-sheet'));
    await ctx.close();
  });

  await check('P09 코드 확인 — Esc·통계·화질·전체 화면·/mute 후원·인사이트 캐시', async () => {
    const mc = src('ml-multichat.js');
    assert.match(mc, /if \(e\.key !== 'Escape' \|\| this\.menu\.hidden\) return;\s*\n\s*e\.preventDefault\(\);/, '채팅 메뉴 Esc 는 preventDefault');
    assert.match(mc, /passesFilters\(m\) && !\(hide && hide\(m\)\)/, '후원 목록이 /mute 를 본다');
    const ce = src('chat-extras.js');
    assert.match(ce, /Math\.round\(stats\.total \/ mins\)/, '분당 평균은 누적 수로');
    assert.match(ce, /stats\.total \+= 1;/);
    const media = src('media.js');
    assert.match(media, /const keepPosition = v\.readyState >= 1;/);
    assert.match(media, /if \(keepPosition\) v\.currentTime = at;/);
    assert.match(src('ml-pane-controls.js'), /if \(type === 'playStarted'\) this\.bridge\.call\('getLevels'\);/);
    const app = src('app.js');
    assert.match(app, /exitHiddenFullscreen\(Object\.values\(pages\)\.map\(\(p\) => p\.root\)\);/);
    const ins = src('insight-panel.js');
    assert.match(ins, /addEventListener\(CHANNEL_SOCIAL_EVENT, \(\) => cache\.clear\(\)\)/);
    assert.match(ins, /listen\(Events\.authChanged, \(\) => cache\.clear\(\)\)/);
    assert.match(src('chat.js'), /listen\(Events\.authChanged, \(\) => emoticonCache\.clear\(\)\);/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`${total - failures.length}/${total} passed`);
if (failures.length) { console.log(`failed: ${failures.join(', ')}`); process.exit(1); }
