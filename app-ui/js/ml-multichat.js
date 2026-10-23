// 멀티라이브 멀티채팅 — 모든 패인의 채팅을 한 타임라인으로 합쳐 본다.
//
// - 켜면 패인마다 추가 채팅 세션(`mlx:<세션 id>`)을 연다. 선택 패인도 따로 연다 — 선택이 바뀌어도
//   합친 목록이 끊기거나 같은 메시지가 두 번 들어오지 않게(선택 패인 채팅 `ml` 은 패널이 계속 쓴다).
// - 줄마다 패인 이름 태그와 패인 색(테두리와 같은 색조)을 붙인다. 보내기는 지금처럼 선택 패인으로 간다.
// - 끄면 추가 세션을 모두 닫고 기존 선택 패인 채팅으로 돌아간다. 켬/끔은 이 기기에 기억한다.
import { api, listen } from './api.js';
import { mergedLinesHtml } from './multichat.js';
import { applyChatSkin } from './chat-extras.js';
import { onChatPrefs } from './chat-prefs.js';

const STORE_KEY = 'cview.ml.mergedChat';
const MAX_LINES = 600;
const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H9l-4 3v-3H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z"/><path d="M19 9h1a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-1v3l-4-3h-4"/></svg>';

const sessionKey = (id) => `mlx:${id}`;

export class MultiLiveMergedChat {
  /**
   * @param {HTMLElement} chatEl 멀티라이브 채팅 패널(.ml-chat)
   * @param {{ getState: () => object, getPanes: () => Map }} deps
   */
  constructor(chatEl, { getState, getPanes }) {
    this.chatEl = chatEl;
    this.getState = getState;
    this.getPanes = getPanes;
    this.on = false;
    try { this.on = localStorage.getItem(STORE_KEY) === '1'; } catch { /* 저장 불가 */ }
    this.conns = new Map(); // 세션 id → { key, chatChannelId, channelId, name, hue }
    this.panelList = chatEl.querySelector('.chat-list');
    this.list = document.createElement('div');
    this.list.className = 'chat-list ml-merged-list';
    this.list.hidden = true;
    this.panelList.after(this.list);
    const skin = () => applyChatSkin(this.list);
    onChatPrefs(skin);
    skin();
    this.title = chatEl.querySelector('.chat-header strong');
    const header = chatEl.querySelector('.chat-header');
    const firstBtn = header.querySelector('.chat-hbtn');
    this.button = document.createElement('button');
    this.button.type = 'button';
    this.button.className = 'icon-btn chat-hbtn ml-merge-btn';
    this.button.innerHTML = ICON;
    this.button.addEventListener('click', () => this.toggle());
    header.insertBefore(this.button, firstBtn || null);
    listen('chat://session-event', (p) => this.onEvent(p));
    this.applyChrome();
  }

  toggle() {
    this.on = !this.on;
    try { localStorage.setItem(STORE_KEY, this.on ? '1' : '0'); } catch { /* 저장 불가 */ }
    if (this.on) this.list.innerHTML = '<div class="empty">모든 패인의 새 메시지를 기다리는 중…</div>';
    this.applyChrome();
    this.sync();
  }

  applyChrome() {
    this.button.classList.toggle('active', this.on);
    this.button.setAttribute('aria-pressed', String(this.on));
    this.button.title = this.on ? '멀티채팅 끄기 — 선택한 패인 채팅만 보기' : '멀티채팅 — 모든 패인 채팅을 한 곳에서 보기';
    this.button.setAttribute('aria-label', this.button.title);
    this.chatEl.classList.toggle('ml-merged', this.on);
    this.list.hidden = !this.on;
    this.panelList.hidden = this.on;
    if (this.title) this.title.textContent = this.on ? '멀티채팅' : '채팅';
  }

  /** 패인 구성에 맞춰 추가 세션을 열고 닫는다(render 마다 불린다 — 바뀐 것만 건드린다). */
  sync() {
    const state = this.getState() || {};
    const sessions = this.on ? (state.sessions || []).filter((s) => s.chatChannelId) : [];
    const hueOf = new Map();
    for (const p of this.getPanes().values()) if (p.sessionId) hueOf.set(p.sessionId, p.hue);
    const keep = new Set();
    for (const s of sessions) {
      keep.add(s.id);
      const name = s.channelName || s.channelId;
      const c = this.conns.get(s.id);
      if (c && c.chatChannelId === s.chatChannelId) {
        c.name = name;
        c.hue = hueOf.get(s.id) ?? c.hue;
        continue;
      }
      const conn = { key: sessionKey(s.id), chatChannelId: s.chatChannelId, channelId: s.channelId, name, hue: hueOf.get(s.id) ?? 0 };
      this.conns.set(s.id, conn);
      api.chatConnect(conn.chatChannelId, conn.channelId, conn.key).catch((e) => {
        if (this.conns.get(s.id) === conn) this.system(`${name} 채팅에 연결하지 못했어요 — ${e.message}`);
      });
    }
    for (const [id, c] of this.conns) {
      if (keep.has(id)) continue;
      this.conns.delete(id);
      api.chatDisconnect(c.key).catch(() => {});
    }
  }

  onEvent(p) {
    if (!this.on || !p || !p.event || p.event.type !== 'messages' || typeof p.session !== 'string' || !p.session.startsWith('mlx:')) return;
    const c = [...this.conns.values()].find((x) => x.key === p.session);
    if (!c) return;
    const html = mergedLinesHtml(p.event.messages, { name: c.name, style: `--lh:${Number(c.hue) || 0}` });
    if (!html) return;
    this.append(html);
  }

  system(text) {
    const line = document.createElement('div');
    line.className = 'chat-line system';
    line.textContent = text;
    this.append(line.outerHTML);
  }

  append(html) {
    const list = this.list;
    const atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    const empty = list.querySelector(':scope > .empty');
    if (empty) empty.remove();
    list.insertAdjacentHTML('beforeend', html);
    while (list.children.length > MAX_LINES) list.firstElementChild.remove();
    if (atBottom) list.scrollTop = list.scrollHeight;
  }
}
