// 멀티라이브 멀티채팅 — 모든 패인의 채팅을 채팅 패널 안에서 함께 본다.
//
// 보기(채팅 머리의 말풍선 버튼 메뉴, 이 기기에 기억):
//   off   — 선택 패인 채팅만(기본 ChatPanel)
//   merged — 통합: 한 타임라인, 줄마다 패인 이름 태그·패인 색
//   grid  — 그리드: 패인마다 칸 하나씩(패널이 넓으면 여러 열, 좁으면 위아래)
//   focus — 포커스: 선택 패인 채팅을 크게, 나머지 패인은 아래 작은 칸으로
// - 켜면 패인마다 추가 채팅 세션(`mlx:<세션 id>`)을 연다. 선택 패인도 따로 연다 — 선택이 바뀌어도
//   목록이 끊기거나 같은 메시지가 두 번 들어오지 않게(선택 패인 채팅 `ml` 은 패널이 계속 쓴다).
// - 통합 목록과 칸 목록에 함께 쌓아 두어 보기를 바꿔도 지난 메시지가 남는다. 보내기는 선택 패인으로 간다.
//   칸 머리를 누르면 그 패인을 선택한다(포커스면 그 칸이 커진다).
import { api, listen } from './api.js';
import { esc } from './dom.js';
import { mergedLinesHtml } from './multichat.js';
import { applyChatSkin } from './chat-extras.js';
import { onChatPrefs } from './chat-prefs.js';
import { chatFollower, followAppend } from './chat-follow.js';
import { bindMenuKeys, focusFirstMenuItem } from './ml-pane-controls.js';

const STORE_KEY = 'cview.ml.chatView';
const MODES = [
  { id: 'off', label: '선택한 패인 채팅만' },
  { id: 'merged', label: '통합 — 한 목록으로' },
  { id: 'grid', label: '그리드 — 패인별로 나눠 보기' },
  { id: 'focus', label: '포커스 — 선택 패인을 크게' },
];
const TITLES = { off: '채팅', merged: '멀티채팅', grid: '채팅 그리드', focus: '채팅 포커스' };
const MAX_LINES = 600;
const MAX_CELL_LINES = 300;
const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H9l-4 3v-3H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z"/><path d="M19 9h1a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-1v3l-4-3h-4"/></svg>';

const sessionKey = (id) => `mlx:${id}`;

function loadMode() {
  try {
    const v = localStorage.getItem(STORE_KEY);
    if (MODES.some((m) => m.id === v)) return v;
    if (localStorage.getItem('cview.ml.mergedChat') === '1') return 'merged'; // 예전 켬/끔 기억
  } catch { /* 저장 불가 */ }
  return 'off';
}

/** 칸에 붙이고 넘치면 앞에서 지운다 — 바닥에 있었으면 계속 바닥을 따라간다. */
function appendTo(list, html, max) {
  followAppend(list, () => {
    const empty = list.querySelector(':scope > .empty');
    if (empty) empty.remove();
    list.insertAdjacentHTML('beforeend', html);
    while (list.children.length > max) list.firstElementChild.remove();
  });
}

