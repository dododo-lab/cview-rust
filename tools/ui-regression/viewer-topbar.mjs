// 시청 화면 상단 바 재배치 회귀 검사 — 라이브 단독 보기(ml-watch, 실제 라이브 시청 화면)와 예전 시청 화면(#viewer, 보조 창·칸이 가득 찼을 때).
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다).
// TB01 모든 기존 기능이 좁음·보통·넓음 폭에서 어딘가(위·아래 막대, ⋯ 메뉴, ⚙ 메뉴)에 보이고 닿는다.
// TB02 폭마다 위·아래 막대 단추가 패인 안에 있고 서로·제목 칸과 겹치지 않는다(넘침·잘림 없음). 열린 ⋯ 메뉴도 패인 안.
// TB03 폭 단계 — 넓음: 팔로우 글자, 보통: 팔로우 아이콘만, 좁음: 채널·보기 묶음이 접히고 그 항목이 ⋯ 에 나온다.
// TB04 위 막대 동작이 기존 명령을 부른다 — 팔로우(channel_follow)·즐겨찾기(favorite_set)·링크 복사(클립보드)·
//      치지직에서 열기(open_external)·멀티라이브에 추가(멀티라이브 보기로)·채팅 표시 방식(설정 저장).
// TB05 화질·지연 배지 — 지표가 오면 높이·지연이 보이고, 8초 넘게 늦으면 경고 표시, 누르면 ⚙ 화질 메뉴(Esc 면 배지로 초점).
// TB06 ⋯ 메뉴 키보드 — Enter 로 열면 첫 항목, ↓/End 이동, Esc 로 닫고 ⋯ 로 초점. 단축키 표시(바꾼 키 반영, Windows 표기).
// TB07 axe(WCAG 2.1 A·AA + best-practice) — 위 막대와 열린 ⋯ 메뉴에 위반이 없다.
// TB08 로그아웃 — 팔로우 단추·항목이 숨고 나머지는 그대로.
// TB09 멀티라이브 그리드 패인 — 위 막대·⋯ 메뉴가 예전 그대로(단독 보기 전용 묶음·항목·묶음 이름이 없다).
// TB10 예전 시청 화면(#viewer) — 같은 짜임(채널·보기 묶음, ⋯ 메뉴 항목·단축키), 폭 단계, 배지, Esc, axe.
// TB11 라이브 머리줄(채널 칩 줄) — 좁음·보통·넓음 × 화면 크기 80~200%(웹뷰 확대 = CSS 폭이 줄어듦): [전환] │ [칩] [채널 추가] 가
//      붙어 있고, 머리줄이 넘치지 않으며, 보이는 칩 수가 머리줄 폭을 따르고(chipWindowFor), 모든 채널이 칩 또는 "+N" 메뉴로 닿는다.
//      넓은 창에서는 6개가 다 보이고 이름이 잘리지 않는다.
// TB12 칩 키보드 — ←/→ 옆 칩, End/Home 끝(가려진 칩이면 창을 옮김), Delete 는 그 채널 닫기(초점은 옆 칩). 시청 단축키(←/→ 되감기)로 새지 않는다.
// TB13 "+N" 넘침 메뉴 — 가려진 채널을 이름·상태와 함께, 창 안(fitPopover)에, Enter 로 그 채널을 라이브로, Esc 면 "+N" 로 초점. axe.
// TB14 칩 상태 — 방송 종료·연결 중은 아바타 점(st-*)과 이름(aria-label)으로 알린다.
// 저장소 루트에서: node tools/ui-regression/viewer-topbar.mjs   (Playwright·axe-core 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진, CVIEW_UI_SHOTS=<폴더> 면 폭마다 장면을 PNG 로 남긴다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0';
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 14).join('\n') }); }
}

// 방송 중 채널·멀티라이브 세션 — fit-polish.mjs liveMock 과 같은 모양에 호출 기록(인자 포함)을 더했다.
function liveMock([opts]) {
  const open = new Date(Date.now() - 5400e3).toISOString();
  const live = { liveTitle: '오늘은 신작 게임 정주행 — 시청자 참여 내전과 경품 추첨까지 함께', categoryName: '마인크래프트', viewerCount: 12400, viewerCountText: '12,400',
    accumulateCount: 48210, accumulateCountText: '48,210', openDate: open, tags: ['게임', '소통'], isLive: true };
  const channel = { channelId: 'ch1', channelName: '한동숙', channelImageUrl: `${location.origin}/__img/avatar/1.svg`, verifiedMark: true, followerCount: 523000, channelDescription: '매일 저녁 8시 방송합니다.' };
  const session = (id, i, name) => ({ id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: live.liveTitle, profileImageUrl: null, viewerCountText: '1만',
    accumulateCount: 48210, accumulateCountText: '4.8만', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false });
  // 머리줄 칩 검사용 — opts.channels 개(기본 2)를 연다. 3번째부터는 이름이 긴 채널·상태(연결 중·종료) 섞기.
  const NAMES = ['한동숙', '침착맨', '서새봄냥 SEOSAEBOM', '똘똘똘이', '귀염1', '쉐리', '풍월량', '릴카', '따효니', '김도'];
  const sessions = Array.from({ length: opts.channels || 2 }, (_, i) => {
    const s = session(`ch${i + 1}`, i, NAMES[i % NAMES.length]);
    s.profileImageUrl = `${location.origin}/__img/avatar/${i + 1}.svg`;
    if (opts.channels && i === 3) s.state = { kind: 'offline' };
    if (opts.channels && i === 4) s.state = { kind: 'loading' };
    return s;
  });
  const state = { sessions, selectedId: 's0', focusMainId: 's0', canAdd: !opts.full, maxSessions: 4,
    layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 };
  window.__ipc = [];
  window.__copied = [];
  try { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (t) => { window.__copied.push(t); } } }); } catch { /* 무시 */ }
  const over = {
    channel_detail: ({ channelId }) => ({ channel: { ...channel, channelId }, live: { ...live }, isFavorite: false }), channel_info: () => ({ verified: true, followerCount: 523000 }),
    live_detail: ({ channelId }) => ({ ...live, channelId, channelName: channelId === 'ch1' ? '한동숙' : '침착맨', channelImageUrl: channel.channelImageUrl, chatChannelId: null, verified: true }),
    player_start: ({ channelId }) => ({ live: { ...live, channelId, channelName: '풍월량', channelImageUrl: channel.channelImageUrl, chatChannelId: null }, audioOnly: false }),
    multilive_state: () => state, multilive_stage_size: () => null,
    multilive_set_layout: ({ layout }) => { state.layout = layout; state.rev++; return state; }, multilive_set_focus_layout: () => state,
    multilive_watch: ({ sessionId }) => { state.watchId = sessionId; state.rev++; return state; },
    multilive_add: () => { state.rev++; return state; }, multilive_select: ({ sessionId }) => { state.selectedId = sessionId; state.rev++; return state; },
    channel_follow: () => null, favorite_set: () => null, open_external: () => null, chat_donation_ranking: () => [],
    following_channels_all: () => (opts.loggedOut ? Promise.reject(new Error('로그인이 필요합니다')) : [{ channelId: 'ch1', channelName: '한동숙' }]),
    favorites_list: () => [],
  };
  if (opts.loggedOut) over.auth_status = () => ({ loggedIn: false, state: 'loggedOut' });
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (cmd !== 'ui_heartbeat') window.__ipc.push({ cmd, args: args ? JSON.parse(JSON.stringify(args)) : null });
    if (over[cmd]) return over[cmd](args || {});
    try { return await orig(cmd, args); } catch (e) { if (/^(multilive_|chat_|player_|window_|bookmark_|save_)/.test(cmd)) return null; throw e; }
  };
}

