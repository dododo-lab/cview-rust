// 홈 맨 위 "방송 중인 팔로잉"(live-following.js) 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·팔로우·채팅 없음).
// 예전 모양은 동그란 아바타만 한 줄로 늘어선 띠였다(39개면 오른쪽에서 잘려 가로 스크롤로만, 제목·시청자 수는 안 보임).
// LF01 개수별 배치 — 0·1·3·12·39·60개 × 좁은·보통·넓은 창. 접힌 상태는 칸 수에 맞는 줄 수(넓으면 두 줄)만, 넘치면 마지막 칸이
//      "+N 더 보기"(N = 숨은 수), 펼치면 모두 + "접기". 몇 개 안 될 때 타일이 늘어나지 않고, 페이지가 가로로 넘치지 않는다.
//      정렬 단추는 넷 이상일 때만. 순수 함수(collapseLimit·arrangeLiveFollowing)도 표로 확인한다.
// LF02 정렬 — 추천(즐겨찾기 → 최근 본 → 시청자순) · 시청자 · 방금 시작. 즐겨찾기·최근 기록이 없으면 추천 = 예전 순서(시청자순).
//      고른 정렬은 다시 열어도 남는다.
// LF03 키보드·이름 — 목록은 Tab 한 번(+ 단추 포함 두 번)으로 지나가고, ←→↑↓·Home·End 로 옮기며, 나갔다 오면 마지막 칸으로.
//      "+N 더 보기" Enter → 펼치고 처음 드러난 칸으로 초점. Enter → 그 채널을 연다. Shift+F10 → 카드 메뉴.
//      주 단추 이름은 채널 이름으로 시작해 제목·시청자 수를 담는다. 목록(role=list)은 제목으로 이름을 받는다. axe 위반 없음.
// LF04 미리보기 카드 — 호버(잠시 머묾)·키보드 초점에 썸네일·제목·단추 카드가 타일 아래에, 구역 안에 뜬다. 카드로 옮겨도 남고
//      나가면 닫힌다. 훑고 지나가면 뜨지 않는다. Esc 로 닫히고 초점은 그대로. 화면 낭독기에는 숨기고 안의 단추는 Tab 에 없다.
//      카드의 멀티라이브 → multilive_add, 시청하기 → 그 채널을 연다. 19세 방송은 영상 미리보기를 걸지 않는다.
// LF05 대비 — 타일 글자(이름·제목·시청자·카테고리·경과·19)·더 보기·빈 상태·정렬·카드 글자가 실제 바탕과 4.5:1 이상(쉼·호버),
//      초점 테두리·즐겨찾기 별 3:1 이상. 다크·라이트. 바탕은 조상 겹침 계산값과 화면 픽셀 최빈색 중 낮은 쪽.
// LF06 모션 줄이기·고대비 — 설정 "절약"(data-motion=reduced)·시스템 동작 줄이기에서 카드·+ 의 전환·이동·크기 변화가 없고
//      구역 안에 도는 애니메이션이 없다. 고대비(forced-colors, Chromium)에서 초점 테두리가 시스템 강조색.
// LF07 빈 상태·실패 — 0개면 구역은 남고 한 줄 안내, 팔로잉을 못 받으면 안내 + 다시 시도(구역 없음).
// LF08 갱신 깜박임 없음 — 다른 화면에 갔다 와도(홈 다시 그리기), 주기 갱신(사이드바 2분 — 여기서는 즐겨찾기 추가가 같은 함수를
//      부른다)에도 남은 채널의 칸·그림 요소가 그대로이고 그림을 다시 받지 않는다. 바뀐 글자만 바뀌고 끝난 방송은 빠지고 새 방송은
//      들어온다. 마우스·초점이 구역에 있는 동안은 자리를 바꾸지 않고, 떠나면 제 순서로.
// LF09~LF16 — PR #120 검토에서 나온 문제(고친 뒤 다시 생기지 않게):
// LF09 마우스로 연 카드를 초점이 body 일 때도 Esc 로 닫고, 포인터 아래에서 다시 뜨지 않으며, 위에 뜬 카드 메뉴가 먼저 닫힌다.
// LF10 '접기'를 누르면 그 단추(→ '+N 더 보기')가 화면에서 제자리에 남는다(위로 밀려 사라지지 않음).
// LF11 정렬·더 보기·접기를 마우스로 누르고 떠난 뒤 2분 갱신(가짜 시계)에 새 즐겨찾기·새 방송이 제 자리(앞)에 보인다.
// LF12 스켈레톤을 거치는 다시 그리기(네트워크 복구 자동 새로고침, online 이벤트)에서 키보드 초점이 같은 칸으로 돌아온다.
// LF13 초점 있던 칸이 숨겨지면(창이 좁아짐) 초점이 '+N 더 보기'로 간다(body 로 떨어지지 않음).
// LF14 화살표 이동이 페이지를 스크롤해도 새 칸의 카드가 뜬다.
// LF15 color-mix 를 모르는 엔진(Safari 16.0~16.1 흉내)에서도 개수 배지 바탕이 불투명하고 글자 4.5:1 이상.
// LF16 홈에 구역 자리가 없어지면(팔로잉 실패) 구역이 떨어진 옛 홈 트리를 붙잡지 않고 카드를 닫는다. 복구되면 같은 요소가 돌아온다.
// LF17~LF24 — v0.1.103 의 수정본을 다시 검증하며 나온 문제:
// LF17 더 보기·접기·정렬을 마우스로 누른 뒤 아무 키(호버 카드 Esc·Shift·PageDown)를 눌러도 2분 갱신의 자리 바꿈이 멈추지 않는다.
// LF18 마우스로 더 보기를 누른 뒤 화살표로 칸을 옮기면 키보드 사용이다 — 2분 갱신이 초점 칸을 옮기지 않고, 초점이 떠나면 제 순서.
// LF19 카드를 띄운 채(포인터 그대로) 단축키·메뉴로 다른 화면에 가면 카드가 닫히고, 다음 화면의 첫 Esc 를 가로채지 않는다.
// LF20 카드 뒤에서 키보드로 연 계정 메뉴의 Esc 는 메뉴부터 닫는다.
// LF21 옆 칸으로 옮긴 직후 휠로 내리면 화면 밖으로 밀려난 칸의 카드가 뜨지 않는다.
// LF22 초점을 가진 '+N 더 보기'·'접기'가 사라지면(창이 넓어짐·방송 종료) 초점이 가까운 칸으로 간다(body 로 떨어지지 않음).
// LF23 마우스를 격자 위에 둔 채 화살표로 페이지를 스크롤해도 초점 칸의 카드가 뜬다(엔진의 합성 포인터 이벤트 무시).
// LF24 스켈레톤을 거친 다시 그리기에서 기억한 칸이 더 보기 뒤로 밀리거나 사라지면 더 보기·같은 자리 칸으로 초점.
// LF25~LF32 — 위 수정을 독립 검토(반박 검증)하며 확인된 문제:
// LF25 마우스로 연 계정 메뉴(Esc 처리가 없다)가 떠 있어도 카드는 Esc 로 닫히고, 팔레트가 닫히는 잔상 중의 두 번째 Esc 도 카드를 닫는다.
// LF26 다른 곳(카드 메뉴·팔레트)이 Esc 로 닫히며 돌려준 초점은 키보드 사용이 아니다 — 2분 갱신이 멈추지 않는다(Chromium 이 :focus-visible 로 친다).
// LF27 칸에서 연 카드 메뉴(Shift+F10·우클릭)가 떠 있는 동안은 미룬 자리 바꿈을 하지 않고, Esc 로 닫으면 초점이 그 칸으로 돌아온다.
// LF28 키보드 초점이 숨는 정렬 단추(셋 이하)·격자(0개)와 함께 사라지면 첫 칸·'전체 보기'로 간다.
// LF29 마우스로 더 보기를 누른 뒤 화살표·End 로 옮긴 초점에 테두리가 보인다(WebKit). 마우스로만 누르면 테두리 없음.
// LF30 칸이 가만있는 포인터 아래에서 밀려난 뒤(방송 종료) 포인터를 칸 밖으로 옮기면 카드가 닫힌다(Chromium).
// LF31 칸에 머무는 동안 휠로 내리면, 머묾 예약이 밀려난 칸의 카드를 띄우지 않는다(WebKit 의 늦은 :hover).
// LF32 앱이 뜨기 전부터 커서가 격자 위에 있고 움직이지 않아도, 화살표 스크롤 뒤 초점 칸의 카드가 뜬다(Chromium).
// LF33~LF35 — 그 보완을 다시 검토해 확인된 문제:
// LF33 카드 메뉴 가장자리 — 격자 위 메뉴 항목에서 Esc 로 닫아도(포인터 그대로) 칸이 숨지 않고, 미리보기 카드에서 연 메뉴도 칸 메뉴로 치며,
//      '+N 더 보기' 우클릭 뒤 다른 곳(사이드바)에서 연 메뉴를 칸 메뉴로 알지 않는다.
// LF34 마우스를 칸에 둔 채 Tab 으로 목록을 지나 나간 뒤 마우스를 치우면, 떠난 초점의 카드가 뒤늦게 뜨지 않는다.
// LF35 포인터가 문서를 떠나면(펜을 듦·커서 위로 다른 창 — 같은 자리 relatedTarget 없는 pointerout) 머묾 예약이 카드를 띄우지 않고, 열린 카드는 닫힌다.
// LF36~LF38 — 세 번째 검토에서 확인된 문제:
// LF36 문서를 떠났던 커서가 움직이지 않고 돌아와도(같은 자리 pointermove) 키보드 카드를 빼앗지 않는다. 실제로 움직이면 호버가 맡는다.
// LF37 포인터가 머리줄(칸이 아닌 곳)에 있을 때 키보드 카드가 열린 채 Tab 으로 목록을 나가면 카드가 닫힌다.
// LF38 격자 위 메뉴 항목에서 Esc 로 닫아 미룬 자리 바꿈이, 포인터를 움직이지 않고 휠로 구역을 지나쳐도 적용된다(WebKit).
// 저장소 루트에서: node tools/ui-regression/live-following.mjs   (Playwright 필요, axe-core 가 있으면 LF03 에서 쓴다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG. CVIEW_LF_ONLY=LF17,LF22 면 고른 것만.)
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, base, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
let axeSource = null;
try { axeSource = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8'); } catch { /* 없으면 axe 만 건너뛴다 */ }

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 최빈색 계산용
const results = [];
// CVIEW_LF_ONLY=LF17,LF22 — 고른 검사만(고치는 동안 빨리 돌릴 때)
const ONLY = (process.env.CVIEW_LF_ONLY || '').split(',').map((s) => s.trim()).filter(Boolean);
async function check(id, fn) {
  if (ONLY.length && !ONLY.some((p) => id.startsWith(`${p} `))) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e && e.stack ? e.stack.split('\n').slice(0, 3).join(' | ') : String(e) }); }
}
const fail = (msg) => { throw new Error(msg); };
const ok = (cond, msg) => { if (!cond) fail(msg); };
const shot = async (page, name, clipSel = '.lf') => {
  if (!SHOTS) return;
  const b = await page.locator(clipSel).first().boundingBox();
  if (!b) return;
  const vp = page.viewportSize();
  const clip = { x: Math.max(0, b.x - 12), y: Math.max(0, b.y - 12), width: Math.min(vp.width - Math.max(0, b.x - 12), b.width + 24), height: Math.min(vp.height - Math.max(0, b.y - 12), b.height + 24 + 300) };
  await page.screenshot({ path: path.join(SHOTS, `${WEBKIT ? 'webkit' : 'chromium'}-${name}.png`), clip });
};

// ── 모의 IPC ──────────────────────────────────────────────────────────────
// opts: { count, favorites: [아이디], recent: [아이디], theme, motion, failFollowing, adultAt }
function mockLF([origin, opts]) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const names = ['네이버 스포츠', '아라하시 타비', '초승달', '소풍왔니', '정령왕', '백곰파', '템비션', '꽁주', '포셔', '엘시', '썽예', '햇살', '이로나롱 치카', '귀알', '계춘회', '쫀득님', '해블린', '포키짱', '크캣', '유우양'];
  const cats = ['talk', '리그 오브 레전드', '로스트아크', '드라마', '마인크래프트', '종합 게임'];
  const titles = ['시청자 참여 내전 + 경품 이벤트 진행 중', '신작 첫 플레이! 스포 금지', '혼돈의 서버 생존기', '대회 리뷰 & 분석', '저녁 먹고 수다 한 판', '새벽 감성 노래방'];
  const now = Date.now();
  const live = (i, viewers = Math.max(30, Math.round(52000 / (1 + i * 0.5)))) => ({
    channelId: `ch${i}`, channelName: names[i % names.length] + (i >= names.length ? ` ${i}` : ''), channelImageUrl: img('avatar', i),
    liveTitle: `${titles[i % titles.length]} #${i}`, thumbnailUrl: img('thumb', i), viewerCount: viewers, viewerCountText: viewers.toLocaleString('ko-KR'),
    // 방송 시작 순서는 시청자 순서와 다르게 섞는다(방금 시작 정렬 검사)
    categoryName: cats[i % cats.length], openDate: new Date(now - (((i * 37) % 120) + 5 + i / 100) * 60000).toISOString(), isLive: true,
    adult: i === (opts.adultAt ?? 1), badge: i % 4 === 0 ? 'partner' : null,
  });
  const state = {
    following: Array.from({ length: opts.count }, (_, i) => live(i)),
    favs: new Set(opts.favorites || []),
    recent: (opts.recent || []).map((id, k) => ({ channelId: id, channelName: id, channelImageUrl: img('avatar', Number(id.slice(2)) || 0), lastWatched: now - k * 3600000 })),
    calls: [],
    live,
  };
  window.__lf = state;
  const handlers = {
    auth_status: () => ({ loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {}, appearance: { theme: opts.theme || 'dark', motionQuality: opts.motion || 'balanced' } }),
    following_lives: () => { if (opts.failFollowing || state.fail) throw new Error('network timed out'); return state.following.map((l) => ({ ...l })); },
    popular_lives: () => Array.from({ length: 24 }, (_, i) => ({ ...live(200 + i), channelId: `pop${i}` })),
    recent_channels: () => state.recent,
    favorites_list: () => [...state.favs].map((channelId) => ({ channelId, channelName: channelId })),
    favorite_set: ({ channelId, favorite }) => { if (favorite) state.favs.add(channelId); else state.favs.delete(channelId); return null; },
    vod_continue_list: () => [], popular_clips: () => [],
    multilive_state: () => ({ sessions: [], layout: 'grid' }),
    multilive_add: ({ channelId }) => ({ sessions: [{ id: 's1', channelId, slot: 0 }], layout: 'grid', selectedId: 's1' }),
  };
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        if (cmd === 'ui_heartbeat') return null;
        state.calls.push([cmd, args || {}]);
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args || {});
      },
    },
    event: { listen: async () => () => {} },
  };
  try {
    localStorage.setItem('cview.onboarded', '1');
    localStorage.setItem('cview.welcomed.v1', '1');
    localStorage.setItem('cview.theme', opts.theme || 'dark');
    if (opts.sort) localStorage.setItem('cview.home.liveSort', opts.sort);
  } catch { /* 무시 */ }
}

