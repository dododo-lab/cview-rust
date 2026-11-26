// macOS 메뉴 막대 드롭다운 팝오버 UI(`menubar.html`) — 지금 시청 중 미니 컨트롤러, 빠른 동작·토글 타일,
// 즐겨찾기·팔로잉 / 인기 라이브 탐색·검색·멀티라이브 추가·독립 채팅 창 열기.
import { api, listen, listenHere, Events } from './api.js';
import { esc, safeUrl, imageSrc, icons, keepFocus } from './dom.js';
import { compactCount } from './cards.js';
import { setTheme, syncTheme, resolvedTheme, onThemeChange } from './theme.js';
import { installUiHealth } from './ui-health.js';

const ICON = {
  play: '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15l13-7.5-13-7.5Z"/></svg>',
  pause: '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
  windowShow: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2.4"/><path d="M3 9h18"/></svg>',
  windowHide: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-6.5 0-10-8-10-8a18.45 18.45 0 0 1 5.06-5.94"/><path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c6.5 0 10 8 10 8a18.5 18.5 0 0 1-2.16 3.19"/><path d="m2 2 20 20"/></svg>',
  moon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z"/></svg>',
  sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4.2"/><path d="M12 2v2.2M12 19.8V22M4.93 4.93l1.56 1.56M17.51 17.51l1.56 1.56M2 12h2.2M19.8 12H22M4.93 19.07l1.56-1.56M17.51 6.49l1.56-1.56"/></svg>',
  plus: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>',
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M21 12a8 8 0 0 1-8 8H7l-4 3v-5.5A8 8 0 1 1 21 12Z"/></svg>',
};

const REFRESH_TTL_MS = 30000;

const state = {
  open: true,
  loggedIn: false,
  windowHidden: false,
  arrowX: 192,
  front: { nowPlaying: null, muted: null, paused: null, volume: null, pip: null },
  general: { notificationsEnabled: true },
  tab: 'following',
  query: '',
  following: [],
  popular: [],
  favorites: new Set(),
  multiIds: new Set(),
  loading: false,
  lastLoadedAt: 0,
  draggingVolume: false,
};

const $ = (id) => document.getElementById(id);

/** 검색어 필터 + 즐겨찾기 우선 정렬(팔로잉 탭). 순수 함수에 가깝다. */
export function filterLives(items, { query = '', favorites = new Set(), prioritizeFavorites = false } = {}) {
  const q = String(query || '').trim().toLowerCase();
  const matched = (items || []).filter((it) => {
    if (!it) return false;
    if (!q) return true;
    const hay = `${it.channelName || ''} ${it.liveTitle || ''} ${it.categoryName || ''}`.toLowerCase();
    return hay.includes(q);
  });
  if (!prioritizeFavorites || !favorites.size) return matched;
  const fav = [];
  const rest = [];
  for (const it of matched) {
    if (favorites.has(it.channelId)) fav.push(it);
    else rest.push(it);
  }
  return [...fav, ...rest];
}

function renderArrow() {
  const stage = $('mb-stage');
  if (stage && Number.isFinite(state.arrowX)) {
    stage.style.setProperty('--arrow-x', `${state.arrowX}px`);
  }
}

function renderHeader() {
  const pill = $('mb-live-pill');
  const countEl = $('mb-live-count');
  const n = state.following.length;
  if (state.loggedIn && n > 0) {
    pill.hidden = false;
    countEl.textContent = `팔로잉 방송 중 ${n}`;
  } else {
    pill.hidden = true;
  }
  const tabCount = $('mb-tab-following-count');
  if (state.loggedIn && n > 0) {
    tabCount.hidden = false;
    tabCount.textContent = String(n);
  } else {
    tabCount.hidden = true;
  }
}

