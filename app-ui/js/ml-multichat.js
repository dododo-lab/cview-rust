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
// - 켜 둔 동안 채팅 머리의 후원 목록(donation-list.js)은 모든 패인의 후원·구독을 채널 이름과 함께 모은다
//   (선택 패인 채팅만 보던 예전 띠는 통합 목록 위에서 다른 패인 후원을 빠뜨렸다).
import { api, listen } from './api.js';
import { esc, fitPopover } from './dom.js';
import { mergedItemsHtml, blindMergedLines, applyMergedStyle, mergedChatStyle, setMergedChatStyle } from './multichat.js';
import { MergedLineHost } from './merged-lines.js';
import { mentionInto } from './chat-composer.js';
import { panelOf } from './chat.js';
import { applyChatSkin } from './chat-extras.js';
import { onChatPrefs, passesFilters } from './chat-prefs.js';
import { chatFollower, followAppend, snapAll, trimFront, scheduleChatFlush, flushStarted } from './chat-follow.js';
import { bindMenuKeys, focusFirstMenuItem } from './ml-pane-controls.js';
import { verifiedBadge } from './verified-badge.js';

const STORE_KEY = 'cview.ml.chatView';
const MODES = [
  { id: 'off', label: '선택한 패인 채팅만' },
  { id: 'merged', label: '통합 — 한 목록으로' },
  { id: 'grid', label: '그리드 — 패인별로 나눠 보기' },
  { id: 'focus', label: '포커스 — 선택 패인을 크게' },
];
const TITLES = { off: '채팅', merged: '멀티채팅', grid: '채팅 그리드', focus: '채팅 포커스' };
const MAX_LINES = 350;
const MAX_CELL_LINES = 150;
// 칸·통합 목록을 올려 둔 채 이만큼 손대지 않으면 다시 바닥을 따라간다(작은 칸에는 '새 메시지' 버튼이 없다).
const RESUME_FOLLOW_MS = 8000;
// 끊김·실패를 알린 칸은 이만큼 기다렸다가(엔진이 스스로 다시 붙으면 취소) 다시 연결한다.
const RECONNECT_MS = 4000;
// 보조 창(?w=player|chat|…)에도 멀티라이브 화면이 만들어진다 — 거기서 같은 `mlx:` 세션을 열면 그 창을 닫을 때
// 셸이 '그 창이 연 세션'으로 보고 메인 창 칸들의 연결까지 끊었다. 멀티라이브는 메인 창에서만 쓰므로 보조 창은 열지 않는다.
const AUX_WINDOW = new URLSearchParams(location.search).has('w');
const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H9l-4 3v-3H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z"/><path d="M19 9h1a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-1v3l-4-3h-4"/></svg>';

const sessionKey = (id) => `mlx:${id}`;
const BLIND_TEXT = '관리자에 의해 삭제된 메시지입니다.';

/** 입력창이 켜질 때까지(최대 ms) 기다렸다 부른다 — 다른 패인을 고르면 채팅이 다시 연결되는 동안 입력이 꺼진다. */
function whenEnabled(input, fn, ms = 4000) {
  if (!input || !input.disabled) { fn(); return; }
  const done = () => { mo.disconnect(); clearTimeout(timer); };
  const mo = new MutationObserver(() => { if (!input.disabled) { done(); fn(); } });
  const timer = setTimeout(() => { done(); fn(); }, ms);
  mo.observe(input, { attributes: true, attributeFilter: ['disabled'] });
}

function loadMode() {
  try {
    const v = localStorage.getItem(STORE_KEY);
    if (MODES.some((m) => m.id === v)) return v;
    if (localStorage.getItem('cview.ml.mergedChat') === '1') return 'merged'; // 예전 켬/끔 기억
  } catch { /* 저장 불가 */ }
  return 'off';
}

const MERGED_EMPTY = '<div class="empty">모든 패인의 새 메시지를 기다리는 중…</div>';
const CELL_EMPTY = '<div class="empty">새 메시지를 기다리는 중…</div>';

/** 보기가 바뀌어 다시 보이는 목록을 버퍼로 새로 그린다(앞 줄과 잇지 않는다). 버퍼가 비면 안내를 둔다. */
function refill(list, items, max, empty) {
  if (!list) return;
  if (!items.length) { if (list.childElementCount > 1 || !list.querySelector(':scope > .empty')) list.innerHTML = empty; return; }
  list.innerHTML = '';
  appendTo(list, mergedItemsHtml(items.slice(-max), null), max);
}