async function openHome(opts = {}) {
  const { width = 1280, height = 900, theme = 'dark', reducedMotion = 'no-preference' } = opts;
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme === 'dark' ? 'dark' : 'light', reducedMotion, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  // 2분 주기 갱신(app.js refreshSidebarFollowing)을 앞당겨 부를 검사는 가짜 시계 — 시간은 그대로 흐르고 fastForward 로 건너뛴다
  if (opts.clock) await page.clock.install();
  await page.addInitScript(mockLF, [base, { count: 12, theme, ...opts }]);
  // mouseBeforeLoad: [x, y] — 앱이 뜨기 전부터 커서가 그 자리에 있다(빈 문서에서 옮겨 두고, 뒤에서 움직이지 않는다)
  if (opts.mouseBeforeLoad) await page.mouse.move(...opts.mouseBeforeLoad);
  await page.goto(base);
  await page.waitForSelector(opts.failFollowing ? '.home .home-notice' : '.home .lf', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  if (theme === 'light') await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
  if (!opts.mouseBeforeLoad) await page.mouse.move(2, height - 2);
  await page.waitForTimeout(150);
  return { ctx, page, errors };
}

/** 지금 격자 상태 — 칸 수·보이는 칸·더 보기. */
const gridState = (page) => page.evaluate(() => {
  const lf = document.querySelector('.lf');
  const g = lf.querySelector('.lf-grid');
  const tracks = getComputedStyle(g).gridTemplateColumns.trim().split(/\s+/).length;
  const items = [...g.querySelectorAll('.lf-item:not(.lf-more-item)')];
  const vis = items.filter((li) => !li.hidden && li.offsetParent);
  const more = g.querySelector('.lf-more-item');
  const gr = g.getBoundingClientRect();
  const rects = vis.map((li) => li.getBoundingClientRect());
  const rows = new Set(rects.map((r) => Math.round(r.top))).size;
  const pageEl = document.querySelector('.page-home');
  return {
    cols: tracks, total: items.length, visible: vis.length, keys: items.map((li) => li.dataset.key), visKeys: vis.map((li) => li.dataset.key),
    more: more && !more.hidden ? { count: more.querySelector('.lf-more-count').textContent, expanded: more.firstElementChild.getAttribute('aria-expanded') } : null,
    rows: rows + (more && !more.hidden && rects.length && Math.round(more.getBoundingClientRect().top) > Math.max(...rects.map((r) => Math.round(r.top))) ? 1 : 0),
    inside: rects.every((r) => r.left >= gr.left - 0.5 && r.right <= gr.right + 0.5),
    maxTileW: Math.max(0, ...rects.map((r) => r.width)),
    gridHidden: g.hidden, emptyVisible: !lf.querySelector('.lf-empty').hidden,
    sortVisible: !lf.querySelector('.lf-sort').hidden, countText: lf.querySelector('.section-title .count').hidden ? '' : lf.querySelector('.section-title .count').textContent,
    overflow: pageEl ? pageEl.scrollWidth - pageEl.clientWidth : 0,
  };
});
const rowsFor = (cols) => (cols <= 1 ? 4 : cols === 2 ? 3 : 2);

// ── LF01 개수별 배치 ─────────────────────────────────────────────────────────
await check('LF01 layout by count × width (collapsed rows, +N tile, expand/collapse, no stretch, no overflow)', async () => {
  const problems = [];
  for (const width of [760, 1280, 2200]) {
    for (const count of [0, 1, 3, 12, 39, 60]) {
      const { ctx, page, errors } = await openHome({ count, width });
      const s = await gridState(page);
      const tag = `${width}px·${count}개`;
      if (errors.length) problems.push(`${tag} 페이지 오류 ${errors[0]}`);
      if (s.overflow > 1) problems.push(`${tag} 가로 넘침 ${s.overflow}px`);
      if (count === 0) {
        if (!s.gridHidden || !s.emptyVisible) problems.push(`${tag} 빈 상태가 아니다`);
      } else {
        const slots = s.cols * rowsFor(s.cols);
        const wantVis = count <= slots ? count : slots - 1;
        if (s.visible !== wantVis) problems.push(`${tag} 보이는 칸 ${s.visible} ≠ ${wantVis} (칸 ${s.cols})`);
        if (count > slots) {
          if (!s.more || s.more.count !== `+${count - (slots - 1)}`) problems.push(`${tag} 더 보기 ${JSON.stringify(s.more)} ≠ +${count - (slots - 1)}`);
        } else if (s.more) problems.push(`${tag} 넘치지 않는데 더 보기가 있다`);
        if (s.rows > rowsFor(s.cols)) problems.push(`${tag} 줄 ${s.rows} > ${rowsFor(s.cols)}`);
        if (!s.inside) problems.push(`${tag} 칸이 격자 밖으로 나간다`);
        if (s.maxTileW > 340) problems.push(`${tag} 타일이 늘어났다 ${s.maxTileW}px`);
        if (s.sortVisible !== (count >= 4)) problems.push(`${tag} 정렬 단추 보임 ${s.sortVisible}`);
        if (s.countText !== String(count)) problems.push(`${tag} 개수 배지 ${s.countText}`);
        if (count > slots) {
          await page.locator('.lf-more').click();
          const e = await gridState(page);
          if (e.visible !== count || !e.more || e.more.count !== '접기' || e.more.expanded !== 'true') problems.push(`${tag} 펼침 ${e.visible}/${count} ${JSON.stringify(e.more)}`);
          if (e.overflow > 1) problems.push(`${tag} 펼침 가로 넘침`);
          if (width === 1280 && count === 39) await shot(page, 'expanded-39');
          await page.locator('.lf-more').click();
          const c = await gridState(page);
          if (c.visible !== wantVis || c.more.expanded !== 'false') problems.push(`${tag} 접기 실패 ${c.visible}`);
        }
      }
      if (width === 1280) await shot(page, `count-${count}`);
      await ctx.close();
    }
  }
  // 순수 함수 표
  const { ctx, page } = await openHome({ count: 1 });
  const pure = await page.evaluate(async () => {
    const m = await import('/js/live-following.js');
    const L = (id, v, min) => ({ channelId: id, viewerCount: v, openDate: new Date(Date.now() - min * 60000).toISOString() });
    const lives = [L('a', 100, 50), L('b', 900, 10), L('c', 500, 5), L('d', 300, 90), L('b', 1, 1)];
    return {
      lim: [[39, 4], [8, 4], [9, 4], [5, 1], [6, 2], [7, 2], [0, 3]].map(([t, c]) => m.collapseLimit(t, c)),
      cols: [684, 1004, 211, 431, 432, 439, 440, 1604].map((w) => m.gridColumns(w)).join(),
      mine: m.arrangeLiveFollowing(lives, { sort: 'mine', favorites: new Set(['d']), recent: [{ channelId: 'a' }, { channelId: 'c' }] }).map((l) => l.channelId).join(''),
      mineArr: m.arrangeLiveFollowing(lives, { favorites: [{ channelId: 'a' }] }).map((l) => l.channelId).join(''),
      plain: m.arrangeLiveFollowing(lives, {}).map((l) => l.channelId).join(''),
      viewers: m.arrangeLiveFollowing(lives, { sort: 'viewers' }).map((l) => l.channelId).join(''),
      fresh: m.arrangeLiveFollowing(lives, { sort: 'fresh' }).map((l) => l.channelId).join(''),
    };
  });
  await ctx.close();
  const want = [{ shown: 7, hidden: 32, slots: 8 }, { shown: 8, hidden: 0, slots: 8 }, { shown: 7, hidden: 2, slots: 8 }, { shown: 3, hidden: 2, slots: 4 }, { shown: 6, hidden: 0, slots: 6 }, { shown: 5, hidden: 2, slots: 6 }, { shown: 0, hidden: 0, slots: 6 }];
  if (JSON.stringify(pure.lim) !== JSON.stringify(want)) problems.push(`collapseLimit ${JSON.stringify(pure.lim)}`);
  if (pure.cols !== '3,4,1,1,2,2,2,7') problems.push(`gridColumns ${pure.cols}`);
  if (pure.mine !== 'dacb') problems.push(`추천(즐겨찾기 d → 최근 a,c → 시청자) ${pure.mine} ≠ dacb`);
  if (pure.mineArr !== 'abcd') problems.push(`즐겨찾기 객체 배열 ${pure.mineArr} ≠ abcd`);
  if (pure.plain !== 'bcda' || pure.viewers !== 'bcda') problems.push(`즐겨찾기·최근 없음 = 시청자순 ${pure.plain}/${pure.viewers} ≠ bcda`);
  if (pure.fresh !== 'cbad') problems.push(`방금 시작 ${pure.fresh} ≠ cbad`);
  ok(!problems.length, problems.join(' / '));
});

// ── LF02 정렬 ───────────────────────────────────────────────────────────────
await check('LF02 sort: 추천(fav → recent → viewers) · 시청자 · 방금 시작, remembered; no favs/recent = old viewers order', async () => {
  const problems = [];
  const domOrder = (page) => page.$$eval('.lf-grid .lf-item:not(.lf-more-item)', (els) => els.map((e) => e.dataset.key));
  const ids = Array.from({ length: 12 }, (_, i) => `ch${i}`);
  const opened = (i) => ((i * 37) % 120) + 5 + i / 100; // 분 전 — mockLF 와 같은 식
  const fresh = [...ids].sort((a, b) => opened(Number(a.slice(2))) - opened(Number(b.slice(2))));
  {
    const { ctx, page } = await openHome({ count: 12, favorites: ['ch9', 'ch5'], recent: ['ch7', 'ch2', 'ch5'] });
    const want = ['ch5', 'ch9', 'ch7', 'ch2', ...ids.filter((x) => !['ch5', 'ch9', 'ch7', 'ch2'].includes(x))];
    const got = await domOrder(page);
    if (got.join() !== want.join()) problems.push(`추천 ${got.join()} ≠ ${want.join()}`);
    const stars = await page.$$eval('.lf-item.is-fav', (els) => els.map((e) => e.dataset.key).sort().join());
    if (stars !== 'ch5,ch9') problems.push(`즐겨찾기 별 ${stars}`);
    await page.locator('[data-lf-sort="viewers"]').click();
    await page.mouse.move(2, 880);
    let v = await domOrder(page);
    if (v.join() !== ids.join()) problems.push(`시청자 ${v.join()}`);
    await page.locator('[data-lf-sort="fresh"]').click();
    v = await domOrder(page);
    if (v.join() !== fresh.join()) problems.push(`방금 시작 ${v.join()} ≠ ${fresh.join()}`);
    // 좁은 타일(1280 창 ≈ 222px)의 셋째 줄 — 방금 시작이면 경과, 아니면 카테고리
    const meta = (page) => page.evaluate(() => { const li = document.querySelector('.lf-grid .lf-item:not(.lf-more-item)'); const w = li.getBoundingClientRect().width; const vis = (sel) => li.querySelector(sel).getBoundingClientRect().width > 0; return { w, cat: vis('.lf-cat'), up: vis('.lf-up') }; });
    let mt = await meta(page);
    if (mt.w < 280 && (mt.cat || !mt.up)) problems.push(`방금 시작 정렬의 좁은 타일 셋째 줄 ${JSON.stringify(mt)}`);
    await page.locator('[data-lf-sort="viewers"]').click();
    mt = await meta(page);
    if (mt.w < 280 && (!mt.cat || mt.up)) problems.push(`시청자 정렬의 좁은 타일 셋째 줄 ${JSON.stringify(mt)}`);
    await page.locator('[data-lf-sort="fresh"]').click();
    await page.mouse.move(2, 880);
    const pressed = await page.$$eval('.lf-sort .seg', (els) => els.map((e) => `${e.dataset.lfSort}:${e.getAttribute('aria-pressed')}`).join());
    if (!pressed.includes('fresh:true') || pressed.includes('mine:true')) problems.push(`aria-pressed ${pressed}`);
    await page.reload();
    await page.waitForSelector('.home .lf');
    await page.waitForTimeout(200);
    v = await domOrder(page);
    if (v.join() !== fresh.join()) problems.push(`다시 연 뒤 정렬이 남지 않았다 ${v.join()}`);
    if (!(await page.locator('[data-lf-sort="fresh"].active').count())) problems.push('다시 연 뒤 방금 시작 단추가 고른 상태가 아니다');
    await ctx.close();
  }
  {
    const { ctx, page } = await openHome({ count: 12 });
    const got = await domOrder(page);
    if (got.join() !== ids.join()) problems.push(`즐겨찾기·최근 없을 때 추천 ≠ 시청자순: ${got.join()}`);
    if (!(await page.locator('[data-lf-sort="mine"].active').count())) problems.push('기본 정렬이 추천이 아니다');
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

// ── LF03 키보드·이름 ─────────────────────────────────────────────────────────
await check('LF03 keyboard roving (Tab once, arrows/Home/End, expand via Enter, open via Enter, Shift+F10 menu) and names', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280 });
  const active = () => page.evaluate(() => {
    const a = document.activeElement;
    const li = a && a.closest('.lf-item');
    return { cls: a ? a.className : '', key: li ? (li.dataset.key || '__more') : null, idx: li ? [...li.parentElement.children].filter((x) => !x.hidden).indexOf(li) : -1 };
  });
  const s = await gridState(page);
  const cols = s.cols;
  const slots = cols * rowsFor(cols);
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  let a = await active();
  if (a.cls !== 'lf-open' || a.idx !== 0) problems.push(`Tab → 첫 칸이 아니다 ${JSON.stringify(a)}`);
  await page.keyboard.press('Tab');
  a = await active();
  if (!a.cls.includes('lf-add') || a.idx !== 0) problems.push(`Tab 두 번 → 첫 칸의 + 가 아니다 ${JSON.stringify(a)}`);
  await page.waitForTimeout(300); // + 가 나타나는 전환
  const addVisible = await page.evaluate(() => getComputedStyle(document.activeElement).opacity);
  if (addVisible !== '1') problems.push(`초점 받은 + 가 보이지 않는다 (opacity ${addVisible})`);
  await page.keyboard.press('Tab');
  a = await active();
  if (a.key !== null) problems.push(`Tab 세 번에도 목록 안 ${JSON.stringify(a)}`);
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  a = await active();
  if (a.cls !== 'lf-open' || a.idx !== 0) problems.push(`Shift+Tab 으로 첫 칸에 돌아오지 않았다 ${JSON.stringify(a)}`);
  await page.keyboard.press('ArrowRight');
  a = await active();
  if (a.idx !== 1) problems.push(`→ ${a.idx}`);
  await page.keyboard.press('ArrowDown');
  a = await active();
  if (a.idx !== 1 + cols) problems.push(`↓ ${a.idx} ≠ ${1 + cols}`);
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowLeft');
  a = await active();
  if (a.idx !== 0) problems.push(`↑← ${a.idx}`);
  await page.keyboard.press('ArrowLeft');
  a = await active();
  if (a.idx !== 0) problems.push(`처음에서 ← 가 움직였다 ${a.idx}`);
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  // 목록을 나갔다 오면 마지막 칸(로빙)
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  a = await active();
  if (a.idx !== 2) problems.push(`다시 들어오면 마지막 칸(2)이 아니다 ${a.idx}`);
  const tabbables = await page.$$eval('.lf-grid [tabindex="0"]', (els) => els.length);
  if (tabbables !== 2) problems.push(`Tab 이 닿는 요소 ${tabbables}개(주 단추 + + 두 개여야)`);
  await page.keyboard.press('End');
  a = await active();
  if (a.key !== '__more') problems.push(`End → 더 보기가 아니다 ${JSON.stringify(a)}`);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(50);
  a = await active();
  const exp = await page.locator('.lf-more').getAttribute('aria-expanded');
  if (exp !== 'true') problems.push('Enter 로 펼쳐지지 않았다');
  if (a.idx !== slots - 1 || a.cls !== 'lf-open') problems.push(`펼친 뒤 처음 드러난 칸(${slots - 1})이 아니다 ${JSON.stringify(a)}`);
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  a = await active();
  if (a.key !== '__more' || (await page.locator('.lf-more').getAttribute('aria-expanded')) !== 'false') problems.push(`접기 뒤 초점 ${JSON.stringify(a)}`);
  await page.keyboard.press('Home');
  // 이름
  const names = await page.evaluate(() => {
    const lf = document.querySelector('.lf');
    const g = lf.querySelector('.lf-grid');
    const t = document.getElementById(g.getAttribute('aria-labelledby'));
    const first = g.querySelector('.lf-item');
    return {
      role: g.getAttribute('role'), list: t ? t.textContent.replace(/\s+/g, ' ').trim() : '', section: document.getElementById(lf.getAttribute('aria-labelledby'))?.textContent.replace(/\s+/g, ' ').trim(),
      open: first.querySelector('.lf-open').getAttribute('aria-label'), add: first.querySelector('.lf-add').getAttribute('aria-label'),
      name: first.querySelector('.lf-name-text').textContent, title: first.querySelector('.lf-title').textContent,
      more: lf.querySelector('.lf-more').getAttribute('aria-label'), peekHidden: lf.querySelector('.lf-peek').getAttribute('aria-hidden'),
    };
  });
  if (names.role !== 'list' || names.list !== '방송 중인 팔로잉 39') problems.push(`목록 이름 ${names.role} "${names.list}"`);
  if (!names.open.startsWith(`${names.name}, `) || !names.open.includes(names.title) || !/시청자 [\d,.만천]+명/.test(names.open)) problems.push(`주 단추 이름 "${names.open}"`);
  if (names.add !== `${names.name} 멀티라이브에 추가`) problems.push(`+ 이름 "${names.add}"`);
  if (!/^방송 중인 팔로잉 \d+개 더 보기$/.test(names.more)) problems.push(`더 보기 이름 "${names.more}"`);
  if (names.peekHidden !== 'true') problems.push('미리보기 카드가 화면 낭독기에 숨지 않았다');
  // axe — 구역만(카드 열린 상태 포함)
  if (axeSource) {
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(500);
    await page.addScriptTag({ content: axeSource });
    const v = await page.evaluate(async () => {
      const r = await window.axe.run(document.querySelector('.lf'), { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] }, rules: { 'color-contrast': { enabled: false } } });
      return r.violations.map((x) => `${x.id}(${x.nodes.length})`);
    });
    if (v.length) problems.push(`axe ${v.join(', ')}`);
  }
  // Shift+F10 → 카드 메뉴
  await page.keyboard.press('Shift+F10');
  await page.waitForTimeout(150);
  const menu = await page.evaluate(() => {
    const m = document.querySelector('.ctx-menu');
    return m ? { items: [...m.querySelectorAll('button')].map((b) => b.textContent.trim()).slice(0, 3).join('|'), focused: m.contains(document.activeElement) } : null;
  });
  if (!menu || !menu.items.startsWith('재생|멀티라이브에 추가')) problems.push(`Shift+F10 메뉴 ${JSON.stringify(menu)}`);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  // Enter → 연다
  const target = await page.evaluate(() => document.activeElement.closest('.lf-item')?.dataset.key);
  await page.evaluate(() => { window.__lf.calls.length = 0; });
  await page.keyboard.press('Enter');
  await page.waitForTimeout(300);
  const opened = await page.evaluate((id) => ({
    recent: JSON.parse(localStorage.getItem('cview.recentChannels') || '[]')[0]?.channelId,
    call: window.__lf.calls.some(([, a]) => a && a.channelId === id),
  }), target);
  if (!target || opened.recent !== target || !opened.call) problems.push(`Enter 로 ${target} 를 열지 않았다 ${JSON.stringify(opened)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

// ── LF04 미리보기 카드 ───────────────────────────────────────────────────────
await check('LF04 peek card: hover dwell / keyboard focus, stays while hovered, no flash on sweep, Esc, actions', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280, adultAt: 3 });
  const peek = () => page.evaluate(() => {
    const p = document.querySelector('.lf-peek');
    const sec = document.querySelector('.lf').getBoundingClientRect();
    const r = p.getBoundingClientRect();
    return {
      open: !p.hidden && p.classList.contains('on'), key: p.dataset.channelId, side: p.dataset.side,
      title: p.querySelector('.lf-peek-title')?.textContent, img: p.querySelector('.lf-peek-media img')?.getAttribute('src') || '',
      preview: p.querySelector('.lf-peek-media')?.dataset.preview || null, top: r.top, left: r.left, right: r.right, secL: sec.left, secR: sec.right,
      tabbable: [...p.querySelectorAll('button, a, [tabindex]')].filter((b) => b.tabIndex >= 0).length, buttons: p.querySelectorAll('button').length,
    };
  });
  const tile = (i) => page.locator('.lf-grid .lf-item:not(.lf-more-item):not([hidden])').nth(i);
  // 훑고 지나가기 — 칸마다 60ms 만 머문다
  let flashed = false;
  for (let i = 0; i < 5; i += 1) {
    const b = await tile(i).boundingBox();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 2 });
    await page.waitForTimeout(60);
    if ((await peek()).open) flashed = true;
  }
  await page.mouse.move(2, 880);
  await page.waitForTimeout(500);
  if (flashed) problems.push('훑고 지나갈 때 카드가 떴다');
  // 머물기
  const b1 = await tile(1).boundingBox();
  await page.mouse.move(b1.x + 30, b1.y + b1.height / 2);
  await page.waitForTimeout(650);
  let p = await peek();
  const key1 = await tile(1).getAttribute('data-key');
  if (!p.open || p.key !== key1) problems.push(`머물러도 카드가 없다 ${JSON.stringify(p)}`);
  if (p.side !== 'below' || p.top < b1.y + b1.height) problems.push(`카드가 칸 아래가 아니다 ${p.side} ${p.top}`);
  if (p.left < p.secL - 0.5 || p.right > p.secR + 0.5) problems.push('카드가 구역 밖으로 나갔다');
  if (!p.img.includes(`/thumb/${key1.slice(2)}.svg`)) problems.push(`썸네일 ${p.img}`);
  if (p.preview !== `live:${key1}`) problems.push(`영상 미리보기 연결 ${p.preview}`);
  if (p.tabbable !== 0 || p.buttons < 3) problems.push(`카드 단추 ${p.buttons}개 중 Tab 닿음 ${p.tabbable}`);
  await shot(page, 'peek-hover');
  // 카드로 옮겨도 남는다
  const pb = await page.locator('.lf-peek').boundingBox();
  await page.mouse.move(b1.x + 30, b1.y + b1.height + 4, { steps: 3 });
  await page.mouse.move(pb.x + pb.width / 2, pb.y + pb.height - 20, { steps: 4 });
  await page.waitForTimeout(400);
  p = await peek();
  if (!p.open || p.key !== key1) problems.push('카드로 옮기자 닫혔다');
  // 나가면 닫힌다
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(400);
  if ((await peek()).open) problems.push('나갔는데 카드가 남았다');
  // 19세 — 영상 미리보기 없음
  const b3 = await tile(3).boundingBox();
  const key3 = await tile(3).getAttribute('data-key');
  await page.mouse.move(b3.x + 30, b3.y + b3.height / 2);
  await page.waitForTimeout(650);
  p = await peek();
  if (!p.open || p.key !== key3 || p.preview) problems.push(`19세 칸 카드 ${JSON.stringify({ open: p.open, key: p.key, preview: p.preview })}`);
  await page.mouse.move(2, 880);
  await page.waitForTimeout(400);
  // 키보드 초점 → 카드, Esc → 닫히고 초점 그대로, → 로 옮기면 다시
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.waitForTimeout(500);
  p = await peek();
  const key0 = await tile(0).getAttribute('data-key');
  if (!p.open || p.key !== key0) problems.push(`키보드 초점에 카드가 없다 ${JSON.stringify({ open: p.open, key: p.key })}`);
  await shot(page, 'peek-keyboard');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  p = await peek();
  const still = await page.evaluate(() => document.activeElement.closest('.lf-item')?.dataset.key);
  if (p.open || still !== key0) problems.push(`Esc 뒤 카드 ${p.open} · 초점 ${still}`);
  await page.waitForTimeout(400);
  if ((await peek()).open) problems.push('Esc 로 닫은 카드가 다시 떴다');
  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(450);
  p = await peek();
  if (!p.open || p.key !== key1) problems.push('→ 로 옮긴 칸에 카드가 없다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  // 카드의 단추 — 화면이 바뀌므로 단추마다 새로 연다. 칸에 머물러 카드를 띄우고, 칸 아래 틈을 지나 카드 단추를 누른다.
  const peekAction = async (sel) => {
    const o = await openHome({ count: 12, width: 1280 });
    const t = o.page.locator('.lf-grid .lf-item:not(.lf-more-item):not([hidden])').nth(2);
    const key = await t.getAttribute('data-key');
    const b = await t.boundingBox();
    await o.page.mouse.move(b.x + 30, b.y + b.height / 2);
    await o.page.waitForTimeout(650);
    const btn = await o.page.locator(`.lf-peek ${sel}`).boundingBox();
    await o.page.mouse.move(b.x + 30, b.y + b.height + 4, { steps: 2 });
    await o.page.mouse.move(btn.x + btn.width / 2, btn.y + btn.height / 2, { steps: 4 });
    await o.page.evaluate(() => { window.__lf.calls.length = 0; });
    await o.page.mouse.click(btn.x + btn.width / 2, btn.y + btn.height / 2);
    await o.page.waitForTimeout(300);
    const r = await o.page.evaluate((id) => ({
      recent: JSON.parse(localStorage.getItem('cview.recentChannels') || '[]')[0]?.channelId,
      calls: window.__lf.calls.filter(([, a]) => a && a.channelId === id).map(([c]) => c),
    }), key);
    if (o.errors.length) problems.push(`${sel} 페이지 오류 ${o.errors[0]}`);
    await o.ctx.close();
    return { key, ...r };
  };
  const ml = await peekAction('.lf-peek-ml');
  if (!ml.calls.includes('multilive_add')) problems.push(`카드의 멀티라이브가 multilive_add 를 부르지 않았다 ${JSON.stringify(ml)}`);
  const play = await peekAction('.lf-peek-play');
  if (play.recent !== play.key || !play.calls.length) problems.push(`카드의 시청하기가 ${play.key} 를 열지 않았다 ${JSON.stringify(play)}`);
  const info = await peekAction('.lf-peek-info');
  if (!info.calls.length || info.recent === info.key) problems.push(`카드의 채널 정보가 채널 화면을 열지 않았다(또는 재생으로 열었다) ${JSON.stringify(info)}`);
  ok(!problems.length, problems.join(' / '));
});

// ── LF05 대비 ───────────────────────────────────────────────────────────────
function colorKit() {
  const parse = (s) => {
    s = String(s).trim();
    if (!s || s === 'transparent') return [0, 0, 0, 0];
    let m = s.match(/^rgba?\(([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s,/]+/).filter(Boolean).map(Number); return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1]; }
    m = s.match(/^color\(srgb\s+([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s/]+/).filter(Boolean).map(Number); return [p[0] * 255, p[1] * 255, p[2] * 255, p.length > 3 ? p[3] : 1]; }
    const probe = document.createElement('i');
    document.body.append(probe);
    probe.style.color = s;
    const c = getComputedStyle(probe).color;
    probe.remove();
    if (!c || c === s) throw new Error(`색을 읽지 못했다: ${s}`);
    return parse(c);
  };
  const over = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));
  const backOf = (el) => {
    const layers = [];
    for (let e = el; e; e = e.parentElement) {
      const c = parse(getComputedStyle(e).backgroundColor);
      if (c[3] > 0) layers.push(c);
      if (c[3] >= 1) break;
    }
    let acc = [255, 255, 255];
    for (let i = layers.length - 1; i >= 0; i -= 1) acc = over(layers[i], acc);
    return acc;
  };
  window.__ck = { parse, over, backOf };
}
const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
const overN = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));

async function pixelBack(page, handle) {
  const b = await handle.boundingBox();
  if (!b || b.width < 2 || b.height < 2) return null;
  const vp = page.viewportSize();
  const clip = { x: Math.max(0, b.x), y: Math.max(0, b.y), width: Math.min(b.width, vp.width - Math.max(0, b.x)), height: Math.min(b.height, vp.height - Math.max(0, b.y)) };
  if (clip.width < 2 || clip.height < 2) return null;
  const buf = await page.screenshot({ clip, animations: 'disabled', caret: 'hide' });
  return calc.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.width; c.height = img.height;
    const x = c.getContext('2d');
    x.drawImage(img, 0, 0);
    const D = x.getImageData(0, 0, c.width, c.height).data;
    const counts = new Map();
    for (let i = 0; i < D.length; i += 4) { const k = (D[i] << 16) | (D[i + 1] << 8) | D[i + 2]; counts.set(k, (counts.get(k) || 0) + 1); }
    let best = 0, key = 0;
    for (const [k, n] of counts) if (n > best) { best = n; key = k; }
    return [(key >> 16) & 255, (key >> 8) & 255, key & 255];
  }, buf.toString('base64'));
}

async function measureAll(page, sels, tag, problems, report) {
  await page.evaluate(colorKit);
  for (const [sel, min, kind] of sels) {
    const handles = await page.$$(sel);
    const vis = [];
    for (const h of handles) if (await h.isVisible()) vis.push(h);
    if (!vis.length) { problems.push(`${tag} ${sel} 를 찾지 못했다`); continue; }
    for (const h of vis.slice(0, 2)) {
      const info = await h.evaluate((el, kind) => {
        const K = window.__ck;
        const cs = getComputedStyle(el);
        const fg = K.parse(kind === 'outline' ? cs.outlineColor : cs.color);
        // 테두리(outline)는 칸 바깥 — 바탕은 칸의 부모(구역) 쪽
        const back = K.backOf(kind === 'outline' ? el.closest('.lf-item').parentElement : el);
        return { fg, back, text: el.textContent.trim().slice(0, 16) };
      }, kind);
      const fg = info.fg[3] < 1 ? overN(info.fg, info.back) : info.fg.slice(0, 3);
      let r = ratio(fg, info.back);
      if (kind !== 'outline') {
        const px = await pixelBack(page, h);
        if (px) r = Math.min(r, ratio(fg, px));
      }
      report.push(`${tag} ${sel} "${info.text}" ${r.toFixed(2)}`);
      if (r < min) problems.push(`${tag} ${sel} "${info.text}" ${r.toFixed(2)} < ${min}`);
    }
  }
}

await check('LF05 contrast: tile/more/empty/sort/peek text ≥ 4.5:1 (rest & hover), focus ring & star ≥ 3:1 — dark & light', async () => {
  const problems = [];
  const report = [];
  const TEXT = [
    ['.lf-item:not(.lf-more-item):not([hidden]) .lf-name-text', 4.5], ['.lf-item:not([hidden]) .lf-title', 4.5], ['.lf-item:not([hidden]) .lf-vn', 4.5],
    ['.lf-item:not([hidden]) .lf-cat', 4.5], ['.lf-item:not([hidden]) .lf-adult', 4.5],
    ['.lf-more-count', 4.5], ['.lf-more-label', 4.5], ['.lf-sort .seg', 4.5], ['.lf-all', 4.5], ['.lf .section-title .count', 4.5],
  ];
  for (const theme of ['dark', 'light']) {
    const { ctx, page } = await openHome({ count: 39, width: 1280, theme, favorites: ['ch2'], adultAt: 1 });
    await measureAll(page, TEXT, `${theme} 쉼`, problems, report);
    await measureAll(page, [['.lf-item.is-fav .lf-fav', 3]], `${theme} 별`, problems, report);
    // 경과 — 좁은 타일은 "방금 시작" 정렬에서만 보인다
    await page.locator('[data-lf-sort="fresh"]').click();
    await page.mouse.move(2, 880);
    await measureAll(page, [['.lf-item:not([hidden]) .lf-up', 4.5]], `${theme} 쉼`, problems, report);
    await page.locator('[data-lf-sort="mine"]').click();
    await page.mouse.move(2, 880);
    // 호버 — 19세 표시가 있는 칸
    const t = page.locator('.lf-grid .lf-item:has(.lf-adult:not([hidden]))').first();
    const b = await t.boundingBox();
    await page.mouse.move(b.x + 20, b.y + 10);
    await page.waitForTimeout(80);
    await measureAll(page, [['.lf-item:hover .lf-name-text', 4.5], ['.lf-item:hover .lf-title', 4.5], ['.lf-item:hover .lf-vn', 4.5], ['.lf-item:hover .lf-cat', 4.5], ['.lf-item:hover .lf-adult', 4.5]], `${theme} 호버`, problems, report);
    await page.waitForTimeout(600); // 카드
    await measureAll(page, [['.lf-peek-title', 4.5], ['.lf-peek-ch', 4.5], ['.lf-peek-cat', 4.5], ['.lf-peek-ml', 4.5], ['.lf-peek-info', 4.5]], `${theme} 카드`, problems, report);
    await shot(page, `contrast-${theme}`);
    await page.mouse.move(2, 880);
    // 초점 테두리
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(400); // 초점 테두리가 조여 드는 애니메이션(.24s)이 끝난 뒤
    await measureAll(page, [['.lf-item .lf-open:focus-visible', 3, 'outline']], `${theme} 초점`, problems, report);
    await page.keyboard.press('End');
    await page.waitForTimeout(400);
    await measureAll(page, [['.lf-more:focus-visible', 3, 'outline']], `${theme} 더 보기 초점`, problems, report);
    await ctx.close();
    // 빈 상태
    const e = await openHome({ count: 0, theme });
    await measureAll(e.page, [['.lf-empty-text strong', 4.5], ['.lf-empty-text span', 4.5]], `${theme} 빈 상태`, problems, report);
    await e.ctx.close();
  }
  if (process.env.CVIEW_LF_REPORT) console.log(report.join('\n'));
  ok(!problems.length, problems.join(' / '));
});

// ── LF06 모션 줄이기·고대비 ─────────────────────────────────────────────────
await check('LF06 reduced motion (setting & system): no peek/+ transition or movement, no running animations; forced-colors focus = Highlight', async () => {
  const problems = [];
  const probe = async (page) => {
    const t = page.locator('.lf-grid .lf-item:not(.lf-more-item)').nth(0);
    const b = await t.boundingBox();
    await page.mouse.move(b.x + 20, b.y + 10);
    await page.waitForTimeout(650);
    return page.evaluate(() => {
      const p = document.querySelector('.lf-peek');
      const add = document.querySelector('.lf-item:hover .lf-add') || document.querySelector('.lf-add');
      const lf = document.querySelector('.lf');
      const ps = getComputedStyle(p);
      const as = getComputedStyle(add);
      const anims = document.getAnimations().filter((a) => a.playState === 'running' && a.effect && a.effect.target && lf.contains(a.effect.target)).map((a) => `${a.effect.target.className}:${a.animationName || a.transitionProperty || '?'}`);
      return { open: !p.hidden, pDur: ps.transitionDuration, pTr: ps.translate, addDur: as.transitionDuration, addScale: as.scale, anims };
    });
  };
  // 앱의 전역 모션 줄이기는 전환을 0.01ms 로 둔다 — 1ms 이하면 없는 것으로 본다
  const zero = (d) => d.split(',').every((x) => parseFloat(x) <= 0.001);
  {
    const { ctx, page } = await openHome({ count: 12 });
    const r = await probe(page);
    if (zero(r.pDur)) problems.push('보통 모션에서 카드 전환이 0 — 검사가 의미 없다');
    await ctx.close();
  }
  for (const [label, opts] of [['설정 절약', { motion: 'reduced' }], ['시스템 동작 줄이기', { reducedMotion: 'reduce' }]]) {
    const { ctx, page } = await openHome({ count: 12, ...opts });
    const r = await probe(page);
    if (!r.open) problems.push(`${label} 카드가 뜨지 않는다`);
    if (!zero(r.pDur) || (r.pTr !== 'none' && r.pTr !== '0px 0px' && r.pTr !== '0px')) problems.push(`${label} 카드 전환 ${r.pDur} / 이동 ${r.pTr}`);
    if (!zero(r.addDur) || (r.addScale !== 'none' && r.addScale !== '1')) problems.push(`${label} + 전환 ${r.addDur} / 크기 ${r.addScale}`);
    if (r.anims.length) problems.push(`${label} 도는 애니메이션 ${r.anims.join(',')}`);
    await ctx.close();
  }
  if (!WEBKIT) {
    const { ctx, page } = await openHome({ count: 12 });
    await page.emulateMedia({ forcedColors: 'active' });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    const fc = await page.evaluate(() => {
      const probe = document.createElement('i');
      probe.style.color = 'Highlight';
      document.body.append(probe);
      const hl = getComputedStyle(probe).color;
      probe.remove();
      const cs = getComputedStyle(document.activeElement);
      return { hl, outline: cs.outlineColor, style: cs.outlineStyle, width: cs.outlineWidth };
    });
    if (fc.outline !== fc.hl || fc.style === 'none' || parseFloat(fc.width) < 2) problems.push(`고대비 초점 ${JSON.stringify(fc)}`);
    await shot(page, 'forced-colors');
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

// ── LF07 빈 상태·실패 ───────────────────────────────────────────────────────
await check('LF07 empty (section stays with one-line notice) and failure (notice + retry, no section)', async () => {
  const problems = [];
  {
    const { ctx, page, errors } = await openHome({ count: 0 });
    const s = await page.evaluate(() => {
      const lf = document.querySelector('.home .lf');
      return lf && { text: lf.querySelector('.lf-empty').textContent.replace(/\s+/g, ' ').trim(), visible: !lf.querySelector('.lf-empty').hidden, all: Boolean(lf.querySelector('[data-goto="following"]')), grid: !lf.querySelector('.lf-grid').hidden, h: lf.getBoundingClientRect().height };
    });
    if (!s || !s.visible || !s.text.includes('지금 방송 중인 팔로잉 채널이 없어요') || !s.all || s.grid) problems.push(`빈 상태 ${JSON.stringify(s)}`);
    if (s && s.h > 130) problems.push(`빈 상태가 너무 크다 ${s.h}px`);
    if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  {
    const { ctx, page } = await openHome({ count: 5, failFollowing: true });
    const s = await page.evaluate(() => ({ notice: document.querySelector('.home .home-notice')?.textContent || '', retry: Boolean(document.querySelector('.home .home-notice [data-retry]')), lf: Boolean(document.querySelector('.home .lf')) }));
    if (!s.notice.includes('팔로잉 라이브를 불러오지 못했습니다') || !s.retry || s.lf) problems.push(`실패 ${JSON.stringify(s)}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

// ── LF08 갱신 깜박임 없음 ───────────────────────────────────────────────────
await check('LF08 refresh without flicker: home re-render and periodic update keep tile/img nodes, no image refetch, deferred reorder', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, recent: [] });
  let avatarReq = 0;
  page.on('request', (r) => { if (r.url().includes('/__img/avatar/')) avatarReq += 1; });
  // 표식
  await page.evaluate(() => {
    const lf = document.querySelector('.lf');
    lf.__mark = 'lf';
    for (const li of lf.querySelectorAll('.lf-grid .lf-item')) { li.__mark = li.dataset.key || 'more'; const img = li.querySelector('.lf-ava img'); if (img) img.__mark = li.dataset.key; }
  });
  const before = await page.evaluate(() => document.querySelector('.lf-grid .lf-item').getBoundingClientRect().toJSON());
  // 1) 다른 화면 → 홈 (홈 전체 다시 그리기)
  await go(page, 'following');
  await page.waitForTimeout(250);
  avatarReq = 0;
  await go(page, 'home');
  await page.waitForFunction(() => window.__lf.calls.filter(([c]) => c === 'following_lives').length >= 2);
  await page.waitForTimeout(400);
  const kept = await page.evaluate(() => {
    const lf = document.querySelector('.home .lf');
    const items = [...lf.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item)')];
    return {
      same: lf.__mark === 'lf',
      lost: items.filter((li) => li.__mark !== li.dataset.key).length,
      imgLost: items.filter((li) => { const img = li.querySelector('.lf-ava img'); return img && img.__mark !== li.dataset.key; }).length,
      rect: items[0].getBoundingClientRect().toJSON(),
    };
  });
  if (!kept.same || kept.lost || kept.imgLost) problems.push(`홈 다시 그리기 뒤 새 요소 — 구역 ${kept.same} 칸 ${kept.lost} 그림 ${kept.imgLost}`);
  if (avatarReq) problems.push(`홈 다시 그리기 뒤 아바타를 ${avatarReq}번 다시 받았다`);
  if (Math.abs(kept.rect.top - before.top) > 1 || Math.abs(kept.rect.left - before.left) > 1) problems.push(`첫 칸이 움직였다 ${before.top}→${kept.rect.top}`);
  // 2) 주기 갱신 — 데이터 바꾸고(시청자 수·끝난 방송·새 방송) 즐겨찾기 추가(같은 갱신 함수)로 부른다
  const plan = await page.evaluate(() => {
    const st = window.__lf;
    const keys = [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item):not([hidden])')].map((li) => li.dataset.key);
    const gone = keys[1];
    const bumped = keys[2];
    st.following = st.following.filter((l) => l.channelId !== gone).map((l) => (l.channelId === bumped ? { ...l, viewerCount: 99999, viewerCountText: '99,999' } : l));
    st.following.push({ ...st.live(80, 120000), channelId: 'ch80' });
    return { keys, gone: keys[1], bumped: keys[2], fav: keys[3] };
  });
  const mutations = await page.evaluateHandle(() => {
    const out = { imgAdded: 0, imgRemoved: 0 };
    const keepKeys = new Set();
    const mo = new MutationObserver((recs) => {
      for (const r of recs) {
        for (const n of r.addedNodes) if (n.nodeType === 1 && (n.tagName === 'IMG' || n.querySelector?.('img'))) { const li = r.target.closest?.('.lf-item'); if (li && li.__mark && li.__mark !== 'more') out.imgAdded += 1; }
        for (const n of r.removedNodes) if (n.nodeType === 1 && n.tagName === 'IMG') { const li = r.target.closest?.('.lf-item'); if (li && li.isConnected && li.__mark !== 'more') out.imgRemoved += 1; }
      }
    });
    mo.observe(document.querySelector('.lf-grid'), { childList: true, subtree: true });
    out.stop = () => mo.disconnect();
    out.keepKeys = keepKeys;
    return out;
  });
  avatarReq = 0;
  // 우클릭 → 즐겨찾기 추가 (마우스가 구역 위 → 자리 바꿈은 미룬다)
  const favTile = page.locator(`.lf-item[data-key="${plan.fav}"] .lf-open`);
  await favTile.click({ button: 'right' });
  await page.locator('.ctx-menu button', { hasText: '즐겨찾기 추가' }).click();
  await page.waitForFunction((id) => document.querySelector(`.lf-item[data-key="${id}"]`)?.classList.contains('is-fav'), plan.fav);
  const mid = await page.evaluate((plan) => {
    const items = [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item)')];
    const vis = items.filter((li) => !li.hidden).map((li) => li.dataset.key);
    return {
      lost: items.filter((li) => li.__mark && li.__mark !== li.dataset.key).length,
      gone: items.some((li) => li.dataset.key === plan.gone), added: items.some((li) => li.dataset.key === 'ch80'),
      bumped: document.querySelector(`.lf-item[data-key="${plan.bumped}"] .lf-vn`)?.textContent,
      count: document.querySelector('.lf .section-title .count').textContent, vis,
      hover: document.querySelector('.lf').matches(':hover'), focusIn: document.querySelector('.lf').contains(document.activeElement),
    };
  }, plan);
  const m1 = await mutations.evaluate((o) => ({ imgAdded: o.imgAdded, imgRemoved: o.imgRemoved }));
  if (mid.lost) problems.push(`갱신 뒤 남은 칸 ${mid.lost}개가 새 요소`);
  if (mid.gone || !mid.added) problems.push(`끝난 방송 빠짐 ${!mid.gone} · 새 방송 들어옴 ${mid.added}`);
  if (mid.bumped !== '99,999') problems.push(`시청자 수가 바뀌지 않았다 ${mid.bumped}`);
  if (mid.count !== '39') problems.push(`개수 배지 ${mid.count}`);
  if (m1.imgAdded || m1.imgRemoved) problems.push(`남은 칸의 그림 요소가 바뀌었다 +${m1.imgAdded} -${m1.imgRemoved}`);
  if (avatarReq > 3) problems.push(`갱신 때 아바타를 ${avatarReq}번 받았다(새 칸·더 보기 겹침만 허용)`);
  // 손이 구역에 있으면(마우스 또는 초점) 자리를 바꾸지 않는다 — 즐겨찾기한 칸이 아직 제자리
  const busy = mid.hover || mid.focusIn;
  const stillThere = mid.vis.indexOf(plan.fav) === plan.keys.filter((k) => k !== plan.gone).indexOf(plan.fav);
  if (busy && !stillThere) problems.push(`손이 구역에 있는데 자리가 바뀌었다 (${mid.vis.indexOf(plan.fav)})`);
  // 떠나면 제 순서 — 즐겨찾기가 맨 앞. 초점은 그대로 둔다(마우스로 연 메뉴가 닫히며 칸에 돌려준 초점 — 키보드 초점이 아니다).
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(250);
  const after = await page.evaluate(() => [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item)')].map((li) => li.dataset.key));
  if (after[0] !== plan.fav) problems.push(`떠난 뒤 즐겨찾기가 맨 앞이 아니다 ${after.slice(0, 3).join()}`);
  await mutations.evaluate((o) => o.stop());
  // 3) 마우스만 구역에 둔 채 갱신 → 자리 그대로, 초점이 빠져도 마우스가 있으면 그대로, 마우스가 떠나면(pointerleave) 제 순서
  const vis2 = await page.$$eval('.lf-grid .lf-item:not(.lf-more-item):not([hidden])', (els) => els.map((e) => e.dataset.key));
  const fav2 = vis2[vis2.length - 1];
  const b0 = await page.locator('.lf-grid .lf-item:not(.lf-more-item):not([hidden])').first().boundingBox();
  await page.mouse.move(b0.x + 40, b0.y + b0.height / 2);
  await page.locator(`.lf-item[data-key="${fav2}"] .lf-open`).focus();
  await page.keyboard.press('Shift+F10');
  await page.waitForSelector('.ctx-menu');
  await page.evaluate(() => [...document.querySelectorAll('.ctx-menu button')].find((b) => b.textContent.includes('즐겨찾기 추가')).click());
  await page.waitForFunction((id) => document.querySelector(`.lf-item[data-key="${id}"]`)?.classList.contains('is-fav'), fav2);
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  await page.waitForTimeout(150);
  const held = await page.evaluate(() => ({ keys: [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item):not([hidden])')].map((li) => li.dataset.key), hover: document.querySelector('.lf').matches(':hover') }));
  if (held.hover && held.keys.indexOf(fav2) !== vis2.indexOf(fav2)) problems.push(`마우스가 구역에 있는데 자리가 바뀌었다 ${vis2.indexOf(fav2)}→${held.keys.indexOf(fav2)}`);
  if (!held.hover) problems.push('검사 준비 실패 — 마우스가 구역 위가 아니다');
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(250);
  const left = await page.evaluate(() => [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item)')].map((li) => li.dataset.key));
  if (left.indexOf(fav2) > 1) problems.push(`마우스가 떠난 뒤에도 새 즐겨찾기가 앞으로 오지 않았다 (${left.indexOf(fav2)})`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

// ── LF09~LF16 — PR #120 검토에서 나온 문제의 회귀 검사 ─────────────────────────
const visTiles = (page) => page.locator('.lf-grid .lf-item:not(.lf-more-item):not([hidden])');
const peekState = (page) => page.evaluate(() => { const p = document.querySelector('.lf-peek'); return { open: !p.hidden, key: p.dataset.channelId || null }; });
const focusInfo = (page) => page.evaluate(() => {
  const a = document.activeElement;
  const li = a && a.closest ? a.closest('.lf-item') : null;
  return { tag: a ? a.tagName : null, cls: a && a.className ? String(a.className) : '', key: li ? (li.dataset.key || '__more') : null };
});

await check('LF09 Esc closes a hover-opened peek even when focus is on body; stays closed under the pointer; a context menu above closes first', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const t = visTiles(page).nth(1);
  const key = await t.getAttribute('data-key');
  const b = await t.boundingBox();
  await page.mouse.move(b.x + 30, b.y + b.height / 2);
  await page.waitForTimeout(650);
  let p = await peekState(page);
  if (!p.open || p.key !== key) problems.push(`호버 카드가 없다 ${JSON.stringify(p)}`);
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  const f = await focusInfo(page);
  if (f.tag !== 'BODY') problems.push(`초점이 body 가 아니다(검사 준비) ${JSON.stringify(f)}`);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  p = await peekState(page);
  if (p.open) problems.push('초점이 body 일 때 Esc 로 카드가 닫히지 않았다');
  await page.mouse.move(b.x + 40, b.y + b.height / 2 + 2); // 같은 칸 안에서 움직여도
  await page.waitForTimeout(700);
  if ((await peekState(page)).open) problems.push('Esc 로 닫은 카드가 포인터 아래에서 다시 떴다');
  // 카드 메뉴가 위에 있으면 Esc 는 메뉴부터
  const t2 = visTiles(page).nth(2);
  const b2 = await t2.boundingBox();
  await page.mouse.move(b2.x + 30, b2.y + b2.height / 2);
  await page.waitForTimeout(650);
  if (!(await peekState(page)).open) problems.push('다른 칸의 카드가 뜨지 않았다');
  await page.mouse.click(b2.x + 30, b2.y + b2.height / 2, { button: 'right' });
  await page.waitForSelector('.ctx-menu');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  const s1 = await page.evaluate(() => ({ menu: Boolean(document.querySelector('.ctx-menu')), peek: !document.querySelector('.lf-peek').hidden }));
  if (s1.menu) problems.push('Esc 에 카드 메뉴가 닫히지 않았다');
  if (!s1.peek) problems.push('메뉴를 닫는 Esc 가 카드까지 닫았다(메뉴부터 하나씩이어야)');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  if ((await peekState(page)).open) problems.push('두 번째 Esc 에 카드가 닫히지 않았다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF10 collapsing keeps the "접기" → "+N" button where it was on screen (no jump, scroll position kept)', async () => {
  const problems = [];
  const { ctx, page } = await openHome({ count: 39, width: 1280, height: 700 });
  await page.locator('.lf-more').click();
  await page.locator('.lf-more').scrollIntoViewIfNeeded();
  await page.evaluate(() => { const sc = document.querySelector('.page-home'); sc.scrollTop += 120; }); // 단추를 화면 가운데쯤
  await page.waitForTimeout(100);
  const before = await page.evaluate(() => ({ top: document.querySelector('.lf-more').getBoundingClientRect().top, label: document.querySelector('.lf-more-count').textContent }));
  if (before.label !== '접기' || before.top < 0 || before.top > 700) problems.push(`검사 준비 실패 ${JSON.stringify(before)}`);
  await page.locator('.lf-more').click();
  await page.waitForTimeout(100);
  const after = await page.evaluate(() => ({ top: document.querySelector('.lf-more').getBoundingClientRect().top, bottom: document.querySelector('.lf-more').getBoundingClientRect().bottom, label: document.querySelector('.lf-more-count').textContent }));
  if (!after.label.startsWith('+')) problems.push(`접히지 않았다 ${after.label}`);
  if (Math.abs(after.top - before.top) > 2) problems.push(`단추가 ${Math.round(before.top)} → ${Math.round(after.top)}px 로 움직였다`);
  if (after.top < 0 || after.bottom > 700) problems.push('접은 뒤 단추가 화면 밖이다');
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF11 after mouse clicks on sort / 더 보기 / 접기 and leaving, the 2-minute refresh reorders (new favorite & new live not buried behind +N)', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, clock: true });
  await page.locator('[data-lf-sort="viewers"]').click();
  await page.locator('[data-lf-sort="mine"]').click();
  await page.locator('.lf-more').click();
  await page.locator('.lf-more').click();
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(150);
  const f = await focusInfo(page);
  // 새 즐겨찾기(접힌 뒤쪽 채널)·새 방송(시청자 가장 많음)
  await page.evaluate(() => {
    const st = window.__lf;
    st.favs.add('ch30');
    st.following.push({ ...st.live(90, 150000), channelId: 'ch90' });
  });
  await page.clock.fastForward(121000);
  await page.waitForFunction(() => document.querySelector('.lf-item[data-key="ch90"]'), null, { timeout: 4000 }).catch(() => {});
  await page.waitForTimeout(200);
  const r = await page.evaluate(() => {
    const items = [...document.querySelectorAll('.lf-grid .lf-item:not(.lf-more-item)')];
    const at = (k) => items.findIndex((li) => li.dataset.key === k);
    const vis = (k) => { const li = items[at(k)]; return Boolean(li && !li.hidden); };
    return { fav: at('ch30'), favVis: vis('ch30'), fresh: at('ch90'), freshVis: vis('ch90') };
  });
  if (r.fresh < 0) problems.push('2분 갱신이 오지 않았다(새 방송 없음)');
  if (r.fav !== 0 || !r.favVis) problems.push(`새 즐겨찾기가 맨 앞에 보이지 않는다 ${JSON.stringify(r)} (초점 ${JSON.stringify(f)})`);
  if (r.fresh !== 1 || !r.freshVis) problems.push(`새 방송이 즐겨찾기 다음에 보이지 않는다 ${JSON.stringify(r)} (초점 ${JSON.stringify(f)})`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF12 a skeleton re-render (network recovery auto reload) gives keyboard focus back to the same tile', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('ArrowRight');
  const f0 = await focusInfo(page);
  const calls0 = await page.evaluate(() => window.__lf.calls.filter(([c]) => c === 'following_lives').length);
  await page.evaluate(() => { window.__sawSkeleton = false; new MutationObserver(() => { if (document.querySelector('.lf-skeleton')) window.__sawSkeleton = true; }).observe(document.body, { childList: true, subtree: true }); });
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await page.waitForFunction((n) => window.__lf.calls.filter(([c]) => c === 'following_lives').length > n && document.querySelector('.home .lf:not(.lf-skeleton)'), calls0, { timeout: 6000 });
  await page.waitForTimeout(400);
  const f1 = await focusInfo(page);
  const sk = await page.evaluate(() => window.__sawSkeleton);
  if (!sk) problems.push('검사 준비 실패 — 스켈레톤을 거치지 않았다');
  if (!f0.key || f1.key !== f0.key || !f1.cls.includes('lf-open')) problems.push(`초점이 돌아오지 않았다 ${JSON.stringify(f0)} → ${JSON.stringify(f1)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF13 when a focused tile gets hidden (window narrows), focus moves to "+N 더 보기" instead of falling to body', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1600 });
  const s0 = await gridState(page);
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  for (let i = 0; i < s0.visible - 1; i += 1) await page.keyboard.press('ArrowRight');
  const f0 = await focusInfo(page);
  await page.setViewportSize({ width: 1280, height: 900 }); // 홈 1004 → 684px(오른쪽 패널 때문에 1180 은 오히려 넓다)
  await page.waitForTimeout(300);
  const s1 = await gridState(page);
  const f1 = await focusInfo(page);
  if (s1.visible >= s0.visible) problems.push(`검사 준비 실패 — 칸이 줄지 않았다 ${s0.visible}→${s1.visible}`);
  if (s1.visKeys.includes(f0.key)) problems.push('검사 준비 실패 — 초점 칸이 그대로 보인다');
  if (f1.key !== '__more') problems.push(`초점이 더 보기로 가지 않았다 ${JSON.stringify(f0)} → ${JSON.stringify(f1)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF14 arrow navigation that scrolls the page still opens the peek for the newly focused tile', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, height: 560 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('End');
  await page.keyboard.press('Enter'); // 펼치고 처음 드러난 칸으로
  await page.waitForTimeout(600);
  if (!(await peekState(page)).open) problems.push('검사 준비 실패 — 키보드 초점 카드가 없다');
  let scrolled = false;
  for (let i = 0; i < 10 && !scrolled; i += 1) {
    const y0 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(650);
    const y1 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    if (y1 !== y0) {
      scrolled = true;
      const f = await focusInfo(page);
      const p = await peekState(page);
      if (!p.open || p.key !== f.key) problems.push(`스크롤한 화살표 이동 뒤 새 칸 카드가 없다 ${JSON.stringify({ focus: f.key, peek: p })}`);
    }
  }
  if (!scrolled) problems.push('검사 준비 실패 — 화살표로 스크롤되지 않았다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF15 count badge stays opaque with ≥ 4.5:1 text when color-mix is unsupported (Safari 16.0~16.1 emulation) — dark & light', async () => {
  const problems = [];
  const report = [];
  for (const emulate of [false, true]) {
    for (const theme of ['dark', 'light']) {
      const ctx0 = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: theme, deviceScaleFactor: 1 });
      const page = await ctx0.newPage();
      if (emulate) {
        // color-mix 를 모르는 엔진 흉내 — CSS 의 color-mix( 를 xcolor-mix( 로(@supports 조건도 함께 거짓이 된다)
        await page.route('**/css/*.css', async (route) => {
          const res = await route.fetch();
          const body = (await res.text()).replace(/color-mix\(/g, 'xcolor-mix(');
          await route.fulfill({ response: res, body, headers: { ...res.headers(), 'content-type': 'text/css' } });
        });
      }
      await page.addInitScript(mockLF, [base, { count: 12, theme }]);
      await page.goto(base);
      await page.waitForSelector('.home .lf .section-title .count:not([hidden])');
      await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
      if (theme === 'light') await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
      await page.mouse.move(2, 890);
      await page.waitForTimeout(200);
      await page.evaluate(colorKit);
      const bg = await page.evaluate(() => window.__ck.parse(getComputedStyle(document.querySelector('.lf .section-title .count')).backgroundColor));
      const tag = `${emulate ? 'color-mix 없음' : '보통'} ${theme}`;
      if (bg[3] < 1) problems.push(`${tag} 배지 바탕이 불투명하지 않다 ${JSON.stringify(bg)}`);
      await measureAll(page, [['.lf .section-title .count', 4.5]], tag, problems, report);
      await ctx0.close();
    }
  }
  if (process.env.CVIEW_LF_REPORT) console.log(report.join('\n'));
  ok(!problems.length, problems.join(' / '));
});

await check('LF16 when the home loses the section slot (following fails after reload), the strip lets go of the old home tree and closes its peek', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  await page.evaluate(() => { window.__lfEl = document.querySelector('.home .lf'); });
  const t = visTiles(page).nth(0);
  const b = await t.boundingBox();
  await page.mouse.move(b.x + 30, b.y + b.height / 2);
  await page.waitForTimeout(650);
  if (!(await peekState(page)).open) problems.push('검사 준비 실패 — 카드가 없다');
  await page.evaluate(() => { window.__lf.fail = true; window.dispatchEvent(new Event('online')); });
  await page.waitForSelector('.home .home-notice', { timeout: 6000 });
  await page.waitForTimeout(200);
  const r = await page.evaluate(() => ({
    connected: window.__lfEl.isConnected, parent: window.__lfEl.parentNode ? window.__lfEl.parentNode.className || window.__lfEl.parentNode.nodeName : null,
    peekHidden: window.__lfEl.querySelector('.lf-peek').hidden, inHome: Boolean(document.querySelector('.home .lf')),
  }));
  if (r.connected || r.inHome) problems.push(`실패한 홈에 구역이 남았다 ${JSON.stringify(r)}`);
  if (r.parent !== null) problems.push(`떨어진 옛 홈(${r.parent})을 붙잡고 있다`);
  if (!r.peekHidden) problems.push('카드가 열린 채 남았다');
  // 다시 되면 같은 구역이 돌아온다
  await page.evaluate(() => { window.__lf.fail = false; window.dispatchEvent(new Event('online')); });
  await page.waitForFunction(() => document.querySelector('.home .lf:not(.lf-skeleton)'), null, { timeout: 6000 });
  const back = await page.evaluate(() => document.querySelector('.home .lf:not(.lf-skeleton)') === window.__lfEl);
  if (!back) problems.push('복구 뒤 같은 구역 요소가 돌아오지 않았다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

// ── LF17~LF24 — v0.1.103(PR #120 수정본) 2차 검증에서 나온 문제의 회귀 검사 ───────────────
const followingCalls = (page) => page.evaluate(() => window.__lf.calls.filter(([c]) => c === 'following_lives').length);
/** 사이드바 2분 주기 갱신(app.js refreshSidebarFollowing → 홈 updateLiveFollowing)을 가짜 시계로 당긴다. */
async function twoMinuteRefresh(page) {
  const n = await followingCalls(page);
  await page.clock.fastForward(121000);
  await page.waitForFunction((k) => window.__lf.calls.filter(([c]) => c === 'following_lives').length > k, n, { timeout: 4000 }).catch(() => {});
  await page.waitForTimeout(200);
  return (await followingCalls(page)) > n;
}
const order = (page) => page.$$eval('.lf-grid .lf-item:not(.lf-more-item)', (els) => els.map((e) => ({ key: e.dataset.key, hidden: e.hidden })));

await check('LF17 a stray key (Esc on a hover card, Shift, PageDown) after mouse clicks on 더 보기 / 접기 / 정렬 does not freeze the 2-minute reorder', async () => {
  const problems = [];
  const variants = [
    { label: '더 보기·접기 + Esc(호버 카드 닫기)', clicks: ['.lf-more', '.lf-more'], key: 'Escape', hoverCard: true },
    { label: '더 보기·접기 + Shift', clicks: ['.lf-more', '.lf-more'], key: 'Shift' },
    { label: '더 보기 + PageDown', clicks: ['.lf-more'], key: 'PageDown' },
    { label: '정렬 + Shift', clicks: ['[data-lf-sort="viewers"]', '[data-lf-sort="mine"]'], key: 'Shift', sortFocusOptional: true },
  ];
  for (const v of variants) {
    const { ctx, page, errors } = await openHome({ count: 39, width: 1280, clock: true });
    for (const sel of v.clicks) await page.locator(sel).click();
    if (v.hoverCard) {
      await visTiles(page).nth(2).scrollIntoViewIfNeeded(); // 접기 단추를 누르느라 내려간 화면 — 첫 줄이 위로 밀려 있을 수 있다
      await page.waitForTimeout(100);
      const b = await visTiles(page).nth(2).boundingBox();
      await page.mouse.move(b.x + 30, b.y + b.height / 2);
      await page.waitForTimeout(650);
      if (!(await peekState(page)).open) problems.push(`${v.label}: 검사 준비 실패 — 호버 카드가 없다`);
    }
    const f0 = await focusInfo(page);
    const inLf = await page.evaluate(() => document.querySelector('.lf').contains(document.activeElement));
    // WebKit 은 tabindex 없는 단추(정렬)를 마우스로 눌러도 초점을 주지 않는다 — 그 엔진에선 이 변형이 원래 문제를 만들지 않는다
    if (!inLf && !(v.sortFocusOptional && WEBKIT)) problems.push(`${v.label}: 검사 준비 실패 — 누른 단추에 초점이 없다 ${JSON.stringify(f0)}`);
    await page.keyboard.press(v.key);
    await page.waitForTimeout(80);
    await page.mouse.move(2, 880, { steps: 3 });
    await page.waitForTimeout(200);
    await page.evaluate(() => {
      const st = window.__lf;
      st.favs.add('ch30');
      st.following.push({ ...st.live(90, 150000), channelId: 'ch90' });
    });
    if (!(await twoMinuteRefresh(page))) problems.push(`${v.label}: 2분 갱신이 오지 않았다`);
    const o = await order(page);
    const at = (k) => o.findIndex((x) => x.key === k);
    if (at('ch30') !== 0 || o[0].hidden) problems.push(`${v.label}: 새 즐겨찾기가 맨 앞이 아니다(${at('ch30')}) — 초점 ${JSON.stringify(f0)}`);
    if (at('ch90') !== 1 || o[1].hidden) problems.push(`${v.label}: 새 방송이 둘째가 아니다(${at('ch90')})`);
    if (errors.length) problems.push(`${v.label}: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF18 arrow keys after a mouse click on "+N 더 보기" count as keyboard use — the 2-minute refresh keeps the focused tile in place until focus leaves', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, clock: true });
  await page.locator('.lf-more').click(); // 펼친다 — 초점은 '접기'에 남는다(마우스)
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(150);
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(100);
  const f0 = await focusInfo(page);
  const o0 = await order(page);
  const i0 = o0.findIndex((x) => x.key === f0.key);
  if (!f0.cls.includes('lf-open') || i0 < 0) problems.push(`검사 준비 실패 — 화살표로 칸에 가지 않았다 ${JSON.stringify(f0)}`);
  await page.evaluate(() => {
    const st = window.__lf;
    st.favs.add('ch20');
    st.following.push({ ...st.live(90, 150000), channelId: 'ch90' });
  });
  if (!(await twoMinuteRefresh(page))) problems.push('2분 갱신이 오지 않았다');
  const f1 = await focusInfo(page);
  const o1 = await order(page);
  const i1 = o1.findIndex((x) => x.key === f0.key);
  if (i1 !== i0) problems.push(`키보드 초점 칸이 손 아래에서 옮겨 갔다 ${i0} → ${i1}`);
  if (f1.key !== f0.key) problems.push(`초점이 바뀌었다 ${f0.key} → ${f1.key}`);
  if (o1.findIndex((x) => x.key === 'ch90') !== o1.length - 1) problems.push('바쁜 동안 새 방송이 끝에 오지 않았다');
  // 초점이 구역을 떠나면 제 순서로
  await page.locator('.home-search input').focus();
  await page.waitForTimeout(150);
  const o2 = await order(page);
  if (o2[0].key !== 'ch20' || o2[1].key !== 'ch90') problems.push(`초점이 떠난 뒤 제 순서가 아니다 ${o2.slice(0, 3).map((x) => x.key).join()}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF19 leaving home (shortcut / menu navigation, pointer not moved) closes the peek and the next screen gets its first Esc', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const b = await visTiles(page).nth(1).boundingBox();
  await page.mouse.move(b.x + 30, b.y + b.height / 2);
  await page.waitForTimeout(650);
  if (!(await peekState(page)).open) problems.push('검사 준비 실패 — 호버 카드가 없다');
  await go(page, 'following'); // 단축키(⌘2·⌘[)·네이티브 메뉴와 같은 cview:navigate 길 — 포인터는 그대로
  await page.waitForTimeout(60);
  const s = await page.evaluate(() => ({ homeHidden: document.querySelector('.page-home').hidden, peek: !document.querySelector('.lf-peek').hidden }));
  if (!s.homeHidden) problems.push('검사 준비 실패 — 홈이 숨지 않았다');
  if (s.peek) problems.push('홈을 떠났는데 숨은 홈의 카드가 열린 채 남았다');
  await page.waitForTimeout(300);
  await page.evaluate(() => {
    window.__esc = [];
    window.addEventListener('keydown', (e) => { if (e.key === 'Escape') window.__esc.push(e.defaultPrevented); });
  });
  await page.keyboard.press('Escape');
  await page.waitForTimeout(50);
  const esc = await page.evaluate(() => window.__esc);
  if (esc.length !== 1 || esc[0]) problems.push(`다음 화면의 첫 Esc 를 숨은 홈이 가로챘다 ${JSON.stringify(esc)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF20 Esc in a keyboard-opened account menu closes that menu first, not the hover card behind it', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const b = await visTiles(page).nth(1).boundingBox();
  await page.mouse.move(b.x + 30, b.y + b.height / 2);
  await page.waitForTimeout(650);
  if (!(await peekState(page)).open) problems.push('검사 준비 실패 — 호버 카드가 없다');
  await page.locator('.account-trigger').focus();
  await page.keyboard.press('Enter');
  await page.waitForSelector('.account-pop');
  const s0 = await page.evaluate(() => ({ inMenu: Boolean(document.activeElement.closest('.account-pop')), peek: !document.querySelector('.lf-peek').hidden }));
  if (!s0.inMenu) problems.push('검사 준비 실패 — 계정 메뉴에 초점이 없다');
  if (!s0.peek) problems.push('검사 준비 실패 — 메뉴를 여는 동안 카드가 닫혔다');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(80);
  const s1 = await page.evaluate(() => ({ menu: Boolean(document.querySelector('.account-pop')), trigger: document.activeElement.matches('.account-trigger') }));
  if (s1.menu) problems.push('Esc 에 계정 메뉴가 닫히지 않았다(뒤의 카드가 먼저 가로챘다)');
  else if (!s1.trigger) problems.push('계정 메뉴를 닫은 뒤 초점이 단추로 돌아오지 않았다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF21 a wheel scroll right after moving to the next tile does not pop the card of a tile that scrolled away', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, height: 700 });
  const t0 = await visTiles(page).nth(0).boundingBox();
  const t1 = await visTiles(page).nth(1).boundingBox();
  const key1 = await visTiles(page).nth(1).getAttribute('data-key');
  await page.mouse.move(t0.x + 30, t0.y + t0.height / 2);
  await page.waitForTimeout(650);
  if (!(await peekState(page)).open) problems.push('검사 준비 실패 — 첫 칸 카드가 없다');
  const x = t1.x + 30;
  const y = t1.y + t1.height / 2;
  await page.mouse.move(x, y); // 카드가 열린 채 옆 칸 — 90ms 뒤 바꿀 예약
  await page.mouse.wheel(0, 320); // 그 사이 휠로 내린다
  await page.waitForTimeout(500);
  const r = await page.evaluate(([px, py]) => {
    const p = document.querySelector('.lf-peek');
    const under = document.elementFromPoint(px, py)?.closest?.('.lf-item')?.dataset.key || null;
    const sc = document.querySelector('.page-home').scrollTop;
    return { open: !p.hidden, key: p.dataset.channelId || null, under, sc };
  }, [x, y]);
  if (r.sc < 100) problems.push(`검사 준비 실패 — 휠로 스크롤되지 않았다 ${r.sc}`);
  if (r.under === key1) problems.push('검사 준비 실패 — 스크롤 뒤에도 같은 칸이 포인터 아래다');
  if (r.open && r.key !== r.under) problems.push(`포인터 아래가 아닌 칸(${r.key})의 카드가 떴다(포인터 아래 ${r.under})`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF22 when "+N 더 보기" / "접기" holding focus disappears (wider window, ended broadcast), focus moves to a nearby tile instead of body', async () => {
  const problems = [];
  const onTile = (f) => f.cls.includes('lf-open') && f.key && f.key !== '__more';
  // 1) 7개·1280px(3칸) — 더 보기에 초점, 창이 넓어져(4칸) 넘침이 없어진다
  {
    const { ctx, page, errors } = await openHome({ count: 7, width: 1280 });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('End');
    const f0 = await focusInfo(page);
    if (f0.key !== '__more') problems.push(`더 보기: 검사 준비 실패 ${JSON.stringify(f0)}`);
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.waitForTimeout(300);
    const s = await gridState(page);
    const f1 = await focusInfo(page);
    if (s.more) problems.push('더 보기: 검사 준비 실패 — 넘침이 남았다');
    if (!onTile(f1)) problems.push(`더 보기가 사라지며 초점이 칸으로 가지 않았다 ${JSON.stringify(f1)}`);
    else if (!s.visKeys.includes(f1.key)) problems.push(`초점 칸이 보이지 않는다 ${f1.key}`);
    if (errors.length) problems.push(`더 보기: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 2) 같은 조건에서 펼친 상태의 '접기'
  {
    const { ctx, page, errors } = await openHome({ count: 7, width: 1280 });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('End');
    await page.keyboard.press('Enter'); // 펼친다
    await page.keyboard.press('End'); // '접기'
    const f0 = await focusInfo(page);
    const exp = await page.locator('.lf-more').getAttribute('aria-expanded');
    if (f0.key !== '__more' || exp !== 'true') problems.push(`접기: 검사 준비 실패 ${JSON.stringify(f0)} ${exp}`);
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.waitForTimeout(300);
    const f1 = await focusInfo(page);
    if (!onTile(f1)) problems.push(`'접기'가 사라지며 초점이 칸으로 가지 않았다 ${JSON.stringify(f1)}`);
    if (errors.length) problems.push(`접기: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 3) 9개·1600px(4칸, +2) — 더 보기에 초점, 2분 갱신에서 방송 하나가 끝나 8개
  {
    const { ctx, page, errors } = await openHome({ count: 9, width: 1600, clock: true });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('End');
    const f0 = await focusInfo(page);
    const s0 = await gridState(page);
    if (f0.key !== '__more' || !s0.more) problems.push(`종료: 검사 준비 실패 ${JSON.stringify(f0)} ${JSON.stringify(s0.more)}`);
    await page.evaluate(() => { const st = window.__lf; st.following = st.following.filter((l) => l.channelId !== 'ch2'); });
    if (!(await twoMinuteRefresh(page))) problems.push('종료: 2분 갱신이 오지 않았다');
    const s1 = await gridState(page);
    const f1 = await focusInfo(page);
    if (s1.more || s1.total !== 8) problems.push(`종료: 검사 준비 실패 — ${s1.total}개 더 보기 ${JSON.stringify(s1.more)}`);
    if (!onTile(f1)) problems.push(`방송이 끝나 더 보기가 사라지며 초점이 칸으로 가지 않았다 ${JSON.stringify(f1)}`);
    if (errors.length) problems.push(`종료: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF23 with the mouse resting on the grid, arrow keys that scroll the page still open the focused tile\'s card (synthetic pointer events ignored)', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, height: 560 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('End');
  await page.keyboard.press('Enter'); // 펼치고 처음 드러난 칸으로
  await page.waitForTimeout(400);
  // 포인터를 격자 위 화면 위쪽(초점 칸이 아닌 칸)에 둔다 — 아래로 스크롤해도 펼친 격자가 계속 그 아래에 있다. 그 칸의 호버 카드가 떠도 괜찮다.
  const g = await page.locator('.lf-grid').boundingBox();
  await page.mouse.move(g.x + g.width - 40, Math.max(g.y + 30, 150), { steps: 3 });
  await page.waitForTimeout(700);
  let scrolled = 0;
  for (let i = 0; i < 12 && scrolled < 3; i += 1) {
    const y0 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(650);
    const y1 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    if (y1 === y0) continue;
    scrolled += 1;
    const f = await focusInfo(page);
    const p = await peekState(page);
    // 포인터는 격자나, 초점 칸 위로 뜬 카드 위에 있다(둘 다 합성 경계 이벤트를 받는다)
    const hover = await page.evaluate(() => document.querySelector('.lf').matches(':hover'));
    if (!hover) problems.push('검사 준비 실패 — 포인터가 구역 위가 아니다');
    if (!p.open || p.key !== f.key) problems.push(`스크롤한 화살표 이동 뒤 초점 칸(${f.key})이 아닌 카드 ${JSON.stringify(p)}`);
  }
  if (!scrolled) problems.push('검사 준비 실패 — 화살표로 스크롤되지 않았다');
  // 실제로 손을 움직이면 호버가 다시 맡는다
  const t = visTiles(page).nth(0);
  await t.scrollIntoViewIfNeeded();
  const tb = await t.boundingBox();
  await page.mouse.move(tb.x + 30, tb.y + tb.height / 2, { steps: 3 });
  await page.waitForTimeout(650);
  const p2 = await peekState(page);
  if (!p2.open || p2.key !== (await t.getAttribute('data-key'))) problems.push(`마우스를 움직인 칸의 카드가 뜨지 않았다 ${JSON.stringify(p2)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF24 a skeleton re-render whose remembered tile is now behind "+N 더 보기" or gone puts focus on 더 보기 / a nearby tile, not body', async () => {
  const problems = [];
  const reloadViaOnline = async (page) => {
    const n = await followingCalls(page);
    await page.evaluate(() => window.dispatchEvent(new Event('online')));
    await page.waitForFunction((k) => window.__lf.calls.filter(([c]) => c === 'following_lives').length > k && document.querySelector('.home .lf:not(.lf-skeleton)'), n, { timeout: 6000 });
    await page.waitForTimeout(400);
  };
  // 1) 초점 칸(시청자 1위)이 다시 받은 목록에서 꼴찌가 되어 '+N 더 보기' 뒤로
  {
    const { ctx, page, errors } = await openHome({ count: 39, width: 1280 });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    const f0 = await focusInfo(page);
    await page.evaluate((k) => { const st = window.__lf; st.following = st.following.map((l) => (l.channelId === k ? { ...l, viewerCount: 1, viewerCountText: '1' } : l)); }, f0.key);
    await reloadViaOnline(page);
    const o = await order(page);
    const moved = o.find((x) => x.key === f0.key);
    if (!moved || !moved.hidden) problems.push(`밀림: 검사 준비 실패 — ${f0.key} 가 숨지 않았다`);
    const f1 = await focusInfo(page);
    if (f1.key !== '__more') problems.push(`기억한 칸이 더 보기 뒤로 밀렸는데 초점이 더 보기로 가지 않았다 ${JSON.stringify(f1)}`);
    if (errors.length) problems.push(`밀림: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 2) 초점 칸의 방송이 끝났다
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    const f0 = await focusInfo(page);
    await page.evaluate((k) => { const st = window.__lf; st.following = st.following.filter((l) => l.channelId !== k); }, f0.key);
    await reloadViaOnline(page);
    const f1 = await focusInfo(page);
    const vis = (await gridState(page)).visKeys;
    if (!f1.cls.includes('lf-open') || !vis.includes(f1.key)) problems.push(`기억한 칸이 사라졌는데 초점이 가까운 칸으로 가지 않았다 ${JSON.stringify(f1)}`);
    else if (vis.indexOf(f1.key) !== 2) problems.push(`같은 자리(2)의 칸이 아니다 ${vis.indexOf(f1.key)}`);
    if (errors.length) problems.push(`사라짐: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF25 Esc still closes the card while a mouse-opened account menu is up; a second Esc during the palette\'s closing ghost closes the card', async () => {
  const problems = [];
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
    await page.locator('.account-trigger').click(); // 마우스로 연다 — 초점이 메뉴로 가지 않고, 메뉴는 제 Esc 처리가 없다
    await page.waitForSelector('.account-pop');
    const b = await visTiles(page).nth(1).boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height / 2, { steps: 3 });
    await page.waitForTimeout(650);
    const s0 = await page.evaluate(() => ({ menu: Boolean(document.querySelector('.account-pop')), peek: !document.querySelector('.lf-peek').hidden, inMenu: Boolean(document.activeElement.closest('.account-pop')) }));
    if (!s0.menu || !s0.peek || s0.inMenu) problems.push(`계정 메뉴: 검사 준비 실패 ${JSON.stringify(s0)}`);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(80);
    if ((await peekState(page)).open) problems.push('마우스로 연 계정 메뉴가 떠 있으면 카드가 Esc 로 닫히지 않는다');
    if (errors.length) problems.push(`계정 메뉴: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
    const b = await visTiles(page).nth(1).boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height / 2);
    await page.waitForTimeout(650);
    await page.keyboard.press('Meta+k');
    await page.waitForSelector('.palette-scrim:not([hidden])');
    await page.waitForTimeout(300);
    if (!(await peekState(page)).open) problems.push('팔레트: 검사 준비 실패 — 카드가 팔레트 아래에 남지 않았다');
    await page.keyboard.press('Escape'); // 팔레트를 닫는다(닫히는 잔상 .exit-ghost 가 잠시 남는다)
    await page.waitForTimeout(40);
    const ghost = await page.evaluate(() => Boolean(document.querySelector('.palette-scrim.exit-ghost')));
    await page.keyboard.press('Escape');
    await page.waitForTimeout(40);
    const p = await peekState(page);
    if (p.open) problems.push(`팔레트가 닫히는 잔상 중의 두 번째 Esc 가 카드를 닫지 않았다 (잔상 ${ghost})`);
    if (errors.length) problems.push(`팔레트: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF26 focus handed back by a menu / palette closed with Esc is not keyboard use — the 2-minute reorder still happens after the mouse leaves', async () => {
  const problems = [];
  const variants = [
    {
      label: '우클릭 메뉴 + Esc',
      run: async (page) => {
        const b = await visTiles(page).nth(2).boundingBox();
        await page.mouse.click(b.x + 30, b.y + b.height / 2, { button: 'right' });
        await page.waitForSelector('.ctx-menu');
        await page.keyboard.press('Escape');
        await page.waitForTimeout(80);
        if (await page.locator('.ctx-menu').count()) problems.push('우클릭 메뉴 + Esc: 검사 준비 실패 — 메뉴가 닫히지 않았다');
      },
    },
    {
      label: '더 보기 + ⌘K + Esc',
      run: async (page) => {
        await page.locator('.lf-more').click();
        await page.keyboard.press('Meta+k');
        await page.waitForSelector('.palette-scrim:not([hidden])');
        await page.keyboard.press('Escape');
        await page.waitForTimeout(300);
      },
    },
  ];
  for (const v of variants) {
    const { ctx, page, errors } = await openHome({ count: 39, width: 1280, clock: true });
    await v.run(page);
    const f0 = await focusInfo(page);
    await page.mouse.move(2, 880, { steps: 3 });
    await page.waitForTimeout(200);
    await page.evaluate(() => {
      const st = window.__lf;
      st.favs.add('ch30');
      st.following.push({ ...st.live(90, 150000), channelId: 'ch90' });
    });
    if (!(await twoMinuteRefresh(page))) problems.push(`${v.label}: 2분 갱신이 오지 않았다`);
    const o = await order(page);
    const at = (k) => o.findIndex((x) => x.key === k);
    if (at('ch30') !== 0 || at('ch90') !== 1) problems.push(`${v.label}: 돌려받은 초점(${JSON.stringify(f0)})에 자리 바꿈이 멈췄다 — 즐겨찾기 ${at('ch30')} · 새 방송 ${at('ch90')}`);
    if (errors.length) problems.push(`${v.label}: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF27 a context menu opened from a tile (Shift+F10 / right-click) keeps the deferred reorder until it closes; Esc returns focus to that tile', async () => {
  const problems = [];
  const lowerCh2 = (page) => page.evaluate(() => { const st = window.__lf; st.following = st.following.map((l) => (l.channelId === 'ch2' ? { ...l, viewerCount: 5, viewerCountText: '5' } : l)); });
  const ch2 = async (page) => { const o = await order(page); const i = o.findIndex((x) => x.key === 'ch2'); return { i, hidden: i >= 0 && o[i].hidden }; };
  // 키보드
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280, clock: true });
    await page.locator('.lf-all').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    const f0 = await focusInfo(page);
    if (f0.key !== 'ch2') problems.push(`키보드: 검사 준비 실패 ${JSON.stringify(f0)}`);
    await lowerCh2(page);
    if (!(await twoMinuteRefresh(page))) problems.push('키보드: 2분 갱신이 오지 않았다');
    let c = await ch2(page);
    if (c.i !== 2 || c.hidden) problems.push(`키보드: 검사 준비 실패 — 갱신이 미뤄지지 않았다 ${JSON.stringify(c)}`);
    await page.keyboard.press('Shift+F10');
    await page.waitForSelector('.ctx-menu');
    await page.waitForTimeout(200);
    c = await ch2(page);
    if (c.i !== 2 || c.hidden) problems.push(`키보드: 메뉴가 떠 있는 동안 칸이 옮겨 갔다 ${JSON.stringify(c)}`);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(100);
    const f1 = await focusInfo(page);
    if (f1.key !== 'ch2' || !f1.cls.includes('lf-open')) problems.push(`키보드: 메뉴를 닫은 뒤 초점이 그 칸이 아니다 ${JSON.stringify(f1)}`);
    await page.locator('.home-search input').focus();
    await page.waitForTimeout(150);
    c = await ch2(page);
    if (!c.hidden) problems.push(`키보드: 초점이 떠난 뒤에도 미룬 자리 바꿈을 하지 않았다 ${JSON.stringify(c)}`);
    if (errors.length) problems.push(`키보드: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 마우스 — 우클릭 뒤 포인터를 메뉴로 옮긴다(구역을 떠난 것처럼 보인다)
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280, clock: true });
    const t = page.locator('.lf-item[data-key="ch2"]');
    const b = await t.boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height / 2, { steps: 3 });
    await lowerCh2(page);
    if (!(await twoMinuteRefresh(page))) problems.push('마우스: 2분 갱신이 오지 않았다');
    let c = await ch2(page);
    if (c.i !== 2 || c.hidden) problems.push(`마우스: 검사 준비 실패 — 갱신이 미뤄지지 않았다 ${JSON.stringify(c)}`);
    await page.mouse.click(b.x + 30, b.y + b.height / 2, { button: 'right' });
    await page.waitForSelector('.ctx-menu');
    const m = await page.locator('.ctx-menu .ctx-item').last().boundingBox();
    await page.mouse.move(m.x + m.width / 2, m.y + m.height / 2, { steps: 4 });
    await page.waitForTimeout(200);
    const onMenu = await page.evaluate(() => !document.querySelector('.lf').matches(':hover'));
    c = await ch2(page);
    if (!onMenu) problems.push('마우스: 검사 준비 실패 — 포인터가 구역을 떠나지 않았다');
    if (c.i !== 2 || c.hidden) problems.push(`마우스: 메뉴로 옮긴 사이 칸이 옮겨 갔다 ${JSON.stringify(c)}`);
    await page.keyboard.press('Escape');
    await page.mouse.move(2, 880, { steps: 3 });
    await page.waitForTimeout(250);
    c = await ch2(page);
    if (!c.hidden) problems.push(`마우스: 메뉴를 닫고 떠난 뒤에도 미룬 자리 바꿈을 하지 않았다 ${JSON.stringify(c)}`);
    if (errors.length) problems.push(`마우스: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF28 keyboard focus on a sort button / tile that disappears with the sort group (≤ 3) or the grid (0) moves to the first tile / "전체 보기"', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 4, width: 1280, clock: true });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Shift+Tab');
  const f0 = await focusInfo(page);
  const onSort = await page.evaluate(() => Boolean(document.activeElement.dataset.lfSort));
  if (!onSort) problems.push(`검사 준비 실패 — 정렬 단추가 아니다 ${JSON.stringify(f0)}`);
  await page.evaluate(() => { const st = window.__lf; st.following = st.following.filter((l) => l.channelId !== 'ch2'); });
  if (!(await twoMinuteRefresh(page))) problems.push('2분 갱신이 오지 않았다');
  const s1 = await page.evaluate(() => ({ sortHidden: document.querySelector('.lf-sort').hidden }));
  const f1 = await focusInfo(page);
  if (!s1.sortHidden) problems.push('검사 준비 실패 — 정렬 단추가 숨지 않았다');
  if (!f1.cls.includes('lf-open')) problems.push(`정렬 단추가 숨으며 초점이 첫 칸으로 가지 않았다 ${JSON.stringify(f1)}`);
  await page.evaluate(() => { window.__lf.following = []; });
  if (!(await twoMinuteRefresh(page))) problems.push('두 번째 2분 갱신이 오지 않았다');
  const f2 = await focusInfo(page);
  if (!f2.cls.includes('lf-all')) problems.push(`모든 방송이 끝나 격자가 숨으며 초점이 '전체 보기'로 가지 않았다 ${JSON.stringify(f2)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF29 arrow / End after a mouse click on "+N 더 보기" shows a focus ring (WebKit script focus); a pure mouse click does not', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280 });
  const ring = () => page.evaluate(() => { const a = document.activeElement; const cs = getComputedStyle(a); return { cls: String(a.className), style: cs.outlineStyle, width: parseFloat(cs.outlineWidth) || 0 }; });
  await page.locator('.lf-more').click();
  await page.mouse.move(2, 880, { steps: 3 });
  await page.waitForTimeout(300);
  const r0 = await ring();
  if (r0.cls.includes('lf-more') && r0.style !== 'none' && r0.width > 0) problems.push(`마우스로만 누른 더 보기에 초점 테두리가 있다 ${JSON.stringify(r0)}`);
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(300);
  const r1 = await ring();
  if (!r1.cls.includes('lf-open') || r1.style === 'none' || r1.width < 2) problems.push(`화살표로 옮긴 칸에 초점 테두리가 없다 ${JSON.stringify(r1)}`);
  await page.keyboard.press('End');
  await page.waitForTimeout(300);
  const r2 = await ring();
  if (!r2.cls.includes('lf-more') || r2.style === 'none' || r2.width < 2) problems.push(`End 로 옮긴 '접기'에 초점 테두리가 없다 ${JSON.stringify(r2)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF30 after a tile slides out from under a still pointer (ended broadcast), moving the pointer off the strip closes its card', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 5, width: 1280, clock: true });
  const t = page.locator('.lf-item[data-key="ch4"]');
  const b = await t.boundingBox();
  const x = b.x + b.width / 2;
  const y = b.y + b.height / 2;
  await page.mouse.move(x, y, { steps: 3 });
  await page.waitForTimeout(650);
  if ((await peekState(page)).key !== 'ch4') problems.push('검사 준비 실패 — ch4 카드가 없다');
  await page.evaluate(() => { const st = window.__lf; st.following = st.following.filter((l) => l.channelId !== 'ch0'); });
  if (!(await twoMinuteRefresh(page))) problems.push('2분 갱신이 오지 않았다');
  const under = await page.evaluate(([px, py]) => document.elementFromPoint(px, py)?.closest?.('.lf-item')?.dataset.key || null, [x, y]);
  if (under === 'ch4') problems.push('검사 준비 실패 — ch4 가 그대로 포인터 아래다');
  // 오른쪽(카드 바깥)으로, 그다음 곧장 아래(히어로)로 — 칸·카드를 지나지 않는다(카드를 지나면 진짜 pointerout 이 닫는다)
  const pb = await page.locator('.lf-peek').boundingBox();
  const hero = await page.locator('.hero').boundingBox();
  const x2 = Math.min(Math.max(x + 260, (pb ? pb.x + pb.width : 0) + 30), hero.x + hero.width - 20);
  if (pb && x2 <= pb.x + pb.width) problems.push('검사 준비 실패 — 카드를 피해 갈 자리가 없다');
  await page.mouse.move(x2, y, { steps: 3 });
  await page.mouse.move(x2, Math.max(hero.y + 60, y + 20), { steps: 6 });
  const onHero = await page.evaluate(([px, py]) => Boolean(document.elementFromPoint(px, py)?.closest?.('.hero')), [x2, Math.max(hero.y + 60, y + 20)]);
  if (!onHero) problems.push('검사 준비 실패 — 포인터가 히어로 위가 아니다');
  await page.waitForTimeout(400);
  const p = await peekState(page);
  if (p.open) problems.push(`포인터가 칸 밖(히어로)으로 갔는데 밀려난 칸의 카드(${p.key})가 남았다`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF31 a hover dwell timer that comes due right after a wheel scroll does not open the card of the tile that scrolled away', async () => {
  const problems = [];
  for (const wait of [300, 340, 370]) {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280, height: 700 });
    const t = visTiles(page).nth(4);
    const key = await t.getAttribute('data-key');
    const b = await t.boundingBox();
    const x = b.x + b.width / 2;
    const y = b.y + b.height / 2;
    await page.mouse.move(x, y, { steps: 2 });
    await page.waitForTimeout(wait); // 머묾(420ms) 예약이 걸린 채
    await page.mouse.wheel(0, 420);
    await page.waitForTimeout(600);
    const r = await page.evaluate(([px, py]) => {
      const p = document.querySelector('.lf-peek');
      return { open: !p.hidden, key: p.dataset.channelId || null, under: document.elementFromPoint(px, py)?.closest?.('.lf-item')?.dataset.key || null, sc: document.querySelector('.page-home').scrollTop };
    }, [x, y]);
    if (r.sc < 100) problems.push(`${wait}ms: 검사 준비 실패 — 스크롤되지 않았다 ${r.sc}`);
    if (r.open && r.key !== r.under) problems.push(`${wait}ms: 밀려난 칸(${r.key})의 카드가 떴다(포인터 아래 ${r.under}, 처음 칸 ${key})`);
    if (errors.length) problems.push(`${wait}ms: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF32 a cursor resting over the grid since before the app loaded (never moved) does not steal the keyboard focus card after arrow scrolls', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 39, width: 1280, height: 560, mouseBeforeLoad: [900, 160] });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(400);
  let scrolled = 0;
  let overGrid = 0;
  for (let i = 0; i < 12 && scrolled < 3; i += 1) {
    const y0 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(650);
    const y1 = await page.evaluate(() => document.querySelector('.page-home').scrollTop);
    if (y1 === y0) continue;
    scrolled += 1;
    const f = await focusInfo(page);
    const p = await peekState(page);
    if (await page.evaluate(() => Boolean(document.elementFromPoint(900, 160)?.closest?.('.lf')))) overGrid += 1;
    if (!p.open || p.key !== f.key) problems.push(`초점 칸(${f.key})이 아닌 카드 ${JSON.stringify(p)}`);
  }
  if (!scrolled) problems.push('검사 준비 실패 — 화살표로 스크롤되지 않았다');
  if (!overGrid) problems.push('검사 준비 실패 — 커서가 구역 위가 아니다');
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF33 card-menu edges: Esc on a menu item over the grid keeps the tile; a menu from the preview card counts; a right-click on "+N 더 보기" does not adopt another menu', async () => {
  const problems = [];
  const lowerCh2 = (page) => page.evaluate(() => { const st = window.__lf; st.following = st.following.map((l) => (l.channelId === 'ch2' ? { ...l, viewerCount: 5, viewerCountText: '5' } : l)); });
  const ch2 = async (page) => { const o = await order(page); const i = o.findIndex((x) => x.key === 'ch2'); return { i, hidden: i >= 0 && o[i].hidden }; };
  // 1) 격자 위에 놓인 메뉴 항목으로 옮긴 뒤 Esc — 포인터는 그대로(아직 구역 위)
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280, clock: true });
    const b = await page.locator('.lf-item[data-key="ch2"]').boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height / 2, { steps: 3 });
    await lowerCh2(page);
    if (!(await twoMinuteRefresh(page))) problems.push('격자 위 메뉴: 2분 갱신이 오지 않았다');
    await page.mouse.click(b.x + 30, b.y + b.height / 2, { button: 'right' });
    await page.waitForSelector('.ctx-menu');
    const items = page.locator('.ctx-menu .ctx-item');
    let spot = null;
    for (let i = 0; i < (await items.count()) && !spot; i += 1) {
      const r = await items.nth(i).boundingBox();
      const p = [Math.round(r.x + r.width / 2), Math.round(r.y + r.height / 2)];
      if (await page.evaluate(([x, y]) => document.elementsFromPoint(x, y).some((el) => el.closest('.lf-grid')), p)) spot = p;
    }
    if (!spot) problems.push('격자 위 메뉴: 검사 준비 실패 — 격자 위에 놓인 메뉴 항목이 없다');
    else {
      await page.mouse.move(spot[0], spot[1], { steps: 3 });
      await page.waitForTimeout(150);
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      const c = await ch2(page);
      if (c.i !== 2 || c.hidden) problems.push(`격자 위 메뉴: 포인터가 구역 위인데 메뉴를 닫자 칸이 옮겨 갔다 ${JSON.stringify(c)}`);
      await page.mouse.move(2, 880, { steps: 4 });
      await page.waitForTimeout(300);
      const c2 = await ch2(page);
      if (!c2.hidden) problems.push(`격자 위 메뉴: 떠난 뒤에도 미룬 자리 바꿈을 하지 않았다 ${JSON.stringify(c2)}`);
    }
    if (errors.length) problems.push(`격자 위 메뉴: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 2) 미리보기 카드에서 연 메뉴
  {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280, clock: true });
    const b = await page.locator('.lf-item[data-key="ch2"]').boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height / 2, { steps: 3 });
    await page.waitForTimeout(650);
    if ((await peekState(page)).key !== 'ch2') problems.push('카드 메뉴: 검사 준비 실패 — ch2 카드가 없다');
    await lowerCh2(page);
    if (!(await twoMinuteRefresh(page))) problems.push('카드 메뉴: 2분 갱신이 오지 않았다');
    const pb = await page.locator('.lf-peek').boundingBox();
    await page.mouse.move(b.x + 30, b.y + b.height + 4, { steps: 2 });
    await page.mouse.move(pb.x + pb.width / 2, pb.y + 40, { steps: 3 });
    await page.mouse.click(pb.x + pb.width / 2, pb.y + 40, { button: 'right' });
    await page.waitForSelector('.ctx-menu');
    const m = await page.locator('.ctx-menu .ctx-item').last().boundingBox();
    await page.mouse.move(m.x + m.width / 2, m.y + m.height / 2, { steps: 4 });
    await page.waitForTimeout(250);
    const off = await page.evaluate(() => !document.querySelector('.lf').matches(':hover'));
    const c = await ch2(page);
    if (!off) problems.push('카드 메뉴: 검사 준비 실패 — 포인터가 구역을 떠나지 않았다');
    if (c.i !== 2 || c.hidden) problems.push(`카드 메뉴: 카드에서 연 메뉴가 떠 있는 사이 칸이 옮겨 갔다 ${JSON.stringify(c)}`);
    await page.keyboard.press('Escape');
    await page.mouse.move(2, 880, { steps: 3 });
    await page.waitForTimeout(300);
    if (!(await ch2(page)).hidden) problems.push('카드 메뉴: 메뉴를 닫고 떠난 뒤에도 미룬 자리 바꿈을 하지 않았다');
    if (errors.length) problems.push(`카드 메뉴: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  // 3) '+N 더 보기' 우클릭(카드 메뉴가 뜨지 않는다) 뒤 2초 안에 사이드바에서 연 메뉴
  {
    const { ctx, page, errors } = await openHome({ count: 39, width: 1280, clock: true });
    // 실제 앱에선 기본 메뉴가 뜨고 사용자가 닫는다 — 헤드리스 WebKit 은 진짜 우클릭 뒤 다음 우클릭의 contextmenu 를 보내지 않아, 이벤트만 보낸다
    await page.locator('.lf-more').dispatchEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    const side = page.locator('.sidebar .side-live[data-channel-id]').first();
    if (!(await side.count())) problems.push('더 보기 우클릭: 검사 준비 실패 — 사이드바 채널이 없다');
    else {
      await side.click({ button: 'right' });
      await page.waitForSelector('.ctx-menu');
      await page.evaluate(() => { const st = window.__lf; st.favs.add('ch30'); });
      if (!(await twoMinuteRefresh(page))) problems.push('더 보기 우클릭: 2분 갱신이 오지 않았다');
      const o = await order(page);
      const hover = await page.evaluate(() => document.querySelector('.lf').matches(':hover'));
      if (hover) problems.push('더 보기 우클릭: 검사 준비 실패 — 포인터가 구역 위다');
      if (o.findIndex((x) => x.key === 'ch30') !== 0) problems.push(`더 보기 우클릭 뒤 사이드바 메뉴를 칸 메뉴로 알아 자리 바꿈을 미뤘다 (ch30 ${o.findIndex((x) => x.key === 'ch30')})`);
    }
    if (errors.length) problems.push(`더 보기 우클릭: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF34 tabbing through the strip with the mouse resting on a tile, then moving the mouse off, does not open the card of the focus that already left', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const t = visTiles(page).nth(4); // 아랫줄 — 곧장 아래가 히어로
  const b = await t.boundingBox();
  const hero = await page.locator('.hero').boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 2 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab'); // 첫 칸 — 키보드 카드 예약(260ms)
  await page.keyboard.press('Tab'); // 그 칸의 +
  await page.keyboard.press('Tab'); // 목록을 나간다
  const out = await page.evaluate(() => !document.querySelector('.lf').contains(document.activeElement));
  if (!out) problems.push('검사 준비 실패 — 초점이 목록을 나가지 않았다');
  await page.mouse.move(b.x + b.width / 2, Math.max(hero.y + 60, b.y + b.height + 40), { steps: 3 });
  await page.waitForTimeout(700);
  const p = await peekState(page);
  const f = await focusInfo(page);
  if (p.open) problems.push(`초점도 포인터도 없는 칸의 카드(${p.key})가 떴다 — 초점 ${JSON.stringify(f)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF35 a pointer that leaves the document at the same spot (pen lifted / window over a still cursor) cancels the pending dwell and closes an open card', async () => {
  const problems = [];
  // 같은 자리·relatedTarget 없는 pointerout — Chromium 이 펜을 들거나(WM_POINTERLEAVE) 창이 커서를 덮을 때(WM_MOUSELEAVE) 보내는 모양
  const leaveDoc = (page, x, y) => page.evaluate(([px, py]) => {
    const el = document.elementFromPoint(px, py);
    el.dispatchEvent(new PointerEvent('pointerout', { bubbles: true, cancelable: true, composed: true, clientX: px, clientY: py, pointerType: 'mouse', relatedTarget: null }));
  }, [x, y]);
  for (const stage of ['pending', 'open']) {
    const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
    const b = await visTiles(page).nth(1).boundingBox();
    const x = Math.round(b.x + 30);
    const y = Math.round(b.y + b.height / 2);
    await page.mouse.move(x, y, { steps: 2 });
    await page.waitForTimeout(stage === 'pending' ? 120 : 650);
    if (stage === 'open' && !(await peekState(page)).open) problems.push('열린 카드: 검사 준비 실패 — 카드가 없다');
    await leaveDoc(page, x, y);
    await page.waitForTimeout(stage === 'pending' ? 700 : 400);
    const p = await peekState(page);
    if (p.open) problems.push(stage === 'pending' ? `포인터가 문서를 떠났는데 머묾 예약이 카드(${p.key})를 띄웠다` : `포인터가 문서를 떠났는데 카드(${p.key})가 남았다`);
    if (errors.length) problems.push(`${stage}: 페이지 오류 ${errors[0]}`);
    await ctx.close();
  }
  ok(!problems.length, problems.join(' / '));
});

await check('LF36 a cursor that left the document and comes back without moving does not take over the keyboard card; a real move then does', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const b = await page.locator('.lf-item[data-key="ch2"]').boundingBox();
  const x = Math.round(b.x + 30);
  const y = Math.round(b.y + b.height / 2);
  await page.mouse.move(x, y, { steps: 2 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab'); // ch0 — 키보드 카드
  await page.waitForTimeout(500);
  let p = await peekState(page);
  if (p.key !== 'ch0') problems.push(`검사 준비 실패 — 키보드 카드가 ch0 가 아니다 ${JSON.stringify(p)}`);
  await page.evaluate(([px, py]) => {
    document.elementFromPoint(px, py).dispatchEvent(new PointerEvent('pointerout', { bubbles: true, cancelable: true, composed: true, clientX: px, clientY: py, pointerType: 'mouse', relatedTarget: null }));
  }, [x, y]);
  await page.waitForTimeout(100);
  await page.mouse.move(x, y); // 덮었던 창이 걷히고 같은 자리로 돌아온다
  await page.waitForTimeout(500);
  p = await peekState(page);
  const f = await focusInfo(page);
  if (p.key !== 'ch0' || f.key !== 'ch0') problems.push(`움직이지 않은 커서가 키보드 카드를 빼앗았다 — 카드 ${p.key} · 초점 ${f.key}`);
  await page.mouse.move(x + 6, y + 2, { steps: 2 });
  await page.waitForTimeout(500);
  p = await peekState(page);
  if (p.key !== 'ch2') problems.push(`실제로 움직였는데 포인터 아래 칸(ch2)의 카드가 아니다 ${JSON.stringify(p)}`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF37 with the pointer resting on the strip header, a keyboard card that is already open closes when Tab leaves the strip', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280 });
  const h = await page.locator('.lf .lf-head .grow').boundingBox();
  await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2, { steps: 2 });
  await page.locator('.lf-all').focus();
  await page.keyboard.press('Tab');
  await page.waitForTimeout(500);
  if ((await peekState(page)).key !== 'ch0') problems.push('검사 준비 실패 — 키보드 카드가 없다');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  const out = await page.evaluate(() => !document.querySelector('.lf').contains(document.activeElement));
  if (!out) problems.push('검사 준비 실패 — 초점이 목록을 나가지 않았다');
  await page.mouse.move(h.x - 300, h.y + h.height / 2, { steps: 4 }); // 머리줄을 따라 사이드바 쪽으로(칸을 지나지 않는다)
  await page.waitForTimeout(600);
  const p = await peekState(page);
  if (p.open) problems.push(`초점이 떠났고 포인터도 그 칸에 없는데 카드(${p.key})가 남았다`);
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await check('LF38 a reorder deferred when a tile menu closed over the grid is applied after a wheel scroll carries the strip away from a still pointer', async () => {
  const problems = [];
  const { ctx, page, errors } = await openHome({ count: 12, width: 1280, clock: true });
  const ch2 = async () => { const o = await order(page); const i = o.findIndex((x) => x.key === 'ch2'); return { i, hidden: i >= 0 && o[i].hidden }; };
  const b = await page.locator('.lf-item[data-key="ch2"]').boundingBox();
  await page.mouse.move(b.x + 30, b.y + b.height / 2, { steps: 3 });
  await page.evaluate(() => { const st = window.__lf; st.following = st.following.map((l) => (l.channelId === 'ch2' ? { ...l, viewerCount: 5, viewerCountText: '5' } : l)); });
  if (!(await twoMinuteRefresh(page))) problems.push('2분 갱신이 오지 않았다');
  await page.mouse.click(b.x + 30, b.y + b.height / 2, { button: 'right' });
  await page.waitForSelector('.ctx-menu');
  const items = page.locator('.ctx-menu .ctx-item');
  let spot = null;
  for (let i = 0; i < (await items.count()) && !spot; i += 1) {
    const r = await items.nth(i).boundingBox();
    const p = [Math.round(r.x + r.width / 2), Math.round(r.y + r.height / 2)];
    if (await page.evaluate(([x, y]) => document.elementsFromPoint(x, y).some((el) => el.closest('.lf-grid')), p)) spot = p;
  }
  if (!spot) problems.push('검사 준비 실패 — 격자 위에 놓인 메뉴 항목이 없다');
  else {
    await page.mouse.move(spot[0], spot[1], { steps: 3 });
    await page.waitForTimeout(150);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(150);
    const c0 = await ch2();
    if (c0.i !== 2 || c0.hidden) problems.push(`검사 준비 실패 — 메뉴를 닫자 이미 자리를 바꿨다 ${JSON.stringify(c0)}`);
    await page.mouse.wheel(0, 700);
    await page.waitForTimeout(1500);
    const off = await page.evaluate(([x, y]) => !document.elementFromPoint(x, y)?.closest?.('.lf'), spot);
    if (!off) problems.push('검사 준비 실패 — 휠 뒤에도 포인터가 구역 위다');
    const c1 = await ch2();
    if (!c1.hidden) problems.push(`구역이 포인터 아래를 지나갔는데 미룬 자리 바꿈이 남았다 ${JSON.stringify(c1)}`);
  }
  if (errors.length) problems.push(`페이지 오류 ${errors[0]}`);
  await ctx.close();
  ok(!problems.length, problems.join(' / '));
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  if (r.ok) console.log(`ok   ${r.id}`);
  else { failed += 1; console.log(`FAIL ${r.id}\n     ${r.error}`); }
}
console.log(`\n${WEBKIT ? 'webkit' : 'chromium'}: ${results.length - failed}/${results.length} 통과`);
process.exit(failed ? 1 : 0);
