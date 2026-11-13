// 접근성·키보드 포커스 — 원본 Components/CardAccessibility, CViewCore/Focus (DSN-06).
// - 글자 없는 아이콘 버튼에 title 을 aria-label 로 옮긴다(화면 낭독기가 이름을 읽게).
// - 카드 격자에서 화살표 키로 포커스를 옮기고, Enter·Space 로 연다(카드의 클릭 동작 그대로).
//   라이브 카드는 칸이 아니라 칸 안의 주 동작 단추(.card-open, cards.js `cardOpenButton`)가 초점을 받는다 — 단추라 Enter·Space 는
//   브라우저가 click 으로 바꾸고, 여기서는 화살표만 옮긴다. 다시보기·클립 카드는 아직 칸 자체(.card[tabindex])가 초점을 받는다.

/** 같은 격자 안에서 방향에 맞는 다음 카드를 고른다 — 화면 좌표로 판단. 순수 함수(좌표 배열 입력). */
export function nextIndex(rects, from, dir) {
  const a = rects[from];
  if (!a) return -1;
  const cx = (r) => r.left + r.width / 2;
  const cy = (r) => r.top + r.height / 2;
  let best = -1;
  let bestScore = Infinity;
  rects.forEach((b, i) => {
    if (i === from) return;
    const dx = cx(b) - cx(a);
    const dy = cy(b) - cy(a);
    const ok = (dir === 'right' && dx > 4 && Math.abs(dy) < a.height / 2)
      || (dir === 'left' && dx < -4 && Math.abs(dy) < a.height / 2)
      || (dir === 'down' && dy > 4)
      || (dir === 'up' && dy < -4);
    if (!ok) return;
    const score = dir === 'left' || dir === 'right' ? Math.abs(dx) : Math.abs(dy) * 4 + Math.abs(dx);
    if (score < bestScore) { bestScore = score; best = i; }
  });
  return best;
}

const DIRS = { ArrowRight: 'right', ArrowLeft: 'left', ArrowDown: 'down', ArrowUp: 'up' };

/** 화살표로 오가는 카드 — 주 동작 단추를 품은 칸, 또는 칸 자체가 초점을 받는 예전 방식 카드. */
const CARD = ':is(.card[tabindex], :has(> .card-open))';
/** 카드에서 초점을 받을 요소. */
function cardFocusTarget(card) {
  return card.matches('[tabindex]') ? card : card.querySelector(':scope > .card-open');
}

const ICON_BUTTON = 'button[title]:not([aria-label]), [role=button][title]:not([aria-label])';

function labelOne(b) {
  // 글자가 없거나 기호뿐인 버튼('✕'·'×'·'⋯' 등) — 화면 낭독기가 '곱하기 기호'로 읽었다. 글자·숫자가 있으면 그 글이 이름이다.
  const text = b.textContent.trim();
  if (!text || !/[\p{L}\p{N}]/u.test(text) || b.classList.contains('icon-btn')) b.setAttribute('aria-label', b.title);
}

function labelIconButtons(root) {
  if (root.matches && root.matches(ICON_BUTTON)) labelOne(root);
  for (const b of root.querySelectorAll(ICON_BUTTON)) labelOne(b);
}

// 채팅 목록은 줄이 프레임마다 들어오지만 아이콘 버튼이 없다 — 그 안의 변화는 보지 않는다.
const SKIP = '.chat-list, .mc-merged-list';

// 선택 묶음(필터 칩·정렬/보기 세그먼트)의 고른 상태 — 화면은 .active 로만 그려 화면 낭독기가 무엇이 골라졌는지 몰랐다.
// 탭(role=tablist → aria-selected)·라디오(role=radiogroup → aria-checked)·스스로 역할을 단 버튼은 건드리지 않는다.
// 고른 버튼이 하나도 없는 묶음(예: 카테고리 바로가기 칩)은 누르면 동작하는 버튼이라 토글로 알리지 않는다.
const PRESS_BTN = 'button.chip, button.seg';
const PRESS_GROUP = '.chips, .segmented';
function syncPressedGroup(group) {
  if (group.matches('[role="tablist"], [role="radiogroup"]')) return;
  const buttons = [...group.querySelectorAll(PRESS_BTN)].filter((b) => b.closest(PRESS_GROUP) === group && !b.hasAttribute('role'));
  if (!buttons.some((b) => b.classList.contains('active'))) return;
  for (const b of buttons) {
    const on = String(b.classList.contains('active'));
    if (b.getAttribute('aria-pressed') !== on) b.setAttribute('aria-pressed', on);
  }
}
export function syncPressed(root) {
  if (root.matches && root.matches(PRESS_GROUP)) syncPressedGroup(root);
  else if (root.matches && root.matches(PRESS_BTN)) { const g = root.closest(PRESS_GROUP); if (g) syncPressedGroup(g); }
  for (const g of root.querySelectorAll ? root.querySelectorAll(PRESS_GROUP) : []) syncPressedGroup(g);
}

