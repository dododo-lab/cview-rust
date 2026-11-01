// 라이브 메뉴 단독 보기(원본 시청 모드) 도구 — 멀티라이브 패인 하나를 라이브 무대로 쓸 때,
// 예전 시청 화면(viewer.js)에 있던 기능을 같은 패인 위에서 그대로 쓰게 한다.
//
// - 단축키: Space 재생/일시정지 · M 음소거 · ↑↓ 볼륨 · L 라이브로 · ←/→ 되감기(⇧ 60초) · 사용자 맨 글자 동작(F·C·P…)
// - 막대 단추: 스크린샷 · 북마크 · 채팅 표시 방식(옆·영상 위·숨김) · 영상+채팅 전체 화면, 위쪽: 새 창으로 분리
// - 위 정보에 인증·팔로워·카테고리 · 가운데 OSD
// 앱 메뉴 동작(⌘S·⇧⌘F·PiP·⌘W 등)은 app.js 가 이 도구로 넘긴다.
import { api } from './api.js';
import { esc, toast, keyBelongsToTarget } from './dom.js';
import { chatPrefs, onChatPrefs, updateChatPrefs } from './chat-prefs.js';
import { matchPlainAction } from './shortcuts.js';
import { togglePipFor, stamp } from './viewer-controls.js';
import { seekTarget } from './viewer-extras.js';
import { compactCount } from './cards.js';
import { kbd } from './platform.js';
import { WatchInfoPanel } from './ml-watch-info.js';

const CHAT_MODES = ['side', 'overlay', 'hidden'];
const CHAT_MODE_LABEL = { side: '채팅: 옆', overlay: '채팅: 영상 위', hidden: '채팅: 숨김' };

export const WATCH_ICON = {
  camera: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>',
  bookmark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  chatFull: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M15 20h5v-5"/><path d="M14 9h4M14 12h4"/></svg>',
  popout: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="13" height="13" rx="2"/><path d="M10 4h10v10M20 4l-8 8"/></svg>',
};

/** 막대(아래)에 넣는 단독 보기 전용 단추 — 멀티라이브 보기에서는 CSS 가 숨긴다. */
export function watchBarHtml() {
  return `
        <button class="ml-pane-btn ml-watch-only" data-mw="shot" title="${kbd('스크린샷 (⌘S)')}" aria-label="스크린샷">${WATCH_ICON.camera}</button>
        <button class="ml-pane-btn ml-watch-only" data-mw="bookmark" title="북마크 (B)" aria-label="북마크">${WATCH_ICON.bookmark}</button>
        <button class="ml-pane-btn ml-watch-only" data-mw="chatmode" title="채팅 표시 방식 (C)" aria-label="채팅 표시 방식">${WATCH_ICON.chat}</button>`;
}

export function watchBarEndHtml() {
  return `<button class="ml-pane-btn ml-watch-only" data-mw="fullchat" title="${kbd('영상+채팅 전체 화면 (⌃⌘F)')}" aria-label="영상과 채팅 전체 화면">${WATCH_ICON.chatFull}</button>`;
}

export function watchTopHtml() {
  return `<button class="ml-pane-btn ml-watch-only" data-mw="popout" title="${kbd('새 창으로 분리 (⌘N)')}" aria-label="새 창으로 분리">${WATCH_ICON.popout}</button>`;
}

export class WatchTools {
  /**
   * @param {import('./multilive.js').MultiLivePage} page
   */
  constructor(page) {
    this.page = page;
    this.root = page.root;
    this.body = page.root.querySelector('.ml-body');
    this.gridChatOverlay = false; // 멀티라이브 보기의 "그리드에서 채팅 오버레이" 설정
    this.channelId = null;
    this.info = null; // { channelId, followers, category } — 인증 마크는 패인 이름 옆(.ml-pane-vb)
    this.hooked = new WeakSet(); // 스크린샷 응답을 듣는 패인 브리지
    // 영상 아래 채널 정보·다시보기·클립·커뮤니티
    this.infoPanel = new WatchInfoPanel(page.root.querySelector('.ml-watch-info'));

    this.osd = document.createElement('div');
    this.osd.className = 'player-osd';
    this.osd.hidden = true;
    page.stage.append(this.osd);

    this.root.addEventListener('click', (e) => {
      const b = e.target.closest('[data-mw]');
      if (!b || !this.active) return;
      e.stopPropagation();
      switch (b.dataset.mw) {
        case 'shot': this.screenshot(); break;
        case 'bookmark': this.bookmark(); break;
        case 'chatmode': this.cycleChatMode(); break;
        case 'fullchat': this.fullscreen(true); break;
        case 'popout': window.dispatchEvent(new CustomEvent('cview:menu', { detail: 'window-player' })); break;
        default: break;
      }
    });
    document.addEventListener('keydown', (e) => this.onKey(e));
    onChatPrefs(() => this.applyChatMode());
  }

  /** 라이브 메뉴 단독 보기가 화면에 떠 있는가. */
  get active() {
    return this.page.watchSlot != null && !this.root.hidden;
  }

