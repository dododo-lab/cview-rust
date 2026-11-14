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
// 저장소 루트에서: node tools/ui-regression/live-following.mjs   (Playwright 필요, axe-core 가 있으면 LF03 에서 쓴다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG.)
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
async function check(id, fn) {
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
    following_lives: () => { if (opts.failFollowing) throw new Error('network timed out'); return state.following.map((l) => ({ ...l })); },
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
  await page.addInitScript(mockLF, [base, { count: 12, theme, ...opts }]);
  await page.goto(base);
  await page.waitForSelector(opts.failFollowing ? '.home .home-notice' : '.home .lf', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  if (theme === 'light') await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
  await page.mouse.move(2, height - 2);
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
  // 떠나면 제 순서 — 즐겨찾기가 맨 앞
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
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

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  if (r.ok) console.log(`ok   ${r.id}`);
  else { failed += 1; console.log(`FAIL ${r.id}\n     ${r.error}`); }
}
console.log(`\n${WEBKIT ? 'webkit' : 'chromium'}: ${results.length - failed}/${results.length} 통과`);
process.exit(failed ? 1 : 0);
