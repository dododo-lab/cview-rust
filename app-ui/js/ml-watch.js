// 라이브 메뉴 단독 보기(원본 시청 모드) 도구 — 멀티라이브 패인 하나를 라이브 무대로 쓸 때,
// 예전 시청 화면(viewer.js)에 있던 기능을 같은 패인 위에서 그대로 쓰게 한다.
//
// - 단축키: Space 재생/일시정지 · M 음소거 · ↑↓ 볼륨 · L 라이브로 · ←/→ 되감기(⇧ 60초) · 사용자 맨 글자 동작(F·C·P…)
// - 위 막대(상단 바): 채널 정보(제목·채널·인증·팔로워·카테고리·지표 칩·화질/지연 배지) | 채널 묶음(팔로우·즐겨찾기·링크 복사)
//   | 보기 묶음(채팅 표시 방식·새 창으로 분리) | ⋯(채널·방송·보기 묶음 — 좁으면 접힌 단추도 여기로) | 닫기
// - 아래 막대 단추: 스크린샷 · 북마크 · 영상+채팅 전체 화면 · 가운데 OSD
// - 패인 ⚙ 의 배속·화면 꽉 채우기·자동 보정/업스케일·이퀄라이저(Windows)는 무대 패인에만(ml-pane-controls.js setStage)
// 앱 메뉴 동작(⌘S·⇧⌘F·PiP·⌘W 등)은 app.js 가 이 도구로 넘긴다.
import { screenshotFormat, screenshotExtension } from './screenshot-format.js';
import { api } from './api.js';
import { esc, toast, keyBelongsToTarget, plainLetter } from './dom.js';
import { chatPrefs, onChatPrefs, updateChatPrefs } from './chat-prefs.js';
import { matchPlainAction, currentBindings, bindingLabel, onShortcuts } from './shortcuts.js';
import { togglePipFor, stamp, toggleFullscreenOn } from './viewer-controls.js';
import { seekPlan } from './viewer-extras.js';
import { compactCount } from './cards.js';
import { kbd } from './platform.js';
import { WatchInfoPanel } from './ml-watch-info.js';
import { CHANNEL_ICON, CHANNEL_SOCIAL_EVENT, loadChannelSocial, setFollow, setFavorite, copyLiveLink, openOnChzzk } from './channel-actions.js';

const CHAT_MODES = ['side', 'overlay', 'hidden'];
const CHAT_MODE_LABEL = { side: '채팅: 옆', overlay: '채팅: 영상 위', hidden: '채팅: 숨김' };

export const WATCH_ICON = {
  camera: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>',
  bookmark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  chatFull: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M15 20h5v-5"/><path d="M14 9h4M14 12h4"/></svg>',
  popout: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="13" height="13" rx="2"/><path d="M10 4h10v10M20 4l-8 8"/></svg>',
};

/**
 * 막대(아래)에 넣는 단독 보기 전용 단추 — 영상을 담는 동작(스크린샷·북마크)만. 멀티라이브 보기에서는 CSS 가 숨긴다.
 * 채팅 표시 방식은 위 막대의 '보기' 묶음으로 옮겼다(watchTopHtml).
 */
export function watchBarHtml() {
  return `
        <button class="ml-pane-btn ml-watch-only" data-mw="shot" title="${kbd('스크린샷 (⌘S)')}" aria-label="스크린샷">${WATCH_ICON.camera}</button>
        <button class="ml-pane-btn ml-watch-only" data-mw="bookmark" title="북마크 (B)" aria-label="북마크">${WATCH_ICON.bookmark}</button>`;
}

export function watchBarEndHtml() {
  return `<button class="ml-pane-btn ml-watch-only" data-mw="fullchat" title="${kbd('영상+채팅 전체 화면 (⌃⌘F)')}" aria-label="영상과 채팅 전체 화면">${WATCH_ICON.chatFull}</button>`;
}

/**
 * 위 막대 오른쪽 — 단독 보기 무대 패인에서만 보인다. 의미별 두 묶음:
 *   채널(팔로우·즐겨찾기·링크 복사) | 보기(채팅 표시 방식·새 창으로 분리) — 그 뒤 ⋯ · 닫기는 패인 공용.
 * 좁은 패인(520px 이하)에서는 두 묶음이 접히고 같은 동작이 ⋯ 메뉴(watchMenuHtml)에 나온다.
 */
