// 플레이어 ↔ 채팅 사이 너비 조절 — 원본 MultiChatPanelWidthPolicy / SplitResizeGeometry 근사.
//
// 채팅 너비는 [min, max] 안에서, 그리고 플레이어 쪽이 최소 폭을 지키는 만큼만 넓힌다.
// 드래그로 바꾸고, 두 번 누르면 기본값, 채팅 머리의 버튼으로 접고 편다. 화면별로 기억한다.

const ICON_COLLAPSE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 6l6 6-6 6M4 4v16"/></svg>';
const ICON_CHAT = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 5a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-5 4V5Z"/></svg>';

export const CHAT_WIDTH = Object.freeze({ min: 260, max: 620, default: 340, minStage: 420 });

/** 채팅 너비 보정 — 순수 함수. 컨테이너가 좁으면 플레이어 최소 폭을 먼저 지킨다. */
export function clampChatWidth(width, containerWidth, limits = CHAT_WIDTH) {
  const w = Number.isFinite(width) ? width : limits.default;
  const roomForChat = Number.isFinite(containerWidth) && containerWidth > 0
    ? containerWidth - limits.minStage : limits.max;
  const upper = Math.max(limits.min, Math.min(limits.max, roomForChat));
  return Math.round(Math.min(upper, Math.max(limits.min, w)));
}

function load(key) {
  try { return JSON.parse(localStorage.getItem(key) || 'null'); } catch { return null; }
}

function save(key, value) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* 이번 실행만 */ }
}

/**
 * @param {{ container: HTMLElement, chat: HTMLElement, key: string }} opts
 *   container — 플레이어 영역과 채팅을 가로로 담은 요소, chat — `.chat` 패널
 */
export function installChatSplitter({ container, chat, key }) {
  const storeKey = `cview.chatPanel.${key}`;
  const state = { width: CHAT_WIDTH.default, collapsed: false, ...(load(storeKey) || {}) };

  const handle = document.createElement('div');
  handle.className = 'chat-splitter';
  handle.title = '끌어서 채팅 너비 조절 · 두 번 눌러 기본값';
  handle.setAttribute('role', 'separator');
  handle.setAttribute('aria-orientation', 'vertical');
  chat.before(handle);

  const collapse = document.createElement('button');
  collapse.className = 'icon-btn chat-collapse';
  collapse.title = '채팅 접기';
  collapse.setAttribute('aria-label', '채팅 접기');
  collapse.innerHTML = ICON_COLLAPSE;
  chat.querySelector('.chat-header').append(collapse);

  const reopen = document.createElement('button');
  reopen.className = 'chat-reopen';
  reopen.title = '채팅 펼치기';
  reopen.innerHTML = `${ICON_CHAT}<span>채팅</span>`;
  container.append(reopen);

  const apply = () => {
    const width = clampChatWidth(state.width, container.getBoundingClientRect().width);
    chat.style.width = `${width}px`;
    chat.classList.toggle('collapsed', state.collapsed);
    handle.hidden = state.collapsed;
    reopen.hidden = !state.collapsed;
    handle.setAttribute('aria-valuenow', String(width));
  };
  const persist = () => save(storeKey, { width: state.width, collapsed: state.collapsed });

  handle.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    handle.setPointerCapture(e.pointerId);
    container.classList.add('resizing');
    const right = container.getBoundingClientRect().right;
    const move = (ev) => {
      state.width = clampChatWidth(right - ev.clientX, container.getBoundingClientRect().width);
      apply();
    };
    const up = () => {
      handle.removeEventListener('pointermove', move);
      container.classList.remove('resizing');
      persist();
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up, { once: true });
    handle.addEventListener('pointercancel', up, { once: true });
  });
  handle.addEventListener('dblclick', () => {
    state.width = CHAT_WIDTH.default;
    apply();
    persist();
  });
  collapse.addEventListener('click', () => {
    state.collapsed = true;
    apply();
    persist();
  });
  reopen.addEventListener('click', () => {
    state.collapsed = false;
    apply();
    persist();
  });
  new ResizeObserver(() => apply()).observe(container);
  apply();
}
