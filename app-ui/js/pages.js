// 사이드바 화면들 — 팔로잉 / 인기 / 검색 / 설정. 홈은 home.js.
import { api } from './api.js';
import { esc, safeUrl, toast } from './dom.js';
import { liveGrid, channelRow, emptyState, loadingState, compactCount } from './cards.js';
import { vodCard } from './channel.js';
import { THEMES, THEME_LABELS, currentTheme, setTheme } from './theme.js';
import { streamAlertsEnabled, setStreamAlertsEnabled } from './donations.js';
import { chatPrefs, updateChatPrefs, blockedLabel, loadChatPrefs } from './chat-prefs.js';

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
    if (!this.lives.length || refresh) this.body.innerHTML = loadingState();
    try {
      const lives = await api.popularLives(this.limit, refresh);
      if (seq !== this.seq) return;
      this.lives = lives;
      this.render();
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = emptyState('불러오지 못했습니다', e.message);
    }
  }

  render() {
    const cats = categoryGroups(this.lives).slice(0, 14);
    const list = arrangeLives(this.lives, { sort: this.sort, category: this.category });
    this.body.innerHTML = `
      <div class="list-toolbar">
        <div class="segmented"><button class="seg${this.sort === 'viewers' ? ' active' : ''}" data-sort="viewers">시청자순</button><button class="seg${this.sort === 'recent' ? ' active' : ''}" data-sort="recent">최근 시작</button></div>
        <div class="chips">
          <button class="chip${this.category === '' ? ' active' : ''}" data-cat="">전체</button>
          ${cats.map((c) => `<button class="chip${this.category === c.name ? ' active' : ''}" data-cat="${esc(c.name)}">${esc(c.name)} <span class="chip-count">${c.count}</span></button>`).join('')}
        </div>
      </div>
      ${list.length ? liveGrid(list) : emptyState('라이브 채널이 없습니다')}
      ${this.lives.length >= this.limit && this.limit < POPULAR_MAX ? '<button class="btn load-more" data-more>더 보기</button>' : ''}`;
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
      const ch = e.target.closest('[data-open-channel-id]');
      if (ch) window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: ch.dataset.openChannelId } }));
    });
  }

  async show(refresh = false) {
    if (!this.isLoggedIn()) {
      this.body.innerHTML = emptyState('네이버 로그인이 필요합니다', '로그인하면 팔로우한 채널의 라이브를 볼 수 있습니다', loginAction);
      return;
    }
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    try {
      if (this.tab === 'live') this.lives = await api.followingLives(refresh);
      else this.channels = await api.followingChannelsAll(refresh);
      if (seq === this.seq) this.render();
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = emptyState('불러오지 못했습니다', e.message);
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
      this.body.innerHTML = `
        <div class="list-toolbar">${tabs}${sortSeg}<button class="btn ghost" data-dense>${this.dense ? '격자 보기' : '간결 보기'}</button>
          <div class="chips"><button class="chip${this.category === '' ? ' active' : ''}" data-cat="">전체 ${this.lives.length}</button>${cats.map((c) => `<button class="chip${this.category === c.name ? ' active' : ''}" data-cat="${esc(c.name)}">${esc(c.name)} <span class="chip-count">${c.count}</span></button>`).join('')}</div>
        </div>${body}`;
      return;
    }
    const sorted = [...this.channels].sort((a, b) => {
      if (this.sort === 'name') return a.channelName.localeCompare(b.channelName, 'ko');
      return (b.isLive - a.isLive) || ((b.viewerCount || 0) - (a.viewerCount || 0)) || a.channelName.localeCompare(b.channelName, 'ko');
    });
    const sortSeg = `<div class="segmented"><button class="seg${this.sort !== 'name' ? ' active' : ''}" data-sort="viewers">방송 중 먼저</button><button class="seg${this.sort === 'name' ? ' active' : ''}" data-sort="name">이름순</button></div>`;
    this.body.innerHTML = `
      <div class="list-toolbar">${tabs}${sortSeg}<span class="page-subtitle">팔로우 ${this.channels.length}개 · 방송 중 ${this.channels.filter((c) => c.isLive).length}개</span></div>
      ${sorted.length ? `<div class="dense-list">${sorted.map(followedRow).join('')}</div>` : emptyState('팔로우한 채널이 없습니다')}`;
  }
}

