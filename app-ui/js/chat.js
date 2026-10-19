// 채팅 패널 — `chat://event` (Rust ChatEventDto) 를 그린다. 원본 ChatPanelView 와 부속:
// CHT-01 이모티콘 선택기, CHT-02 자동완성(:이모티콘·@멘션), CHT-03 컨텍스트 메뉴, CHT-04 프로필 시트,
// CHT-05 차단, CHT-06 금지어 필터, CHT-08 멘션 강조·트레이, CHT-09 하이라이트, CHT-10 메시지 고정·공지 숨기기,
// CHT-11 자동 스크롤·"새 메시지" 버튼·읽음 구분선(⌘J), CHT-12 지우기(⇧⌘K)·재연결, CHT-14 표시 옵션,
// CHT-16 후원 필터, CHT-20 입력 상태 배너.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast } from './dom.js';
import { DonationRanking, eventCardHtml, eventClasses, EVENT_ICONS } from './donations.js';
import { installChatExtras } from './chat-extras.js';
import {
  chatPrefs, onChatPrefs, updateChatPrefs, passesFilters, mentionsMe, getMyNickname,
} from './chat-prefs.js';

const ICON = {
  smile: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0"/><circle cx="9" cy="10" r=".6" fill="currentColor"/><circle cx="15" cy="10" r=".6" fill="currentColor"/></svg>',
  at: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M16 12v1.5a2.5 2.5 0 0 0 5 0V12a9 9 0 1 0-3.5 7.1"/></svg>',
  trash: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>',
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>',
  down: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  pin: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-5-5 5-1-1 5-5-5-4 2-2 5 1 4-4z"/></svg>',
};

/** `{:name:}` 이모티콘 치환 (치지직 채팅 본문 규칙). 나머지는 이스케이프. */
export function renderContent(content, emojis, showEmoji = true) {
  const map = emojis || {};
  return String(content ?? '')
    .split(/(\{:[^{}:]+:\})/g)
    .map((part) => {
      const m = /^\{:([^{}:]+):\}$/.exec(part);
      const url = m && safeUrl(map[m[1]]);
      if (!url) return esc(part);
      return showEmoji ? `<img class="emoji" src="${esc(url)}" alt="${esc(m[1])}" referrerpolicy="no-referrer">` : `<span class="emoji-text">:${esc(m[1])}:</span>`;
    })
    .join('');
}

// 닉네임 색을 바꾸는 역할만 클래스로 붙인다. 역할 문자열을 그대로 클래스로 쓰면 일반 시청자
// `viewer` 가 시청 화면 섹션의 `.viewer`(position:absolute; inset:0) 와 겹쳐 닉네임이 창 전체를 덮는다.
const ROLE_CLASSES = new Set(['streamer', 'manager', 'channelManager']);

function roleClass(role) {
  return ROLE_CLASSES.has(role) ? ` ${role}` : '';
}

