// 채팅만 보기 — 원본 ChatOnlyView: 영상 없이 한 채널의 채팅만 넓게 (CHT-22).
import { api } from './api.js';
import { esc, safeUrl, toast } from './dom.js';
import { ChatPanel } from './chat.js';

export class ChatOnlyPage {
  constructor(content, { isLoggedIn, onBack, onPlay }) {
    this.acceptsParams = true;
    this.onBack = onBack;
    this.onPlay = onPlay;
    this.root = document.createElement('section');
    this.root.className = 'page page-chatonly';
    this.root.dataset.page = 'chat';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header chatonly-head">
        <button class="back-btn" data-co="back">‹ 뒤로</button>
        <span class="chatonly-avatar"></span>
        <div class="chatonly-title"><h1 class="page-title ellipsis">채팅</h1><span class="page-subtitle ellipsis"></span></div>
        <div class="grow"></div>
        <button class="btn primary" data-co="play">▶ 영상 보기</button>
      </header>
      <aside class="chat chatonly-chat">
        <header class="chat-header"><strong>채팅</strong><span class="chat-state">연결 끊김</span></header>
        <div class="chat-notice" hidden></div>
        <div class="chat-list"></div>
        <form class="chat-input"><input type="text" maxlength="100" placeholder="채팅을 입력하세요…" autocomplete="off"><button class="btn primary" type="submit">채팅</button></form>
      </aside>`;
    content.append(this.root);
    this.chat = new ChatPanel(this.root.querySelector('.chatonly-chat'), { isLoggedIn });
    this.root.addEventListener('click', (e) => {
      const a = e.target.closest('[data-co]');
      if (!a) return;
      if (a.dataset.co === 'back') this.onBack();
      if (a.dataset.co === 'play' && this.channelId) this.onPlay(this.channelId);
    });
    new MutationObserver(() => { if (this.root.hidden && this.chat.active) this.chat.disconnect(); })
      .observe(this.root, { attributes: true, attributeFilter: ['hidden'] });
  }

  async show(params = {}) {
    if (!params.channelId) return;
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
