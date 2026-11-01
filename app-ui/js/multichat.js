// 멀티채팅 — 원본 MultiChatView / MergedChatView / MultiChatSearchView / MultiChatOverviewView.
// MUL-01 여러 채널 채팅(탭·그리드, 추가·닫기, 세션 기억), MUL-02 통합(머지) 채팅(한 타임라인, 보낼 채널 선택),
// MUL-03 메시지 검색(⌥⌘F)·다음/이전 채널(⌃⇥), MUL-04 개요 칩(상태·분당 메시지).
// 채널마다 Rust 추가 채팅 세션(`mc:<channelId>`)을 연다.
import { api, listen } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { ChatPanel, renderContent } from './chat.js';
import { eventCardHtml, eventClasses } from './donations.js';
import { applyChatSkin } from './chat-extras.js';
import { nickColorClass } from './nick-color.js';
import { chatPrefs, onChatPrefs, passesFilters } from './chat-prefs.js';
import { kbd } from './platform.js';
import { chatFollower } from './chat-follow.js';
import { MultiLiveAddDrawer } from './ml-add-drawer.js';
import { verifiedBadge } from './verified-badge.js';

export const multiChatIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 5a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H8l-4 3V5Z"/><path d="M18 8h1a2 2 0 0 1 2 2v10l-3.5-2.6H11a2 2 0 0 1-2-2V15h5a4 4 0 0 0 4-4V8Z" opacity=".6"/></svg>';
const COLORS = ['#00c77f', '#5ba3ff', '#ff8a3d', '#c77dff', '#ff5d8f', '#2ec4b6', '#f4c430', '#8d99ae'];
const MAX_CHANNELS = 8;

const chatMarkup = `
  <aside class="chat mc-chat">
    <header class="chat-header"><strong class="mc-name">채팅</strong><span class="chat-state">연결 끊김</span></header>
    <div class="chat-notice" hidden></div>
    <div class="chat-list"></div>
    <form class="chat-input"><input type="text" maxlength="100" placeholder="채팅을 입력하세요…" autocomplete="off"><button class="btn primary" type="submit">채팅</button></form>
  </aside>`;

/** 검색어가 줄에 있는가(대소문자 무시). 순수 함수. */
export function lineMatches(text, query) {
  return !query || String(text || '').toLowerCase().includes(String(query).toLowerCase());
}

/** 통합(머지) 채팅 줄 — 채널 태그를 붙인 한 타임라인. 채팅 설정(차단·금지어·후원 필터·이모티콘)을 따른다.
 *  멀티라이브 멀티채팅도 쓴다. `style` 은 줄의 색 변수(--mc-color 등). */
export function mergedLinesHtml(messages, { name, style }) {
  const tag = name ? `<span class="mc-tag">${esc(name)}</span>` : '';
  const prefs = chatPrefs();
  return (messages || []).filter((m) => m.kind !== 'system' && passesFilters(m, prefs)).map((m) => {
    const ev = eventClasses(m);
    const nick = `<span class="nick${['streamer', 'manager', 'channelManager'].includes(m.role) ? ` ${m.role}` : ''}${nickColorClass(m.nickname, m.role)}">${esc(m.nickname || '익명')}</span>`;
    if (ev.length) {
      const body = m.content && m.content.trim() ? renderContent(m.content, m.emojis, prefs.emoticonEnabled) : '';
      return `<div class="chat-line merged-line ${ev.join(' ')}" style="${style}">${eventCardHtml(m, { tag, nick, content: body })}</div>`;
    }
    return `
      <div class="chat-line merged-line" style="${style}">${tag}${nick}<span class="text">${renderContent(m.content, m.emojis, prefs.emoticonEnabled)}</span></div>`;
  }).join('');
}

