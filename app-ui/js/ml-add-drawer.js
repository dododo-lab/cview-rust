// 멀티라이브·멀티채팅 채널 추가 — 오른쪽 채팅 위로 미끄러져 나오는 사이드바.
//
// - 화면 가운데를 가리던 모달 대신, 채팅 자리를 잠시 덮는다 — 무대(패인)는 그대로 보인다.
// - 고른 채널은 바로 추가하고 사이드바는 열어 둔다 — 여러 채널을 이어서 넣을 수 있다.
// - 정렬(시청자·방송 시간·이름)과 출처(전체·팔로잉·인기)는 기억한다. 새로고침은 캐시를 건너뛴다.
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { uptimeText } from './cards.js';

const ICON_CLOSE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>';
const ICON_REFRESH = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11.5a8 8 0 1 0-2.4 5.9"/><path d="M20 4.5v7h-7"/></svg>';
const ICON_PLUS = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 6v12M6 12h12"/></svg>';
const ICON_CHECK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
const ICON_SEARCH = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="11" cy="11" r="6.5"/><path d="M16 16l4 4"/></svg>';

export const ADD_SORTS = Object.freeze([
  { id: 'viewers', label: '시청자 많은순' },
  { id: 'viewers-asc', label: '시청자 적은순' },
  { id: 'recent', label: '최근 시작순' },
  { id: 'uptime', label: '오래 방송한순' },
  { id: 'name', label: '이름순' },
]);

export const ADD_SOURCES = Object.freeze([
  { id: 'all', label: '전체' },
  { id: 'following', label: '팔로잉' },
  { id: 'popular', label: '인기' },
]);

const STORE_KEY = 'cview.ml.addDrawer';
const STALE_MS = 60000; // 다시 열 때 이보다 오래된 목록이면 새로 받는다

function openTime(l) {
  const t = Date.parse(l.openDate || '');
  return Number.isFinite(t) ? t : null;
}

/** 정렬 — 순수 함수. 시간을 모르는 방송은 시간 정렬에서 맨 뒤, 같으면 시청자 많은순. */
export function sortLives(list, sort) {
  const byViewers = (a, b) => (b.viewerCount || 0) - (a.viewerCount || 0);
  const byTime = (dir) => (a, b) => {
    const ta = openTime(a);
    const tb = openTime(b);
    if (ta == null || tb == null) return (ta == null) - (tb == null) || byViewers(a, b);
    return (ta - tb) * dir || byViewers(a, b);
  };
  const cmp = {
    viewers: byViewers,
    'viewers-asc': (a, b) => (a.viewerCount || 0) - (b.viewerCount || 0),
    recent: byTime(-1),
    uptime: byTime(1),
    name: (a, b) => (a.channelName || '').localeCompare(b.channelName || '', 'ko') || byViewers(a, b),
  }[sort] || byViewers;
  return [...list].sort(cmp);
}

/** 출처·검색어로 거르기 — 순수 함수. */
export function filterLives(list, { source = 'all', query = '' } = {}) {
  const q = query.trim().toLowerCase();
  return list.filter((l) => (source === 'all' || (source === 'following' ? l.following : l.popular))
    && (!q
      || (l.channelName || '').toLowerCase().includes(q)
      || (l.liveTitle || '').toLowerCase().includes(q)
      || (l.categoryName || '').toLowerCase().includes(q)));
}

/** 팔로잉·인기 목록 합치기 — 같은 채널은 하나로, 출처 표시는 둘 다 남긴다. */
export function mergeLives(following, popular) {
  const map = new Map();
  for (const [list, key] of [[following, 'following'], [popular, 'popular']]) {
    for (const l of list || []) {
      if (!l.isLive || !l.channelId) continue;
      const prev = map.get(l.channelId);
      map.set(l.channelId, { ...(prev || l), following: Boolean(prev?.following), popular: Boolean(prev?.popular), [key]: true });
    }
  }
  return [...map.values()];
}