export class MultiLiveMergedChat {
  /**
   * @param {HTMLElement} chatEl 멀티라이브 채팅 패널(.ml-chat)
   * @param {{ getState: () => object, getPanes: () => Map, onSelect: (sessionId: string) => void }} deps
   */
  constructor(chatEl, { getState, getPanes, onSelect }) {
    this.chatEl = chatEl;
    this.getState = getState;
    this.getPanes = getPanes;
    this.onSelect = onSelect;
    this.mode = loadMode();
    this.conns = new Map(); // 세션 id → { key, chatChannelId, channelId, name, hue, cell, list }
    this.panelList = chatEl.querySelector('.chat-list');
    this.list = document.createElement('div');
    this.list.className = 'chat-list ml-merged-list';
    this.list.innerHTML = '<div class="empty">모든 패인의 새 메시지를 기다리는 중…</div>';
    this.grid = document.createElement('div');
    this.grid.className = 'ml-mc-grid';
    this.panelList.after(this.list, this.grid);
    const skin = () => { applyChatSkin(this.list); applyChatSkin(this.grid); };
    onChatPrefs(skin);
    skin();
    this.title = chatEl.querySelector('.chat-header strong');

    const header = chatEl.querySelector('.chat-header');
    const wrap = document.createElement('div');
    wrap.className = 'ml-mc-view';
    wrap.innerHTML = `<button type="button" class="icon-btn chat-hbtn ml-merge-btn" aria-haspopup="menu" aria-expanded="false">${ICON}</button>
      <div class="ml-more-menu ml-mc-menu" role="menu" hidden>${MODES.map((m) => `<button type="button" class="ml-menu-item" role="menuitemradio" data-mc-view="${m.id}">${esc(m.label)}</button>`).join('')}</div>`;
    header.insertBefore(wrap, header.querySelector('.chat-hbtn') || null);
    this.button = wrap.querySelector('.ml-merge-btn');
    this.menu = wrap.querySelector('.ml-mc-menu');
    // 키보드 — 키보드로 열면 지금 보기(없으면 첫 항목)로, ↑↓ 이동, 고르거나 Esc 로 닫으면 연 버튼으로 초점을 돌려준다.
    bindMenuKeys(this.menu);
    this.button.addEventListener('click', (e) => {
      e.stopPropagation();
      const open = this.menu.hidden;
      this.openMenu(open);
      if (open && this.button.matches(':focus-visible')) {
        const cur = this.menu.querySelector('[data-mc-view].active');
        if (cur) cur.focus({ preventScroll: true }); else focusFirstMenuItem(this.menu);
      }
    });
    this.menu.addEventListener('click', (e) => {
      const b = e.target.closest('[data-mc-view]');
      if (b) { this.setMode(b.dataset.mcView); this.openMenu(false); }
    });
    document.addEventListener('pointerdown', (e) => { if (!this.menu.hidden && !e.target.closest('.ml-mc-view')) this.openMenu(false); });
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !this.menu.hidden) this.openMenu(false); });
    this.grid.addEventListener('click', (e) => {
      const head = e.target.closest('.ml-mc-cell-head');
      if (head && head.dataset.session) this.onSelect(head.dataset.session);
    });
    listen('chat://session-event', (p) => this.onEvent(p));
    this.applyChrome();
  }

  get on() { return this.mode !== 'off'; }

  openMenu(open) {
    const back = !open && this.menu.contains(document.activeElement); // 숨으며 초점이 body 로 떨어지지 않게
    this.menu.hidden = !open;
    this.button.setAttribute('aria-expanded', String(open));
    if (back) this.button.focus({ preventScroll: true });
  }

  setMode(mode) {
    if (!MODES.some((m) => m.id === mode) || mode === this.mode) return;
    this.mode = mode;
    try { localStorage.setItem(STORE_KEY, mode); } catch { /* 저장 불가 */ }
    this.applyChrome();
    this.sync();
  }

  applyChrome() {
    this.button.classList.toggle('active', this.on);
    const label = MODES.find((m) => m.id === this.mode).label;
    this.button.title = `채팅 보기: ${label}`;
    this.button.setAttribute('aria-label', this.button.title);
    for (const b of this.menu.querySelectorAll('[data-mc-view]')) {
      const cur = b.dataset.mcView === this.mode;
      b.classList.toggle('active', cur);
      b.setAttribute('aria-checked', String(cur));
    }
    this.chatEl.dataset.mcView = this.mode;
    this.panelList.hidden = this.on;
    this.list.hidden = this.mode !== 'merged';
    this.grid.hidden = this.mode !== 'grid' && this.mode !== 'focus';
    if (this.title) this.title.textContent = TITLES[this.mode];
  }

  /** 패인 구성에 맞춰 추가 세션을 열고 닫고, 칸을 배치한다(render 마다 불린다 — 바뀐 것만 건드린다). */
  sync() {
    const state = this.getState() || {};
    const all = (state.sessions || []).filter((s) => s.chatChannelId);
    const sessions = this.on ? all : [];
    const hueOf = new Map();
    for (const p of this.getPanes().values()) if (p.sessionId) hueOf.set(p.sessionId, p.hue);
    const keep = new Set();
    for (const s of sessions) {
      keep.add(s.id);
      const name = s.channelName || s.channelId;
      const hue = hueOf.get(s.id);
      let c = this.conns.get(s.id);
      if (c && c.chatChannelId !== s.chatChannelId) { this.drop(s.id, c); c = null; }
      if (!c) {
        c = { key: sessionKey(s.id), chatChannelId: s.chatChannelId, channelId: s.channelId, name, hue: hue ?? 0 };
        c.cell = document.createElement('section');
        c.cell.className = 'ml-mc-cell';
        c.cell.innerHTML = `<button type="button" class="ml-mc-cell-head" data-session="${esc(s.id)}"><i></i><span class="ellipsis"></span><b class="ml-mc-cell-send" title="채팅 입력이 이 채널로 갑니다">입력 대상</b></button>
          <div class="chat-list"><div class="empty">새 메시지를 기다리는 중…</div></div>`;
        c.list = c.cell.querySelector('.chat-list');
        this.conns.set(s.id, c);
        const conn = c;
        api.chatConnect(c.chatChannelId, c.channelId, c.key).catch((e) => {
          if (this.conns.get(s.id) === conn) this.system(conn, `${conn.name} 채팅에 연결하지 못했어요 — ${e.message}`);
        });
      }
      c.name = name;
      if (hue != null) c.hue = hue;
      c.cell.style.setProperty('--lh', String(Number(c.hue) || 0));
      const label = c.cell.querySelector('.ml-mc-cell-head span');
      if (label.textContent !== name) label.textContent = name;
    }
    for (const [id, c] of this.conns) if (!keep.has(id)) this.drop(id, c);
    this.layoutCells(sessions, state.selectedId);
  }

  drop(id, c) {
    this.conns.delete(id);
    c.cell.remove();
    api.chatDisconnect(c.key).catch(() => {});
  }

  /** 칸 순서·크기 — 그리드는 패인 순서대로 고르게, 포커스는 선택 패인을 맨 위에 크게. */
  layoutCells(sessions, selectedId) {
    const grid = this.grid;
    const focus = this.mode === 'focus';
    const order = focus && selectedId ? [selectedId, ...sessions.map((s) => s.id).filter((id) => id !== selectedId)] : sessions.map((s) => s.id);
    order.forEach((id, i) => {
      const c = this.conns.get(id);
      if (!c) return;
      c.cell.style.order = String(i);
      c.cell.classList.toggle('main', focus && id === selectedId);
      c.cell.classList.toggle('selected', id === selectedId);
      if (c.cell.parentNode !== grid) grid.append(c.cell);
    });
    const n = order.length;
    grid.classList.toggle('focus', focus);
    grid.style.setProperty('--mc-n', String(Math.max(1, n)));
    grid.style.gridTemplateRows = focus && n > 1 ? `minmax(0, 3fr) repeat(${n - 1}, minmax(0, 1fr))` : '';
    // 칸 크기가 바뀌면(보기·선택·패인 수) 최신 메시지가 잘리지 않게 바닥으로 맞춘다.
    const sig = `${this.mode}|${order.join(',')}`;
    if (sig !== this.layoutSig) {
      this.layoutSig = sig;
      requestAnimationFrame(() => { for (const c of this.conns.values()) { const f = chatFollower(c.list); f.stuck = true; f.snap(); } });
    }
  }

  onEvent(p) {
    if (!this.on || !p || !p.event || p.event.type !== 'messages' || typeof p.session !== 'string' || !p.session.startsWith('mlx:')) return;
    const c = this.conns.get(p.session.slice(4));
    if (!c || c.key !== p.session) return;
    if (p.event.chatChannelId && p.event.chatChannelId !== c.chatChannelId) return; // 바꾸기 전 채널의 늦은 메시지
    const style = `--lh:${Number(c.hue) || 0}`;
    const tagged = mergedLinesHtml(p.event.messages, { name: c.name, style });
    if (!tagged) return;
    appendTo(this.list, tagged, MAX_LINES);
    appendTo(c.list, mergedLinesHtml(p.event.messages, { name: '', style }), MAX_CELL_LINES);
  }

  system(c, text) {
    const line = `<div class="chat-line system">${esc(text)}</div>`;
    appendTo(this.list, line, MAX_LINES);
    if (c.list) appendTo(c.list, line, MAX_CELL_LINES);
  }
}
