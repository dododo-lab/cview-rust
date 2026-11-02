// 직접 새로고침할 때의 로딩 표시(모든 메뉴 공통) — 시작 로딩 화면의 로고가 가운데 뜨고, 뒤의 화면은
// 흐리고 약간 어둡게 가린다. 사용자가 새로고침 버튼·⌘R·메뉴·팔레트로 고를 때만 쓴다. 첫 진입(스켈레톤)이나
// 자동 갱신(빈 썸네일 다시 채우기·주기 갱신·네트워크 복구)에는 띄우지 않는다.
//
// - 빨리 끝나면(SHOW_DELAY_MS 안) 아예 띄우지 않고, 한번 떴으면 MIN_VISIBLE_MS 는 보인다 — 번쩍이지 않게.
// - 성공·실패·다른 새로고침·다른 화면으로 이동(cancelRefreshVeil) 모두에서 걷힌다. 응답이 끝나지 않아도
//   MAX_MS 뒤에는 걷는다.
// - 흐림은 화면 본문(.page-body)에만 건다 — 머리줄(제목·새로고침 버튼)은 그대로 또렷하고 눌린다.
import { motionReduced } from './motion.js';

const SHOW_DELAY_MS = 160;
const MIN_VISIBLE_MS = 480;
const MAX_MS = 15000;
const LEAVE_MS = 220;

// 시작 로딩 화면(index.html #splash)과 같은 로고 — 그라디언트 id 만 겹치지 않게 바꿨다.
const BUBBLE = 'M330 262 H694 A98 98 0 0 1 792 360 V598 A98 98 0 0 1 694 696 H452 L342 786 A14 14 0 0 1 320 774 L330 696 A98 98 0 0 1 232 598 V360 A98 98 0 0 1 330 262 Z';
const LOGO = `<svg class="rv-logo" viewBox="214 190 640 600" aria-hidden="true">
  <defs>
    <linearGradient id="rvM" x1=".1" y1="0" x2=".9" y2="1"><stop offset="0" stop-color="#00D188"/><stop offset="1" stop-color="#00A3D9"/></linearGradient>
    <linearGradient id="rvH" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".28"/><stop offset=".45" stop-color="#fff" stop-opacity="0"/></linearGradient>
    <linearGradient id="rvS" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset=".5" stop-color="#fff" stop-opacity=".55"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
    <clipPath id="rvClip"><path d="${BUBBLE}"/></clipPath>
  </defs>
  <g class="rv-bubble">
    <path fill="url(#rvM)" d="${BUBBLE}"/>
    <path fill="url(#rvH)" d="${BUBBLE}"/>
    <g clip-path="url(#rvClip)"><rect class="rv-sheen" x="132" y="190" width="200" height="620" fill="url(#rvS)" transform="skewX(-18)"/></g>
  </g>
  <path class="rv-play" fill="#fff" stroke="#fff" stroke-width="34" stroke-linejoin="round" d="M462 392 L462 566 L612 479 Z"/>
  <circle class="rv-ping" cx="776" cy="258" r="44" fill="none" stroke="#FF3B30" stroke-width="14"/>
  <circle class="rv-dot-ring" cx="776" cy="258" r="64"/>
  <circle class="rv-dot" cx="776" cy="258" r="44" fill="#FF3B30"/>
</svg>`;

let veil = null;
let active = null; // { root, body, shownAt, showTimer, maxTimer }
let leaveTimer = 0;

function ensureVeil() {
  if (veil && veil.isConnected) return veil;
  const host = document.querySelector('.content');
  if (!host) return null;
  veil = document.createElement('div');
  veil.className = 'refresh-veil';
  veil.hidden = true;
  veil.setAttribute('role', 'status');
  veil.setAttribute('aria-live', 'polite');
  veil.innerHTML = `<div class="rv-card">${LOGO}<span class="rv-label">새로고침 중…</span></div>`;
  host.append(veil);
  return veil;
}

function reveal(state) {
  if (active !== state) return;
  const el = ensureVeil();
  if (!el) return;
  clearTimeout(leaveTimer);
  state.shownAt = performance.now();
  el.classList.remove('leaving');
  el.hidden = false;
  if (state.body) state.body.classList.add('is-refreshing');
  state.root.setAttribute('aria-busy', 'true');
}

function finish(state, { immediate = false } = {}) {
  if (!state || state.done) return;
  state.done = true;
  clearTimeout(state.showTimer);
  clearTimeout(state.maxTimer);
  if (active === state) active = null;
  const drop = () => {
    // 다음 새로고침이 이미 덮개를 다시 띄웠으면 그쪽이 쓰는 것(같은 화면의 흐림·덮개)은 걷지 않는다.
    const next = active && active.shownAt ? active : null;
    if (state.body && !(next && next.body === state.body)) state.body.classList.remove('is-refreshing');
    if (!(next && next.root === state.root)) state.root.removeAttribute('aria-busy');
    if (next || !veil) return;
    if (immediate || motionReduced()) { veil.hidden = true; veil.classList.remove('leaving'); return; }
    veil.classList.add('leaving');
    clearTimeout(leaveTimer);
    leaveTimer = setTimeout(() => { if (!(active && active.shownAt)) { veil.hidden = true; veil.classList.remove('leaving'); } }, LEAVE_MS);
  };
  if (!state.shownAt) return; // 띄우기 전에 끝났다 — 아무것도 보이지 않았다
  const wait = immediate ? 0 : MIN_VISIBLE_MS - (performance.now() - state.shownAt);
  if (wait > 0) setTimeout(drop, wait); else drop();
}

/**
 * 사용자가 직접 고른 새로고침을 덮개와 함께 실행한다. `run()` 이 돌려준 약속이 끝나면(성공·실패) 걷힌다.
 * `root` 는 그 화면의 `.page` — 덮는 동안 `refreshVeiled(root)` 가 참이라 화면이 보던 내용을 스켈레톤으로 바꾸지 않는다.
 */
export async function withRefreshVeil(root, run) {
  if (active) finish(active, { immediate: true });
  const state = { root, body: root && (root.querySelector('.page-body') || root), shownAt: 0, done: false };
  active = state;
  state.showTimer = setTimeout(() => reveal(state), SHOW_DELAY_MS);
  state.maxTimer = setTimeout(() => finish(state), MAX_MS);
  try {
    return await run();
  } finally {
    finish(state);
  }
}

/** 이 화면이 지금 덮개 아래에서 새로고침 중인가 — 그렇다면 보던 내용을 그대로 두고(흐리게) 새 내용으로 바꾼다. */
export function refreshVeiled(root) {
  return Boolean(active && active.root === root);
}

/** 다른 화면으로 옮기면 바로 걷는다(남은 요청은 그 화면이 알아서 버린다). */
export function cancelRefreshVeil() {
  if (active) finish(active, { immediate: true });
}
