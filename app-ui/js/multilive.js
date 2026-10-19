// 멀티라이브 — 여러 방송을 한 화면에서 (원본 `MultiLiveView`·`MultiLiveManager`).
//
// - 세션·선택·오디오·화질 상한은 Rust `MultiLiveManager` 가 정하고 `multilive://state` 로 알려 준다.
// - 패인마다 플레이어 iframe 이 하나다. iframe 은 엔진 슬롯 번호로 묶여 레이아웃이 바뀌어도
//   옮기지 않는다(옮기면 다시 로드된다) — 위치는 CSS grid 의 `order` 로만 바꾼다.
// - 슬롯 명령(`multilive://command`)은 패인 iframe 이 생기기 전에 올 수 있어 슬롯별로 줄 세운다.
// - 패인을 누르면 선택 — 오디오·채팅·고화질이 그 패인을 따라간다.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast } from './dom.js';
import { PlayerBridge } from './player.js';
import { ChatPanel } from './chat.js';

export const multiLiveIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 5a2 2 0 0 1 2-2h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5Zm9 9a2 2 0 0 1 2-2h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-5a2 2 0 0 1-2-2v-5ZM14 3h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2ZM5 12h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2Z" opacity=".9"/></svg>';

const ICON_CLOSE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>';
const ICON_SOUND = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h4l5-4v14l-5-4H4V9Z"/><path d="M16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';
const ICON_PLUS = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';

const STATE_TEXT = {
  idle: '대기 중',
  loading: '불러오는 중…',
  playing: '',
  offline: '방송이 종료되었습니다',
};

/** 패인 배치 순서 — 포커스 레이아웃이면 메인 패인(매니저의 focusMain)을 맨 앞에 둔다. */
export function paneOrder(sessions, mainId, layout) {
  const ids = sessions.map((s) => s.id);
  if (layout !== 'focus' || !mainId || !ids.includes(mainId)) return ids;
  return [mainId, ...ids.filter((id) => id !== mainId)];
}