function relTime(at, now = Date.now()) {
  const sec = Math.round((now - at) / 1000);
  if (sec < 10) return '방금';
  if (sec < 60) return `${sec}초 전`;
  return `${Math.floor(sec / 60)}분 전`;
}

export class MultiLiveAddDrawer {
  /**
   * @param {HTMLElement} host `.ml-body` — 채팅과 같은 컨테이너
   * @param {{ isLoggedIn: () => boolean, getState: () => any, onAdd: (channelId: string) => Promise<void>,
   *   toggleSelector?: string, unit?: string }} opts
   *   toggleSelector — 여는 버튼(바깥 클릭으로 치지 않는다), unit — 꽉 참 안내에 쓰는 이름(패인·채널)
   */
  constructor(host, { isLoggedIn, getState, onAdd, toggleSelector = '[data-ml="add"]', unit = '패인' }) {
    this.host = host;
    this.toggleSelector = toggleSelector;
    this.unit = unit;
    this.isLoggedIn = isLoggedIn;
    this.getState = getState;
    this.onAdd = onAdd;
    this.items = [];
    this.loadedAt = 0;
    this.loading = false;
    this.adding = new Set();
    this.failed = '';
    const saved = (() => { try { return JSON.parse(localStorage.getItem(STORE_KEY) || '{}') || {}; } catch { return {}; } })();
    this.sort = ADD_SORTS.some((s) => s.id === saved.sort) ? saved.sort : 'viewers';
    this.source = ADD_SOURCES.some((s) => s.id === saved.source) ? saved.source : 'all';

    this.el = document.createElement('aside');
    this.el.className = 'ml-addbar';
    this.el.setAttribute('role', 'dialog');
    this.el.setAttribute('aria-label', '채널 추가');
    this.el.setAttribute('aria-hidden', 'true');
    this.el.inert = true;
    this.el.innerHTML = `
      <header class="ml-addbar-head">
        <strong>채널 추가</strong>
        <span class="ml-addbar-count"></span>
        <div class="grow"></div>
        <button class="ml-tb-icon ml-addbar-refresh" data-add="refresh" title="새로고침" aria-label="새로고침">${ICON_REFRESH}</button>
        <button class="ml-tb-icon" data-add="close" title="닫기 (Esc)" aria-label="닫기">${ICON_CLOSE}</button>
      </header>
      <div class="ml-addbar-tools">
        <label class="ml-addbar-search">${ICON_SEARCH}<input type="search" placeholder="채널·제목·카테고리로 찾기" autocomplete="off" spellcheck="false" aria-label="방송 찾기"></label>
        <div class="ml-addbar-row">
          <div class="segmented ml-addbar-source" role="group" aria-label="출처">
            ${ADD_SOURCES.map((s) => `<button class="seg" data-source="${s.id}">${s.label}</button>`).join('')}
          </div>
          <select class="select ml-addbar-sort" aria-label="정렬">
            ${ADD_SORTS.map((s) => `<option value="${s.id}">${s.label}</option>`).join('')}
          </select>
        </div>
      </div>
      <div class="ml-addbar-notice" hidden></div>
      <div class="ml-addbar-list" tabindex="-1"></div>
      <footer class="ml-addbar-foot"><span class="ml-addbar-status"></span></footer>`;
    host.append(this.el);

    this.list = this.el.querySelector('.ml-addbar-list');
    this.filter = this.el.querySelector('input[type="search"]');
    this.sortSelect = this.el.querySelector('.ml-addbar-sort');
    this.notice = this.el.querySelector('.ml-addbar-notice');
    this.status = this.el.querySelector('.ml-addbar-status');
    this.countEl = this.el.querySelector('.ml-addbar-count');
    this.refreshBtn = this.el.querySelector('[data-add="refresh"]');
    this.sortSelect.value = this.sort;

    this.el.addEventListener('click', (e) => this.onClick(e));
    this.filter.addEventListener('input', () => this.renderList());
    this.filter.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        this.list.querySelector('.ml-addrow:not(:disabled)')?.click();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        this.list.querySelector('.ml-addrow:not(:disabled)')?.focus();
      }
    });
    this.list.addEventListener('keydown', (e) => {
      if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
      const rows = [...this.list.querySelectorAll('.ml-addrow:not(:disabled)')];
      const i = rows.indexOf(document.activeElement);
      if (i < 0) return;
      e.preventDefault();
      if (e.key === 'ArrowUp' && i === 0) this.filter.focus();
      else rows[Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1))]?.focus();
    });
    this.sortSelect.addEventListener('change', () => { this.sort = this.sortSelect.value; this.persist(); this.renderList(); });
    this.el.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { e.stopPropagation(); this.close(); }
    });
    // 바깥(무대·머리줄)을 누르면 닫는다 — 여는 버튼은 onClick 의 토글이 처리한다.
    document.addEventListener('pointerdown', (e) => {
      if (this.isOpen && !this.el.contains(e.target) && !e.target.closest(this.toggleSelector) && !e.target.closest('.toast, .sheet-scrim')) this.close();
    });
    // '3분 전' 같은 갱신 시각은 열려 있을 때만 다시 쓴다.
    setInterval(() => { if (this.isOpen) this.renderStatus(); }, 15000);
    this.renderSource();
  }

  get isOpen() { return this.el.classList.contains('open'); }

  persist() {
    try { localStorage.setItem(STORE_KEY, JSON.stringify({ sort: this.sort, source: this.source })); } catch { /* 이번 실행만 */ }
  }

  toggle() { if (this.isOpen) this.close(); else this.open(); }

  open() {
    // 채팅이 펼쳐져 있으면 그 너비를 그대로 덮는다 — 접혀 있거나 오버레이거나 채팅 칸이 없으면(멀티채팅) 기본 너비.
    const chat = this.host.querySelector(':scope > .ml-chat');
    const docked = chat && !chat.classList.contains('collapsed') && !this.host.classList.contains('chat-overlay');
    const w = docked ? chat.getBoundingClientRect().width : 0;
    this.el.style.setProperty('--ml-addbar-w', `${Math.max(320, Math.round(w)) || 360}px`);
    this.el.classList.add('open');
    this.el.setAttribute('aria-hidden', 'false');
    this.el.inert = false;
    this.filter.value = '';
    this.sync();
    if (!this.items.length || Date.now() - this.loadedAt > STALE_MS) this.load(false);
    else this.renderList();
    requestAnimationFrame(() => this.filter.focus({ preventScroll: true }));
  }

  close() {
    if (!this.isOpen) return;
    this.el.classList.remove('open');
    this.el.setAttribute('aria-hidden', 'true');
    this.el.inert = true;
  }

  /** 멀티라이브 상태가 바뀌면 — 추가됨 표시·남은 자리·꽉 참 안내. */
  sync() {
    if (!this.isOpen) return;
    const st = this.getState() || {};
    const n = (st.sessions || []).length;
    this.countEl.textContent = `${n} / ${st.maxSessions ?? 4}`;
    this.notice.hidden = st.canAdd !== false;
    this.notice.textContent = `최대 ${st.maxSessions ?? 4}개까지 볼 수 있어요 — ${this.unit}을 닫으면 더 추가할 수 있습니다`;
    this.renderList();
  }

  async load(refresh) {
    if (this.loading) return;
    this.loading = true;
    this.failed = '';
    this.refreshBtn.classList.add('spinning');
    this.refreshBtn.disabled = true;
    if (!this.items.length) this.list.innerHTML = '<div class="empty"><div class="spinner"></div></div>';
    this.renderStatus();
    const [following, popular] = await Promise.allSettled([
      this.isLoggedIn() ? api.followingLives(refresh) : Promise.resolve([]),
      api.popularLives(50, refresh),
    ]);
    this.loading = false;
    this.refreshBtn.classList.remove('spinning');
    this.refreshBtn.disabled = false;
    if (following.status === 'rejected' && popular.status === 'rejected') {
      this.failed = popular.reason?.message || '방송 목록을 불러오지 못했습니다';
      if (refresh) toast(this.failed);
    } else {
      this.items = mergeLives(following.status === 'fulfilled' ? following.value : [], popular.status === 'fulfilled' ? popular.value : []);
      this.loadedAt = Date.now();
      if (refresh) toast('방송 목록을 새로 받았습니다', 1200);
    }
    this.renderList();
  }

  renderSource() {
    for (const b of this.el.querySelectorAll('[data-source]')) {
      const on = b.dataset.source === this.source;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
  }

  renderStatus() {
    if (this.loading) { this.status.textContent = '불러오는 중…'; return; }
    if (this.failed && !this.items.length) { this.status.textContent = this.failed; return; }
    const shown = this.list.querySelectorAll('.ml-addrow').length;
    this.status.textContent = this.loadedAt ? `방송 ${shown}개 · ${relTime(this.loadedAt)} 갱신` : '';
  }

  renderList() {
    if (this.loading && !this.items.length) { this.renderStatus(); return; }
    const st = this.getState() || {};
    const canAdd = st.canAdd !== false;
    const added = new Set((st.sessions || []).map((s) => s.channelId));
    const items = sortLives(filterLives(this.items, { source: this.source, query: this.filter.value }), this.sort);
    if (!items.length) {
      const why = this.failed && !this.items.length
        ? `<strong>방송 목록을 불러오지 못했습니다</strong><button class="btn" data-add="refresh">다시 시도</button>`
        : this.source === 'following' && !this.isLoggedIn()
          ? '<strong>로그인하면 팔로잉 방송을 볼 수 있어요</strong>'
          : `<strong>${this.filter.value.trim() ? '찾는 방송이 없습니다' : '추가할 수 있는 방송이 없습니다'}</strong>`;
      this.list.innerHTML = `<div class="empty">${why}</div>`;
      this.renderStatus();
      return;
    }
    const scroll = this.list.scrollTop;
    this.list.innerHTML = items.map((l) => {
      const thumb = safeUrl(l.thumbnailUrl);
      const isAdded = added.has(l.channelId);
      const busy = this.adding.has(l.channelId);
      const uptime = uptimeText(l.openDate);
      const tag = l.following ? '<span class="ml-addrow-tag">팔로잉</span>' : '';
      const meta = [l.categoryName, uptime].filter(Boolean).map(esc).join(' · ');
      return `
        <button class="ml-addrow${isAdded ? ' added' : ''}${busy ? ' busy' : ''}" data-add="pick" data-channel-id="${esc(l.channelId)}"
          ${isAdded || busy || !canAdd ? 'disabled' : ''} title="${esc(l.liveTitle || l.channelName || '')}">
          <span class="ml-addrow-thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
            <span class="ml-addrow-viewers">${esc(l.viewerCountText ?? String(l.viewerCount ?? ''))}명</span></span>
          <span class="ml-addrow-text">
            <span class="ml-addrow-name"><span class="ellipsis">${esc(l.channelName)}</span>${tag}</span>
            <span class="ml-addrow-title ellipsis">${esc(l.liveTitle || '')}</span>
            ${meta ? `<span class="ml-addrow-meta ellipsis">${meta}</span>` : ''}
          </span>
          <span class="ml-addrow-act" aria-hidden="true">${busy ? '<span class="spinner"></span>' : isAdded ? ICON_CHECK : ICON_PLUS}</span>
        </button>`;
    }).join('');
    this.list.scrollTop = scroll;
    this.renderStatus();
  }

  async onClick(e) {
    const src = e.target.closest('[data-source]');
    if (src) {
      this.source = src.dataset.source;
      this.persist();
      this.renderSource();
      this.renderList();
      return;
    }
    const act = e.target.closest('[data-add]');
    if (!act) return;
    switch (act.dataset.add) {
      case 'close': this.close(); break;
      case 'refresh': this.load(true); break;
      case 'pick': {
        const id = act.dataset.channelId;
        if (!id || this.adding.has(id)) break;
        this.adding.add(id);
        this.renderList();
        try { await this.onAdd(id); } finally {
          this.adding.delete(id);
          if (this.isOpen) this.sync(); else this.renderList();
        }
        break;
      }
      default: break;
    }
  }
}
