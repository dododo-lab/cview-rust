// 채팅 패널 — `chat://event` (Rust ChatEventDto) 를 그린다.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast } from './dom.js';

const MAX_LINES = 300;

/** `{:name:}` 이모티콘 치환 (치지직 채팅 본문 규칙). 나머지는 이스케이프. */
export function renderContent(content, emojis) {
  const map = emojis || {};
  return String(content ?? '')
    .split(/(\{:[^{}:]+:\})/g)
    .map((part) => {
      const m = /^\{:([^{}:]+):\}$/.exec(part);
      const url = m && safeUrl(map[m[1]]);
      return url ? `<img class="emoji" src="${esc(url)}" alt="${esc(m[1])}" referrerpolicy="no-referrer">` : esc(part);
    })
    .join('');
}

export function formatDonation(amount) {
  if (typeof amount !== 'number' || !Number.isFinite(amount) || amount <= 0) return '';
  return `${amount.toLocaleString('ko-KR')}원`;
}

function lineHtml(m) {
  const badges = (m.badges || [])
    .map((u) => safeUrl(u))
    .filter(Boolean)
    .map((u) => `<img class="badge" src="${esc(u)}" alt="" referrerpolicy="no-referrer">`)
    .join('');
  const style = m.titleColor && /^#[0-9a-f]{3,8}$/i.test(m.titleColor) ? ` style="color:${m.titleColor}"` : '';
  if (m.kind === 'system') {
    return `<div class="chat-line system" data-id="${esc(m.id)}">${esc(m.content)}</div>`;
  }
  const classes = ['chat-line'];
  if (m.kind === 'donation') classes.push('donation');
  if (m.isBlinded) classes.push('blinded');
  const amount = m.kind === 'donation' && formatDonation(m.donationAmount)
    ? `<span class="amount">${esc(formatDonation(m.donationAmount))}</span>` : '';
  const sub = m.kind === 'subscription' && m.subscriptionMonths
    ? `<span class="amount">${esc(m.subscriptionMonths)}개월 구독</span>` : '';
  return `<div class="${classes.join(' ')}" data-id="${esc(m.id)}">${badges}<span class="nick ${esc(m.role)}"${style}>${esc(m.nickname || '익명')}</span>${amount}${sub}<span class="text">${renderContent(m.content, m.emojis)}</span></div>`;
}

const STATE_LABEL = {
  disconnected: ['연결 끊김', 'bad'],
  connecting: ['연결 중', 'warn'],
  connected: ['연결됨', 'ok'],
  reconnecting: ['재연결 중', 'warn'],
  failed: ['연결 실패', 'bad'],
};

export class ChatPanel {
  constructor(root, { isLoggedIn }) {
    this.root = root;
    this.list = root.querySelector('.chat-list');
    this.state = root.querySelector('.chat-state');
    this.notice = root.querySelector('.chat-notice');
    this.input = root.querySelector('.chat-input input');
    this.sendButton = root.querySelector('.chat-input button');
    this.isLoggedIn = isLoggedIn;
    this.connected = false;
    this.canSend = false;
    this.active = false;

    root.querySelector('.chat-input').addEventListener('submit', (e) => {
      e.preventDefault();
      this.send();
    });
    listen(Events.chat, (ev) => this.onEvent(ev));
    this.updateInput();
  }

  async connect(chatChannelId, channelId) {
    this.clear();
    this.active = true;
    this.setState('connecting');
    if (!chatChannelId) {
      this.setState('failed');
      this.system('채팅 채널 정보가 없습니다');
      return;
    }
    try {
      const res = await api.chatConnect(chatChannelId, channelId);
      this.canSend = Boolean(res && res.canSend);
    } catch (e) {
      this.setState('failed');
      this.system(e.message);
    }
    this.updateInput();
  }

  async disconnect() {
    this.active = false;
    this.connected = false;
    this.canSend = false;
    try { await api.chatDisconnect(); } catch { /* 무시 */ }
    this.setState('disconnected');
    this.updateInput();
  }

  clear() {
    this.list.innerHTML = '<div class="empty">메시지 대기 중…</div>';
    this.notice.hidden = true;
  }

  setState(name, detail) {
    const [label, cls] = STATE_LABEL[name] || [name, ''];
    this.state.textContent = name === 'reconnecting' && detail ? `${label} (${detail})` : label;
    this.state.className = `chat-state ${cls}`;
    this.connected = name === 'connected';
    this.updateInput();
  }

  updateInput() {
    const loggedIn = this.isLoggedIn();
    const enabled = loggedIn && this.connected && this.canSend;
    this.input.disabled = !enabled;
    this.sendButton.disabled = !enabled;
    this.input.placeholder = loggedIn ? '채팅을 입력하세요…' : '로그인 후 채팅 참여 가능';
  }

  append(messages) {
    if (!messages || !messages.length) return;
    const placeholder = this.list.querySelector('.empty');
    if (placeholder) placeholder.remove();
    const atBottom = this.list.scrollHeight - this.list.scrollTop - this.list.clientHeight < 40;
    this.list.insertAdjacentHTML('beforeend', messages.map(lineHtml).join(''));
    while (this.list.children.length > MAX_LINES) this.list.firstElementChild.remove();
    if (atBottom) this.list.scrollTop = this.list.scrollHeight;
  }

  system(text) {
    this.append([{ id: `sys-${Date.now()}`, kind: 'system', content: text }]);
  }

  onEvent(ev) {
    if (!ev || !this.active) return;
    switch (ev.type) {
      case 'state':
        this.setState(ev.state, ev.detail);
        break;
      case 'connected':
        this.setState('connected');
        break;
      case 'reconnecting':
        this.setState('reconnecting');
        break;
      case 'disconnected':
        this.setState('disconnected');
        break;
      case 'messages':
        this.append(ev.messages);
        break;
      case 'notice':
        this.notice.innerHTML = `📌 <strong>${esc(ev.message.nickname)}</strong> ${renderContent(ev.message.content, ev.message.emojis)}`;
        this.notice.hidden = false;
        break;
      case 'noticeCleared':
        this.notice.hidden = true;
        break;
      case 'blinded': {
        const line = this.list.querySelector(`[data-id="${CSS.escape(ev.messageId)}"]`);
        if (line) {
          line.classList.add('blinded');
          const text = line.querySelector('.text');
          if (text) text.textContent = ev.blindText || '관리자에 의해 삭제된 메시지입니다.';
        }
        break;
      }
      case 'kicked':
        this.system('채팅방에서 퇴장되었습니다');
        break;
      case 'penalized':
        this.system('채팅 이용이 제한되었습니다');
        break;
      case 'sendRejected':
        toast(ev.message || '메시지를 보내지 못했습니다');
        break;
      case 'system':
        this.system(ev.text);
        break;
      case 'cleared':
        this.clear();
        break;
      default:
        break;
    }
  }

  async send() {
    const text = this.input.value.trim();
    if (!text) return;
    this.input.disabled = true;
    try {
      await api.chatSend(text);
      this.input.value = '';
    } catch (e) {
      toast(e.message);
    } finally {
      this.updateInput();
      this.input.focus();
    }
  }
}