/** 칸에 붙이고 넘치면 앞에서 지운다 — 바닥에 있었으면 계속 바닥을 따라간다. */
function appendTo(list, html, max, snaps = null) {
  if (!html) return;
  followAppend(list, (follow) => {
    const empty = list.querySelector(':scope > .empty');
    if (empty) empty.remove();
    list.insertAdjacentHTML('beforeend', html);
    // 올려 읽는 중이면 보던 줄을 제자리에 — WebKit 은 앞줄이 한꺼번에 빠지면 보던 자리가 튀거나 바닥에 닿았다.
    trimFront(list, max, { keepView: !follow });
  }, { resumeMs: RESUME_FOLLOW_MS, snaps });
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
    this.pending = []; // [conn, messages, recent] — 프레임마다 한 번 붙인다
    this.pendingCount = 0;
    // 통합 목록의 경량 링버퍼(칸마다는 c.backlog) — 보기와 관계없이 늘 채우고, 보이는 보기가 바뀌면 이것으로 다시 그린다.
    this.mergedBacklog = [];
    this.shownView = undefined; // 지금 DOM 에 그려 둔 보기 묶음: 'merged' | 'cells' | null(끔)
    this.title = chatEl.querySelector('.chat-header strong');

    const header = chatEl.querySelector('.chat-header');
    const wrap = document.createElement('div');
    wrap.className = 'ml-mc-view';
    wrap.innerHTML = `<button type="button" class="icon-btn chat-hbtn ml-merge-btn" aria-haspopup="menu" aria-expanded="false">${ICON}</button>
      <div class="ml-more-menu ml-mc-menu" role="menu" hidden>${MODES.map((m) => `<button type="button" class="ml-menu-item" role="menuitemradio" data-mc-view="${m.id}">${esc(m.label)}</button>`).join('')}
        <div class="ml-menu-sep" role="separator"></div>
        <button type="button" class="ml-menu-item" role="menuitemcheckbox" data-mc-bubble title="통합·칸 목록을 단일 채팅처럼 프로필 사진과 말풍선으로 — 끄면 한 줄에 촘촘하게">프로필·말풍선으로 보기</button></div>`;
    // 채팅 패널의 도구 서랍('⋯') 맨 앞에 둔다 — 다른 머리 아이콘과 함께 접힌다.
    const tools = header.querySelector('.chat-tools-inner');
    if (tools) tools.prepend(wrap); else header.append(wrap);
    this.button = wrap.querySelector('.ml-merge-btn');
    this.menu = wrap.querySelector('.ml-mc-menu');
    this.bubbleItem = this.menu.querySelector('[data-mc-bubble]');
    // 통합 목록도 채팅 디자인을 따른다. 줄 모양(촘촘·말풍선)은 멀티채팅 통합과 같은 설정 chat.mergedChatStyle.
    const skin = () => {
      applyChatSkin(this.list); applyChatSkin(this.grid);
      applyMergedStyle(this.list); applyMergedStyle(this.grid);
      this.bubbleItem.setAttribute('aria-checked', String(mergedChatStyle() === 'bubble'));
      this.bubbleItem.classList.toggle('active', mergedChatStyle() === 'bubble');
    };
    onChatPrefs(skin);
    skin();
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
      // 채팅 상자(.ml-chat)에도 data-mc-view 가 붙어 있다 — 메뉴 항목만 본다
      const b = e.target.closest('button[data-mc-view]');
      if (b) { this.setMode(b.dataset.mcView); this.openMenu(false); return; }
      if (e.target.closest('[data-mc-bubble]')) { setMergedChatStyle(mergedChatStyle() === 'bubble' ? 'compact' : 'bubble'); this.openMenu(false); }
    });
    // 통합 줄·칸 줄 동작 — 닉네임 말풍선·줄 메뉴·최근 메시지(merged-lines.js). 칸 줄은 칸 묶음(.ml-mc-grid)이 한꺼번에 받는다.
    // 멘션은 그 줄의 패인을 골라(입력은 선택 패인으로 간다) 다시 연결되면 넣는다.
    const lineOpts = {
      resolve: (src) => this.conns.get(String(src).slice(4)) || null,
      input: () => chatEl.querySelector('.chat-input input'),
      isLoggedIn: () => Boolean(panelOf(chatEl)?.isLoggedIn()),
      mention: (nick, src) => {
        const id = String(src).slice(4);
        const input = chatEl.querySelector('.chat-input input');
        if ((this.getState() || {}).selectedId !== id && this.conns.has(id)) this.onSelect(id);
        whenEnabled(input, () => { const p = panelOf(chatEl); if (p) p.mention(nick); else mentionInto(input, nick); });
      },
    };
    this.lines = new MergedLineHost(this.list, lineOpts);
    this.cellLines = new MergedLineHost(this.grid, lineOpts);
    document.addEventListener('pointerdown', (e) => { if (!this.menu.hidden && !e.target.closest('.ml-mc-view')) this.openMenu(false); });
    // 이 Esc 는 메뉴만 닫는다 — 앱 전체 Esc(app.js escapeTaken)는 메뉴가 이미 숨은 뒤에 보므로, 막지 않으면
    // 단독 보기(또는 채널 추가 서랍)까지 함께 닫혔다(패인 ⚙·⋯ 메뉴와 같은 처리).
    document.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || this.menu.hidden) return;
      e.preventDefault(); // escapeTaken·채널 추가 서랍이 defaultPrevented 를 보고 양보한다
      this.openMenu(false);
    });
    window.addEventListener('resize', () => { if (!this.menu.hidden) fitPopover(this.menu, this.chatEl); });
    this.grid.addEventListener('click', (e) => {
      const head = e.target.closest('.ml-mc-cell-head');
      if (head && head.dataset.session) this.onSelect(head.dataset.session);
    });
    listen('chat://session-event', (p) => this.onEvent(p));
    // 선택 패인 채팅(패널) 목록은 켜 둔 동안 숨는다 — 패널 안내(/help·퇴장·연결 오류)는 보이는 목록에도 띄운다.
    const panel = panelOf(chatEl);
    if (panel) panel.onSystem = (text) => { if (this.on) this.system(null, text); };
    this.applyChrome();
  }

  get on() { return this.mode !== 'off'; }

  openMenu(open) {
    const back = !open && this.menu.contains(document.activeElement); // 숨으며 초점이 body 로 떨어지지 않게
    this.menu.hidden = !open;
    this.button.setAttribute('aria-expanded', String(open));
    // 버튼이 도구 서랍 왼쪽 끝이라 오른쪽 정렬 메뉴가 좁은 패널(.ml-chat 은 넘침을 자른다) 왼쪽 밖으로 잘렸다.
    if (open) fitPopover(this.menu, this.chatEl);
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
    // 후원 목록 — 켜 둔 동안은 모든 패인(추가 세션)이 채우고, 항목을 누르면 지금 보기(통합 목록·칸)의 그 줄로 간다.
    // 보조 창은 추가 세션을 열지 않으므로(AUX_WINDOW) 선택 패인 채팅으로 둔다.
    const feed = this.on && !AUX_WINDOW;
    panelOf(this.chatEl)?.donations?.setExternal(feed ? 'ml' : null, feed ? {
      findLine: (entry) => this.lineBox().querySelector(`.chat-line[data-id="${CSS.escape(entry.m.id)}"][data-src="${CSS.escape(entry.src ? entry.src.key : '')}"]`),
      presentKeys: () => new Set([...this.lineBox().querySelectorAll('.chat-line.ev[data-id][data-src]')].map((l) => `${l.dataset.src}|${l.dataset.id}`)),
    } : {});
    // 보기를 바꾸면 열린 닉네임 말풍선·이 사람만 보기를 걷는다(감춘 목록의 띠가 남지 않게)
    this.lines?.reset();
    this.cellLines?.reset();
    this.panelList.hidden = this.on;
    this.list.hidden = this.mode !== 'merged';
    this.grid.hidden = this.mode !== 'grid' && this.mode !== 'focus';
    // 보이지 않는 보기의 DOM 은 비워 노드 누적을 막는다. 두 보기의 경량 버퍼(mergedBacklog·c.backlog)는 flush 가
    // 늘 채우므로, 보이는 보기가 바뀔 때만 그 버퍼로 목록을 새로 그린다(지난 메시지가 남는다 — 그리드↔포커스는 그대로).
    const shown = this.mode === 'merged' ? 'merged' : (this.mode === 'grid' || this.mode === 'focus') ? 'cells' : null;
    if (shown !== this.shownView) {
      this.shownView = shown;
      if (shown !== 'merged' && this.list.childElementCount > 1) this.list.innerHTML = MERGED_EMPTY;
      for (const c of this.conns.values()) if (shown !== 'cells' && c.list && c.list.childElementCount > 1) c.list.innerHTML = CELL_EMPTY;
      if (shown === 'merged') {
        refill(this.list, this.mergedBacklog, MAX_LINES, MERGED_EMPTY);
        this.lines?.afterAppend();
      } else if (shown === 'cells') {
        for (const c of this.conns.values()) refill(c.list, c.backlog || [], MAX_CELL_LINES, CELL_EMPTY);
        this.cellLines?.afterAppend();
      } else {
        this.mergedBacklog = [];
        for (const c of this.conns.values()) c.backlog = [];
      }
    }
    if (this.title) this.title.textContent = TITLES[this.mode];
  }

  /**
   * 사용자가 채팅을 지웠다(⋯ 지우기·⇧⌘K·/clear) — 통합·칸 보기의 줄과 버퍼도 지운다. 예전엔 숨은 선택 패인 목록만
   * 지워져 보이는 통합 목록·칸은 그대로였다. 끔(off)이면 할 일이 없다.
   */
  clearByUser() {
    if (!this.on) return;
    this.pending = [];
    this.pendingCount = 0;
    this.mergedBacklog = [];
    this.lines?.reset();
    this.cellLines?.reset();
    if (this.mode === 'merged') this.list.innerHTML = '<div class="empty">채팅을 지웠습니다</div>';
    for (const c of this.conns.values()) {
      c.backlog = [];
      if (c.list && (this.mode === 'grid' || this.mode === 'focus')) c.list.innerHTML = '<div class="empty">채팅을 지웠습니다</div>';
    }
  }

  /** 지금 보기에서 줄이 보이는 상자 — 통합이면 통합 목록, 그리드·포커스면 칸 묶음. */
  lineBox() {
    return this.mode === 'merged' ? this.list : this.grid;
  }

  /** 패인 구성에 맞춰 추가 세션을 열고 닫고, 칸을 배치한다(render 마다 불린다 — 바뀐 것만 건드린다). */
  sync() {
    const state = this.getState() || {};
    // 멀티라이브 상한을 넘어 숨긴 채널(ml-view-picker.js)은 함께 보는 채팅에서도 뺀다.
    const all = (state.sessions || []).filter((s) => s.chatChannelId && s.inMultiView !== false);
    const sessions = this.on && !AUX_WINDOW ? all : [];
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
        this.connect(s.id, c);
      }
      c.name = name;
      if (hue != null) c.hue = hue;
      c.cell.style.setProperty('--lh', String(Number(c.hue) || 0));
      const label = c.cell.querySelector('.ml-mc-cell-head span');
      if (label.textContent !== name) label.textContent = name;
      const badge = s.verifiedBadge || '';
      if (c.badge !== badge) {
        c.badge = badge;
        label.nextElementSibling?.classList.contains('vbadge') && label.nextElementSibling.remove();
        label.insertAdjacentHTML('afterend', verifiedBadge(badge, 'sm'));
      }
    }
    for (const [id, c] of this.conns) if (!keep.has(id)) this.drop(id, c);
    if (!sessions.length) { this.lines.reset(); this.cellLines.reset(); }
    this.layoutCells(sessions, state.selectedId);
  }

  connect(id, c) {
    clearTimeout(c.retry);
    c.retry = null;
    c.connectAt = performance.now();
    api.chatConnect(c.chatChannelId, c.channelId, c.key).catch((e) => {
      if (this.conns.get(id) !== c) return;
      this.system(c, `${c.name} 채팅에 연결하지 못했어요 — ${e.message}`);
      this.scheduleReconnect(id, c);
    });
  }

  /** 끊긴 칸을 잠시 뒤 다시 연결한다 — 그사이 엔진이 다시 붙거나 메시지가 오면 취소된다. */
  scheduleReconnect(id, c) {
    if (c.retry) return;
    c.retry = setTimeout(() => {
      c.retry = null;
      if (this.conns.get(id) !== c) return;
      c.skipRecent = true; // 다시 붙으면 오는 최근 50줄은 이미 칸에 있다
      this.connect(id, c);
    }, RECONNECT_MS);
  }

  drop(id, c) {
    this.conns.delete(id);
    clearTimeout(c.retry);
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
    // 포커스 — 메인 칸 160px·나머지 90px 아래로는 줄이지 않고 넘치면 격자가 스크롤한다(ml-mc-grid)
    grid.style.gridTemplateRows = focus && n > 1 ? `minmax(160px, 3fr) repeat(${n - 1}, minmax(90px, 1fr))` : '';
    // 칸 크기가 바뀌면(보기·선택·패인 수) 최신 메시지가 잘리지 않게 바닥으로 맞춘다.
    const sig = `${this.mode}|${order.join(',')}`;
    if (sig !== this.layoutSig) {
      this.layoutSig = sig;
      requestAnimationFrame(() => { for (const c of this.conns.values()) { const f = chatFollower(c.list); f.stuck = true; f.snap(); } });
    }
  }

  onEvent(p) {
    if (!this.on || !p || !p.event || typeof p.session !== 'string' || !p.session.startsWith('mlx:')) return;
    const c = this.conns.get(p.session.slice(4));
    if (!c || c.key !== p.session) return;
    const ev = p.event;
    if (ev.chatChannelId && ev.chatChannelId !== c.chatChannelId) return; // 바꾸기 전 채널의 늦은 메시지
    // 연결 상태 — 선택 패인이 아닌 칸은 상태 표시가 없어, 끊긴 채 남으면 칸이 조용히 멈춰 보였다.
    const state = ev.type === 'state' ? ev.state : ev.type;
    // 다시 연결하며 떼어 낸 이전 엔진의 '끊김'이 늦게 올 수 있다 — 연결 직후 것은 무시한다(되풀이 재연결 방지).
    if (state === 'disconnected' || state === 'failed') {
      if (performance.now() - (c.connectAt || 0) > RECONNECT_MS) this.scheduleReconnect(p.session.slice(4), c);
      return;
    }
    if (state === 'connected' || state === 'connecting' || state === 'reconnecting') { clearTimeout(c.retry); c.retry = null; return; }
    // 관리자 삭제 — 통합 줄과 칸 줄 모두(예전엔 받지 않아 지워진 메시지가 남았다). 아직 붙이지 않은 것도.
    if (ev.type === 'blinded') {
      blindMergedLines([this.list, c.list], c.key, ev.messageId, ev.blindText);
      const maskItem = (it) => {
        if (it.src === c.key && it.m && it.m.id === ev.messageId) {
          it.m = { ...it.m, isBlinded: true, content: ev.blindText || BLIND_TEXT, emojis: {} };
        }
      };
      for (const it of this.mergedBacklog) maskItem(it);
      if (c.backlog) for (const it of c.backlog) maskItem(it);
      for (const [pc, msgs] of this.pending) {
        if (pc !== c) continue;
        const i = msgs.findIndex((m) => m.id === ev.messageId);
        if (i >= 0) msgs[i] = { ...msgs[i], isBlinded: true, content: ev.blindText || BLIND_TEXT, emojis: {} };
      }
      return;
    }
    if (ev.type !== 'messages') return;
    clearTimeout(c.retry);
    c.retry = null;
    if (ev.recent && c.skipRecent) { c.skipRecent = false; return; }
    c.skipRecent = false;
    if (!ev.messages || !ev.messages.length) return;
    // 모았다가 프레임마다 한 번 붙인다 — 예전엔 패인마다 이벤트가 올 때마다 두 목록에 붙이고 바닥을 맞췄다(패인 4개면 프레임에 8번).
    this.pending.push([c, ev.messages.slice(), Boolean(ev.recent)]);
    this.pendingCount += ev.messages.length;
    while (this.pending.length > 1 && this.pendingCount - this.pending[0][1].length >= MAX_LINES) this.pendingCount -= this.pending.shift()[1].length;
    scheduleChatFlush(this, () => this.flush(), { pending: this.pendingCount });
  }

  /** 모아 둔 메시지를 통합 목록과 칸에 붙인다 — 단일 채팅과 같은 줄 렌더러(묶음·아바타·이모티콘 단독 확대·줄 메뉴). */
  flush() {
    flushStarted(this);
    const batch = this.pending;
    this.pending = [];
    this.pendingCount = 0;
    const items = [];
    const cells = new Map();
    const events = [];
    // /mute(선택 패인 채팅 패널의 localHidden) — 통합·칸 목록도 같은 기준으로 거른다(버퍼에도 남겨 다시 그릴 때도).
    const hide = panelOf(this.chatEl)?.localHidden || null;
    for (const [c, msgs, recent] of batch) {
      if (!this.conns.has(c.key.slice(4)) || this.conns.get(c.key.slice(4)) !== c) continue; // 그사이 닫힌 패인
      const style = `--lh:${Number(c.hue) || 0}`;
      for (const m of msgs) items.push({ m, name: c.name, style, src: c.key, hide });
      if (!cells.has(c)) cells.set(c, []);
      for (const m of msgs) cells.get(c).push({ m, style, src: c.key, hide });
      events.push([c, msgs, recent]);
    }
    if (!items.length) return;
    // 두 보기의 경량 버퍼는 늘 채우고(보기를 바꾸면 이것으로 다시 그린다), DOM 에는 지금 보이는 보기만 붙인다
    // (DOM 노드·강제 레이아웃 절반 절감). 예전엔 보이는 보기의 줄을 버퍼에 남기지 않아 통합→그리드→통합이면 지난 줄이 사라졌다.
    this.mergedBacklog.push(...items);
    if (this.mergedBacklog.length > MAX_LINES) this.mergedBacklog.splice(0, this.mergedBacklog.length - MAX_LINES);
    for (const [c, cellItems] of cells) {
      if (!c.backlog) c.backlog = [];
      c.backlog.push(...cellItems);
      if (c.backlog.length > MAX_CELL_LINES) c.backlog.splice(0, c.backlog.length - MAX_CELL_LINES);
    }
    if (this.mode === 'merged') {
      appendTo(this.list, mergedItemsHtml(items, this.list.lastElementChild), MAX_LINES);
      this.lines.afterAppend();
    } else if (this.mode === 'grid' || this.mode === 'focus') {
      // 칸마다 붙이기(쓰기)를 먼저 다 하고 바닥 맞추기(읽기)는 뒤에 한 번에 — 칸 수만큼 강제 레이아웃이 일어나지 않게.
      const snaps = [];
      for (const [c, cellItems] of cells) appendTo(c.list, mergedItemsHtml(cellItems, c.list.lastElementChild), MAX_CELL_LINES, snaps);
      snapAll(snaps);
      this.cellLines.afterAppend();
    }
    // 후원 목록 — 줄을 붙인 뒤에 넣어야 목록이 '채팅에 있는 줄'로 그린다. 채팅 설정(차단·후원 숨김)을 줄과 같이 따른다.
    const donations = panelOf(this.chatEl)?.donations;
    if (donations && donations.external === 'ml') {
      for (const [c, msgs, recent] of events) {
        // /mute 로 숨긴 사람의 후원도 뺀다 — 예전엔 줄은 숨는데 후원 띠·목록에는 계속 떴다(단일 채팅 chat.js 와 같은 기준).
        const shown = msgs.filter((m) => (m.kind === 'donation' || m.kind === 'subscription') && passesFilters(m) && !(hide && hide(m)));
        if (shown.length) donations.add(shown, { fresh: recent ? new Set() : null, src: { key: c.key, name: c.name, hue: c.hue } });
      }
    }
  }

  /** @param {object|null} c 칸 — 없으면(패널 안내) 선택 패인 칸 */
  system(c, text) {
    if (this.pending.length) this.flush(); // 앞서 온 메시지 뒤에
    const line = `<div class="chat-line system">${esc(text)}</div>`;
    const cell = c || this.conns.get((this.getState() || {}).selectedId) || null;
    if (this.mode === 'merged') appendTo(this.list, line, MAX_LINES);
    else if ((this.mode === 'grid' || this.mode === 'focus') && cell?.list) appendTo(cell.list, line, MAX_CELL_LINES);
  }
}