function renderNowPlaying() {
  const box = $('mb-now');
  const now = state.front && state.front.nowPlaying;
  if (!now) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const isVod = now.kind === 'vod';
  const kindEl = $('mb-now-kind');
  kindEl.textContent = isVod ? '다시보기' : 'LIVE';
  kindEl.classList.toggle('is-vod', isVod);

  $('mb-now-channel').textContent = now.channelName || 'CView';
  $('mb-now-title').textContent = now.title || now.category || (isVod ? '다시보기 재생 중' : '라이브 시청 중');

  const thumbBox = $('mb-now-thumb');
  const rawThumb = safeUrl(now.thumbnailUrl) || safeUrl(now.imageUrl);
  const wantSrc = rawThumb ? imageSrc(rawThumb, safeUrl(now.thumbnailUrl) ? 'live' : 'avatar') : '';
  if (thumbBox.dataset.src !== wantSrc) {
    thumbBox.dataset.src = wantSrc;
    if (wantSrc) {
      thumbBox.innerHTML = `<img src="${esc(wantSrc)}" alt="" referrerpolicy="no-referrer">`;
    } else {
      const ch = (Array.from(String(now.channelName || '').trim())[0] || 'C').toUpperCase();
      thumbBox.innerHTML = `<span class="mb-now-fallback" aria-hidden="true">${esc(ch)}</span>`;
    }
  }

  const paused = Boolean(state.front.paused);
  const playBtn = $('mb-play');
  playBtn.innerHTML = paused ? ICON.play : ICON.pause;
  playBtn.title = paused ? '재생' : '일시정지';
  playBtn.setAttribute('aria-label', paused ? '재생' : '일시정지');

  const muted = Boolean(state.front.muted);
  const muteBtn = $('mb-mute');
  muteBtn.innerHTML = muted ? icons.muted : icons.volume;
  muteBtn.title = muted ? '음소거 해제' : '음소거';
  muteBtn.setAttribute('aria-label', muted ? '음소거 해제' : '음소거');
  muteBtn.disabled = state.front.muted == null;

  const vol = typeof state.front.volume === 'number' ? state.front.volume : (muted ? 0 : 100);
  const range = $('mb-volume');
  range.disabled = state.front.volume == null && state.front.muted == null;
  if (!state.draggingVolume) range.value = String(vol);
  $('mb-vol-val').textContent = `${state.draggingVolume ? range.value : vol}%`;

  const pipBtn = $('mb-pip');
  const pipOn = Boolean(state.front.pip);
  pipBtn.hidden = state.front.pip == null;
  pipBtn.setAttribute('aria-pressed', pipOn ? 'true' : 'false');
  pipBtn.title = pipOn ? '화면 속 화면 끄기' : '화면 속 화면 (PiP)';
}

function renderTiles() {
  const winTile = $('mb-tile-window');
  const hidden = Boolean(state.windowHidden);
  winTile.classList.toggle('is-on', !hidden);
  $('mb-tile-window-icon').innerHTML = hidden ? ICON.windowShow : ICON.windowHide;
  $('mb-tile-window-label').textContent = hidden ? 'CView 열기' : '창 숨기기';
  $('mb-tile-window-sub').textContent = hidden ? '메뉴 막대에 숨김' : '창 표시 중';

  const pool = state.loggedIn && state.following.length ? state.following : state.popular;
  const topCount = Math.min(4, pool.length);
  $('mb-tile-queue').disabled = topCount === 0;
  $('mb-tile-queue-sub').textContent = topCount > 0 ? `${state.loggedIn && state.following.length ? '팔로잉' : '인기'} ${topCount}채널` : '방송 없음';

  const notifyOn = state.general.notificationsEnabled !== false;
  const notifyTile = $('mb-tile-notify');
  notifyTile.setAttribute('aria-pressed', notifyOn ? 'true' : 'false');
  $('mb-tile-notify-sub').textContent = notifyOn ? '켜짐' : '꺼짐';

  const dark = resolvedTheme() === 'dark';
  const themeTile = $('mb-tile-theme');
  themeTile.setAttribute('aria-pressed', dark ? 'true' : 'false');
  $('mb-tile-theme-icon').innerHTML = dark ? ICON.moon : ICON.sun;
  $('mb-tile-theme-sub').textContent = dark ? '다크 모드' : '라이트 모드';

  const mlCount = state.multiIds.size;
  $('mb-tile-ml-sub').textContent = mlCount > 0 ? `${mlCount}패인 실행 중` : '무대 열기';
}