export class MultiLivePage {
  constructor(content, { isLoggedIn }) {
    this.isLoggedIn = isLoggedIn;
    this.root = document.createElement('section');
    this.root.className = 'page page-multilive';
    this.root.dataset.page = 'multilive';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header ml-header">
        <h1 class="page-title">멀티라이브</h1>
        <span class="page-subtitle ml-count"></span>
        <div class="grow"></div>
        <div class="segmented" role="group" aria-label="레이아웃">
          <button class="seg" data-layout="grid">그리드</button>
          <button class="seg" data-layout="focus">포커스</button>
        </div>
        <button class="btn primary" data-ml="add">${ICON_PLUS}<span>채널 추가</span></button>
        <button class="btn ghost" data-ml="stop-all">모두 닫기</button>
      </header>
      <div class="page-body ml-body">
        <div class="ml-stage">
          <div class="ml-empty">
            <strong>여러 방송을 한 화면에서 보세요</strong>
            <span>채널을 추가하면 최대 4개까지 나란히 재생합니다. 패인을 누르면 소리와 채팅이 그 방송으로 바뀝니다.</span>
            <button class="btn primary lg" data-ml="add">${ICON_PLUS}<span>채널 추가</span></button>
          </div>
        </div>
        <aside class="chat ml-chat">
          <header class="chat-header">
            <strong>채팅</strong>
            <span class="chat-state">연결 끊김</span>
          </header>
          <div class="chat-notice" hidden></div>
          <div class="chat-list"></div>
          <form class="chat-input">
            <input type="text" maxlength="100" placeholder="채팅을 입력하세요…" autocomplete="off">
            <button class="btn primary" type="submit">채팅</button>
          </form>
        </aside>
      </div>
      <div class="ml-picker" hidden>
        <div class="ml-picker-panel" role="dialog" aria-label="채널 추가">
          <header class="ml-picker-head">
            <strong>채널 추가</strong>
            <input type="search" class="ml-picker-filter" placeholder="채널·제목으로 찾기" autocomplete="off" spellcheck="false">
            <button class="icon-btn" data-ml="picker-close" title="닫기">${ICON_CLOSE}</button>
          </header>
          <div class="ml-picker-list"></div>
        </div>
      </div>`;
    content.append(this.root);

    this.stage = this.root.querySelector('.ml-stage');
    this.empty = this.root.querySelector('.ml-empty');
    this.count = this.root.querySelector('.ml-count');
    this.picker = this.root.querySelector('.ml-picker');
    this.pickerList = this.root.querySelector('.ml-picker-list');
    this.pickerFilter = this.root.querySelector('.ml-picker-filter');
    this.chat = new ChatPanel(this.root.querySelector('.ml-chat'), { isLoggedIn });

    this.state = { sessions: [], selectedId: null, canAdd: true, maxSessions: 4, layout: 'grid' };
    this.panes = new Map(); // slot → { el, bridge, sessionId }
    this.pending = new Map(); // slot → [script]
    this.chatKey = null;
    this.pickerItems = [];

    listen(Events.multiLiveCommand, (p) => p && this.dispatch(p.slot, p.script));
    listen(Events.multiLiveState, (s) => this.render(s));

    this.root.addEventListener('click', (e) => this.onClick(e));
    this.pickerFilter.addEventListener('input', () => this.renderPicker());
    new ResizeObserver(() => this.reportStageSize()).observe(this.stage);
  }

  async show() {
    try {
      this.render(await api.multiLiveState());
    } catch (e) {
      toast(e.message);
    }
    this.reportStageSize();
  }

  // ── Rust → 패인 ─────────────────────────────

  dispatch(slot, script) {
    const pane = this.panes.get(slot);
    if (pane) {
      pane.bridge.exec(script);
      return;
    }
    if (!this.pending.has(slot)) this.pending.set(slot, []);
    this.pending.get(slot).push(script);
  }

  ensurePane(slot) {
    let pane = this.panes.get(slot);
    if (pane) return pane;
    const el = document.createElement('div');
    el.className = 'ml-pane';
    el.dataset.slot = String(slot);
    el.innerHTML = `
      <iframe src="player/player.html" title="멀티라이브 플레이어 ${slot}" allow="autoplay; fullscreen"></iframe>
      <div class="ml-pane-hit" data-ml="select"></div>
      <div class="ml-pane-state"><div class="spinner"></div><span></span></div>
      <div class="ml-pane-top">
        <span class="ml-pane-sound" title="소리 나는 패인">${ICON_SOUND}</span>
        <span class="ml-pane-cap"></span>
        <div class="grow"></div>
        <button class="ml-pane-close" data-ml="remove" title="닫기">${ICON_CLOSE}</button>
      </div>
      <div class="ml-pane-info">
        <div class="ml-pane-avatar"></div>
        <div class="ml-pane-text">
          <div class="ml-pane-name ellipsis"></div>
          <div class="ml-pane-title ellipsis"></div>
        </div>
      </div>`;
    this.stage.append(el);
    const iframe = el.querySelector('iframe');
    const bridge = new PlayerBridge(iframe, {
      commandEvent: null,
      forward: (message) => api.multiLivePlayerMessage(slot, message),
    });
    // 페이지는 자동재생 정책 때문에 음소거로 시작한다. 들리는지는 Rust 가 볼륨(0/1)으로 정한다.
    bridge.onMessage((type) => {
      if (type === 'playStarted') bridge.call('setMuted', false);
    });
    pane = { el, bridge, sessionId: null };
    this.panes.set(slot, pane);
    const queued = this.pending.get(slot);
    if (queued) {
      this.pending.delete(slot);
      for (const s of queued) bridge.exec(s);
    }
    return pane;
  }

  // ── 상태 렌더링 ─────────────────────────────

  render(state) {
    if (!state) return;
    this.state = state;
    const sessions = state.sessions || [];
    const n = sessions.length;
    this.count.textContent = `${n} / ${state.maxSessions}`;
    for (const b of this.root.querySelectorAll('[data-layout]')) {
      b.classList.toggle('active', b.dataset.layout === state.layout);
    }
    for (const b of this.root.querySelectorAll('[data-ml="add"]')) b.disabled = !state.canAdd;
    this.root.querySelector('[data-ml="stop-all"]').hidden = n === 0;

    this.stage.className = `ml-stage ml-n${Math.min(n, 4)} ${state.layout === 'focus' && n > 1 ? 'ml-focus' : ''}`;
    this.empty.hidden = n > 0;

    const order = paneOrder(sessions, state.focusMainId || state.selectedId, state.layout);
    const live = new Set();
    for (const s of sessions) {
      if (s.slot == null) continue;
      live.add(s.slot);
      const pane = this.ensurePane(s.slot);
      pane.sessionId = s.id;
      this.renderPane(pane, s, order.indexOf(s.id), s.id === state.selectedId);
    }
    for (const [slot, pane] of this.panes) {
      if (!live.has(slot)) {
        pane.el.hidden = true;
        pane.sessionId = null;
      }
    }
    this.syncChat(sessions.find((s) => s.id === state.selectedId) || null);
  }

  renderPane(pane, s, index, selected) {
    const { el } = pane;
    el.hidden = false;
    el.style.order = String(index);
    el.classList.toggle('ml-first', index === 0);
    el.classList.toggle('selected', selected);
    el.classList.toggle('audible', s.isAudible);
    el.querySelector('.ml-pane-name').textContent = s.channelName || s.channelId;
    el.querySelector('.ml-pane-title').textContent = s.liveTitle || '';
    const avatar = safeUrl(s.profileImageUrl);
    el.querySelector('.ml-pane-avatar').innerHTML = avatar
      ? `<img src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '';
    el.querySelector('.ml-pane-cap').textContent = s.maxHeightCap > 0 ? `≤${s.maxHeightCap}p` : '';
    const kind = s.state && s.state.kind;
    const text = kind === 'error' ? (s.state.message || '재생 오류') : (STATE_TEXT[kind] ?? '');
    const overlay = el.querySelector('.ml-pane-state');
    overlay.hidden = !text;
    overlay.classList.toggle('error', kind === 'error');
    overlay.querySelector('.spinner').hidden = kind !== 'loading';
    overlay.querySelector('span').textContent = text;
  }

