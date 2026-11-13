// 통합 채팅 목록의 줄 동작 — 멀티채팅 통합(multichat.js)과 멀티라이브 멀티채팅(ml-multichat.js)이 함께 쓴다.
// - 닉네임·아바타를 누르면 닉네임 말풍선(chat-user-pop.js — 팔로우·채널 보기·이 사람만 보기·멘션·하이라이트·차단)
// - 우클릭이면 줄 메뉴(복사·멘션·프로필·하이라이트·차단), 줄을 두 번 누르면 그 사람의 최근 메시지
// 단일 채팅(ChatPanel)과 같은 구현을 쓴다 — 프로필·하이라이트·차단·줄 메뉴는 ChatPanel 의 메서드를 이 목록에 건다.
// 줄마다 출처(data-src)가 달라서 말풍선이 볼 채널(팔로우·채널 정보)과 멘션을 보낼 채널은 줄의 출처로 정한다.
// 예전 통합 줄은 태그·닉네임·글뿐이라 이것들이 하나도 되지 않았다.
import { ChatPanel } from './chat.js';
import { openUserPop, closeUserPop } from './chat-user-pop.js';
import { esc } from './dom.js';

const P = ChatPanel.prototype;
const NO_INPUT = { disabled: true };

export class MergedLineHost {
  /**
   * @param {HTMLElement} list 통합 목록(.chat-list)
   * @param {{ resolve: (src: string) => ({ channelId?: string, chatChannelId?: string }|null),
   *           input: () => (HTMLInputElement|null), mention: (nick: string, src: string) => void, isLoggedIn: () => boolean }} opts
   *   resolve — 출처 키 → 그 채널, input — 멘션이 들어갈 입력창(보낼 수 있는지 말풍선이 본다), mention — 그 채널로 멘션 넣기,
   *   isLoggedIn — 말풍선의 팔로우 단추
   */
  constructor(list, { resolve, input, mention, isLoggedIn }) {
    this.isLoggedIn = isLoggedIn;
    this.list = list;
    this.resolve = resolve;
    this.getInput = input;
    this.mentionTo = mention;
    this.views = new Map();
    this.soloUid = null;
    // 이 사람 채팅만 보기(CHT-25) — 단일 채팅과 같은 띠(.chat-solo)를 목록 위에 둔다.
    this.soloBar = document.createElement('div');
    this.soloBar.className = 'chat-solo';
    this.soloBar.hidden = true;
    list.before(this.soloBar);
    this.soloBar.addEventListener('click', (e) => { if (e.target.closest('[data-solo-off]')) this.setSolo(null); });

    list.addEventListener('click', (e) => {
      const anchor = e.target.closest('.nick, .chat-avatar');
      const line = anchor && anchor.closest('.chat-line[data-uid]');
      if (!line || !line.dataset.uid || !list.contains(line)) return;
      openUserPop(this.view(line.dataset.src || ''), anchor, line);
    });
    list.addEventListener('contextmenu', (e) => {
      const line = e.target.closest('.chat-line[data-uid]');
      if (!line || !line.dataset.uid) return;
      e.preventDefault();
      this.view(line.dataset.src || '').openLineMenu(line, e.clientX, e.clientY);
    });
    list.addEventListener('dblclick', (e) => {
      if (e.target.closest('.nick, .chat-avatar')) return; // 닉네임은 말풍선이 맡는다
      const line = e.target.closest('.chat-line[data-uid]');
      if (line && line.dataset.uid) this.view(line.dataset.src || '').showProfile(line.dataset.uid, line.dataset.nick);
    });
  }

  /**
   * 출처별 '패널 모양' — 말풍선·줄 메뉴가 ChatPanel 에서 쓰는 이름(list·input·channelId·mention·block…)을 준다.
   * 같은 출처는 같은 객체라 같은 닉네임을 다시 누르면 말풍선이 닫힌다(chat-user-pop 의 토글).
   */
  view(src) {
    let v = this.views.get(src);
    if (!v) {
      const host = this;
      v = {
        get list() { return host.list; },
        get input() { return host.getInput() || NO_INPUT; },
        get soloUid() { return host.soloUid; },
        isLoggedIn: () => Boolean(host.isLoggedIn && host.isLoggedIn()),
        channelId: null,
        chatChannelId: null,
        setSolo: (uid, nick) => host.setSolo(uid, nick),
        mention: (nick) => host.mentionTo(nick, src),
        showProfile: P.showProfile,
        toggleHighlight: P.toggleHighlight,
        block: P.block,
        copyLine: P.copyLine,
        openLineMenu: P.openLineMenu,
      };
      this.views.set(src, v);
    }
    const r = this.resolve(src) || {};
    v.channelId = r.channelId || null;
    v.chatChannelId = r.chatChannelId || null;
    return v;
  }

  /** 이 사람 채팅만 보기 — 다른 사람 줄은 CSS(.chat-list.solo)로 감춘다. uid 가 null 이면 모두 보기. */
  setSolo(uid, nick = '') {
    if ((this.soloUid || null) === (uid || null)) return;
    this.soloUid = uid || null;
    // 칸 묶음(.ml-mc-grid)에 걸었으면 묶음에 — 칸 목록들은 CSS 가 함께 거른다
    this.list.classList.toggle('solo', Boolean(this.soloUid));
    for (const el of this.list.querySelectorAll('.solo-match')) el.classList.remove('solo-match');
    if (this.soloUid) {
      this.markSolo();
      this.soloBar.innerHTML = `<span class="solo-body"><b>${esc(nick)}</b> 님 채팅만 보는 중</span><button class="link-btn" data-solo-off>모두 보기</button>`;
    }
    this.soloBar.hidden = !this.soloUid;
  }

  markSolo() {
    for (const el of this.list.querySelectorAll(`.chat-line[data-uid="${CSS.escape(this.soloUid)}"]:not(.solo-match)`)) el.classList.add('solo-match');
  }

  /** 새 줄을 붙인 뒤 — 이 사람만 보는 중이면 새 줄에도 표시한다. */
  afterAppend() {
    if (this.soloUid) this.markSolo();
  }

  /** 목록을 비울 때(보기 바꾸기 등) — 열린 말풍선을 닫고 이 사람만 보기를 푼다. */
  reset() {
    for (const v of this.views.values()) closeUserPop(v);
    this.setSolo(null);
  }
}
