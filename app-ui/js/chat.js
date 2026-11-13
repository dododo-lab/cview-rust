// 채팅 패널 — `chat://event` (Rust ChatEventDto) 를 그린다. 원본 ChatPanelView 와 부속:
// CHT-01 이모티콘 선택기, CHT-02 자동완성(:이모티콘·@멘션), CHT-03 컨텍스트 메뉴, CHT-04 프로필 시트,
// CHT-05 차단, CHT-06 금지어 필터, CHT-08 멘션 강조·트레이, CHT-09 하이라이트, CHT-10 메시지 고정·공지 숨기기,
// CHT-11 자동 스크롤·"새 메시지" 버튼·읽음 구분선(⌘J), CHT-12 지우기(⇧⌘K)·재연결, CHT-14 표시 옵션,
// CHT-16 후원 필터, CHT-20 입력 상태 배너, CHT-25 닉네임 말풍선(chat-user-pop.js)·이 사람 채팅만 보기,
// 줄 동작 막대·키보드 줄 고르기·채팅 검색(chat-actions.js), 입력 기록(↑/↓)·남은 글자 수.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast, imageSrc, dismiss, renderContent, emojiUrl } from './dom.js';

export { renderContent };
import { DonationRanking, eventCardHtml, eventClasses, EVENT_ICONS } from './donations.js';
import { installChatExtras } from './chat-extras.js';
import { nickColorClass } from './nick-color.js';
import { avatarHtml } from './chat-avatar.js';
import { kbd } from './platform.js';
import { chatFollower, trimFront } from './chat-follow.js';
import { openUserPop, closeUserPop } from './chat-user-pop.js';
import { installChatActions, lineCopyText } from './chat-actions.js';
import { ChatComposer } from './chat-composer.js';
import {
  chatPrefs, onChatPrefs, updateChatPrefs, passesFilters, mentionsMe, getMyNickname,
} from './chat-prefs.js';

const ICON = {
  at: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M16 12v1.5a2.5 2.5 0 0 0 5 0V12a9 9 0 1 0-3.5 7.1"/></svg>',
  trash: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>',
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>',
  down: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5.5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/></svg>',
  pin: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-5-5 5-1-1 5-5-5-4 2-2 5 1 4-4z"/></svg>',
};


// 닉네임 색을 바꾸는 역할만 클래스로 붙인다. 역할 문자열을 그대로 클래스로 쓰면 일반 시청자
// `viewer` 가 시청 화면 섹션의 `.viewer`(position:absolute; inset:0) 와 겹쳐 닉네임이 창 전체를 덮는다.
const ROLE_CLASSES = new Set(['streamer', 'manager', 'channelManager']);
// 창이 숨은 동안 채팅 줄을 모아 붙이는 간격(보일 때는 다음 프레임, 늦어도 250ms).
const HIDDEN_FLUSH_MS = 1000;
/** 입장 때 받은 지난 기록(ev.recent) 메시지 — append 가 표시하고 flushLines 가 읽는다. */
const RECENT = new WeakSet();
const BLIND_TEXT = '관리자에 의해 삭제된 메시지입니다.';

function roleClass(role) {
  return ROLE_CLASSES.has(role) ? ` ${role}` : '';
}

