// 사이드바 화면들 — 팔로잉 / 인기 / 검색 / 설정. 홈은 home.js.
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';
import { swapContent } from './motion.js';
import { liveGrid, channelRow, emptyState, loadingState, compactCount, skeletonGrid, freshnessText, errorState, friendlyError, uptimeText } from './cards.js';
import { FOLLOW_SORTS, FOLLOW_FILTERS, arrangeFollowed, followedInsights, followBadges, followAgeText, parseFollowDate, FollowExtras } from './following-info.js';
import { vodCard } from './channel.js';

const loginAction = '<button class="btn primary" data-action="login">로그인</button>';

// 인기 — 원본 AllBroadcastsView: 30개씩 더 보기, 정렬(시청자·최근 시작), 카테고리 필터 (NAV-03).
const POPULAR_PAGE = 30;
const POPULAR_MAX = 300;

/** 정렬·카테고리 필터 — 순수 함수. */
export function arrangeLives(lives, { sort = 'viewers', category = '' } = {}) {
  const list = category ? lives.filter((l) => l.categoryName === category) : [...lives];
  if (sort === 'recent') list.sort((a, b) => Date.parse(b.openDate || 0) - Date.parse(a.openDate || 0));
  else list.sort((a, b) => (b.viewerCount || 0) - (a.viewerCount || 0));
  return list;
}

/** 인기 화면 머리 설명 — 정렬과 카테고리 필터. 순수 함수. */
export function popularSubtitle(sort, category = '') {
  const order = sort === 'recent' ? '최근 시작한 순' : '시청자 많은 순';
  return category ? `${category} · ${order}` : order;
}

/** 카테고리별 묶음 — [{ name, count, viewers }] 시청자 많은 순. */
export function categoryGroups(lives) {
  const map = new Map();
  for (const l of lives) {
    const name = l.categoryName || '기타';
    const g = map.get(name) || { name, count: 0, viewers: 0, thumb: l.thumbnailUrl };
    g.count += 1;
    g.viewers += l.viewerCount || 0;
    map.set(name, g);
  }
  return [...map.values()].sort((a, b) => b.viewers - a.viewers);
}

export class PopularPage {
  constructor(root) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.limit = POPULAR_PAGE;
    this.sort = 'viewers';
    this.category = '';
    this.lives = [];
    this.seq = 0;
    root.querySelector('[data-action="refresh"]')?.addEventListener('click', () => this.show(true));
    this.body.addEventListener('click', (e) => {
      const s = e.target.closest('[data-sort]');
      if (s) { this.sort = s.dataset.sort; this.render(); return; }
      const c = e.target.closest('[data-cat]');
      if (c) { this.category = c.dataset.cat; this.render(); return; }
      if (e.target.closest('[data-more]')) { this.limit = Math.min(POPULAR_MAX, this.limit + POPULAR_PAGE); this.show(); }
    });
  }

  async show(refresh = false) {
    const seq = ++this.seq;
    if (!this.lives.length || refresh) this.body.innerHTML = skeletonGrid(12);
    try {
      const lives = await api.popularLives(this.limit, refresh);
      if (seq !== this.seq) return;
      this.lives = lives;
      this.loadedAt = Date.now();
      this.render();
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('불러오지 못했습니다', e);
    }
  }

  render() {
    const cats = categoryGroups(this.lives).slice(0, 14);
    const list = arrangeLives(this.lives, { sort: this.sort, category: this.category });
    // 머리 설명도 지금 정렬·필터를 따른다(예전엔 '최근 시작'을 골라도 '시청자 많은 순'으로 남았다)
    const sub = this.root.querySelector('.page-subtitle');
    if (sub) sub.textContent = popularSubtitle(this.sort, this.category);
    swapContent(this.body, `
      <div class="list-toolbar">
        <div class="segmented"><button class="seg${this.sort === 'viewers' ? ' active' : ''}" data-sort="viewers">시청자순</button><button class="seg${this.sort === 'recent' ? ' active' : ''}" data-sort="recent">최근 시작</button></div>
        <span class="freshness">${esc(freshnessText(this.loadedAt))}</span>
        <div class="chips">
          <button class="chip${this.category === '' ? ' active' : ''}" data-cat="">전체</button>
          ${cats.map((c) => `<button class="chip${this.category === c.name ? ' active' : ''}" data-cat="${esc(c.name)}">${esc(c.name)} <span class="chip-count">${c.count}</span></button>`).join('')}
        </div>
      </div>
      ${list.length ? liveGrid(list) : emptyState('라이브 채널이 없습니다')}
      ${this.lives.length >= this.limit && this.limit < POPULAR_MAX ? '<button class="btn load-more" data-more>더 보기</button>' : ''}`);
  }
}

