// 채팅만 보기 — 원본 ChatOnlyView: 영상 없이 한 채널의 채팅만 넓게 (CHT-22).
import { api } from './api.js';
import { esc, safeUrl, toast } from './dom.js';
import { ChatPanel } from './chat.js';

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
        <div class="grow"></div>
        ${standalone ? '<button class="btn" data-co="pick">채널 바꾸기</button>' : '<button class="btn ghost" data-co="window" title="이 채팅을 새 창으로 (⇧⌘C)">새 창</button>'}
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
    this.chat = new ChatPanel(this.root.querySelector('.chatonly-chat'), { isLoggedIn, session });
    if (standalone) this.root.querySelector('[data-co=back]').remove();
    this.root.addEventListener('click', (e) => {
      const a = e.target.closest('[data-co]');
      if (!a) return;
      if (a.dataset.co === 'back') this.onBack();
      if (a.dataset.co === 'pick') this.showPicker();
      if (a.dataset.co === 'window' && this.channelId) api.windowOpen('chat', { channelId: this.channelId }).catch((er) => toast(er.message));
      if (a.dataset.co === 'pick-channel') this.show({ channelId: a.dataset.channelId });
      if (a.dataset.co === 'play' && this.channelId) this.onPlay(this.channelId);
    });
    new MutationObserver(() => { if (this.root.hidden && this.chat.active) this.chat.disconnect(); })
      .observe(this.root, { attributes: true, attributeFilter: ['hidden'] });
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
      return `<button class="picker-row" data-co="pick-channel" data-channel-id="${esc(l.channelId)}">${a ? `<img class="avatar" src="${esc(a)}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}<span class="ellipsis"><b>${esc(l.channelName)}</b> ${esc(l.categoryName || '')}</span><small>${esc(l.viewerCountText)}</small></button>`;
    }).join('') || '<p class="muted">방송 중인 채널이 없습니다</p>'}</div>`;
  }

  async show(params = {}) {
    if (!params.channelId) { if (this.standalone && !this.channelId) this.showPicker(); return; }
    this.root.querySelector('.chat-picker').hidden = true;
    this.channelId = params.channelId;
    try {
      const live = await api.liveDetail(params.channelId, true);
      const avatar = safeUrl(live.channelImageUrl);
      this.root.querySelector('.chatonly-avatar').innerHTML = avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '';
      this.root.querySelector('.chatonly-title .page-title').textContent = live.channelName || '채팅';
      this.root.querySelector('.chatonly-title .page-subtitle').textContent = live.isLive ? `${live.liveTitle || ''} · ${live.viewerCountText}명 시청 중` : '방송 중이 아닙니다';
      await this.chat.connect(live.chatChannelId, live.channelId);
    } catch (e) {
      toast(e.message);
    }
  }
}