/** 본문 바로가기 — 지금 보이는 화면(시청 화면이 떠 있으면 그 화면)으로 초점을 옮긴다. */
function bindSkipLink() {
  const link = document.querySelector('.skip-link');
  if (!link) return;
  link.addEventListener('click', (e) => {
    e.preventDefault();
    const main = document.getElementById('main-content');
    const visible = (el) => el && !el.hidden && el.offsetParent !== null;
    const target = [...document.querySelectorAll('.viewer, #main-content > .page')].find(visible) || main;
    if (!target) return;
    target.dataset.skipTarget = '';
    if (!target.hasAttribute('tabindex')) target.tabIndex = -1;
    target.focus({ preventScroll: true });
  });
}

export function installA11y() {
  labelIconButtons(document);
  syncPressed(document);
  bindSkipLink();
  // 새로 붙은 노드만 훑는다(예전엔 변화마다 문서 전체를 다시 검색해 채팅 묶음마다 비용이 들었다).
  const queue = new Set();
  let pending = false;
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.target.closest && r.target.closest(SKIP)) continue;
      for (const n of r.addedNodes) if (n.nodeType === 1) queue.add(n);
    }
    if (pending || !queue.size) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      for (const n of queue) if (n.isConnected) { labelIconButtons(n); syncPressed(n); }
      queue.clear();
    });
  }).observe(document.body, { childList: true, subtree: true });
  // 다시 그리지 않고 .active 만 옮기는 묶음(홈 카테고리 칩 등) — 칩·세그먼트 버튼의 class 변화만 본다
  new MutationObserver((records) => {
    const groups = new Set();
    for (const r of records) {
      if (!r.target.matches || !r.target.matches(PRESS_BTN)) continue;
      const g = r.target.closest(PRESS_GROUP);
      if (g) groups.add(g);
    }
    for (const g of groups) syncPressedGroup(g);
  }).observe(document.body, { attributes: true, attributeFilter: ['class'], subtree: true });

  document.addEventListener('keydown', (e) => {
    const dir = DIRS[e.key];
    if (!dir && e.key !== 'Enter' && e.key !== ' ') return; // 글자 입력마다 조상을 훑지 않게
    const card = e.target && e.target.closest && e.target.closest(CARD);
    if (!card || e.metaKey || e.ctrlKey || e.altKey) return;
    if (!dir) {
      // 예전 방식 카드만 — 칸 자체에 초점이 있을 때. 주 동작 단추·+·아바타는 단추라 브라우저가 click 으로 바꾼다(또 누르면 두 번 열린다).
      if (e.target !== card || !card.hasAttribute('tabindex')) return;
      e.preventDefault();
      card.click();
      return;
    }
    const scope = card.parentElement;
    const cards = [...scope.querySelectorAll(`:scope > ${CARD}, :scope > * > ${CARD}`)].filter((c) => c.offsetParent);
    const i = cards.indexOf(card);
    const j = nextIndex(cards.map((c) => c.getBoundingClientRect()), i, dir);
    if (j >= 0) {
      e.preventDefault();
      cardFocusTarget(cards[j]).focus();
      cards[j].scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
  });
}

/**
 * 탭 묶음(role=tablist)의 선택 상태를 맞춘다 — 선택 탭만 aria-selected·tabindex 0(나머지 -1, 화살표로 옮긴다),
 * 패널은 role=tabpanel 로 선택 탭과 잇는다. `prefix` 로 탭·패널 id 를 만든다(`<prefix>-tab-<id>`, `<prefix>-panel`).
 * 예전엔 CSS active 만 바뀌어 화면 낭독기가 지금 탭을 알 수 없었다.
 */