export function watchTopHtml() {
  return `
        <div class="ml-wt-group ml-watch-only" role="group" aria-label="채널">
          <button class="ml-wt-follow" data-mw="follow" title="팔로우" hidden><span class="ml-wt-ico">${CHANNEL_ICON.follow}</span><span class="ml-wt-label">팔로우</span></button>
          <button class="ml-pane-btn" data-mw="fav" aria-pressed="false" title="즐겨찾기에 추가" aria-label="즐겨찾기">${CHANNEL_ICON.star}</button>
          <button class="ml-pane-btn" data-mw="copy" title="방송 링크 복사" aria-label="방송 링크 복사">${CHANNEL_ICON.link}</button>
        </div>
        <span class="ml-wt-sep ml-watch-only" aria-hidden="true"></span>
        <div class="ml-wt-group ml-watch-only" role="group" aria-label="보기">
          <button class="ml-pane-btn" data-mw="chatmode" title="채팅 표시 방식 (C)" aria-label="채팅 표시 방식">${WATCH_ICON.chat}</button>
          <button class="ml-pane-btn" data-mw="popout" title="${kbd('새 창으로 분리 (⌘N)')}" aria-label="새 창으로 분리">${WATCH_ICON.popout}</button>
        </div>`;
}

/** 위 정보 지표 줄 끝 — 지금 화질·지연 배지(누르면 ⚙ 화질·지연 메뉴). 단독 보기 무대 패인에서만, 값이 오면 보인다. */
export function watchStatusHtml() {
  return '<button class="ml-q-badge ml-watch-only" data-mp="quality" aria-haspopup="menu" aria-expanded="false" hidden><span class="ml-q-res"></span><span class="ml-q-lat"></span></button>';
}

/**
 * 단독 보기 ⋯ 메뉴 — 채널 / 방송 / 보기 묶음. 위 막대에 단추가 없는 동작(채널 정보·치지직에서 열기·멀티라이브에 추가·
 * 다시 연결)과 단축키가 있는 영상 동작(북마크·스크린샷·PiP·영상+채팅 전체 화면)은 늘 보이고, 위 막대 단추와 같은 동작
 * (팔로우·즐겨찾기·링크·채팅 표시·새 창 — `ml-narrow-only`)은 위 막대가 접혔을 때(좁은 패인)만 보인다.
 * 채널 정보·다시 연결·PiP 는 패인 공용 항목(ml-pane-controls.js)을 그대로 쓰고 순서만 맞춘다.
 */
