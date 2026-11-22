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
// TB09k 그리드 패인 키보드 — 막대가 숨어 있어도 Tab 이 막대 단추에 닿고, 닿으면 막대가 깨어난다.
// TB10 예전 시청 화면(#viewer) — 같은 짜임(채널·보기 묶음, ⋯ 메뉴 항목·단축키), 폭 단계, 배지, Esc, axe.
// TB11 라이브 머리줄(채널 칩 줄) — 좁음·보통·넓음 × 화면 크기 80~200%(웹뷰 확대 = CSS 폭이 줄어듦): [전환] │ [칩] [채널 추가] 가
//      붙어 있고, 머리줄이 넘치지 않으며, 보이는 칩 수가 머리줄 폭을 따르고(chipWindowFor), 모든 채널이 칩 또는 "+N" 메뉴로 닿는다.
//      넓은 창에서는 6개가 다 보이고 이름이 잘리지 않는다.
// TB12 칩 키보드 — ←/→ 옆 칩, End/Home 끝(가려진 칩이면 창을 옮김), Delete 는 그 채널 닫기(초점은 옆 칩). 시청 단축키(←/→ 되감기)로 새지 않는다.
// TB13 "+N" 넘침 메뉴 — 가려진 채널을 이름·상태와 함께, 창 안(fitPopover)에, Enter 로 그 채널을 라이브로, Esc 면 "+N" 로 초점. axe.
// TB14 칩 상태 — 방송 종료·연결 중은 아바타 점(st-*)과 이름(aria-label)으로 알린다.
// TB15 오른쪽 배치(2026-11-20 시안 B) — 제목·전환은 왼쪽, [칩 줄][채널 추가]는 붙어서 오른쪽 끝(멀티라이브 보기의 채널 추가와 같은 자리),
//      사이는 창 끌기 빈칸. 탭 순서가 보이는 순서(전환 → 칩 → 추가)와 같다. 좁은 폭에서도 넘치지 않는다.
// TB16 미끄러지는 판 — 지금 보는 칩 밑에 판(.ml-chip-glider)이 꼭 맞고, 다른 칩을 고르면 따라간다(모션이면 translate·width 만 움직여
//      끝에는 다시 꼭 맞음). 보는 칩이 창 밖으로 가려지면 판도 숨는다.
// TB17 칩 추가·닫기 레이아웃 모션(FLIP) — 새 칩은 opacity·transform 으로 피어나고, 남은 칩은 transform 으로 새 자리로 미끄러진다.
//      모션 줄이기면 칩 줄에 아무 애니메이션도 없다.
// TB18 마이크로 인터랙션 — × 는 숨을 때 작게·돌아 있다가 칩을 가리키면 제 크기(아바타는 줄며 사라짐), 칩 누름 .95, 가리키면 아바타 1.08,
//      "+N" 가리킴·펼침 판. 전환 속성은 합성 단계(transform·scale·rotate·opacity)와 색뿐 — 크기·위치 전환이 없다.
// R15~R22 PR #139 적대적 리뷰 결함 회귀 — R15 칩 줄이 넘치는 좁은 머리줄에서 상태 글자만 바뀌어도 판이 고른 칩에 맞음,
//   R16 오른쪽 끝 칩 줄에서 칩을 닫는 동안 왼쪽 칩·판이 칩 줄 밖(잘리는 곳)으로 나가지 않음, R17 "+N" 메뉴로 가려진 채널을 고르면
//   창 이동으로 새로 보인 기존 칩은 피어나지 않음(진짜 새 채널만), R18 피어나는 중에 다시 그려도 피어남이 이어지고 옆으로 튀지 않음,
//   R19 판이 미끄러지는 도중 제자리로 돌아오면 옛 목표로 갔다 튀지 않음, R20 × 누름 .88, R21 "+N" 초점 링이 칩 줄에 잘리지 않음,
//   (재검토) R23 칩을 더해 칩 줄이 넘치게 될 때 왼쪽 잘림·끝 프레임 튐 없음, R24 휠 넘김 직후 다시 그려도 들어오던 칩이 피어나지 않음,
//   R25 숨었던 판이 다른 칩을 고르며 다시 나타날 때 옅게 나타남, R24 는 옅게 들어오던 칩의 투명도 연속성도,
//   R27 넘치지 않는 칩 줄에 칩을 더하면 폭이 움직이고 판은 한 번만 미끄러져(도중 취소 없음) 맞음,
//   R28 칩 줄 폭이 움직이는 중 상태 글자가 붙어도(넘치지 않음·넘치게 됨·보는 칩이 오른쪽 끝) 칩·줄 가장자리가 튀거나
//       왼쪽으로 잘리지 않음(넘치지 않으면 폭 움직임을 새로 건다), R23 은 끝 무렵 튐(프레임당 ≤6px), R26 판이 미끄러지는 중 상태 글자가 붙어도 이어서 감, R18 은 미끄러짐 연속성,
//   R22 고르지 않은 칩의 상태 점 둘레에 칩 줄 바탕과 다른 고리가 없음(아바타를 도려냄).
// R01~R12 PR #138 적대적 리뷰 결함 회귀 — R01 Delete 자동 반복 무시, R02 마우스로 × 닫기(클릭 초점 흉내)는 초점을 옆 칩으로 옮기지 않음,
//   R03 상태만 바뀌면 칩 초점·열린 "+N" 메뉴 유지(메뉴 상태 글자도 갱신), R04 마우스로 연 "+N" 을 앱 Esc 로 닫아도 aria-expanded 풀림,
//   R05 "+N" 메뉴가 열린 채 휠로 넘기면 닫힘, R06 단독 보기 위 막대 아바타가 보이고 누르면 채널 정보, R07 채팅 '영상 위'+좁은 무대에서 ⋯ 가
//   패인 안, R08 팔로우를 바꾼 뒤 다른 채널을 갔다 와도 바뀐 상태(서버 캐시보다 앞섬), R09 카드 메뉴로 바꾼 즐겨찾기가 상단 바 별에 반영,
//   R10 상단 바 즐겨찾기 뒤 사이드바·홈 목록 다시 읽기, R11 로그아웃·로그인하면 팔로우 단추가 숨고 다시 나타남(단독 보기·#viewer),
//   R12 #viewer 경과 시간 다시 그리기(30초)에도 화질 배지 키보드 초점 유지.
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
  if (process.env.CVIEW_ONLY && !id.startsWith(process.env.CVIEW_ONLY)) return; // 하나만 돌리기: CVIEW_ONLY=R07
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
    multilive_remove: ({ sessionId }) => { state.sessions = state.sessions.filter((x) => x.id !== sessionId); state.rev++; return JSON.parse(JSON.stringify(state)); },
    following_channels_all: () => (opts.loggedOut ? Promise.reject(new Error('로그인이 필요합니다'))
      : new Promise((ok) => setTimeout(() => ok([{ channelId: 'ch1', channelName: '한동숙' }]), opts.followDelay || 0))), // followDelay: 캐시가 비어 늦게 오는 팔로우 상태(R14)
    favorites_list: () => [],
  };
  // rail: 예전 시청 화면 머리줄의 팔로잉 라이브 레일(viewer-extras loadRail) 항목 수.
  if (opts.rail) over.following_lives = () => Array.from({ length: opts.rail }, (_, i) => ({ ...live, channelId: `f${i}`, channelName: `팔로잉 ${i}`, channelImageUrl: `${location.origin}/__img/avatar/${(i % 6) + 1}.svg` }));
  if (opts.loggedOut) over.auth_status = () => ({ loggedIn: false, state: 'loggedOut' });
  // Rust 이벤트 흉내 — 멀티라이브 상태(multilive://state)·로그인(auth://changed). __emitState(fn) 는 상태를 고친 뒤 보낸다.
  const listeners = {};
  window.__TAURI__.event.listen = async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; };
  window.__emit = (name, payload) => { for (const cb of listeners[name] || []) cb({ payload }); };
  window.__emitState = (fn) => { if (fn) fn(state); state.rev++; window.__emit('multilive://state', JSON.parse(JSON.stringify(state))); };
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (cmd !== 'ui_heartbeat') window.__ipc.push({ cmd, args: args ? JSON.parse(JSON.stringify(args)) : null });
    if (over[cmd]) return over[cmd](args || {});
    try { return await orig(cmd, args); } catch (e) { if (/^(multilive_|chat_|player_|window_|bookmark_|save_)/.test(cmd)) return null; throw e; }
  };
}

const browser = await engine.launch();
async function open({ width = 1280, height = 780, theme = 'dark', userAgent, clock = false, motion = false, ...opts } = {}) {
  // motion: 모션을 켠 채로(TB16·TB17 — 칩 줄 애니메이션). 기본은 모션 줄이기(전환이 끝 상태로 바로).
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: motion ? 'no-preference' : 'reduce', ...(userAgent ? { userAgent } : {}) });
  const page = await ctx.newPage();
  if (clock) await page.clock.install(); // 시계를 앞당겨 30초 타이머를 돌린다(R12) — 시간은 그대로 흐른다
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
  // 단추는 숨은 막대에서도 visibility:visible 이다(키보드 Tab 용) — 막대 자체가 보이는지를 본다. 모션 줄이기의 전역 0.01ms 전환 탓에
  // 클래스를 붙인 직후엔 아직 hidden 이고 한두 프레임 뒤 보인다. 끝내 보이지 않으면 시간 초과로 실패한다.
  await page.waitForFunction((s) => getComputedStyle(document.querySelector(`${s} .ml-pane-top`)).visibility === 'visible', sel, { timeout: 3000 });
  const top = await visibleSels(page, sel, ['.ml-wt-group', '.ml-q-badge', '.ml-pane-top button[data-ml="select"]', '.ml-pane-top button[data-ml="info"]', '.ml-pane-top button[data-ml="retry"]', '.ml-drag']);
  assert.deepEqual(top, ['.ml-pane-top button[data-ml="select"]', '.ml-pane-top button[data-ml="info"]', '.ml-pane-top button[data-ml="retry"]', '.ml-drag'], '그리드 위 막대');
  await toggleMore(page, sel);
  const items = await page.$$eval(`${sel} .ml-more-pane > *`, (els) => els.filter((e) => e.getClientRects().length).map((e) => e.textContent.trim()));
  assert.deepEqual(items, ['라이브로 크게 보기', '이 패인 채팅 보기', '채널 정보', '다시 연결'], '그리드 ⋯ 메뉴는 예전 그대로');
  assert.deepEqual(errors, []);
  await ctx.close();
});

