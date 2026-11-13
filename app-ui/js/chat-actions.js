// 채팅 줄 동작 — 줄 위 빠른 동작 막대(답장·복사·더보기), 키보드로 줄 고르기, 채팅 검색.
// - 동작 막대는 패널마다 하나를 만들어 두고 마우스가 올라간(또는 키보드로 고른) 줄에 옮겨 붙인다. 줄마다
//   버튼을 그리면 붐비는 채팅에서 줄 비용이 늘어(P04) 하나만 돌려쓴다. 막대는 줄의 자식이라 줄이 밀려 올라가도
//   같이 움직이고, 줄이 지워지면 함께 떨어졌다가 다음 호버에 다시 붙는다(목록의 자식 수 — 줄 상한 — 에는 안 든다).
// - 키보드: 목록에 초점을 두고 ↑/↓(Home/End)로 줄을 고르고, Enter 로 그 사람 말풍선, Shift+F10·메뉴 키로 줄 메뉴,
//   Esc 로 고르기를 푼다. 고른 줄은 aria-activedescendant 로 화면 읽기에 알린다.
// - 검색: 머리줄 도구의 돋보기로 연다. 글자·닉네임이 맞는 줄만 남긴다(CSS .chat-list.searching). 새로 붙는 줄도 거른다.

const ICON = {
  reply: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 6 6v5"/></svg>',
  copy: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h8"/></svg>',
  more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5.5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/></svg>',
  search: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/></svg>',
  close: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
};

/** 이 줄에 동작 막대를 붙일 수 있나 — 사용자 줄(말풍선·이벤트 카드·공지)만. */
const ACTIONABLE = '.chat-line[data-uid]:not([data-uid=""])';

/** 검색어가 줄에 맞나 — 닉네임·본문(이모티콘 이름 포함)을 소문자로 비교. 순수 함수. */
export function lineMatches(line, q) {
  if (!q) return true;
  const nick = (line.dataset.nick || '').toLowerCase();
  if (nick.includes(q)) return true;
  const text = line.querySelector('.text, .ev-body');
  if (!text) return false;
  if (text.textContent.toLowerCase().includes(q)) return true;
  for (const img of text.querySelectorAll('img.emoji')) if ((img.alt || '').toLowerCase().includes(q)) return true;
  return false;
}

let ids = 0;