function renderTabs() {
  const isFol = state.tab === 'following';
  const tFol = $('mb-tab-following');
  const tPop = $('mb-tab-popular');
  tFol.classList.toggle('is-active', isFol);
  tFol.setAttribute('aria-selected', isFol ? 'true' : 'false');
  tFol.tabIndex = isFol ? 0 : -1;
  tPop.classList.toggle('is-active', !isFol);
  tPop.setAttribute('aria-selected', !isFol ? 'true' : 'false');
  tPop.tabIndex = isFol ? -1 : 0;
  $('mb-list').setAttribute('aria-labelledby', isFol ? 'mb-tab-following' : 'mb-tab-popular');
}

function itemHtml(item, { favorite = false, watching = false, inMulti = false } = {}) {
  const id = item.channelId || '';
  const name = item.channelName || '채널';
  const avatarUrl = safeUrl(item.channelImageUrl);
  const avatar = avatarUrl
    ? `<img class="mb-avatar" src="${esc(imageSrc(avatarUrl, 'avatar'))}" alt="" loading="lazy" referrerpolicy="no-referrer">`
    : `<span class="mb-avatar-initial" aria-hidden="true">${esc(Array.from(name.trim())[0] || 'C')}</span>`;
  const viewers = item.viewerCountText || compactCount(item.viewerCount || 0);
  const cat = (item.categoryName || '').trim();
  const title = (item.liveTitle || '').trim() || '라이브 방송 중';
  return `
    <div class="mb-item${watching ? ' is-watching' : ''}" data-channel-id="${esc(id)}">
      <button type="button" class="mb-item-main" data-item-act="watch" data-channel-id="${esc(id)}" title="${esc(`${name} — ${title}`)}">
        <span class="mb-avatar-wrap">${avatar}</span>
        <span class="mb-item-body">
          <span class="mb-item-row1">
            ${favorite ? '<span class="mb-fav" title="즐겨찾기" aria-label="즐겨찾기">★</span>' : ''}
            <span class="mb-item-name">${esc(name)}</span>
            <span class="mb-item-viewers">${esc(viewers)}</span>
          </span>
          <span class="mb-item-row2">
            ${cat ? `<span class="mb-item-cat">${esc(cat)}</span>` : ''}
            <span class="mb-item-title">${esc(title)}</span>
          </span>
        </span>
      </button>
      <div class="mb-item-actions">
        <button type="button" class="mb-act-btn${inMulti ? ' is-in-multi' : ''}" data-item-act="multi" data-channel-id="${esc(id)}" title="${inMulti ? '멀티라이브에서 보기' : '멀티라이브에 추가'}" aria-label="${esc(`${name} ${inMulti ? '멀티라이브에서 보기' : '멀티라이브에 추가'}`)}">
          ${inMulti ? ICON.check : ICON.plus}
        </button>
        <button type="button" class="mb-act-btn" data-item-act="chat" data-channel-id="${esc(id)}" title="독립 채팅 창 열기" aria-label="${esc(`${name} 채팅 창 열기`)}">
          ${ICON.chat}
        </button>
      </div>
    </div>`;
}

