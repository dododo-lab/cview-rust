// 홈 — 라이브 탐색과 시청 리포트를 한 화면에 모은다. 팔로잉은 스트립 한 곳,
// 추천·발견은 겹치지 않는 채널로, 이어 보기·최근 채널은 한 시청 구역으로 그린다.
// Swift `HomeView_v3` 의 "지금 볼 라이브를 먼저" 원칙을 따르되, 지금 Rust 명령으로 받을 수 있는
// 데이터(팔로잉·인기 라이브)와 로컬 최근 시청 기록만으로 그린다.
import { api } from './api.js';
import { esc, safeUrl, icons, imageSrc } from './dom.js';
import { cardOpenButton, liveLabel, lacksThumbs, THUMB_FILL_DELAY_MS, THUMB_FILL_TRIES, emptyState, errorState, friendlyError, liveGrid, recentChannels, uptimeText } from './cards.js';
import { clipCard } from './channel.js';
import { continueCard } from './media.js';
import { motionReduced } from './motion.js';
import { previewAttr } from './hover-preview.js';
import { verifiedBadge } from './verified-badge.js';
import { withRefreshVeil, refreshVeiled } from './refresh-veil.js';
import { LiveFollowingStrip, liveFollowingSkeleton } from './live-following.js';
import { syncTabs, bindTabKeys } from './a11y.js';

const HERO_SIZE = 5;
// 자동 넘김 — 대기열 진행 막대(CSS `queue-progress`, 7초)가 끝나면 다음 장으로. 호버 중엔 CSS 가 멈춘다.
// 모션 줄이기(시스템·설정 "절약")면 넘기지 않는다 — 절약은 애니메이션 시간을 0 으로 만들어 끝 이벤트가 곧바로 온다.
// 50 — 사이드바·인사이트·추가 서랍·검색 첫 화면과 같은 크기라야 api.js 가 같은 요청으로 합친다(48 이면 따로 갔다).
const POPULAR_SIZE = 50;
const POPULAR_GRID = 36;
const GRID_STEP = 12; // 발견 격자는 12개씩 펼친다 — 첫 화면이 끝없이 길어지지 않게
const CHIP_LIMIT = 8;
const ALL = '';

function greeting(now = new Date()) {
  const h = now.getHours();
  if (h < 5) return '늦은 밤이에요';
  if (h < 11) return '좋은 아침이에요';
  if (h < 17) return '좋은 오후예요';
  return '좋은 저녁이에요';
}

const WEEKDAYS = ['일', '월', '화', '수', '목', '금', '토'];
function dateLabel(now = new Date()) {
  return `${now.getMonth() + 1}월 ${now.getDate()}일 ${WEEKDAYS[now.getDay()]}요일`;
}

const byViewers = (a, b) => (b.viewerCount || 0) - (a.viewerCount || 0);
const uniqueLives = (lives) => [...new Map(lives.map((live) => [live.channelId, live])).values()];

function avatarImg(url, cls = 'avatar') {
  const src = safeUrl(url);
  return src
    ? `<img class="${cls}" src="${esc(imageSrc(src, 'avatar'))}" alt="" loading="lazy" referrerpolicy="no-referrer">`
    : `<div class="${cls}"></div>`;
}

function channelData(live) {
  return `data-channel-id="${esc(live.channelId)}" data-channel-name="${esc(live.channelName)}" data-channel-image="${esc(safeUrl(live.channelImageUrl))}"`;
}

// ── 조각들 ─────────────────────────────────────────────

/** 섹션 머리 — 제목 + 한 줄 설명 + 오른쪽 동작. */
function sectionHead(title, sub = '', actions = '') {
  return `
    <div class="section-head">
      <div class="section-titles">
        <h2 class="section-title">${title}</h2>
        ${sub ? `<p class="section-sub">${esc(sub)}</p>` : ''}
      </div>
      <div class="grow"></div>
      ${actions}
    </div>`;
}

const shelfNav = `<div class="shelf-nav"><button class="icon-btn round" data-shelf="-1" title="이전" aria-label="이전">${icons.chevronLeft}</button><button class="icon-btn round" data-shelf="1" title="다음" aria-label="다음">${icons.chevronRight}</button></div>`;

function loginBanner() {
  return `
    <section class="home-banner">
      <div class="home-banner-art" aria-hidden="true">${icons.following}</div>
      <div class="home-banner-text">
        <strong>로그인하고 팔로우한 채널을 홈에서 바로 만나보세요</strong>
        <span>방송 중인 팔로잉 채널, 채팅 참여, 맞춤 추천을 쓸 수 있어요</span>
      </div>
      <button class="btn primary lg" data-action="login">네이버 로그인</button>
    </section>`;
}

function heroSlide(live) {
  const thumb = safeUrl(live.thumbnailUrl);
  const up = uptimeText(live.openDate);
  return `
    <article class="hero-main card" ${channelData(live)}>
      ${cardOpenButton(liveLabel(live))}
      <div class="hero-media"${live.adult ? '' : previewAttr('live', live.channelId)}>
        ${thumb ? `<img fetchpriority="high" decoding="async" src="${esc(imageSrc(thumb, 'hero'))}" alt="" referrerpolicy="no-referrer">` : ''}
      </div>
      <div class="hero-shade"></div>
      <div class="hero-info">
        <div class="hero-tags">
          <span class="badge-live">LIVE</span>
          <span class="chip-glass"><i class="dot"></i>${esc(live.viewerCountText)}명 시청</span>
          ${up ? `<span class="chip-glass">${esc(up)}</span>` : ''}
          <span class="chip-glass soft">지금 인기</span>
          ${live.adult ? '<span class="chip-glass warn">19</span>' : ''}
        </div>
        <h3 class="hero-title">${esc(live.liveTitle || '제목 없음')}</h3>
        <div class="hero-channel">
          ${avatarImg(live.channelImageUrl)}
          <div class="ellipsis"><strong>${esc(live.channelName)}</strong>${verifiedBadge(live)}${live.categoryName ? `<span> · ${esc(live.categoryName)}</span>` : ''}</div>
        </div>
        <div class="hero-actions">
          <span class="btn primary lg hero-cta" aria-hidden="true">${icons.play}시청하기</span>
          <button type="button" class="btn lg hero-ghost" data-ml-add title="멀티라이브에 추가"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 6v12M6 12h12"/></svg>멀티라이브</button>
        </div>
      </div>
    </article>`;
}

