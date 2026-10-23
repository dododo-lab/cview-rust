// 멀티라이브 — 여러 방송을 한 화면에서 (원본 `MultiLiveView`·`MultiLiveManager`).
//
// - 세션·선택·오디오·화질 상한은 Rust `MultiLiveManager` 가 정하고 `multilive://state` 로 알려 준다.
// - 패인마다 플레이어 iframe 이 하나다. iframe 은 엔진 슬롯 번호로 묶여 레이아웃이 바뀌어도
//   옮기지 않는다(옮기면 다시 로드된다) — 위치는 CSS grid 의 `order` 로만 바꾼다.
// - 슬롯 명령(`multilive://command`)은 패인 iframe 이 생기기 전에 올 수 있어 슬롯별로 줄 세운다.
// - 패인을 누르면 선택 — 오디오·채팅·고화질이 그 패인을 따라간다.
import { api, listen, listenHere, Events } from './api.js';
import { setPicture, setSharpenDefault } from './picture.js';
import { esc, safeUrl, toast, imageSrc, streamEndedArt, dismiss } from './dom.js';
import { PlayerBridge } from './player.js';
import { ChatPanel } from './chat.js';
import { StreamAlerts } from './donations.js';
import { PaneControls, paneControlsHtml, PANE_ICON } from './ml-pane-controls.js';
import { installChatSplitter } from './splitter.js';
import { FOCUS_DIRECTIONS, FOCUS_RATIO, focusGrid, ratioFromPointer, clampFocusRatio } from './focus-layout.js';
import { kbd } from './platform.js';
import { uptimeText } from './cards.js';
import { MultiLiveAddDrawer } from './ml-add-drawer.js';
import { snapshotPanes, playLayoutMotion, playExit, cancelExit, bindPaneDrag, releaseDragged, motionEnabled } from './ml-motion.js';
import { MultiLiveMergedChat } from './ml-multichat.js';

import { multiLiveIcon, liveModeSwitch, setLiveModeActive } from './live-mode.js';

export { multiLiveIcon };

const ICON_CLOSE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>';
const ICON_SOUND = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h4l5-4v14l-5-4H4V9Z"/><path d="M16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';
const ICON_PLUS = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';

const STAT_ICON = {
  viewers: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/></svg>',
  uptime: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>',
};

const DIRECTION_ICONS = {
  left: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="4" width="11" height="16" rx="2"/><rect x="16" y="4" width="5" height="7" rx="1.5" opacity=".55"/><rect x="16" y="13" width="5" height="7" rx="1.5" opacity=".55"/></svg>',
  right: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="10" y="4" width="11" height="16" rx="2"/><rect x="3" y="4" width="5" height="7" rx="1.5" opacity=".55"/><rect x="3" y="13" width="5" height="7" rx="1.5" opacity=".55"/></svg>',
  top: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="3" width="18" height="11" rx="2"/><rect x="3" y="16" width="8" height="5" rx="1.5" opacity=".55"/><rect x="13" y="16" width="8" height="5" rx="1.5" opacity=".55"/></svg>',
  bottom: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="10" width="18" height="11" rx="2"/><rect x="3" y="3" width="8" height="5" rx="1.5" opacity=".55"/><rect x="13" y="3" width="8" height="5" rx="1.5" opacity=".55"/></svg>',
};

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

/** 패인 테두리 색 — 세션마다 무작위 색조, 이미 쓰는 색조와는 되도록 멀리. 순수 함수(rand 주입). */
export function pickTintHue(used, rand = Math.random) {
  let best = Math.floor(rand() * 360);
  let bestGap = -1;
  for (let i = 0; i < 12; i++) {
    const h = Math.floor(rand() * 360);
    const gap = used.length ? Math.min(...used.map((u) => Math.min(Math.abs(h - u), 360 - Math.abs(h - u)))) : 360;
    if (gap > bestGap) { best = h; bestGap = gap; }
  }
  return best;
}

