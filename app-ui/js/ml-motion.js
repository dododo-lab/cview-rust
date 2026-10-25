// 멀티라이브 패인 모션 — 채널 추가·제거, 배치 바꾸기의 움직임과 끌어서 자리·메인 바꾸기.
//
// - 패인은 iframe 이라 DOM 에서 옮기지 않는다(옮기면 다시 로드된다). 그래서 FLIP 으로 움직인다:
//   바뀌기 전 자리를 재 두고, 새 배치를 적용한 뒤, 이전 자리에서 새 자리로 transform 만 애니메이션한다.
//   transform·opacity 만 쓰므로 영상이 여러 개 재생 중이어도 합성기(GPU)에서 돈다.
// - 닫힌 패인은 마지막 자리에 잠깐 떠 있다가 줄어들며 사라진다 — 그 뒤에 재생을 멈춘다.
// - 끌기는 포인터 이벤트로 직접 한다(HTML5 끌어 놓기는 iframe 위에서 자주 끊긴다).
//   메인(포커스) 패인 위에 놓으면 맞바꾸고 메인이 된다 — 규칙은 Rust `drop_tile`.

import { motionReduced } from './motion.js';

const EASE = 'cubic-bezier(0.32, 0.72, 0, 1)';
const MOVE_MS = 340;
const ENTER_MS = 300;
const ENTER_HOLD_MS = 90; // 새 패인이 나타나기 전 잠깐 — 추가 직후의 선택(메인으로) 응답을 기다린다
const EXIT_MS = 220;
const DRAG_THRESHOLD = 6;

/** 시스템 모션 줄이기·설정 "절약" 둘 다 — Web Animations 는 CSS 의 시간 0 규칙을 받지 않는다. */
export function motionEnabled() {
  return !motionReduced();
}

/** 보이는 패인의 화면 위 자리(애니메이션 중이면 지금 보이는 자리). */
export function snapshotPanes(panes) {
  const rects = new Map();
  for (const [slot, p] of panes) {
    if (p.el.hidden || p.exiting) continue;
    const r = p.el.getBoundingClientRect();
    if (r.width && r.height) rects.set(slot, r); // 한 채널 크게에서 가려진 패인(display:none)은 '새로 나타남'으로
  }
  return rects;
}

/**
 * 새 배치가 적용된 뒤 — 이전 자리에서 새 자리로 미끄러지고, 새로 생긴 패인은 커지며 나타난다.
 * - 이동은 균등 scale(가로·세로 같은 배율, 이전 칸 안에 들게) + 가운데 맞춤 — scale(sx, sy) 는 340ms 동안
 *   영상·글자·모서리를 찌그러뜨렸다.
 * - 새 패인은 ENTER_HOLD_MS 동안 보이지 않다가 나타난다. 그 사이 배치가 또 바뀌면(채널 추가 직후 선택 → 메인으로)
 *   옮겨 가지 않고 최종 자리에서 나타난다 — 서브 칸에 나타났다가 메인으로 다시 날아가는 두 번 움직임이 없다.
 */
export function playLayoutMotion(panes, before) {
  if (!motionEnabled()) return;
  const now = performance.now();
  for (const [slot, p] of panes) {
    if (p.el.hidden || p.exiting) continue;
    const b = p.el.getBoundingClientRect();
    if (!b.width || !b.height) continue;
    const a = before.get(slot);
    const holding = p.enterHoldUntil && now < p.enterHoldUntil;
    if (!a || holding) {
      if (!holding) p.enterHoldUntil = now + ENTER_HOLD_MS;
      p.anim?.cancel();
      p.anim = p.el.animate([
        { opacity: 0, transform: 'scale(0.9)' },
        { opacity: 1, transform: 'none' },
      ], { duration: ENTER_MS, delay: Math.max(0, p.enterHoldUntil - now), easing: EASE, fill: 'backwards' });
      continue;
    }
    const s = Math.min(a.width / b.width, a.height / b.height);
    const dx = (a.left + a.width / 2) - (b.left + (b.width * s) / 2);
    const dy = (a.top + a.height / 2) - (b.top + (b.height * s) / 2);
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(s - 1) < 0.01) continue;
    p.anim?.cancel();
    p.anim = p.el.animate([
      { transformOrigin: '0 0', transform: `translate(${dx}px, ${dy}px) scale(${s})` },
      { transformOrigin: '0 0', transform: 'none' },
    ], { duration: MOVE_MS, easing: EASE });
  }
}

/**
 * 닫힌 패인 — 마지막 자리(rect, 화면 좌표)에 띄워 둔 채 줄어들며 사라지게 하고, 끝나면 done() 을 부른다.
 * 그 사이 같은 슬롯이 다시 쓰이면 cancelExit() 이 되돌리고 done() 은 부르지 않는다.
 */
export function playExit(pane, stage, rect, done) {
  const s = stage.getBoundingClientRect();
  if (!motionEnabled() || !rect || !rect.width) { done(); return; }
  const token = {};
  pane.exiting = token;
  pane.anim?.cancel();
  const st = pane.el.style;
  st.position = 'absolute';
  st.left = `${rect.left - s.left - stage.clientLeft}px`;
  st.top = `${rect.top - s.top - stage.clientTop}px`;
  st.width = `${rect.width}px`;
  st.height = `${rect.height}px`;
  st.zIndex = '2';
  st.pointerEvents = 'none';
  pane.anim = pane.el.animate([
    { opacity: 1, transform: 'none' },
    { opacity: 0, transform: 'scale(0.88)' },
  ], { duration: EXIT_MS, easing: 'cubic-bezier(0.4, 0, 1, 1)', fill: 'forwards' });
  pane.anim.finished.then(() => {
    if (pane.exiting !== token) return;
    clearExit(pane);
    done();
  }, () => {});
}