  get pane() {
    return this.page.watchSlot != null ? this.page.panes.get(this.page.watchSlot) || null : null;
  }

  get session() {
    const st = this.page.state;
    return (st && st.watchId && (st.sessions || []).find((x) => x.id === st.watchId)) || null;
  }

  get video() {
    return this.pane?.controls.video || null;
  }

  /** multilive.render 끝에서 — 단독 보기에 들어오고 나가거나 채널이 바뀔 때 화면을 맞춘다. */
  sync() {
    const s = this.page.watchSlot != null ? this.session : null;
    const id = s ? s.channelId : null;
    this.applyChatMode();
    if (id !== this.channelId) {
      this.channelId = id;
      this.info = null;
      for (const meta of this.root.querySelectorAll('.ml-watch-meta')) meta.remove();
      this.loadInfo(id);
    }
    this.infoPanel.show(id);
    // 위 정보 줄의 인증·팔로워·카테고리 — 지금 무대 패인에만
    for (const p of this.page.panes.values()) {
      const meta = p.el.querySelector('.ml-watch-meta');
      if (p !== this.pane || !id) meta?.remove();
    }
    this.renderInfo();
    const pane = this.pane;
    if (pane && !this.hooked.has(pane.bridge)) {
      this.hooked.add(pane.bridge);
      pane.bridge.onMessage((type, data) => {
        if (type === 'screenshot' && pane.shotPending) {
          pane.shotPending = false;
          this.saveScreenshot(data);
        }
      });
    }
  }

  // ── 위 정보·머리줄 ─────────────────────────

  async loadInfo(channelId) {
    if (!channelId) return;
    const [ch, live] = await Promise.allSettled([api.channelInfo(channelId), api.liveDetail(channelId)]);
    if (channelId !== this.channelId) return;
    const c = ch.status === 'fulfilled' ? ch.value : null;
    const l = live.status === 'fulfilled' ? live.value : null;
    this.info = {
      followers: c && typeof c.followerCount === 'number' ? c.followerCount : null,
      category: (l && l.categoryName) || '',
    };
    this.renderInfo();
  }

  renderInfo() {
    const pane = this.pane;
    if (!pane || !this.info) return;
    const line = pane.el.querySelector('.ml-pane-line');
    let meta = line.querySelector('.ml-watch-meta');
    if (!meta) {
      meta = document.createElement('span');
      meta.className = 'ml-watch-meta ellipsis';
      line.append(meta);
    }
    const { followers, category } = this.info;
    const html = [
      followers != null ? `팔로워 ${esc(compactCount(followers))}명` : '',
      category ? esc(category) : '',
    ].filter(Boolean).join('<i class="sep">·</i>');
    if (meta.innerHTML !== html) meta.innerHTML = html;
  }

  // ── 채팅 표시 방식 (PLY-12) — 예전 시청 화면과 같은 설정(chat.displayMode)을 쓴다 ──

  get chatMode() {
    const m = chatPrefs().displayMode;
    return CHAT_MODES.includes(m) ? m : 'side';
  }

  /** 멀티라이브 보기는 "그리드에서 채팅 오버레이" 설정, 단독 보기는 채팅 표시 방식을 따른다. */
  applyChatMode() {
    const watching = this.page.watchSlot != null;
    const mode = this.chatMode;
    this.body.classList.toggle('chat-overlay', watching ? mode === 'overlay' : this.gridChatOverlay);
    this.body.classList.toggle('chat-hidden', watching && mode === 'hidden');
    const p = chatPrefs();
    this.body.style.setProperty('--overlay-w', `${Math.min(560, Math.max(240, Number(p.overlayWidth) || 320))}px`);
    for (const b of this.root.querySelectorAll('[data-mw="chatmode"]')) {
      b.title = `${CHAT_MODE_LABEL[mode]} — 눌러서 바꾸기 (C)`;
      // 좁은 패인의 ⋯ 메뉴 항목(ml-pane-controls.js)은 지금 방식을 글자로 — 막대 단추는 켜짐 색으로
      const label = b.classList.contains('ml-menu-item') ? b.querySelector('span') : null;
      if (label) label.textContent = `${CHAT_MODE_LABEL[mode]} — 바꾸기`;
      else b.classList.toggle('active', mode !== 'side');
    }
    this.page.reportStageSize();
  }

  cycleChatMode() {
    const next = CHAT_MODES[(CHAT_MODES.indexOf(this.chatMode) + 1) % CHAT_MODES.length];
    updateChatPrefs({ displayMode: next });
    this.flash(CHAT_MODE_LABEL[next]);
  }

  // ── 재생 동작 ─────────────────────────────

  togglePlay() {
    const v = this.video;
    if (!v) return;
    this.pane.controls.togglePlay();
    this.flash(v.paused ? '재생' : '일시정지');
  }

  toggleMute() {
    const pane = this.pane;
    if (!pane) return;
    const c = pane.controls;
    if (!c.toggleMute()) {
      // 들리지 않는 패인(다른 패인이 소리를 가짐) — 이 패인 소리를 켠다.
      this.page.makeAudible(pane);
    }
    this.flash(c.gain === 0 ? '음소거' : '음소거 해제');
  }