function renderList() {
  const listEl = $('mb-list');
  keepFocus(listEl, () => {
    if (state.tab === 'following' && !state.loggedIn) {
      listEl.innerHTML = `
        <div class="mb-empty">
          <strong>로그인이 필요합니다</strong>
          <span>네이버 계정으로 로그인하면 방송 중인 팔로잉과 즐겨찾기 채널을 바로 볼 수 있어요.</span>
          <div class="mb-empty-actions">
            <button type="button" class="mb-cta-btn primary" data-act="login">네이버 로그인</button>
            <button type="button" class="mb-cta-btn" data-act="switch-popular">인기 라이브 보기</button>
          </div>
        </div>`;
      return;
    }

    const source = state.tab === 'following' ? state.following : state.popular;
    if (state.loading && !source.length) {
      listEl.innerHTML = '<div class="mb-empty"><span>방송 목록을 불러오는 중…</span></div>';
      return;
    }

    const items = filterLives(source, {
      query: state.query,
      favorites: state.favorites,
      prioritizeFavorites: state.tab === 'following',
    });

    if (!items.length) {
      if (state.query.trim()) {
        listEl.innerHTML = '<div class="mb-empty"><strong>일치하는 방송이 없습니다</strong><span>다른 검색어로 찾아보세요.</span></div>';
      } else if (state.tab === 'following') {
        listEl.innerHTML = `
          <div class="mb-empty">
            <strong>방송 중인 팔로잉 채널이 없습니다</strong>
            <span>인기 라이브 탭에서 지금 진행 중인 방송을 둘러보세요.</span>
            <div class="mb-empty-actions">
              <button type="button" class="mb-cta-btn" data-act="switch-popular">인기 라이브 보기</button>
            </div>
          </div>`;
      } else {
        listEl.innerHTML = '<div class="mb-empty"><strong>방송 목록을 불러오지 못했습니다</strong><span>새로고침으로 다시 시도해 주세요.</span></div>';
      }
      return;
    }

    const watchingId = state.front?.nowPlaying?.channelId || '';
    if (state.tab === 'following' && !state.query.trim()) {
      const favs = items.filter((i) => state.favorites.has(i.channelId));
      const others = items.filter((i) => !state.favorites.has(i.channelId));
      const chunks = [];
      if (favs.length) {
        chunks.push(`<div class="mb-group-label">즐겨찾기 · ${favs.length}</div>`);
        for (const it of favs) {
          chunks.push(itemHtml(it, { favorite: true, watching: it.channelId === watchingId, inMulti: state.multiIds.has(it.channelId) }));
        }
      }
      if (others.length) {
        if (favs.length) chunks.push(`<div class="mb-group-label">팔로잉 · ${others.length}</div>`);
        for (const it of others) {
          chunks.push(itemHtml(it, { favorite: false, watching: it.channelId === watchingId, inMulti: state.multiIds.has(it.channelId) }));
        }
      }
      listEl.innerHTML = chunks.join('');
      return;
    }

    listEl.innerHTML = items
      .map((it) => itemHtml(it, {
        favorite: state.favorites.has(it.channelId),
        watching: it.channelId === watchingId,
        inMulti: state.multiIds.has(it.channelId),
      }))
      .join('');
  });
}

function renderAll() {
  renderArrow();
  renderHeader();
  renderNowPlaying();
  renderTiles();
  renderTabs();
  renderList();
}

let refreshSeq = 0;
let inFlightRefresh = null;

async function refreshData({ force = false } = {}) {
  if (!force && inFlightRefresh) {
    return inFlightRefresh;
  }
  if (!force && state.lastLoadedAt && Date.now() - state.lastLoadedAt < REFRESH_TTL_MS) {
    renderAll();
    return;
  }
  const seq = ++refreshSeq;
  state.loading = true;
  const refreshBtn = $('mb-refresh');
  if (force) refreshBtn.classList.add('is-spinning');
  renderList();
  const run = (async () => {
    try {
      const [authRes, snapRes, settingsRes, mlRes, popRes] = await Promise.allSettled([
        api.authStatus(true),
        api.trayGetState(),
        api.settingsGet(),
        api.multiLiveState(),
        api.popularLives(30, force),
      ]);
      if (seq !== refreshSeq) return;
      if (snapRes.status === 'fulfilled' && snapRes.value) {
        applySnapshot(snapRes.value, { render: false, trustAuth: authRes.status !== 'fulfilled' });
      }
      if (authRes.status === 'fulfilled' && authRes.value && typeof authRes.value.loggedIn === 'boolean') {
        state.loggedIn = authRes.value.loggedIn;
      }
      if (settingsRes.status === 'fulfilled' && settingsRes.value) {
        const s = settingsRes.value;
        if (s.general) state.general = s.general;
        if (s.appearance && s.appearance.theme) syncTheme(s.appearance.theme);
      }
      if (mlRes.status === 'fulfilled' && mlRes.value) {
        state.multiIds = new Set((mlRes.value.sessions || []).map((x) => x.channelId).filter(Boolean));
      }
      if (popRes.status === 'fulfilled' && Array.isArray(popRes.value)) {
        state.popular = popRes.value;
      }
      if (state.loggedIn) {
        const [folRes, favRes] = await Promise.allSettled([
          api.followingLives(force),
          api.favoritesList(),
        ]);
        if (seq !== refreshSeq) return;
        if (folRes.status === 'fulfilled' && Array.isArray(folRes.value)) {
          state.following = folRes.value;
        }
        if (favRes.status === 'fulfilled' && Array.isArray(favRes.value)) {
          state.favorites = new Set(favRes.value.map((f) => f.channelId));
        }
      } else {
        state.following = [];
        state.favorites = new Set();
      }
      state.lastLoadedAt = Date.now();
    } finally {
      if (seq === refreshSeq) {
        state.loading = false;
        refreshBtn.classList.remove('is-spinning');
        inFlightRefresh = null;
        renderAll();
      }
    }
  })();
  inFlightRefresh = run;
  return run;
}