export function cancelExit(pane) {
  if (!pane.exiting) return;
  clearExit(pane);
}

function clearExit(pane) {
  pane.exiting = null;
  pane.anim?.cancel();
  pane.anim = null;
  const st = pane.el.style;
  st.position = ''; st.left = ''; st.top = ''; st.width = ''; st.height = ''; st.zIndex = ''; st.pointerEvents = '';
}

/**
 * 패인 끌기 — 패인 어디서든(또는 손잡이) 눌러 끌면 패인이 포인터를 따라오고, 놓은 패인과 자리를 바꾼다.
 * @param {object} page MultiLivePage — panes, stage, state, watchSlot, single 을 읽는다.
 * @param {(draggedId: string, targetId: string, pane: object) => void} onDrop
 */
export function bindPaneDrag(page, onDrop) {
  const { stage } = page;
  let drag = null;

  const paneOf = (el) => {
    const node = el && el.closest('.ml-pane');
    return node ? page.panes.get(Number(node.dataset.slot)) : null;
  };

  stage.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || (drag && drag.started)) return;
    drag = null; // 무대 밖에서 놓아 끝나지 않은 '누르기만 한' 끌기는 버린다
    if (!e.target.closest('.ml-pane-hit, .ml-drag')) return;
    const pane = paneOf(e.target);
    const n = (page.state.sessions || []).length;
    if (!pane || !pane.sessionId || pane.exiting || n < 2 || page.watchSlot != null || page.single != null) return;
    drag = { pane, id: e.pointerId, x: e.clientX, y: e.clientY, started: false, target: null, handle: e.target };
  });

  const start = () => {
    const { pane } = drag;
    drag.handle.setPointerCapture?.(drag.id);
    pane.anim?.cancel();
    pane.anim = null;
    drag.rects = [];
    for (const p of page.panes.values()) {
      if (p === pane || !p.sessionId || p.el.hidden || p.exiting) continue;
      drag.rects.push({ p, r: p.el.getBoundingClientRect() });
    }
    drag.started = true;
    stage.classList.add('ml-dragging');
    pane.el.classList.add('ml-lifted');
  };

  const setTarget = (target) => {
    if (drag.target === target) return;
    if (drag.target) drag.target.el.classList.remove('drop-target');
    drag.target = target;
    // 무엇이 될지는 든 패인(포인터 바로 아래) 가운데에 띄운다 — 놓을 패인은 든 패인에 가려지므로.
    const hint = drag.pane.el.querySelector('.ml-drop-hint');
    if (hint) hint.textContent = target ? dropHint(page, drag.pane, target) : '';
    drag.pane.el.classList.toggle('has-drop', Boolean(target));
    if (target) target.el.classList.add('drop-target');
  };

  stage.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.started) {
      if (Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
      start();
    }
    drag.pane.el.style.transform = `translate(${dx}px, ${dy}px) scale(0.94)`;
    const hit = drag.rects.find(({ r }) => e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom);
    setTarget(hit ? hit.p : null);
  });

  const end = (e, cancelled) => {
    if (!drag || e.pointerId !== drag.id) return;
    const d = drag;
    drag = null;
    if (!d.started) return;
    // 끌기 뒤에 오는 click 은 선택으로 보지 않는다.
    page.suppressClickUntil = performance.now() + 400;
    const target = d.target;
    if (target) target.el.classList.remove('drop-target');
    stage.classList.remove('ml-dragging');
    // 놓은 자리에서 새 자리(또는 제자리)로 미끄러지게 — 새 순서가 담긴 render 가 지금 위치를 재고 transform 을 푼다
    // (onDrop 이 응답을 기다리는 동안 옛 순서 상태로는 풀지 않는다 — multilive.js dragAwait).
    page.dragRelease = d.pane;
    if (!cancelled && target && target.sessionId && d.pane.sessionId && target !== d.pane) onDrop(d.pane.sessionId, target.sessionId, d.pane);
    else page.render(page.state);
  };
  stage.addEventListener('pointerup', (e) => end(e, false));
  stage.addEventListener('pointercancel', (e) => end(e, true));
  stage.addEventListener('lostpointercapture', (e) => { if (drag && drag.started && e.pointerId === drag.id) end(e, true); });
}

/** 놓을 패인 위에 띄울 말 — 포커스 배치에서 메인이 걸리면 "메인으로", 아니면 "자리 바꾸기". */
function dropHint(page, dragged, target) {
  const st = page.state || {};
  if (st.layout === 'focus') {
    const main = st.focusMainId || st.selectedId || (st.sessions || [])[0]?.id;
    if (target.sessionId === main) return '메인으로';
    if (dragged.sessionId === main) return '여기와 메인 바꾸기';
  }
  return '자리 바꾸기';
}

/** 끌고 놓은 패인 — 지금 보이는 자리를 잰 뒤 끌기 transform 을 푼다(이후 FLIP 이 새 자리로 데려간다). */
export function releaseDragged(pane) {
  if (!pane) return;
  pane.el.classList.remove('ml-lifted', 'has-drop');
  pane.el.style.transform = '';
}
