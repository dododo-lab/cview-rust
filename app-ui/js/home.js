// 홈 — 인사 헤더 / 방송 중인 팔로잉 스트립 / 추천 히어로 / 최근 시청 / 팔로잉 선반 / 카테고리 칩 + 인기 격자.
// Swift `HomeView_v3` 의 "지금 볼 라이브를 먼저" 원칙을 따르되, 지금 Rust 명령으로 받을 수 있는
// 데이터(팔로잉·인기 라이브)와 로컬 최근 시청 기록만으로 그린다.
import { api } from './api.js';
import { esc, safeUrl, icons } from './dom.js';
import { liveCard, emptyState, liveGrid, recentChannels, uptimeText, compactCount } from './cards.js';
import { clipCard } from './channel.js';
import { continueCard } from './media.js';

const HERO_SIZE = 5;
// 자동 넘김 — 대기열 진행 막대(CSS `queue-progress`, 7초)가 끝나면 다음 장으로. 호버 중엔 CSS 가 멈춘다.
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const POPULAR_SIZE = 48;
const POPULAR_GRID = 24;
const CHIP_LIMIT = 8;
const ALL = '';

function greeting(now = new Date()) {
  const h = now.getHours();
  if (h < 5) return '늦은 밤이에요';
  if (h < 11) return '좋은 아침이에요';
  if (h < 17) return '좋은 오후예요';
  return '좋은 저녁이에요';
}

const byViewers = (a, b) => (b.viewerCount || 0) - (a.viewerCount || 0);

function avatarImg(url, cls = 'avatar') {
  const src = safeUrl(url);
  return src
    ? `<img class="${cls}" src="${esc(src)}" alt="" loading="lazy" referrerpolicy="no-referrer">`
    : `<div class="${cls}"></div>`;
}

function channelData(live) {
  return `data-channel-id="${esc(live.channelId)}" data-channel-name="${esc(live.channelName)}" data-channel-image="${esc(safeUrl(live.channelImageUrl))}"`;
}

// ── 조각들 ─────────────────────────────────────────────

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
      <div class="section-head">
        <h2 class="section-title">방송 중인 팔로잉 <span class="count">${lives.length}</span></h2>
        <div class="grow"></div>
        <button class="link-btn" data-goto="following">전체 보기</button>
      </div>
      <div class="strip">${items}</div>
    </section>`;
}

function heroSlide(live, source) {
  const thumb = safeUrl(live.thumbnailUrl);
  const up = uptimeText(live.openDate);
  return `
    <article class="hero-main card" tabindex="0" ${channelData(live)}>
      <div class="hero-media">
        ${thumb ? `<img src="${esc(thumb)}" alt="" referrerpolicy="no-referrer">` : ''}
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
        <span class="btn primary lg hero-cta">${icons.play}시청하기</span>
      </div>
    </article>`;
}

function heroQueue(lives, active) {
  return lives.map((l, i) => {
    const thumb = safeUrl(l.thumbnailUrl);
    return `
      <button class="queue-item${i === active ? ' active' : ''}" data-hero-index="${i}" title="${esc(l.liveTitle)}">
        <span class="queue-thumb">${thumb ? `<img src="${esc(thumb)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}<span class="queue-progress"></span></span>
        <span class="queue-text">
          <span class="queue-title">${esc(l.liveTitle || '제목 없음')}</span>
          <span class="queue-meta ellipsis">${esc(l.channelName)} · ${esc(l.viewerCountText)}명</span>
        </span>
      </button>`;
  }).join('');
}

function recentShelf(recent, liveById) {
  const items = recent.map((r) => {
    const live = liveById.get(r.channelId);
    return `
      <button class="recent-item${live ? ' is-live' : ''}" data-channel-id="${esc(r.channelId)}" data-channel-name="${esc(r.channelName)}" data-channel-image="${esc(r.channelImageUrl)}">
        ${avatarImg(r.channelImageUrl)}
        <span class="recent-text">
          <span class="ellipsis"><strong>${esc(r.channelName || '채널')}</strong></span>
          <span class="recent-meta ellipsis">${live ? `<b>LIVE</b> ${esc(live.viewerCountText)}명` : '오프라인일 수 있음'}</span>
        </span>
      </button>`;
  }).join('');
  return `
    <section class="home-section">
      <div class="section-head"><h2 class="section-title">최근 시청</h2></div>
      <div class="recent-row">${items}</div>
    </section>`;
}