const browser = await engine.launch();
async function open({ width = 1280, height = 780, theme = 'dark', userAgent, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: 'reduce', ...(userAgent ? { userAgent } : {}) });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  // 채팅 표시 방식을 바꿀 때 WebKit 이 내는 무해한 'ResizeObserver loop' 알림은 빼고 센다(main 에서도 같다 — ml-view-picker.mjs 와 같은 처리).
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(liveMock, [opts]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}

const PANE = '.page-multilive.as-watch .ml-pane.ml-single-target';

/** 라이브 단독 보기로 들어가 무대 패인 컨트롤을 보이게 한다. 가짜 영상·지표(1080p·지연 2.1초)를 깐다. */
async function openWatch(opts = {}) {
  const o = await open(opts);
  const { page } = o;
  await go(page, 'multilive');
  await page.waitForTimeout(300);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
  await page.waitForSelector(PANE);
  await page.waitForFunction((sel) => { const f = document.querySelector(`${sel} iframe`); return f && f.contentDocument?.readyState === 'complete' && f.contentDocument.getElementById('video'); }, PANE);
  await feedMetrics(page, PANE, 2.14);
  await page.waitForTimeout(400); // 팔로우·즐겨찾기 상태
  await wakeWatch(page);
  return o;
}

async function feedMetrics(page, paneSel, latency) {
  await page.evaluate(([sel, lat]) => {
    const p = document.querySelector(sel);
    const f = p.querySelector('iframe');
    try { const v = f.contentDocument.getElementById('video'); if (v) Object.defineProperty(v, 'videoHeight', { get: () => 1080, configurable: true }); } catch { /* 무시 */ }
    window.__cviewPlayerBridge({ type: 'metrics', data: { latency: lat, currentLevelHeight: 1080 } }, f.contentWindow);
    let fv = p.querySelector('.fake-video');
    if (!fv) { fv = document.createElement('div'); fv.className = 'fake-video'; p.prepend(fv); }
    Object.assign(fv.style, { position: 'absolute', inset: '0', background: 'linear-gradient(135deg, hsl(210 60% 42%), hsl(265 55% 22%))', pointerEvents: 'none', zIndex: '1' });
  }, [paneSel, latency]);
  await page.waitForTimeout(900); // 패인 상태 맞춤 주기(700ms)
}

/** 컨트롤이 숨지 않게 — 위 막대 위에 마우스를 두고 깨운다. */
async function wakeWatch(page) {
  const r = await page.$eval(PANE, (p) => { const b = p.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + 40 }; });
  await page.mouse.move(r.x, r.y);
  await page.mouse.move(r.x + 2, r.y + 1);
  await page.waitForTimeout(260);
}

const shot = async (page, name) => {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: path.join(SHOTS, `${WEBKIT ? 'webkit-' : ''}${name}.png`) });
};

// 기능 → 닿는 단추(패인 안). where: 'bar' 위·아래 막대 / 'more' ⋯ 메뉴 / 'gear' ⚙ 메뉴
const WATCH_FEATURES = [
  ['채널 정보', '[data-ml="info"]'], ['팔로우', '[data-mw="follow"]'], ['즐겨찾기', '[data-mw="fav"]'], ['링크 복사', '[data-mw="copy"]'],
  ['치지직에서 열기', '[data-mw="external"]'], ['멀티라이브에 추가', '[data-mw="multi"]'], ['북마크', '[data-mw="bookmark"]'], ['스크린샷', '[data-mw="shot"]'],
  ['다시 연결', '[data-ml="retry"]'], ['채팅 표시 방식', '[data-mw="chatmode"]'], ['새 창으로 분리', '[data-mw="popout"]'], ['PiP', '[data-mp="pip"]'],
  ['영상+채팅 전체 화면', '[data-mw="fullchat"]'], ['전체 화면', '[data-mp="fullscreen"]'], ['닫기', '[data-ml="remove"]'],
  ['재생/일시정지', '.ml-pane-bar [data-mp="play"]'], ['소리·볼륨', '.ml-pane-bar [data-ml="audio"]'],
  ['화질', '[data-mp="level"]'], ['라이브로 이동', '.ml-gear-menu [data-mp="live"]'], ['재생 속도', '[data-mp="rate"]'], ['화질·지연 메뉴', '.ml-pane-bar [data-mp="gear"]'],
];