/** 히어로 아래 넘김 탭 — 위쪽 진행 막대가 다 차면 다음 장으로(자동 넘김). */
function heroQueue(lives, active) {
  return lives.map((l, i) => {
    const thumb = safeUrl(l.thumbnailUrl);
    return `
      <button class="queue-item${i === active ? ' active' : ''}" data-hero-index="${i}" title="${esc(l.liveTitle)}" role="tab" aria-selected="${i === active}" tabindex="${i === active ? 0 : -1}">
        <span class="queue-progress" aria-hidden="true"></span>
        <span class="queue-thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}</span>
        <span class="queue-text">
          <span class="queue-title">${esc(l.liveTitle || '제목 없음')}</span>
          <span class="queue-meta ellipsis">${esc(l.channelName)}${verifiedBadge(l, 'sm')} · ${esc(l.viewerCountText)}명</span>
        </span>
      </button>`;
  }).join('');
}

/** 히어로 뒤 앰비언트 빛 — 지금 장의 썸네일을 크게 흐려 화면에 색을 번지게 한다(정지 화면, 장이 바뀔 때만 다시 그림). */
// ── 앰비언트 빛 ──────────────────────────────────────
// 히어로 뒤로 번지는 빛은 썸네일을 아주 작게(가로 48칸) 줄여 흐리고 채도를 올린 그림(data URL)을 늘려 그린다.
// 예전엔 썸네일 원본에 CSS `filter: blur(56px) saturate(1.6)` + `translateZ(0)` 을 걸어 상자·마스크·흐린 그림 합성 레이어를 만들었고,
// 흐림 반경만큼 넓게 잡힌 레이어 경계가 아래 제목·선반까지 '겹침' 레이어로 끌어들였다(실제 앱 WebContent 약 80MB, 2026-11-24 VM 측정).
// 흐린 빛은 저주파라 작은 그림을 늘려도 같아 보이고, 필터·합성 레이어가 없어 메모리가 들지 않는다.
const AMBIENT_W = 48;
const AMBIENT_SIGMA = 56; // CSS blur(56px) 의 표준편차 — 작은 그림 칸 단위로 바꿔 쓴다
const AMBIENT_SATURATE = 1.6;
const AMBIENT_BOX_W = 784; // 1280 창의 앰비언트 상자(히어로 + 좌우 24px)
const AMBIENT_BOX_H = 420;
const ambientCache = new Map(); // `${주소}|${가로}x${세로}` → data URL(최근 몇 장만)

/** 세 번의 상자 흐림(가우시안 근사) — 밖은 투명으로 보고(CSS blur 처럼 가장자리가 옅어진다) 미리 곱한 알파로 계산한다. */
function boxBlur3(px, w, h, sigma) {
  const r = Math.max(1, Math.round((Math.sqrt(4 * sigma * sigma + 1) - 1) / 2)); // 3회 상자(폭 2r+1)의 분산 ≈ σ²
  const tmp = new Float32Array(px.length);
  const pass = (src, dst, horizontal) => {
    const [n, m] = horizontal ? [w, h] : [h, w];
    const at = (i, j) => (horizontal ? (j * w + i) : (i * w + j)) * 4;
    for (let j = 0; j < m; j += 1) {
      for (let c = 0; c < 4; c += 1) {
        let sum = 0;
        for (let k = -r; k <= r; k += 1) if (k >= 0 && k < n) sum += src[at(k, j) + c];
        for (let i = 0; i < n; i += 1) {
          dst[at(i, j) + c] = sum / (2 * r + 1);
          const add = i + r + 1;
          const sub = i - r;
          if (add < n) sum += src[at(add, j) + c];
          if (sub >= 0) sum -= src[at(sub, j) + c];
        }
      }
    }
  };
  for (let k = 0; k < 3; k += 1) { pass(px, tmp, true); pass(tmp, px, false); }
}