function followingShelf(lives) {
  return `
    <section class="home-section">
      <div class="section-head">
        <h2 class="section-title">팔로잉 라이브</h2>
        <div class="grow"></div>
        <button class="icon-btn round" data-shelf="-1" title="이전">${icons.chevronLeft}</button>
        <button class="icon-btn round" data-shelf="1" title="다음">${icons.chevronRight}</button>
      </div>
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
  const chip = (value, label) =>
    `<button class="chip${value === selected ? ' active' : ''}" data-category="${esc(value)}">${esc(label)}</button>`;
  return `<div class="chips" role="tablist">${chip(ALL, '전체')}${top.map((n) => chip(n, n)).join('')}</div>`;
}

function popularGrid(popular, category) {
  const list = (category ? popular.filter((l) => l.categoryName === category) : popular).slice(0, POPULAR_GRID);
  return list.length ? liveGrid(list) : emptyState('이 카테고리의 라이브가 없습니다');
}

export function homeSkeleton() {
  const card = '<div class="sk-card"><div class="sk thumb-sk"></div><div class="sk line"></div><div class="sk line short"></div></div>';
  return `
    <div class="home">
      <div class="hero"><div class="sk hero-sk"></div><div class="hero-queue">${'<div class="sk queue-sk"></div>'.repeat(4)}</div></div>
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
    this.summary = root.querySelector('.home-summary');
    this.isLoggedIn = isLoggedIn;
    this.getAuth = getAuth;
    this.seq = 0;
    this.category = ALL;
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
      if (e.animationName !== 'queue-progress' || this.root.hidden || reducedMotion.matches) return;
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

  renderGreeting(following = null, popular = null) {
    const auth = this.getAuth();
    const nick = auth && auth.loggedIn && auth.nickname ? `${auth.nickname}님, ` : '';
    this.greet.textContent = `${nick}${greeting()}`;
    const parts = [];
    if (following) parts.push(`<span>팔로잉 <b>${following.length}</b>명 방송 중</span>`);
    if (popular && popular.length) {
      const total = popular.reduce((s, l) => s + (l.viewerCount || 0), 0);
      parts.push(`<span>인기 라이브 <b>${esc(compactCount(total))}</b>명 시청 중</span>`);
    }
    this.summary.innerHTML = parts.length ? parts.join('<i class="sep"></i>') : '지금 볼 만한 라이브를 모았어요';
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
      html += `<div class="home-notice">팔로잉 라이브를 불러오지 못했습니다 · ${esc(followingRes.reason && followingRes.reason.message)}</div>`;
    } else if (following.length) {
      html += followingStrip(following);
    }

    if (this.hero.length) {
      html += `
        <section class="hero">
          <div class="hero-slot">${heroSlide(this.hero[this.heroIndex], this.heroSource)}</div>
          <div class="hero-queue">
            <div class="queue-head">${this.heroSource === 'following' ? '팔로잉 추천' : '지금 뜨는 라이브'}</div>
            ${heroQueue(this.hero, this.heroIndex)}
          </div>
        </section>`;
    }

    if (recent.length) html += recentShelf(recent, liveById);
    html += '<section class="home-section" data-rail="continue" hidden></section>';
    if (following.length > HERO_SIZE) html += followingShelf(following);

    if (popularRes.status === 'fulfilled') {
      // 팔로잉이 히어로를 차지했으면 인기 격자는 팔로우 밖 채널부터(발견) — Swift popularLive 규칙.
      const discover = following.length ? popular.filter((l) => !followIds.has(l.channelId)) : popular.slice(HERO_SIZE);
      this.discover = discover;
      html += `
        <section class="home-section">
          <div class="section-head">
            <h2 class="section-title">${following.length ? '새로운 라이브 발견' : '지금 인기 라이브'}</h2>
            <div class="grow"></div>
            <button class="link-btn" data-goto="popular">인기 전체 보기</button>
          </div>
          ${categoryChips(discover, this.category)}
          <div class="popular-grid">${discover.length ? popularGrid(discover, this.category) : emptyState('추천할 라이브 채널이 없습니다')}</div>
        </section>`;
    } else {
      html += emptyState('인기 라이브를 불러오지 못했습니다', popularRes.reason && popularRes.reason.message);
    }
    html += '<section class="home-section" data-rail="clips" hidden></section>';
    html += '</div>';
    this.body.innerHTML = html;
  }

  /** 이어 보기·인기 클립 레일 — 첫 화면을 막지 않게 나중에 채운다 (NAV-14). */
  async loadRails(seq) {
    const [cont, clips] = await Promise.allSettled([api.vodContinueList(12), api.popularClips('WITHIN_1_DAY', 'POPULAR')]);
    if (seq !== this.seq) return;
    const fill = (name, title, html, goto) => {
      const box = this.body.querySelector(`[data-rail="${name}"]`);
      if (!box || !html) return;
      box.innerHTML = `<div class="section-head"><h2 class="section-title">${title}</h2><div class="grow"></div><button class="link-btn" data-goto-lib="${goto}">전체 보기</button></div>${html}`;
      box.hidden = false;
    };
    if (cont.status === 'fulfilled' && cont.value.length) fill('continue', '이어 보기', `<div class="shelf">${cont.value.map(continueCard).join('')}</div>`, 'continue');
    if (clips.status === 'fulfilled' && clips.value.length) fill('clips', '오늘의 인기 클립', `<div class="shelf clip-shelf">${clips.value.slice(0, 16).map(clipCard).join('')}</div>`, 'clips');
  }

  setHero(index) {
    if (!this.hero.length) return;
    this.heroIndex = (index + this.hero.length) % this.hero.length;
    const slot = this.body.querySelector('.hero-slot');
    if (!slot) return;
    slot.innerHTML = heroSlide(this.hero[this.heroIndex], this.heroSource);
    for (const q of this.body.querySelectorAll('.queue-item')) {
      q.classList.toggle('active', Number(q.dataset.heroIndex) === this.heroIndex);
    }
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
      for (const c of this.body.querySelectorAll('.chip')) c.classList.toggle('active', c === chip);
      this.body.querySelector('.popular-grid').innerHTML = popularGrid(this.discover || [], this.category);
      return;
    }
    const shelfBtn = e.target.closest('[data-shelf]');
    if (shelfBtn) {
      const shelf = shelfBtn.closest('.home-section').querySelector('.shelf');
      shelf.scrollBy({ left: Number(shelfBtn.dataset.shelf) * shelf.clientWidth * 0.9, behavior: 'smooth' });
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