/** 패인 안에서 지금 보이는 요소인가(크기·visibility·불투명). */
const visibleSels = (page, paneSel, sels) => page.$eval(paneSel, (p, list) => list.filter((s) => [...p.querySelectorAll(s)].some((el) => {
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && !el.closest('[hidden]');
})), sels);

async function toggleMore(page, paneSel = PANE) {
  await page.$eval(paneSel, (p) => { p.classList.add('ctl-visible'); p.querySelector('[data-mp="more"]').click(); });
  await page.waitForTimeout(180);
}
async function toggleGear(page, paneSel = PANE) {
  await page.$eval(paneSel, (p) => { p.classList.add('ctl-visible'); p.querySelector('.ml-pane-bar [data-mp="gear"]').click(); });
  await page.waitForTimeout(180);
}

/** 모든 기능이 막대·⋯·⚙ 중 어딘가에서 보이는지 — 빠진 기능 이름 목록. */
async function unreachable(page, features, paneSel = PANE) {
  const sels = features.map((f) => f[1]);
  const seen = new Set(await visibleSels(page, paneSel, sels));
  await toggleMore(page, paneSel);
  for (const s of await visibleSels(page, paneSel, sels)) seen.add(s);
  await toggleMore(page, paneSel);
  await toggleGear(page, paneSel);
  for (const s of await visibleSels(page, paneSel, sels)) seen.add(s);
  await toggleGear(page, paneSel);
  return features.filter((f) => !seen.has(f[1])).map((f) => f[0]);
}

/** 막대 안 보이는 단추들의 겹침·패인 밖 넘침, 제목 칸과 단추 겹침. */
const layoutProblems = (page, paneSel, barSel, textSel) => page.$eval(paneSel, (p, [bars, textQ]) => {
  const out = [];
  const pr = p.getBoundingClientRect();
  for (const bar of p.querySelectorAll(bars)) {
    const btns = [...bar.querySelectorAll('button')].filter((b) => { const r = b.getBoundingClientRect(); return r.width > 0 && r.height > 0 && !b.closest('[hidden]') && !b.closest('.ml-pane-text, .viewer-title'); });
    const rects = btns.map((b) => ({ b, r: b.getBoundingClientRect(), name: b.getAttribute('aria-label') || b.dataset.mw || b.dataset.mp || b.className }));
    for (const { r, name } of rects) {
      if (r.left < pr.left - 1 || r.right > pr.right + 1 || r.top < pr.top - 1 || r.bottom > pr.bottom + 1) out.push(`${name} 이 패인 밖 (${Math.round(r.left)}~${Math.round(r.right)} / ${Math.round(pr.left)}~${Math.round(pr.right)})`);
    }
    for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i].r; const c = rects[j].r;
      if (a.left < c.right - 1 && c.left < a.right - 1 && a.top < c.bottom - 1 && c.top < a.bottom - 1) out.push(`${rects[i].name} ↔ ${rects[j].name} 겹침`);
    }
    const text = textQ && bar.querySelector(textQ);
    if (text && text.getClientRects().length) {
      const t = text.getBoundingClientRect();
      for (const { r, name } of rects) if (r.left < t.right - 1 && t.left < r.right - 1 && r.top < t.bottom - 1 && t.top < r.bottom - 1) out.push(`제목 칸 ↔ ${name} 겹침`);
    }
    if (bar.scrollWidth > bar.clientWidth + 1) out.push(`${bar.className} 가로 넘침 ${bar.scrollWidth - bar.clientWidth}px`);
  }
  return out;
}, [barSel, textSel]);

const menuInside = (page, paneSel, menuSel) => page.$eval(paneSel, (p, m) => {
  const menu = p.querySelector(m) || document.querySelector(m);
  const a = p.getBoundingClientRect(); const r = menu.getBoundingClientRect();
  return { inside: r.left >= a.left - 1 && r.right <= a.right + 1 && r.top >= a.top - 1 && r.bottom <= a.bottom + 1, r: [r.left, r.top, r.right, r.bottom].map(Math.round), a: [a.left, a.top, a.right, a.bottom].map(Math.round) };
}, menuSel);

const WIDTHS = [['narrow', 600], ['normal', 1100], ['wide', 1600]];
const paneWidth = (page) => page.$eval(PANE, (p) => p.getBoundingClientRect().width);