// 팔로잉 허브 — 원본 FollowingView: 방송 중 격자 + 전체 채널(오프라인 포함), 정렬, 카테고리, 간결 보기 (NAV-04).
export class FollowingPage {
  constructor(root, { isLoggedIn }) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.isLoggedIn = isLoggedIn;
    this.tab = 'live';
    this.sort = 'viewers';
    this.category = '';
    this.dense = false;
    this.lives = [];
    this.channels = [];
    this.allSort = 'live';
    this.allFilter = 'all';
    this.allQuery = '';
    this.extras = new FollowExtras();
    this.seq = 0;
    root.querySelector('[data-action="refresh"]')?.addEventListener('click', () => this.show(true));
    this.body.addEventListener('click', (e) => {
      const t = e.target.closest('[data-ftab]');
      if (t) { this.tab = t.dataset.ftab; this.show(); return; }
      const s = e.target.closest('[data-sort]');
      if (s) { this.sort = s.dataset.sort; this.render(); return; }
      const c = e.target.closest('[data-cat]');
      if (c) { this.category = c.dataset.cat; this.render(); return; }
      if (e.target.closest('[data-dense]')) { this.dense = !this.dense; this.render(); return; }
      const as = e.target.closest('[data-fsort]');
      if (as) { this.allSort = as.dataset.fsort; this.render(); return; }
      const af = e.target.closest('[data-ffilter]');
      if (af) { this.allFilter = af.dataset.ffilter; this.render(); return; }
      if (e.target.closest('[data-queue-multi]')) {
        const ids = arrangeLives(this.lives, { sort: this.sort, category: this.category }).slice(0, 4).map((l) => l.channelId);
        if (ids.length) window.dispatchEvent(new CustomEvent('cview:multi-queue', { detail: { channelIds: ids } }));
        return;
      }
      const ch = e.target.closest('[data-open-channel-id]');
      if (ch) window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: ch.dataset.openChannelId } }));
    });
    // 전체 채널 이름 찾기 — 목록만 다시 그린다(입력창은 그대로 두어 커서를 잃지 않게).
    this.body.addEventListener('input', (e) => {
      if (!e.target.matches('[data-fquery]')) return;
      this.allQuery = e.target.value;
      this.renderAllList();
    });
  }

  async show(refresh = false) {
    if (!this.isLoggedIn()) {
      this.body.innerHTML = emptyState('네이버 로그인이 필요합니다', '로그인하면 팔로우한 채널의 라이브를 볼 수 있습니다', loginAction);
      return;
    }
    const seq = ++this.seq;
    const hasData = this.tab === 'live' ? this.lives.length : this.channels.length;
    if (!hasData || refresh) this.body.innerHTML = this.tab === 'live' ? skeletonGrid(8) : loadingState();
    try {
      if (this.tab === 'live') this.lives = await api.followingLives(refresh);
      else this.channels = await api.followingChannelsAll(refresh);
      if (seq === this.seq) this.render();
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('불러오지 못했습니다', e);
    }
  }

  render() {
    const tabs = `<div class="segmented"><button class="seg${this.tab === 'live' ? ' active' : ''}" data-ftab="live">방송 중</button><button class="seg${this.tab === 'all' ? ' active' : ''}" data-ftab="all">전체 채널</button></div>`;
    if (this.tab === 'live') {
      const cats = categoryGroups(this.lives).slice(0, 12);
      const list = arrangeLives(this.lives, { sort: this.sort, category: this.category });
      const sortSeg = `<div class="segmented"><button class="seg${this.sort === 'viewers' ? ' active' : ''}" data-sort="viewers">시청자순</button><button class="seg${this.sort === 'recent' ? ' active' : ''}" data-sort="recent">최근 시작</button></div>`;
      const body = list.length
        ? (this.dense ? `<div class="dense-list">${list.map(denseRow).join('')}</div>` : liveGrid(list))
        : emptyState('라이브 채널이 없습니다', '팔로우한 채널 중 방송 중인 채널이 없습니다');
      swapContent(this.body, `
        ${followingInsights(this.lives, list)}
        <div class="list-toolbar">${tabs}${sortSeg}<button class="btn ghost" data-dense>${this.dense ? '격자 보기' : '간결 보기'}</button>
          <div class="chips"><button class="chip${this.category === '' ? ' active' : ''}" data-cat="">전체 ${this.lives.length}</button>${cats.map((c) => `<button class="chip${this.category === c.name ? ' active' : ''}" data-cat="${esc(c.name)}">${esc(c.name)} <span class="chip-count">${c.count}</span></button>`).join('')}</div>
        </div>${body}`);
      return;
    }
    const n = (id) => (id === 'all' ? this.channels.length
      : id === 'live' ? this.channels.filter((c) => c.isLive).length
        : id === 'offline' ? this.channels.filter((c) => !c.isLive).length
          : this.channels.filter((c) => c.notification === true).length);
    const hasNotify = this.channels.some((c) => typeof c.notification === 'boolean');
    const hasDates = this.channels.some((c) => c.followDate);
    const sortSeg = `<div class="segmented">${FOLLOW_SORTS.filter((o) => hasDates || (o.id !== 'newest' && o.id !== 'oldest')).map((o) => `<button class="seg${this.allSort === o.id ? ' active' : ''}" data-fsort="${o.id}">${o.label}</button>`).join('')}</div>`;
    const chips = `<div class="chips">${FOLLOW_FILTERS.filter((f) => hasNotify || f.id !== 'notify').map((f) => `<button class="chip${this.allFilter === f.id ? ' active' : ''}" data-ffilter="${f.id}">${f.label} <span class="chip-count">${n(f.id)}</span></button>`).join('')}</div>`;
    swapContent(this.body, `
      ${followedInsights(this.channels)}
      <div class="list-toolbar">${tabs}${sortSeg}<input type="search" class="follow-find" data-fquery placeholder="채널 이름 찾기" value="${esc(this.allQuery)}" aria-label="팔로우한 채널 이름 찾기">${chips}</div>
      <div class="follow-all"></div>`);
    this.renderAllList();
  }

  renderAllList() {
    const box = this.body.querySelector('.follow-all');
    if (!box) return;
    const list = arrangeFollowed(this.channels, { sort: this.allSort, filter: this.allFilter, query: this.allQuery });
    box.innerHTML = list.length
      ? `<div class="dense-list">${list.map((c) => followedRow(c)).join('')}</div>`
      : emptyState(this.channels.length ? '조건에 맞는 채널이 없습니다' : '팔로우한 채널이 없습니다');
    this.extras.attach(box);
  }
}