function timeText(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** 이모티콘만 있는 메시지인가 — 말풍선 없이 크게 그린다(카카오톡의 이모티콘처럼). 순수 함수. */
export function isEmoteOnly(content, emojis) {
  const map = emojis || {};
  let found = false;
  const rest = String(content ?? '').replace(/\{:([^{}:]+):\}/g, (all, key) => { if (Object.hasOwn(map, key) && emojiUrl(map[key])) { found = true; return ''; } return all; });
  return found && !rest.trim();
}

/**
 * 말풍선 묶음 열쇠 — 같은 사람이 같은 분에 보낸 일반 메시지는 한 묶음(카카오톡처럼 프로필·이름은 첫 줄에만,
 * 시간은 마지막 줄에만). 시스템·공지·후원 같은 이벤트는 묶지 않는다(null). 순수 함수.
 */
export function groupKey(m) {
  if (!m || m.kind === 'system' || m.kind === 'notice' || !m.userId || eventClasses(m).length) return null;
  return `${m.userId}|${timeText(m.timestamp)}`;
}

/**
 * 채팅 한 줄 — 단일 채팅(ChatPanel)과 멀티채팅 통합 목록(multichat.js mergedLinesHtml)이 함께 쓴다.
 * @param {{ cont?: boolean, next?: boolean, merged?: boolean, tag?: string, src?: string, style?: string, g?: string, avatars?: string }} [grp]
 *   cont — 앞 줄과 같은 묶음, next — 다음 줄도 같은 묶음.
 *   통합 목록만: merged — 통합 줄(.merged-line), tag — 출처(채널) 태그 HTML(이미 이스케이프됨), src — 관리자 삭제(blinded)를 찾을 출처 키(data-src),
 *   style — 줄의 색 변수(--mc-color 등), g — 다음 묶음과 이어 볼 묶음 열쇠(data-g), avatars — 아바타 모드(없으면 설정값).
 */
export function lineHtml(m, prefs, grp = {}) {
  const badges = (m.badges || [])
    .map((u) => safeUrl(u))
    .filter(Boolean)
    .map((u) => `<img decoding="async" class="badge" src="${esc(imageSrc(u, 'static'))}" alt="" referrerpolicy="no-referrer">`)
    .join('');
  const style = m.titleColor && /^#[0-9a-f]{3,8}$/i.test(m.titleColor) ? ` style="color:${m.titleColor}"` : '';
  if (m.kind === 'system') {
    return `<div class="chat-line system" data-id="${esc(m.id)}"><span class="sys-pill">${esc(m.content)}</span></div>`;
  }
  const classes = ['chat-line'];
  const tag = grp.tag || '';
  if (grp.merged || grp.src) classes.push('merged-line');
  if (m.isBlinded) classes.push('blinded');
  if (m.kind === 'notice') classes.push('notice-line');
  if (prefs.highlightMentions && mentionsMe(m)) classes.push('mention');
  if ((prefs.highlightedUsers || []).includes(m.userId)) classes.push('highlighted');
  const time = `<time class="chat-time">${esc(timeText(m.timestamp))}</time>`;
  const isMe = Boolean(getMyNickname()) && m.nickname === getMyNickname();
  const nick = `<span class="nick${roleClass(m.role)}${nickColorClass(m.nickname, m.role, isMe)}"${style}>${esc(m.nickname || '익명')}</span>`;
  const text = `<span class="text">${renderContent(m.content, m.emojis, prefs.emoticonEnabled)}</span>`;
  let data = `data-id="${esc(m.id)}" data-uid="${esc(m.userId || '')}" data-nick="${esc(m.nickname || '')}" data-role="${esc(m.role || '')}"`;
  if (grp.src) data += ` data-src="${esc(grp.src)}"`;
  if (grp.g) data += ` data-g="${esc(grp.g)}"`;
  if (grp.style) data += ` style="${esc(grp.style)}"`;
  const evClasses = eventClasses(m);
  if (evClasses.length) {
    classes.push(...evClasses);
    const body = m.content && m.content.trim() ? renderContent(m.content, m.emojis, prefs.emoticonEnabled) : '';
    return `<div class="${classes.join(' ')}" ${data}>${eventCardHtml(m, { tag, time, badges: badges ? `<span class="badges">${badges}</span>` : '', nick, content: body })}</div>`;
  }
  if (m.kind === 'notice') {
    return `<div class="${classes.join(' ')}" ${data}><span class="notice-icon">${EVENT_ICONS.megaphone}</span><div class="notice-body"><div class="notice-from">${tag}${nick}<span class="notice-tag">공지</span>${time}</div>${text}</div></div>`;
  }
  // 말풍선 구조 — 아바타 · [머리(배지·닉네임) / 줄(말풍선 + 시간)]. 묶음의 이어진 줄은 CSS 가 아바타·머리를 감춘다.
  const me = getMyNickname();
  if (me && m.nickname === me) classes.push('own');
  classes.push('msg');
  if (grp.cont) classes.push('cont');
  if (grp.next) classes.push('has-next');
  if (prefs.emoticonEnabled !== false && isEmoteOnly(m.content, m.emojis)) classes.push('emote-only');
  const mentionTag = classes.includes('mention') ? '<span class="msg-mention">나를 언급</span>' : '';
  // 아바타 — 프로필 사진(찾으면) 또는 사용자 ID 로 정해지는 랜덤 아이콘(chat-avatar.js). 내 말은 CSS 가 감춘다.
  const avatar = avatarHtml(m, grp.avatars || prefs.chatAvatars || 'photo');
  return `<div class="${classes.join(' ')}" ${data}>${avatar}<span class="msg-main"><span class="msg-head">${tag}<span class="badges">${badges}</span>${nick}${mentionTag}</span><span class="msg-row">${text}${time}</span></span></div>`;
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

/**
 * 로그인 상태가 바뀌면(로그인·만료·로그아웃) 열린 채팅을 새 신원으로 다시 연결한다 — 비로그인으로 붙은 채팅은
 * 로그인한 뒤에도 익명 연결이라 보내기가 되지 않았다(닫았다 열어야 했다). 입력창 상태도 함께 고친다.
 */
export function refreshChatAuth() {
  for (const p of PANELS) {
    p.updateInput();
    const live = p.chatChannelId && p.active;
    if (live && p.authAtConnect !== p.isLoggedIn()) p.connect(p.chatChannelId, p.channelId);
  }
}

/**
 * 네트워크가 돌아오면 연결에 실패한 채 멈춘 채팅을 다시 연결한다 — 끊긴 동안 연결 요청이 실패하면 패널이
 * '연결 실패'로 남고 아무도 다시 시도하지 않아, 방송은 돌아와도 채팅은 "다시 연결"을 눌러야 했다.
 * 엔진이 스스로 재시도 중인 패널(재연결 중)과 사용자가 끊은 패널(연결 끊김)은 건드리지 않는다.
 */
/** 이 상자(.chat)를 그리는 채팅 패널 — 멀티라이브 통합 목록이 선택 패인 채팅의 입력부·로그인 상태를 빌린다. */
export function panelOf(root) {
  for (const p of PANELS) if (p.root === root) return p;
  return null;
}

export function reconnectFailedChats() {
  for (const p of PANELS) {
    if (p.active && p.chatChannelId && p.stateName === 'failed') p.connect(p.chatChannelId, p.channelId);
  }
}

export class ChatPanel {
  /**
   * @param {HTMLElement} root `.chat` 패널
   * @param {{ isLoggedIn: () => boolean, onHighlight?: (message) => void, onLogin?: () => void, mentionNotify?: boolean }} options
   *   onHighlight — 실시간(최근 기록 제외)으로 들어온 후원·구독 메시지(스트림 알림용)
   *   mentionNotify — 멘션 시스템 알림을 이 패널에서 띄우는가(기본: 기본 세션 패널만)
   */
  constructor(root, { isLoggedIn, onHighlight = null, onLogin = null, session = null, mentionNotify = !session }) {
    this.root = root;
    // 세션 키 — null 이면 기본 세션(chat://event), 있으면 추가 세션(chat://session-event, 멀티채팅·멀티라이브).
    this.session = session;
    this.mentionNotify = mentionNotify;
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
    this.inputHooks = []; // 보내기 가능 상태가 바뀔 때(로그인·연결·보내기 권한) — 통합 채팅 입력이 따라 한다
    this.sendHooks = [];
    PANELS.add(this);
    // 정리할 것들 — 멀티채팅에서 패널을 닫으면 destroy() 가 푼다.
    this.cleanups = [];
    this.buildChrome();
    root.querySelector('.chat-input').addEventListener('submit', (e) => {
      e.preventDefault();
      if (this.autocomplete.hidden) this.send();
    });
    const unlisten = session
      ? listen('chat://session-event', (p) => { if (p && p.session === session) this.onEvent(p.event); })
      : listen(Events.chat, (ev) => this.onEvent(ev));
    this.cleanups.push(() => Promise.resolve(unlisten).then((un) => typeof un === 'function' && un()).catch(() => {}));
    this.cleanups.push(onChatPrefs(() => this.applyPrefs()));
    this.applyPrefs();
    this.updateInput();
    installChatExtras(this);
  }

  // ── 머리·입력 부속 ───────────────────────────

  buildChrome() {
    const header = this.root.querySelector('.chat-header');
    header.insertAdjacentHTML('beforeend', `
      <div class="grow"></div>
      <div class="chat-tools" inert>
        <div class="chat-tools-inner">
          <button class="icon-btn chat-hbtn mention-btn" title="나를 부른 메시지" aria-label="멘션">${ICON.at}<span class="mention-count" hidden></span></button>
          <button class="icon-btn chat-hbtn chat-tool-extra" data-chat="clear" title="${kbd('채팅 지우기 (⇧⌘K)')}" aria-label="채팅 지우기">${ICON.trash}</button>
          <button class="icon-btn chat-hbtn" data-chat="reconnect" title="다시 연결" aria-label="다시 연결">${ICON.refresh}</button>
        </div>
      </div>
      <button class="icon-btn chat-hbtn chat-tools-toggle" data-chat="tools" title="채팅 도구" aria-label="채팅 도구 펼치기" aria-expanded="false">${ICON.more}<span class="chat-tools-dot" hidden></span></button>`);
    // 기능 아이콘(멘션·읽기 폭·통계·내보내기·지우기·다시 연결)은 평소 '⋯' 뒤에 접어 두고, 누르면 왼쪽으로 미끄러져 나온다.
    this.tools = header.querySelector('.chat-tools');
    this.toolsToggle = header.querySelector('.chat-tools-toggle');
    this.toolsDot = header.querySelector('.chat-tools-dot');
    // 다 펼쳐진 뒤에만 넘침을 풀어 서랍 안 버튼의 메뉴(멀티채팅 보기 등)가 잘리지 않게 한다.
    this.tools.addEventListener('transitionend', (e) => {
      if (e.target === this.tools && this.tools.classList.contains('open')) this.tools.classList.add('settled');
    });
    header.addEventListener('click', (e) => {
      const b = e.target.closest('[data-chat], .mention-btn');
      if (!b) return;
      if (b.dataset.chat === 'tools') { this.setToolsOpen(!this.tools.classList.contains('open')); return; }
      if (b.classList.contains('mention-btn')) this.toggleMentionTray();
      else if (b.dataset.chat === 'clear') this.clear(true);
      else if (b.dataset.chat === 'reconnect') this.reconnect();
    });
    this.mentionCount = header.querySelector('.mention-count');
    // 연결 상태는 바뀔 때 화면 읽기가 알린다. 끊기거나 실패하면 '⋯' 서랍을 열지 않아도 바로 다시 연결할 수 있게
    // 상태 옆에 버튼을 꺼내 둔다(예전엔 '연결 실패' 글자만 보이고 다시 연결은 서랍 안에 숨어 있었다).
    this.state.setAttribute('role', 'status');
    this.retry = document.createElement('button');
    this.retry.type = 'button';
    this.retry.className = 'link-btn chat-retry';
    this.retry.textContent = '다시 연결';
    this.retry.hidden = true;
    this.state.after(this.retry);
    this.retry.addEventListener('click', () => this.reconnect());

    this.pinBar = document.createElement('div');
    this.pinBar.className = 'chat-pin';
    this.pinBar.hidden = true;
    this.notice.after(this.pinBar);
    this.pinBar.addEventListener('click', (e) => { if (e.target.closest('[data-unpin]')) this.pinBar.hidden = true; });

    // 이 사람 채팅만 보기(CHT-25) — 고정 줄 아래에 무엇을 거르는지와 풀기 버튼.
    this.soloUid = null;
    this.soloBar = document.createElement('div');
    this.soloBar.className = 'chat-solo';
    this.soloBar.hidden = true;
    this.pinBar.after(this.soloBar);
    this.soloBar.addEventListener('click', (e) => { if (e.target.closest('[data-solo-off]')) this.setSolo(null); });
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
    this.follower = chatFollower(this.list, () => chatPrefs().autoScroll);

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

    // 입력부(이모티콘 선택기·자동완성·글자 수·입력 기록)는 ChatComposer 가 맡는다 — 멀티채팅 통합 입력도 같은 것을 쓴다.
    // 이모티콘·닉네임 목록은 이 패널의 것을 읽는다.
    this.composer = new ChatComposer(form, { source: () => this, host: this.root });
    this.cleanups.push(() => this.composer.destroy());
    const onDocClick = (e) => {
      if (!this.tray.contains(e.target) && !e.target.closest('.mention-btn')) this.tray.hidden = true;
    };
    document.addEventListener('click', onDocClick);
    this.cleanups.push(() => document.removeEventListener('click', onDocClick));

    this.list.addEventListener('contextmenu', (e) => this.onContextMenu(e));
    this.header = header;
    this.list.addEventListener('dblclick', (e) => {
      if (e.target.closest('.nick, .chat-avatar')) return; // 닉네임은 말풍선이 맡는다
      const line = e.target.closest('.chat-line[data-uid]');
      if (line && line.dataset.uid) this.showProfile(line.dataset.uid, line.dataset.nick);
    });
    // 닉네임·아바타를 누르면 그 사람 말풍선(CHT-25). 고정 줄 복제본은 data-uid 를 지워 두어 걸리지 않는다.
    this.list.addEventListener('click', (e) => {
      const anchor = e.target.closest('.nick, .chat-avatar');
      const line = anchor && anchor.closest('.chat-line[data-uid]');
      if (!line || !line.dataset.uid || !this.list.contains(line)) return;
      openUserPop(this, anchor, line);
    });
    this.cleanups.push(() => closeUserPop(this));
    const onDocKey = (e) => {
      if (!this.active || !this.root.offsetParent) return;
      // 보이는 패널이 여럿이면(멀티채팅 격자) 한 패널만 받는다 — 초점이 든 패널, 없으면 첫 패널.
      // 패널마다 받으면 ⌘J 가 공유 설정을 N 번 뒤집어(짝수면 그대로) 알림만 N 개 떴다.
      const shown = [...PANELS].filter((p) => p.active && p.root.offsetParent);
      if ((shown.find((p) => p.root.contains(document.activeElement)) || shown[0]) !== this) return;
      if (e.metaKey && e.shiftKey && e.key.toLowerCase() === 'k') { e.preventDefault(); this.clear(true); }
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === 'j') {
        e.preventDefault();
        const on = !chatPrefs().autoScroll;
        updateChatPrefs({ autoScroll: on });
        toast(on ? '자동 스크롤 켬' : '자동 스크롤 끔', 1200);
      }
    };
    document.addEventListener('keydown', onDocKey);
    this.cleanups.push(() => document.removeEventListener('keydown', onDocKey));
    installChatActions(this);
  }

  /** 패널을 버린다(멀티채팅에서 채널 닫기) — 이벤트 구독·문서 리스너·설정 구독·패널 목록에서 뺀다. */
  async destroy() {
    await this.disconnect();
    for (const fn of this.cleanups.splice(0)) { try { await fn(); } catch { /* 무시 */ } }
    PANELS.delete(this);
  }

  // 글자 크기·줄 간격·시간·배지 표시는 applyChatSkin(chat-extras)이 디자인과 함께 입힌다(멀티채팅 통합 목록과 공유).
  applyPrefs() {
    const p = chatPrefs();
    this.maxLines = Math.min(2000, Math.max(50, Number(p.maxVisibleMessages) || 300));
  }

  // ── 연결 ───────────────────────────────────

  // 연결 순번 — 빠른 채널·패인 전환 때 늦게 끝난 connect/disconnect 가 새 연결 상태를 덮지 않게.
  async connect(chatChannelId, channelId) {
    const seq = this.connSeq = (this.connSeq || 0) + 1;
    if (!this.session) for (const p of PANELS) if (p !== this && !p.session) p.active = false;
    this.clear();
    this.active = true;
    if (this.channelId !== channelId) { this.seenEmoji.clear(); this.setSolo(null); closeUserPop(this); }
    this.chatChannelId = chatChannelId;
    this.channelId = channelId;
    this.mentions = [];
    this.renderMentionCount();
    this.noticeDismissed = false;
    this.pinBar.hidden = true;
    this.ranking.load(channelId);
    this.loadEmoticons(channelId);
    this.setState('connecting');
    this.authAtConnect = this.isLoggedIn();
    if (!chatChannelId) {
      this.setState('failed');
      this.system('채팅 채널 정보가 없습니다');
      return;
    }
    try {
      const res = await api.chatConnect(chatChannelId, channelId, this.session);
      if (seq !== this.connSeq) return;
      this.canSend = Boolean(res && res.canSend);
    } catch (e) {
      if (seq !== this.connSeq) return;
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
    const seq = this.connSeq = (this.connSeq || 0) + 1;
    this.active = false;
    this.connected = false;
    this.canSend = false;
    this.ranking.clear();
    // 후원 읽어 주기(TTS) 대기열은 전역이다 — 연결된 채팅이 하나도 없으면 남은 것을 멈춘다(화면을 떠나도 계속 읽었다)
    if (window.speechSynthesis && ![...PANELS].some((p) => p.connected)) speechSynthesis.cancel();
    try { await api.chatDisconnect(this.session); } catch { /* 무시 */ }
    if (seq !== this.connSeq) return;
    this.setState('disconnected');
    this.updateInput();
  }

  clear(byUser = false) {
    this.clearLineSelection?.();
    this.pendingLines = [];
    this.lastGroup = null;
    this.list.innerHTML = `<div class="empty">${byUser ? '채팅을 지웠습니다' : '메시지 대기 중…'}</div>`;
    if (!byUser) this.notice.hidden = true;
    this.unread = 0;
    this.renderJump();
    // 검색 — 채널이 바뀌면 닫고, 지우기(/clear·⇧⌘K)면 줄 수만 다시 센다(예전엔 지운 줄의 'N개'가 남았다).
    if (this.searchQuery) { if (byUser) this.refreshSearch?.(); else this.closeSearch?.(); }
  }

  setState(name, detail) {
    const [label, cls] = STATE_LABEL[name] || [name, ''];
    this.state.textContent = name === 'reconnecting' && detail ? `${label} (${detail})` : label;
    this.state.className = `chat-state ${cls}`;
    this.stateName = name;
    this.connected = name === 'connected';
    this.retry.hidden = !(this.active && this.chatChannelId && (name === 'failed' || name === 'disconnected'));
    this.updateInput();
  }

  /** CHT-20 입력 상태 배너 — 왜 보낼 수 없는지 알려 준다. */
  updateInput(reason = null) {
    const loggedIn = this.isLoggedIn();
    const enabled = loggedIn && this.connected && this.canSend;
    this.input.disabled = !enabled;
    this.sendButton.disabled = !enabled;
    this.composer.setEnabled(enabled);
    this.input.placeholder = loggedIn ? '채팅을 입력하세요…' : '로그인 후 채팅 참여 가능';
    let html = '';
    if (!reason && this.restrictedUntil > Date.now()) reason = this.restrictText;
    if (reason) html = esc(reason);
    else if (!loggedIn) html = '로그인하면 채팅에 참여할 수 있어요 <button class="link-btn" data-login>로그인</button>';
    else if (this.connected && !this.canSend) html = '이 채팅방에서는 지금 채팅을 보낼 수 없습니다 (팔로워·구독자 전용이거나 제한 중)';
    this.banner.innerHTML = html;
    this.banner.hidden = !html;
    this.banner.classList.toggle('warn', Boolean(reason));
    this.banner.setAttribute('role', reason ? 'alert' : 'status');
    for (const fn of this.inputHooks) {
      try { fn(); } catch (e) { console.error(e); }
    }
  }

  // ── 메시지 ─────────────────────────────────

  get atBottom() {
    return this.list.scrollHeight - this.list.scrollTop - this.list.clientHeight < 40;
  }

  /**
   * 메시지 추가 — 거르기·멘션 알림 같은 장부 일은 바로, DOM 넣기는 프레임마다 한 번으로 묶는다.
   * 붐비는 채널은 초당 수십 번 이벤트가 오는데, 매번 넣고 스크롤 위치를 읽고 쓰면 그때마다 레이아웃을 다시 계산한다.
   */
  /**
   * @param {object[]} messages
   * @param {{ recent?: boolean }} [opts] recent — 입장 때 받은 지난 기록. 그리기만 하고 '여기까지 읽음'·새 메시지 수·
   *   멘션 알림·TTS·채팅 통계에는 넣지 않는다(지금 막 온 글이 아니다).
   */
  append(messages, { recent = false } = {}) {
    if (!messages || !messages.length) return;
    const prefs = chatPrefs();
    // localHidden — /mute 로 잠시 숨긴 닉네임(chat-extras.js). 붙인 뒤 지우면 한 프레임 보였다가 사라졌다.
    const hidden = this.localHidden;
    const shown = messages.filter((m) => passesFilters(m, prefs) && !(hidden && hidden(m)));
    if (!shown.length) return;
    for (const m of shown) {
      if (recent) RECENT.add(m);
      if (m.nickname && m.kind !== 'system') this.recentNicks.set(m.nickname, Date.now());
      for (const [id, url] of Object.entries(m.emojis || {})) {
        if (emojiUrl(url) && !this.seenEmoji.has(id)) this.seenEmoji.set(id, url);
      }
      if (prefs.highlightMentions && mentionsMe(m)) {
        this.mentions.push(m);
        // 멘션 알림 (SYS-05) — 앱이 뒤에 있을 때만, 라이브·멀티라이브 채팅에서만(멀티채팅 패널 여럿은 빼고).
        if (!recent && prefs.mentionNotify !== false && !document.hasFocus() && this.active && this.mentionNotify) {
          api.notifySend(`@${m.nickname} 님이 불렀어요`, String(m.content || '').slice(0, 140)).catch(() => {});
        }
      }
    }
    if (this.recentNicks.size > 400) {
      const keep = [...this.recentNicks.entries()].sort((a, b) => b[1] - a[1]).slice(0, 300);
      this.recentNicks = new Map(keep);
    }
    this.pendingLines = (this.pendingLines || []).concat(shown);
    // 안 보이는 동안 쌓여도 화면에 남을 만큼만 그린다.
    if (this.pendingLines.length > this.maxLines) this.pendingLines = this.pendingLines.slice(-this.maxLines);
    if (!this.flushScheduled) {
      this.flushScheduled = true;
      // 보이면 다음 프레임, 창이 숨어 rAF 가 멈춰도 타이머로 결국 그린다. 숨은 동안은 덜 자주(1초) 모아 붙인다 —
      // 보는 사람이 없어 늦어도 그만이고, 다시 보이면 rAF 가 바로 그린다.
      const run = () => this.flushLines();
      this.flushRaf = requestAnimationFrame(run);
      this.flushTimer = setTimeout(run, document.hidden ? HIDDEN_FLUSH_MS : 250);
    }
  }

  flushLines() {
    cancelAnimationFrame(this.flushRaf);
    clearTimeout(this.flushTimer);
    this.flushScheduled = false;
    const shown = this.pendingLines || [];
    this.pendingLines = [];
    if (!shown.length) return;
    const prefs = chatPrefs();
    const placeholder = this.list.querySelector('.empty');
    if (placeholder) placeholder.remove();
    const follow = prefs.autoScroll && this.follower.stuck;
    // 지난 기록(입장 직후 recent)은 읽을 글이 아니다 — 구분선·새 메시지 수·후크(TTS·통계)는 지금 온 줄만 센다.
    const fresh = shown.some((m) => RECENT.has(m)) ? shown.filter((m) => !RECENT.has(m)) : shown;
    if (!follow && fresh.length && !this.list.querySelector('.read-divider') && this.list.children.length) {
      this.list.insertAdjacentHTML('beforeend', '<div class="read-divider"><span>여기까지 읽음</span></div>');
      this.lastGroup = null; // 구분선 너머로는 묶지 않는다
    }
    // 말풍선 묶음 — 앞 묶음이 이번에 이어지면 그 마지막 줄의 시간을 감춘다(has-next).
    const keys = shown.map(groupKey);
    const cont = keys.map((k, i) => Boolean(k) && k === (i ? keys[i - 1] : this.lastGroup));
    if (cont[0]) this.list.lastElementChild?.classList.add('has-next');
    this.lastGroup = keys[keys.length - 1];
    this.list.insertAdjacentHTML('beforeend', shown.map((m, i) => lineHtml(m, prefs, { cont: cont[i], next: cont[i + 1] })).join(''));
    if (this.soloUid) this.markSolo();
    if (globalThis.__cviewPerf) globalThis.__cviewPerf.chatLines += shown.length;
    this.renderMentionCount();
    for (const fn of this.appendHooks) {
      try { fn(shown, fresh); } catch (e) { console.error(e); }
    }
    this.trimLines();
    if (follow) this.follower.snap();
    else if (fresh.length) { this.unread += fresh.length; this.renderJump(); }
  }

  /** 줄 상한(trimFront — 넘친 줄을 모았다가 한 번에 지운다). */
  trimLines() {
    if (!trimFront(this.list, this.maxLines)) return;
    // 맨 위에 남은 줄이 묶음 중간이면 아바타·이름을 다시 보인다
    this.list.firstElementChild?.classList.remove('cont');
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
    this.follower.stuck = true;
    this.list.scrollTop = this.list.scrollHeight;
    this.unread = 0;
    this.renderJump();
  }

  system(text) {
    this.append([{ id: `sys-${Date.now()}`, kind: 'system', content: text }]);
  }

  onEvent(ev) {
    if (!ev || !this.active) return;
    // 채널을 바꾼 뒤 늦게 도착한 이전 채널 이벤트(메시지·끊김 상태)는 버린다.
    if (ev.chatChannelId && this.chatChannelId && ev.chatChannelId !== this.chatChannelId) return;
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
        this.append(ev.messages, { recent: Boolean(ev.recent) });
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
      case 'blinded':
        this.blind(ev.messageId, ev.blindText);
        break;
      case 'kicked':
        this.system('채팅방에서 퇴장되었습니다');
        break;
      case 'penalized': {
        // 셸은 내 제한만 보낸다(state.rs forward_to_shell). 기간이 있으면 그동안 배너에 남기고, 끝나면 걷는다.
        const sec = Math.max(0, Number(ev.duration) || 0);
        const text = penaltyText(sec);
        this.system(text);
        this.restrictText = text;
        this.restrictedUntil = sec > 0 ? Date.now() + sec * 1000 : 0;
        clearTimeout(this.restrictTimer);
        if (sec > 0) this.restrictTimer = setTimeout(() => this.updateInput(), sec * 1000 + 50);
        this.updateInput(text);
        break;
      }
      case 'sendRejected':
        this.flashInput(ev.message || '메시지를 보내지 못했습니다');
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
    let failed = '';
    try {
      await api.chatSend(text, this.session);
      this.input.value = '';
      this.remember(text);
    } catch (e) {
      failed = (e && e.message) || '메시지를 보내지 못했습니다';
    } finally {
      this.updateInput();
      this.renderCount();
      this.input.focus();
    }
    // 실패는 이 패널 입력창 위에 — 멀티채팅처럼 패널이 여럿이면 토스트만으로는 어느 채팅인지 몰랐다. 쓴 글은 남긴다.
    if (failed) this.flashInput(failed);
  }

  /** 입력창 위 배너에 잠깐(5초) 알린다 — 전송 실패·서버 거절. 토스트도 함께 띄운다. */
  flashInput(text) {
    toast(text);
    this.updateInput(text);
    clearTimeout(this.bannerTimer);
    this.bannerTimer = setTimeout(() => this.updateInput(), 5000);
  }

  // ── 입력부(ChatComposer)에 넘긴다 — 줄 메뉴·말풍선·확장(chat-extras)·검사가 패널에서 부르던 이름 그대로 ──
  get emojiButton() { return this.composer.emojiButton; }
  get picker() { return this.composer.picker; }
  get autocomplete() { return this.composer.autocomplete; }
  get counter() { return this.composer.counter; }
  get history() { return this.composer.history; }
  remember(text) { this.composer.remember(text); }
  recall(dir) { return this.composer.recall(dir); }
  renderCount() { this.composer.renderCount(); }
  fitsInput(value) { return this.composer.fitsInput(value); }
  mention(nick) { this.composer.mention(nick); }
  insertAtCaret(text) { this.composer.insertAtCaret(text); }
  closePicker() { this.composer.closePicker(); }
  togglePicker() { this.composer.togglePicker(); }
  renderPicker() { this.composer.renderPicker(); }

  /** 관리자 삭제(블라인드) — 이미 붙은 줄은 글을 바꾸고, 아직 붙기 전(다음 프레임·가려진 창의 1초 묶음)이면
   *  붙일 글을 바꿔 둔다. 예전엔 붙기 전에 온 삭제를 놓쳐 지워진 메시지가 그대로 보였다. */
  blind(messageId, blindText) {
    if (!messageId) return;
    const text = blindText || BLIND_TEXT;
    const pending = this.pendingLines || [];
    const i = pending.findIndex((m) => m.id === messageId);
    if (i >= 0) pending[i] = { ...pending[i], isBlinded: true, content: text, emojis: {} };
    const line = this.list.querySelector(`.chat-line[data-id="${CSS.escape(messageId)}"]`);
    if (!line) return;
    line.classList.add('blinded');
    line.classList.remove('emote-only');
    const el = line.querySelector('.text, .ev-body');
    if (el) el.textContent = text;
  }

  // ── 멘션 트레이 (CHT-08) ───────────────────

  renderMentionCount() {
    const n = this.mentions.length;
    this.mentionCount.hidden = n === 0;
    this.mentionCount.textContent = n > 99 ? '99+' : String(n);
    // 도구를 접어 두었을 때도 새 멘션은 '⋯' 위 점으로 알린다.
    this.toolsDot.hidden = n === 0 || this.tools.classList.contains('open');
  }

  /** 머리줄 도구 서랍 — 펼치면 아이콘이 '⋯' 왼쪽으로 미끄러져 나오고, 접으면 다시 숨는다. */
  setToolsOpen(open) {
    this.tools.classList.toggle('open', open);
    this.header.classList.toggle('tools-open', open);
    clearTimeout(this.toolsSettle);
    if (!open) this.tools.classList.remove('settled');
    // 모션 줄이기 등으로 transitionend 가 오지 않아도 펼친 뒤에는 넘침을 푼다.
    else this.toolsSettle = setTimeout(() => { if (this.tools.classList.contains('open')) this.tools.classList.add('settled'); }, 360);
    // 접힌 동안에는 탭 이동·스크린리더에서 빠진다.
    this.tools.inert = !open;
    this.toolsToggle.setAttribute('aria-expanded', String(open));
    this.toolsToggle.setAttribute('aria-label', open ? '채팅 도구 접기' : '채팅 도구 펼치기');
    this.renderMentionCount();
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
    if (!line || !line.dataset.uid) return;
    e.preventDefault();
    this.openLineMenu(line, e.clientX, e.clientY);
  }

  /** 줄 메뉴 — 우클릭, 동작 막대의 '더보기', 키보드(Shift+F10·메뉴 키)가 함께 쓴다. */
  openLineMenu(line, x, y) {
    const uid = line.dataset.uid;
    const nick = line.dataset.nick;
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
      // 통합 목록(merged-lines.js)은 고정 줄이 없다
      ...(this.pinBar ? [['pin', '메시지 고정']] : []),
      ['highlight', highlighted ? '하이라이트 해제' : '이 사용자 하이라이트'],
      ['-'],
      ['block', '사용자 차단', 'danger'],
    ];
    showMenu(x, y, items, (action) => {
      switch (action) {
        case 'copy-text': this.copyLine(line); break;
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

  /** 줄 글 복사 — 이모티콘은 :이름: 으로 넣는다(예전엔 그림이 빠진 채 글만 복사됐다). */
  copyLine(line) {
    copy(lineCopyText(line));
  }

  pin(line) {
    this.detachActions?.();
    const clone = line.cloneNode(true);
    clone.classList.remove('mention', 'highlighted', 'kb-cur', 'search-match', 'solo-match');
    clone.removeAttribute('id');
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

  /** 이 사람 채팅만 보기(CHT-25) — 다른 사람 줄은 CSS(.chat-list.solo)로 감춘다. uid 가 null 이면 모두 보기. */
  setSolo(uid, nick = '') {
    if (!this.soloBar || (this.soloUid || null) === (uid || null)) return;
    this.soloUid = uid || null;
    this.list.classList.toggle('solo', Boolean(this.soloUid));
    for (const el of this.list.querySelectorAll('.solo-match')) el.classList.remove('solo-match');
    if (this.soloUid) {
      this.markSolo();
      this.soloBar.innerHTML = `<span class="solo-body"><b>${esc(nick)}</b> 님 채팅만 보는 중</span><button class="link-btn" data-solo-off>모두 보기</button>`;
    }
    this.soloBar.hidden = !this.soloUid;
    // 걸러 보이는 줄 수가 확 바뀌므로 맨 아래에 붙어 있던 중이면 다시 붙인다.
    if (this.follower.stuck) this.follower.snap();
  }

  markSolo() {
    for (const el of this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(this.soloUid)}"]:not(.solo-match)`)) el.classList.add('solo-match');
  }

  block(uid, nick) {
    if (!uid) return;
    const p = chatPrefs();
    const blocked = [...new Set([...(p.blockedUsers || []), uid])];
    updateChatPrefs({ blockedUsers: blocked, blockedUserNames: { ...(p.blockedUserNames || {}), [uid]: nick } });
    for (const el of this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(uid)}"]`)) el.remove();
    if (this.soloUid === uid) this.setSolo(null);
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
    const close = () => dismiss(scrim);
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
    scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); close(); } });
  }

  // ── 이모티콘 (CHT-01) · 자동완성 (CHT-02) ───

  async loadEmoticons(channelId) {
    if (!channelId) return;
    this.emoticonError = false;
    if (emoticonCache.has(channelId)) { this.emoticons = emoticonCache.get(channelId); return; }
    this.emoticons = []; // 앞 채널의 팩이 남아 다른 채널 이모티콘을 고르지 않게
    try {
      const packs = await api.chatEmoticons(channelId);
      // 빈 목록은 캐시하지 않는다 — 한 번 실패한 채널이 앱을 다시 켤 때까지 빈 선택기로 남았다.
      if (packs.length) emoticonCache.set(channelId, packs);
      if (this.channelId !== channelId) return;
      this.emoticons = packs;
      // 목록이 오기 전에 선택기를 열었으면 바로 다시 그린다(닫았다 열어야 보였다).
      if (!this.picker.hidden) this.renderPicker();
    } catch {
      if (this.channelId !== channelId) return;
      this.emoticons = [];
      this.emoticonError = true;
      if (!this.picker.hidden) this.renderPicker();
    }
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
}

/** 채팅 제한 안내 — 초 단위 기간을 사람 말로. 순수 함수. */
export function penaltyText(sec) {
  const s = Math.max(0, Math.round(Number(sec) || 0));
  if (!s) return '채팅 이용이 잠시 제한되었습니다';
  if (s < 60) return `${s}초 동안 채팅 이용이 제한되었습니다`;
  if (s < 3600) return `${Math.round(s / 60)}분 동안 채팅 이용이 제한되었습니다`;
  return `${Math.round(s / 360) / 10}시간 동안 채팅 이용이 제한되었습니다`;
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
  menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - r.width - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - r.height - 8))}px`;
  // 키보드 — 열면 첫 항목에 초점, ↑/↓·Home/End 로 옮기고 Esc·Tab 으로 닫으면 연 자리로 초점을 돌린다.
  // (예전엔 초점이 메뉴로 가지 않아 Esc 도 먹지 않고, 화살표로 고를 수 없었다.)
  const opener = document.activeElement;
  const buttons = [...menu.querySelectorAll('.ctx-item')];
  buttons.forEach((b) => { b.tabIndex = -1; });
  buttons[0]?.focus({ preventScroll: true });
  const outside = (e) => { if (!menu.contains(e.target)) close(); };
  const onBlur = () => close();
  function close(restore = false) {
    if (!menu.isConnected) return;
    menu.remove();
    document.removeEventListener('mousedown', outside, true);
    window.removeEventListener('blur', onBlur);
    if (restore && opener && opener !== document.body && opener.isConnected && typeof opener.focus === 'function') opener.focus({ preventScroll: true });
  }
  setTimeout(() => document.addEventListener('mousedown', outside, true), 0);
  window.addEventListener('blur', onBlur);
  menu.addEventListener('click', (e) => {
    const item = e.target.closest('[data-ctx]');
    if (!item) return;
    close(true);
    onPick(item.dataset.ctx);
  });
  menu.addEventListener('keydown', (e) => {
    const i = buttons.indexOf(document.activeElement);
    const move = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: buttons.length - 1 }[e.key];
    if (move !== undefined) {
      e.preventDefault();
      buttons[(move + buttons.length) % buttons.length]?.focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      close(true);
    }
  });
}