for (const [label, width] of WIDTHS) {
  await check(`TB01-03 watch ${label} (${width}px): every feature reachable, no overflow, staged folding`, async () => {
    const { ctx, page, errors } = await openWatch({ width });
    const w = await paneWidth(page);
    if (label === 'narrow') assert.ok(w <= 520, `좁음 패인 ${w}`);
    else if (label === 'normal') assert.ok(w > 520 && w < 760, `보통 패인 ${w}`);
    else assert.ok(w >= 760, `넓음 패인 ${w}`);
    await shot(page, `watch-${label}`);
    // TB01
    assert.deepEqual(await unreachable(page, WATCH_FEATURES), [], '닿지 않는 기능');
    // TB02
    await wakeWatch(page);
    assert.deepEqual(await layoutProblems(page, PANE, '.ml-pane-top, .ml-pane-bar', '.ml-pane-text'), [], '막대 배치');
    await toggleMore(page);
    const m = await menuInside(page, PANE, '.ml-more-pane');
    assert.ok(m.inside, `⋯ 메뉴가 패인 밖 ${m.r} / ${m.a}`);
    await shot(page, `watch-${label}-menu`);
    await toggleMore(page);
    // TB03
    const st = await page.$eval(PANE, (p) => {
      const vis = (el) => Boolean(el) && el.getClientRects().length > 0 && getComputedStyle(el).display !== 'none';
      const top = p.querySelector('.ml-pane-top');
      return {
        groups: [...top.querySelectorAll('.ml-wt-group')].map(vis), followLabel: vis(top.querySelector('.ml-wt-follow .ml-wt-label')),
        badge: vis(p.querySelector('.ml-q-badge')), drag: vis(top.querySelector('.ml-drag')), topSelect: vis(top.querySelector('[data-ml="select"]')),
        barChatmode: vis(p.querySelector('.ml-pane-bar [data-mw="chatmode"]')),
      };
    });
    assert.equal(st.drag, false, '단독 보기에는 끌기 손잡이가 없다');
    assert.equal(st.topSelect, false, '단독 보기 위 막대에 "이 패인 채팅 보기" 없음');
    assert.equal(st.barChatmode, false, '채팅 표시 방식은 아래 막대에서 위 막대로 옮겼다');
    if (label === 'narrow') {
      assert.deepEqual(st.groups, [false, false], '좁음: 묶음 접힘');
      assert.equal(st.badge, false, '좁음: 배지 숨김(⚙ 머리에 같은 값)');
      await toggleMore(page);
      const narrow = await visibleSels(page, PANE, ['.ml-more-pane [data-mw="follow"]', '.ml-more-pane [data-mw="fav"]', '.ml-more-pane [data-mw="copy"]', '.ml-more-pane [data-mw="chatmode"]', '.ml-more-pane [data-mw="popout"]']);
      assert.equal(narrow.length, 5, `좁음: ⋯ 에 접힌 항목 ${narrow.join(',')}`);
      await toggleMore(page);
    } else {
      assert.deepEqual(st.groups, [true, true], '묶음 보임');
      assert.equal(st.badge, true, '화질·지연 배지 보임');
      assert.equal(st.followLabel, label === 'wide', '팔로우 글자는 넓음에서만');
      await toggleMore(page);
      const dup = await visibleSels(page, PANE, ['.ml-more-pane [data-mw="follow"]', '.ml-more-pane [data-mw="fav"]', '.ml-more-pane [data-mw="copy"]', '.ml-more-pane [data-mw="chatmode"]', '.ml-more-pane [data-mw="popout"]']);
      assert.deepEqual(dup, [], '위 막대에 있는 동작은 ⋯ 에 겹쳐 두지 않는다');
      await toggleMore(page);
    }
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('TB04 top bar actions call existing commands', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600 });
  const follow = await page.$eval(`${PANE} .ml-wt-follow`, (b) => ({ label: b.getAttribute('aria-label'), text: b.textContent.trim() }));
  assert.deepEqual(follow, { label: '팔로잉 — 한동숙 팔로우 끊기', text: '팔로잉' }, '팔로우 목록에 있으면 팔로잉(보이는 글자가 이름 앞)');
  const ipc = () => page.evaluate(() => window.__ipc.splice(0));
  await ipc();
  await page.click(`${PANE} .ml-wt-follow`);
  await page.waitForTimeout(200);
  let calls = await ipc();
  assert.deepEqual(calls.find((c) => c.cmd === 'channel_follow')?.args, { channelId: 'ch1', follow: false }, '팔로우 끊기');
  assert.equal(await page.$eval(`${PANE} .ml-wt-follow`, (b) => b.textContent.trim()), '팔로우');
  await wakeWatch(page);
  await page.click(`${PANE} .ml-pane-top [data-mw="fav"]`);
  await page.waitForTimeout(200);
  calls = await ipc();
  const fav = calls.find((c) => c.cmd === 'favorite_set');
  assert.equal(fav?.args.channelId, 'ch1'); assert.equal(fav?.args.favorite, true);
  assert.equal(await page.$eval(`${PANE} .ml-pane-top [data-mw="fav"]`, (b) => b.getAttribute('aria-pressed')), 'true');
  await wakeWatch(page);
  await page.click(`${PANE} .ml-pane-top [data-mw="copy"]`);
  assert.deepEqual(await page.evaluate(() => window.__copied), ['https://chzzk.naver.com/live/ch1']);
  await wakeWatch(page);
  await page.click(`${PANE} .ml-pane-top [data-mw="chatmode"]`);
  await page.waitForTimeout(150);
  calls = await ipc();
  assert.ok(calls.some((c) => c.cmd === 'settings_update' && c.args?.patch?.displayMode === 'overlay'), '채팅 표시 방식 저장');
  await toggleMore(page);
  await page.click(`${PANE} .ml-more-pane [data-mw="external"]`);
  calls = await ipc();
  assert.deepEqual(calls.find((c) => c.cmd === 'open_external')?.args, { url: 'https://chzzk.naver.com/live/ch1' });
  await wakeWatch(page);
  await toggleMore(page);
  await page.click(`${PANE} .ml-more-pane [data-mw="multi"]`);
  await page.waitForFunction(() => !document.querySelector('.page-multilive.as-watch') && !document.querySelector('.page-multilive').hidden);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TB05 quality/latency badge shows values, warns when behind, opens the quality menu', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600 });
  const b = await page.$eval(`${PANE} .ml-q-badge`, (x) => ({ text: x.textContent, behind: x.classList.contains('behind'), label: x.getAttribute('aria-label') }));
  assert.equal(b.text, '1080p2.1초');
  assert.equal(b.behind, false);
  assert.match(b.label, /화질 1080p \(자동\) · 지연 2\.1초/);
  await feedMetrics(page, PANE, 12.4);
  assert.equal(await page.$eval(`${PANE} .ml-q-badge`, (x) => x.classList.contains('behind')), true, '늦으면 경고');
  await wakeWatch(page);
  await page.focus(`${PANE} .ml-q-badge`);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(150);
  const open1 = await page.$eval(PANE, (p) => ({ menu: !p.querySelector('.ml-gear-menu').hidden, exp: p.querySelector('.ml-q-badge').getAttribute('aria-expanded'), levels: p.querySelectorAll('.ml-gear-menu [data-mp="level"]').length }));
  assert.equal(open1.menu, true, '⚙ 화질 메뉴가 열린다');
  assert.equal(open1.exp, 'true');
  assert.ok(open1.levels >= 1, '화질 항목');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('ml-q-badge')), true, 'Esc 면 배지로 초점');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TB06 overflow menu keyboard and shortcut hints (custom binding, Windows labels)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600 });
  await page.focus(`${PANE} [data-mp="more"]`);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(150);
  const first = await page.evaluate(() => document.activeElement?.getAttribute('data-ml') || document.activeElement?.getAttribute('data-mw'));
  assert.equal(first, 'info', '첫 항목(채널 정보)으로 초점');
  await page.keyboard.press('ArrowDown');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.mw), 'external', '↓ 는 보이는 다음 항목(접힌 항목 건너뜀)');
  await page.keyboard.press('End');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.mw), 'fullchat', 'End 는 마지막');
  const keys = await page.$$eval(`${PANE} .ml-more-pane kbd.ml-menu-key`, (ks) => Object.fromEntries(ks.filter((k) => k.getClientRects().length).map((k) => [k.dataset.keyOf, k.textContent])));
  assert.deepEqual(keys, { bookmark: 'B', screenshot: '⌘S', pip: 'P', fullchat: '⌃⌘F' });
  await page.keyboard.press('Escape');
  await page.waitForTimeout(150);
  const after = await page.$eval(PANE, (p) => ({ hidden: p.querySelector('.ml-more-pane').hidden, focus: document.activeElement === p.querySelector('[data-mp="more"]') }));
  assert.deepEqual(after, { hidden: true, focus: true }, 'Esc: 닫고 ⋯ 로');
  // 사용자가 바꾼 키 — 채팅 표시 방식을 V 로
  await page.evaluate(async () => { const m = await import('/js/shortcuts.js'); m.applyKeyboardSettings({ bindings: ['toggleChat', { key: 'v', modifiers: 0 }] }); });
  assert.match(await page.$eval(`${PANE} .ml-pane-top [data-mw="chatmode"]`, (b) => b.title), /\(V\)$/);
  assert.deepEqual(errors, []);
  await ctx.close();
  const win = await openWatch({ width: 600, userAgent: WINDOWS_UA });
  await toggleMore(win.page);
  const wk = await win.page.$$eval(`${PANE} .ml-more-pane kbd.ml-menu-key`, (ks) => Object.fromEntries(ks.filter((k) => k.getClientRects().length).map((k) => [k.dataset.keyOf, k.textContent])));
  assert.equal(wk.screenshot, 'Ctrl+S'); assert.equal(wk.popout, 'Ctrl+N'); assert.equal(wk.fullchat, 'Ctrl+Alt+Shift+F');
  await win.ctx.close();
});

