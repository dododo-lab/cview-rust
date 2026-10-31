// 홈 — 인사 헤더 / 방송 중인 팔로잉 스트립 / 앰비언트 스포트라이트(히어로 + 넘김 탭) / 최근 시청 퀵픽 /
// 지금 인기 TOP 10 / 팔로잉 선반 / 카테고리 칩 + 발견 격자(더 보기로 펼침).
// Swift `HomeView_v3` 의 "지금 볼 라이브를 먼저" 원칙을 따르되, 지금 Rust 명령으로 받을 수 있는
// 데이터(팔로잉·인기 라이브)와 로컬 최근 시청 기록만으로 그린다.
import { api } from './api.js';
import { esc, safeUrl, icons, imageSrc } from './dom.js';
import { liveCard, emptyState, errorState, friendlyError, liveGrid, recentChannels, uptimeText, compactCount } from './cards.js';
import { clipCard } from './channel.js';
import { continueCard } from './media.js';
import { motionReduced, swapContent } from './motion.js';
import { previewAttr } from './hover-preview.js';

const HERO_SIZE = 5;
// 자동 넘김 — 대기열 진행 막대(CSS `queue-progress`, 7초)가 끝나면 다음 장으로. 호버 중엔 CSS 가 멈춘다.
// 모션 줄이기(시스템·설정 "절약")면 넘기지 않는다 — 절약은 애니메이션 시간을 0 으로 만들어 끝 이벤트가 곧바로 온다.
const POPULAR_SIZE = 48;
const POPULAR_GRID = 36;
const GRID_STEP = 12; // 발견 격자는 12개씩 펼친다 — 첫 화면이 끝없이 길어지지 않게
const TOP_SIZE = 10;
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
        <span>방송 중인 팔로잉 채널, 채팅 참여, 맞춤 추천이 열립니다</span>
      </div>
      <button class="btn primary lg" data-action="login">네이버 로그인</button>
    </section>`;
}

/** 방송 중인 팔로잉 — 원형 아바타 스트립. */
function followingStrip(lives) {
  const items = lives.map((l) => `
    <button class="strip-item" ${channelData(l)} title="${esc(l.liveTitle)}">
      <span class="strip-ring">${avatarImg(l.channelImageUrl)}</span>
      <span class="strip-live">LIVE</span>
      <span class="strip-name ellipsis">${esc(l.channelName)}</span>
      <span class="strip-meta ellipsis">${esc(l.categoryName || `${l.viewerCountText}명`)}</span>
    </button>`).join('');
  return `
    <section class="home-section">
      ${sectionHead(`방송 중인 팔로잉 <span class="count">${lives.length}</span>`, '', '<button class="link-btn" data-goto="following">전체 보기</button>')}
      <div class="strip">${items}</div>
    </section>`;
}

function heroSlide(live, source) {
  const thumb = safeUrl(live.thumbnailUrl);
  const up = uptimeText(live.openDate);
  return `
    <article class="hero-main card" tabindex="0" ${channelData(live)}>
      <div class="hero-media"${live.adult ? '' : previewAttr('live', live.channelId)}>
        ${thumb ? `<img fetchpriority="high" decoding="async" src="${esc(imageSrc(thumb, 'hero'))}" alt="" referrerpolicy="no-referrer">` : ''}
      </div>
      <div class="hero-shade"></div>
      <div class="hero-info">
        <div class="hero-tags">
          <span class="badge-live">LIVE</span>
          <span class="chip-glass"><i class="dot"></i>${esc(live.viewerCountText)}명 시청</span>
          ${up ? `<span class="chip-glass">${esc(up)}</span>` : ''}
          <span class="chip-glass soft">${source === 'following' ? '팔로잉' : '지금 인기'}</span>
          ${live.adult ? '<span class="chip-glass warn">19</span>' : ''}
        </div>
        <h3 class="hero-title">${esc(live.liveTitle || '제목 없음')}</h3>
        <div class="hero-channel">
          ${avatarImg(live.channelImageUrl)}
          <div class="ellipsis"><strong>${esc(live.channelName)}</strong>${live.categoryName ? `<span> · ${esc(live.categoryName)}</span>` : ''}</div>
        </div>
        <div class="hero-actions">
          <span class="btn primary lg hero-cta">${icons.play}시청하기</span>
          <button class="btn lg hero-ghost" data-ml-add title="멀티라이브에 추가"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 6v12M6 12h12"/></svg>멀티라이브</button>
        </div>
      </div>
    </article>`;
}

/** 히어로 아래 넘김 탭 — 위쪽 진행 막대가 다 차면 다음 장으로(자동 넘김). */
function heroQueue(lives, active) {
  return lives.map((l, i) => {
    const thumb = safeUrl(l.thumbnailUrl);
    return `
      <button class="queue-item${i === active ? ' active' : ''}" data-hero-index="${i}" title="${esc(l.liveTitle)}" role="tab" aria-selected="${i === active}">
        <span class="queue-progress" aria-hidden="true"></span>
        <span class="queue-thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}</span>
        <span class="queue-text">
          <span class="queue-title">${esc(l.liveTitle || '제목 없음')}</span>
          <span class="queue-meta ellipsis">${esc(l.channelName)} · ${esc(l.viewerCountText)}명</span>
        </span>
      </button>`;
  }).join('');
}

/** 히어로 뒤 앰비언트 빛 — 지금 장의 썸네일을 크게 흐려 화면에 색을 번지게 한다(정지 화면, 장이 바뀔 때만 다시 그림). */
function heroAmbient(live) {
  const thumb = safeUrl(live && live.thumbnailUrl);
  return thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" referrerpolicy="no-referrer">` : '';
}

