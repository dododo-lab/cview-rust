// 채팅만 보기 — 원본 ChatOnlyView: 영상 없이 한 채널의 채팅만 넓게 (CHT-22).
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { ChatPanel } from './chat.js';
import { kbd } from './platform.js';
import { verifiedBadge } from './verified-badge.js';

export class ChatOnlyPage {
  constructor(content, { isLoggedIn, onBack, onPlay, session = null, standalone = false }) {
    this.isLoggedIn = isLoggedIn;
    this.standalone = standalone;
    this.acceptsParams = true;
    this.onBack = onBack;
    this.onPlay = onPlay;
    this.root = document.createElement('section');
    this.root.className = 'page page-chatonly';
    this.root.dataset.page = 'chat';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header chatonly-head" data-tauri-drag-region>
        <button class="back-btn" data-co="back">‹ 뒤로</button>
        <span class="chatonly-avatar"></span>
        <div class="chatonly-title"><h1 class="page-title ellipsis">채팅</h1><span class="page-subtitle ellipsis"></span></div>
        <div class="grow" data-tauri-drag-region></div>
        ${standalone ? '<button class="btn" data-co="pick">채널 바꾸기</button>' : `<button class="btn ghost" data-co="window" title="${kbd('이 채팅을 새 창으로 (⇧⌘C)')}">새 창</button>`}
        <button class="btn primary" data-co="play">▶ 영상 보기</button>
      </header>
      <div class="chat-picker" hidden></div>
      <aside class="chat chatonly-chat">
        <header class="chat-header"><strong>채팅</strong><span class="chat-state">연결 끊김</span></header>
        <div class="chat-notice" hidden></div>
        <div class="chat-list"></div>
        <form class="chat-input"><input type="text" maxlength="100" placeholder="채팅을 입력하세요…" autocomplete="off"><button class="btn primary" type="submit">채팅</button></form>
      </aside>`;
    content.append(this.root);
    // 채팅 창은 채널 하나의 라이브 채팅이다 — 멘션 알림도 띄운다(창마다 따로 세션을 쓰게 된 뒤 기본값이 꺼져 알림이 오지 않았다).
    this.chat = new ChatPanel(this.root.querySelector('.chatonly-chat'), { isLoggedIn, session, mentionNotify: true });
    if (standalone) this.root.querySelector('[data-co=back]').remove();
    this.root.addEventListener('click', (e) => {
      const a = e.target.closest('[data-co]');
      if (!a) return;
      if (a.dataset.co === 'back') this.onBack();
      if (a.dataset.co === 'pick') this.togglePicker(a);
      if (a.dataset.co === 'window' && this.channelId) api.windowOpen('chat', { channelId: this.channelId }).catch((er) => toast(er.message));
      if (a.dataset.co === 'pick-channel') this.show({ channelId: a.dataset.channelId });
      if (a.dataset.co === 'play' && this.channelId) this.onPlay(this.channelId);
    });
    // 숨으면 진행 중인 열기도 버린다(seq) — 정보 응답 전에 떠나면 숨은 화면에서 채팅이 연결돼 남았다.
    new MutationObserver(() => { if (this.root.hidden) { this.seq = (this.seq || 0) + 1; if (this.chat.active) this.chat.disconnect(); } })
      .observe(this.root, { attributes: true, attributeFilter: ['hidden'] });
    // 열어 둔 동안 방송 상태를 1분마다 다시 본다(보이고 앱이 앞에 있을 때만) — 예전엔 열 때 한 번만 봐서 꺼진 채널을 기다리던 창이
    // 방송이 시작돼도 '방송 중이 아닙니다'에 머물렀고, 채팅 채널이 없거나 바뀌면 ⌘R 전까지 다시 붙지 않았다.
    setInterval(() => this.checkLive(), 60 * 1000);
  }

  /** '채널 바꾸기' — 다시 누르거나 Esc 면 닫는다(예전엔 채널을 골라야만 닫혔다, 감사 UX-M09). */
  togglePicker(opener) {
    const box = this.root.querySelector('.chat-picker');
    if (!box.hidden && this.channelId) { box.hidden = true; return; }
    if (!this.pickerKeys) {
      this.pickerKeys = true;
      box.addEventListener('keydown', (e) => {
        if (e.key !== 'Escape' || !this.channelId) return;
        e.preventDefault();
        e.stopPropagation();
        box.hidden = true;
        opener?.focus({ preventScroll: true });
      });
    }
    this.showPicker();
  }

  /** 채널 피커 (CHT-23) — 방송 중인 팔로잉(없으면 인기)에서 고른다. */
  async showPicker() {
    const box = this.root.querySelector('.chat-picker');
    box.hidden = false;
    box.innerHTML = '<div class="empty"><div class="spinner"></div></div>';
    let lives = [];
    try { lives = this.isLoggedIn() ? await api.followingLives(false) : await api.popularLives(30, false); } catch (e) { toast(e.message); }
    box.innerHTML = `<h3 class="section-title">채팅을 볼 채널</h3><div class="picker-list">${lives.map((l) => {
      const a = safeUrl(l.channelImageUrl);
      return `<button class="picker-row" data-co="pick-channel" data-channel-id="${esc(l.channelId)}">${a ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(a, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}<span class="ellipsis"><b>${esc(l.channelName)}</b>${verifiedBadge(l, 'sm')} ${esc(l.categoryName || '')}</span><small>${esc(l.viewerCountText)}</small></button>`;
    }).join('') || '<p class="muted">방송 중인 채널이 없습니다</p>'}</div>`;
  }

  async checkLive() {
    const id = this.channelId;
    if (!id || this.root.hidden || document.hidden || !document.hasFocus()) return;
    const seq = this.seq;
    let live;
    try { live = await api.liveDetail(id, true); } catch { return; }
    if (seq !== this.seq || id !== this.channelId || this.root.hidden) return;
    this.root.querySelector('.chatonly-title .page-subtitle').textContent = live.isLive ? `${live.liveTitle || ''} · ${live.viewerCountText}명 시청 중` : '방송 중이 아닙니다';
    const stale = live.chatChannelId && (live.chatChannelId !== this.chat.chatChannelId || !this.chat.active || this.chat.stateName === 'failed');
    if (stale) await this.chat.connect(live.chatChannelId, live.channelId);
  }

  /** ⌘R·다시 불러오기가 지금 채널로 다시 열게 — 예전엔 '채널 바꾸기'로 바꾼 뒤 ⌘R 을 누르면 창을 연 처음 채널로 돌아갔다. */
  routeParams() {
    return this.channelId ? { channelId: this.channelId } : {};
  }

  async show(params = {}) {
    if (!params.channelId) { if (this.standalone && !this.channelId) this.showPicker(); return; }
    this.root.querySelector('.chat-picker').hidden = true;
    this.channelId = params.channelId;
    // 빠르게 채널을 바꾸면 늦게 온 정보가 새 채널 머리·채팅을 덮지 않게 순번을 본다.
    const seq = this.seq = (this.seq || 0) + 1;
    try {
      const live = await api.liveDetail(params.channelId, true);
      if (seq !== this.seq) return;
      const avatar = safeUrl(live.channelImageUrl);
      this.root.querySelector('.chatonly-avatar').innerHTML = avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '';
      this.root.querySelector('.chatonly-title .page-title').textContent = live.channelName || '채팅';
      this.root.querySelector('.chatonly-title .page-subtitle').textContent = live.isLive ? `${live.liveTitle || ''} · ${live.viewerCountText}명 시청 중` : '방송 중이 아닙니다';
      await this.chat.connect(live.chatChannelId, live.channelId);
    } catch (e) {
      if (seq !== this.seq) return;
      // 앞 채널의 채팅을 끊고 머리줄도 비운다 — 예전엔 새 채널 정보를 받지 못하면 앞 채널 채팅이 그대로 이어져, 거기 쓴 말은
      // 앞 채널로 가는데 '영상 보기'는 새 채널을 열었다(시청 화면은 이미 이렇게 끊는다).
      if (this.chat.active) this.chat.disconnect();
      this.root.querySelector('.chatonly-avatar').innerHTML = '';
      this.root.querySelector('.chatonly-title .page-title').textContent = '채팅';
      this.root.querySelector('.chatonly-title .page-subtitle').textContent = kbd('채널 정보를 불러오지 못했습니다 — ⌘R 로 다시 시도해 주세요');
      toast(e.message);
    }
  }
}