for (const theme of ['dark', 'light']) {
  await check(`TB07 axe: watch top bar and open overflow menu (${theme})`, async () => {
    const { ctx, page } = await openWatch({ width: 1600, theme });
    await page.evaluate(AXE);
    const run = () => page.evaluate(async (sel) => {
      const r = await window.axe.run({ include: [[sel]] }, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
      return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
    }, PANE);
    assert.deepEqual(await run(), [], '닫힌 상태');
    await toggleMore(page);
    assert.deepEqual(await run(), [], '⋯ 메뉴 열림');
    await ctx.close();
  });
}

await check('TB08 logged out: follow hidden, the rest stays', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, loggedOut: true });
  const vis = await visibleSels(page, PANE, ['.ml-pane-top [data-mw="follow"]', '.ml-pane-top [data-mw="fav"]', '.ml-pane-top [data-mw="copy"]']);
  assert.deepEqual(vis, ['.ml-pane-top [data-mw="fav"]', '.ml-pane-top [data-mw="copy"]']);
  await toggleMore(page);
  assert.deepEqual(await visibleSels(page, PANE, ['.ml-more-pane [data-mw="follow"]']), []);
  assert.deepEqual(await layoutProblems(page, PANE, '.ml-pane-top', '.ml-pane-text'), []);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TB09 multilive grid panes keep their old top bar and menu', async () => {
  const { ctx, page, errors } = await open({ width: 1280 });
  await go(page, 'multilive');
  await page.waitForSelector('.ml-pane[data-slot="0"]');
  await page.waitForTimeout(400);
  const sel = '.ml-pane[data-slot="0"]';
  await page.$eval(sel, (p) => p.classList.add('ctl-visible'));
  const top = await visibleSels(page, sel, ['.ml-wt-group', '.ml-q-badge', '.ml-pane-top [data-ml="select"]', '.ml-pane-top [data-ml="info"]', '.ml-pane-top [data-ml="retry"]', '.ml-drag']);
  assert.deepEqual(top, ['.ml-pane-top [data-ml="select"]', '.ml-pane-top [data-ml="info"]', '.ml-pane-top [data-ml="retry"]', '.ml-drag'], '그리드 위 막대');
  await toggleMore(page, sel);
  const items = await page.$$eval(`${sel} .ml-more-pane > *`, (els) => els.filter((e) => e.getClientRects().length).map((e) => e.textContent.trim()));
  assert.deepEqual(items, ['라이브로 크게 보기', '이 패인 채팅 보기', '채널 정보', '다시 연결'], '그리드 ⋯ 메뉴는 예전 그대로');
  assert.deepEqual(errors, []);
  await ctx.close();
});