  changeVolume(delta) {
    const c = this.pane?.controls;
    if (!c) return;
    const next = Math.min(100, Math.max(0, Number(c.range.value) + delta));
    c.range.value = String(next);
    c.setGain(next / 100);
    c.volumeReveal.peek();
    c.wake();
    this.flash(`볼륨 ${next}%`);
  }

  goLive() {
    const c = this.pane?.controls;
    if (!c) return;
    const v = c.video;
    if (v && v.paused) c.bridge.call('play');
    if (v && v.playbackRate !== 1) c.bridge.call('setRate', 1);
    c.bridge.call('seekToLiveEdge');
    this.flash('라이브로 이동');
  }

  /** 라이브 되감기(PLY-03) — 재생 목록이 허용하는 구간 안에서. */
  seekBy(step) {
    const video = this.video;
    if (!video || !video.seekable || !video.seekable.length) return;
    const start = video.seekable.start(0);
    const end = video.seekable.end(video.seekable.length - 1);
    const target = seekTarget(video.currentTime, step, start, end);
    if (target === null) return;
    if (Math.abs(target - video.currentTime) < 1) {
      this.flash(step < 0 ? '더 되감을 수 없어요' : '라이브 끝입니다');
      return;
    }
    this.pane.bridge.call('seek', target);
    const behind = Math.max(0, Math.round(end - target));
    this.flash(behind > 2 ? `라이브보다 ${behind}초 뒤` : '라이브');
  }

  screenshot() {
    const pane = this.pane;
    if (!pane) return;
    pane.shotPending = true;
    pane.bridge.call('takeScreenshot', 'png');
  }

  async saveScreenshot(data) {
    if (!data || !data.ok) {
      toast('스크린샷을 찍지 못했습니다 — 영상이 아직 준비되지 않았습니다');
      return;
    }
    const name = this.session?.channelName || 'CView';
    try {
      const path = await api.saveFile('screenshot', `${name}_${stamp()}.png`, data.base64, null);
      this.flash('스크린샷 저장됨');
      toast(`스크린샷을 저장했습니다 — ${path}`, 3500);
    } catch (e) {
      toast(e.message);
    }
  }

  async bookmark() {
    const id = this.channelId;
    if (!id) return;
    try {
      await api.bookmarkAdd(id, null);
      this.flash('북마크 추가');
    } catch (e) {
      toast(e.message);
    }
  }

  async togglePip() {
    const label = await togglePipFor(this.video);
    if (label) this.flash(label);
  }

  /** 영상 전체 화면(패인) 또는 영상+채팅 전체 화면(무대+채팅). 이미 전체 화면이면 끝낸다. */
  fullscreen(withChat) {
    if (document.fullscreenElement) {
      document.exitFullscreen();
      return;
    }
    const target = withChat ? this.body : this.pane?.el;
    if (target && target.requestFullscreen) target.requestFullscreen();
  }

  flash(text) {
    // 패인 안에 둔다 — 패인 전체 화면에서도 보이게.
    const host = this.pane?.el || this.page.stage;
    if (this.osd.parentElement !== host) host.append(this.osd);
    this.osd.textContent = text;
    this.osd.hidden = false;
    this.osd.classList.remove('fade');
    void this.osd.offsetWidth;
    this.osd.classList.add('fade');
    clearTimeout(this.osdTimer);
    this.osdTimer = setTimeout(() => { this.osd.hidden = true; }, 1300);
  }

  // ── 단축키 — 예전 시청 화면(viewer-controls·viewer-extras)과 같은 키 ──

  onKey(e) {
    if (!this.active || !this.pane || keyBelongsToTarget(e)) return;
    const mod = e.metaKey || e.ctrlKey;
    const act = matchPlainAction(e);
    if (act) {
      e.preventDefault();
      if (act === 'toggleFullscreen') this.fullscreen(false);
      else if (act === 'toggleChat') this.cycleChatMode();
      else if (act === 'togglePiP') this.togglePip();
      else if (act === 'screenshot') this.screenshot();
      else if (act === 'bookmarkStream') this.bookmark();
      return;
    }
    if (mod || e.altKey) {
      if (e.metaKey && e.shiftKey && e.key.toLowerCase() === 'f') { e.preventDefault(); this.fullscreen(false); }
      else if (e.metaKey && e.ctrlKey && e.key.toLowerCase() === 'f') { e.preventDefault(); this.fullscreen(true); }
      return;
    }
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (key === ' ') { e.preventDefault(); this.togglePlay(); }
    else if (key === 'm') this.toggleMute();
    else if (key === 'ArrowUp' || key === 'ArrowDown') { e.preventDefault(); this.changeVolume(key === 'ArrowUp' ? 5 : -5); }
    else if (key === 'ArrowLeft' || key === 'ArrowRight') { e.preventDefault(); this.seekBy((e.shiftKey ? 60 : 10) * (key === 'ArrowLeft' ? -1 : 1)); }
    else if (key === 'l') this.goLive();
    else if (key === 'b' && !e.shiftKey) this.bookmark();
  }
}