/** 인증 이벤트 직후 새로고침 — 트레이 스냅샷이 아직 직전 주기 값이어도 방금 받은 로그인 상태를 지킨다. */
async function refreshDataForAuth(expectedLoggedIn) {
  await refreshData({ force: true });
  if (state.loggedIn !== expectedLoggedIn) {
    state.loggedIn = expectedLoggedIn;
    if (expectedLoggedIn) {
      const [folRes, favRes] = await Promise.allSettled([
        api.followingLives(true),
        api.favoritesList(),
      ]);
      if (folRes.status === 'fulfilled' && Array.isArray(folRes.value)) state.following = folRes.value;
      if (favRes.status === 'fulfilled' && Array.isArray(favRes.value)) {
        state.favorites = new Set(favRes.value.map((f) => f.channelId));
      }
    } else {
      state.following = [];
      state.favorites = new Set();
    }
    renderAll();
  }
}

function applySnapshot(snap, { render = true, trustAuth = true } = {}) {
  if (!snap) return;
  const wasLoggedIn = state.loggedIn;
  const becameOpen = !state.open && Boolean(snap.open);
  state.open = Boolean(snap.open);
  if (trustAuth) state.loggedIn = Boolean(snap.loggedIn);
  state.windowHidden = Boolean(snap.windowHidden);
  if (typeof snap.arrowX === 'number') state.arrowX = snap.arrowX;
  if (snap.front) state.front = snap.front;
  if (render) renderAll();
  if (becameOpen || (trustAuth && wasLoggedIn !== state.loggedIn)) {
    refreshData({ force: false });
  }
}

