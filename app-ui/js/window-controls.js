// Windows 창 단추 — 제목 막대·메뉴 막대 없는 창(tauri.windows.conf.json)의 오른쪽 위 메뉴·최소화·최대화·닫기.
// macOS 신호등처럼 화면 안에 그린다. 메뉴 버튼(또는 Alt 한 번)은 숨긴 메뉴 막대의 메뉴를 그 자리에 띄운다 —
// 단축키는 메뉴 막대를 숨겨도 그대로 산다. macOS·브라우저 미리보기에서는 아무것도 하지 않는다.
import { api, isTauri } from './api.js';
import { IS_WINDOWS } from './platform.js';

const ICONS = {
  menu: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"><path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11"/></svg>',
  min: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><path d="M0 5.5h10"/></svg>',
  max: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><rect x="0.5" y="0.5" width="9" height="9" rx="1.2"/></svg>',
  restore: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><rect x="0.5" y="2.5" width="7" height="7" rx="1"/><path d="M2.5 2.5V1.6c0-.6.5-1.1 1.1-1.1h4.8c.6 0 1.1.5 1.1 1.1v4.8c0 .6-.5 1.1-1.1 1.1h-.9"/></svg>',
  close: '<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="round"><path d="m.5.5 9 9M9.5.5l-9 9"/></svg>',
};

/** 창 단추 묶음을 붙인다. Windows 가 아니면 null. */
export function installWindowControls() {
  if (!IS_WINDOWS) return null;
  const win = isTauri && window.__TAURI__.window ? window.__TAURI__.window.getCurrentWindow() : null;
  const bar = document.createElement('div');
  bar.className = 'win-controls';
  bar.setAttribute('role', 'toolbar');
  bar.setAttribute('aria-label', '창 조작');
  bar.innerHTML = `
    <button type="button" class="wc-btn wc-menu" title="메뉴 (Alt)" aria-label="메뉴" aria-haspopup="menu">${ICONS.menu}</button>
    <span class="wc-sep" aria-hidden="true"></span>
    <button type="button" class="wc-btn wc-min" title="최소화" aria-label="최소화">${ICONS.min}</button>
    <button type="button" class="wc-btn wc-max" title="최대화" aria-label="최대화">${ICONS.max}</button>
    <button type="button" class="wc-btn wc-close" title="닫기" aria-label="닫기">${ICONS.close}</button>`;
  document.body.append(bar);
  const menuBtn = bar.querySelector('.wc-menu');
  const maxBtn = bar.querySelector('.wc-max');

  const openMenu = () => {
    const r = menuBtn.getBoundingClientRect();
    menuBtn.classList.add('open');
    // 네이티브 팝업은 닫힐 때까지 돌아오지 않는다 — 끝나면 눌림 표시를 걷는다.
    api.windowMenuPopup(r.left, r.bottom).catch(() => {}).finally(() => menuBtn.classList.remove('open'));
  };
  menuBtn.addEventListener('click', openMenu);
  bar.querySelector('.wc-min').addEventListener('click', () => win && win.minimize());
  maxBtn.addEventListener('click', () => win && win.toggleMaximize());
  bar.querySelector('.wc-close').addEventListener('click', () => win && win.close());
  // 단추 위에서 누른 마우스가 아래의 끌기 영역(data-tauri-drag-region)으로 새지 않게.
  bar.addEventListener('mousedown', (e) => e.stopPropagation());
  bar.addEventListener('dblclick', (e) => e.stopPropagation());

  // 최대화 ↔ 이전 크기 아이콘
  const syncMax = async () => {
    if (!win) return;
    let maxed = false;
    try { maxed = await win.isMaximized(); } catch { /* 무시 */ }
    document.documentElement.classList.toggle('is-maximized', maxed);
    maxBtn.innerHTML = maxed ? ICONS.restore : ICONS.max;
    maxBtn.title = maxed ? '이전 크기로' : '최대화';
    maxBtn.setAttribute('aria-label', maxBtn.title);
  };
  if (win) { win.onResized(syncMax); syncMax(); }

  // Alt 한 번(다른 키·클릭 없이 눌렀다 뗌) — Windows 메뉴 막대처럼 메뉴를 연다.
  let altAlone = false;
  window.addEventListener('keydown', (e) => { altAlone = e.key === 'Alt' && !e.repeat && !e.ctrlKey && !e.shiftKey && !e.metaKey; }, true);
  window.addEventListener('mousedown', () => { altAlone = false; }, true);
  window.addEventListener('blur', () => { altAlone = false; });
  window.addEventListener('keyup', (e) => {
    if (e.key !== 'Alt' || !altAlone) return;
    altAlone = false;
    if (!document.hasFocus() || document.fullscreenElement) return;
    e.preventDefault();
    openMenu();
  }, true);
  return bar;
}