function denseRow(l) {
  const avatar = safeUrl(l.channelImageUrl);
  return `<div class="dense-row card" tabindex="0" data-channel-id="${esc(l.channelId)}" data-channel-name="${esc(l.channelName)}" data-channel-image="${esc(avatar)}">
    <span class="card-avatar" data-open-channel>${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}</span>
    <span class="dense-name ellipsis"><b>${esc(l.channelName)}</b> <span>${esc(l.categoryName || '')}</span></span>
    <span class="dense-title ellipsis">${esc(l.liveTitle || '')}</span>
    <span class="dense-viewers"><i class="dot"></i>${esc(l.viewerCountText)}</span>
  </div>`;
}

function followedRow(c) {
  const avatar = safeUrl(c.channelImageUrl);
  return `<div class="dense-row${c.isLive ? ' card' : ''}" tabindex="0" ${c.isLive ? `data-channel-id="${esc(c.channelId)}"` : `data-open-channel-id="${esc(c.channelId)}"`}>
    <span class="card-avatar" ${c.isLive ? 'data-open-channel' : ''}>${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}</span>
    <span class="dense-name ellipsis"><b>${esc(c.channelName)}</b> <span>${c.isLive ? esc(c.categoryName || '방송 중') : '오프라인'}</span></span>
    <span class="dense-title ellipsis">${esc(c.liveTitle || '')}</span>
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

export class SearchPage {
  constructor(root) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.input = root.querySelector('input[type=search]');
    this.input.placeholder = '채널, 라이브, 다시보기 검색…';
    this.acceptsParams = true;
    this.seq = 0;
    this.tab = 'all';
    this.result = null;
    this.videos = null;
    root.querySelector('form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.run(this.input.value);
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
      if (e.target.closest('[data-recent-clear]')) { try { localStorage.removeItem(RECENT_SEARCH_KEY); } catch { /* 무시 */ } this.renderIdle(); return; }
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

  renderIdle() {
    const recent = recentSearches();
    this.body.innerHTML = recent.length
      ? `<div class="section-head"><h2 class="section-title">최근 검색어</h2><div class="grow"></div><button class="link-btn" data-recent-clear>모두 지우기</button></div>
         <div class="chips">${recent.map((q) => `<span class="chip recent-chip"><button class="link-btn" data-recent="${esc(q)}">${esc(q)}</button><button class="chip-x" data-recent-remove="${esc(q)}" title="지우기">✕</button></span>`).join('')}</div>`
      : emptyState('검색어를 입력하세요', '채널 이름, 방송 제목, 다시보기로 찾을 수 있습니다');
  }

  async run(keyword) {
    const q = keyword.trim();
    if (!q) { this.renderIdle(); return; }
    saveRecentSearch(q);
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    const [main, vods] = await Promise.allSettled([api.search(q), api.searchVideos(q, 0, 24)]);
    if (seq !== this.seq) return;
    if (main.status === 'rejected' && vods.status === 'rejected') {
      this.body.innerHTML = emptyState('검색하지 못했습니다', main.reason.message);
      return;
    }
    this.query = q;
    this.result = main.status === 'fulfilled' ? main.value : { lives: [], channels: [] };
    this.videos = vods.status === 'fulfilled' ? vods.value.items : [];
    this.renderResults();
  }

  renderResults() {
    if (!this.result) return;
    const { lives, channels } = this.result;
    const videos = this.videos || [];
    const tabs = [['all', `전체`], ['lives', `라이브 ${lives.length}`], ['channels', `채널 ${channels.length}`], ['videos', `다시보기 ${videos.length}`]];
    const tabBar = `<div class="segmented search-tabs">${tabs.map(([id, l]) => `<button class="seg${this.tab === id ? ' active' : ''}" data-stab="${id}">${l}</button>`).join('')}</div>`;
    const sec = (id, title, items, html, limit) => {
      if (!items.length) return '';
      const full = this.tab === id;
      if (this.tab !== 'all' && !full) return '';
      return `<div class="section-head"><h2 class="section-title">${title}</h2><div class="grow"></div>${!full && items.length > limit ? `<button class="link-btn" data-stab="${id}">전체 보기</button>` : ''}</div>${html(full ? items : items.slice(0, limit))}`;
    };
    const html = [
      sec('lives', '라이브', lives, (l) => liveGrid(l), 8),
      sec('channels', '채널', channels, (c) => `<div class="channel-list">${c.map(channelRow).join('')}</div>`, 6),
      sec('videos', '다시보기', videos, (v) => `<div class="grid">${v.map(vodCard).join('')}</div>`, 8),
    ].join('');
    this.body.innerHTML = tabBar + (html || emptyState('검색 결과가 없습니다', `"${esc(this.query)}"`));
  }
}

const CHAT_TOGGLES = [
  ['showTimestamp', '시간 표시', '메시지 옆에 받은 시각을 보여 줍니다'],
  ['showBadge', '배지 표시', '구독·팬 배지를 보여 줍니다'],
  ['emoticonEnabled', '이모티콘 표시', '끄면 이모티콘을 :이름: 글자로 보여 줍니다'],
  ['highlightRoles', '역할 색', '스트리머·매니저 닉네임에 색을 입힙니다'],
  ['highlightMentions', '멘션 강조', '나를 부른 메시지를 강조하고 모아 둡니다'],
  ['autoScroll', '자동 스크롤', '맨 아래를 보고 있을 때 새 메시지를 따라갑니다 (⌘J)'],
  ['showDonation', '후원·구독 표시', '끄면 채팅에서 후원·구독 카드를 숨깁니다'],
  ['showDonationsOnly', '후원만 보기', '후원·구독 메시지만 보여 줍니다'],
];

function chatSettingsHtml() {
  const p = chatPrefs();
  const toggle = ([key, label, help]) => `
    <label class="setting"><span class="setting-label">${esc(label)}<small>${esc(help)}</small></span>
      <span class="toggle"><input type="checkbox" data-chat="${key}" ${p[key] ? 'checked' : ''}><span></span></span></label>`;
  const range = (key, label, min, max, step) => `
    <label class="setting"><span class="setting-label">${esc(label)}</span>
      <span class="range-field"><input type="range" min="${min}" max="${max}" step="${step}" value="${esc(p[key])}" data-chat="${key}"><output>${esc(p[key])}</output></span></label>`;
  const words = (p.blockedWords || []).map((w) => `<span class="word-chip">${esc(w)}<button data-remove-word="${esc(w)}" title="지우기">✕</button></span>`).join('');
  const users = (p.blockedUsers || []).map((u) => `<span class="word-chip">${esc(blockedLabel(u))}<button data-unblock="${esc(u)}" title="차단 해제">해제</button></span>`).join('');
  return `
    <section class="settings-group"><h3>채팅</h3>
      ${range('fontSize', '글자 크기', 11, 20, 1)}
      ${range('lineSpacing', '줄 간격', 0, 8, 1)}
      ${CHAT_TOGGLES.map(toggle).join('')}
      <label class="setting"><span class="setting-label">최대 표시 줄 수<small>오래된 메시지부터 지웁니다</small></span>
        <select class="select" data-chat="maxVisibleMessages">${[150, 300, 500, 1000].map((n) => `<option value="${n}" ${Number(p.maxVisibleMessages) === n ? 'selected' : ''}>${n}줄</option>`).join('')}</select></label>
      ${toggle(['chatFilterEnabled', '금지어 필터', '아래 단어가 들어간 메시지를 숨깁니다'])}
      <div class="setting column"><div class="word-list">${words || '<span class="muted">금지어가 없습니다</span>'}</div>
        <form class="word-form" data-word-form><input type="text" maxlength="40" placeholder="숨길 단어 추가"><button class="btn">추가</button></form></div>
      <div class="setting column"><span class="setting-label">차단한 사용자<small>채팅의 닉네임을 오른쪽 클릭해 차단할 수 있습니다</small></span>
        <div class="word-list">${users || '<span class="muted">차단한 사용자가 없습니다</span>'}</div></div>
    </section>`;
}

export class SettingsPage {
  constructor(root, { getAuth, onLogin, onLogout }) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.getAuth = getAuth;
    this.onLogin = onLogin;
    this.onLogout = onLogout;
    this.settings = null;
    this.body.addEventListener('change', (e) => this.onChange(e.target));
    this.body.addEventListener('click', (e) => {
      const action = e.target.closest('[data-action]');
      if (!action) return;
      if (action.dataset.action === 'login') this.onLogin();
      if (action.dataset.action === 'logout') this.onLogout();
    });
    this.body.addEventListener('submit', (e) => {
      const form = e.target.closest('[data-word-form]');
      if (!form) return;
      e.preventDefault();
      const input = form.querySelector('input');
      const word = input.value.trim();
      if (!word) return;
      const words = [...new Set([...(chatPrefs().blockedWords || []), word])];
      updateChatPrefs({ blockedWords: words }).then(() => this.render());
    });
    this.body.addEventListener('click', (e) => {
      const w = e.target.closest('[data-remove-word]');
      if (w) {
        updateChatPrefs({ blockedWords: (chatPrefs().blockedWords || []).filter((x) => x !== w.dataset.removeWord) }).then(() => this.render());
        return;
      }
      const u = e.target.closest('[data-unblock]');
      if (u) {
        const p = chatPrefs();
        const names = { ...(p.blockedUserNames || {}) };
        delete names[u.dataset.unblock];
        updateChatPrefs({ blockedUsers: (p.blockedUsers || []).filter((x) => x !== u.dataset.unblock), blockedUserNames: names }).then(() => this.render());
      }
    });
    this.body.addEventListener('click', async (e) => {
      const choice = e.target.closest('[data-theme-choice]');
      if (!choice) return;
      await setTheme(choice.dataset.themeChoice);
      this.render();
    });
  }

  async show() {
    await loadChatPrefs();
    try {
      this.settings = await api.settingsGet();
    } catch (e) {
      this.body.innerHTML = emptyState('설정을 불러오지 못했습니다', e.message);
      return;
    }
    this.render();
  }

  render() {
    if (!this.settings) return;
    const auth = this.getAuth();
    const player = this.settings.player || {};
    const general = this.settings.general || {};
    const account = auth && auth.loggedIn
      ? `<div class="setting"><span class="setting-label">${esc(auth.nickname || '로그인됨')}<small>네이버 계정 (${esc(auth.method || 'cookie')})</small></span><button class="btn" data-action="logout">로그아웃</button></div>`
      : '<div class="setting"><span class="setting-label">로그인되지 않음<small>팔로잉 목록과 채팅 참여에 네이버 로그인이 필요합니다</small></span><button class="btn primary" data-action="login">로그인</button></div>';
    this.body.innerHTML = `
      <section class="settings-group"><h3>계정</h3>${account}</section>
      <section class="settings-group"><h3>화면</h3>
        <div class="setting">
          <span class="setting-label">테마<small>시스템은 macOS 화면 모드를 따릅니다</small></span>
          <div class="segmented" role="radiogroup" aria-label="테마">
            ${THEMES.map((t) => `<button class="seg ${t === currentTheme() ? 'active' : ''}" data-theme-choice="${t}" role="radio" aria-checked="${t === currentTheme()}">${THEME_LABELS[t]}</button>`).join('')}
          </div>
        </div>
        <label class="setting">
          <span class="setting-label">후원·구독 알림<small>시청 화면 위에 새 후원과 구독을 잠깐 띄웁니다</small></span>
          <span class="toggle"><input type="checkbox" data-local="streamAlerts" ${streamAlertsEnabled() ? 'checked' : ''}><span></span></span>
        </label>
      </section>
      ${chatSettingsHtml()}
      <section class="settings-group"><h3>플레이어</h3>
        ${PLAYER_TOGGLES.map((t) => toggleRow('player', t, player)).join('')}
        <label class="setting">
          <span class="setting-label">스트림 프록시<small>로컬 프록시는 CDN 응답을 보정하고 암호화 방송 키를 인증합니다</small></span>
          <select class="select" data-domain="player" data-key="streamProxyMode">
            <option value="localProxy" ${player.streamProxyMode !== 'none' ? 'selected' : ''}>로컬 프록시 (권장)</option>
            <option value="none" ${player.streamProxyMode === 'none' ? 'selected' : ''}>직접 재생</option>
          </select>
        </label>
      </section>
      <section class="settings-group"><h3>일반</h3>
        ${GENERAL_TOGGLES.map((t) => toggleRow('general', t, general)).join('')}
      </section>`;
  }

  async onChange(target) {
    if (target.dataset.chat) {
      const key = target.dataset.chat;
      const value = target.type === 'checkbox' ? target.checked : Number(target.value);
      await updateChatPrefs({ [key]: value });
      if (target.type === 'range') {
        const out = target.parentElement.querySelector('output');
        if (out) out.textContent = target.value;
      }
      return;
    }
    if (target.dataset.local === 'streamAlerts') {
      setStreamAlertsEnabled(target.checked);
      return;
    }
    const { domain, key } = target.dataset;
    if (!domain || !key) return;
    const value = target.type === 'checkbox' ? target.checked : target.value;
    try {
      this.settings = await api.settingsUpdate(domain, { [key]: value });
      toast('설정을 저장했습니다', 1400);
    } catch (e) {
      toast(e.message);
      this.render();
    }
  }
}