// TB09k 숨은 그리드 패인 막대에도 키보드 Tab 이 닿고, 닿으면 막대가 깨어난다.
// (v0.2.9~0.2.12: 숨은 막대의 visibility:hidden 이 단추를 Tab 순서에서 빼서, 마우스를 올리지 않으면 키보드로는 막대·⋯ 메뉴에 닿지 못했다.)
await check('TB09k keyboard Tab reaches a hidden grid pane bar and wakes it', async () => {
  const { ctx, page, errors } = await open({ width: 1280 });
  await go(page, 'multilive');
  await page.waitForSelector('.ml-pane[data-slot="0"]');
  await page.mouse.move(4, 700);
  await page.waitForTimeout(600);
  const sel = '.ml-pane[data-slot="0"]';
  assert.equal(await page.$eval(sel, (p) => p.classList.contains('ctl-visible')), false, '처음엔 막대가 숨어 있다');
  // 문서 순서로 패인 바로 앞의 초점 요소에서 시작한다.
  const start = await page.$eval(sel, (pane) => {
    const prev = [...document.querySelectorAll('button, [tabindex], a[href], input, select, iframe')].filter((e) => (e.compareDocumentPosition(pane) & Node.DOCUMENT_POSITION_FOLLOWING) && !pane.contains(e)
      && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden' && !e.disabled && e.tabIndex >= 0 && !e.closest('[hidden],[inert]')).pop();
    prev?.focus();
    return Boolean(prev) && document.activeElement === prev;
  });
  assert.ok(start, '시작 초점');
  const seq = [];
  let hit = null;
  for (let i = 0; i < 6 && !hit; i++) {
    await page.keyboard.press('Tab');
    const st = await page.$eval(sel, (pane) => {
      const a = document.activeElement;
      const inBar = pane.contains(a) && Boolean(a.closest('.ml-pane-top, .ml-pane-bar'));
      return { inBar, id: `${a?.tagName.toLowerCase()}${a?.dataset?.ml ? `[ml=${a.dataset.ml}]` : a?.dataset?.mp ? `[mp=${a.dataset.mp}]` : ''}`, awake: pane.classList.contains('ctl-visible') };
    });
    seq.push(st.id);
    if (st.inBar) hit = st;
  }
  assert.ok(hit, `Tab 이 slot0 막대 단추에 닿는다 (순서 ${JSON.stringify(seq)})`);
  assert.ok(hit.awake, '단추에 닿으면 막대가 깨어난다');
  await page.waitForFunction((s) => getComputedStyle(document.querySelector(`${s} .ml-pane-top`)).visibility === 'visible', sel, { timeout: 3000 });
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
  const innerRight = h.getBoundingClientRect().right - parseFloat(getComputedStyle(h).paddingRight);
  return {
    headerW: h.clientWidth, overflow: h.scrollWidth - h.clientWidth, segToPicker: Math.round(pr.left - seg.right), pickerToAdd: Math.round(add.left - pr.right),
    addToEnd: Math.round(innerRight - add.right),
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
    assert.ok(st.segToPicker >= 8, `전환 → 칩 줄 사이 빈칸 ${st.segToPicker}px`);
    assert.ok(st.pickerToAdd >= 0 && st.pickerToAdd <= 12, `칩 줄 → 채널 추가 ${st.pickerToAdd}px`);
    assert.ok(st.addToEnd >= 8 && st.addToEnd <= 12, `채널 추가가 오른쪽 끝 ${st.addToEnd}px`);
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


// ── PR #138 리뷰 결함 회귀(R01~R12) ──
const ipcOf = (page, cmd) => page.evaluate((c) => window.__ipc.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const clearIpc = (page) => page.evaluate(() => { window.__ipc.length = 0; });

await check('R01 holding Delete on a chip closes only one channel (auto-repeat ignored)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 6 });
  await page.focus('.ml-watch-chip[data-session-id="s2"]');
  await clearIpc(page);
  const prevented = await page.$eval('.ml-watch-chip[data-session-id="s2"]', (c) => !c.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', repeat: true, bubbles: true, cancelable: true })));
  await page.waitForTimeout(150);
  assert.equal(prevented, true, '반복 입력도 기본 동작은 막는다');
  assert.deepEqual(await ipcOf(page, 'multilive_remove'), [], '반복 입력으로는 닫지 않는다');
  await page.keyboard.press('Delete');
  await page.waitForTimeout(200);
  for (let i = 0; i < 4; i++) await page.$eval(':focus', (c) => c.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', repeat: true, bubbles: true, cancelable: true }))).catch(() => {});
  await page.waitForTimeout(200);
  assert.deepEqual((await ipcOf(page, 'multilive_remove')).map((a) => a.sessionId), ['s2'], '처음 한 번만 닫는다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R02 mouse close on × (click focuses it, as in Chromium/WebView2) does not move focus to a chip', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 6 });
  await page.$eval('.ml-watch-close[data-session-id="s2"]', (x) => { x.focus(); x.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 })); });
  await page.waitForFunction(() => !document.querySelector('.ml-watch-tab[data-session-id="s2"]'));
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('.ml-watch-picker'))), false, '마우스로 닫으면 초점이 칩 줄에 남지 않는다');
  // 키보드(Enter, detail 0)로 닫으면 옆 칩으로
  await page.focus('.ml-watch-close[data-session-id="s3"]');
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => !document.querySelector('.ml-watch-tab[data-session-id="s3"]'));
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('ml-watch-chip') && document.activeElement.dataset.sessionId), 's4', '키보드로 닫으면 옆 칩');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R03 state-only updates keep chip focus and an open "+N" menu (menu state text refreshes)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6 });
  await page.focus('.ml-watch-chip[data-session-id="s1"]');
  await page.evaluate(() => window.__emitState((st) => { st.sessions[2].state = { kind: 'loading' }; st.sessions[1].state = { kind: 'error' }; }));
  await page.waitForTimeout(250);
  const a = await page.evaluate(() => ({ focus: document.activeElement?.dataset.sessionId, s2: document.querySelector('.ml-watch-tab[data-session-id="s2"]').className, s1: document.querySelector('.ml-watch-chip[data-session-id="s1"]').getAttribute('aria-label') }));
  assert.equal(a.focus, 's1', '칩 초점 유지');
  assert.match(a.s2, /st-loading/); assert.match(a.s1, /\(재생 오류\)/);
  await page.click('.ml-watch-more.end');
  await page.waitForTimeout(120);
  const hiddenId = await page.$eval('.ml-watch-overflow [data-session-id]', (b) => b.dataset.sessionId);
  await page.evaluate((id) => window.__emitState((st) => { st.sessions.find((x) => x.id === id).state = { kind: 'error' }; }), hiddenId);
  await page.waitForTimeout(250);
  const m = await page.evaluate((id) => ({ hidden: document.querySelector('.ml-watch-overflow').hidden, text: document.querySelector(`.ml-watch-overflow [data-session-id="${id}"] .ml-wo-state`).textContent }), hiddenId);
  assert.deepEqual(m, { hidden: false, text: '재생 오류' }, '메뉴는 열린 채 상태 글자만 바뀐다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

/** 단독 보기 영상의 되감기 호출을 엿본다 — seekable 0~100초·현재 90초로 두고 iframe 의 seek·seekToLiveEdge 를 기록. */
const spySeek = (page) => page.$eval(`${PANE} iframe`, (f) => {
  const w = f.contentWindow;
  const v = f.contentDocument.getElementById('video');
  Object.defineProperty(v, 'seekable', { configurable: true, get: () => ({ length: 1, start: () => 0, end: () => 100 }) });
  Object.defineProperty(v, 'currentTime', { configurable: true, get: () => 90, set: () => {} });
  window.__seeks = [];
  w.seek = (t) => { window.__seeks.push(t); };
  w.seekToLiveEdge = () => { window.__seeks.push('live'); };
});

await check('R13 mouse-clicked chip (focused by Chromium/WebView2) leaves ←/→ seeking, Space playing and Delete harmless', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  // 1) 진짜 마우스 클릭으로 다른 채널로 바꾼다 — Chromium 은 누른 칩에 초점을 주고, 다시 그린 뒤에도 그 칩이 초점을 가졌었다.
  await page.click('.ml-watch-chip[data-session-id="s1"]');
  await page.waitForFunction(() => document.querySelector('.ml-watch-chip.active')?.dataset.sessionId === 's1');
  await page.waitForSelector(`${PANE} iframe`);
  await page.waitForFunction((sel) => { const f = document.querySelector(`${sel} iframe`); return f && f.contentDocument?.readyState === 'complete' && f.contentDocument.getElementById('video'); }, PANE);
  await page.waitForTimeout(500);
  assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('.ml-watch-picker'))), false, '마우스로 바꾼 뒤 초점이 칩 줄에 남지 않는다');
  // 2) 마우스를 누른 채(포인터 초점 — :focus-visible 아님) 칩에 초점이 있어도 칩 키보드는 끼어들지 않는다.
  const r = await page.$eval('.ml-watch-chip[data-session-id="s2"]', (c) => { const b = c.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; });
  await page.mouse.move(r.x, r.y);
  await page.mouse.down();
  // WebKit 은 단추에 마우스 초점을 주지 않는다 — Chromium 과 같게 포인터 초점을 흉내 낸다(키보드 모드가 아닌 focus()).
  await page.$eval('.ml-watch-chip[data-session-id="s2"]', (c) => { if (document.activeElement !== c) c.focus(); });
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.sessionId), 's2', '포인터 초점');
  await spySeek(page);
  await clearIpc(page);
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(120);
  const st = await page.evaluate(() => ({ seeks: window.__seeks.slice(), focus: document.activeElement?.dataset?.sessionId || null}));
  assert.deepEqual(st.seeks, [80], '← 는 10초 되감기');
  assert.equal(st.focus, 's2', '← 가 칩 이동으로 새지 않는다');
  await page.keyboard.press('Delete');
  // Backspace 는 합성 이벤트로 — Playwright WebKit 은 진짜 Backspace 를 '뒤로 가기'로 처리해 페이지를 떠난다(앱의 WKWebView·WebView2 는 아니다).
  await page.evaluate(() => document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true })));
  await page.waitForTimeout(200);
  assert.deepEqual(await ipcOf(page, 'multilive_remove'), [], 'Delete·Backspace 로 채널을 닫지 않는다');
  await page.mouse.up();
  await page.waitForTimeout(200);
  // 3) 키보드(Tab)로 온 칩은 그대로 칩 키보드 — ← 는 옆 칩, 되감기 없음.
  await page.evaluate(() => { document.activeElement?.blur?.(); window.__seeks.length = 0; });
  await page.focus('.ml-watch-close[data-session-id="s2"]').catch(() => {});
  await page.keyboard.press('Shift+Tab');
  const kb = await page.evaluate(() => ({ id: document.activeElement?.dataset?.sessionId, chip: document.activeElement?.classList.contains('ml-watch-chip'), fv: document.activeElement?.matches(':focus-visible') }));
  assert.deepEqual(kb, { id: 's2', chip: true, fv: true }, 'Shift+Tab 으로 칩에 온다');
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(120);
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.sessionId), 's1', '키보드 초점이면 ← 는 칩 이동');
  assert.deepEqual(await page.evaluate(() => window.__seeks), [], '키보드 칩 이동은 되감지 않는다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R04 app-level Esc on a mouse-opened "+N" menu resets aria-expanded', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6 });
  await page.click('.ml-watch-more.end');
  await page.waitForTimeout(120);
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Escape');
  await page.waitForTimeout(150);
  assert.deepEqual(await page.evaluate(() => [document.querySelector('.ml-watch-overflow').hidden, document.querySelector('.ml-watch-more.end').getAttribute('aria-expanded'), Boolean(document.querySelector('.page-multilive.as-watch'))]), [true, 'false', true]);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R05 wheel over the chips closes the open "+N" menu', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6 });
  await page.click('.ml-watch-more.end');
  await page.waitForTimeout(120);
  const b = await page.locator('.ml-watch-picker').boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await page.mouse.wheel(0, 100);
  await page.waitForTimeout(150);
  assert.deepEqual(await page.evaluate(() => [document.querySelector('.ml-watch-overflow').hidden, document.querySelector('.ml-watch-more.end')?.getAttribute('aria-expanded') ?? 'false']), [true, 'false']);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R06 single-view top bar keeps the avatar; clicking it opens channel info', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  const av = await page.$eval(`${PANE} .ml-pane-avatar`, (a) => ({ shown: a.getClientRects().length > 0 && getComputedStyle(a).display !== 'none', img: Boolean(a.querySelector('img')) }));
  assert.deepEqual(av, { shown: true, img: true }, '아바타가 보인다');
  await page.evaluate(() => { window.__opened = null; window.addEventListener('cview:open-channel', (e) => { window.__opened = e.detail.channelId; }); });
  await wakeWatch(page);
  await page.click(`${PANE} .ml-pane-avatar`);
  assert.equal(await page.evaluate(() => window.__opened), 'ch1');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R07 overflow menu stays inside a narrow pane with chat "on video"', async () => {
  for (const width of [700, 650]) { // 무대 약 568px 이하(1280 창 200% 근처)
    const { ctx, page, errors } = await openWatch({ width });
    await page.$eval(`${PANE} .ml-pane-top [data-mw="chatmode"]`, (b) => b.click()); // 옆 → 영상 위
    await page.waitForFunction(() => document.querySelector('.ml-body.chat-overlay'));
    await page.waitForTimeout(250);
    await toggleMore(page);
    const m = await menuInside(page, PANE, '.ml-more-pane');
    assert.ok(m.inside, `${width}: ⋯ 메뉴가 패인 밖 ${m.r} / ${m.a}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  }
});

await check('R08 follow change survives switching channels (local state beats the server cache)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  await page.click(`${PANE} .ml-wt-follow`); // 팔로잉 → 팔로우 끊기
  await page.waitForTimeout(150);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's1' } })));
  await page.waitForTimeout(400);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
  await page.waitForTimeout(500);
  assert.equal(await page.$eval(`${PANE} .ml-wt-follow`, (b) => b.textContent.trim()), '팔로우', '돌아와도 끊은 상태');
  await clearIpc(page);
  await wakeWatch(page);
  await page.click(`${PANE} .ml-wt-follow`);
  await page.waitForTimeout(150);
  assert.deepEqual(await ipcOf(page, 'channel_follow'), [{ channelId: 'ch1', follow: true }], '다시 누르면 반대 요청');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R09 favorite changed from the card menu reaches the top-bar star', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  assert.equal(await page.$eval(`${PANE} .ml-pane-top [data-mw="fav"]`, (b) => b.getAttribute('aria-pressed')), 'false');
  await page.evaluate(() => {
    const card = document.createElement('div');
    card.dataset.channelId = 'ch1'; card.dataset.channelName = '한동숙'; card.textContent = 'card';
    Object.assign(card.style, { position: 'fixed', left: '300px', top: '300px', width: '40px', height: '40px', zIndex: 99 });
    document.body.append(card);
    card.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
  });
  await page.waitForSelector('.ctx-menu [data-ctx="fav"]');
  await page.click('.ctx-menu [data-ctx="fav"]');
  await page.waitForTimeout(250);
  assert.equal(await page.$eval(`${PANE} .ml-pane-top [data-mw="fav"]`, (b) => b.getAttribute('aria-pressed')), 'true', '상단 바 별이 켜진다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R10 favorite from the top bar refreshes sidebar/home lists', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  await clearIpc(page);
  await wakeWatch(page);
  await page.click(`${PANE} .ml-pane-top [data-mw="fav"]`);
  await page.waitForTimeout(250);
  const cmds = await page.evaluate(() => window.__ipc.map((c) => c.cmd));
  const at = cmds.indexOf('favorite_set');
  assert.ok(at >= 0 && cmds.slice(at + 1).includes('following_lives') && cmds.slice(at + 1).includes('favorites_list'), `다시 읽기: ${cmds.join(',')}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R11 logout/login hides and restores the follow button (single view)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  const followShown = () => page.$eval(`${PANE} .ml-wt-follow`, (b) => !b.hidden);
  assert.equal(await followShown(), true);
  await page.evaluate(() => {
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => (c === 'following_channels_all' && window.__loggedOut ? Promise.reject(new Error('로그인이 필요합니다')) : inv(c, a));
    window.__loggedOut = true;
    window.__emit('auth://changed', { loggedIn: false, state: 'loggedOut' });
  });
  await page.waitForTimeout(300);
  assert.equal(await followShown(), false, '로그아웃하면 숨는다');
  await page.evaluate(() => { window.__loggedOut = false; window.__emit('auth://changed', { loggedIn: true, state: 'loggedIn', nickname: '도도' }); });
  await page.waitForTimeout(300);
  assert.equal(await followShown(), true, '로그인하면 다시 보인다');
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


await check('R11 logout/login hides and restores the follow button (#viewer)', async () => {
  const { ctx, page, errors } = await openViewer({ width: 1700 });
  const shown = () => page.$eval('#viewer .vt-follow', (b) => !b.hidden);
  assert.equal(await shown(), true);
  await page.evaluate(() => {
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => (c === 'following_channels_all' && window.__loggedOut ? Promise.reject(new Error('로그인이 필요합니다')) : inv(c, a));
    window.__loggedOut = true;
    window.__emit('auth://changed', { loggedIn: false, state: 'loggedOut' });
  });
  await page.waitForTimeout(300);
  assert.equal(await shown(), false, '로그아웃하면 숨는다');
  await page.evaluate(() => { window.__loggedOut = false; window.__emit('auth://changed', { loggedIn: true, state: 'loggedIn', nickname: '도도' }); });
  await page.waitForTimeout(300);
  assert.equal(await shown(), true, '로그인하면 다시 보인다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R12 #viewer quality badge keeps keyboard focus across the 30s meta redraw', async () => {
  const { ctx, page, errors } = await openViewer({ width: 1700, clock: true });
  await page.focus('#viewer .vt-q-badge');
  await page.clock.fastForward(31000);
  await page.waitForTimeout(150);
  const st = await page.evaluate(() => ({ focus: document.activeElement?.classList.contains('vt-q-badge'), attached: document.querySelector('#viewer .meta-stats .vt-q-badge') !== null }));
  assert.deepEqual(st, { focus: true, attached: true });
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R14 #viewer re-fits the channel rail when the follow button shows up late (× and ⋯ stay visible)', async () => {
  // 팔로잉 라이브 11개 레일, 팔로우 상태는 2.5초 늦게(캐시가 빈 첫 시청) — 그동안 레일은 팔로우 단추 없이 접혀 있다.
  const { ctx, page, errors } = await openViewer({ width: 1240, rail: 11, followDelay: 2500 });
  const fits = () => page.evaluate(() => {
    const top = document.querySelector('#viewer .stage-top');
    const t = top.getBoundingClientRect();
    const vis = (sel) => { const el = document.querySelector(sel); if (!el) return null; const b = el.getBoundingClientRect(); return b.width > 0 && b.right <= t.right + 0.5 && b.left >= t.left - 0.5; };
    return { follow: !document.querySelector('#viewer .vt-follow').hidden, close: vis('#viewer-close'), more: vis('#viewer-more'),
      rail: document.querySelectorAll('#viewer .viewer-rail .rail-item:not([hidden])').length, overflow: top.scrollWidth - top.clientWidth };
  });
  const before = await fits();
  assert.equal(before.follow, false, `아직 팔로우 상태 전 ${JSON.stringify(before)}`);
  assert.ok(before.rail > 0 && before.close && before.more, `시작: 레일이 보이고 ×·⋯ 가 무대 안 ${JSON.stringify(before)}`);
  await page.waitForFunction(() => !document.querySelector('#viewer .vt-follow').hidden, null, { timeout: 6000 });
  await page.waitForTimeout(250);
  const after = await fits();
  assert.deepEqual({ close: after.close, more: after.more }, { close: true, more: true }, `팔로우 단추가 늦게 나타나도 ×·⋯ 가 무대 안 ${JSON.stringify(after)}`);
  assert.ok(after.overflow <= 1, `위 막대가 넘치지 않는다 ${JSON.stringify(after)}`);
  assert.ok(after.rail < before.rail, `레일을 다시 접는다 ${before.rail} → ${after.rail}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── 오른쪽 배치·칩 줄 모션(2026-11-20) ──
const rectOf = (page, sel) => page.$eval(sel, (e) => { const r = e.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width }; });

for (const width of [2000, 1280, 760]) {
  await check(`TB15 right layout at ${width}px: switch left, chips + add packed at the right end, drag gap between, tab order follows`, async () => {
    const { ctx, page, errors } = await openWatch({ width, channels: 6, theme: 'light' });
    await page.mouse.move(5, 400);
    const st = await headerState(page);
    assert.equal(st.overflow <= 1, true, `머리줄 넘침 ${st.overflow}px`);
    assert.ok(st.pickerToAdd >= 0 && st.pickerToAdd <= 12, `칩 줄 → 채널 추가 ${st.pickerToAdd}px`);
    assert.ok(st.addToEnd >= 8 && st.addToEnd <= 12, `채널 추가 → 오른쪽 끝 ${st.addToEnd}px`);
    const lay = await page.evaluate(() => {
      const h = document.querySelector('.ml-header');
      const r = (e) => e.getBoundingClientRect();
      const title = r(h.querySelector('.page-title')); const seg = r(h.querySelector('.live-mode-seg'));
      const gap = h.querySelector('.ml-head-gap.after'); const g = r(gap); const p = r(h.querySelector('.ml-watch-picker'));
      const order = [h.querySelector('.live-mode-seg .seg.active'), h.querySelector('.ml-watch-tab:not([hidden]) .ml-watch-chip'), h.querySelector('.ml-toolbar .ml-add')];
      const domOrdered = order.every((e, i) => i === 0 || (order[i - 1].compareDocumentPosition(e) & Node.DOCUMENT_POSITION_FOLLOWING));
      const xs = order.map((e) => r(e).left);
      return { titleToSeg: seg.left - title.right, gapBetween: g.left >= seg.right - 0.5 && g.right <= p.left + 0.5, drag: gap.hasAttribute('data-tauri-drag-region'),
        gapW: g.width, domOrdered, visualOrdered: xs[0] < xs[1] && xs[1] < xs[2], sep: getComputedStyle(h.querySelector('.ml-head-gap.before')).display };
    });
    assert.ok(lay.titleToSeg >= 0 && lay.titleToSeg <= 16, `제목 → 전환 ${lay.titleToSeg}px (전환은 왼쪽에 그대로)`);
    assert.ok(lay.gapBetween && lay.drag, '전환과 칩 줄 사이 빈칸은 창 끌기 자리');
    if (width >= 1280) assert.ok(lay.gapW >= 40, `넓은 창: 빈칸 ${lay.gapW}px`);
    assert.equal(lay.sep, 'none', '예전 구분선은 없다');
    assert.ok(lay.domOrdered && lay.visualOrdered, '탭 순서 = 보이는 순서(전환 → 칩 → 채널 추가)');
    // 멀티라이브 보기로 바꿔도 채널 추가 단추는 같은 자리
    const addWatch = await rectOf(page, '.ml-toolbar .ml-add');
    await page.click('.page-multilive .live-mode-seg [data-live-mode="multilive"]');
    await page.waitForFunction(() => !document.querySelector('.page-multilive.as-watch') && document.querySelector('.page-multilive:not([hidden]) .ml-toolbar .ml-add')?.offsetParent);
    await page.waitForTimeout(250);
    const addMl = await rectOf(page, '.page-multilive .ml-toolbar .ml-add');
    assert.ok(Math.abs(addMl.right - addWatch.right) <= 1, `채널 추가 오른쪽 끝: 라이브 ${addWatch.right} / 멀티라이브 ${addMl.right}`);
    await shot(page, `right-layout-${width}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

/** 판과 지금 보는 칩의 어긋남(px) — 판이 숨었으면 null. */
const gliderFit = (page) => page.evaluate(() => {
  const g = document.querySelector('.ml-chip-glider');
  const t = document.querySelector('.ml-watch-tab.active:not([hidden])');
  if (!g || getComputedStyle(g).opacity === '0' || !t) return null;
  const a = g.getBoundingClientRect(); const b = t.getBoundingClientRect();
  return Math.max(Math.abs(a.left - b.left), Math.abs(a.right - b.right));
});
const settleAnims = (page) => page.waitForFunction(() => document.querySelector('.ml-watch-picker').getAnimations({ subtree: true }).every((a) => a.playState !== 'running'), null, { timeout: 3000 });
/** 칩 줄 안에서 돌고 있는 애니메이션 — 대상·움직이는 속성. */
const runningAnims = (page) => page.evaluate(() => document.querySelector('.ml-watch-picker').getAnimations({ subtree: true }).filter((a) => a.playState === 'running').map((a) => {
  const t = a.effect.target;
  const props = [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k).filter((x) => !['offset', 'easing', 'composite', 'computedOffset'].includes(x))))];
  return { el: t.classList.contains('ml-chip-glider') ? 'glider' : t.classList.contains('ml-watch-picker') ? 'picker' : t.classList.contains('ml-watch-tab') ? `tab:${t.dataset.sessionId}` : t.className, props, css: a instanceof CSSAnimation || a instanceof CSSTransition };
}));
const watchChip = async (page, id) => {
  await page.click(`.ml-watch-chip[data-session-id="${id}"]`);
  await page.waitForFunction((i) => document.querySelector('.ml-watch-tab.active')?.dataset.sessionId === i, id);
};

await check('TB16 sliding glider: fits the watched chip, follows a new pick (translate/width only), hides when the chip is scrolled away', async () => {
  for (const motion of [false, true]) {
    const { ctx, page, errors } = await openWatch({ width: 1600, channels: 6, motion });
    await page.mouse.move(5, 400);
    const fit0 = await gliderFit(page);
    assert.ok(fit0 != null && fit0 <= 1, `판이 지금 보는 칩에 맞다 ${fit0}`);
    const bg = await page.$eval('.ml-watch-chip.active', (c) => [c.closest('.ml-watch-picker').classList.contains('has-glider'), getComputedStyle(c).backgroundColor]);
    assert.deepEqual(bg, [true, 'rgba(0, 0, 0, 0)'], '판이 놓이면 고른 칩은 제 면이 없다');
    await watchChip(page, 's2');
    if (motion) {
      const anims = await runningAnims(page);
      const g = anims.filter((a) => a.el === 'glider');
      assert.ok(g.length >= 1, `판이 미끄러진다 ${JSON.stringify(anims)}`);
      for (const a of anims) assert.ok(a.css || a.props.every((x) => ({ glider: ['translate', 'width'], picker: ['width'] }[a.el] || ['transform', 'opacity', 'translate']).includes(x)), `합성 단계 속성만 ${JSON.stringify(a)}`);
      await settleAnims(page);
    }
    const fit1 = await gliderFit(page);
    assert.ok(fit1 != null && fit1 <= 1, `새로 고른 칩에 판이 맞다 ${fit1} (motion ${motion})`);
    // 휠로 창을 넘겨 보는 칩(s0)이 정말 가려지면 판도 숨고, 되돌려 다시 보이면 다시 맞는다 — 창을 줄여 칩 창을 작게(6개 중 ≤4)
    await page.setViewportSize({ width: 1000, height: 780 });
    await page.waitForFunction(() => document.querySelectorAll('.ml-watch-tab:not([hidden])').length <= 4);
    await watchChip(page, 's0');
    await settleAnims(page);
    const wheel = async (dy) => { const box = await rectOf(page, '.ml-watch-picker'); await page.mouse.move(box.left + 40, box.top + 10); await page.mouse.wheel(0, dy); await page.waitForTimeout(80); };
    for (let i = 0; i < 6 && !(await page.$eval('.ml-watch-tab.active', (t) => t.hidden)); i++) await wheel(120);
    await settleAnims(page); // 창이 옮겨지면 판도 미끄러진다 — 다 간 뒤에 잰다
    assert.equal(await page.$eval('.ml-watch-tab.active', (t) => t.hidden), true, '보는 칩이 창 밖으로 갔다');
    assert.equal(await gliderFit(page), null, '보는 칩이 가려지면 판도 숨는다');
    for (let i = 0; i < 6 && (await page.$eval('.ml-watch-tab.active', (t) => t.hidden)); i++) await wheel(-120);
    await settleAnims(page);
    const fit2 = await gliderFit(page);
    assert.ok(fit2 != null && fit2 <= 1, `다시 보이면 판이 칩에 맞다 ${fit2} (motion ${motion})`);
    assert.deepEqual(errors, []);
    await ctx.close();
  }
});

await check('TB17 chip add/close layout motion (FLIP): new chip blooms, others slide; nothing animates with reduced motion', async () => {
  for (const motion of [true, false]) {
    const { ctx, page, errors } = await openWatch({ width: 1900, channels: 3, motion });
    await page.mouse.move(5, 400);
    await page.evaluate(() => window.__emitState((st) => { const n = JSON.parse(JSON.stringify(st.sessions[1])); Object.assign(n, { id: 's9', slot: 3, channelId: 'ch9', channelName: '새 채널' }); st.sessions.push(n); }));
    await page.waitForSelector('.ml-watch-tab[data-session-id="s9"]:not([hidden])');
    const add = await runningAnims(page);
    if (motion) {
      const born = add.find((a) => a.el === 'tab:s9');
      assert.ok(born && born.props.includes('opacity') && born.props.includes('transform'), `새 칩이 피어난다 ${JSON.stringify(add)}`);
      assert.ok(add.some((a) => /^tab:s[01]$/.test(a.el) && a.props.join() === 'transform'), `남은 칩이 새 자리로 미끄러진다 ${JSON.stringify(add)}`);
      await settleAnims(page);
    } else assert.deepEqual(add.filter((a) => !a.css), [], '모션 줄이기: JS 애니메이션 없음');
    await page.evaluate(() => window.__emitState((st) => { st.sessions = st.sessions.filter((x) => x.id !== 's1'); }));
    await page.waitForFunction(() => !document.querySelector('.ml-watch-tab[data-session-id="s1"]'));
    const rm = await runningAnims(page);
    // 칩 줄은 오른쪽 끝에 붙어 있어 닫은 칩의 왼쪽 칩들이 오른쪽으로 미끄러진다(오른쪽 칩은 제자리)
    if (motion) assert.ok(rm.some((a) => a.el === 'tab:s0' && a.props.join() === 'transform'), `닫은 뒤 남은 칩이 미끄러진다 ${JSON.stringify(rm)}`);
    else assert.deepEqual(rm.filter((a) => !a.css), [], '모션 줄이기: JS 애니메이션 없음');
    await settleAnims(page);
    const fitEnd = await gliderFit(page);
    assert.ok(fitEnd != null && fitEnd <= 1, `끝나면 판이 보는 칩에 맞다 ${fitEnd}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  }
});

await check('TB18 micro-interactions: close morph, press scale, hover avatar lift, "+N" states, compositor-only transitions', async () => {
  // 모션 줄이기면 가리킴의 크기 변화가 없다(motion.css) — 아바타는 그대로
  {
    const { ctx, page } = await openWatch({ width: 1100, channels: 6 });
    await page.hover('.ml-watch-tab:not(.active):not([hidden]) .ml-watch-chip .ellipsis');
    await page.waitForTimeout(60);
    assert.equal(await page.$eval('.ml-watch-tab:not(.active):not([hidden]):hover .ml-watch-av', (e) => getComputedStyle(e).scale), 'none', '모션 줄이기: 아바타 크기 그대로');
    await ctx.close();
  }
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6, theme: 'light', motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(500);
  const sel = '.ml-watch-tab:not(.active):not([hidden])';
  const id = await page.$eval(sel, (t) => t.dataset.sessionId);
  const T = `.ml-watch-tab[data-session-id="${id}"]`;
  const look = () => page.$eval(T, (t) => {
    const c = getComputedStyle(t.querySelector('.ml-watch-close')); const av = getComputedStyle(t.querySelector('.ml-watch-av')); const img = getComputedStyle(t.querySelector('.ml-watch-chip img, .ml-watch-dot'));
    return { x: [c.opacity, c.scale, c.rotate], av: av.scale, img: [img.opacity, img.scale] };
  });
  assert.deepEqual(await look(), { x: ['0', '0.5', '-90deg'], av: 'none', img: ['0.78', 'none'] }, '쉴 때 × 는 작게·돌아 숨는다');
  await page.hover(`${T} .ml-watch-chip .ellipsis`);
  await page.waitForTimeout(500);
  const hov = await look();
  assert.deepEqual(hov.x, ['1', '1', '0deg'], '가리키면 × 가 제 크기');
  assert.equal(hov.av, '1.08', '가리키면 아바타 자리가 살짝 커진다');
  assert.deepEqual(hov.img, ['0', '0.5'], '아바타는 줄며 사라진다');
  // 누름 — 누른 채 밖으로 나가 떼면 클릭이 아니다
  const c = await rectOf(page, `${T} .ml-watch-chip`);
  await page.mouse.move(c.left + c.width / 2, c.top + 8);
  await page.mouse.down();
  await page.waitForTimeout(250);
  assert.equal(await page.$eval(`${T} .ml-watch-chip`, (b) => getComputedStyle(b).scale), '0.95', '누르면 .95');
  await page.mouse.move(5, 400);
  await page.mouse.up();
  // "+N" — 가리키면 떠 있는 판, 펼치면 브랜드색 고리
  const more = '.ml-watch-more.end:not([hidden])';
  const shadow = () => page.$eval(more, (b) => getComputedStyle(b).boxShadow);
  assert.equal(await shadow(), 'none');
  await page.hover(more);
  await page.waitForTimeout(300);
  assert.notEqual(await shadow(), 'none', '"+N" 가리킴 판');
  await page.click(more);
  await page.waitForSelector('.ml-watch-overflow:not([hidden])');
  await page.waitForTimeout(300);
  assert.match(await shadow(), /inset/, '펼친 "+N" 고리');
  await page.keyboard.press('Escape');
  // 전환 속성 — 크기·위치(레이아웃) 전환이 없다
  const props = await page.evaluate(() => ['.ml-watch-chip', '.ml-watch-close', '.ml-watch-more', '.ml-watch-av', '.ml-watch-chip img', '.ml-chip-glider']
    .map((q) => [q, getComputedStyle(document.querySelector(q)).transitionProperty]));
  for (const [q, p] of props) assert.ok(!/\b(all|width|height|left|top|right|bottom|margin|padding|max-width|transform-origin)\b/.test(p), `${q} 전환 속성 ${p}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── PR #139 적대적 리뷰 결함 회귀 ──
const insidePicker = (page) => page.evaluate(() => {
  const p = document.querySelector('.ml-watch-picker').getBoundingClientRect();
  const out = [];
  for (const e of document.querySelectorAll('.ml-watch-tab:not([hidden]), .ml-chip-glider')) {
    if (e.classList.contains('ml-chip-glider') && getComputedStyle(e).opacity === '0') continue;
    const r = e.getBoundingClientRect();
    if (r.left < p.left - 1 || r.right > p.right + 1) out.push(`${e.dataset.sessionId || 'glider'} ${Math.round(r.left)}..${Math.round(r.right)} / ${Math.round(p.left)}..${Math.round(p.right)}`);
  }
  return out;
});

/**
 * ms 동안 매 프레임 — 요소(칩은 채널 id, 판은 'glider')별 한 프레임 최대 이동(px), `late` ms 뒤 프레임들만의 최대 이동(lateJump),
 * 같은 프레임 s0 이동과의 최대 차(relJump), 칩이 칩 줄 왼쪽 밖(넘겨 본 만큼은 빼고)으로 나간 최대 거리, 칩 줄 폭 애니메이션을 본 적 있는지, 판과 보는 칩의 최대 어긋남(gliderOff).
 * 앞선 단언이 실패해도 매달린 evaluate 가 처리 안 된 거부로 실행을 끊지 않게 catch 를 달아 둔다(await 하면 그대로 거부된다).
 */
const trackChips = (page, ms, sel = '.ml-watch-tab:not([hidden])', late = Infinity) => {
  const run = page.evaluate(([ms, sel, late]) => new Promise((res) => {
    const prev = new Map(); const jump = {}; const lateJump = {}; const relJump = {}; let leftOut = 0; let pickerAnim = false; let gliderOff = 0; const t0 = performance.now();
    const pk = document.querySelector('.ml-watch-picker');
    const step = () => {
      const now = performance.now() - t0;
      const p = pk.getBoundingClientRect();
      pickerAnim ||= pk.getAnimations().some((a) => a.effect?.target === pk && a.playState === 'running');
      const delta = {};
      for (const e of document.querySelectorAll(sel)) {
        const id = e.dataset.sessionId || 'glider'; const x = e.getBoundingClientRect().left;
        if (prev.has(id)) {
          delta[id] = x - prev.get(id);
          const d = Math.abs(x - prev.get(id));
          jump[id] = Math.round(Math.max(jump[id] || 0, d));
          if (now > late) lateJump[id] = Math.round(Math.max(lateJump[id] || 0, d) * 10) / 10;
        }
        prev.set(id, x);
        // 넘겨 본 만큼(scrollLeft > 0)의 왼쪽은 넘겨 볼 수 있어 잘림이 아니다
        if (id !== 'glider') leftOut = Math.max(leftOut, Math.round(p.left - Math.max(0, pk.scrollLeft) - x));
      }
      // s0 과 같은 프레임 이동의 차 — 프레임이 밀려 모두 함께 크게 움직인 것과, 한 칩만 순간 이동한 것을 가른다
      if ('s0' in delta) for (const [id, d] of Object.entries(delta)) relJump[id] = Math.round(Math.max(relJump[id] || 0, Math.abs(d - delta.s0)));
      const g = document.querySelector('.ml-chip-glider'); const t = document.querySelector('.ml-watch-tab.active:not([hidden])');
      if (g && t && getComputedStyle(g).opacity !== '0') { const a = g.getBoundingClientRect(); const b = t.getBoundingClientRect(); gliderOff = Math.max(gliderOff, Math.round(Math.max(Math.abs(a.left - b.left), Math.abs(a.right - b.right)))); }
      if (now < ms) requestAnimationFrame(step); else res({ jump, lateJump, relJump, leftOut, pickerAnim, gliderOff });
    };
    step();
  }), [ms, sel, late]);
  run.catch(() => {});
  return run;
};
const addSession = (page, id, slot, name = `새 ${id}`) => page.evaluate(([i, sl, nm]) => window.__emitState((st) => { const n = JSON.parse(JSON.stringify(st.sessions[1])); Object.assign(n, { id: i, slot: sl, channelId: `c${i}`, channelName: nm }); st.sessions.push(n); }), [id, slot, name]);
const maxJump = (j) => Math.max(0, ...Object.values(j.jump));

await check('R15 narrow overflowing header: a state-only change (status text) still re-fits the glider', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1000, channels: 6 });
  await page.mouse.move(5, 400);
  // 칩 줄이 머리줄에 눌려 가로로 넘치는 상태 — 칩 줄 바깥 크기가 칩 폭과 상관없이 고정된다(실제로는 아주 좁은 머리줄·큰 글꼴)
  await page.addStyleTag({ content: '.page-multilive.as-watch .ml-watch-picker { max-width: 240px; }' });
  await page.waitForTimeout(150);
  const over = await page.$eval('.ml-watch-picker', (p) => p.scrollWidth - p.clientWidth);
  assert.ok(over > 0, `칩 줄이 넘친다 ${over}px`);
  for (const kind of ['loading', 'playing', 'offline']) {
    await page.evaluate((k) => window.__emitState((st) => { st.sessions[0].state = { kind: k }; }), kind);
    await page.waitForTimeout(120);
    const fit = await gliderFit(page);
    assert.ok(fit != null && fit <= 1, `${kind}: 판이 고른 칩에 맞다 ${fit}`);
  }
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R16 closing a chip in the right-aligned row: chips and glider stay inside the row while sliding', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 4, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  await page.evaluate(() => window.__emitState((st) => { st.sessions = st.sessions.filter((x) => x.id !== 's2'); }));
  await page.waitForFunction(() => !document.querySelector('.ml-watch-tab[data-session-id="s2"]'));
  const bad = [];
  for (let i = 0; i < 8; i++) { bad.push(...(await insidePicker(page))); await page.waitForTimeout(40); }
  assert.deepEqual(bad, [], '칩 줄 밖으로 나간 칩·판');
  await settleAnims(page);
  const fit = await gliderFit(page);
  assert.ok(fit != null && fit <= 1, `끝나면 판이 맞다 ${fit}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R17 picking a hidden channel from "+N": revealed existing chips do not bloom', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  await page.click('.ml-watch-more.end:not([hidden])');
  await page.waitForSelector('.ml-watch-overflow:not([hidden])');
  const id = await page.$eval('.ml-watch-overflow [data-ml="watch"]:last-child', (b) => b.dataset.sessionId);
  await page.click(`.ml-watch-overflow [data-session-id="${id}"]`);
  await page.waitForFunction((i) => document.querySelector('.ml-watch-tab.active')?.dataset.sessionId === i, id);
  const anims = await runningAnims(page);
  const bloom = anims.filter((a) => a.el.startsWith('tab:') && a.props.includes('opacity') && a.props.includes('transform'));
  assert.deepEqual(bloom, [], `기존 채널이 새 칩처럼 피어난다 ${JSON.stringify(anims)}`);
  await settleAnims(page);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R18 re-render while a new chip blooms: the bloom continues and it slides like the others (no one-frame jump)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1900, channels: 3, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  await addSession(page, 's9', 3);
  await page.waitForSelector('.ml-watch-tab[data-session-id="s9"]');
  // s9 를 피어나는 도중(150ms)으로 — 첫 프레임이 언제 시작됐는지에 따라 달라지지 않게
  await page.$eval('.ml-watch-tab[data-session-id="s9"]', (t) => { for (const a of t.getAnimations()) if (a.effect.getKeyframes().some((k) => k.opacity != null)) a.currentTime = 150; });
  const track = trackChips(page, 600); // s10 이 붙는 동안 — s9 도 다른 칩처럼 옛 자리에서 미끄러져야 한다(순간 이동 없이)
  await addSession(page, 's10', 4);
  await page.waitForSelector('.ml-watch-tab[data-session-id="s10"]');
  const of = (id) => page.$eval(`.ml-watch-tab[data-session-id="${id}"]`, (t) => t.getAnimations().filter((a) => a.playState === 'running' && a.effect.getKeyframes().some((k) => k.opacity != null)).map((a) => a.currentTime));
  const [s9, s10] = [await of('s9'), await of('s10')];
  const j = await track;
  assert.ok(s9.length === 1 && s10.length === 1 && s9[0] >= 140 && s9[0] - s10[0] >= 100, `s9 는 처음부터가 아니라 이어서 피어난다 ${JSON.stringify({ s9, s10 })}`);
  assert.ok((j.relJump.s9 ?? 0) < 40, `피어나던 s9 가 다른 칩과 달리 한 프레임에 튀지 않는다 ${JSON.stringify(j)}`);
  await settleAnims(page);
  const fit = await gliderFit(page);
  assert.ok(fit != null && fit <= 1, `판 ${fit}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R19 glider returning to its start mid-slide does not finish at the old target and jump', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 4, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  await page.click('.ml-watch-chip[data-session-id="s2"]');
  await page.waitForFunction(() => document.querySelector('.ml-watch-tab.active')?.dataset.sessionId === 's2');
  // 판을 출발점(s0)에 멈춰 두고 s0 으로 되돌아간다 — 옛 움직임(→ s2)이 남아 있으면 다시 돌릴 때 s2 로 간다
  await page.evaluate(() => { for (const a of document.querySelector('.ml-chip-glider').getAnimations()) { a.pause(); a.currentTime = 0; } });
  await page.click('.ml-watch-chip[data-session-id="s0"]');
  await page.waitForFunction(() => document.querySelector('.ml-watch-tab.active')?.dataset.sessionId === 's0');
  await page.evaluate(() => { for (const a of document.querySelector('.ml-chip-glider').getAnimations()) a.play(); });
  await page.waitForTimeout(250);
  const fit = await gliderFit(page);
  assert.ok(fit != null && fit <= 2, `되돌아온 판이 s0 에 있다 ${fit}`);
  await settleAnims(page);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R20 pressing a chip close (×) gives press feedback (.88)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 4, motion: true });
  await page.mouse.move(5, 400);
  const T = '.ml-watch-tab[data-session-id="s1"]';
  await page.hover(`${T} .ml-watch-chip .ellipsis`);
  await page.waitForTimeout(400);
  const x = await rectOf(page, `${T} .ml-watch-close`);
  await page.mouse.move(x.left + x.width / 2, x.top + 10);
  await page.mouse.down();
  await page.waitForTimeout(250);
  const sc = await page.$eval(`${T} .ml-watch-close`, (b) => getComputedStyle(b).scale);
  await page.mouse.move(5, 400);
  await page.mouse.up();
  assert.equal(sc, '0.88');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R21 keyboard focus ring on "+N" is not clipped by the chip row', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6 });
  await page.mouse.move(5, 400);
  const last = await page.$$eval('.ml-watch-tab:not([hidden]) .ml-watch-close', (xs) => xs.length);
  assert.ok(last > 0);
  await page.$$eval('.ml-watch-tab:not([hidden]) .ml-watch-close', (xs) => xs[xs.length - 1].focus());
  await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(() => document.activeElement.matches('.ml-watch-more.end:focus-visible')), true, '"+N" 에 키보드 초점');
  await page.waitForTimeout(350); // 고리가 바깥에서 조여 드는 등장(motion.css mo-focus-ring)이 끝난 뒤
  const ring = await page.$eval('.ml-watch-more.end', (b) => {
    const cs = getComputedStyle(b); const r = b.getBoundingClientRect(); const p = b.parentElement; const pr = p.getBoundingClientRect();
    const out = parseFloat(cs.outlineWidth) + parseFloat(cs.outlineOffset);
    const box = { top: pr.top + p.clientTop, bottom: pr.top + p.clientTop + p.clientHeight, left: pr.left + p.clientLeft, right: pr.left + p.clientLeft + p.clientWidth };
    return { clipTop: Math.max(0, box.top - (r.top - out)), clipBottom: Math.max(0, r.bottom + out - box.bottom), clipRight: Math.max(0, r.right + out - box.right), style: cs.outlineStyle };
  });
  assert.deepEqual(ring, { clipTop: 0, clipBottom: 0, clipRight: 0, style: 'solid' });
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R22 status dot on an unselected chip has no off-colour ring (avatar is cut out instead)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1900, channels: 6, theme: 'dark' });
  await page.mouse.move(5, 400);
  const st = await page.$eval('.ml-watch-tab.st-offline:not(.active)', (t) => ({
    ring: getComputedStyle(t.querySelector('.ml-watch-st')).boxShadow,
    mask: (() => { const cs = getComputedStyle(t.querySelector('img, .ml-watch-dot')); return cs.maskImage || cs.webkitMaskImage; })(),
  }));
  assert.equal(st.ring, 'none', '점 고리 없음');
  assert.match(st.mask, /radial-gradient/, '아바타에서 점 자리를 도려낸다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R23 adding a chip that makes the row overflow: no chip clipped on the left, no jump at the end, glider fits', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1900, channels: 3, motion: true });
  await page.mouse.move(5, 400);
  await page.addStyleTag({ content: '.page-multilive.as-watch .ml-watch-picker { max-width: 380px; }' });
  await page.waitForTimeout(400);
  const over = () => page.$eval('.ml-watch-picker', (p) => p.scrollWidth - p.clientWidth);
  assert.ok((await over()) <= 1, `처음엔 넘치지 않는다 ${await over()}`);
  // 칩 이동(340ms, 끝으로 갈수록 느려짐)이 거의 끝난 270ms 뒤에는 기존 칩이 한 프레임에 몇 px 도 움직이지 않아야 한다 —
  // 옛 결함은 폭 움직임이 끝나는 프레임에 붙임을 떼며 넘친 만큼(~30px) 튀었다
  const track = trackChips(page, 700, '.ml-watch-tab:not([hidden])', 270);
  await addSession(page, 's9', 3);
  await page.waitForSelector('.ml-watch-tab[data-session-id="s9"]').catch(() => {});
  const j = await track;
  assert.ok((await over()) > 1, '칩을 더하면 넘친다');
  const late = Math.max(0, ...['s0', 's1', 's2'].map((id) => j.lateJump[id] ?? 0));
  assert.ok(late <= 6, `끝 무렵 기존 칩이 튄다 ${late}px ${JSON.stringify(j)}`);
  assert.ok(maxJump(j) < 50, `한 프레임에 튄 칩 ${JSON.stringify(j)}`);
  assert.ok(j.leftOut <= 1, `칩 줄 왼쪽 밖으로 나간 칩 ${JSON.stringify(j)}`);
  await settleAnims(page);
  assert.equal(await page.$eval('.ml-watch-picker', (p) => p.classList.contains('ml-chips-anim')), false);
  const fit = await gliderFit(page);
  assert.ok(fit != null && fit <= 1, `끝나면 판이 보는 칩에 맞다 ${fit}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R24 re-render right after a wheel step: chips sliding in (CSS enter) do not restart as a bloom, nor pop to full opacity', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1100, channels: 6, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  const box = await rectOf(page, '.ml-watch-picker');
  await page.mouse.move(box.left + 40, box.top + 10);
  await page.mouse.wheel(0, 120);
  await page.waitForFunction(() => [...document.querySelectorAll('.ml-watch-tab:not([hidden])')].some((t) => Number(getComputedStyle(t).opacity) < 0.75), null, { timeout: 2000, polling: 'raf' });
  // 들어오는 중에 다른 칩을 고른다(다시 그리기) — 마우스 클릭은 Playwright 가 움직임이 멈추길 기다려 직접 부른다.
  // 다시 그린 첫 프레임에 — 옅던 칩이 1 로 튀지 않고 그 투명도에서 이어 가는지, 작게(scale) 피어나지 않는지
  const r = await page.evaluate(() => new Promise((res) => {
    const op = () => Object.fromEntries([...document.querySelectorAll('.ml-watch-tab:not([hidden])')].map((t) => [t.dataset.sessionId, Number(getComputedStyle(t).opacity)]));
    const before = op();
    const old = document.querySelector('.ml-watch-tab');
    document.querySelector('.ml-watch-tab:not([hidden]):not(.active) .ml-watch-chip').click();
    const t0 = performance.now();
    const step = () => {
      if (document.querySelector('.ml-watch-tab') !== old || performance.now() - t0 > 1500) {
        const scaled = [...document.querySelectorAll('.ml-watch-tab')].flatMap((t) => t.getAnimations()
          .filter((a) => a.playState === 'running' && a.effect.getKeyframes().some((k) => /scale/.test(k.transform || ''))).map(() => t.dataset.sessionId));
        res({ before, after: op(), scaled, rerendered: document.querySelector('.ml-watch-tab') !== old });
      } else requestAnimationFrame(step);
    };
    step();
  }));
  assert.ok(r.rerendered, '칩 줄을 다시 그렸다');
  assert.deepEqual(r.scaled, [], '기존 칩이 작게(scale .6)부터 피어난다');
  const faint = Object.keys(r.before).filter((id) => r.before[id] < 0.75 && id in r.after);
  assert.ok(faint.length > 0, `옅게 들어오던 칩이 있다 ${JSON.stringify(r)}`);
  for (const id of faint) assert.ok(r.after[id] < r.before[id] + 0.35, `${id} 투명도가 이어진다 ${r.before[id].toFixed(2)} → ${r.after[id].toFixed(2)}`);
  await settleAnims(page);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R25 glider fades in (not a pop) when it reappears — watched chip scrolled away, then another visible chip is picked', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1000, channels: 6, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForFunction(() => document.querySelectorAll('.ml-watch-tab:not([hidden])').length <= 4);
  await watchChip(page, 's0');
  await settleAnims(page);
  const wheel = async (dy) => { const box = await rectOf(page, '.ml-watch-picker'); await page.mouse.move(box.left + 40, box.top + 10); await page.mouse.wheel(0, dy); await page.waitForTimeout(80); };
  for (let i = 0; i < 6 && !(await page.$eval('.ml-watch-tab.active', (t) => t.hidden)); i++) await wheel(120);
  await settleAnims(page);
  await page.waitForTimeout(300);
  await page.mouse.move(5, 400);
  assert.equal(await page.$eval('.ml-chip-glider', (g) => getComputedStyle(g).opacity), '0', '보는 칩이 가려져 판이 숨었다');
  const samples = page.evaluate(() => new Promise((res) => {
    const out = []; const t0 = performance.now();
    const step = () => { out.push(Number(getComputedStyle(document.querySelector('.ml-chip-glider')).opacity)); if (performance.now() - t0 < 700) requestAnimationFrame(step); else res(out); };
    step();
  }));
  samples.catch(() => {});
  await page.evaluate(() => document.querySelector('.ml-watch-tab:not([hidden]):not(.active) .ml-watch-chip').click());
  const op = await samples;
  assert.equal(op.at(-1), 1, '다시 보인다');
  assert.ok(op.some((o) => o > 0.02 && o < 0.98), `옅게 나타난다(중간 값이 있다) ${op.map((o) => o.toFixed(2)).join(' ')}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R26 status text appearing while the glider slides: it continues from where it is (no cancel-and-jump)', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 4, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  const left = (q) => page.$eval(q, (e) => e.getBoundingClientRect().left);
  const s0 = await left('.ml-watch-tab[data-session-id="s0"]');
  // s2 를 고르고, 판의 미끄러짐이 시작된 그 프레임에 출발점(s0)에서 멈춘다 — 그 채 s2 에 상태 글자가 붙으면 끊고 s2 로
  // 순간 이동하면 안 되고 지금 자리에서 다시 미끄러져야 한다
  const paused = await page.evaluate(() => new Promise((res) => {
    const g = document.querySelector('.ml-chip-glider');
    document.querySelector('.ml-watch-chip[data-session-id="s2"]').click();
    const t0 = performance.now();
    const step = () => {
      const anims = g.getAnimations().filter((a) => !(a instanceof CSSTransition) && a.playState === 'running');
      if (anims.length) { for (const a of anims) { a.pause(); a.currentTime = 0; } res(anims.length); } else if (performance.now() - t0 > 2000) res(0); else requestAnimationFrame(step);
    };
    step();
  }));
  assert.ok(paused >= 1, `판의 미끄러짐을 출발점에 멈췄다 ${paused}`);
  assert.equal(await page.$eval('.ml-watch-tab.active', (t) => t.dataset.sessionId), 's2');
  const s2 = await left('.ml-watch-tab[data-session-id="s2"]');
  assert.ok(Math.abs(s2 - s0) > 60, `s0 과 s2 가 떨어져 있다 ${s0} ${s2}`);
  await page.evaluate(() => window.__emitState((st) => { st.sessions.find((x) => x.id === 's2').state = { kind: 'loading' }; }));
  await page.waitForFunction(() => document.querySelector('.ml-watch-tab.active .ml-watch-state')?.textContent);
  const g = await left('.ml-chip-glider');
  assert.ok(Math.abs(g - s0) < 20, `판이 지금 자리(s0 ${Math.round(s0)})에서 이어 간다 — 판 ${Math.round(g)}, s2 ${Math.round(s2)}`);
  await settleAnims(page);
  const fit = await gliderFit(page);
  // 상태 글자가 붙은 칩은 폭이 소수 px — 판은 offsetWidth(정수)로 놓아 1px 남짓 반올림 차이가 난다
  assert.ok(fit != null && fit <= 1.5, `상태 글자가 붙은 칩에 판이 맞다 ${fit}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('R27 adding a chip in a non-overflowing row: the row width animates, the glider follows once (not restarted) and fits', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1900, channels: 3, motion: true });
  await page.mouse.move(5, 400);
  await page.waitForTimeout(400);
  const w0 = (await rectOf(page, '.ml-watch-picker')).width;
  // 판의 움직임(애니메이션 객체)을 프레임마다 모아 cancel 이벤트를 센다 — 반올림 흔들림에 끊기면 420ms 전에 취소·교체된다
  const cancels = page.evaluate(() => new Promise((res) => {
    const g = document.querySelector('.ml-chip-glider'); const seen = new Set(); let cancelled = 0; const t0 = performance.now();
    const step = () => {
      for (const a of g.getAnimations()) if (!(a instanceof CSSTransition) && !seen.has(a)) { seen.add(a); a.addEventListener('cancel', () => { cancelled++; }); }
      if (performance.now() - t0 < 800) requestAnimationFrame(step); else res({ seen: seen.size, cancelled });
    };
    step();
  }));
  cancels.catch(() => {});
  const track = trackChips(page, 900, '.ml-watch-tab:not([hidden]), .ml-chip-glider');
  // 이름이 긴 채널 — 판이 충분히 멀리(> 71px) 미끄러져야 340ms 때 스프링이 목표를 1px 넘게 지나쳐 있다
  await addSession(page, 's9', 3, '이름이 꽤 긴 새 채널 하나');
  // 판 스프링(420ms)은 칩 줄 폭 움직임(340ms)이 끝날 때 아직 목표를 지나쳐 있다 — 그때 다시 시작하면 ~760ms 로 늘고 두 번 튄다
  await page.waitForTimeout(620);
  const gliding = await page.$eval('.ml-chip-glider', (g) => g.getAnimations().filter((a) => !(a instanceof CSSTransition) && a.playState === 'running').length);
  const j = await track;
  const c = await cancels;
  const p = await page.$eval('.ml-watch-picker', (e) => ({ w: e.getBoundingClientRect().width, over: e.scrollWidth - e.clientWidth, anim: e.classList.contains('ml-chips-anim') }));
  assert.ok(c.seen >= 1 && c.cancelled === 0, `판의 미끄러짐이 도중에 취소·교체되지 않는다 ${JSON.stringify(c)}`);
  assert.ok(p.over <= 1 && p.w - w0 > 110, `넘치지 않고 칩 줄이 넓어졌다 ${w0} → ${JSON.stringify(p)}`);
  assert.ok(j.pickerAnim, '칩 줄 폭이 움직였다');
  assert.equal(p.anim, false, '폭 움직임이 끝나면 오른쪽 붙임을 뗀다');
  assert.equal(gliding, 0, '판은 한 번만 미끄러진다(폭 움직임 끝에 다시 시작하지 않는다)');
  // 프레임이 밀리면 모두 함께 크게 움직인다 — 같은 프레임 s0 이동과의 차로 판의 순간 이동만 본다
  assert.ok((j.relJump.glider ?? 0) < 50, `판이 한 프레임에 튄다 ${JSON.stringify(j)}`);
  assert.ok(j.gliderOff < 40, `판이 보는 칩에서 크게 벗어나지 않는다 ${JSON.stringify(j)}`);
  const fit = await gliderFit(page);
  assert.ok(fit != null && fit <= 1, `끝나면 판이 보는 칩에 맞다 ${fit}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// [이름, 칩 줄 상한, 먼저 볼 채널, 새 채널을 앞에 넣기]
for (const [label, cap, watch, front] of [['fits', null, null, false], ['becomes overflowing', 440, null, false], ['becomes overflowing, watched chip at the right end', 440, 's2', true]]) {
  await check(`R28 status text arriving while the row width animates (${label}): chips and row edge continue, none clipped on the left`, async () => {
    const { ctx, page, errors } = await openWatch({ width: 1900, channels: 3, motion: true });
    await page.mouse.move(5, 400);
    if (cap) await page.addStyleTag({ content: `.page-multilive.as-watch .ml-watch-picker { max-width: ${cap}px; }` });
    if (watch) {
      await page.evaluate((id) => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: id } })), watch);
      await page.waitForFunction((id) => document.querySelector('.ml-watch-tab.active')?.dataset.sessionId === id, watch);
    }
    await page.waitForTimeout(500);
    const active = watch || 's0';
    await page.evaluate((f) => window.__emitState((st) => { const n = JSON.parse(JSON.stringify(st.sessions[1])); Object.assign(n, { id: 's9', slot: 3, channelId: 'cs9', channelName: '새 s9' }); if (f) st.sessions.unshift(n); else st.sessions.push(n); }), front);
    await page.waitForFunction(() => { const p = document.querySelector('.ml-watch-picker'); return p.getAnimations().some((a) => a.effect?.target === p && a.playState === 'running'); }, null, { polling: 'raf', timeout: 2000 });
    await page.waitForTimeout(100);
    // 보는 칩에 상태 글자 — 같은 짜임(제자리 갱신) 경로. 상태 알림은 동기로 처리돼 같은 작업 안에서 앞뒤 자리·폭을 잰다
    // (새 움직임은 첫 프레임 전이라 시작 값에 있다 — 프레임 밀림과 상관없는 순간 튐). 이어 첫 두 프레임도 — ResizeObserver 의
    // 드러내기 스크롤처럼 다음 프레임에 일어나는 튐.
    const r = await page.evaluate((id) => new Promise((res) => {
      const p = document.querySelector('.ml-watch-picker');
      const g = document.querySelector('.ml-chip-glider');
      const at = () => ({ w: p.getBoundingClientRect().width, sl: p.scrollLeft, gOff: g.getBoundingClientRect().left - document.querySelector('.ml-watch-tab.active').getBoundingClientRect().left, ...Object.fromEntries([...document.querySelectorAll('.ml-watch-tab:not([hidden])')].map((t) => [t.dataset.sessionId, t.getBoundingClientRect().left])) });
      const widthAnims = () => p.getAnimations().filter((a) => a.effect?.target === p && a.playState === 'running');
      const old = widthAnims();
      const frames = [at()];
      window.__emitState((st) => { st.sessions.find((x) => x.id === id).state = { kind: 'offline' }; });
      frames.push(at());
      const fresh = widthAnims().filter((a) => !old.includes(a)).length;
      // 칩마다 옆으로 옮기는 움직임(translateX·translate)은 하나만 — 옛 이동을 끊지 않고 두면 새 이동이 0 근처라 건너뛸 때
      // 옛 이동이 새 자리 위에 겹쳐 튄다(그 경우는 드물게만 나서 겹친 수로 잰다)
      const moves = Object.fromEntries([...document.querySelectorAll('.ml-watch-tab:not([hidden])')].map((t) => [t.dataset.sessionId, t.getAnimations()
        .filter((a) => !(a instanceof CSSAnimation) && a.playState === 'running' && a.effect.getKeyframes().some((k) => /translate/.test(k.transform || '') || (k.translate && k.translate !== 'none'))).length]));
      const stated = !!document.querySelector('.ml-watch-tab.active .ml-watch-state')?.textContent;
      requestAnimationFrame(() => { frames.push(at()); requestAnimationFrame(() => { frames.push(at()); res({ frames, fresh, stated, moves }); }); });
    }), active);
    // 그 뒤 — 왼쪽 잘림(앞에 더한 칩이 폭 움직임 동안 피어나며 잘리는 것은 칩 추가 쪽 일이라 상태 글자 뒤부터 잰다)
    const j = await trackChips(page, 600);
    const d = (a, b, k) => Math.abs(r.frames[b][k] - r.frames[a][k]);
    const ids = Object.keys(r.frames[0]).filter((k) => !['w', 'sl', 'gOff'].includes(k) && r.frames.every((f) => k in f));
    // 판도 보는 칩과의 어긋남이 그 순간 바뀌지 않는다 — 드러내기 스크롤 뒤에 판 자리를 재면 판만 스크롤만큼 튀었다
    assert.ok(d(0, 1, 'gOff') <= 3, `상태 글자가 붙는 순간 판이 보는 칩에서 튄다 ${d(0, 1, 'gOff').toFixed(1)}px ${JSON.stringify(r.frames.map((f) => f.gOff))}`);
    const sync = Math.max(0, ...ids.map((k) => d(0, 1, k)));
    assert.ok(r.stated, '상태 글자가 붙었다');
    assert.ok(Object.values(r.moves).every((n) => n <= 1), `옛 이동이 끊기지 않고 겹쳐 있다 ${JSON.stringify(r.moves)}`);
    assert.ok(sync <= 3, `상태 글자가 붙는 순간 칩이 튄다 ${sync.toFixed(1)}px ${JSON.stringify(r.frames)}`);
    // 보는 칩 드러내기 스크롤은 FLIP 이 재기 전에 끝나야 한다 — 다음 프레임(ResizeObserver)에 스크롤이 바뀌면 그만큼 모든 칩이
    // 튄다(옛 결함 ~30px). 칩 자리의 프레임 사이 이동은 WebKit 의 긴 프레임에 FLIP 진행이 섞여 기준으로 쓰지 않는다.
    // (넘치지 않는 경우는 폭이 움직이는 동안 오른쪽 붙임이라 WebKit 이 scrollLeft 를 음수로 보고하며 폭 따라 바뀐다 — 넘침 경우만)
    assert.ok(Math.max(d(1, 2, 'sl'), d(2, 3, 'sl')) <= 0.5, `상태 글자 뒤 다음 프레임에 칩 줄이 스크롤된다 ${JSON.stringify(r.frames.map((f) => f.sl))}`);
    // WebKit 은 칩 줄 폭과 칩 이동의 첫 프레임이 1~2px 어긋날 때가 있다 — 옛 결함은 상태 글자 폭(~45px)만큼 잘렸다
    assert.ok(j.leftOut <= 3, `칩 줄 왼쪽 밖으로 나간 칩 ${JSON.stringify(j)}`);
    if (!cap) {
      // 넘치지 않으면 칩 줄 폭도 지금 폭에서 새 폭으로 다시 움직인다 — 한 프레임에 새 폭으로 바뀌면 줄 가장자리가 튄다
      assert.ok(r.fresh >= 1, '상태 글자 뒤 칩 줄 폭 움직임을 새로 건다');
      const dw = d(0, 1, 'w');
      // WebKit 은 새 움직임을 한 프레임 진행한 값으로 시작할 때가 있다(~5px) — 다시 잡지 않으면 남은 폭(~70px)이 한 번에 바뀐다
      assert.ok(dw <= 12, `상태 글자가 붙는 순간 칩 줄 폭이 바뀐다 ${dw.toFixed(1)}px ${JSON.stringify(r.frames.map((f) => f.w))}`);
    }
    await settleAnims(page);
    const over = await page.$eval('.ml-watch-picker', (p) => p.scrollWidth - p.clientWidth);
    if (cap) assert.ok(over > 1, `상태 글자로 칩 줄이 넘친다 ${over}`);
    else assert.ok(over <= 1, `넘치지 않는다 ${over}`);
    const fit = await gliderFit(page);
    assert.ok(fit != null && fit <= 1.5, `끝나면 판이 맞다 ${fit}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

// CM01~CM03 채널 칩 우클릭 메뉴(2026-11-20) — 우클릭·메뉴 키로 열고, 고른 뒤 포인터 초점이 칩 키보드로 새지 않는다.
const chipMenuItems = (page) => page.$$eval('.ctx-menu .ctx-item', (b) => b.map((x) => x.dataset.ctx));
await check('CM01 chip right-click menu: channel actions, copy link, focus does not turn into chip keyboard focus', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  const box = await page.$eval('.ml-watch-tab[data-session-id="s1"] .ml-watch-chip', (c) => { const r = c.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; });
  await page.mouse.click(box.x, box.y, { button: 'right' });
  await page.waitForSelector('.ctx-menu');
  const items = await chipMenuItems(page);
  for (const k of ['watch', 'channel', 'chat', 'retry', 'fav', 'copy', 'open', 'close', 'close-others']) assert.ok(items.includes(k), `메뉴에 ${k} ${items}`);
  await page.waitForTimeout(500); // 가리킴 미리보기 지연(350ms)이 지나도 — 마우스는 칩 위에 그대로
  assert.equal(await page.$('#ml-peek:not([hidden])'), null, '메뉴가 열린 동안 칩 미리보기가 없다');
  await page.click('.ctx-menu [data-ctx="copy"]');
  await page.waitForTimeout(150);
  assert.ok((await page.evaluate(() => window.__copied)).some((t) => /ch2/.test(t)), '그 채널 링크를 복사');
  assert.equal(await page.evaluate(() => !!document.activeElement?.closest('.ml-watch-picker')), false, '고른 뒤 초점이 칩으로 돌아가지 않는다');
  await page.evaluate(() => window.__ipc.splice(0));
  await page.keyboard.press('Delete');
  await page.waitForTimeout(150);
  assert.ok(!(await page.evaluate(() => window.__ipc.splice(0))).some((c) => c.cmd === 'multilive_remove'), 'Delete 가 채널 닫기로 새지 않는다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CM02 chip menu "close others": keeps that channel on stage and closes the rest', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  await page.click('.ml-watch-tab[data-session-id="s2"] .ml-watch-chip', { button: 'right' });
  await page.waitForSelector('.ctx-menu');
  await page.evaluate(() => window.__ipc.splice(0));
  await page.click('.ctx-menu [data-ctx="close-others"]');
  await page.waitForFunction(() => window.__ipc.filter((c) => c.cmd === 'multilive_remove').length >= 2);
  const calls = (await page.evaluate(() => window.__ipc.splice(0))).filter((c) => /^multilive_(watch|remove)$/.test(c.cmd)).map((c) => `${c.cmd}:${c.args.sessionId}`);
  assert.deepEqual(calls, ['multilive_watch:s2', 'multilive_remove:s0', 'multilive_remove:s1'], `먼저 남길 채널로 바꾸고 나머지를 닫는다 ${calls}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CM03 chip menu by keyboard: Shift+F10 on a focused chip opens it (first item focused), Esc returns focus to the chip', async () => {
  const { ctx, page, errors } = await openWatch({ width: 1600, channels: 3 });
  await page.focus('.ml-watch-chip.active');
  await page.keyboard.press('Shift+F10');
  await page.waitForSelector('.ctx-menu');
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('ctx-item')), true, '첫 항목에 초점');
  assert.ok(!(await chipMenuItems(page)).includes('watch'), '보는 채널에는 "이 채널 보기"가 없다');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);
  assert.equal(await page.$('.ctx-menu'), null);
  assert.equal(await page.evaluate(() => document.activeElement?.closest('.ml-watch-tab')?.dataset.sessionId), 's0', 'Esc 면 칩으로 초점');
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
