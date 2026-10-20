// 접근성·키보드 포커스 — 원본 Components/CardAccessibility, CViewCore/Focus (DSN-06).
// - 글자 없는 아이콘 버튼에 title 을 aria-label 로 옮긴다(화면 낭독기가 이름을 읽게).
// - 카드 격자에서 화살표 키로 포커스를 옮기고, Enter·Space 로 연다(카드의 클릭 동작 그대로).

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

const ICON_BUTTON = 'button[title]:not([aria-label]), [role=button][title]:not([aria-label])';

function labelOne(b) {
  if (!b.textContent.trim() || b.classList.contains('icon-btn')) b.setAttribute('aria-label', b.title);
}

function labelIconButtons(root) {
  if (root.matches && root.matches(ICON_BUTTON)) labelOne(root);
  for (const b of root.querySelectorAll(ICON_BUTTON)) labelOne(b);
}

// 채팅 목록은 줄이 프레임마다 들어오지만 아이콘 버튼이 없다 — 그 안의 변화는 보지 않는다.
const SKIP = '.chat-list, .mc-merged-list';

export function installA11y() {
  labelIconButtons(document);
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
      for (const n of queue) if (n.isConnected) labelIconButtons(n);
      queue.clear();
    });
  }).observe(document.body, { childList: true, subtree: true });

  document.addEventListener('keydown', (e) => {
    const card = e.target && e.target.closest && e.target.closest('.card[tabindex]');
    if (!card || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === 'Enter' || e.key === ' ') {
      if (e.target !== card) return;
      e.preventDefault();
      card.click();
      return;
    }
    const dir = DIRS[e.key];
    if (!dir) return;
    const scope = card.parentElement;
    const cards = [...scope.querySelectorAll(':scope > .card[tabindex], :scope > * > .card[tabindex]')].filter((c) => c.offsetParent);
    const i = cards.indexOf(card);
    const j = nextIndex(cards.map((c) => c.getBoundingClientRect()), i, dir);
    if (j >= 0) {
      e.preventDefault();
      cards[j].focus();
      cards[j].scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
  });
}