/** 지금 인기 TOP 10 — 큰 순위 숫자 + 썸네일 카드. */
function topRail(lives) {
  const items = lives.map((l, i) => {
    const thumb = safeUrl(l.thumbnailUrl);
    return `
      <article class="top-item" tabindex="0" ${channelData(l)} title="${esc(l.liveTitle)}">
        <span class="top-rank" aria-hidden="true">${i + 1}</span>
        <div class="top-card">
          <div class="thumb"${l.adult ? '' : previewAttr('live', l.channelId)}>
            ${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
            <span class="badge-live">LIVE</span>
            <span class="badge-viewers"><i class="dot"></i>${esc(l.viewerCountText)}명</span>
            <button class="card-ml-add" data-ml-add title="멀티라이브에 추가" aria-label="멀티라이브에 추가"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 6v12M6 12h12"/></svg></button>
          </div>
          <div class="top-text">
            <div class="top-title">${esc(l.liveTitle || '제목 없음')}</div>
            <div class="top-meta ellipsis">${avatarImg(l.channelImageUrl, 'avatar xs')}<span>${esc(l.channelName)}</span>${l.categoryName ? `<span class="category">· ${esc(l.categoryName)}</span>` : ''}</div>
          </div>
        </div>
      </article>`;
  }).join('');
  return `
    <section class="home-section">
      ${sectionHead('지금 인기 TOP 10', '시청자 수 기준 실시간 순위', `${shelfNav}<button class="link-btn" data-goto="popular">인기 전체 보기</button>`)}
      <div class="shelf top-shelf">${items}</div>
    </section>`;
}

function recentShelf(recent, liveById) {
  const items = recent.map((r) => {
    const live = liveById.get(r.channelId);
    return `
      <button class="recent-item${live ? ' is-live' : ''}" data-channel-id="${esc(r.channelId)}" data-channel-name="${esc(r.channelName)}" data-channel-image="${esc(r.channelImageUrl)}">
        ${avatarImg(r.channelImageUrl)}
        <span class="recent-text">
          <span class="ellipsis"><strong>${esc(r.channelName || '채널')}</strong></span>
          <span class="recent-meta ellipsis">${live ? `<b>LIVE</b> ${esc(live.viewerCountText)}명${live.categoryName ? ` · ${esc(live.categoryName)}` : ''}` : '오프라인일 수 있음'}</span>
        </span>
        <span class="recent-play" aria-hidden="true">${icons.play}</span>
      </button>`;
  }).join('');
  return `
    <section class="home-section">
      ${sectionHead('최근 시청', '최근에 본 채널로 바로 돌아가기')}
      <div class="recent-row">${items}</div>
    </section>`;
}

function followingShelf(lives) {
  return `
    <section class="home-section">
      ${sectionHead('팔로잉 라이브', '시청자 많은 순', shelfNav)}
      <div class="shelf">${lives.map(liveCard).join('')}</div>
    </section>`;
}