function timeText(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function lineHtml(m, prefs) {
  const badges = (m.badges || [])
    .map((u) => safeUrl(u))
    .filter(Boolean)
    .map((u) => `<img class="badge" src="${esc(u)}" alt="" referrerpolicy="no-referrer">`)
    .join('');
  const style = m.titleColor && /^#[0-9a-f]{3,8}$/i.test(m.titleColor) ? ` style="color:${m.titleColor}"` : '';
  if (m.kind === 'system') {
    return `<div class="chat-line system" data-id="${esc(m.id)}"><span class="sys-pill">${esc(m.content)}</span></div>`;
  }
  const classes = ['chat-line'];
  if (m.isBlinded) classes.push('blinded');
  if (m.kind === 'notice') classes.push('notice-line');
  if (prefs.highlightMentions && mentionsMe(m)) classes.push('mention');
  if ((prefs.highlightedUsers || []).includes(m.userId)) classes.push('highlighted');
  const time = `<time class="chat-time">${esc(timeText(m.timestamp))}</time>`;
  const nick = `<span class="nick${roleClass(m.role)}"${style}>${esc(m.nickname || '익명')}</span>`;
  const text = `<span class="text">${renderContent(m.content, m.emojis, prefs.emoticonEnabled)}</span>`;
  const data = `data-id="${esc(m.id)}" data-uid="${esc(m.userId || '')}" data-nick="${esc(m.nickname || '')}" data-role="${esc(m.role || '')}"`;
  const evClasses = eventClasses(m);
  if (evClasses.length) {
    classes.push(...evClasses);
    const body = m.content && m.content.trim() ? renderContent(m.content, m.emojis, prefs.emoticonEnabled) : '';
    return `<div class="${classes.join(' ')}" ${data}>${eventCardHtml(m, { time, badges: badges ? `<span class="badges">${badges}</span>` : '', nick, content: body })}</div>`;
  }
  if (m.kind === 'notice') {
    return `<div class="${classes.join(' ')}" ${data}><span class="notice-icon">${EVENT_ICONS.megaphone}</span><div class="notice-body"><div class="notice-from">${nick}<span class="notice-tag">공지</span>${time}</div>${text}</div></div>`;
  }
  // 공통 구조 — 아바타·머리(시간·배지·닉네임)·본문. 줄 형태 스타일은 CSS 가 한 줄로 이어 붙이고,
  // 카드·말풍선 스타일(라운지·하이브리드·카카오·iMessage)은 같은 구조를 블록으로 배치한다.
  const me = getMyNickname();
  if (me && m.nickname === me) classes.push('own');
  classes.push('msg');
  const initial = esc(Array.from(m.nickname || '•')[0] || '•');
  const mentionTag = classes.includes('mention') ? '<span class="msg-mention">나를 언급</span>' : '';
  return `<div class="${classes.join(' ')}" ${data}><span class="chat-avatar" aria-hidden="true">${initial}</span><span class="msg-main"><span class="msg-head">${time}<span class="badges">${badges}</span>${nick}${mentionTag}</span>${text}</span></div>`;
}

const STATE_LABEL = {
  disconnected: ['연결 끊김', 'bad'],
  connecting: ['연결 중', 'warn'],
  connected: ['연결됨', 'ok'],
  reconnecting: ['재연결 중', 'warn'],
  failed: ['연결 실패', 'bad'],
};

/** 이모티콘 팩 캐시(채널별). */
const emoticonCache = new Map();
/** 모든 채팅 패널 — Rust 채팅 세션은 하나라 마지막으로 연결한 패널만 이벤트를 그린다. */
const PANELS = new Set();

export class ChatPanel {
  /**
   * @param {HTMLElement} root `.chat` 패널
   * @param {{ isLoggedIn: () => boolean, onHighlight?: (message) => void, onLogin?: () => void }} options
   *   onHighlight — 실시간(최근 기록 제외)으로 들어온 후원·구독 메시지(스트림 알림용)
   */
  constructor(root, { isLoggedIn, onHighlight = null, onLogin = null, session = null }) {
    this.root = root;
    // 세션 키 — null 이면 기본 세션(chat://event), 있으면 추가 세션(chat://session-event, 멀티채팅).
    this.session = session;
    this.onHighlight = onHighlight;
    this.onLogin = onLogin;
    this.ranking = new DonationRanking(root);
    this.list = root.querySelector('.chat-list');
    this.state = root.querySelector('.chat-state');
    this.notice = root.querySelector('.chat-notice');
    this.input = root.querySelector('.chat-input input');
    this.sendButton = root.querySelector('.chat-input button');
    this.isLoggedIn = isLoggedIn;
    this.connected = false;
    this.canSend = false;
    this.active = false;
    this.chatChannelId = null;
    this.channelId = null;
    this.unread = 0;
    this.mentions = [];
    this.recentNicks = new Map(); // nick → 마지막 시각(자동완성)
    this.emoticons = [];
    this.seenEmoji = new Map(); // 채팅에서 본 이모티콘 id → url (이모티콘 API 가 막힌 채널의 대체 팩)

    this.appendHooks = [];
    this.sendHooks = [];
    PANELS.add(this);
    this.buildChrome();
    root.querySelector('.chat-input').addEventListener('submit', (e) => {
      e.preventDefault();
      if (this.autocomplete.hidden) this.send();
    });
    if (session) listen('chat://session-event', (p) => { if (p && p.session === session) this.onEvent(p.event); });
    else listen(Events.chat, (ev) => this.onEvent(ev));
    onChatPrefs(() => this.applyPrefs());
    this.applyPrefs();
    this.updateInput();
    installChatExtras(this);
  }

  // ── 머리·입력 부속 ───────────────────────────

  buildChrome() {
    const header = this.root.querySelector('.chat-header');
    header.insertAdjacentHTML('beforeend', `
      <div class="grow"></div>
      <button class="icon-btn chat-hbtn mention-btn" title="나를 부른 메시지" aria-label="멘션">${ICON.at}<span class="mention-count" hidden></span></button>
      <button class="icon-btn chat-hbtn" data-chat="clear" title="채팅 지우기 (⇧⌘K)" aria-label="채팅 지우기">${ICON.trash}</button>
      <button class="icon-btn chat-hbtn" data-chat="reconnect" title="다시 연결" aria-label="다시 연결">${ICON.refresh}</button>`);
    header.addEventListener('click', (e) => {
      const b = e.target.closest('[data-chat], .mention-btn');
      if (!b) return;
      if (b.classList.contains('mention-btn')) this.toggleMentionTray();
      else if (b.dataset.chat === 'clear') this.clear(true);
      else if (b.dataset.chat === 'reconnect') this.reconnect();
    });
    this.mentionCount = header.querySelector('.mention-count');

    this.pinBar = document.createElement('div');
    this.pinBar.className = 'chat-pin';
    this.pinBar.hidden = true;
    this.notice.after(this.pinBar);
    this.pinBar.addEventListener('click', (e) => { if (e.target.closest('[data-unpin]')) this.pinBar.hidden = true; });
    this.notice.addEventListener('click', (e) => {
      if (e.target.closest('[data-hide-notice]')) { this.notice.hidden = true; this.noticeDismissed = true; }
    });

    this.tray = document.createElement('div');
    this.tray.className = 'chat-tray';
    this.tray.hidden = true;
    this.root.append(this.tray);
    this.tray.addEventListener('click', (e) => {
      if (e.target.closest('[data-tray-clear]')) { this.mentions = []; this.renderMentionCount(); this.tray.hidden = true; }
    });

    this.jump = document.createElement('button');
    this.jump.className = 'chat-jump';
    this.jump.hidden = true;
    this.jump.innerHTML = `${ICON.down}<span>새 메시지</span>`;
    this.list.after(this.jump);
    this.jump.addEventListener('click', () => this.scrollToBottom());
    this.list.addEventListener('scroll', () => this.onScroll(), { passive: true });

    this.banner = document.createElement('div');
    this.banner.className = 'chat-input-banner';
    this.banner.hidden = true;
    const form = this.root.querySelector('.chat-input');
    form.before(this.banner);
    this.banner.addEventListener('click', (e) => {
      if (!e.target.closest('[data-login]')) return;
      if (this.onLogin) this.onLogin();
      else window.dispatchEvent(new Event('cview:login'));
    });

    this.emojiButton = document.createElement('button');
    this.emojiButton.type = 'button';
    this.emojiButton.className = 'icon-btn chat-emoji-btn';
    this.emojiButton.title = '이모티콘';
    this.emojiButton.innerHTML = ICON.smile;
    this.input.before(this.emojiButton);
    this.picker = document.createElement('div');
    this.picker.className = 'emoji-picker';
    this.picker.hidden = true;
    form.append(this.picker);
    this.emojiButton.addEventListener('click', (e) => { e.stopPropagation(); this.togglePicker(); });
    this.picker.addEventListener('click', (e) => this.onPickerClick(e));
    this.picker.addEventListener('input', () => this.renderPicker());
    document.addEventListener('click', (e) => {
      if (!this.picker.contains(e.target) && e.target !== this.emojiButton) this.picker.hidden = true;
      if (!this.tray.contains(e.target) && !e.target.closest('.mention-btn')) this.tray.hidden = true;
    });

    this.autocomplete = document.createElement('div');
    this.autocomplete.className = 'chat-autocomplete';
    this.autocomplete.hidden = true;
    form.append(this.autocomplete);
    this.input.addEventListener('input', () => this.updateAutocomplete());
    this.input.addEventListener('keydown', (e) => this.onInputKey(e));
    this.autocomplete.addEventListener('mousedown', (e) => {
      const item = e.target.closest('[data-ac]');
      if (item) { e.preventDefault(); this.applyAutocomplete(item.dataset.ac); }
    });

    this.list.addEventListener('contextmenu', (e) => this.onContextMenu(e));
    this.header = header;
    this.list.addEventListener('dblclick', (e) => {
      const line = e.target.closest('.chat-line[data-uid]');
      if (line && line.dataset.uid) this.showProfile(line.dataset.uid, line.dataset.nick);
    });
    document.addEventListener('keydown', (e) => {
      if (!this.active || !this.root.offsetParent) return;
      if (e.metaKey && e.shiftKey && e.key.toLowerCase() === 'k') { e.preventDefault(); this.clear(true); }
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === 'j') {
        e.preventDefault();
        const on = !chatPrefs().autoScroll;
        updateChatPrefs({ autoScroll: on });
        toast(on ? '자동 스크롤 켬' : '자동 스크롤 끔', 1200);
      }
    });
  }

  applyPrefs() {
    const p = chatPrefs();
    this.root.style.setProperty('--chat-font', `${Math.min(24, Math.max(10, Number(p.fontSize) || 14))}px`);
    this.root.style.setProperty('--chat-gap', `${Math.min(12, Math.max(0, Number(p.lineSpacing) || 0))}px`);
    this.root.classList.toggle('hide-time', !p.showTimestamp);
    this.root.classList.toggle('hide-badge', !p.showBadge);
    this.root.classList.toggle('no-role-color', !p.highlightRoles);
    this.maxLines = Math.min(2000, Math.max(50, Number(p.maxVisibleMessages) || 300));
  }

  // ── 연결 ───────────────────────────────────

  async connect(chatChannelId, channelId) {
    if (!this.session) for (const p of PANELS) if (p !== this && !p.session) p.active = false;
    this.clear();
    this.active = true;
    if (this.channelId !== channelId) this.seenEmoji.clear();
    this.chatChannelId = chatChannelId;
    this.channelId = channelId;
    this.mentions = [];
    this.renderMentionCount();
    this.noticeDismissed = false;
    this.pinBar.hidden = true;
    this.ranking.load(channelId);
    this.loadEmoticons(channelId);
    this.setState('connecting');
    if (!chatChannelId) {
      this.setState('failed');
      this.system('채팅 채널 정보가 없습니다');
      return;
    }
    try {
      const res = await api.chatConnect(chatChannelId, channelId, this.session);
      this.canSend = Boolean(res && res.canSend);
    } catch (e) {
      this.setState('failed');
      this.system(e.message);
    }
    this.updateInput();
  }

  async reconnect() {
    if (!this.chatChannelId) return;
    toast('채팅에 다시 연결합니다', 1200);
    await this.connect(this.chatChannelId, this.channelId);
  }

  async disconnect() {
    this.active = false;
    this.connected = false;
    this.canSend = false;
    this.ranking.clear();
    try { await api.chatDisconnect(this.session); } catch { /* 무시 */ }
    this.setState('disconnected');
    this.updateInput();
  }

  clear(byUser = false) {
    this.list.innerHTML = `<div class="empty">${byUser ? '채팅을 지웠습니다' : '메시지 대기 중…'}</div>`;
    if (!byUser) this.notice.hidden = true;
    this.unread = 0;
    this.renderJump();
  }

  setState(name, detail) {
    const [label, cls] = STATE_LABEL[name] || [name, ''];
    this.state.textContent = name === 'reconnecting' && detail ? `${label} (${detail})` : label;
    this.state.className = `chat-state ${cls}`;
    this.connected = name === 'connected';
    this.updateInput();
  }

  /** CHT-20 입력 상태 배너 — 왜 보낼 수 없는지 알려 준다. */
  updateInput(reason = null) {
    const loggedIn = this.isLoggedIn();
    const enabled = loggedIn && this.connected && this.canSend;
    this.input.disabled = !enabled;
    this.sendButton.disabled = !enabled;
    this.emojiButton.disabled = !enabled;
    this.input.placeholder = loggedIn ? '채팅을 입력하세요…' : '로그인 후 채팅 참여 가능';
    let html = '';
    if (reason) html = esc(reason);
    else if (!loggedIn) html = '로그인하면 채팅에 참여할 수 있어요 <button class="link-btn" data-login>로그인</button>';
    else if (this.connected && !this.canSend) html = '이 채팅방에서는 지금 채팅을 보낼 수 없습니다 (팔로워·구독자 전용이거나 제한 중)';
    this.banner.innerHTML = html;
    this.banner.hidden = !html;
  }

  // ── 메시지 ─────────────────────────────────

  get atBottom() {
    return this.list.scrollHeight - this.list.scrollTop - this.list.clientHeight < 40;
  }

  append(messages) {
    if (!messages || !messages.length) return;
    const prefs = chatPrefs();
    const shown = messages.filter((m) => passesFilters(m, prefs));
    if (!shown.length) return;
    const placeholder = this.list.querySelector('.empty');
    if (placeholder) placeholder.remove();
    const follow = prefs.autoScroll && this.atBottom;
    if (!follow && !this.list.querySelector('.read-divider') && this.list.children.length) {
      this.list.insertAdjacentHTML('beforeend', '<div class="read-divider"><span>여기까지 읽음</span></div>');
    }
    this.list.insertAdjacentHTML('beforeend', shown.map((m) => lineHtml(m, prefs)).join(''));
    for (const m of shown) {
      if (m.nickname && m.kind !== 'system') this.recentNicks.set(m.nickname, Date.now());
      for (const [id, url] of Object.entries(m.emojis || {})) {
        if (url && !this.seenEmoji.has(id)) this.seenEmoji.set(id, url);
      }
      if (prefs.highlightMentions && mentionsMe(m)) {
        this.mentions.push(m);
        // 멘션 알림 (SYS-05) — 앱이 뒤에 있을 때만, 메인 채팅 패널 한 곳에서만.
        if (prefs.mentionNotify !== false && !document.hasFocus() && this.active && !this.session) {
          api.notifySend(`@${m.nickname} 님이 불렀어요`, String(m.content || '').slice(0, 140)).catch(() => {});
        }
      }
    }
    if (this.recentNicks.size > 400) {
      const keep = [...this.recentNicks.entries()].sort((a, b) => b[1] - a[1]).slice(0, 300);
      this.recentNicks = new Map(keep);
    }
    this.renderMentionCount();
    for (const fn of this.appendHooks) {
      try { fn(shown); } catch (e) { console.error(e); }
    }
    while (this.list.children.length > this.maxLines) this.list.firstElementChild.remove();
    if (follow) this.list.scrollTop = this.list.scrollHeight;
    else { this.unread += shown.length; this.renderJump(); }
  }

  onScroll() {
    if (this.atBottom) {
      this.unread = 0;
      this.renderJump();
      clearTimeout(this.dividerTimer);
      this.dividerTimer = setTimeout(() => {
        if (this.atBottom) this.list.querySelector('.read-divider')?.remove();
      }, 3000);
    }
  }

  renderJump() {
    this.jump.hidden = this.unread === 0;
    this.jump.querySelector('span').textContent = this.unread > 0 ? `새 메시지 ${this.unread > 99 ? '99+' : this.unread}개` : '새 메시지';
  }

  scrollToBottom() {
    this.list.scrollTop = this.list.scrollHeight;
    this.unread = 0;
    this.renderJump();
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
        if (!ev.recent && this.onHighlight) {
          for (const m of ev.messages || []) {
            if ((m.kind === 'donation' || m.kind === 'subscription') && passesFilters(m)) this.onHighlight(m);
          }
        }
        break;
      case 'notice':
        if (this.noticeDismissed) break;
        this.notice.innerHTML = `<span class="notice-icon">📌</span><span class="notice-body"><strong>${esc(ev.message.nickname)}</strong> ${renderContent(ev.message.content, ev.message.emojis)}</span><button class="icon-btn notice-x" data-hide-notice title="공지 숨기기">✕</button>`;
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
        this.updateInput('채팅 이용이 제한되었습니다');
        break;
      case 'sendRejected':
        toast(ev.message || '메시지를 보내지 못했습니다');
        this.updateInput(ev.message || '메시지를 보내지 못했습니다');
        clearTimeout(this.bannerTimer);
        this.bannerTimer = setTimeout(() => this.updateInput(), 5000);
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
    // 슬래시 명령 등 — 훅이 처리하면 서버로 보내지 않는다(CHT-07).
    for (const fn of this.sendHooks) {
      if (fn(text)) { this.input.value = ''; return; }
    }
    this.input.disabled = true;
    try {
      await api.chatSend(text, this.session);
      this.input.value = '';
    } catch (e) {
      toast(e.message);
    } finally {
      this.updateInput();
      this.input.focus();
    }
  }

  // ── 멘션 트레이 (CHT-08) ───────────────────

  renderMentionCount() {
    const n = this.mentions.length;
    this.mentionCount.hidden = n === 0;
    this.mentionCount.textContent = n > 99 ? '99+' : String(n);
  }

  toggleMentionTray() {
    if (!this.tray.hidden) { this.tray.hidden = true; return; }
    const items = this.mentions.slice(-50).reverse();
    this.tray.innerHTML = `
      <div class="tray-head"><strong>나를 부른 메시지</strong><div class="grow"></div>${items.length ? '<button class="link-btn" data-tray-clear>모두 지우기</button>' : ''}</div>
      ${items.length ? items.map((m) => `<div class="tray-item"><time>${esc(timeText(m.timestamp))}</time><b>${esc(m.nickname)}</b> ${renderContent(m.content, m.emojis)}</div>`).join('') : `<div class="tray-empty">${getMyNickname() ? `@${esc(getMyNickname())} 로 부른 메시지가 없습니다` : '로그인하면 나를 부른 메시지를 모아 줍니다'}</div>`}`;
    this.tray.hidden = false;
  }

  // ── 컨텍스트 메뉴 (CHT-03) · 프로필 (CHT-04) ─

  onContextMenu(e) {
    const line = e.target.closest('.chat-line[data-uid]');
    if (!line) return;
    e.preventDefault();
    const uid = line.dataset.uid;
    const nick = line.dataset.nick;
    const text = line.querySelector('.text')?.textContent || '';
    const amount = line.querySelector('.amount')?.textContent || '';
    const highlighted = (chatPrefs().highlightedUsers || []).includes(uid);
    const items = [
      ['copy-text', '메시지 복사'],
      ['copy-nick', '닉네임 복사'],
      ...(amount ? [['copy-amount', `금액 복사 (${amount})`]] : []),
      ['copy-id', '사용자 ID 복사'],
      ['-'],
      ['mention', `@${nick} 멘션`],
      ['profile', '프로필 보기'],
      ['pin', '메시지 고정'],
      ['highlight', highlighted ? '하이라이트 해제' : '이 사용자 하이라이트'],
      ['-'],
      ['block', '사용자 차단', 'danger'],
    ];
    showMenu(e.clientX, e.clientY, items, (action) => {
      switch (action) {
        case 'copy-text': copy(text); break;
        case 'copy-nick': copy(nick); break;
        case 'copy-amount': copy(amount); break;
        case 'copy-id': copy(uid); break;
        case 'mention': this.mention(nick); break;
        case 'profile': this.showProfile(uid, nick); break;
        case 'pin': this.pin(line); break;
        case 'highlight': this.toggleHighlight(uid); break;
        case 'block': this.block(uid, nick); break;
        default: break;
      }
    });
  }

  mention(nick) {
    if (this.input.disabled) { toast('지금은 채팅을 보낼 수 없습니다'); return; }
    this.input.value = `${`@${nick} ${this.input.value}`.trimEnd()} `;
    this.input.focus();
  }

  pin(line) {
    const clone = line.cloneNode(true);
    clone.classList.remove('mention', 'highlighted');
    clone.removeAttribute('data-uid');
    clone.removeAttribute('data-id');
    this.pinBar.innerHTML = `<span class="pin-icon">${ICON.pin}</span><div class="pin-body"></div><button class="icon-btn notice-x" data-unpin title="고정 해제">✕</button>`;
    this.pinBar.querySelector('.pin-body').append(clone);
    this.pinBar.hidden = false;
  }

  toggleHighlight(uid) {
    const list = new Set(chatPrefs().highlightedUsers || []);
    if (list.has(uid)) list.delete(uid); else list.add(uid);
    updateChatPrefs({ highlightedUsers: [...list] });
    for (const el of this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(uid)}"]`)) el.classList.toggle('highlighted', list.has(uid));
  }

  block(uid, nick) {
    if (!uid) return;
    const p = chatPrefs();
    const blocked = [...new Set([...(p.blockedUsers || []), uid])];
    updateChatPrefs({ blockedUsers: blocked, blockedUserNames: { ...(p.blockedUserNames || {}), [uid]: nick } });
    for (const el of this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(uid)}"]`)) el.remove();
    toast(`${nick} 님을 차단했습니다 — 설정 > 채팅에서 풀 수 있어요`, 2500);
  }

  showProfile(uid, nick) {
    const lines = [...this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(uid)}"]`)];
    const first = lines[lines.length - 1];
    const badges = first ? first.querySelectorAll('img.badge') : [];
    const role = first ? first.dataset.role : '';
    const roleLabel = { streamer: '스트리머', manager: '매니저', channelManager: '채널 관리자' }[role] || '시청자';
    const recent = lines.slice(-10).reverse().map((l) => `<li><time>${esc(l.querySelector('.chat-time')?.textContent || '')}</time> ${l.querySelector('.text')?.innerHTML || ''}</li>`).join('');
    const scrim = document.createElement('div');
    scrim.className = 'sheet-scrim';
    scrim.innerHTML = `
      <div class="sheet profile-sheet" role="dialog" aria-label="${esc(nick)} 프로필" tabindex="-1">
        <div class="profile-head"><h2>${esc(nick)}</h2><span class="role-pill">${roleLabel}</span><div class="grow"></div><span class="profile-badges">${[...badges].map((b) => b.outerHTML).join('')}</span></div>
        <p class="profile-sub">이 화면에서 본 메시지 ${lines.length}개</p>
        <ul class="profile-recent">${recent || '<li class="muted">최근 메시지가 없습니다</li>'}</ul>
        <div class="sheet-actions">
          <button class="btn" data-p="copy">ID 복사</button>
          <button class="btn" data-p="highlight">${(chatPrefs().highlightedUsers || []).includes(uid) ? '하이라이트 해제' : '하이라이트'}</button>
          <button class="btn" data-p="mention">멘션</button>
          <button class="btn danger" data-p="block">차단</button>
          <button class="btn primary" data-p="close">닫기</button>
        </div>
      </div>`;
    document.body.append(scrim);
    scrim.querySelector('.sheet').focus();
    const close = () => scrim.remove();
    scrim.addEventListener('click', (e) => {
      if (e.target === scrim) { close(); return; }
      const b = e.target.closest('[data-p]');
      if (!b) return;
      if (b.dataset.p === 'copy') copy(uid);
      if (b.dataset.p === 'highlight') this.toggleHighlight(uid);
      if (b.dataset.p === 'mention') this.mention(nick);
      if (b.dataset.p === 'block') this.block(uid, nick);
      close();
    });
    scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  }

  // ── 이모티콘 (CHT-01) · 자동완성 (CHT-02) ───

  async loadEmoticons(channelId) {
    if (!channelId) return;
    if (emoticonCache.has(channelId)) { this.emoticons = emoticonCache.get(channelId); return; }
    try {
      const packs = await api.chatEmoticons(channelId);
      emoticonCache.set(channelId, packs);
      if (this.channelId === channelId) this.emoticons = packs;
    } catch { this.emoticons = []; }
  }

  /** 이모티콘 팩 + 채팅에서 본 이모티콘 팩. */
  packs() {
    const seen = [...this.seenEmoji.entries()].slice(-200).reverse()
      .map(([id, url]) => ({ emoticonId: id, emoticonName: id, imageUrl: url }));
    return seen.length
      ? [...this.emoticons, { emoticonPackId: 'seen', emoticonPackName: '채팅에서 본 이모티콘', emoticons: seen }]
      : this.emoticons;
  }

  allEmoticons() {
    const out = [];
    for (const p of this.packs()) {
      for (const e of p.emoticons || []) out.push({ id: e.emoticonId, name: e.emoticonName || e.emoticonId, url: e.imageUrl, pack: p.emoticonPackName });
    }
    return out;
  }

  togglePicker() {
    if (!this.picker.hidden) { this.picker.hidden = true; return; }
    this.pickerPack = this.pickerPack || 0;
    this.picker.innerHTML = `
      <input type="search" class="emoji-search" placeholder="이모티콘 검색" autocomplete="off">
      <div class="emoji-tabs"></div>
      <div class="emoji-grid"></div>`;
    this.picker.hidden = false;
    this.renderPicker();
    this.picker.querySelector('.emoji-search').focus();
  }

  renderPicker() {
    const tabs = this.picker.querySelector('.emoji-tabs');
    const grid = this.picker.querySelector('.emoji-grid');
    if (!tabs || !grid) return;
    const packs = this.packs();
    if (!packs.length) { grid.innerHTML = '<div class="tray-empty">이 채널은 이모티콘 목록을 주지 않습니다 — 채팅에 이모티콘이 보이면 여기에 모입니다</div>'; return; }
    const q = (this.picker.querySelector('.emoji-search').value || '').trim().toLowerCase();
    if (this.pickerPack >= packs.length) this.pickerPack = 0;
    tabs.hidden = Boolean(q);
    tabs.innerHTML = packs.map((p, i) => {
      const icon = safeUrl(p.emoticonPackImageUrl);
      return `<button type="button" class="emoji-tab${i === this.pickerPack ? ' active' : ''}" data-pack="${i}" title="${esc(p.emoticonPackName)}">${icon ? `<img src="${esc(icon)}" alt="" referrerpolicy="no-referrer">` : esc(p.emoticonPackName.slice(0, 2))}</button>`;
    }).join('');
    const items = q
      ? this.allEmoticons().filter((e) => e.id.toLowerCase().includes(q) || String(e.name).toLowerCase().includes(q)).slice(0, 120)
      : (packs[this.pickerPack].emoticons || []).map((e) => ({ id: e.emoticonId, name: e.emoticonName || e.emoticonId, url: e.imageUrl }));
    grid.innerHTML = items.length
      ? items.map((e) => `<button type="button" class="emoji-cell" data-emoji="${esc(e.id)}" title="${esc(e.name)}"><img src="${esc(safeUrl(e.url))}" alt="${esc(e.name)}" loading="lazy" referrerpolicy="no-referrer"></button>`).join('')
      : '<div class="tray-empty">찾는 이모티콘이 없습니다</div>';
  }

  onPickerClick(e) {
    const tab = e.target.closest('[data-pack]');
    if (tab) { this.pickerPack = Number(tab.dataset.pack); this.renderPicker(); return; }
    const cell = e.target.closest('[data-emoji]');
    if (cell) this.insertAtCaret(`{:${cell.dataset.emoji}:}`);
  }

  insertAtCaret(text) {
    const el = this.input;
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    el.value = el.value.slice(0, start) + text + el.value.slice(end);
    const pos = start + text.length;
    el.focus();
    el.setSelectionRange(pos, pos);
  }

  /** 커서 앞 토큰(`:abc` 또는 `@abc`). */
  currentToken() {
    const el = this.input;
    const upto = el.value.slice(0, el.selectionStart ?? el.value.length);
    const m = /(^|\s)([:@])([^\s:@]{1,30})$/.exec(upto);
    return m ? { kind: m[2], query: m[3], start: upto.length - m[3].length - 1 } : null;
  }

  updateAutocomplete() {
    const tok = this.currentToken();
    if (!tok) { this.autocomplete.hidden = true; return; }
    const q = tok.query.toLowerCase();
    let items = [];
    if (tok.kind === ':') {
      items = this.allEmoticons().filter((e) => e.id.toLowerCase().startsWith(q) || String(e.name).toLowerCase().includes(q)).slice(0, 8)
        .map((e) => ({ value: `{:${e.id}:}`, html: `<img src="${esc(safeUrl(e.url))}" alt="" referrerpolicy="no-referrer"><span>${esc(e.name)}</span>` }));
    } else {
      items = [...this.recentNicks.entries()].sort((a, b) => b[1] - a[1]).map(([n]) => n)
        .filter((n) => n.toLowerCase().includes(q)).slice(0, 8)
        .map((n) => ({ value: `@${n} `, html: `<span>@${esc(n)}</span>` }));
    }
    this.acItems = items;
    this.acIndex = 0;
    this.acToken = tok;
    this.autocomplete.innerHTML = items.map((it, i) => `<div class="ac-item${i === 0 ? ' active' : ''}" data-ac="${esc(it.value)}">${it.html}</div>`).join('');
    this.autocomplete.hidden = items.length === 0;
  }

  applyAutocomplete(value) {
    const tok = this.acToken;
    const el = this.input;
    if (!tok) return;
    const caret = el.selectionStart ?? el.value.length;
    el.value = el.value.slice(0, tok.start) + value + el.value.slice(caret);
    const pos = tok.start + value.length;
    el.setSelectionRange(pos, pos);
    this.autocomplete.hidden = true;
    el.focus();
  }

  onInputKey(e) {
    if (this.autocomplete.hidden) return;
    const items = this.autocomplete.querySelectorAll('.ac-item');
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      this.acIndex = (this.acIndex + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items.forEach((el, i) => el.classList.toggle('active', i === this.acIndex));
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      this.applyAutocomplete(this.acItems[this.acIndex].value);
    } else if (e.key === 'Escape') {
      this.autocomplete.hidden = true;
    }
  }
}