// ── 라이브 머리줄(채널 칩 줄) ──
const chipWindowFor = (w) => { for (const [max, n] of [[600, 2], [900, 3], [1180, 4], [1500, 5], [1850, 6], [2200, 7]]) if (w <= max) return n; return 8; };
const headerState = (page) => page.evaluate(() => {
  const h = document.querySelector('.ml-header');
  const seg = h.querySelector('.live-mode-seg').getBoundingClientRect();
  const p = h.querySelector('.ml-watch-picker'); const pr = p.getBoundingClientRect();
  const add = h.querySelector('.ml-toolbar .ml-add').getBoundingClientRect();
  const tabs = [...p.querySelectorAll('.ml-watch-tab')];
  const shown = tabs.filter((t) => !t.hidden);
  const more = (k) => { const m = p.querySelector(`.ml-watch-more.${k}`); return m.hidden ? 0 : Number(m.textContent.slice(1)); };
  const active = p.querySelector('.ml-watch-tab.active').getBoundingClientRect();
  return {
    headerW: h.clientWidth, overflow: h.scrollWidth - h.clientWidth, segToPicker: Math.round(pr.left - seg.right), pickerToAdd: Math.round(add.left - pr.right),
    addInside: add.right <= h.getBoundingClientRect().right + 0.5, total: tabs.length, shown: shown.length, start: more('start'), end: more('end'),
    activeInside: active.left >= pr.left - 0.5 && active.right <= pr.right + 0.5,
    clipped: shown.map((t) => { const e = t.querySelector('.ellipsis'); return e.scrollWidth - e.clientWidth > 1 ? e.textContent : null; }).filter(Boolean),
  };
});
const ZOOMS = [['narrow', 1280, 2], ['narrow', 1280, 1.5], ['normal', 1280, 1], ['normal', 1600, 1.25], ['wide', 1280, 0.8], ['wide', 2000, 1]];
for (const [label, win, zoom] of ZOOMS) {
  await check(`TB11 live header ${label} (window ${win}px at ${Math.round(zoom * 100)}%): grouped, no overflow, every channel reachable`, async () => {
    const width = Math.round(win / zoom);
    const { ctx, page, errors } = await openWatch({ width, channels: 6, theme: 'light' });
    await page.mouse.move(5, 400);
    const st = await headerState(page);
    assert.equal(st.overflow <= 1, true, `머리줄 넘침 ${st.overflow}px`);
    assert.ok(st.addInside, '채널 추가가 머리줄 안');
    assert.ok(st.segToPicker >= 0 && st.segToPicker <= 28, `전환 → 칩 줄 ${st.segToPicker}px`);
    assert.ok(st.pickerToAdd >= 0 && st.pickerToAdd <= 12, `칩 줄 → 채널 추가 ${st.pickerToAdd}px`);
    assert.equal(st.shown, Math.min(6, chipWindowFor(st.headerW)), `머리줄 ${st.headerW}px 에 칩 ${st.shown}개`);
    assert.equal(st.start + st.end + st.shown, st.total, '+N 이 가려진 수와 같다');
    assert.ok(st.activeInside, '보는 칩이 칩 줄 안');
    if (label === 'wide' && win === 2000) assert.deepEqual(st.clipped, [], '넓은 창: 이름이 잘리지 않는다');
    // 가려진 채널은 "+N" 메뉴에서 닿는다
    const reach = new Set(await page.$$eval('.ml-watch-tab:not([hidden])', (ts) => ts.map((t) => t.dataset.sessionId)));
    for (const k of ['start', 'end']) {
      if (!(await page.$(`.ml-watch-more.${k}:not([hidden])`))) continue;
      await page.click(`.ml-watch-more.${k}`);
      await page.waitForTimeout(120);
      const m = await menuInside(page, 'body', '.ml-watch-overflow');
      assert.ok(m.inside, `+N 메뉴가 창 밖 ${m.r}`);
      for (const id of await page.$$eval('.ml-watch-overflow [data-ml="watch"]', (bs) => bs.map((b) => b.dataset.sessionId))) reach.add(id);
      await page.keyboard.press('Escape');
      await page.waitForTimeout(80);
    }
    assert.equal(reach.size, 6, `닿는 채널 ${[...reach].join(',')}`);
    await shot(page, `header-${label}-${win}-${Math.round(zoom * 100)}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('TB12 chip keyboard: arrows/Home/End move (shifting the window), Delete closes, no seek leak', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6 });
  const focused = () => page.evaluate(() => (document.activeElement?.classList.contains('ml-watch-chip') ? document.activeElement.dataset.sessionId : document.activeElement?.className));
  await page.focus('.ml-watch-chip.active');
  assert.equal(await focused(), 's0');
  await page.evaluate(() => window.__ipc.splice(0));
  await page.keyboard.press('ArrowRight');
  assert.equal(await focused(), 's1', '→ 옆 칩');
  await page.keyboard.press('End');
  assert.equal(await focused(), 's5', 'End 마지막 칩');
  assert.equal(await page.$eval('.ml-watch-tab[data-session-id="s5"]', (t) => t.hidden), false, '가려진 마지막 칩이 보이게 창을 옮긴다');
  await page.keyboard.press('Home');
  assert.equal(await focused(), 's0', 'Home 첫 칩');
  await page.keyboard.press('ArrowLeft');
  assert.equal(await focused(), 's0', '처음에서 ← 는 그대로');
  assert.equal(await page.evaluate(() => document.querySelector('.player-osd:not([hidden])')?.textContent || ''), '', '되감기 OSD 가 뜨지 않는다');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Delete');
  await page.waitForTimeout(150);
  const calls = await page.evaluate(() => window.__ipc.splice(0));
  assert.deepEqual(calls.filter((c) => c.cmd === 'multilive_remove').map((c) => c.args.sessionId), ['s1'], 'Delete → 그 채널 닫기');
  assert.ok(!calls.some((c) => c.cmd === 'multilive_watch'), '닫기는 고르기가 아니다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TB13 "+N" overflow menu: names and states, fits the window, Enter watches, Esc returns focus, axe', async () => {
  const { ctx, page, errors } = await openWatch({ width: 640, channels: 6, theme: 'light' });
  await page.focus('.ml-watch-more.end');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(150);
  const m = await page.evaluate(() => {
    const o = document.querySelector('.ml-watch-overflow');
    return { hidden: o.hidden, label: o.getAttribute('aria-label'), expanded: document.querySelector('.ml-watch-more.end').getAttribute('aria-expanded'),
      items: [...o.querySelectorAll('[data-ml="watch"]')].map((b) => b.textContent.replace(/\s+/g, ' ').trim()), focus: document.activeElement?.closest('.ml-watch-overflow') ? document.activeElement.dataset.sessionId : null };
  });
  assert.equal(m.hidden, false); assert.equal(m.expanded, 'true'); assert.equal(m.label, '뒤쪽에 가려진 채널');
  assert.ok(m.items.some((t) => /똘똘똘이.*방송 종료/.test(t)) && m.items.some((t) => /귀염1.*연결 중/.test(t)), `상태 글자 ${m.items.join(' / ')}`);
  assert.ok(m.focus, '키보드로 열면 첫 항목에 초점');
  const inside = await menuInside(page, 'body', '.ml-watch-overflow');
  assert.ok(inside.inside, `창 밖 ${inside.r}`);
  await page.evaluate(AXE);
  const bad = await page.evaluate(async () => {
    const r = await window.axe.run({ include: [['.ml-header']] }, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
    return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
  });
  assert.deepEqual(bad, []);
  await shot(page, 'header-overflow-menu');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(120);
  assert.deepEqual(await page.evaluate(() => [document.querySelector('.ml-watch-overflow').hidden, document.activeElement?.classList.contains('ml-watch-more'), Boolean(document.querySelector('.page-multilive.as-watch'))]), [true, true, true], 'Esc: 메뉴만 닫고 +N 로');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(150);
  await page.evaluate(() => window.__ipc.splice(0));
  const first = await page.evaluate(() => document.activeElement?.dataset.sessionId);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(200);
  const calls = await page.evaluate(() => window.__ipc.splice(0));
  assert.deepEqual(calls.filter((c) => c.cmd === 'multilive_watch').map((c) => c.args.sessionId), [first], 'Enter → 그 채널을 라이브로');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TB14 chip states: offline/loading dots and labels', async () => {
  const { ctx, page, errors } = await openWatch({ width: 2000, channels: 6 });
  const st = await page.$$eval('.ml-watch-tab', (ts) => ts.map((t) => ({ id: t.dataset.sessionId, cls: [...t.classList].filter((c) => c.startsWith('st-')).join(),
    label: t.querySelector('.ml-watch-chip').getAttribute('aria-label') || t.querySelector('.ml-watch-chip').title,
    dot: getComputedStyle(t.querySelector('.ml-watch-st')).display })));
  const by = Object.fromEntries(st.map((x) => [x.id, x]));
  assert.equal(by.s3.cls, 'st-offline'); assert.match(by.s3.label, /\(방송 종료\)/); assert.equal(by.s3.dot, 'block');
  assert.equal(by.s4.cls, 'st-loading'); assert.match(by.s4.label, /\(연결 중\)/); assert.equal(by.s4.dot, 'block');
  assert.equal(by.s1.cls, ''); assert.equal(by.s1.dot, 'none', '재생 중이면 점이 없다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── 예전 시청 화면(#viewer) — 라이브 칸이 가득 차 이 화면으로 연다 ──
const VSTAGE = '#viewer .player-stage';
async function openViewer(opts = {}) {
  const o = await open({ ...opts, full: true });
  const { page } = o;
  await go(page, 'multilive'); // 멀티라이브 상태(라이브 칸 가득 참)를 먼저 읽는다
  await page.waitForSelector('.ml-pane[data-slot="0"]');
  await page.waitForTimeout(300);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: 'ch9' } })));
  await page.waitForSelector('#viewer:not([hidden]) .stage-top .viewer-title .title:not(:empty)');
  await page.waitForFunction(() => { const f = document.getElementById('player-frame'); return f && f.contentDocument?.readyState === 'complete' && f.contentDocument.getElementById('video'); });
  await page.evaluate(() => {
    document.querySelectorAll('.cv-ad, .preroll, [class*="preroll"]').forEach((x) => x.remove());
    const f = document.getElementById('player-frame');
    try { const v = f.contentDocument.getElementById('video'); Object.defineProperty(v, 'videoHeight', { get: () => 1080, configurable: true }); } catch { /* 무시 */ }
    window.__cviewPlayerBridge({ type: 'metrics', data: { latency: 1.84, currentLevelHeight: 1080 } }, f.contentWindow);
    const stage = document.querySelector('#viewer .player-stage');
    let fv = stage.querySelector('.fake-video');
    if (!fv) { fv = document.createElement('div'); fv.className = 'fake-video'; stage.querySelector('iframe').after(fv); }
    Object.assign(fv.style, { position: 'absolute', inset: '0', background: 'linear-gradient(135deg, hsl(160 50% 30%), hsl(230 50% 20%))', pointerEvents: 'none' });
  });
  await page.waitForTimeout(500);
  const r = await page.$eval(VSTAGE, (s) => { const b = s.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + 40 }; });
  await page.mouse.move(r.x, r.y);
  await page.mouse.move(r.x + 2, r.y);
  await page.waitForTimeout(260);
  return o;
}
const VIEWER_FEATURES = [
  ['채널 정보', '[data-vt="channel"]'], ['팔로우', '[data-vt="follow"]'], ['즐겨찾기', '[data-vt="fav"]'], ['링크 복사', '[data-vt="copy"]'],
  ['치지직에서 열기', '[data-vt="external"]'], ['멀티라이브에 추가', '[data-vt="multi"]'], ['북마크', '#ctl-bookmark'], ['스크린샷', '#ctl-shot'],
  ['다시 연결', '#reload'], ['채팅 표시 방식', '#ctl-chatmode, [data-vt="chatmode"]'], ['새 창으로 분리', '[data-vt="popout"]'], ['PiP', '[data-vt="pip"]'],
  ['영상+채팅 전체 화면', '#ctl-fullscreen-chat'], ['전체 화면', '#fullscreen'], ['닫기', '#viewer-close'], ['녹화', '#ctl-record'],
  ['플레이어 설정(재생 속도·화면·EQ)', '#ctl-panel'], ['화질', '#ctl-quality'], ['재생/일시정지', '#ctl-play'], ['볼륨', '#mute'],
];

for (const [label, width] of [['narrow', 900], ['normal', 1280], ['wide', 1700]]) {
  await check(`TB10 viewer ${label} (${width}px): grouped top bar, menu, folding, no overflow`, async () => {
    const { ctx, page, errors } = await openViewer({ width });
    const w = await page.$eval(VSTAGE, (s) => s.getBoundingClientRect().width);
    if (label === 'narrow') assert.ok(w <= 640, `좁음 무대 ${w}`);
    else if (label === 'normal') assert.ok(w > 640 && w < 860, `보통 무대 ${w}`);
    else assert.ok(w >= 860, `넓음 무대 ${w}`);
    await shot(page, `viewer-${label}`);
    const seen = new Set(await visibleSels(page, VSTAGE, VIEWER_FEATURES.map((f) => f[1])));
    await page.click('#viewer-more');
    await page.waitForTimeout(150);
    const m = await menuInside(page, VSTAGE, '.vt-menu');
    assert.ok(m.inside, `⋯ 메뉴가 무대 밖 ${m.r} / ${m.a}`);
    for (const s of await visibleSels(page, VSTAGE, VIEWER_FEATURES.map((f) => f[1]))) seen.add(s);
    await shot(page, `viewer-${label}-menu`);
    assert.deepEqual(VIEWER_FEATURES.filter((f) => !seen.has(f[1])).map((f) => f[0]), [], '닿지 않는 기능');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(150);
    assert.equal(await page.$eval('.vt-menu', (x) => x.hidden), true);
    assert.equal(await page.$eval('#viewer-more', (x) => x.getAttribute('aria-expanded')), 'false');
    const st = await page.$eval('#viewer .stage-top', (top) => ({
      groups: [...top.querySelectorAll('.vt-group')].map((g) => g.getClientRects().length > 0 && getComputedStyle(g).display !== 'none'),
      chatInView: Boolean(top.querySelector('.vt-view #ctl-chatmode')),
      label: (() => { const l = top.querySelector('.vt-follow .vt-label'); return Boolean(l) && l.getClientRects().length > 0; })(),
      // 지표 줄(두 줄까지 보이고 넘치면 숨김) 안에서 실제로 보이는 배지 글자
      badge: (() => {
        const b = top.querySelector('.vt-q-badge'); const box = top.querySelector('.meta-stats');
        if (!b || !box || !b.getClientRects().length) return null;
        const r = b.getBoundingClientRect(); const c = box.getBoundingClientRect();
        return r.bottom <= c.bottom + 0.5 ? b.textContent : null;
      })(),
    }));
    assert.equal(st.chatInView, true, '채팅 표시 방식은 보기 묶음 안');
    if (label === 'narrow') {
      assert.deepEqual(st.groups, [false, false]);
      assert.equal(st.badge, null);
    } else {
      assert.deepEqual(st.groups, [true, true]);
      assert.equal(st.label, label === 'wide', '팔로우 글자는 넓음에서만');
      if (label === 'wide') assert.equal(st.badge, '1080p1.8초');
    }
    assert.deepEqual(await layoutProblems(page, VSTAGE, '.stage-top, .player-controls', '.viewer-title'), [], '막대 배치');
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('TB10 viewer: keyboard, actions and axe', async () => {
  const { ctx, page, errors } = await openViewer({ width: 1700 });
  await page.focus('#viewer-more');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.vt), 'channel');
  await page.evaluate(AXE);
  const bad = await page.evaluate(async () => {
    const r = await window.axe.run({ include: [['#viewer .player-stage']] }, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
    return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
  });
  assert.deepEqual(bad, []);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(120);
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'viewer-more');
  await page.evaluate(() => window.__ipc.splice(0));
  await page.click('#viewer .vt-follow');
  await page.waitForTimeout(150);
  const calls = await page.evaluate(() => window.__ipc.splice(0));
  assert.deepEqual(calls.find((c) => c.cmd === 'channel_follow')?.args, { channelId: 'ch9', follow: true }, '목록에 없는 채널 → 팔로우');
  // 배지 → 화질 메뉴, Esc 면 배지로
  await page.focus('#viewer .vt-q-badge');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(120);
  assert.equal(await page.$eval('#ctl-quality-menu', (m) => m.hidden), false);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(120);
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('vt-q-badge')), true);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let fail = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  if (!r.ok) fail++;
}
console.log(`\n${results.length - fail}/${results.length} passed`);
process.exit(fail ? 1 : 0);