function categoryChips(popular, selected) {
  const counts = new Map();
  for (const l of popular) {
    if (l.categoryName) counts.set(l.categoryName, (counts.get(l.categoryName) || 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, CHIP_LIMIT).map(([name]) => name);
  if (selected && !top.includes(selected)) top.push(selected);
  const chip = (value, label, n) =>
    `<button class="chip${value === selected ? ' active' : ''}" data-category="${esc(value)}">${esc(label)}<span class="chip-count">${n}</span></button>`;
  return `<div class="chips home-chips" role="tablist">${chip(ALL, '전체', popular.length)}${top.map((n) => chip(n, n, counts.get(n) || 0)).join('')}</div>`;
}

function popularGrid(popular, category, limit = GRID_STEP) {
  const all = (category ? popular.filter((l) => l.categoryName === category) : popular).slice(0, POPULAR_GRID);
  if (!all.length) return emptyState('이 카테고리의 라이브가 없습니다');
  const more = all.length > limit
    ? `<div class="grid-more"><button class="btn more-btn" data-grid-more>더 보기 <span>${all.length - limit}</span></button></div>`
    : '';
  return liveGrid(all.slice(0, limit)) + more;
}

export function homeSkeleton() {
  const card = '<div class="sk-card"><div class="sk thumb-sk"></div><div class="sk line"></div><div class="sk line short"></div></div>';
  return `
    <div class="home">
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
    this.seq = 0;
    this.category = ALL;
    this.gridLimit = GRID_STEP;
    this.popular = [];
    this.hero = [];
    this.heroSource = 'popular';
    this.heroIndex = 0;

    root.querySelector('[data-action="refresh"]').addEventListener('click', () => this.show(true));
    root.querySelector('.home-search').addEventListener('submit', (e) => {
      e.preventDefault();
      const input = e.currentTarget.querySelector('input');
      const q = input.value.trim();
      if (!q) return;
      input.value = '';
      goSearch(q);
    });
    this.body.addEventListener('click', (e) => this.onClick(e));
    this.body.addEventListener('animationend', (e) => {
      if (e.animationName !== 'queue-progress' || this.root.hidden || motionReduced()) return;
      this.setHero(this.heroIndex + 1);
    });
  }

  async show(refresh = false) {
    const seq = ++this.seq;
    this.renderGreeting();
    if (!this.body.querySelector('.home') || refresh) this.body.innerHTML = homeSkeleton();
    const loggedIn = this.isLoggedIn();
    const [following, popular, recent] = await Promise.allSettled([
      loggedIn ? api.followingLives(refresh) : Promise.resolve([]),
      api.popularLives(POPULAR_SIZE, refresh),
      api.recentChannels(8),
    ]);
    if (seq !== this.seq) return;
    // 최근 시청은 DB(재생할 때 기록)가 원본 — 비어 있으면 예전 localStorage 기록을 쓴다.
    this.dbRecent = recent.status === 'fulfilled' ? recent.value : [];
    this.render(loggedIn, following, popular);
    this.loadRails(seq);
  }

  /** 인사말의 닉네임만 다시 쓴다(요약 줄은 그대로). */
  refreshGreetingName() {
    const auth = this.getAuth();
    const nick = auth && auth.loggedIn && auth.nickname ? `${auth.nickname}님, ` : '';
    this.greet.textContent = `${nick}${greeting()}`;
  }

  renderGreeting(following = null, popular = null) {
    this.refreshGreetingName();
    if (this.date) this.date.textContent = dateLabel();
    const parts = [];
    if (following) parts.push(`<span class="pill${following.length ? ' live' : ''}"><i class="dot"></i>팔로잉 <b>${following.length}</b>명 방송 중</span>`);
    if (popular && popular.length) {
      const total = popular.reduce((s, l) => s + (l.viewerCount || 0), 0);
      parts.push(`<span class="pill">인기 라이브 <b>${esc(compactCount(total))}</b>명 시청 중</span>`);
    }
    this.summary.innerHTML = parts.length ? parts.join('') : '지금 볼 만한 라이브를 모았어요';
  }

  render(loggedIn, followingRes, popularRes) {
    const following = followingRes.status === 'fulfilled' ? [...(followingRes.value || [])].sort(byViewers) : [];
    const popular = popularRes.status === 'fulfilled' ? popularRes.value || [] : [];
    this.popular = popular;
    this.renderGreeting(loggedIn && followingRes.status === 'fulfilled' ? following : null, popular);

    const followIds = new Set(following.map((l) => l.channelId));
    this.heroSource = following.length ? 'following' : 'popular';
    this.hero = (following.length ? following : popular).slice(0, HERO_SIZE);
    if (this.heroIndex >= this.hero.length) this.heroIndex = 0;

    const liveById = new Map([...popular, ...following].map((l) => [l.channelId, l]));
    const recent = (this.dbRecent && this.dbRecent.length ? this.dbRecent : recentChannels()).slice(0, 8);

    let html = '<div class="home">';
    if (!loggedIn) html += loginBanner();
    else if (followingRes.status === 'rejected') {
      // 원문 대신 쉬운 안내 + 다시 시도(app.js 가 홈을 다시 불러온다) — 다른 화면의 실패와 같게
      html += `<div class="home-notice" role="alert">팔로잉 라이브를 불러오지 못했습니다 · ${esc(friendlyError(followingRes.reason))} <button class="link-btn" data-retry>다시 시도</button></div>`;
    } else if (following.length) {
      html += followingStrip(following);
    }

    if (this.hero.length) {
      html += `
        <section class="hero" aria-label="${this.heroSource === 'following' ? '팔로잉 추천' : '지금 뜨는 라이브'}">
          <div class="hero-ambient" aria-hidden="true">${heroAmbient(this.hero[this.heroIndex])}</div>
          <div class="hero-slot">${heroSlide(this.hero[this.heroIndex], this.heroSource)}</div>
          <div class="hero-queue" role="tablist">${heroQueue(this.hero, this.heroIndex)}</div>
        </section>`;
    }

    if (recent.length) html += recentShelf(recent, liveById);

    // TOP 10 — 히어로가 인기 앞쪽을 쓰고 있으면(비로그인·팔로잉 없음) 그 다음 순위부터 겹치지 않게.
    const topFrom = following.length ? 0 : HERO_SIZE;
    const top = popular.slice(topFrom, topFrom + TOP_SIZE);
    if (top.length >= 3) html += topRail(top);
    html += '<section class="home-section" data-rail="continue" hidden></section>';
    if (following.length > HERO_SIZE) html += followingShelf(following);

    if (popularRes.status === 'fulfilled') {
      // 팔로잉이 히어로를 차지했으면 인기 격자는 팔로우 밖 채널부터(발견) — Swift popularLive 규칙.
      // 위 히어로·TOP 10 에 이미 나온 채널은 빼서 같은 카드가 두 번 보이지 않게 한다.
      const shown = new Set([...followIds, ...(top.length >= 3 ? top : []).map((l) => l.channelId), ...this.hero.map((l) => l.channelId)]);
      const discover = popular.filter((l) => !shown.has(l.channelId));
      this.discover = discover;
      html += `
        <section class="home-section">
          ${sectionHead(following.length ? '새로운 라이브 발견' : '더 많은 라이브', following.length ? '팔로우하지 않은 채널 중 지금 인기' : '카테고리로 골라 보기', '<button class="link-btn" data-goto="popular">인기 전체 보기</button>')}
          ${categoryChips(discover, this.category)}
          <div class="popular-grid">${discover.length ? popularGrid(discover, this.category, this.gridLimit) : emptyState('추천할 라이브 채널이 없습니다')}</div>
        </section>`;
    } else {
      html += errorState('인기 라이브를 불러오지 못했습니다', popularRes.reason);
    }
    html += '<section class="home-section" data-rail="clips" hidden></section>';
    html += '</div>';
    swapContent(this.body, html);
    this.preloadNextHero();
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
  async loadRails(seq) {
    const [cont, clips] = await Promise.allSettled([api.vodContinueList(12), api.popularClips('WITHIN_1_DAY', 'POPULAR')]);
    if (seq !== this.seq) return;
    const fill = (name, title, sub, html, goto) => {
      const box = this.body.querySelector(`[data-rail="${name}"]`);
      if (!box || !html) return;
      box.innerHTML = `${sectionHead(title, sub, `${shelfNav}<button class="link-btn" data-goto-lib="${goto}">전체 보기</button>`)}${html}`;
      box.hidden = false;
    };
    if (cont.status === 'fulfilled' && cont.value.length) fill('continue', '이어 보기', '보던 다시보기를 멈춘 곳부터', `<div class="shelf">${cont.value.map(continueCard).join('')}</div>`, 'continue');
    if (clips.status === 'fulfilled' && clips.value.length) fill('clips', '오늘의 인기 클립', '하루 동안 가장 많이 본 순간', `<div class="shelf clip-shelf">${clips.value.slice(0, 16).map(clipCard).join('')}</div>`, 'clips');
  }

  setHero(index) {
    if (!this.hero.length) return;
    this.heroIndex = (index + this.hero.length) % this.hero.length;
    const slot = this.body.querySelector('.hero-slot');
    if (!slot) return;
    slot.innerHTML = heroSlide(this.hero[this.heroIndex], this.heroSource);
    const ambient = this.body.querySelector('.hero-ambient');
    if (ambient) ambient.innerHTML = heroAmbient(this.hero[this.heroIndex]);
    for (const q of this.body.querySelectorAll('.queue-item')) {
      const on = Number(q.dataset.heroIndex) === this.heroIndex;
      q.classList.toggle('active', on);
      q.setAttribute('aria-selected', String(on));
    }
    this.preloadNextHero();
  }

  onClick(e) {
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
      this.body.querySelector('.popular-grid').innerHTML = popularGrid(this.discover || [], this.category, this.gridLimit);
      return;
    }
    if (e.target.closest('[data-grid-more]')) {
      this.gridLimit += GRID_STEP;
      this.body.querySelector('.popular-grid').innerHTML = popularGrid(this.discover || [], this.category, this.gridLimit);
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
    if (goto) window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: goto.dataset.goto } }));
  }
}

/** 홈 검색창 → 검색 화면으로 넘겨 바로 검색한다. */
function goSearch(q) {
  window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'search', params: { query: q } } }));
}
