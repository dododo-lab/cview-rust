// 오른쪽 인사이트 패널 — 원본 DiscoveryInsightRail(인기·카테고리)·FollowingLiveInsightRail(팔로잉)을 옮기고,
// 나머지 메뉴(홈·검색·멀티채팅·보관함·통계·설정)는 앱이 이미 가진 데이터로 화면에 맞는 요약을 보여 준다.
// 라이브(시청)·멀티라이브에서는 숨긴다. 접기 상태는 기억하고, 좁은 창에서는 자동으로 숨긴다(CSS).
import { api } from './api.js';
import { esc, safeUrl } from './dom.js';
import { compactCount } from './cards.js';
import { categoryGroups } from './pages.js';

const COLLAPSE_KEY = 'cview.insights.collapsed';
const REFRESH_MS = 60000;
const CACHE_MS = 45000;
export const INSIGHT_ROUTES = new Set(['home', 'following', 'popular', 'categories', 'search', 'multichat', 'library', 'stats', 'settings']);

const ICON = {
  spark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5 18 18M6 18l2.5-2.5M15.5 8.5 18 6"/></svg>',
  flame: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12.5 2.5c.6 3.4 5 5.6 5 10.6a5.5 5.5 0 0 1-11 0c0-2.4 1.1-4 2.4-5.2.3 1.7 1.1 2.8 2.2 3.4-.1-3.4-.2-6.2 1.4-8.8Z"/></svg>',
  grid: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="3" width="8" height="8" rx="2.5"/><rect x="13" y="3" width="8" height="8" rx="2.5" opacity=".6"/><rect x="3" y="13" width="8" height="8" rx="2.5" opacity=".6"/><rect x="13" y="13" width="8" height="8" rx="2.5"/></svg>',
  people: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm7.5 0a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2.5 19c0-3 2.9-5.5 6.5-5.5s6.5 2.5 6.5 5.5v1h-13v-1Zm14.5 1v-1c0-1.7-.6-3.2-1.6-4.4.4-.1.7-.1 1.1-.1 3 0 5.5 2 5.5 4.5v1h-5Z"/></svg>',
  clock: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>',
  heart: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 20.3 4.6 13a4.8 4.8 0 0 1 6.8-6.8l.6.6.6-.6A4.8 4.8 0 0 1 19.4 13L12 20.3Z"/></svg>',
  box: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7.5 12 4l8 3.5v9L12 20l-8-3.5z"/><path d="M4 7.5 12 11l8-3.5M12 11v9"/></svg>',
  gear: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/></svg>',
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  chevron: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m9 6 6 6-6 6"/></svg>',
  insight: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/></svg>',
};

// ── 데이터(짧게 캐시) ─────────────────────────────
const cache = new Map();
async function cached(key, fn) {
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < CACHE_MS) return hit.value;
  const value = await fn();
  cache.set(key, { at: Date.now(), value });
  return value;
}
const safe = async (p, fallback) => { try { return await p; } catch { return fallback; } };

// ── 순수 계산(원본 PopularInsights / FollowingInsights 규칙) ─────
/** 라이브 목록 요약 — 개수·총/평균 시청자·카테고리 수. 순수 함수. */
export function liveSummary(lives) {
  const viewerTotal = lives.reduce((a, l) => a + (l.viewerCount || 0), 0);
  const cats = new Set(lives.map((l) => l.categoryName).filter(Boolean));
  return { liveCount: lives.length, viewerTotal, average: lives.length ? Math.round(viewerTotal / lives.length) : 0, categoryCount: cats.size };
}

/** 막 시작한 방송(최근 순, 기본 3시간 안). 순수 함수. */
export function freshLives(lives, now = Date.now(), withinMs = 3 * 3600 * 1000, limit = 4) {
  return lives
    .filter((l) => l.openDate && now - Date.parse(l.openDate) < withinMs && now >= Date.parse(l.openDate))
    .sort((a, b) => Date.parse(b.openDate) - Date.parse(a.openDate))
    .slice(0, limit);
}

/** 시청 규모 구간(1천 미만·1천~1만·1만 이상). 순수 함수. */
export function viewerBuckets(lives) {
  const b = [['1만 이상', 0], ['1천~1만', 0], ['1천 미만', 0]];
  for (const l of lives) {
    const v = l.viewerCount || 0;
    b[v >= 10000 ? 0 : v >= 1000 ? 1 : 2][1] += 1;
  }
  return b;
}