/** 썸네일 → 상자 비율에 맞춰 덮어 자른(object-fit: cover) 작은 흐린 그림의 data URL. 실패하면 ''. */
async function ambientDataUrl(src, boxW, boxH) {
  const w = AMBIENT_W;
  const h = Math.max(8, Math.round((AMBIENT_W * boxH) / Math.max(1, boxW)));
  const key = `${src}|${w}x${h}`;
  if (ambientCache.has(key)) return ambientCache.get(key);
  const img = new Image();
  img.crossOrigin = 'anonymous'; // cvimg 응답은 ACAO: * — 캔버스가 오염되지 않는다
  img.referrerPolicy = 'no-referrer';
  img.src = src;
  await img.decode();
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  const scale = Math.max(w / img.naturalWidth, h / img.naturalHeight);
  const dw = img.naturalWidth * scale;
  const dh = img.naturalHeight * scale;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(img, (w - dw) / 2, (h - dh) / 2, dw, dh);
  const data = ctx.getImageData(0, 0, w, h);
  const px = new Float32Array(data.data.length);
  for (let i = 0; i < px.length; i += 4) { // 미리 곱한 알파
    const a = data.data[i + 3] / 255;
    px[i] = data.data[i] * a; px[i + 1] = data.data[i + 1] * a; px[i + 2] = data.data[i + 2] * a; px[i + 3] = data.data[i + 3];
  }
  boxBlur3(px, w, h, (AMBIENT_SIGMA * w) / Math.max(1, boxW));
  const s = AMBIENT_SATURATE; // CSS saturate() 행렬(Filter Effects 1)
  for (let i = 0; i < px.length; i += 4) {
    const a = px[i + 3] / 255;
    if (a <= 0) { data.data[i + 3] = 0; continue; }
    const R = px[i] / a; const G = px[i + 1] / a; const B = px[i + 2] / a;
    data.data[i] = (0.213 + 0.787 * s) * R + (0.715 - 0.715 * s) * G + (0.072 - 0.072 * s) * B;
    data.data[i + 1] = (0.213 - 0.213 * s) * R + (0.715 + 0.285 * s) * G + (0.072 - 0.072 * s) * B;
    data.data[i + 2] = (0.213 - 0.213 * s) * R + (0.715 - 0.715 * s) * G + (0.072 + 0.928 * s) * B;
    data.data[i + 3] = px[i + 3];
  }
  ctx.putImageData(data, 0, 0);
  const url = canvas.toDataURL('image/png');
  canvas.width = 0; canvas.height = 0;
  ambientCache.set(key, url);
  if (ambientCache.size > HERO_SIZE * 2) ambientCache.delete(ambientCache.keys().next().value);
  return url;
}

/** 앰비언트 상자에 live 의 빛을 칠한다 — 늦게 끝난 앞 장의 결과가 새 장을 덮지 않게 상자에 표시해 둔다. */
async function paintAmbient(box, live) {
  if (!box) return;
  const thumb = safeUrl(live && live.thumbnailUrl);
  const src = thumb ? imageSrc(thumb, 'live') : '';
  box.dataset.src = src;
  if (!src) { box.replaceChildren(); return; }
  // 상자 크기는 재지 않는다 — 홈을 갈아 끼운 직후 clientWidth 를 읽으면 강제 레이아웃(약 30ms, perf.mjs P01)이었다.
  // 보통 크기(가로 784·세로 420)로 만들고 img 는 object-fit: cover 라 창 폭이 달라도 흐린 빛은 같다.
  const w = AMBIENT_BOX_W;
  const h = AMBIENT_BOX_H;
  let url = '';
  try { url = await ambientDataUrl(src, w, h); } catch { url = ''; }
  if (box.dataset.src !== src || !box.isConnected) return;
  if (!url) { box.replaceChildren(); return; }
  let img = box.firstElementChild;
  if (!(img instanceof HTMLImageElement)) { img = document.createElement('img'); img.alt = ''; box.replaceChildren(img); }
  img.src = url;
}

function recentShelf(recent, liveById) {
  const items = recent.map((r) => {
    const live = liveById.get(r.channelId);
    return `
      <button class="recent-item${live ? ' is-live' : ''}" data-channel-id="${esc(r.channelId)}" data-channel-name="${esc(r.channelName)}" data-channel-image="${esc(r.channelImageUrl)}">
        ${avatarImg(r.channelImageUrl)}
        <span class="recent-text">
          <span class="ellipsis"><strong>${esc(r.channelName || '채널')}</strong>${verifiedBadge(live)}</span>
          <span class="recent-meta ellipsis">${live ? `<b>LIVE</b> ${esc(live.viewerCountText)}명${live.categoryName ? ` · ${esc(live.categoryName)}` : ''}` : '오프라인일 수 있음'}</span>
        </span>
        <span class="recent-play" aria-hidden="true">${icons.play}</span>
      </button>`;
  }).join('');
  return `
    <div class="home-recent-channels">
      <h3 class="home-row-title">최근 본 채널</h3>
      <div class="recent-row">${items}</div>
    </div>`;
}

const recapSkeleton = () => `<section class="home-recap" data-home-recap aria-label="시청 리포트 미리보기" aria-busy="true"><div class="home-recap-intro"><p class="home-kicker">최근 30일</p><h2>나의 시청 취향</h2><span>좋아하는 방송이 쌓이는 곳</span></div><div class="home-recap-metrics">${'<div class="sk recap-sk"></div>'.repeat(3)}</div><button class="btn" data-home-report="overview">리포트 보기 ${icons.chevronRight}</button></section>`;