export class MultiLivePage {
  constructor(content, { isLoggedIn }) {
    this.isLoggedIn = isLoggedIn;
    this.root = document.createElement('section');
    this.root.className = 'page page-multilive';
    this.root.dataset.page = 'multilive';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header ml-header" data-tauri-drag-region>
        <h1 class="page-title">멀티라이브</h1>
        ${liveModeSwitch('multilive')}
        <span class="page-subtitle ml-count"></span>
        <div class="ml-watch-picker" role="group" aria-label="라이브로 볼 채널" hidden></div>
        <div class="grow"></div>
        <div class="ml-toolbar">
          <div class="ml-focus-controls" hidden>
            <div class="segmented ml-tb-seg" role="group" aria-label="포커스 방향">
              ${FOCUS_DIRECTIONS.map((d) => `<button class="seg icon" data-direction="${d.id}" title="메인 위치: ${d.label}" aria-label="메인 위치 ${d.label}">${DIRECTION_ICONS[d.id]}</button>`).join('')}
            </div>
            <label class="ml-ratio" title="메인 패인 크기">
              <input type="range" min="${FOCUS_RATIO.min * 100}" max="${FOCUS_RATIO.max * 100}" step="1" aria-label="메인 패인 크기">
              <output></output>
            </label>
            <span class="ml-tb-sep" aria-hidden="true"></span>
          </div>
          <div class="segmented ml-tb-seg" role="group" aria-label="레이아웃">
            <button class="seg" data-layout="grid" title="${kbd('균등 분할 (⌃⌘1)')}">그리드</button>
            <button class="seg" data-layout="focus" title="${kbd('포커스 — 한 방송을 크게 (⌃⌘2)')}">포커스</button>
            <button class="seg" data-layout="custom" title="${kbd('사용자 배치 — 경계를 끌어 크기 조절 (⌃⌘3)')}">사용자</button>
          </div>
          <span class="ml-tb-sep" aria-hidden="true"></span>
          <button class="ml-tb-icon ml-multiaudio" data-ml="multi-audio" title="여러 소리 — 선택하지 않은 패인 소리도 함께 듣기" aria-label="여러 소리" aria-pressed="false">${ICON_SOUND}</button>
          <div class="ml-more">
            <button class="ml-tb-icon" data-ml-more title="더 보기" aria-label="더 보기" aria-haspopup="menu" aria-expanded="false"><svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5.5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/></svg></button>
            <div class="ml-more-menu" role="menu" hidden>
              <button class="ml-menu-item" role="menuitem" data-ml="presets"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4h12v16l-6-4-6 4z"/></svg>프리셋 저장·불러오기</button>
              <button class="ml-menu-item" role="menuitem" data-ml="retry-all"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11.5a8 8 0 1 0-2.4 5.9"/><path d="M20 4.5v7h-7"/></svg>모두 다시 연결</button>
              <button class="ml-menu-item" role="menuitem" data-ml="alert-pos"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3"/></svg>후원 알림 위치 조정</button>
              <div class="ml-menu-sep"></div>
              <button class="ml-menu-item danger" role="menuitem" data-ml="stop-all"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M6 6l12 12M18 6 6 18"/></svg>모두 닫기</button>
            </div>
          </div>
          <button class="btn primary ml-add" data-ml="add" title="채널 추가">${ICON_PLUS}<span>채널 추가</span></button>
        </div>
      </header>
      <div class="page-body ml-body">
        <div class="ml-stage">
          <div class="ml-empty">
            <strong>여러 방송을 한 화면에서 보세요</strong>
            <span>채널을 추가하면 최대 4개까지 나란히 재생합니다. 패인을 누르면 소리와 채팅이 그 방송으로 바뀝니다.</span>
            <button class="btn primary lg" data-ml="add">${ICON_PLUS}<span>채널 추가</span></button>
            <button class="btn lg ml-restore" data-ml="restore" hidden>지난 구성 다시 열기</button>
          </div>
          <button class="ml-focus-add" data-ml="add" hidden title="채널 추가" aria-label="채널 추가">${ICON_PLUS}<span>채널 추가</span></button>
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
      </div>`;
    content.append(this.root);

    this.stage = this.root.querySelector('.ml-stage');
    this.empty = this.root.querySelector('.ml-empty');
    this.focusAdd = this.root.querySelector('.ml-focus-add');
    this.count = this.root.querySelector('.ml-count');
    this.watchPicker = this.root.querySelector('.ml-watch-picker');
    this.focusControls = this.root.querySelector('.ml-focus-controls');
    this.ratioInput = this.root.querySelector('.ml-ratio input');
    this.ratioOutput = this.root.querySelector('.ml-ratio output');
    this.divider = document.createElement('div');
    this.divider.className = 'ml-divider';
    this.divider.title = '끌어서 메인 크기 조절';
    this.divider.hidden = true;
    this.stage.append(this.divider);
    this.localRatio = null; // 드래그 중 미리보기 비율
    this.bindFocusControls();
    installChatSplitter({ container: this.root.querySelector('.ml-body'), chat: this.root.querySelector('.ml-chat'), key: 'multilive' });
    this.alerts = new StreamAlerts(this.stage);
    this.single = null; // 한 채널 크게(MUL-11) — 슬롯
    this.gridDividers = this.buildGridDividers();
    this.stage.addEventListener('dblclick', (e) => {
      const pane = e.target.closest('.ml-pane');
      if (pane && this.watchSlot == null && !e.target.closest('.ml-pane-bar, .ml-pane-top, .ml-pane-menu')) this.toggleSingle(Number(pane.dataset.slot));
    });
    document.addEventListener('keydown', (e) => {
      if (this.root.hidden || !(e.metaKey && e.ctrlKey)) return;
      const map = { '1': 'grid', '2': 'focus', '3': 'custom' };
      if (map[e.key]) { e.preventDefault(); this.run(api.multiLiveSetLayout(map[e.key])); }
    });
    this.proposed = new Set();
    setInterval(() => this.proposeTick(), 120000);
    // 패인 재생 상태(일시정지·버퍼링·지연) — 화면이 보일 때만 맞춘다.
    setInterval(() => {
      if (this.root.hidden || document.hidden) return;
      const now = Date.now();
      for (const p of this.panes.values()) {
        if (!p.sessionId) continue;
        const s = (this.state.sessions || []).find((x) => x.id === p.sessionId);
        p.controls.sync(Boolean(s && s.isAudible));
        this.watchFirstFrame(p, s, now);
      }
    }, 700);
    // 방송 경과 시간은 분 단위라 30초마다 칩만 다시 그린다.
    setInterval(() => {
      if (this.root.hidden || document.hidden) return;
      for (const p of this.panes.values()) {
        const s = p.sessionId && (this.state.sessions || []).find((x) => x.id === p.sessionId);
        if (s) this.renderPaneStats(p.el, s);
      }
    }, 30000);
    // 멀티라이브 채팅은 자기 세션(`ml`)을 쓴다 — 라이브 화면과 기본 세션을 나눠 쓰면 라이브를 열거나 닫을 때
    // (단독 보기로 넘어가며 viewer.close 가 끊기, 다른 채널 시청이 기본 세션을 가져가기) 이 채팅이 끊긴 채 남았다.
    this.chat = new ChatPanel(this.root.querySelector('.ml-chat'), { isLoggedIn, session: 'ml', mentionNotify: true, onHighlight: (m) => this.alerts.push(m) });
    // 멀티채팅 — 모든 패인 채팅을 통합·그리드·포커스로(보내기는 선택 패인으로).
    this.mergedChat = new MultiLiveMergedChat(this.root.querySelector('.ml-chat'), {
      getState: () => this.state,
      getPanes: () => this.panes,
      onSelect: (id) => { if (id !== this.state.selectedId) this.run(api.multiLiveSelect(id)); },
    });

    this.state = { sessions: [], selectedId: null, canAdd: true, maxSessions: 4, layout: 'grid' };
    this.panes = new Map(); // slot → { el, bridge, sessionId }
    this.motionSig = null; // 마지막으로 그린 배치(패인 구성·순서·레이아웃) — 바뀌면 FLIP 으로 움직인다
    this.dragRelease = null; // 방금 끌어 놓은 패인
    // 끌어서 자리 바꾸기·메인 바꾸기(MUL-07) — 포인터로 직접 끈다(ml-motion.js).
    bindPaneDrag(this, (dragged, target) => this.run(api.multiLiveReorder(dragged, target)));
    this.pending = new Map(); // slot → [script]
    this.chatKey = null;
    this.addBar = new MultiLiveAddDrawer(this.root.querySelector('.ml-body'), {
      isLoggedIn,
      getState: () => this.state,
      onAdd: (channelId) => this.run(api.multiLiveAdd(channelId)),
    });

    listenHere(Events.multiLiveCommand, (p) => p && this.dispatch(p.slot, p.script));
    listen(Events.multiLiveState, (s) => this.render(s));

    this.root.addEventListener('click', (e) => this.onClick(e));
    // "⋯" 더 보기 메뉴 — 프리셋·모두 다시 연결·모두 닫기
    const moreBtn = this.root.querySelector('[data-ml-more]');
    const moreMenu = this.root.querySelector('.ml-more-menu');
    const closeMore = () => { moreMenu.hidden = true; moreBtn.setAttribute('aria-expanded', 'false'); };
    moreBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      moreMenu.hidden = !moreMenu.hidden;
      moreBtn.setAttribute('aria-expanded', String(!moreMenu.hidden));
    });
    moreMenu.addEventListener('click', () => closeMore());
    document.addEventListener('pointerdown', (e) => { if (!moreMenu.hidden && !e.target.closest('.ml-more')) closeMore(); });
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !moreMenu.hidden) closeMore(); });
    new ResizeObserver(() => this.reportStageSize()).observe(this.stage);
  }

  async show() {
    try {
      this.render(await api.multiLiveState());
    } catch (e) {
      toast(e.message);
    }
    try {
      const ml = (await api.settingsGet()).multiLive || {};
      this.root.querySelector('.ml-body').classList.toggle('chat-overlay', Boolean(ml.chatOverlayInGrid));
      this.root.style.setProperty('--ml-chat-opacity', String(ml.chatOverlayOpacity ?? 0.5));
      const font = Number(ml.chatOverlayFontSize) || 12;
      this.root.style.setProperty('--ml-chat-font', `${font}px`);
      this.root.style.setProperty('--ml-chat-scale', String(font / 14));
    } catch { /* 앱 밖 */ }
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
      <div class="ml-pane-state"><div class="spinner"></div>${streamEndedArt}<span></span></div>
      <div class="ml-drop-hint" aria-hidden="true"></div>
      <div class="ml-pane-top">
        <button class="ml-pane-btn ml-drag" title="끌어서 자리 바꾸기 — 메인 패인에 놓으면 메인이 됩니다" aria-label="끌어서 자리 바꾸기"><svg viewBox="0 0 24 24" fill="currentColor"><circle cx="9" cy="6" r="1.6"/><circle cx="15" cy="6" r="1.6"/><circle cx="9" cy="12" r="1.6"/><circle cx="15" cy="12" r="1.6"/><circle cx="9" cy="18" r="1.6"/><circle cx="15" cy="18" r="1.6"/></svg></button>
        <div class="ml-pane-avatar" data-ml="info" title="채널 정보"></div>
        <div class="ml-pane-text">
          <div class="ml-pane-title ellipsis"></div>
          <div class="ml-pane-line"><span class="ml-pane-name ellipsis"></span></div>
          <div class="ml-pane-stats"></div>
        </div>
        <div class="grow"></div>
        <button class="ml-pane-btn ml-sec ml-watch-btn" data-ml="watch" title="라이브로 크게 보기 — 다시 연결 없이 라이브 메뉴로" aria-label="라이브로 크게 보기">${PANE_ICON.live}</button>
        <button class="ml-pane-btn ml-sec" data-ml="select" title="이 패인 채팅 보기" aria-label="이 패인 채팅 보기">${PANE_ICON.chat}</button>
        <button class="ml-pane-btn ml-sec" data-ml="info" title="채널 정보" aria-label="채널 정보">${PANE_ICON.info}</button>
        <button class="ml-pane-btn ml-sec" data-ml="retry" title="다시 연결" aria-label="다시 연결">${PANE_ICON.retry}</button>
        <button class="ml-pane-btn" data-mp="more" title="더 보기" aria-label="더 보기" aria-haspopup="menu">${PANE_ICON.more}</button>
        <button class="ml-pane-close" data-ml="remove" title="닫기">${ICON_CLOSE}</button>
      </div>
      <span class="ml-pane-sound" title="소리 나는 패인">${ICON_SOUND}</span>${paneControlsHtml()}`;
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
    pane.controls = new PaneControls(pane, {
      makeAudible: () => this.makeAudible(pane),
      applySharpenAll: (on) => this.applySharpenAll(on),
    });
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
    // 늦게 도착한 낡은 상태는 무시한다 — 그대로 그리면 방금 재사용된 슬롯을 '닫힌 패인'으로 보고 멈춘다.
    if (this.state && typeof state.rev === 'number' && typeof this.state.rev === 'number' && state.rev < this.state.rev) return;
    // 배치가 바뀌면(추가·제거·순서·레이아웃·메인) 바꾸기 전 자리를 재 둔다 — 끝에서 새 자리로 미끄러진다.
    const sig = this.motionSignature(state);
    const visible = !this.root.hidden && !document.hidden && motionEnabled();
    const before = visible && (this.dragRelease || (this.motionSig != null && sig !== this.motionSig)) ? snapshotPanes(this.panes) : null;
    this.motionSig = sig;
    if (this.dragRelease) { releaseDragged(this.dragRelease); this.dragRelease = null; }
    this.state = state;
    const sessions = state.sessions || [];
    const n = sessions.length;
    this.count.textContent = `${n} / ${state.maxSessions}`;
    for (const b of this.root.querySelectorAll('[data-layout]')) {
      b.classList.toggle('active', b.dataset.layout === state.layout);
    }
    for (const b of this.root.querySelectorAll('[data-ml="add"]')) {
      b.disabled = !state.canAdd;
      if (b.classList.contains('ml-add')) b.title = state.canAdd ? '채널 추가' : `최대 ${state.maxSessions}개까지 볼 수 있어요 — 패인을 닫으면 추가할 수 있습니다`;
    }
    this.root.querySelector('[data-ml="stop-all"]').hidden = n === 0;
    this.root.querySelector('[data-ml="retry-all"]').hidden = n === 0;
    this.root.querySelector('[data-ml="alert-pos"]').hidden = n === 0;
    const ma = this.root.querySelector('[data-ml="multi-audio"]');
    ma.classList.toggle('active', Boolean(state.multiAudio));
    ma.setAttribute('aria-pressed', String(Boolean(state.multiAudio)));
    // 지난 구성 기억(MUL-15) — 세션이 있으면 저장, 비면 다시 열기 버튼
    const savedIds = this.savedSessionIds();
    if (n > 0) this.saveSessionIds(sessions.map((x) => x.channelId));
    const restore = this.root.querySelector('[data-ml="restore"]');
    restore.hidden = n > 0 || !savedIds.length;
    restore.textContent = `지난 구성 다시 열기 (${savedIds.length}개)`;
    if (this.single != null && !sessions.some((x) => x.slot === this.single)) this.single = null;
    // 라이브 메뉴 단독 보기(원본 시청 모드) — 같은 세션을 무대에 올려 다시 연결하지 않는다.
    const watched = state.watchId ? sessions.find((x) => x.id === state.watchId) : null;
    this.watchSlot = watched && watched.slot != null ? watched.slot : null;
    this.root.classList.toggle('as-watch', this.watchSlot != null);
    // 단독 무대일 땐 라이브, 아니면 멀티라이브 — 머리줄 제목과 전환 버튼이 지금 보기를 따른다.
    const mode = this.watchSlot != null ? 'live' : 'multilive';
    this.root.querySelector('.ml-header .page-title').textContent = mode === 'live' ? '라이브' : '멀티라이브';
    setLiveModeActive(this.root.querySelector('.ml-header'), mode);
    this.renderWatchPicker(sessions, watched);
    const single = this.watchSlot ?? this.single;

    const focus = state.layout === 'focus' && n > 1;
    this.stage.className = `ml-stage ml-n${n <= 4 ? n : (n <= 9 ? 'many' : 'lots')} ${focus ? `ml-focus dir-${state.focusDirection}` : ''} ${state.layout === 'custom' && n > 1 && n <= 4 ? 'ml-custom' : ''} ${single != null ? 'ml-single' : ''}`;
    this.empty.hidden = n > 0;
    this.focusControls.hidden = state.layout !== 'focus';
    for (const b of this.root.querySelectorAll('[data-direction]')) {
      b.classList.toggle('active', b.dataset.direction === state.focusDirection);
    }
    if (this.localRatio == null) {
      this.ratioInput.value = String(Math.round((state.focusRatio || 0.7) * 100));
      this.ratioOutput.textContent = `${this.ratioInput.value}%`; this.syncRatioFill();
    }

    const order = paneOrder(sessions, state.focusMainId || state.selectedId, state.layout);
    const live = new Set();
    for (const s of sessions) {
      if (s.slot == null) continue;
      live.add(s.slot);
      const pane = this.ensurePane(s.slot);
      cancelExit(pane); // 사라지는 중에 같은 슬롯이 다시 쓰이면 되돌린다
      if (pane.sessionId !== s.id) {
        const used = [...this.panes.values()].filter((p) => p !== pane && p.sessionId && p.hue != null).map((p) => p.hue);
        pane.hue = pickTintHue(used);
        pane.el.style.setProperty('--ml-hue', String(pane.hue));
      }
      pane.sessionId = s.id;
      pane.controls?.setChannel(s.channelId);
      this.renderPane(pane, s, order.indexOf(s.id), s.id === state.selectedId);
    }
    this.applyFocusGrid(focus, order);
    this.applyCustomGrid(state.layout === 'custom' && n > 1 && n <= 4 && single == null, n);
    for (const [slot, pane] of this.panes) pane.el.classList.toggle('ml-single-target', single === slot);
    for (const [slot, pane] of this.panes) {
      if (live.has(slot) || pane.exiting) continue;
      // 닫힌 패인 — 숨기기만 하면 iframe 이 MediaSource 버퍼를 계속 쥔다(모두 닫은 뒤 RSS 3GB대).
      const release = () => {
        pane.bridge.run('try { stop(); video.removeAttribute("src"); video.load(); } catch (e) {}');
        pane.el.hidden = true;
      };
      const rect = before && !pane.el.hidden ? before.get(slot) : null;
      const had = pane.sessionId || !pane.el.hidden;
      pane.sessionId = null;
      if (!had) { pane.el.hidden = true; continue; }
      if (rect) {
        // 마지막 자리에서 줄어들며 사라진 뒤 멈춘다 — 그동안 소리는 바로 끈다.
        pane.bridge.run('try { video.muted = true; } catch (e) {}');
        playExit(pane, this.stage, rect, release);
      } else {
        release();
      }
    }
    if (before) playLayoutMotion(this.panes, before);
    this.syncChat(sessions.find((s) => s.id === state.selectedId) || null);
    // 채팅 테두리는 채팅을 보여 주는(선택) 패인의 색을 따른다.
    const shown = [...this.panes.values()].find((p) => p.sessionId && p.sessionId === state.selectedId);
    const chatEl = this.root.querySelector('.ml-chat');
    chatEl.classList.toggle('tinted', Boolean(shown));
    if (shown) chatEl.style.setProperty('--ml-hue', String(shown.hue));
    // 후원 알림은 그 채팅의 패인(포커스·한 채널 크게면 메인) 위에 — 패인이 없으면 무대 전체.
    this.alerts.attach(shown ? shown.el : this.stage);
    this.addBar.sync();
    this.mergedChat.sync();
  }