function agoText(iso, now = Date.now()) {
  const m = Math.max(0, Math.round((now - Date.parse(iso)) / 60000));
  if (m < 60) return `${m}분 전 시작`;
  return `${Math.floor(m / 60)}시간 ${m % 60}분 전 시작`;
}

function durationText(sec) {
  const s = Math.max(0, Math.round(Number(sec) || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}시간 ${m}분` : `${m}분`;
}

function bytesText(n) {
  const v = Number(n) || 0;
  if (v < 1024 * 1024) return `${Math.round(v / 1024)} KB`;
  if (v < 1024 ** 3) return `${(v / 1024 / 1024).toFixed(1)} MB`;
  return `${(v / 1024 ** 3).toFixed(1)} GB`;
}

// ── 그리기 조각 ─────────────────────────────
const header = (title, subtitle) => `<div class="ip-head"><span class="ip-head-icon">${ICON.insight}</span><div class="ip-head-text"><strong>${esc(title)}</strong><small>${esc(subtitle)}</small></div></div>`;
const tiles = (items) => `<div class="ip-tiles">${items.map(([v, l]) => `<div class="ip-tile"><strong>${esc(v)}</strong><span>${esc(l)}</span></div>`).join('')}</div>`;
const section = (icon, title, body) => (body ? `<section class="ip-section"><h4><span class="ip-sec-icon">${ICON[icon] || ''}</span>${esc(title)}</h4>${body}</section>` : '');
function bars(items, fmt = compactCount) {
  if (!items.length) return '';
  const max = Math.max(...items.map((x) => x.value), 1);
  return `<ol class="ip-bars">${items.map((x) => `<li${x.channelId ? ` data-ip-channel="${esc(x.channelId)}"` : ''}${x.category ? ` data-ip-category="${esc(x.category)}"` : ''}><div class="ip-bar-row"><span class="ip-bar-name ellipsis">${esc(x.name)}</span><span class="ip-bar-val">${esc(fmt(x.value))}</span></div><span class="ip-track"><span class="ip-fill" style="width:${((x.value / max) * 100).toFixed(1)}%"></span></span></li>`).join('')}</ol>`;
}
function liveRows(lives, meta) {
  if (!lives.length) return '';
  return `<div class="ip-lives">${lives.map((l) => {
    const a = safeUrl(l.channelImageUrl);
    return `<button class="ip-live" data-ip-play="${esc(l.channelId)}" title="${esc(l.liveTitle || '')}">${a ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(a)}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}<span class="ip-live-text"><b class="ellipsis">${esc(l.channelName)}</b><small class="ellipsis">${esc(meta(l))}</small></span></button>`;
  }).join('')}</div>`;
}
function columns(values, labels) {
  const max = Math.max(...values, 1);
  return `<div class="ip-cols">${values.map((v, i) => `<div class="ip-col${i === values.length - 1 ? ' now' : ''}" title="${esc(labels[i])} · ${esc(durationText(v))}"><span style="height:${Math.max(v > 0 ? 6 : 2, (v / max) * 100).toFixed(1)}%"></span><small>${esc(labels[i])}</small></div>`).join('')}</div>`;
}
const empty = (text) => `<div class="ip-empty">${esc(text)}</div>`;
const rows = (pairs) => `<dl class="ip-kv">${pairs.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl>`;

// ── 메뉴별 내용 ─────────────────────────────
const popular = () => cached('popular', () => api.popularLives(50, false));
const following = (isLoggedIn) => (isLoggedIn() ? cached('following', () => api.followingLives(false)) : Promise.resolve(null));
const overview = () => cached('overview', () => api.insightsOverview());

function popularRail(lives, title = '실시간 인사이트') {
  const s = liveSummary(lives);
  const cats = categoryGroups(lives).slice(0, 6).map((c) => ({ name: c.name, value: c.viewers, category: c.name }));
  return header(title, s.liveCount ? `불러온 라이브 ${s.liveCount}개 기준` : '지금 방송 중인 인기 라이브가 없어요')
    + (s.liveCount ? tiles([[String(s.liveCount), '라이브'], [compactCount(s.viewerTotal), '총 시청자'], [compactCount(s.average), '평균 시청자'], [String(s.categoryCount), '카테고리']]) : '')
    + section('flame', '지금 가장 많이 보는 라이브', liveRows([...lives].sort((a, b) => b.viewerCount - a.viewerCount).slice(0, 4), (l) => `${l.categoryName || '기타'} · ${l.viewerCountText}명`))
    + section('grid', '인기 카테고리', bars(cats))
    + section('spark', '막 시작한 라이브', liveRows(freshLives(lives), (l) => agoText(l.openDate)));
}

const PROVIDERS = {
  async home({ isLoggedIn }) {
    const [f, p, o] = await Promise.all([safe(following(isLoggedIn), null), safe(popular(), []), safe(overview(), null)]);
    const lives = f || [];
    const s = liveSummary(lives);
    const today = o && o.daily.length ? o.daily[o.daily.length - 1].seconds : 0;
    return header('오늘의 인사이트', f ? `팔로잉 ${s.liveCount}명 방송 중` : '로그인하면 팔로잉 요약이 보여요')
      + tiles([[f ? String(s.liveCount) : '—', '팔로잉 방송'], [f ? compactCount(s.viewerTotal) : '—', '팔로잉 시청자'], [durationText(today), '오늘 시청'], [o ? `${o.streak}일` : '—', '연속 시청']])
      + section('flame', '지금 많이 보는 팔로잉', liveRows([...lives].sort((a, b) => b.viewerCount - a.viewerCount).slice(0, 4), (l) => `${l.categoryName || '기타'} · ${l.viewerCountText}명`))
      + section('grid', '지금 뜨는 카테고리', bars(categoryGroups(p).slice(0, 5).map((c) => ({ name: c.name, value: c.viewers, category: c.name }))))
      + (o && o.daily.length ? section('clock', '최근 7일 내 시청', columns(o.daily.slice(-7).map((d) => d.seconds), o.daily.slice(-7).map((d) => ['일', '월', '화', '수', '목', '금', '토'][new Date(d.date).getDay()]))) : '');
  },
  async following({ isLoggedIn }) {
    const lives = await safe(following(isLoggedIn), null);
    if (!lives) return header('팔로잉 인사이트', '로그인하면 팔로잉 인사이트가 보여요') + empty('네이버 로그인 후 이용할 수 있어요');
    const all = await safe(cached('followAll', () => api.followingChannelsAll(false)), []);
    const o = await safe(overview(), null);
    const s = liveSummary(lives);
    const followedIds = new Set(all.map((c) => c.channelId));
    const mostWatched = (o ? o.channels : []).filter((c) => c.seconds >= 60).slice(0, 5).map((c) => ({ name: c.name, value: c.seconds }));
    return header('팔로잉 인사이트', `팔로잉 ${all.length || '?'}명 중 ${s.liveCount}명 방송 중`)
      + tiles([[String(s.liveCount), '방송 중'], [String(all.length || followedIds.size || '—'), '전체 팔로잉'], [compactCount(s.viewerTotal), '팔로잉 시청자'], [String(s.categoryCount), '방송 분야']])
      + section('heart', '많이 보는 팔로잉', bars(mostWatched, durationText))
      + section('grid', '방송 분야', bars([...categoryGroups(lives)].sort((a, b) => b.count - a.count).slice(0, 5).map((c) => ({ name: c.name, value: c.count, category: c.name })), (v) => `${v}명`))
      + section('people', '시청 규모', bars(viewerBuckets(lives).map(([name, value]) => ({ name, value })), (v) => `${v}명`))
      + section('spark', '최근 시작한 팔로잉', liveRows(freshLives(lives), (l) => agoText(l.openDate)));
  },
  async popular() {
    return popularRail(await safe(popular(), []));
  },
  async categories() {
    const lives = await safe(popular(), []);
    const groups = categoryGroups(lives);
    const o = await safe(overview(), null);
    const live = new Map(groups.map((g) => [g.name, g.count]));
    const mine = (o ? o.categories : []).filter((c) => c.seconds >= 60).slice(0, 4);
    return header('카테고리 인사이트', groups.length ? `카테고리 ${groups.length}개 · 라이브 ${lives.length}개 기준` : '카테고리 정보를 불러오지 못했어요')
      + section('flame', '시청자 많은 카테고리', bars(groups.slice(0, 6).map((g) => ({ name: g.name, value: g.viewers, category: g.name }))))
      + section('grid', '라이브가 많은 카테고리', bars([...groups].sort((a, b) => b.count - a.count).slice(0, 5).map((g) => ({ name: g.name, value: g.count, category: g.name })), (v) => `${v}개`))
      + section('heart', '자주 본 카테고리', mine.length ? `<div class="ip-chips">${mine.map((c) => `<button class="ip-chip" data-ip-category="${esc(c.name)}">${esc(c.name)}<small>지금 라이브 ${live.get(c.name) || 0}</small></button>`).join('')}</div>` : '');
  },
  async search() {
    const lives = await safe(popular(), []);
    let recent = [];
    try { recent = JSON.parse(localStorage.getItem('cview.recentSearches') || '[]'); } catch { /* 없음 */ }
    return header('검색 인사이트', '무엇을 찾을지 고민될 때')
      + section('flame', '지금 많이 보는 채널', liveRows(lives.slice(0, 5), (l) => `${l.categoryName || '기타'} · ${l.viewerCountText}명`))
      + section('grid', '많이 보는 카테고리', `<div class="ip-chips">${categoryGroups(lives).slice(0, 8).map((c) => `<button class="ip-chip" data-ip-category="${esc(c.name)}">${esc(c.name)}<small>${compactCount(c.viewers)}</small></button>`).join('')}</div>`)
      + section('clock', '최근 검색어', recent.length ? `<div class="ip-chips">${recent.slice(0, 8).map((q) => `<button class="ip-chip" data-ip-search="${esc(q)}">${esc(q)}</button>`).join('')}</div>` : '');
  },
  async multichat({ isLoggedIn }) {
    const chips = [...document.querySelectorAll('[data-page=multichat] .mc-chip')];
    const rows2 = chips.map((c) => ({ name: (c.querySelector('.mc-chip-name') || c).textContent.trim(), rate: parseInt((c.querySelector('.mc-rate') || {}).textContent || '0', 10) || 0 }));
    const lives = await safe(following(isLoggedIn), null);
    const open = new Set(chips.map((c) => c.querySelector('[data-mc-remove]')?.dataset.mcRemove));
    const suggest = (lives || []).filter((l) => !open.has(l.channelId)).sort((a, b) => b.viewerCount - a.viewerCount).slice(0, 4);
    const total = rows2.reduce((a, r) => a + r.rate, 0);
    return header('멀티채팅 인사이트', rows2.length ? `채널 ${rows2.length}개 연결` : '채널을 추가하면 채팅 흐름이 보여요')
      + tiles([[String(rows2.length), '연결 채널'], [`${total}`, '분당 메시지']])
      + section('chat', '채팅 속도', bars(rows2.filter((r) => r.rate > 0).map((r) => ({ name: r.name, value: r.rate })), (v) => `${v}/분`))
      + section('spark', '함께 보면 좋은 팔로잉', liveRows(suggest, (l) => `${l.categoryName || '기타'} · ${l.viewerCountText}명`));
  },
  async library() {
    const [cont, later, marks, dl, recent] = await Promise.all([
      safe(api.vodContinueList(50), []), safe(api.vodWatchLaterList(), []), safe(api.bookmarksList(), []),
      safe(api.downloadList(), { jobs: [], history: [] }), safe(api.recentChannels(5), [])]);
    const done = dl.history.filter((h) => h.completed);
    return header('보관함 인사이트', '저장해 둔 것 한눈에')
      + tiles([[String(cont.length), '이어 보기'], [String(later.length), '나중에 보기'], [String(marks.length), '북마크'], [String(done.length), '받은 파일']])
      + section('box', '받은 파일 용량', done.length ? rows([['파일', `${done.length}개`], ['합계', bytesText(done.reduce((a, h) => a + (h.fileSize || 0), 0))]]) : '')
      + section('clock', '최근 본 채널', recent.length ? liveRows(recent.map((c) => ({ ...c, viewerCountText: '' })), (c) => (c.lastWatched ? new Date(c.lastWatched).toLocaleDateString() : '')) : '');
  },
  async stats() {
    const o = await safe(overview(), null);
    if (!o || !o.sessionCount) return header('기록 요약', '아직 시청 기록이 없어요') + empty('라이브를 보면 기록이 쌓여요');
    const peak = o.hourly.indexOf(Math.max(...o.hourly));
    return header('기록 요약', `${o.countedSessionCount}회 · 채널 ${o.uniqueChannelCount}개`)
      + tiles([[durationText(o.last7), '최근 7일'], [`${o.streak}일`, '연속 시청'], [durationText(o.average), '회당 평균'], [o.hourly[peak] > 0 ? `${peak}시` : '—', '주로 보는 시간']])
      + section('clock', '최근 7일', columns(o.daily.slice(-7).map((d) => d.seconds), o.daily.slice(-7).map((d) => ['일', '월', '화', '수', '목', '금', '토'][new Date(d.date).getDay()])))
      + section('heart', '가장 많이 본 채널', bars(o.channels.filter((c) => c.seconds >= 60).slice(0, 4).map((c) => ({ name: c.name, value: c.seconds })), durationText));
  },
  async settings({ isLoggedIn }) {
    const [info, health, auth] = await Promise.all([safe(api.appInfo(), null), safe(api.dataHealth(), null), safe(api.authStatus(), null)]);
    return header('앱 상태', info ? `CView ${info.version}${info.build && info.build !== '0' ? ` (${info.build})` : ''}` : 'CView')
      + tiles([[info ? info.version : '—', '버전'], [auth && auth.loggedIn ? '로그인' : '로그아웃', '계정'], [health ? bytesText(health.storeBytes) : '—', '저장소'], [health ? `${health.backups.length}개` : '—', '백업']])
      + section('gear', '환경', info ? rows([['운영체제', `${info.os} · ${info.arch}`], ['계정', auth && auth.loggedIn ? (auth.nickname || '로그인됨') : (isLoggedIn() ? '로그인됨' : '로그인 안 됨')], ['남은 디스크', health && health.freeBytes != null ? bytesText(health.freeBytes) : '—']]) : '')
      + section('box', '저장된 항목', health ? bars(health.tables.filter((t) => t.rows > 0).slice(0, 5).map((t) => ({ name: t.label, value: t.rows })), (v) => `${v}`) : '');
  },
};

// ── 패널 ─────────────────────────────
export class InsightPanel {
  constructor(content, { isLoggedIn }) {
    this.isLoggedIn = isLoggedIn;
    this.content = content;
    this.route = null;
    this.seq = 0;
    this.el = document.createElement('aside');
    this.el.className = 'insight-panel';
    this.el.setAttribute('aria-label', '인사이트');
    this.el.innerHTML = `<button class="ip-toggle" title="인사이트 접기" aria-label="인사이트 접기">${ICON.chevron}</button><div class="ip-scroll"></div>`;
    this.reopen = document.createElement('button');
    this.reopen.className = 'ip-reopen';
    this.reopen.title = '인사이트 열기';
    this.reopen.setAttribute('aria-label', '인사이트 열기');
    this.reopen.innerHTML = `${ICON.insight}<span>인사이트</span>`;
    content.append(this.el, this.reopen);
    this.body = this.el.querySelector('.ip-scroll');
    this.el.querySelector('.ip-toggle').addEventListener('click', () => this.setCollapsed(true));
    this.reopen.addEventListener('click', () => this.setCollapsed(false));
    this.el.addEventListener('click', (e) => this.onClick(e));
    let collapsed = false;
    try { collapsed = localStorage.getItem(COLLAPSE_KEY) === '1'; } catch { /* 기본 펼침 */ }
    this.collapsed = collapsed;
    this.apply();
    setInterval(() => { if (this.visible && !this.collapsed && !document.hidden) this.load(); }, REFRESH_MS);
  }

  get visible() {
    return INSIGHT_ROUTES.has(this.route);
  }

  setCollapsed(on) {
    this.collapsed = on;
    try { localStorage.setItem(COLLAPSE_KEY, on ? '1' : '0'); } catch { /* 무시 */ }
    this.apply();
    if (!on) this.load();
  }

  apply() {
    const show = this.visible;
    this.content.classList.toggle('with-insights', show && !this.collapsed);
    this.content.classList.toggle('insights-collapsed', show && this.collapsed);
    this.el.hidden = !show || this.collapsed;
    this.reopen.hidden = !show || !this.collapsed;
  }

  show(route) {
    const changed = route !== this.route;
    this.route = route;
    this.apply();
    if (this.visible && !this.collapsed && changed) {
      this.body.innerHTML = '<div class="ip-skeleton"><span></span><span></span><span></span><span></span><i></i><i></i><i></i></div>';
      this.load();
    }
  }

  async load() {
    const provider = PROVIDERS[this.route];
    if (!provider) return;
    const seq = ++this.seq;
    try {
      const html = await provider({ isLoggedIn: this.isLoggedIn });
      if (seq === this.seq) this.body.innerHTML = html || empty('보여 줄 인사이트가 없어요');
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = empty(`인사이트를 불러오지 못했어요 — ${e.message || e}`);
    }
  }

  onClick(e) {
    const play = e.target.closest('[data-ip-play]');
    if (play) { window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: play.dataset.ipPlay } })); return; }
    const ch = e.target.closest('[data-ip-channel]');
    if (ch) { window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: ch.dataset.ipChannel } })); return; }
    const cat = e.target.closest('[data-ip-category]');
    if (cat) { window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'categories', params: { category: cat.dataset.ipCategory } } })); return; }
    const q = e.target.closest('[data-ip-search]');
    if (q) window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'search', params: { query: q.dataset.ipSearch } } }));
  }
}
