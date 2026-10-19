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

function labelIconButtons(root) {
  for (const b of root.querySelectorAll('button[data-tip]:not([aria-label])')) {
    if (b.dataset.tip) b.setAttribute('aria-label', b.dataset.tip);
  }
  for (const b of root.querySelectorAll('button[title]:not([aria-label]), [role=button][title]:not([aria-label])')) {
    if (!b.textContent.trim() || b.classList.contains('icon-btn')) b.setAttribute('aria-label', b.title);
  }
}

const FOCUSABLE = 'button, a[href], input:not([type="hidden"]), select, textarea, [tabindex]';

function focusable(root) {
  return [...root.querySelectorAll(FOCUSABLE)].filter((el) => el.tabIndex >= 0
    && !el.disabled && !el.closest('[hidden], [inert]') && el.getClientRects().length);
}

/** Keep visual tabs, their accessible selection, and keyboard order in sync. */
export function syncTabs(root) {
  for (const list of root.querySelectorAll('[role="tablist"]')) {
    const tabs = [...list.querySelectorAll('button')];
    const selected = tabs.find((tab) => tab.classList.contains('active')) || tabs[0];
    for (const tab of tabs) {
      tab.setAttribute('role', 'tab');
      tab.setAttribute('aria-selected', String(tab === selected));
      tab.tabIndex = tab === selected ? 0 : -1;
    }
  }
}

export function installA11y() {
  let activeDialog = null;
  let returnFocus = null;
  let outsideFocus = document.activeElement;
  let headingId = 0;
  let inerted = [];
  const sync = () => {
    labelIconButtons(document);
    syncTabs(document);
    const dialogs = [...document.querySelectorAll('[role="dialog"]')].filter((el) => el.getClientRects().length);
    for (const dialog of dialogs) {
      dialog.setAttribute('aria-modal', 'true');
      if (!dialog.hasAttribute('aria-label') && !dialog.hasAttribute('aria-labelledby')) {
        const title = dialog.querySelector('h1, h2, h3');
        if (title) {
          if (!title.id) title.id = `dialog-title-${++headingId}`;
          dialog.setAttribute('aria-labelledby', title.id);
        }
      }
    }
    const next = dialogs.at(-1) || null;
    if (next !== activeDialog) {
      for (const el of inerted) el.inert = false;
      inerted = [];
      const previous = activeDialog;
      activeDialog = next;
      if (next) {
        // Keep the background out of the keyboard and accessibility trees.
        for (let branch = next; branch.parentElement; branch = branch.parentElement) {
          for (const sibling of branch.parentElement.children) {
            if (sibling !== branch && sibling instanceof HTMLElement && !sibling.inert) {
              sibling.inert = true;
              inerted.push(sibling);
            }
          }
          if (branch.parentElement === document.body) break;
        }
        if (!previous) returnFocus = outsideFocus;
        if (!next.contains(document.activeElement)) {
          next.tabIndex = -1;
          (focusable(next)[0] || next).focus();
        }
      } else if (returnFocus?.isConnected) {
        returnFocus.focus();
        returnFocus = null;
      }
    }
  };
  sync();
  let pending = false;
  new MutationObserver(() => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => { pending = false; sync(); });
  }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['hidden', 'class', 'title', 'data-tip'] });

  document.addEventListener('focusin', (e) => {
    if (!e.target.closest('[role="dialog"]')) outsideFocus = e.target;
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Tab' && activeDialog) {
      const items = focusable(activeDialog);
      const first = items[0] || activeDialog;
      const last = items.at(-1) || activeDialog;
      if (!activeDialog.contains(document.activeElement) || (!e.shiftKey && document.activeElement === last)) {
        e.preventDefault(); first.focus();
      } else if (e.shiftKey && document.activeElement === first) {
        e.preventDefault(); last.focus();
      }
      return;
    }
    const tab = e.target.closest('[role="tab"], [role="radio"]');
    const list = tab?.closest('[role="tablist"], [role="radiogroup"]');
    if (!list || e.altKey || e.ctrlKey || e.metaKey) return;
    const tabs = [...list.querySelectorAll('[role="tab"], [role="radio"]')].filter((el) => !el.disabled);
    let index = tabs.indexOf(tab);
    if (e.key === 'ArrowRight') index = (index + 1) % tabs.length;
    else if (e.key === 'ArrowLeft') index = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === 'Home') index = 0;
    else if (e.key === 'End') index = tabs.length - 1;
    else return;
    e.preventDefault();
    tabs[index]?.focus();
    tabs[index]?.click();
  }, true);

  document.addEventListener('keydown', (e) => {
    const card = e.target && e.target.closest && e.target.closest('.card[tabindex]');
    if (!card || e.target !== card || e.metaKey || e.ctrlKey || e.altKey) return;
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