/** 팔로잉 인사이트 레일 (NAV-05) — 방송 중·총 시청자·많은 카테고리·막 시작, 이 목록 멀티뷰로 담기. */
export function followingInsights(lives, shown, now = Date.now()) {
  if (!lives.length) return '';
  const viewers = lives.reduce((a, l) => a + (l.viewerCount || 0), 0);
  const top = categoryGroups(lives)[0];
  const fresh = lives.filter((l) => l.openDate && now - Date.parse(l.openDate) < 30 * 60 * 1000).length;
  const longest = lives.filter((l) => Number.isFinite(Date.parse(l.openDate || ''))).sort((a, b) => Date.parse(a.openDate) - Date.parse(b.openDate))[0];
  const item = (label, value) => `<div class="insight"><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`;
  return `<div class="insight-rail">
    ${item('방송 중', `${lives.length}개`)}
    ${item('총 시청자', compactCount(viewers))}
    ${top ? item('가장 많은 카테고리', `${top.name} · ${top.count}`) : ''}
    ${item('평균 시청자', compactCount(Math.round(viewers / lives.length)))}
    ${item('30분 안에 시작', `${fresh}개`)}
    ${longest ? item('가장 오래 방송 중', `${longest.channelName} · ${uptimeText(longest.openDate, now)}`) : ''}
    <div class="grow"></div>
    ${shown.length > 1 ? `<button class="btn primary" data-queue-multi title="지금 목록의 위 4개 채널을 멀티라이브로 엽니다">이 목록 멀티라이브로 (${Math.min(4, shown.length)})</button>` : ''}
  </div>`;
}