async function runAction(act) {
  switch (act) {
    case 'open-home':
      await api.trayControl({ kind: 'open-route', route: 'home', params: {} });
      break;
    case 'refresh':
      await refreshData({ force: true });
      break;
    case 'settings':
      await api.trayControl({ kind: 'open-route', route: 'settings', params: {} });
      break;
    case 'show-main':
      await api.trayControl({ kind: 'show-main' });
      break;
    case 'toggle-window':
      await api.trayControl({ kind: 'toggle-window' });
      break;
    case 'play-pause':
      state.front = { ...state.front, paused: !state.front.paused };
      renderNowPlaying();
      await api.trayControl({ kind: 'play-pause' });
      break;
    case 'mute':
      state.front = { ...state.front, muted: !state.front.muted };
      renderNowPlaying();
      await api.trayControl({ kind: 'mute' });
      break;
    case 'pip':
      state.front = { ...state.front, pip: !state.front.pip };
      renderNowPlaying();
      await api.trayControl({ kind: 'pip' });
      break;
    case 'queue-top4': {
      const pool = state.loggedIn && state.following.length ? state.following : state.popular;
      const ids = [...pool]
        .sort((a, b) => (b.viewerCount || 0) - (a.viewerCount || 0))
        .slice(0, 4)
        .map((l) => l.channelId)
        .filter(Boolean);
      if (ids.length) await api.trayControl({ kind: 'queue-multi', channelIds: ids });
      break;
    }
    case 'random-live': {
      const pool = state.loggedIn && state.following.length ? state.following : state.popular;
      if (!pool.length) {
        await refreshData({ force: true });
        return;
      }
      const pick = pool[Math.floor(Math.random() * pool.length)];
      if (pick?.channelId) await api.trayControl({ kind: 'open-live', channelId: pick.channelId });
      break;
    }
    case 'toggle-notify': {
      const next = state.general.notificationsEnabled === false;
      state.general = { ...state.general, notificationsEnabled: next };
      renderTiles();
      try {
        const updated = await api.settingsUpdate('general', { notificationsEnabled: next });
        if (updated?.general) state.general = updated.general;
      } catch { /* 무시 */ }
      renderTiles();
      break;
    }
    case 'toggle-theme': {
      const next = resolvedTheme() === 'dark' ? 'light' : 'dark';
      await setTheme(next);
      renderTiles();
      break;
    }
    case 'open-multilive':
      await api.trayControl({ kind: 'open-multilive' });
      break;
    case 'open-multichat':
      await api.trayControl({ kind: 'open-route', route: 'multichat', params: {} });
      break;
    case 'open-library':
      await api.trayControl({ kind: 'open-route', route: 'library', params: {} });
      break;
    case 'open-stats':
      await api.trayControl({ kind: 'open-route', route: 'home', params: { view: 'report' } });
      break;
    case 'check-update':
      await api.trayControl({ kind: 'check-update' });
      break;
    case 'login':
      await api.trayControl({ kind: 'login' });
      break;
    case 'switch-popular':
      switchTab('popular');
      break;
    case 'quit':
      await api.trayControl({ kind: 'quit' });
      break;
    default:
      break;
  }
}

function switchTab(tab) {
  if (tab !== 'following' && tab !== 'popular') return;
  state.tab = tab;
  renderTabs();
  renderList();
}