  /** 패인 배치의 지문 — 이게 바뀔 때만 움직임을 준다(시청자 수 같은 잦은 상태 갱신은 그대로). */
  motionSignature(state) {
    const sessions = state.sessions || [];
    const order = paneOrder(sessions, state.focusMainId || state.selectedId, state.layout);
    const slotOf = new Map(sessions.map((x) => [x.id, x.slot]));
    return [state.layout, state.focusDirection, state.watchId || '', this.single ?? '', Boolean(state.canAdd), order.map((id) => slotOf.get(id)).join(',')].join('|');
  }

  renderPane(pane, s, index, selected) {
    const { el } = pane;
    el.hidden = false;
    el.style.order = String(index);
    el.classList.toggle('ml-first', index === 0);
    el.classList.toggle('selected', selected);
    el.classList.toggle('audible', s.isAudible);
    pane.controls.sync(s.isAudible);
    el.querySelector('.ml-pane-name').textContent = s.channelName || s.channelId;
    el.querySelector('.ml-pane-title').textContent = s.liveTitle || '';
    this.renderPaneStats(el, s);
    const avatar = safeUrl(s.profileImageUrl);
    el.querySelector('.ml-pane-avatar').innerHTML = avatar
      ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '';
    el.querySelector('.ml-pane-cap').textContent = s.maxHeightCap > 0 ? `≤${s.maxHeightCap}p` : '';
    const kind = s.state && s.state.kind;
    const text = kind === 'error' ? (s.state.message || '재생 오류') : (STATE_TEXT[kind] ?? '');
    const overlay = el.querySelector('.ml-pane-state');
    overlay.hidden = !text;
    overlay.classList.toggle('error', kind === 'error');
    overlay.classList.toggle('ended', kind === 'offline');
    overlay.querySelector('.spinner').hidden = kind !== 'loading';
    overlay.querySelector('span').textContent = text;
  }