function denseRow(l) {
  const avatar = safeUrl(l.channelImageUrl);
  return `<div class="dense-row live-dense card" tabindex="0" data-channel-id="${esc(l.channelId)}" data-channel-name="${esc(l.channelName)}" data-channel-image="${esc(avatar)}">
    <span class="card-avatar" data-open-channel>${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}</span>
    <span class="dense-name ellipsis"><b>${esc(l.channelName)}</b> <span>${esc(l.categoryName || '')}</span></span>
    <span class="dense-title ellipsis">${esc(l.liveTitle || '')}</span>
    ${l.openDate ? `<span class="dense-uptime" title="방송 경과 시간">${esc(uptimeText(l.openDate))}</span>` : '<span></span>'}
    <span class="dense-viewers"><i class="dot"></i>${esc(l.viewerCountText)}</span>
  </div>`;
}

function followedRow(c) {
  const avatar = safeUrl(c.channelImageUrl);
  const followed = parseFollowDate(c.followDate);
  const since = followAgeText(followed);
  return `<div class="dense-row follow-row${c.isLive ? ' card' : ''}" tabindex="0" data-fid="${esc(c.channelId)}" data-live="${c.isLive ? 1 : 0}" ${c.isLive ? `data-channel-id="${esc(c.channelId)}"` : `data-open-channel-id="${esc(c.channelId)}"`}>
    <span class="card-avatar" ${c.isLive ? 'data-open-channel' : ''}>${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}</span>
    <span class="dense-name ellipsis"><b>${esc(c.channelName)}</b>${followBadges(c)} <span>${c.isLive ? esc(c.categoryName || '방송 중') : '오프라인'}</span></span>
    <span class="dense-title ellipsis${c.isLive ? '' : ' fx-last'}">${esc(c.liveTitle || '')}</span>
    <span class="follow-meta"><span class="fx-followers"></span>${since ? `<span class="fx-since" title="팔로우한 날 ${esc(new Date(followed).toLocaleDateString('ko-KR'))}">${esc(since)}</span>` : ''}</span>
    ${c.isLive ? `<span class="dense-viewers"><i class="dot"></i>${esc(compactCount(c.viewerCount || 0))}</span>` : '<span class="dense-viewers off">채널 보기</span>'}
  </div>`;
}

// 통합 검색 — 원본 SearchViews: 라이브·채널·다시보기, 탭, 최근 검색어 (NAV-06).
const RECENT_SEARCH_KEY = 'cview.recentSearches';
function recentSearches() {
  try { return JSON.parse(localStorage.getItem(RECENT_SEARCH_KEY) || '[]').slice(0, 12); } catch { return []; }
}
function saveRecentSearch(q) {
  const list = [q, ...recentSearches().filter((x) => x !== q)].slice(0, 12);
  try { localStorage.setItem(RECENT_SEARCH_KEY, JSON.stringify(list)); } catch { /* 무시 */ }
}

const VOD_PAGE = 24;