export function watchMenuHtml(shared) {
  const key = (k) => `<kbd class="ml-menu-key" data-key-of="${k}"></kbd>`;
  const item = (mw, icon, label, { narrow = false, k = '', attrs = '' } = {}) => `<button class="ml-menu-item ml-watch-only${narrow ? ' ml-narrow-only' : ''}" role="menuitem" data-mw="${mw}"${attrs}>${icon}<span>${label}</span>${k}</button>`;
  return `
        <div class="ml-menu-label ml-watch-only" aria-hidden="true">채널</div>
        ${shared.info}
        ${item('follow', CHANNEL_ICON.follow, '팔로우', { narrow: true, attrs: ' hidden' })}
        ${item('fav', CHANNEL_ICON.star, '즐겨찾기에 추가', { narrow: true })}
        ${item('copy', CHANNEL_ICON.link, '방송 링크 복사', { narrow: true })}
        ${item('external', CHANNEL_ICON.external, '치지직에서 열기')}
        ${item('multi', CHANNEL_ICON.multi, '멀티라이브에 추가')}
        <div class="ml-menu-sep ml-watch-only"></div>
        <div class="ml-menu-label ml-watch-only" aria-hidden="true">방송</div>
        ${item('bookmark', WATCH_ICON.bookmark, '북마크', { k: key('bookmark') })}
        ${item('shot', WATCH_ICON.camera, '스크린샷', { k: key('screenshot') })}
        ${shared.retry}
        <div class="ml-menu-sep ml-watch-only"></div>
        <div class="ml-menu-label ml-watch-only" aria-hidden="true">보기</div>
        ${item('chatmode', WATCH_ICON.chat, '채팅 표시 방식', { narrow: true, k: key('chat') })}
        ${item('popout', WATCH_ICON.popout, '새 창으로 분리', { narrow: true, k: key('popout') })}
        ${shared.pip}
        ${item('fullchat', WATCH_ICON.chatFull, '영상+채팅 전체 화면', { k: key('fullchat') })}`;
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
    this.osd.setAttribute('role', 'status'); // 음소거·볼륨 같은 알림을 화면 읽기 프로그램도 읽게
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
        case 'follow': this.toggleFollow(); break;
        case 'fav': this.toggleFavorite(); break;
        case 'copy': if (this.channelId) copyLiveLink(this.channelId); break;
        case 'external': if (this.channelId) openOnChzzk(this.channelId); break;
        case 'multi': if (this.channelId && this.page.onAddChannel) this.page.onAddChannel(this.channelId); break;
        default: break;
      }
    });
    document.addEventListener('keydown', (e) => this.onKey(e));
    onChatPrefs(() => this.applyChatMode());
    // 단축키 표시 — 사용자가 바꾼 키(설정 › 단축키)를 단추 툴팁·⋯ 메뉴에 그대로 보인다.
    onShortcuts(() => this.applyShortcutHints());
    this.applyShortcutHints();
    // 다른 화면(채널 화면·카드 메뉴·채팅 말풍선)에서 바꾼 팔로우·즐겨찾기도 따라간다.
    window.addEventListener(CHANNEL_SOCIAL_EVENT, (e) => {
      const d = e.detail || {};
      if (!this.social || d.channelId !== this.channelId) return;
      if (typeof d.following === 'boolean') this.social.following = d.following;
      if (typeof d.favorite === 'boolean') this.social.favorite = d.favorite;
      this.renderSocial();
    });
  }

  // ── 상단 바 채널 묶음(팔로우·즐겨찾기) ─────────────

  /** 로그인·로그아웃 — 팔로우 단추를 그 상태로 다시 읽는다(로그아웃이면 숨는다). */
  refreshAuth() {
    if (this.channelId) this.loadSocial(this.channelId);
  }

  async loadSocial(channelId) {
    this.social = null;
    this.renderSocial();
    if (!channelId) return;
    const social = await loadChannelSocial(channelId, this.page.isLoggedIn());
    if (channelId !== this.channelId) return;
    this.social = social;
    this.renderSocial();
  }

  /** 팔로우(로그인했을 때만)·즐겨찾기 단추와 ⋯ 항목을 지금 상태로. 상태를 모르면 팔로우는 숨긴다. */
  renderSocial() {
    const s = this.social;
    const name = this.session?.channelName || '채널';
    for (const b of this.root.querySelectorAll('[data-mw="follow"]')) {
      const known = Boolean(s) && typeof s.following === 'boolean';
      b.hidden = !known;
      if (!known) continue;
      const on = s.following;
      b.classList.toggle('on', on);
      b.disabled = Boolean(this.socialBusy);
      const label = on ? '팔로잉' : '팔로우';
      const hint = on ? `${name} 팔로잉 중 — 눌러서 팔로우 끊기` : `${name} 팔로우`;
      b.title = hint;
      const icon = b.querySelector('.ml-wt-ico, svg');
      if (b.classList.contains('ml-menu-item')) {
        b.querySelector('span').textContent = on ? '팔로우 끊기' : '팔로우';
        b.setAttribute('aria-label', hint);
      } else {
        b.querySelector('.ml-wt-label').textContent = label;
        // 보이는 글자(팔로우/팔로잉)를 이름 앞에 — 아이콘만일 때도 같은 이름(WCAG 2.5.3). 상태는 글자 자체가 알린다.
        b.setAttribute('aria-label', on ? `팔로잉 — ${name} 팔로우 끊기` : `팔로우 — ${name}`);
        if (icon) icon.innerHTML = on ? CHANNEL_ICON.following : CHANNEL_ICON.follow;
      }
    }
    for (const b of this.root.querySelectorAll('[data-mw="fav"]')) {
      const on = Boolean(s && s.favorite);
      b.setAttribute('aria-pressed', String(on));
      b.classList.toggle('on', on);
      b.title = on ? '즐겨찾기에서 빼기' : '즐겨찾기에 추가';
      if (b.classList.contains('ml-menu-item')) {
        b.removeAttribute('aria-pressed');
        b.querySelector('span').textContent = b.title;
      } else {
        b.setAttribute('aria-label', '즐겨찾기');
        const svg = on ? CHANNEL_ICON.starOn : CHANNEL_ICON.star;
        if (b.dataset.icon !== String(on)) { b.innerHTML = svg; b.dataset.icon = String(on); }
      }
    }
  }

  async toggleFollow() {
    const s = this.social;
    if (!this.channelId || !s || typeof s.following !== 'boolean' || this.socialBusy) return;
    this.socialBusy = true;
    this.renderSocial();
    const id = this.channelId;
    const next = await setFollow(id, this.session?.channelName, !s.following);
    this.socialBusy = false;
    if (next !== null && id === this.channelId) s.following = next;
    this.renderSocial();
  }

  async toggleFavorite() {
    if (!this.channelId) return;
    const s = this.social || (this.social = { following: null, favorite: false });
    const id = this.channelId;
    const live = this.session;
    const next = await setFavorite(id, live?.channelName, live?.profileImageUrl, !s.favorite);
    if (next !== null && id === this.channelId) s.favorite = next;
    this.renderSocial();
  }

  /** 단추 툴팁·⋯ 메뉴 오른쪽의 단축키 — 지금 바인딩(바꿀 수 있는 것)과 고정 키. */
  applyShortcutHints() {
    const b = currentBindings();
    const label = (x) => (x && x.key ? bindingLabel(x) : '');
    const keys = {
      chat: label(b.toggleChat), pip: label(b.togglePiP), screenshot: label(b.screenshot) || kbd('⌘S'),
      bookmark: 'B', popout: kbd('⌘N'), fullchat: kbd('⌃⌘F'),
    };
    for (const k of this.root.querySelectorAll('.ml-more-pane [data-key-of]')) {
      const text = keys[k.dataset.keyOf] || '';
      if (k.textContent !== text) k.textContent = text;
    }
    this.applyChatMode();
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
      this.loadSocial(id);
    }
    this.infoPanel.show(id);
    if (s) this.infoPanel.syncViewers(s.viewerCountText); // 시청자 수가 바뀌면 정보란 숫자가 굴러 바뀐다
    // 위 정보 줄의 인증·팔로워·카테고리 — 지금 무대 패인에만
    for (const p of this.page.panes.values()) {
      const meta = p.el.querySelector('.ml-watch-meta');
      if (p !== this.pane || !id) meta?.remove();
    }
    this.renderInfo();
    const pane = this.pane;
    if (pane && !this.hooked.has(pane.bridge)) {
      this.hooked.add(pane.bridge);
      this.applyShortcutHints(); // 새로 만든 패인의 ⋯ 메뉴에도 단축키를 채운다
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
    // 단독 보기 오버레이 채팅은 시청 화면과 같은 '오버레이 배경 투명도'(채팅 설정)를 따른다 — 그리드는 멀티라이브 설정(감사 UX-H20)
    if (watching) this.body.style.setProperty('--ml-chat-opacity', String(Math.min(0.95, Math.max(0.1, Number(p.overlayBackgroundOpacity) || 0.5))));
    else this.body.style.removeProperty('--ml-chat-opacity');
    for (const b of this.root.querySelectorAll('[data-mw="chatmode"]')) {
      const key = currentBindings().toggleChat;
      b.title = `${CHAT_MODE_LABEL[mode]} — 눌러서 바꾸기${key && key.key ? ` (${bindingLabel(key)})` : ''}`;
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
    // 플레이어의 play()/pause() 는 곧바로 v.paused 를 바꾼다 — 누르기 전 상태로 문구를 고른다(예전엔 반대로 떴다).
    const wasPaused = v.paused;
    this.pane.controls.togglePlay();
    this.flash(wasPaused ? '재생' : '일시정지');
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
    c.normalRate(); // ⚙ 에서 고른 배속도 1 로(예전 시청 화면 LIVE 와 같다)
    c.bridge.call('seekToLiveEdge');
    this.flash('라이브로 이동');
  }

  /** 라이브 되감기(PLY-03) — 재생 목록이 허용하는 구간 안에서. */
  seekBy(step) {
    const video = this.video;
    if (!video || !video.seekable || !video.seekable.length) return;
    const start = video.seekable.start(0);
    const end = video.seekable.end(video.seekable.length - 1);
    const plan = seekPlan(video.currentTime, step, start, end);
    if (plan === null) return;
    if (plan.live) {
      this.pane.bridge.call('seekToLiveEdge');
      this.flash('라이브');
      return;
    }
    const target = plan.target;
    if (plan.atLive || Math.abs(target - video.currentTime) < 1) {
      this.flash(step < 0 ? '더 되감을 수 없어요' : '라이브 끝입니다');
      return;
    }
    this.pane.bridge.call('seek', target);
    const behind = Math.max(0, Math.round(end - target));
    this.flash(behind > 2 ? `라이브보다 ${behind}초 뒤` : '라이브');
  }

  async screenshot() {
    const pane = this.pane;
    if (!pane) return;
    pane.shotPending = true;
    pane.bridge.call('takeScreenshot', await screenshotFormat());
  }

  async saveScreenshot(data) {
    if (!data || !data.ok) {
      toast('스크린샷을 찍지 못했습니다 — 영상이 아직 준비되지 않았습니다');
      return;
    }
    const name = this.session?.channelName || 'CView';
    try {
      const path = await api.saveFile('screenshot', `${name}_${stamp()}.${screenshotExtension(data)}`, data.base64, null);
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
    toggleFullscreenOn(withChat ? this.body : this.pane?.el);
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
    const key = e.key.length === 1 ? (plainLetter(e) || e.key) : e.key;
    if (key === ' ') { e.preventDefault(); this.togglePlay(); }
    else if (key === 'm') this.toggleMute();
    else if (key === 'ArrowUp' || key === 'ArrowDown') { e.preventDefault(); this.changeVolume(key === 'ArrowUp' ? 5 : -5); }
    else if (key === 'ArrowLeft' || key === 'ArrowRight') { e.preventDefault(); this.seekBy((e.shiftKey ? 60 : 10) * (key === 'ArrowLeft' ? -1 : 1)); }
    else if (key === 'l') this.goLive();
    else if (key === 'b' && !e.shiftKey) this.bookmark();
  }
}