function categoryChips(popular, selected) {
  const counts = new Map();
  for (const l of popular) {
    if (l.categoryName) counts.set(l.categoryName, (counts.get(l.categoryName) || 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, CHIP_LIMIT).map(([name]) => name);
  if (selected && !top.includes(selected)) top.push(selected);
  const chip = (value, label, n) =>
    `<button class="chip${value === selected ? ' active' : ''}" data-category="${esc(value)}">${esc(label)}<span class="chip-count">${n}</span></button>`;
  // 개수는 격자가 펼칠 수 있는 만큼(POPULAR_GRID) — 예전엔 '전체 45'인데 끝까지 펼쳐도 36개였다.
  const shown = (n) => Math.min(n, POPULAR_GRID);
  return `<div class="chips home-chips" role="group" aria-label="카테고리">${chip(ALL, '전체', shown(popular.length))}${top.map((n) => chip(n, n, shown(counts.get(n) || 0))).join('')}</div>`;
}

function popularGrid(popular, category, limit = GRID_STEP) {
  const all = (category ? popular.filter((l) => l.categoryName === category) : popular).slice(0, POPULAR_GRID);
  if (!all.length) return emptyState('이 카테고리의 라이브가 없습니다');
  const more = all.length > limit
    ? `<div class="grid-more"><button class="btn more-btn" data-grid-more>더 보기 <span>${all.length - limit}</span></button></div>`
    : '';
  return liveGrid(all.slice(0, limit)) + more;
}

export function homeSkeleton(loggedIn = false) {
  const card = '<div class="sk-card"><div class="sk thumb-sk"></div><div class="sk line"></div><div class="sk line short"></div></div>';
  return `
    <div class="home">
      ${loggedIn ? liveFollowingSkeleton(12) : ''}
      <div class="hero"><div class="sk hero-sk"></div><div class="hero-queue">${'<div class="sk queue-sk"></div>'.repeat(5)}</div></div>
      <div class="sk title-sk"></div>
      <div class="grid">${card.repeat(8)}</div>
    </div>`;
}

// ── 화면 ──────────────────────────────────────────────

export class HomePage {
  constructor(root, { isLoggedIn, getAuth = () => null }) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.greet = root.querySelector('.home-greeting');
    this.date = root.querySelector('.home-date');
    this.summary = root.querySelector('.home-summary');
    this.isLoggedIn = isLoggedIn;
    this.getAuth = getAuth;
    this.acceptsParams = true;
    this.view = 'live';
    this.viewScroll = { live: 0, report: 0 };
    this.panels = root.querySelector('.home-panels');
    this.liveView = root.querySelector('.home-live-view');
    this.viewTabs = root.querySelector('.home-tabs');
    bindTabKeys(this.viewTabs);
    this.viewTabs.addEventListener('click', (e) => {
      const tab = e.target.closest('[data-tab]');
      if (tab) goHome({ view: tab.dataset.tab });
    });
    this.seq = 0;
    this.category = ALL;
    this.gridLimit = GRID_STEP;
    this.popular = [];
    this.hero = [];
    this.heroIndex = 0;
    this.favorites = [];
    // 맨 위 "방송 중인 팔로잉" — 홈을 다시 그려도 이 요소는 옮겨 붙인다(정렬·펼침·초점 유지, 같은 칸은 다시 그리지 않음).
    this.liveStrip = new LiveFollowingStrip();
    // 홈을 떠나면(단축키·메뉴로 다른 화면 — 포인터는 그대로) 미리보기 카드를 닫는다. 남으면 숨은 홈의 카드가 다음 화면의 첫 Esc 를 삼켰다.
    new MutationObserver(() => { if (root.hidden) { this.liveStrip.onHidden(); this.reportPage?.hideTip(); } })
      .observe(root, { attributes: true, attributeFilter: ['hidden'] });
    // 첫 내용이 그려지면 풀린다 — 시작 로딩 화면이 이걸 기다렸다가 걷힌다(app.js).
    this.firstRender = new Promise((resolve) => { this.resolveFirstRender = resolve; });

    root.querySelector('[data-action="refresh"]').addEventListener('click', () => withRefreshVeil(root, () => this.show(true)));
    root.querySelector('.home-search').addEventListener('submit', (e) => {
      e.preventDefault();
      const input = e.currentTarget.querySelector('input');
      const q = input.value.trim();
      if (!q) return;
      input.value = '';
      goSearch(q);
    });
    this.body.addEventListener('click', (e) => this.onClick(e));
    // 히어로 넘김 탭 — 다른 탭 목록처럼 화살표·Home/End 로 옮긴다(가로든 넓은 창의 세로 목록이든). 예전엔 Tab 으로 다섯 개를
    // 하나씩 지나야 했다(roving tabindex 없음).
    this.body.addEventListener('keydown', (e) => {
      const tab = e.target.closest?.('.hero-queue [role="tab"]');
      if (!tab || e.metaKey || e.ctrlKey || e.altKey) return;
      const tabs = [...tab.parentElement.querySelectorAll('[role="tab"]')];
      const i = tabs.indexOf(tab);
      const j = { ArrowRight: i + 1, ArrowDown: i + 1, ArrowLeft: i - 1, ArrowUp: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
      if (j === undefined) return;
      e.preventDefault();
      const next = tabs[(j + tabs.length) % tabs.length];
      this.setHero(Number(next.dataset.heroIndex));
      next.focus();
    });
    this.body.addEventListener('animationend', (e) => {
      if (e.animationName !== 'queue-progress' || this.root.hidden || motionReduced()) return;
      this.setHero(this.heroIndex + 1);
    });
  }

  attachReport(reportPage) {
    this.reportPage = reportPage;
    this.setView(this.view);
  }

  retry() {
    return this.show({ view: this.view, refresh: true });
  }

  setView(view) {
    const next = view === 'report' && this.reportPage ? 'report' : 'live';
    const changed = this.view !== next;
    if (changed) this.viewScroll[this.view] = this.root.scrollTop;
    this.view = next;
    this.root.dataset.homeView = this.view;
    this.liveView.hidden = this.view !== 'live';
    if (this.reportPage) {
      this.reportPage.root.hidden = this.view !== 'report';
      this.reportPage.hideTip();
    }
    if (this.view !== 'live') this.liveStrip.onHidden();
    syncTabs(this.viewTabs, this.view, this.panels, 'home');
    this.renderGreeting();
    if (changed) this.root.scrollTop = this.viewScroll[this.view];
  }

  async show(params = false) {
    const refresh = params === true || Boolean(params && params.refresh);
    this.setView(params && typeof params === 'object' && params.view ? params.view : this.view);
    const seq = ++this.seq;
    if (refresh) this.reportPage?.reportCache.clear();
    if (this.view === 'report') {
      try { await this.reportPage.show(typeof params === 'object' ? params : {}); }
      finally { this.resolveFirstRender(); }
      return;
    }
    const loggedIn = this.isLoggedIn();
    if (!refresh && this.loadedLoggedIn === loggedIn && this.loadedAt && Date.now() - this.loadedAt < 60000) {
      this.loadRails(seq);
      this.loadRecap(seq);
      return;
    }
    // 직접 새로고침(덮개 아래)이면 보던 홈을 흐리게 둔 채 바꾼다 — 스켈레톤으로 비우지 않는다.
    if (!this.body.querySelector('.home') || (refresh && !refreshVeiled(this.root))) {
      // 초점 자리를 기억하고(붙어 있을 때만 — 아니면 앞서 기억한 것을 덮는다) 옛 홈에서 떼어 낸다(옛 트리를 붙잡지 않게).
      if (this.liveStrip.el.isConnected) this.liveStrip.beforeSwap();
      this.liveStrip.detach();
      this.body.innerHTML = homeSkeleton(loggedIn);
      this.liveStrip.watch(this.body.querySelector('.lf-skeleton .lf-grid'));
    }
    // 시작 로딩 화면(app.js)이 단계마다 진행을 보여 줄 수 있게 요청 하나가 끝날 때마다 알린다.
    const step = (name) => (v) => { if (this.onLoadStep) this.onLoadStep(name); return v; };
    const tracked = (name, p) => p.then(step(name), (e) => { step(name)(); throw e; });
    const [following, popular, recent, favorites] = await Promise.allSettled([
      loggedIn ? tracked('following', api.followingLives(refresh)) : Promise.resolve([]),
      tracked('popular', api.popularLives(POPULAR_SIZE, refresh)),
      tracked('recent', api.recentChannels(8)),
      // 즐겨찾기(로컬 DB) — 맨 위 구역의 추천 순서. 못 받으면 즐겨찾기 없이 그린다.
      loggedIn ? api.favoritesList() : Promise.resolve([]),
    ]);
    if (seq !== this.seq) return;
    // 최근 시청은 DB(재생할 때 기록)가 원본 — 비어 있으면 예전 localStorage 기록을 쓴다.
    this.dbRecent = recent.status === 'fulfilled' ? recent.value : [];
    if (favorites.status === 'fulfilled' && Array.isArray(favorites.value)) this.favorites = favorites.value;
    this.render(loggedIn, following, popular);
    this.loadedLoggedIn = loggedIn;
    this.loadedAt = Date.now();
    this.resolveFirstRender();
    this.loadRails(seq, refresh);
    this.loadRecap(seq);
    this.fillThumbsLater(refresh, following.status === 'fulfilled' ? following.value : []);
  }

  /** 팔로잉 카드에 썸네일이 빈 칸이 남았으면 조금 뒤 다시 불러 채운다(횟수 제한) — FollowingPage 와 같은 규칙. */
  fillThumbsLater(refresh, following) {
    clearTimeout(this.fillTimer);
    if (refresh || !lacksThumbs(following)) this.fillTries = 0;
    if (!lacksThumbs(following) || (this.fillTries || 0) >= THUMB_FILL_TRIES) return;
    this.fillTries = (this.fillTries || 0) + 1;
    this.fillTimer = setTimeout(() => {
      this.loadedAt = 0;
      if (!this.root.hidden && this.view === 'live') this.show(false);
    }, THUMB_FILL_DELAY_MS);
  }

  /** 첫 화면에 바로 보이는 그림(히어로·팔로잉 줄·첫 줄 카드)들 — 시작 로딩 화면이 이것들이 그려질 때까지 기다린다. */
  aboveFoldImages() {
    const limit = (this.root.closest('.content') || document.documentElement).getBoundingClientRect().bottom || window.innerHeight;
    return [...this.body.querySelectorAll('img')].filter((img) => {
      if (!img.getAttribute('src')) return false;
      const r = img.getBoundingClientRect();
      return r.width > 0 && r.top < limit;
    });
  }

  /** 인사말의 닉네임만 다시 쓴다(요약 줄은 그대로). */
  refreshGreetingName() {
    const auth = this.getAuth();
    const nick = auth && auth.loggedIn && auth.nickname ? `${auth.nickname}님, ` : '';
    this.greet.textContent = `${nick}${greeting()}`;
  }

  renderGreeting() {
    this.refreshGreetingName();
    if (this.date) this.date.textContent = dateLabel();
    this.summary.textContent = this.view === 'report' ? '좋아하는 채널과 함께한 시간을 돌아보세요' : '좋아하는 방송과 나의 시청 기록을 한곳에서';
  }

  /**
   * 사이드바 주기 갱신(app.js, 2분마다)이 받은 팔로잉 라이브로 맨 위 구역만 고친다 — 홈 전체를 다시 그리지 않아
   * 깜박이지 않고, 새로 시작한 방송은 나타나고 끝난 방송은 빠진다. 홈을 아직 그리지 않았거나 로그아웃이면 아무것도 않는다.
   * @param {Array} lives @param {Set<string>|Array} [favorites]
   */
  updateLiveFollowing(lives, favorites) {
    if (!this.isLoggedIn() || !Array.isArray(lives) || !this.liveStrip.el.isConnected) return;
    if (favorites) this.favorites = [...favorites];
    this.liveStrip.update(lives, { favorites: this.favorites });
    const followIds = new Set(lives.map((live) => live.channelId));
    if ([...this.hero, ...(this.discover || [])].some((live) => followIds.has(live.channelId))) {
      // 새로 팔로우한 채널이 추천에도 남아 있으면 다음 홈 갱신에서 중복을 걷는다.
      this.loadedAt = 0;
      if (!this.root.hidden && this.view === 'live') this.show(false);
    }
  }

  render(loggedIn, followingRes, popularRes) {
    const following = followingRes.status === 'fulfilled' ? uniqueLives(followingRes.value || []).sort(byViewers) : [];
    const popular = popularRes.status === 'fulfilled' ? uniqueLives(popularRes.value || []).sort(byViewers) : [];
    this.popular = popular;
    this.renderGreeting();

    const followIds = new Set(following.map((l) => l.channelId));
    // 팔로잉은 위 스트립에 한 번만. 스포트라이트와 발견 격자는 팔로우 밖의 인기 방송으로 채운다.
    this.hero = popular.filter((live) => !followIds.has(live.channelId)).slice(0, HERO_SIZE);
    if (this.heroIndex >= this.hero.length) this.heroIndex = 0;

    const liveById = new Map([...popular, ...following].map((l) => [l.channelId, l]));
    const recent = (this.dbRecent && this.dbRecent.length ? this.dbRecent : recentChannels()).slice(0, 8);

    let html = '<div class="home">';
    if (!loggedIn) html += loginBanner();
    else if (followingRes.status === 'rejected') {
      // 원문 대신 쉬운 안내 + 다시 시도(app.js 가 홈을 다시 불러온다) — 다른 화면의 실패와 같게
      html += `<div class="home-notice" role="alert">팔로잉 라이브를 불러오지 못했습니다 · ${esc(friendlyError(followingRes.reason))} <button class="link-btn" data-retry>다시 시도</button></div>`;
    } else {
      // 맨 위 "방송 중인 팔로잉"(live-following.js) — 아무도 방송하지 않아도 한 줄 안내로 자리를 지킨다. 아래에서 옮겨 붙인다.
      html += '<div data-lf-slot></div>';
      this.liveStrip.update(following, { favorites: this.favorites, recent });
    }

    if (this.reportPage) html += recapSkeleton();

    if (this.hero.length) {
      html += `
        <section class="home-section home-spotlight">
        ${sectionHead('지금 눈여겨볼 라이브', '새롭게 만나는 인기 방송', '<button class="link-btn" data-goto="popular" data-goto-tab="live">전체 방송 보기</button>')}
        <div class="hero" aria-label="지금 뜨는 라이브">
          <div class="hero-ambient" aria-hidden="true"></div>
          <div class="hero-slot">${heroSlide(this.hero[this.heroIndex])}</div>
          <div class="hero-queue" role="tablist" aria-label="추천 방송 넘기기">${heroQueue(this.hero, this.heroIndex)}</div>
        </div></section>`;
    }

    // 보던 영상·채널은 추천 바로 아래에 모은다. 기록이 없으면 구역 자체를 숨겨 빈 간격도 남기지 않는다.
    html += `<section class="home-section home-history"${recent.length ? '' : ' hidden'}>
      ${sectionHead('최근 시청', '보던 영상과 채널을 이어서 만나보세요', '<button class="link-btn" data-goto-lib="recent">전체 기록 보기</button>')}
      <div class="home-continue" data-rail="continue" hidden></div>
      ${recent.length ? recentShelf(recent, liveById) : ''}
    </section>`;

    if (popularRes.status === 'fulfilled') {
      // 팔로잉이 히어로를 차지했으면 인기 격자는 팔로우 밖 채널부터(발견) — Swift popularLive 규칙.
      // 위 스포트라이트와 팔로잉에 나온 채널은 빼서 같은 카드가 두 번 보이지 않게 한다.
      const shown = new Set([...followIds, ...this.hero.map((l) => l.channelId)]);
      const discover = popular.filter((l) => !shown.has(l.channelId));
      this.discover = discover;
      html += `
        <section class="home-section">
          ${sectionHead('더 많은 라이브 발견', '관심 있는 카테고리로 골라 보세요')}
          ${categoryChips(discover, this.category)}
          <div class="popular-grid">${discover.length ? popularGrid(discover, this.category, this.gridLimit) : emptyState('추천할 라이브 채널이 없습니다')}</div>
        </section>`;
    } else {
      html += errorState('인기 라이브를 불러오지 못했습니다', popularRes.reason);
    }
    html += '<section class="home-section" data-rail="clips" hidden></section>';
    html += '</div>';
    this.swapKeepingStrip(html);
    paintAmbient(this.body.querySelector('.hero-ambient'), this.hero[this.heroIndex]);
    this.preloadNextHero();
  }

  /**
   * 홈 본문을 바꾼다. 맨 위 구역(liveStrip)이 이미 홈에 붙어 있고 새 홈에도 자리가 있으면 그 요소는 문서에서 빼지 않고
   * 앞뒤 형제만 갈아 끼운다 — 떼었다 붙이면 초점·호버가 풀리고, 세그먼트 판(motion.js)이 다시 재며 강제 레이아웃을 일으켰다.
   * 처음(스켈레톤 다음)이면 swapContent 로 그리고 자리에 붙인다.
   */
  swapKeepingStrip(html) {
    const strip = this.liveStrip.el;
    const home = strip.parentElement;
    if (home && home.parentElement === this.body && home.classList.contains('home') && html.includes('data-lf-slot')) {
      const tpl = document.createElement('template');
      tpl.innerHTML = html;
      const fresh = tpl.content.querySelector('.home');
      const slot = fresh && fresh.querySelector(':scope > [data-lf-slot]');
      if (slot) {
        // swapContent 와 같은 규칙 — 보이던 내용을 갈아 끼운 것이라 떠오르지 않는다
        this.body.classList.toggle('no-enter', !this.body.querySelector('.sk, .ip-skeleton, .empty.loading'));
        const before = [];
        const after = [];
        let seen = false;
        for (const n of [...fresh.childNodes]) {
          if (n === slot) { seen = true; continue; }
          (seen ? after : before).push(n);
        }
        for (const n of [...home.childNodes]) if (n !== strip) n.remove();
        strip.before(...before);
        strip.after(...after);
        return;
      }
    }
    // 처음 붙일 때 — 문서에 넣기 전에 템플릿 안에서 자리를 구역으로 바꿔 한 번에 붙인다. 빈 자리를 먼저 넣고 바꾸면
    // 세그먼트 판(motion.js)의 MutationObserver 가 같은 묶음에서 .home 과 구역을 두 번 훑어 "다시 붙음"으로 보고
    // 시작 중 판을 동기로 다시 쟀다(강제 레이아웃 약 30ms — perf.mjs P01).
    // 초점 자리는 구역이 붙어 있을 때만 기억한다 — 스켈레톤을 거쳐 왔으면 그때 기억한 것을 덮지 않게(예전엔 늘 덮어 복원이 실패했다).
    if (strip.isConnected) this.liveStrip.beforeSwap();
    else this.liveStrip.hidePeek();
    const tpl = document.createElement('template');
    tpl.innerHTML = html;
    const slot = tpl.content.querySelector('[data-lf-slot]');
    if (slot) this.liveStrip.mount(slot);
    else this.liveStrip.detach({ forget: true }); // 자리가 없다(로그아웃·팔로잉 실패) — 옛 홈 트리를 붙잡지 않게 뗀다
    // swapContent 와 같은 규칙(보이던 내용을 갈아 끼우면 떠오르지 않는다)
    this.body.classList.toggle('no-enter', this.body.childElementCount > 0 && !this.body.querySelector('.sk, .ip-skeleton, .empty.loading'));
    this.body.replaceChildren(tpl.content);
    if (slot) this.liveStrip.restoreFocus();
  }

  /** 다음 히어로 장의 큰 썸네일(720)을 미리 받아 둔다 — 자동 넘김 때 빈 칸이 잠깐 보이지 않게.
   * 첫 화면 그림들과 다투지 않도록 조금 뒤에 한 장만. */
  preloadNextHero() {
    clearTimeout(this.preloadTimer);
    if (this.hero.length < 2) return;
    const next = this.hero[(this.heroIndex + 1) % this.hero.length];
    const thumb = safeUrl(next && next.thumbnailUrl);
    if (!thumb) return;
    this.preloadTimer = setTimeout(() => {
      const img = new Image();
      img.decoding = 'async';
      img.referrerPolicy = 'no-referrer';
      img.src = imageSrc(thumb, 'hero');
    }, 1200);
  }

  /** 이어 보기·인기 클립 레일 — 첫 화면을 막지 않게 나중에 채운다 (NAV-14). */
  async loadRails(seq, refresh = false) {
    // 직접 새로고침(⌘R)이면 인기 클립도 응답 캐시(최대 60초)를 건너뛴다 — 라이브 줄과 같은 규칙.
    const [cont, clips] = await Promise.allSettled([api.vodContinueList(12), api.popularClips('WITHIN_1_DAY', 'POPULAR', refresh)]);
    if (seq !== this.seq) return;
    const fill = (name, title, sub, html, more) => {
      const box = this.body.querySelector(`[data-rail="${name}"]`);
      if (!box || !html) return;
      box.innerHTML = name === 'continue'
        ? `<div class="home-row-head"><h3 class="home-row-title">${esc(title)}</h3><div class="grow"></div>${shelfNav}${more}</div>${html}`
        : `${sectionHead(title, sub, `${shelfNav}${more}`)}${html}`;
      box.hidden = false;
      const history = box.closest('.home-history');
      if (history) history.hidden = false;
    };
    // 받았는데 비었으면 레일을 내린다 — 60초 안에 돌아오면 본문을 다시 그리지 않아(show), 보관함에서 지운 이어 보기·끝난
    // 클립 레일이 그대로 남고 누르면 지운 다시보기가 다시 열렸다.
    const clear = (name) => {
      const box = this.body.querySelector(`[data-rail="${name}"]`);
      if (!box || box.hidden) return;
      box.hidden = true;
      box.innerHTML = '';
      const history = box.closest('.home-history');
      if (history && !history.querySelector('.recent-item') && ![...history.querySelectorAll('[data-rail]')].some((r) => !r.hidden)) history.hidden = true;
    };
    if (cont.status === 'fulfilled' && !cont.value.length) clear('continue');
    if (clips.status === 'fulfilled' && !clips.value.length) clear('clips');
    if (cont.status === 'fulfilled' && cont.value.length) fill('continue', '이어 보기', '보던 다시보기를 멈춘 곳부터', `<div class="shelf continue-shelf">${cont.value.map(continueCard).join('')}</div>`, '<button class="link-btn" data-goto-lib="continue">전체 보기</button>');
    if (clips.status === 'fulfilled' && clips.value.length) fill('clips', '오늘의 인기 클립', '하루 동안 가장 많이 본 순간', `<div class="shelf clip-shelf">${clips.value.slice(0, 16).map(clipCard).join('')}</div>`, '<button class="link-btn" data-goto="popular" data-goto-tab="clips" data-goto-period="WITHIN_1_DAY">전체 보기</button>');
  }

  async loadRecap(seq) {
    const box = this.body.querySelector('[data-home-recap]');
    if (!box || !this.reportPage) return;
    const button = `<button class="btn" data-home-report="overview">리포트 보기 ${icons.chevronRight}</button>`;
    const intro = `<div class="home-recap-intro"><p class="home-kicker">최근 30일</p><h2>나의 시청 취향</h2><span>좋아하는 방송이 쌓이는 곳</span></div>`;
    try {
      const r = await this.reportPage.fetchReport('30d');
      if (seq !== this.seq || !box.isConnected) return;
      const metric = (label, value) => `<div class="home-recap-metric"><span>${esc(label)}</span><strong class="ellipsis" title="${esc(value)}">${esc(value)}</strong></div>`;
      box.innerHTML = intro + (r.total > 0
        ? `<div class="home-recap-metrics">${metric('가장 오래 본 채널', r.channels[0]?.name || '—')}${metric('즐겨 본 카테고리', r.categories[0]?.name || '—')}${metric('시청한 방송', `${r.sessionCount.toLocaleString()}회`)}</div>`
        : '<p class="home-recap-empty">라이브를 보면 나만의 시청 취향이 여기에 쌓여요.</p>') + button;
    } catch {
      if (seq !== this.seq || !box.isConnected) return;
      box.innerHTML = intro + '<p class="home-recap-empty">시청 리포트를 불러오지 못했어요.</p><button class="btn" data-recap-retry>다시 시도</button>';
    } finally {
      if (seq === this.seq) box.setAttribute('aria-busy', 'false');
    }
  }

  setHero(index) {
    if (!this.hero.length) return;
    this.heroIndex = (index + this.hero.length) % this.hero.length;
    const slot = this.body.querySelector('.hero-slot');
    if (!slot) return;
    // 키보드 초점이 히어로 카드에 있던 채 자동으로 넘어가면 초점이 문서 처음으로 떨어졌다 — 새 장의 같은 단추로 옮긴다.
    const focused = slot.contains(document.activeElement) ? (document.activeElement.matches('[data-ml-add]') ? '[data-ml-add]' : '.card-open') : '';
    slot.innerHTML = heroSlide(this.hero[this.heroIndex]);
    if (focused) slot.querySelector(focused)?.focus({ preventScroll: true });
    const ambient = this.body.querySelector('.hero-ambient');
    paintAmbient(ambient, this.hero[this.heroIndex]);
    for (const q of this.body.querySelectorAll('.queue-item')) {
      const on = Number(q.dataset.heroIndex) === this.heroIndex;
      q.classList.toggle('active', on);
      q.setAttribute('aria-selected', String(on));
      q.tabIndex = on ? 0 : -1;
    }
    this.preloadNextHero();
  }

  onClick(e) {
    const report = e.target.closest('[data-home-report]');
    if (report) { goHome({ view: 'report', tab: report.dataset.homeReport, period: '30d' }); return; }
    if (e.target.closest('[data-recap-retry]')) { this.loadRecap(this.seq); return; }
    const queue = e.target.closest('[data-hero-index]');
    if (queue) {
      this.setHero(Number(queue.dataset.heroIndex));
      return;
    }
    const chip = e.target.closest('[data-category]');
    if (chip) {
      this.category = chip.dataset.category;
      this.gridLimit = GRID_STEP;
      for (const c of this.body.querySelectorAll('.chip')) c.classList.toggle('active', c === chip);
      this.body.querySelector('.popular-grid').classList.remove('no-enter'); // 다른 분류는 새 목록 — 떠오른다
      this.body.querySelector('.popular-grid').innerHTML = popularGrid(this.discover || [], this.category, this.gridLimit);
      return;
    }
    if (e.target.closest('[data-grid-more]')) {
      this.gridLimit += GRID_STEP;
      const box = this.body.querySelector('.popular-grid');
      const had = box.querySelectorAll('.grid > *').length;
      // 이미 보이던 카드는 다시 떠오르지 않게(no-enter) — 예전엔 더 보기마다 위 카드 8장(풍부면 전부)이 투명해졌다 다시 떴다.
      box.classList.add('no-enter');
      box.innerHTML = popularGrid(this.discover || [], this.category, this.gridLimit);
      // 누른 단추가 다시 그려져 초점이 문서 처음으로 떨어졌다 — 새 더 보기(없으면 새로 붙은 첫 카드)로.
      const card = box.querySelectorAll('.grid > *')[had];
      const next = box.querySelector('[data-grid-more]') || (card?.matches('a, button, [tabindex]') ? card : card?.querySelector('a, button, [tabindex]'));
      next?.focus({ preventScroll: true });
      return;
    }
    const shelfBtn = e.target.closest('[data-shelf]');
    if (shelfBtn) {
      const shelf = shelfBtn.closest('.home-section').querySelector('.shelf');
      shelf.scrollBy({ left: Number(shelfBtn.dataset.shelf) * shelf.clientWidth * 0.9, behavior: motionReduced() ? 'auto' : 'smooth' });
      return;
    }
    const vod = e.target.closest('[data-vod-no]');
    if (vod) { window.dispatchEvent(new CustomEvent('cview:open-vod', { detail: { videoNo: Number(vod.dataset.vodNo) } })); return; }
    const clip = e.target.closest('[data-clip-uid]');
    if (clip) { window.dispatchEvent(new CustomEvent('cview:open-clip', { detail: { clipUid: clip.dataset.clipUid } })); return; }
    const lib = e.target.closest('[data-goto-lib]');
    if (lib) { window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'library', params: { tab: lib.dataset.gotoLib } } })); return; }
    const goto = e.target.closest('[data-goto]');
    if (goto) window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: goto.dataset.goto, params: { ...(goto.dataset.gotoTab ? { tab: goto.dataset.gotoTab } : {}), ...(goto.dataset.gotoPeriod ? { period: goto.dataset.gotoPeriod } : {}) } } }));
  }
}

function goHome(params) {
  window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'home', params } }));
}

/** 홈 검색창 → 검색 화면으로 넘겨 바로 검색한다. */
function goSearch(q) {
  window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'search', params: { query: q } } }));
}