export class SearchPage {
  constructor(root) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.input = root.querySelector('input[type=search]');
    this.input.placeholder = '채널, 라이브, 다시보기 검색…';
    this.acceptsParams = true;
    this.seq = 0;
    this.tab = 'all';
    this.query = '';
    this.lastQuery = ''; // 마지막으로 제출한 검색어 — 오류 화면의 "다시 시도"가 이것으로 다시 찾는다
    this.result = null; // { lives, channels } — 채널·라이브 검색이 실패했으면 null
    this.mainError = null;
    this.videos = null; // { items, hasMore, loading, error } — 다시보기 검색이 실패했으면 null
    this.videosError = null;
    root.querySelector('form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.run(this.input.value);
    });
    // 입력란이 비면(X 단추·전체 지우기) 이전 결과를 남기지 않고 처음 화면으로 — 늦게 온 응답도 무시된다
    this.input.addEventListener('input', () => {
      if (!this.input.value.trim() && !this.idle) this.run('');
    });
    this.body.addEventListener('click', (e) => {
      const r = e.target.closest('[data-recent]');
      if (r) { this.input.value = r.dataset.recent; this.run(r.dataset.recent); return; }
      const x = e.target.closest('[data-recent-remove]');
      if (x) {
        const list = recentSearches().filter((q) => q !== x.dataset.recentRemove);
        try { localStorage.setItem(RECENT_SEARCH_KEY, JSON.stringify(list)); } catch { /* 무시 */ }
        this.renderIdle();
        return;
      }
      const dc = e.target.closest('[data-discover-cat]');
      if (dc) { window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'categories', params: { category: dc.dataset.discoverCat } } })); return; }
      if (e.target.closest('[data-recent-clear]')) { try { localStorage.removeItem(RECENT_SEARCH_KEY); } catch { /* 무시 */ } this.renderIdle(); return; }
      const rs = e.target.closest('[data-search-retry]');
      if (rs) { e.stopPropagation(); this.retrySource(rs.dataset.searchRetry); return; }
      if (e.target.closest('[data-vod-more]')) { e.stopPropagation(); this.loadMoreVideos(); return; }
      const t = e.target.closest('[data-stab]');
      if (t) { this.tab = t.dataset.stab; this.renderResults(); return; }
      const vod = e.target.closest('[data-vod-no]');
      if (vod) window.dispatchEvent(new CustomEvent('cview:open-vod', { detail: { videoNo: Number(vod.dataset.vodNo) } }));
      const ch = e.target.closest('.channel-row[data-channel-id]');
      if (ch && ch.dataset.openLive !== '1') { e.stopPropagation(); window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: ch.dataset.channelId } })); }
    }, true);
    this.renderIdle();
  }

  show(params = {}) {
    if (params && params.query) { this.input.value = params.query; this.run(params.query); }
    this.input.focus();
  }

  /** 오류 화면의 "다시 시도"(app.js) — 경로 매개변수가 없어도 마지막으로 제출한 검색어로 다시 찾는다. */
  retry() {
    const q = this.lastQuery;
    if (!q) { this.show(); return; }
    this.input.value = q;
    this.run(q);
  }

  async loadDiscovery() {
    if (this.discovery) return this.discovery;
    try {
      const lives = await api.popularLives(50, false);
      this.discovery = { cats: categoryGroups(lives).slice(0, 14), top: lives.slice(0, 8) };
    } catch { this.discovery = { cats: [], top: [] }; }
    return this.discovery;
  }

  async renderDiscovery() {
    const box = this.body.querySelector('.discovery');
    if (!box) return;
    const d = await this.loadDiscovery();
    if (!this.body.contains(box)) return;
    box.innerHTML = `
      ${d.cats.length ? `<div class="section-head"><h2 class="section-title">지금 많이 보는 카테고리</h2></div>
        <div class="chips">${d.cats.map((c) => `<button class="chip" data-discover-cat="${esc(c.name)}">${esc(c.name)} <span class="chip-count">${compactCount(c.viewers)}</span></button>`).join('')}</div>` : ''}
      ${d.top.length ? `<div class="section-head"><h2 class="section-title">인기 채널</h2></div>
        <div class="discover-rail">${d.top.map((l) => { const a = safeUrl(l.channelImageUrl); return `<button class="discover-item" data-recent="${esc(l.channelName)}" title="${esc(l.liveTitle || '')}">${a ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(a, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}<span class="ellipsis">${esc(l.channelName)}</span><small>${esc(l.viewerCountText)}</small></button>`; }).join('')}</div>` : ''}`;
  }

  renderIdle() {
    this.idle = true;
    const recent = recentSearches();
    setTimeout(() => this.renderDiscovery(), 0);
    this.body.innerHTML = '<div class="discovery-top"></div><div class="discovery"></div>';
    this.body.querySelector('.discovery-top').innerHTML = recent.length
      ? `<div class="section-head"><h2 class="section-title">최근 검색어</h2><div class="grow"></div><button class="link-btn" data-recent-clear>모두 지우기</button></div>
         <div class="chips">${recent.map((q) => `<span class="chip recent-chip"><button class="link-btn" data-recent="${esc(q)}">${esc(q)}</button><button class="chip-x" data-recent-remove="${esc(q)}" title="지우기">✕</button></span>`).join('')}</div>`
      : emptyState('검색어를 입력하세요', '채널 이름, 방송 제목, 다시보기로 찾을 수 있습니다');
  }

  async run(keyword) {
    const q = keyword.trim();
    const seq = ++this.seq; // 비운 검색도 순번을 올린다 — 늦게 온 이전 결과가 최근 검색 화면을 덮지 않게
    if (!q) {
      this.query = '';
      this.result = null;
      this.videos = null;
      this.renderIdle();
      return;
    }
    this.lastQuery = q;
    this.idle = false;
    saveRecentSearch(q);
    this.body.innerHTML = loadingState();
    const [main, vods] = await Promise.allSettled([api.search(q), api.searchVideos(q, 0, VOD_PAGE)]);
    if (seq !== this.seq) return;
    if (main.status === 'rejected' && vods.status === 'rejected') {
      this.query = '';
      this.result = null;
      this.videos = null;
      this.body.innerHTML = errorState('검색하지 못했습니다', main.reason);
      return;
    }
    this.query = q;
    this.setMain(main);
    this.setVideos(vods);
    this.renderResults();
  }

  setMain(res) {
    this.result = res.status === 'fulfilled' ? res.value : null;
    this.mainError = res.status === 'rejected' ? res.reason : null;
  }

  setVideos(res) {
    this.videos = res.status === 'fulfilled' ? { items: res.value.items || [], hasMore: Boolean(res.value.hasMore), loading: false, error: null } : null;
    this.videosError = res.status === 'rejected' ? res.reason : null;
  }

  /** 한쪽 검색만 실패했을 때 그쪽만 다시 받는다 — 받은 다른 쪽 결과는 그대로 둔다. */
  async retrySource(kind) {
    const q = this.query;
    const seq = this.seq;
    if (!q) return;
    if (kind === 'main') { this.mainError = null; this.mainRetrying = true; } else { this.videosError = null; this.videosRetrying = true; }
    this.renderResults();
    const [res] = await Promise.allSettled([kind === 'main' ? api.search(q) : api.searchVideos(q, 0, VOD_PAGE)]);
    if (seq !== this.seq || q !== this.query) return; // 그사이 다른 검색으로 바뀌었다
    if (kind === 'main') { this.mainRetrying = false; this.setMain(res); } else { this.videosRetrying = false; this.setVideos(res); }
    this.renderResults();
  }

  /** 다시보기 다음 구간 — 받은 개수를 시작 위치로. 실패하면 받은 목록은 두고 "다시 시도"를 보인다. */
  async loadMoreVideos() {
    const v = this.videos;
    if (!v || v.loading || !v.hasMore) return;
    const q = this.query;
    const seq = this.seq;
    v.loading = true;
    v.error = null;
    this.renderResults();
    try {
      const page = await api.searchVideos(q, v.items.length, VOD_PAGE);
      if (seq !== this.seq || q !== this.query || v !== this.videos) return;
      const seen = new Set(v.items.map((x) => x.videoNo));
      const fresh = (page.items || []).filter((x) => !seen.has(x.videoNo));
      v.items.push(...fresh);
      v.hasMore = Boolean(page.hasMore) && fresh.length > 0;
    } catch (e) {
      if (seq !== this.seq || q !== this.query || v !== this.videos) return;
      v.error = e;
    }
    v.loading = false;
    this.renderResults();
  }

  renderResults() {
    if (!this.query) return;
    const mainFailed = !this.result;
    const vodFailed = !this.videos;
    const { lives, channels } = this.result || { lives: [], channels: [] };
    const v = this.videos || { items: [], hasMore: false };
    const videos = v.items;
    const n = (failed, count, more = false) => (failed ? '실패' : `${count}${more ? '+' : ''}`);
    const tabs = [['all', '전체'], ['lives', `라이브 ${n(mainFailed, lives.length)}`], ['channels', `채널 ${n(mainFailed, channels.length)}`], ['videos', `다시보기 ${n(vodFailed, videos.length, v.hasMore)}`]];
    const tabBar = `<div class="segmented search-tabs">${tabs.map(([id, l]) => `<button class="seg${this.tab === id ? ' active' : ''}" data-stab="${id}">${l}</button>`).join('')}</div>`;
    const sec = (id, title, items, html, limit, more = false) => {
      if (!items.length) return '';
      const full = this.tab === id;
      if (this.tab !== 'all' && !full) return '';
      return `<div class="section-head"><h2 class="section-title">${title}</h2><div class="grow"></div>${!full && (items.length > limit || more) ? `<button class="link-btn" data-stab="${id}">전체 보기</button>` : ''}</div>${html(full ? items : items.slice(0, limit))}`;
    };
    // 실패한 쪽 — "결과 없음"과 구분해 알리고 그쪽만 다시 받는다
    const failBox = (kind, label, err, retrying) => (retrying
      ? `<div class="search-source-error" role="status">${esc(label)} 다시 찾는 중…</div>`
      : `<div class="search-source-error" role="alert"><span><strong>${esc(label)} 검색에 실패했습니다</strong><small>${esc(friendlyError(err))}</small></span><button class="btn" data-search-retry="${kind}">다시 시도</button></div>`);
    const showMain = this.tab === 'all' || this.tab === 'lives' || this.tab === 'channels';
    const showVod = this.tab === 'all' || this.tab === 'videos';
    const errors = [
      showMain && mainFailed ? failBox('main', '라이브·채널', this.mainError, this.mainRetrying) : '',
      showVod && vodFailed ? failBox('videos', '다시보기', this.videosError, this.videosRetrying) : '',
    ].join('');
    let vodMore = '';
    if (this.tab === 'videos' && this.videos) {
      if (v.loading) vodMore = '<button class="btn load-more" disabled>불러오는 중…</button>';
      else if (v.error) vodMore = `<div class="search-source-error" role="alert"><span><strong>다시보기를 더 불러오지 못했습니다</strong><small>${esc(friendlyError(v.error))}</small></span><button class="btn" data-vod-more>다시 시도</button></div>`;
      else if (v.hasMore) vodMore = '<button class="btn load-more" data-vod-more>더 보기</button>';
    }
    const html = [
      sec('lives', '라이브', lives, (l) => liveGrid(l), 8),
      sec('channels', '채널', channels, (c) => `<div class="channel-list">${c.map(channelRow).join('')}</div>`, 6),
      sec('videos', '다시보기', videos, (x) => `<div class="grid">${x.map(vodCard).join('')}</div>`, 8, v.hasMore),
    ].join('');
    const empty = !html && !errors ? emptyState('검색 결과가 없습니다', `"${this.query}"`) : '';
    this.body.innerHTML = tabBar + errors + html + vodMore + empty;
  }
}