export function installChatActions(panel) {
  const list = panel.list;
  const uid = `chat${++ids}`;

  // ── 동작 막대 ──
  const bar = document.createElement('div');
  bar.className = 'chat-actions';
  bar.setAttribute('role', 'toolbar');
  bar.setAttribute('aria-label', '메시지 동작');
  bar.innerHTML = `
    <button type="button" class="ca-btn" data-ca="reply" title="답장 (@멘션)" aria-label="답장">${ICON.reply}</button>
    <button type="button" class="ca-btn" data-ca="copy" title="메시지 복사" aria-label="메시지 복사">${ICON.copy}</button>
    <button type="button" class="ca-btn" data-ca="more" title="더보기" aria-label="더보기">${ICON.more}</button>`;
  const replyBtn = bar.querySelector('[data-ca="reply"]');
  let barLine = null;

  const attach = (line) => {
    if (line === barLine && bar.parentNode === line) return;
    barLine = line;
    if (!line) { bar.remove(); return; }
    const own = line.classList.contains('own');
    replyBtn.hidden = own;
    replyBtn.disabled = panel.input.disabled;
    replyBtn.title = panel.input.disabled ? '지금은 채팅을 보낼 수 없습니다' : `@${line.dataset.nick || ''} 에게 답장`;
    bar.classList.toggle('on-left', own);
    line.append(bar);
  };
  panel.detachActions = () => attach(null);

  list.addEventListener('mouseover', (e) => {
    if (e.target === bar || bar.contains(e.target)) return;
    const line = e.target.closest(ACTIONABLE);
    if (line && list.contains(line)) attach(line);
  });
  list.addEventListener('mouseleave', () => { if (!cur) attach(null); else attach(cur); });

  bar.addEventListener('click', (e) => {
    const b = e.target.closest('[data-ca]');
    const line = barLine;
    if (!b || !line) return;
    e.stopPropagation();
    if (b.dataset.ca === 'reply') panel.mention(line.dataset.nick || '');
    else if (b.dataset.ca === 'copy') panel.copyLine(line);
    else if (b.dataset.ca === 'more') {
      const r = b.getBoundingClientRect();
      panel.openLineMenu(line, r.left, r.bottom + 4);
    }
  });
  // 막대 위 더블 클릭이 프로필을 열지 않게
  bar.addEventListener('dblclick', (e) => e.stopPropagation());

  // ── 키보드로 줄 고르기 ──
  list.tabIndex = 0;
  list.setAttribute('role', 'log');
  // 붐비는 채팅을 줄마다 읽어 주면 화면 읽기가 묻힌다 — 자동 낭독은 끄고, 고른 줄만 읽힌다.
  list.setAttribute('aria-live', 'off');
  list.setAttribute('aria-label', '채팅 메시지 — 화살표로 메시지를 고를 수 있습니다');
  let cur = null;
  const select = (line) => {
    if (cur && cur !== line) { cur.classList.remove('kb-cur'); cur.removeAttribute('id'); }
    cur = line && line.isConnected ? line : null;
    if (!cur) { list.removeAttribute('aria-activedescendant'); attach(null); return; }
    cur.classList.add('kb-cur');
    if (!cur.id) cur.id = `${uid}-cur`;
    list.setAttribute('aria-activedescendant', cur.id);
    cur.scrollIntoView({ block: 'nearest' });
    attach(cur);
  };
  panel.clearLineSelection = () => select(null);
  const visibleLines = () => [...list.querySelectorAll(ACTIONABLE)].filter((l) => l.offsetParent !== null);
  list.addEventListener('keydown', (e) => {
    if (e.target !== list) return; // 막대 버튼 위 키는 버튼 몫
    const keys = ['ArrowUp', 'ArrowDown', 'Home', 'End'];
    if (keys.includes(e.key)) {
      const lines = visibleLines();
      if (!lines.length) return;
      e.preventDefault();
      let i = cur ? lines.indexOf(cur) : -1;
      if (e.key === 'Home') i = 0;
      else if (e.key === 'End') i = lines.length - 1;
      else if (i < 0) i = lines.length - 1; // 처음엔 가장 최근 줄부터
      else i = Math.max(0, Math.min(lines.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)));
      select(lines[i]);
      return;
    }
    if (!cur || !cur.isConnected) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      const anchor = cur.querySelector('.nick, .chat-avatar');
      if (anchor) anchor.click();
    } else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault();
      const r = cur.getBoundingClientRect();
      panel.openLineMenu(cur, r.left + 24, r.top + Math.min(r.height, 28));
    } else if (e.key === 'Escape') {
      e.preventDefault();
      select(null);
    }
  });
  list.addEventListener('blur', (e) => { if (!list.contains(e.relatedTarget) && !e.relatedTarget?.closest?.('.ctx-menu, .user-pop')) select(null); });

  // ── 검색 ──
  const searchBtn = document.createElement('button');
  searchBtn.className = 'icon-btn chat-hbtn';
  searchBtn.dataset.chatSearch = '';
  searchBtn.title = '채팅 검색';
  searchBtn.setAttribute('aria-label', '채팅 검색');
  searchBtn.innerHTML = ICON.search;
  panel.header.querySelector('.chat-tools-inner').prepend(searchBtn);

  const box = document.createElement('div');
  box.className = 'chat-search';
  box.hidden = true;
  box.innerHTML = `<span class="cs-icon" aria-hidden="true">${ICON.search}</span>
    <input type="search" class="cs-input" placeholder="닉네임·내용 찾기" aria-label="채팅 검색" autocomplete="off" spellcheck="false">
    <span class="cs-count" aria-live="polite"></span>
    <button type="button" class="icon-btn cs-close" title="검색 닫기" aria-label="검색 닫기">${ICON.close}</button>`;
  panel.soloBar.after(box);
  const input = box.querySelector('.cs-input');
  const count = box.querySelector('.cs-count');
  // 맞는 줄이 없을 때 목록 위 안내 — 예전엔 빈 목록과 '없음' 세 글자뿐이라 채팅이 끊긴 것처럼 보였다.
  const empty = document.createElement('div');
  empty.className = 'chat-search-empty';
  empty.hidden = true;
  list.before(empty);
  panel.searchQuery = '';

  const mark = (lines) => {
    const q = panel.searchQuery;
    for (const l of lines) {
      if (!l.classList.contains('chat-line') || l.classList.contains('system')) continue;
      l.classList.toggle('search-match', lineMatches(l, q));
    }
  };
  const renderCount = () => {
    if (!panel.searchQuery) { count.textContent = ''; empty.hidden = true; return; }
    const n = list.querySelectorAll('.chat-line.search-match').length;
    count.textContent = n ? `${n}개` : '없음';
    empty.hidden = n > 0;
    if (!n) empty.textContent = `‘${input.value.trim()}’와 맞는 채팅이 아직 없습니다 — 새로 오는 채팅도 찾아 보여 줍니다`;
  };
  let debounce = 0;
  const run = () => {
    const q = input.value.trim().toLowerCase();
    panel.searchQuery = q;
    list.classList.toggle('searching', Boolean(q));
    if (q) mark(list.children);
    else for (const l of list.querySelectorAll('.search-match')) l.classList.remove('search-match');
    renderCount();
    if (cur && cur.offsetParent === null) select(null);
    if (panel.follower.stuck) panel.follower.snap();
  };
  input.addEventListener('input', () => { clearTimeout(debounce); debounce = setTimeout(run, 120); });
  const open = () => {
    box.hidden = false;
    searchBtn.setAttribute('aria-pressed', 'true');
    input.focus();
    input.select();
  };
  const close = () => {
    clearTimeout(debounce);
    input.value = '';
    run();
    box.hidden = true;
    searchBtn.setAttribute('aria-pressed', 'false');
  };
  panel.openSearch = open;
  panel.closeSearch = close;
  panel.refreshSearch = renderCount;
  searchBtn.setAttribute('aria-pressed', 'false');
  searchBtn.addEventListener('click', (e) => { e.stopPropagation(); if (box.hidden) open(); else close(); });
  box.querySelector('.cs-close').addEventListener('click', () => { close(); list.focus(); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); list.focus(); }
    else if (e.key === 'Enter') { e.preventDefault(); clearTimeout(debounce); run(); }
  });

  // 새로 붙은 줄도 검색어로 거르고, 고른 줄이 지워졌으면 고르기를 푼다.
  panel.appendHooks.push((shown) => {
    if (panel.searchQuery) {
      mark([...list.children].slice(-shown.length - 1));
      renderCount();
    }
    if (cur && !cur.isConnected) select(null);
  });
  return { bar, box, searchBtn };
}

/** 줄 하나를 복사할 글 — "닉네임: 본문" (이모티콘은 :이름: 으로). */
export function lineCopyText(line) {
  const text = line.querySelector('.text, .ev-body');
  if (!text) return '';
  let out = '';
  for (const n of text.childNodes) {
    if (n.nodeType === 3) out += n.textContent;
    else if (n.tagName === 'IMG') out += `:${n.alt || ''}:`;
    else out += n.textContent;
  }
  return out.trim();
}