  /** 채팅은 선택 패인의 채널을 따라간다. */
  syncChat(session) {
    const key = session && session.chatChannelId ? `${session.id}:${session.chatChannelId}` : null;
    if (key === this.chatKey) return;
    this.chatKey = key;
    if (key) this.chat.connect(session.chatChannelId, session.channelId);
    else this.chat.disconnect();
  }

  reportStageSize() {
    if (this.root.hidden) return;
    clearTimeout(this.sizeTimer);
    this.sizeTimer = setTimeout(() => {
      const r = this.stage.getBoundingClientRect();
      if (r.width > 0 && r.height > 0) {
        const scale = window.devicePixelRatio || 1;
        api.multiLiveStageSize(r.width * scale, r.height * scale).catch(() => {});
      }
    }, 200);
  }

  // ── 동작 ───────────────────────────────────

  async onClick(e) {
    const layout = e.target.closest('[data-layout]');
    if (layout) {
      this.run(api.multiLiveSetLayout(layout.dataset.layout));
      return;
    }
    const action = e.target.closest('[data-ml]');
    if (!action) {
      if (e.target === this.picker) this.closePicker();
      return;
    }
    const pane = action.closest('.ml-pane');
    const sessionId = pane && this.panes.get(Number(pane.dataset.slot))?.sessionId;
    switch (action.dataset.ml) {
      case 'add': this.openPicker(); break;
      case 'picker-close': this.closePicker(); break;
      case 'stop-all': this.run(api.multiLiveStopAll()); break;
      case 'select': if (sessionId && sessionId !== this.state.selectedId) this.run(api.multiLiveSelect(sessionId)); break;
      case 'remove': if (sessionId) this.run(api.multiLiveRemove(sessionId)); break;
      case 'pick': this.closePicker(); await this.add(action.dataset.channelId); break;
      default: break;
    }
  }