function copy(text) {
  if (!text) return;
  navigator.clipboard.writeText(text).then(() => toast('복사했습니다', 1000), () => toast('복사하지 못했습니다'));
}

/** 공용 컨텍스트 메뉴 — [action, label, tone?] 목록, '-' 는 구분선. */
export function showMenu(x, y, items, onPick) {
  document.querySelector('.ctx-menu')?.remove();
  const menu = document.createElement('div');
  menu.className = 'ctx-menu';
  menu.setAttribute('role', 'menu');
  menu.innerHTML = items.map(([a, label, tone]) => (a === '-'
    ? '<div class="ctx-sep"></div>'
    : `<button class="ctx-item${tone ? ` ${tone}` : ''}" role="menuitem" data-ctx="${esc(a)}">${esc(label)}</button>`)).join('');
  document.body.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(x, window.innerWidth - r.width - 8)}px`;
  menu.style.top = `${Math.min(y, window.innerHeight - r.height - 8)}px`;
  const outside = (e) => { if (!menu.contains(e.target)) close(); };
  function close() {
    menu.remove();
    document.removeEventListener('mousedown', outside, true);
    window.removeEventListener('blur', close);
  }
  setTimeout(() => document.addEventListener('mousedown', outside, true), 0);
  window.addEventListener('blur', close);
  menu.addEventListener('click', (e) => {
    const item = e.target.closest('[data-ctx]');
    if (!item) return;
    close();
    onPick(item.dataset.ctx);
  });
  menu.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}
