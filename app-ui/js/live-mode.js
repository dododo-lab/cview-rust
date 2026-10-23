// 라이브 메뉴의 두 보기 — 한 방송(라이브)과 여러 방송(멀티라이브).
// 멀티라이브는 사이드바에 따로 두지 않고 라이브 화면 머리줄의 전환 버튼으로 오간다.
// 제목은 화면마다 정해져 있어(라이브 · 멀티라이브) 전환하면 저절로 바뀐다.
export const liveIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 6a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Zm7 3v6l5-3-5-3Z"/></svg>';
export const multiLiveIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 5a2 2 0 0 1 2-2h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5Zm9 9a2 2 0 0 1 2-2h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-5a2 2 0 0 1-2-2v-5ZM14 3h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2ZM5 12h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2Z" opacity=".9"/></svg>';

/** 사이드바에서 라이브 메뉴가 맡는 화면들. */
export const LIVE_MODES = ['live', 'multilive'];

/** 머리줄 전환 버튼 — `active` 는 'live' | 'multilive'. 누르면 `bindLiveModeSwitch` 가 그 화면으로 보낸다. */
export function liveModeSwitch(active) {
  const seg = (mode, icon, label, tip) => `<button class="seg icon${mode === active ? ' active' : ''}" data-live-mode="${mode}" title="${tip}" aria-label="${label}" aria-pressed="${mode === active}">${icon}</button>`;
  return `<div class="segmented live-mode-seg" role="group" aria-label="라이브 보기">
    ${seg('live', liveIcon, '라이브', '라이브 — 한 방송 보기')}${seg('multilive', multiLiveIcon, '멀티라이브', '멀티라이브 — 여러 방송 함께 보기')}
  </div>`;
}

/** 전환 버튼의 선택 표시만 바꾼다(멀티라이브 화면이 라이브 단독 무대로 쓰일 때). */
export function setLiveModeActive(root, active) {
  for (const b of root.querySelectorAll('[data-live-mode]')) {
    const on = b.dataset.liveMode === active;
    b.classList.toggle('active', on);
    b.setAttribute('aria-pressed', String(on));
  }
}

/** 문서 어디서든 전환 버튼을 누르면 그 화면으로 — 한 번만 건다. */
export function bindLiveModeSwitch(navigate) {
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-live-mode]');
    if (!b || b.classList.contains('active')) return;
    navigate(b.dataset.liveMode);
  });
}