function bindEvents() {
  document.addEventListener('click', (e) => {
    const tabBtn = e.target.closest('[data-tab]');
    if (tabBtn) {
      switchTab(tabBtn.dataset.tab);
      return;
    }
    const itemBtn = e.target.closest('[data-item-act]');
    if (itemBtn) {
      const channelId = itemBtn.dataset.channelId;
      if (!channelId) return;
      const act = itemBtn.dataset.itemAct;
      if (act === 'watch') {
        api.trayControl({ kind: 'open-live', channelId }).catch(() => {});
      } else if (act === 'multi') {
        if (state.multiIds.has(channelId)) {
          api.trayControl({ kind: 'open-multilive' }).catch(() => {});
        } else {
          state.multiIds.add(channelId);
          renderTiles();
          renderList();
          api.trayControl({ kind: 'add-multilive', channelId }).catch(() => {});
        }
      } else if (act === 'chat') {
        api.trayControl({ kind: 'open-chat-window', channelId }).catch(() => {});
      }
      return;
    }
    const actBtn = e.target.closest('[data-act]');
    if (actBtn && !actBtn.disabled) {
      runAction(actBtn.dataset.act).catch(() => {});
    }
  });

  // 아바타 이미지 실패 시 이니셜 폴백
  document.addEventListener('error', (e) => {
    const img = e.target;
    if (!img || img.tagName !== 'IMG') return;
    if (img.classList.contains('mb-avatar')) {
      const row = img.closest('.mb-item');
      const name = row?.querySelector('.mb-item-name')?.textContent || 'C';
      const span = document.createElement('span');
      span.className = 'mb-avatar-initial';
      span.setAttribute('aria-hidden', 'true');
      span.textContent = Array.from(name.trim())[0] || 'C';
      img.replaceWith(span);
    }
  }, true);

  // 볼륨 슬라이더 실시간 반영(스와이프 중 재렌더 덮어쓰기 방지)
  const vol = $('mb-volume');
  let volTimer = 0;
  const sendVol = (value) => {
    clearTimeout(volTimer);
    volTimer = setTimeout(() => {
      api.trayControl({ kind: 'volume', value }).catch(() => {});
    }, 35);
  };
  vol.addEventListener('pointerdown', () => { state.draggingVolume = true; });
  window.addEventListener('pointerup', () => { state.draggingVolume = false; });
  window.addEventListener('pointercancel', () => { state.draggingVolume = false; });
  vol.addEventListener('input', () => {
    const v = Number(vol.value) || 0;
    $('mb-vol-val').textContent = `${v}%`;
    sendVol(v);
  });

  // 검색 필터
  const search = $('mb-search');
  const clearBtn = $('mb-search-clear');
  const syncSearch = () => {
    state.query = search.value;
    clearBtn.hidden = !state.query;
    renderList();
  };
  search.addEventListener('input', syncSearch);
  clearBtn.addEventListener('click', () => {
    search.value = '';
    syncSearch();
    search.focus();
  });

  // 키보드 내비게이션: Esc(검색 지우기 → 팝오버 닫기), / 또는 ⌘F(검색 초점), ⌘R(새로고침), ⌘,(설정), ↑/↓(채널 이동)
  document.addEventListener('keydown', (e) => {
    const inInput = e.target === search;
    if (e.key === 'Escape') {
      e.preventDefault();
      if (search.value) {
        search.value = '';
        syncSearch();
        return;
      }
      api.trayPopoverClose().catch(() => {});
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      refreshData({ force: true });
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === ',') {
      e.preventDefault();
      runAction('settings');
      return;
    }
    if (((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') || (!inInput && e.key === '/')) {
      e.preventDefault();
      search.focus();
      search.select();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const mains = [...document.querySelectorAll('.mb-list .mb-item-main')];
      if (!mains.length) return;
      const idx = mains.indexOf(document.activeElement);
      if (inInput && e.key === 'ArrowDown') {
        e.preventDefault();
        mains[0].focus();
      } else if (idx >= 0) {
        e.preventDefault();
        const next = e.key === 'ArrowDown' ? Math.min(mains.length - 1, idx + 1) : idx - 1;
        if (next < 0) search.focus();
        else mains[next].focus();
      }
    }
  });

  // 탭 좌우·처음·끝 화살표 전환(WAI-ARIA Tabs 패턴)
  document.querySelector('.mb-tabs').addEventListener('keydown', (e) => {
    let next = null;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      next = state.tab === 'following' ? 'popular' : 'following';
    } else if (e.key === 'Home') {
      next = 'following';
    } else if (e.key === 'End') {
      next = 'popular';
    }
    if (!next) return;
    e.preventDefault();
    switchTab(next);
    $(`mb-tab-${next}`)?.focus();
  });

  onThemeChange(() => renderTiles());
}

async function init() {
  installUiHealth();
  bindEvents();
  listenHere(Events.trayState, (snap) => applySnapshot(snap));
  listen(Events.authChanged, (auth) => {
    const loggedIn = Boolean(auth && auth.loggedIn);
    if (loggedIn !== state.loggedIn) {
      state.loggedIn = loggedIn;
      state.lastLoadedAt = 0;
      if (state.open) refreshDataForAuth(loggedIn);
    }
  });
  listen(Events.appearanceChanged, (a) => {
    if (a && a.theme) syncTheme(a.theme);
    renderTiles();
  });
  listen(Events.generalChanged, (g) => {
    if (g) {
      state.general = g;
      renderTiles();
    }
  });
  listen(Events.multiLiveState, (st) => {
    if (st && Array.isArray(st.sessions)) {
      state.multiIds = new Set(st.sessions.map((x) => x.channelId).filter(Boolean));
      if (state.open) {
        renderTiles();
        renderList();
      }
    }
  });
  await refreshData({ force: false });
}

init().catch((e) => console.error('메뉴 막대 팝오버 초기화 실패', e));