  /** 패인 위 지표 칩 — 라이브 화면 상단과 같은 모양(LIVE·시청자·경과·누적). 좁은 패인은 CSS 가 뒤에서부터 접는다. */
  renderPaneStats(el, s) {
    const stats = el.querySelector('.ml-pane-stats');
    const kind = s.state && s.state.kind;
    if (kind === 'offline') {
      stats.innerHTML = '<span class="stat off">오프라인</span>';
      return;
    }
    const uptime = uptimeText(s.openDate);
    stats.innerHTML = [
      '<span class="stat live">LIVE</span>',
      s.viewerCountText ? `<span class="stat" title="현재 시청자">${STAT_ICON.viewers}${esc(s.viewerCountText)}명</span>` : '',
      uptime ? `<span class="stat stat-uptime" title="방송 경과 시간">${STAT_ICON.uptime}${esc(uptime)}</span>` : '',
      s.accumulateCount > 0 ? `<span class="stat stat-acc" title="누적 시청자">누적 ${esc(s.accumulateCountText)}명</span>` : '',
    ].filter(Boolean).join('');
  }

  /** 포커스 레이아웃 — 방향·비율에 맞춰 grid 템플릿과 패인 위치, 구분선 위치를 정한다. */
  applyFocusGrid(focus, order) {
    const panes = [...this.panes.values()].filter((p) => p.sessionId);
    if (!focus) {
      this.stage.style.gridTemplateColumns = '';
      this.stage.style.gridTemplateRows = '';
      for (const p of panes) { p.el.style.gridRow = ''; p.el.style.gridColumn = ''; }
      this.focusAdd.hidden = true;
      this.divider.hidden = true;
      return;
    }
    const st = this.state;
    const ratio = this.localRatio ?? st.focusRatio;
    // 더 넣을 수 있으면 서브 줄 끝에 채널 추가 칸 — 가득 차면 칸을 빼 서브 패인이 줄을 다 쓴다.
    const grid = focusGrid(order.length, st.focusDirection, ratio, Boolean(st.canAdd));
    this.stage.style.gridTemplateColumns = grid.columns;
    this.stage.style.gridTemplateRows = grid.rows;
    for (const p of panes) {
      const area = grid.areas[order.indexOf(p.sessionId)];
      if (!area) continue;
      p.el.style.gridRow = area.row;
      p.el.style.gridColumn = area.column;
    }
    this.focusAdd.hidden = !grid.add;
    if (grid.add) { this.focusAdd.style.gridRow = grid.add.row; this.focusAdd.style.gridColumn = grid.add.column; }
    const r = clampFocusRatio(ratio);
    const d = this.divider;
    d.hidden = false;
    d.className = `ml-divider ${st.focusDirection === 'top' || st.focusDirection === 'bottom' ? 'horizontal' : 'vertical'}`;
    const pos = st.focusDirection === 'right' || st.focusDirection === 'bottom' ? 1 - r : r;
    d.style.left = d.classList.contains('vertical') ? `calc(${(pos * 100).toFixed(2)}% - 4px)` : '';
    d.style.top = d.classList.contains('horizontal') ? `calc(${(pos * 100).toFixed(2)}% - 4px)` : '';
  }