export class MultiChatPage {
  /** @param {{ isLoggedIn: () => boolean, onPlay?: (channelId: string) => void }} opts onPlay — 채널 추가 줄을 누르면 라이브로 */
  constructor(content, { isLoggedIn, onPlay = null }) {
    this.isLoggedIn = isLoggedIn;
    this.root = document.createElement('section');
    this.root.className = 'page page-multichat';
    this.root.dataset.page = 'multichat';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header mc-header" data-tauri-drag-region>
        <h1 class="page-title">멀티채팅</h1>
        <span class="page-subtitle mc-count"></span>
        <div class="grow"></div>
        <input type="search" class="mc-search" placeholder="${kbd('메시지 검색 (⌥⌘F)')}">
        <div class="segmented"><button class="seg" data-mc-layout="tabs">탭</button><button class="seg" data-mc-layout="grid">그리드</button><button class="seg" data-mc-layout="merged">통합</button></div>
        <button class="btn primary" data-mc="add">+ 채널</button>
        <button class="btn ghost" data-mc="close-all">모두 닫기</button>
      </header>
      <div class="mc-overview"></div>
      <div class="mc-search-note" role="status" hidden></div>
      <div class="mc-body">
        <div class="mc-empty"><strong>여러 채널의 채팅을 한 화면에서</strong><span>최대 ${MAX_CHANNELS}개 채널을 추가해 탭·그리드로 보거나 하나의 타임라인으로 합쳐 볼 수 있어요.</span><button class="btn primary lg" data-mc="add">+ 채널 추가</button></div>
        <div class="mc-panes"></div>
        <div class="mc-merged" hidden>
          <div class="chat-list mc-merged-list"></div>
          <div class="chat-input-banner mc-merged-banner" hidden></div>
          <form class="chat-input mc-merged-input"><select class="select mc-target"></select><input type="text" maxlength="100" placeholder="선택한 채널로 보내기" autocomplete="off"><button class="btn primary">보내기</button></form>
        </div>
      </div>`;
    content.append(this.root);
    this.panesBox = this.root.querySelector('.mc-panes');
    this.overview = this.root.querySelector('.mc-overview');
    this.merged = this.root.querySelector('.mc-merged');
    this.mergedList = this.root.querySelector('.mc-merged-list');
    // 통합 목록도 채팅 디자인 스타일을 따른다.
    const skin = () => applyChatSkin(this.merged);
    onChatPrefs(skin);
    skin();
    this.target = this.root.querySelector('.mc-target');
    this.search = this.root.querySelector('.mc-search');
    this.searchNote = this.root.querySelector('.mc-search-note');
    this.mergedForm = this.root.querySelector('.mc-merged-input');
    this.mergedBanner = this.root.querySelector('.mc-merged-banner');
    this.channels = []; // { channelId, name, image, color, chatChannelId, panel, el, rate: [] }
    this.active = null;
    this.layout = 'tabs';
    this.loaded = false;
    this.addBar = new MultiLiveAddDrawer(this.root.querySelector('.mc-body'), {
      isLoggedIn,
      getState: () => {
        const n = this.channels.length + (this.pendingAdds?.size || 0);
        return { sessions: this.channels, maxSessions: MAX_CHANNELS, canAdd: n < MAX_CHANNELS };
      },
      onAdd: (channelId) => this.add(channelId),
      onOpen: onPlay,
      addLabel: '채팅 추가',
      toggleSelector: '[data-mc="add"]',
      unit: '채널',
    });

    this.root.addEventListener('click', (e) => this.onClick(e));
    this.search.addEventListener('input', () => this.applySearch());
    this.mergedForm.addEventListener('submit', (e) => { e.preventDefault(); this.sendMerged(); });
    this.target.addEventListener('change', () => this.updateMergedInput());
    this.mergedBanner.addEventListener('click', (e) => { if (e.target.closest('[data-login]')) window.dispatchEvent(new Event('cview:login')); });
    listen('chat://session-event', (p) => this.onSessionEvent(p));
    document.addEventListener('keydown', (e) => {
      if (this.root.hidden) return;
      if (e.metaKey && e.altKey && e.key.toLowerCase() === 'f') { e.preventDefault(); this.search.focus(); }
      if (e.ctrlKey && e.key === 'Tab' && this.channels.length) {
        e.preventDefault();
        const i = this.channels.findIndex((c) => c.channelId === this.active);
        const next = this.channels[(i + (e.shiftKey ? -1 : 1) + this.channels.length) % this.channels.length];
        this.focus(next.channelId);
      }
    });
    // 개요 칩은 화면이 보일 때만 갱신한다.
    setInterval(() => { if (!this.root.hidden && !document.hidden) this.renderOverview(); }, 5000);
  }

  async show() {
    if (!this.loaded) {
      this.loaded = true;
      try {
        const s = await api.settingsGet();
        const mc = s.multiChat || {};
        // 설정에는 그리드 여부만 있어 '통합' 배치는 이 기기에 따로 기억한다.
        let saved = null;
        try { saved = localStorage.getItem('cview.mc.layout'); } catch { /* 저장 불가 */ }
        this.layout = ['grid', 'tabs', 'merged'].includes(saved) ? saved : (mc.isGridLayout ? 'grid' : 'tabs');
        for (const saved of (mc.savedSessions || []).slice(0, MAX_CHANNELS)) await this.add(saved.channelId, false);
        if (mc.selectedChannelId) this.focus(mc.selectedChannelId);
      } catch { /* 처음 */ }
    }
    this.render();
  }

  persist() {
    api.settingsUpdate('multiChat', {
      savedSessions: this.channels.map((c) => ({ channelId: c.channelId, channelName: c.name })),
      selectedChannelId: this.active,
      isGridLayout: this.layout === 'grid',
    }).catch(() => {});
    try { localStorage.setItem('cview.mc.layout', this.layout); } catch { /* 저장 불가 */ }
  }

  async add(channelId, save = true) {
    if (!channelId || this.channels.some((c) => c.channelId === channelId)) { this.focus(channelId); return; }
    // 추가 중인 채널 — 더블클릭·복원과 사용자 추가가 겹쳐 같은 채널이 두 번(같은 세션 키로) 붙지 않게.
    this.pendingAdds ??= new Set();
    if (this.pendingAdds.has(channelId)) return;
    if (this.channels.length + this.pendingAdds.size >= MAX_CHANNELS) { toast(`최대 ${MAX_CHANNELS}개까지 볼 수 있습니다`); return; }
    this.pendingAdds.add(channelId);
    let live;
    try { live = await api.liveDetail(channelId, false); } catch (e) { this.pendingAdds.delete(channelId); toast(e.message); return; }
    this.pendingAdds.delete(channelId);
    if (this.channels.some((c) => c.channelId === channelId)) { this.focus(channelId); return; }
    const color = COLORS[this.channels.length % COLORS.length];
    const el = document.createElement('div');
    el.className = 'mc-pane';
    el.dataset.channelId = channelId;
    el.style.setProperty('--mc-color', color);
    el.innerHTML = chatMarkup;
    this.panesBox.append(el);
    el.querySelector('.mc-name').textContent = live.channelName || channelId;
    el.querySelector('.mc-name').insertAdjacentHTML('afterend', verifiedBadge(live, 'sm'));
    const session = `mc:${channelId}`;
    const panel = new ChatPanel(el.querySelector('.chat'), { isLoggedIn: this.isLoggedIn, session });
    // 패널 머리 버튼들이 생긴 뒤 맨 끝에 닫기를 둔다.
    // 도구 아이콘은 ChatPanel 이 '⋯' 서랍에 접어 두므로, 닫기만 그 뒤에 둔다.
    el.querySelector('.chat-header').insertAdjacentHTML('beforeend', '<button class="icon-btn chat-hbtn" data-mc="remove" title="이 채널 닫기">✕</button>');
    el.querySelector('.mc-name').title = live.channelName || channelId;
    const entry = { channelId, name: live.channelName || channelId, image: live.channelImageUrl, color, chatChannelId: live.chatChannelId, panel, el, session, rate: [], isLive: live.isLive };
    panel.appendHooks.push((msgs) => {
      const now = Date.now();
      for (let i = 0; i < msgs.length; i++) entry.rate.push(now);
      if (this.search.value.trim()) this.filterTail(panel.list, msgs.length);
    });
    panel.inputHooks.push(() => this.updateMergedInput());
    this.channels.push(entry);
    panel.connect(live.chatChannelId, channelId);
    if (!this.active) this.active = channelId;
    if (save) { this.focus(channelId); this.persist(); }
    this.render();
  }

  async remove(channelId) {
    const i = this.channels.findIndex((c) => c.channelId === channelId);
    if (i < 0) return;
    const [c] = this.channels.splice(i, 1);
    await c.panel.destroy();
    c.el.remove();
    if (this.active === channelId) this.active = this.channels[0]?.channelId || null;
    this.persist();
    this.render();
  }

  focus(channelId) {
    if (!this.channels.some((c) => c.channelId === channelId)) return;
    this.active = channelId;
    this.render();
    this.persist();
  }

  render() {
    this.addBar.sync();
    const n = this.channels.length;
    this.root.querySelector('.mc-count').textContent = `${n} / ${MAX_CHANNELS}`;
    this.root.querySelector('.mc-empty').hidden = n > 0;
    for (const b of this.root.querySelectorAll('[data-mc-layout]')) b.classList.toggle('active', b.dataset.mcLayout === this.layout);
    this.panesBox.className = `mc-panes layout-${this.layout}`;
    this.panesBox.hidden = this.layout === 'merged' || n === 0;
    this.merged.hidden = this.layout !== 'merged' || n === 0;
    for (const c of this.channels) c.el.hidden = this.layout === 'tabs' && c.channelId !== this.active;
    this.target.innerHTML = this.channels.map((c) => `<option value="${esc(c.session)}" ${c.channelId === this.active ? 'selected' : ''}>${esc(c.name)}</option>`).join('');
    this.renderOverview();
    this.applySearch();
    this.updateMergedInput();
  }

  /**
   * 통합 입력 — 고른 채널 패널의 로그인·연결·보내기 권한을 그대로 따른다(탭·그리드와 같게).
   * 예전엔 비로그인이어도 입력·보내기가 켜져 있었다(서버는 막지만 보낼 수 있는 것처럼 보였다).
   */
  updateMergedInput() {
    const c = this.channels.find((x) => x.session === this.target.value);
    const loggedIn = this.isLoggedIn();
    const p = c && c.panel;
    const enabled = Boolean(loggedIn && p && p.connected && p.canSend);
    const input = this.mergedForm.querySelector('input');
    input.disabled = !enabled;
    this.mergedForm.querySelector('button').disabled = !enabled;
    this.target.disabled = !loggedIn || !this.channels.length;
    input.placeholder = loggedIn ? '선택한 채널로 보내기' : '로그인 후 채팅 참여 가능';
    let html = '';
    if (!loggedIn) html = '로그인하면 채팅에 참여할 수 있어요 <button class="link-btn" data-login>로그인</button>';
    else if (p && !p.connected) html = `${esc(c.name)} 채팅에 연결되어 있지 않습니다`;
    else if (p && !p.canSend) html = `${esc(c.name)} 채팅방에서는 지금 채팅을 보낼 수 없습니다 (팔로워·구독자 전용이거나 제한 중)`;
    this.mergedBanner.innerHTML = html;
    this.mergedBanner.hidden = !html;
  }

  /** 개요 칩 — 채널·연결 상태·분당 메시지 (MUL-04). 탭 모드에서는 탭 역할도 한다. */
  renderOverview() {
    const now = Date.now();
    this.overview.innerHTML = this.channels.map((c) => {
      c.rate = c.rate.filter((t) => now - t < 60000);
      const state = c.panel.state.className.includes('ok') ? 'ok' : (c.panel.state.className.includes('bad') ? 'bad' : 'warn');
      const avatar = safeUrl(c.image);
      return `<button class="mc-chip${c.channelId === this.active ? ' active' : ''}" data-mc-focus="${esc(c.channelId)}" style="--mc-color:${c.color}">
        ${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
        <span class="mc-chip-name">${esc(c.name)}</span><i class="mc-dot ${state}"></i><span class="mc-rate">${c.rate.length}/분</span>
        <span class="mc-chip-x" data-mc-remove="${esc(c.channelId)}" title="닫기">✕</span></button>`;
    }).join('');
  }

  // ── 통합 채팅 (MUL-02) ──
  onSessionEvent(p) {
    if (!p || !p.event || p.event.type !== 'messages' || this.root.hidden || this.layout !== 'merged') return;
    const c = this.channels.find((x) => x.session === p.session);
    if (!c) return;
    const html = mergedLinesHtml(p.event.messages, { name: c.name, style: `--mc-color:${c.color}` });
    if (!html) return;
    const follower = chatFollower(this.mergedList);
    const follow = follower.stuck;
    this.mergedList.querySelector(':scope > .empty')?.remove(); // 첫 메시지가 오면 "기다리는 중" 안내를 걷는다
    const before = this.mergedList.lastElementChild;
    this.mergedList.insertAdjacentHTML('beforeend', html);
    // 검색 중이면 새로 붙은 줄만 거른다(예전엔 메시지 묶음마다 600줄 + 모든 패널을 다시 훑었다).
    const q = this.search.value.trim();
    if (q) {
      for (let line = before ? before.nextElementSibling : this.mergedList.firstElementChild; line; line = line.nextElementSibling) {
        const ok = lineMatches(line.textContent, q);
        line.classList.toggle('search-hidden', !ok);
        line.classList.toggle('search-hit', ok);
      }
    }
    while (this.mergedList.children.length > 600) this.mergedList.firstElementChild.remove();
    if (q) this.queueSearchNote();
    if (follow) follower.snap();
  }

  async sendMerged() {
    const input = this.mergedForm.querySelector('input');
    const text = input.value.trim();
    if (!text || input.disabled) return;
    try { await api.chatSend(text, this.target.value); input.value = ''; } catch (e) { toast(e.message); }
  }

  // ── 검색 (MUL-03) ──
  applySearch() {
    const q = this.search.value.trim();
    const boxes = this.layout === 'merged' ? [this.mergedList] : this.channels.map((c) => c.panel.list);
    let hits = 0;
    for (const box of boxes) {
      for (const line of box.querySelectorAll('.chat-line')) {
        const ok = lineMatches(line.textContent, q);
        line.classList.toggle('search-hidden', !ok);
        line.classList.toggle('search-hit', Boolean(q) && ok);
        if (q && ok) hits += 1;
      }
    }
    this.search.title = q ? `일치 ${hits}개` : '메시지 검색';
    this.renderSearchNote(q, hits);
  }

  /** 패널에 새로 붙은 끝의 n줄만 검색어로 거른다 — 검색 중에 온 메시지가 걸러지지 않고 섞였다. */
  filterTail(list, n) {
    const q = this.search.value.trim();
    let line = list.lastElementChild;
    for (let i = 0; i < n && line; line = line.previousElementSibling) {
      if (!line.classList.contains('chat-line')) continue;
      const ok = lineMatches(line.textContent, q);
      line.classList.toggle('search-hidden', !ok);
      line.classList.toggle('search-hit', ok);
      i += 1;
    }
    this.queueSearchNote();
  }

  /** 새 메시지마다 세지 않고 한 프레임에 한 번 일치 수를 다시 센다. */
  queueSearchNote() {
    if (this.notePending) return;
    this.notePending = true;
    requestAnimationFrame(() => {
      this.notePending = false;
      const q = this.search.value.trim();
      const boxes = this.layout === 'merged' ? [this.mergedList] : this.channels.map((c) => c.panel.list);
      const hits = boxes.reduce((n, b) => n + b.querySelectorAll('.chat-line.search-hit').length, 0);
      this.search.title = q ? `일치 ${hits}개` : '메시지 검색';
      this.renderSearchNote(q, hits);
    });
  }

  /** 검색 중 상태 — 일치 수와 검색 지우기. 0건이면 "기다리는 중"이 아니라 결과 없음으로 알린다. */
  renderSearchNote(q, hits) {
    const show = Boolean(q) && this.channels.length > 0;
    this.searchNote.hidden = !show;
    this.root.classList.toggle('mc-searching', show);
    if (!show) { this.searchNote.innerHTML = ''; return; }
    this.searchNote.classList.toggle('none', hits === 0);
    this.searchNote.innerHTML = `<span>${hits ? `"${esc(q)}" 일치 ${hits}개` : `"${esc(q)}"와 일치하는 메시지가 없습니다 · 새 메시지가 오면 여기에 걸러 보입니다`}</span><button class="link-btn" data-mc="clear-search">검색 지우기</button>`;
  }

  async onClick(e) {
    const rm = e.target.closest('[data-mc-remove]');
    if (rm) { e.stopPropagation(); this.remove(rm.dataset.mcRemove); return; }
    const f = e.target.closest('[data-mc-focus]');
    if (f) { this.focus(f.dataset.mcFocus); return; }
    const l = e.target.closest('[data-mc-layout]');
    if (l) { this.layout = l.dataset.mcLayout; if (this.layout === 'merged') { this.mergedList.innerHTML = '<div class="empty">새 메시지를 기다리는 중…</div>'; chatFollower(this.mergedList).stuck = true; } this.render(); this.persist(); return; }
    const a = e.target.closest('[data-mc]');
    if (!a) return;
    if (a.dataset.mc === 'add') this.addBar.toggle();
    if (a.dataset.mc === 'clear-search') { this.search.value = ''; this.applySearch(); this.search.focus(); }
    if (a.dataset.mc === 'remove') this.remove(a.closest('.mc-pane').dataset.channelId);
    if (a.dataset.mc === 'close-all') for (const c of [...this.channels]) await this.remove(c.channelId);
  }
}