export function syncTabs(tablist, activeId, panel, prefix) {
  const panelId = `${prefix}-panel`;
  if (panel) { panel.id = panelId; panel.setAttribute('role', 'tabpanel'); }
  for (const b of tablist.querySelectorAll('[role="tab"]')) {
    const on = b.dataset.tab === activeId;
    b.id = `${prefix}-tab-${b.dataset.tab}`;
    b.classList.toggle('active', on);
    b.setAttribute('aria-selected', String(on));
    b.setAttribute('aria-controls', panelId);
    b.tabIndex = on ? 0 : -1;
    if (on && panel) panel.setAttribute('aria-labelledby', b.id);
    // 탭이 넘쳐 가로로 밀리는 묶음(.page-tabs)은 고른 탭이 보이게 묶음 안에서만 민다(페이지는 움직이지 않는다)
    if (on && tablist.scrollWidth > tablist.clientWidth) {
      const l = b.offsetLeft - 8; const r = b.offsetLeft + b.offsetWidth + 8 - tablist.clientWidth;
      if (tablist.scrollLeft > l) tablist.scrollLeft = l; else if (tablist.scrollLeft < r) tablist.scrollLeft = r;
    }
  }
}

/** 탭 묶음의 키보드 — ←/→ 로 옆 탭, Home/End 로 처음·끝 탭을 고르고 초점을 옮긴다(고르면 click 으로 바뀐다). */
/** 가로로 넘치는 묶음의 넘친 쪽을 data-edge(start·end·both)로 알린다 — CSS 가 그쪽 끝을 흐리게 한다. */
export function watchEdges(box) {
  const update = () => {
    const max = box.scrollWidth - box.clientWidth;
    const start = box.scrollLeft > 2;
    const end = max > 2 && box.scrollLeft < max - 2;
    const edge = start && end ? 'both' : start ? 'start' : end ? 'end' : '';
    if (edge) box.dataset.edge = edge; else delete box.dataset.edge;
  };
  box.addEventListener('scroll', update, { passive: true });
  // 설정처럼 다시 그려 묶음이 통째로 바뀌는 화면이 있어, 떨어져 나가면 관찰을 끊는다
  // 첫 계산은 ResizeObserver 의 첫 알림(레이아웃 뒤)에서 — 만들자마자 scrollWidth 를 읽으면 숨은 화면에서도 강제 레이아웃이 일어났다
  const ro = new ResizeObserver(() => { if (!box.isConnected) { ro.disconnect(); return; } update(); });
  ro.observe(box);
  return update;
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * 모달 안에 키보드 초점을 가둔다 — Tab/⇧Tab 이 끝에서 처음으로(거꾸로) 돌고, 바깥에 초점이 있으면 안으로 데려온다.
 * 예전엔 시트·팔레트·환영 안내·정리 확인에서 Tab 을 몇 번 누르면 뒤 화면으로 빠졌다(aria-modal 인데도).
 * 돌려준 함수를 부르면 풀고 연 자리로 초점을 돌려준다(restore). 모달이 문서에서 빠지거나 숨으면 스스로 풀린다.
 * @param {HTMLElement} box
 * @param {{ restore?: boolean }} [opts]
 */
export function trapFocus(box, { restore = true } = {}) {
  const back = document.activeElement;
  let on = true;
  const release = () => {
    if (!on) return;
    on = false;
    document.removeEventListener('keydown', onKey, true);
    if (restore && back && back !== document.body && back.isConnected && typeof back.focus === 'function') back.focus({ preventScroll: true });
  };
  const onKey = (e) => {
    if (!box.isConnected || box.closest('[hidden]')) { on = false; document.removeEventListener('keydown', onKey, true); return; }
    if (e.key !== 'Tab' || e.metaKey || e.ctrlKey || e.altKey) return;
    // tabindex=-1(화살표로만 옮기는 목록 항목 등)은 Tab 순서에 없다 — 끝으로 치면 마지막 Tab 이 모달 밖으로 빠졌다.
    const items = [...box.querySelectorAll(FOCUSABLE)].filter((el) => el.tabIndex >= 0 && el.getClientRects().length > 0 && !el.closest('[hidden], [inert]'));
    const at = document.activeElement;
    if (!items.length) { e.preventDefault(); if (box.tabIndex >= -1) box.focus({ preventScroll: true }); return; }
    const first = items[0];
    const last = items[items.length - 1];
    if (!box.contains(at)) { e.preventDefault(); (e.shiftKey ? last : first).focus(); return; }
    if (e.shiftKey && (at === first || at === box)) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && at === last) { e.preventDefault(); first.focus(); }
  };
  document.addEventListener('keydown', onKey, true);
  return release;
}

export function bindTabKeys(tablist) {
  if (tablist.classList.contains('page-tabs')) watchEdges(tablist);
  tablist.addEventListener('keydown', (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tabs = [...tablist.querySelectorAll('[role="tab"]')];
    const i = tabs.indexOf(e.target.closest('[role="tab"]'));
    if (i < 0) return;
    const j = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (j === undefined) return;
    e.preventDefault();
    const next = tabs[(j + tabs.length) % tabs.length];
    next.focus();
    next.click();
  });
}