  bindFocusControls() {
    this.root.addEventListener('click', (e) => {
      const b = e.target.closest('[data-direction]');
      if (b) this.run(api.multiLiveSetFocusLayout(b.dataset.direction, null));
    });
    this.ratioInput.addEventListener('input', () => {
      this.localRatio = Number(this.ratioInput.value) / 100;
      this.ratioOutput.textContent = `${this.ratioInput.value}%`; this.syncRatioFill();
      this.render(this.state);
    });
    this.ratioInput.addEventListener('change', () => this.commitRatio());
    this.divider.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      this.divider.setPointerCapture(e.pointerId);
      this.stage.classList.add('resizing');
      const move = (ev) => {
        const r = this.stage.getBoundingClientRect();
        this.localRatio = ratioFromPointer(this.state.focusDirection, (ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height);
        this.ratioInput.value = String(Math.round(this.localRatio * 100));
        this.ratioOutput.textContent = `${this.ratioInput.value}%`; this.syncRatioFill();
        this.render(this.state);
      };
      const up = () => {
        this.divider.removeEventListener('pointermove', move);
        this.stage.classList.remove('resizing');
        this.commitRatio();
      };
      this.divider.addEventListener('pointermove', move);
      this.divider.addEventListener('pointerup', up, { once: true });
      this.divider.addEventListener('pointercancel', up, { once: true });
    });
    this.divider.addEventListener('dblclick', () => {
      this.localRatio = 0.7;
      this.commitRatio();
    });
  }

  async commitRatio() {
    const ratio = this.localRatio;
    if (ratio == null) return;
    try {
      const state = await api.multiLiveSetFocusLayout(null, ratio);
      this.localRatio = null;
      this.render(state);
    } catch (e) {
      this.localRatio = null;
      toast(e.message);
    }
    this.reportStageSize();
  }

  // ── 사용자 배치 그리드 (MUL-06) ──────────────
  buildGridDividers() {
    const mk = (cls) => { const d = document.createElement('div'); d.className = `ml-divider ${cls} ml-grid-div`; d.hidden = true; this.stage.append(d); return d; };
    const v = mk('vertical');
    const h = mk('horizontal');
    for (const [d, axis] of [[v, 'h'], [h, 'v']]) {
      d.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        d.setPointerCapture(e.pointerId);
        this.stage.classList.add('resizing');
        const move = (ev) => {
          const r = this.stage.getBoundingClientRect();
          const frac = axis === 'h' ? (ev.clientX - r.left) / r.width : (ev.clientY - r.top) / r.height;
          this.localGrid = { ...(this.localGrid || { h: this.state.horizontalRatio, v: this.state.verticalRatio }), [axis]: Math.min(0.8, Math.max(0.2, frac)) };
          this.applyCustomGrid(true, (this.state.sessions || []).length);
        };
        const up = () => {
          d.removeEventListener('pointermove', move);
          this.stage.classList.remove('resizing');
          const g = this.localGrid;
          this.localGrid = null;
          if (g) this.run(api.multiLiveSetRatios(g.h, g.v));
        };
        d.addEventListener('pointermove', move);
        d.addEventListener('pointerup', up, { once: true });
      });
      d.addEventListener('dblclick', () => this.run(api.multiLiveSetRatios(0.5, 0.5)));
    }
    return { v, h };
  }

  applyCustomGrid(on, n) {
    const { v, h } = this.gridDividers;
    if (!on) {
      if (!this.stage.classList.contains('ml-focus')) { this.stage.style.gridTemplateColumns = ''; this.stage.style.gridTemplateRows = ''; }
      v.hidden = true; h.hidden = true;
      return;
    }
    const st = this.state;
    const hr = (this.localGrid && this.localGrid.h) ?? st.horizontalRatio ?? 0.5;
    const vr = (this.localGrid && this.localGrid.v) ?? st.verticalRatio ?? 0.5;
    this.stage.style.gridTemplateColumns = `${hr.toFixed(3)}fr ${(1 - hr).toFixed(3)}fr`;
    this.stage.style.gridTemplateRows = n >= 3 ? `${vr.toFixed(3)}fr ${(1 - vr).toFixed(3)}fr` : '1fr';
    v.hidden = false;
    v.style.left = `calc(${(hr * 100).toFixed(2)}% - 4px)`;
    h.hidden = n < 3;
    h.style.top = `calc(${(vr * 100).toFixed(2)}% - 4px)`;
  }

  /**
   * 첫 프레임 감시 — 상태는 '재생 중'인데 영상 데이터가 20초 넘게 하나도 없으면 그 패인만 다시 연결한다(최대 2번).
   * 일부러 로딩을 멈춘 패인(디코드 예산·시청 모드)과 화면에 안 보이는 패인은 보지 않는다.
   */
  watchFirstFrame(pane, s, now) {
    const kind = s && s.state && s.state.kind;
    let video = null;
    try { video = pane.bridge.iframe.contentDocument.getElementById('video'); } catch { /* 준비 전 */ }
    const stuck = kind === 'playing' && !s.decodeSuspended && !pane.el.hidden && video && video.readyState === 0;
    if (!stuck) {
      pane.zeroSince = null;
      if (video && video.readyState > 0) pane.frameRetries = 0;
      return;
    }
    pane.zeroSince ??= now;
    if (now - pane.zeroSince < 20000 || (pane.frameRetries || 0) >= 2) return;
    pane.frameRetries = (pane.frameRetries || 0) + 1;
    pane.zeroSince = now;
    toast(`${s.channelName || '패인'} 영상이 시작되지 않아 다시 연결합니다`, 2200);
    this.run(api.multiLiveRetry(s.id));
  }

  /** "모든 패인에 적용" — 지금 패인들의 선명한 화면을 같게 하고, 처음 보는 채널의 기본값도 그렇게 둔다. */
  applySharpenAll(on) {
    setSharpenDefault(on);
    for (const pane of this.panes.values()) {
      if (!pane.sessionId || !pane.controls?.channelId) continue;
      setPicture(pane.controls.channelId, { sharpen: Boolean(on) });
      pane.controls.applyPicture();
    }
    toast(on ? '모든 패인을 선명한 화면으로' : '모든 패인의 선명한 화면을 껐습니다', 1500);
  }

  /** 패인 볼륨을 올리면 그 패인 소리를 켠다(여러 소리 모드면 더하고, 아니면 선택). */
  makeAudible(pane) {
    const id = pane.sessionId;
    if (!id) return;
    if (this.state.multiAudio) this.run(api.multiLiveAudio(id, false));
    else if (id !== this.state.selectedId) this.run(api.multiLiveSelect(id));
  }

  // ── 한 채널 크게 (MUL-11) ─────────────────────
  toggleSingle(slot) {
    this.single = this.single === slot ? null : slot;
    const s = (this.state.sessions || []).find((x) => x.slot === slot);
    if (this.single != null && s && s.id !== this.state.selectedId) this.run(api.multiLiveSelect(s.id));
    else this.render(this.state);
    this.reportStageSize();
  }

  // ── 지난 구성 (MUL-15) ───────────────────────
  savedSessionIds() {
    try { return JSON.parse(localStorage.getItem('cview.ml.lastSessions') || '[]'); } catch { return []; }
  }

  saveSessionIds(ids) {
    try { localStorage.setItem('cview.ml.lastSessions', JSON.stringify(ids)); } catch { /* 무시 */ }
  }

  async restoreSessions(ids) {
    for (const id of ids) {
      try { this.render(await api.multiLiveAdd(id)); } catch (e) { toast(e.message); }
    }
  }

  // ── 채널 프리셋 (MUL-14) ─────────────────────
  async showPresets() {
    let presets = [];
    try { presets = (await api.presetList()).filter((p) => p.kind === 'live'); } catch (e) { toast(e.message); }
    const ids = (this.state.sessions || []).map((s) => s.channelId);
    const scrim = document.createElement('div');
    scrim.className = 'sheet-scrim';
    scrim.innerHTML = `<form class="sheet" role="dialog" aria-label="멀티라이브 프리셋">
      <h2>멀티라이브 프리셋</h2>
      <div class="preset-list">${presets.map((p) => `<div class="preset-row" data-preset-id="${esc(p.id)}"><strong>${esc(p.name)}</strong><span class="muted">채널 ${p.channels.length}개</span><div class="grow"></div><button type="button" class="btn" data-preset-load>불러오기</button><button type="button" class="icon-btn" data-preset-del title="삭제">✕</button></div>`).join('') || '<p class="muted">저장한 프리셋이 없습니다</p>'}</div>
      ${ids.length ? `<div class="word-form"><input type="text" maxlength="30" placeholder="지금 구성(${ids.length}개 채널) 이름"><button class="btn primary">저장</button></div>` : ''}
      <div class="sheet-actions"><button type="button" class="btn" data-preset-close>닫기</button></div></form>`;
    document.body.append(scrim);
    const close = () => dismiss(scrim);
    scrim.addEventListener('click', async (e) => {
      if (e.target === scrim || e.target.closest('[data-preset-close]')) { close(); return; }
      const row = e.target.closest('[data-preset-id]');
      if (!row) return;
      const p = presets.find((x) => x.id === row.dataset.presetId);
      if (e.target.closest('[data-preset-del]')) { try { await api.presetDelete(p.id); row.remove(); } catch (er) { toast(er.message); } }
      if (e.target.closest('[data-preset-load]')) { close(); toast(`${p.name} 불러오는 중…`, 1500); await this.restoreSessions(p.channels); }
    });
    scrim.querySelector('form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const name = scrim.querySelector('.word-form input')?.value.trim();
      if (!name) return;
      try { await api.presetSave(name, ids); toast(`프리셋 "${name}" 저장`, 1500); close(); } catch (er) { toast(er.message); }
    });
  }

  // ── 제안 토스트 (MUL-13) — 방송 중 팔로잉 중 아직 없는 인기 채널을 하나씩 권한다 ──
  async proposeTick() {
    const sessions = (this.state && this.state.sessions) || [];
    if (this.root.hidden || !sessions.length || !this.state.canAdd) return;
    let lives = [];
    try { lives = await api.followingLives(false); } catch { return; }
    const have = new Set(sessions.map((s) => s.channelId));
    const pick = lives.filter((l) => !have.has(l.channelId) && !this.proposed.has(l.channelId)).sort((a, b) => b.viewerCount - a.viewerCount)[0];
    if (!pick) return;
    this.proposed.add(pick.channelId);
    toast(`${pick.channelName} 님이 방송 중이에요 — 멀티라이브에 추가할까요?`, 8000, { label: '추가', run: () => this.add(pick.channelId) });
  }

  /** 메인 크기 슬라이더의 채움 위치(트랙 색)를 값에 맞춘다. */
  syncRatioFill() {
    const i = this.ratioInput;
    const pct = ((Number(i.value) - Number(i.min)) / (Number(i.max) - Number(i.min))) * 100;
    i.style.setProperty('--fill', `${Math.max(0, Math.min(100, pct))}%`);
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

  /**
   * 라이브 단독 무대일 때 머리줄의 채널 고르기 — 멀티라이브의 다른 채널을 다시 연결 없이 라이브로 바꿔 본다.
   * 채널이 하나뿐이거나 멀티라이브 보기면 숨긴다.
   */
  renderWatchPicker(sessions, watched) {
    const list = watched ? sessions.filter((x) => x.slot != null) : [];
    this.watchPicker.hidden = list.length < 2;
    if (this.watchPicker.hidden) { this.watchPicker.innerHTML = ''; return; }
    this.watchPicker.innerHTML = list.map((x) => {
      const on = x.id === watched.id;
      const avatar = safeUrl(x.profileImageUrl);
      const name = x.channelName || x.channelId;
      return `<button class="ml-watch-chip${on ? ' active' : ''}" data-ml="watch" data-session-id="${esc(x.id)}" aria-pressed="${on}" title="${esc(on ? `${name} — 지금 보는 채널` : `${name} 라이브로 보기`)}">
        ${avatar ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="ml-watch-dot"></span>'}
        <span class="ellipsis">${esc(name)}</span>
      </button>`;
    }).join('');
  }

  async onClick(e) {
    const layout = e.target.closest('[data-layout]');
    if (layout) {
      this.run(api.multiLiveSetLayout(layout.dataset.layout));
      return;
    }
    const action = e.target.closest('[data-ml]');
    if (!action) return;
    // 패인을 끌어 놓은 직후의 click 은 선택이 아니다.
    if (action.dataset.ml === 'select' && this.suppressClickUntil && performance.now() < this.suppressClickUntil) return;
    const pane = action.closest('.ml-pane');
    const sessionId = pane && this.panes.get(Number(pane.dataset.slot))?.sessionId;
    switch (action.dataset.ml) {
      case 'add': this.addBar.toggle(); break;
      case 'stop-all': this.run(api.multiLiveStopAll()); break;
      case 'select': if (sessionId && sessionId !== this.state.selectedId) this.run(api.multiLiveSelect(sessionId)); break;
      case 'remove': if (sessionId) this.run(api.multiLiveRemove(sessionId)); break;
      case 'audio':
        if (!sessionId) break;
        // 소리 나는 패인은 끄기, 볼륨 0 인 패인은 되돌려 켜기(PaneControls). 그 밖엔 이 패인을 듣는다.
        if (this.panes.get(Number(pane.dataset.slot))?.controls?.toggleMute()) break;
        if (this.state.multiAudio) this.run(api.multiLiveAudio(sessionId, false));
        else this.run(api.multiLiveSelect(sessionId));
        break;
      case 'single': if (pane && this.watchSlot == null) this.toggleSingle(Number(pane.dataset.slot)); break;
      case 'watch': {
        // 패인의 '라이브로 크게 보기' 또는 라이브 머리줄의 채널 고르기(data-session-id).
        const id = action.dataset.sessionId || sessionId;
        if (id && !(this.watchSlot != null && id === this.state.watchId)) window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: id } }));
        break;
      }
      case 'multi-audio': this.run(api.multiLiveAudio(null, true)); break;
      case 'presets': this.showPresets(); break;
      case 'restore': this.restoreSessions(this.savedSessionIds()); break;
      case 'retry': if (sessionId) { toast('다시 연결합니다', 1200); this.run(api.multiLiveRetry(sessionId)); } break;
      case 'alert-pos': this.alerts.preview(); break;
      case 'retry-all': toast('모든 패인을 다시 연결합니다', 1500); this.run(api.multiLiveRetry(null)); break;
      case 'info': {
        const s = (this.state.sessions || []).find((x) => x.id === sessionId);
        if (s) window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: s.channelId } }));
        break;
      }
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