  async run(promise) {
    try {
      this.render(await promise);
    } catch (e) {
      toast(e.message);
    }
  }

  /** 채널 추가 — 카드·시청 화면의 "멀티라이브에 추가" 도 이걸 부른다. */
  async add(channelId) {
    if (!channelId) return;
    toast('멀티라이브에 추가하는 중…', 1500);
    await this.run(api.multiLiveAdd(channelId));
  }

  async openPicker() {
    this.picker.hidden = false;
    this.pickerFilter.value = '';
    this.pickerList.innerHTML = '<div class="empty"><div class="spinner"></div></div>';
    this.pickerFilter.focus();
    const [following, popular] = await Promise.allSettled([
      this.isLoggedIn() ? api.followingLives(false) : Promise.resolve([]),
      api.popularLives(40, false),
    ]);
    const seen = new Set();
    const items = [];
    const push = (list, group) => {
      for (const l of list || []) {
        if (!l.isLive || seen.has(l.channelId)) continue;
        seen.add(l.channelId);
        items.push({ ...l, group });
      }
    };
    if (following.status === 'fulfilled') push(following.value, '팔로잉');
    if (popular.status === 'fulfilled') push(popular.value, '인기');
    this.pickerItems = items;
    this.renderPicker();
  }

  renderPicker() {
    const q = this.pickerFilter.value.trim().toLowerCase();
    const added = new Set((this.state.sessions || []).map((s) => s.channelId));
    const items = this.pickerItems.filter((l) => !q
      || (l.channelName || '').toLowerCase().includes(q)
      || (l.liveTitle || '').toLowerCase().includes(q));
    if (!items.length) {
      this.pickerList.innerHTML = '<div class="empty"><strong>추가할 수 있는 방송이 없습니다</strong></div>';
      return;
    }
    let group = null;
    this.pickerList.innerHTML = items.map((l) => {
      const head = l.group !== group ? `<div class="ml-picker-group">${esc(l.group)}</div>` : '';
      group = l.group;
      const thumb = safeUrl(l.thumbnailUrl);
      const isAdded = added.has(l.channelId);
      return `${head}
        <button class="ml-pick" data-ml="pick" data-channel-id="${esc(l.channelId)}" ${isAdded ? 'disabled' : ''}>
          <span class="ml-pick-thumb">${thumb ? `<img src="${esc(thumb)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}</span>
          <span class="ml-pick-text">
            <span class="ml-pick-name ellipsis">${esc(l.channelName)}${isAdded ? ' · 추가됨' : ''}</span>
            <span class="ml-pick-title ellipsis">${esc(l.liveTitle || '')}</span>
          </span>
          <span class="ml-pick-viewers">${esc(l.viewerCountText)}명</span>
        </button>`;
    }).join('');
  }

  closePicker() {
    this.picker.hidden = true;
  }
}

/**
 * 카드의 "멀티라이브에 추가"(+) 버튼 — 카드 열기보다 먼저(캡처 단계에서) 가로챈다.
 * @param {(channelId: string) => void} onAdd
 */
export function bindMultiLiveAddButtons(onAdd) {
  document.addEventListener('click', (e) => {
    const button = e.target.closest('[data-ml-add]');
    if (!button) return;
    e.preventDefault();
    e.stopPropagation();
    const card = button.closest('[data-channel-id]');
    if (card) onAdd(card.dataset.channelId);
  }, true);
}

