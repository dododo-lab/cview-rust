// 멀티라이브 패인 모션 — 채널 추가·제거, 배치 바꾸기의 움직임과 끌어서 자리·메인 바꾸기.
//
// - 패인은 iframe 이라 DOM 에서 옮기지 않는다(옮기면 다시 로드된다). 그래서 FLIP 으로 움직인다:
//   바뀌기 전 자리를 재 두고, 새 배치를 적용한 뒤, 이전 자리에서 새 자리로 transform 만 애니메이션한다.
//   transform·opacity 만 쓰므로 영상이 여러 개 재생 중이어도 합성기(GPU)에서 돈다.
// - 닫힌 패인은 마지막 자리에 잠깐 떠 있다가 줄어들며 사라진다 — 그 뒤에 재생을 멈춘다.
// - 끌기는 포인터 이벤트로 직접 한다(HTML5 끌어 놓기는 iframe 위에서 자주 끊긴다).
//   메인(포커스) 패인 위에 놓으면 맞바꾸고 메인이 된다 — 규칙은 Rust `drop_tile`.

const REDUCED_MOTION = window.matchMedia('(prefers-reduced-motion: reduce)');
const EASE = 'cubic-bezier(0.32, 0.72, 0, 1)';
const MOVE_MS = 340;
const ENTER_MS = 300;
const EXIT_MS = 220;
const DRAG_THRESHOLD = 6;

export function motionEnabled() {
  return !REDUCED_MOTION.matches;
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

/** 새 배치가 적용된 뒤 — 이전 자리에서 새 자리로 미끄러지고, 새로 생긴 패인은 커지며 나타난다. */
export function playLayoutMotion(panes, before) {
  if (!motionEnabled()) return;
  for (const [slot, p] of panes) {
    if (p.el.hidden || p.exiting) continue;
    const b = p.el.getBoundingClientRect();
    if (!b.width || !b.height) continue;
    const a = before.get(slot);
    if (!a) {
      p.anim?.cancel();
      p.anim = p.el.animate([
        { opacity: 0, transform: 'scale(0.9)' },
        { opacity: 1, transform: 'none' },
      ], { duration: ENTER_MS, easing: EASE });
      continue;
    }
    const dx = a.left - b.left, dy = a.top - b.top;
    const sx = a.width / b.width, sy = a.height / b.height;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) continue;
    p.anim?.cancel();
    p.anim = p.el.animate([
      { transformOrigin: '0 0', transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
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
    // 놓은 자리에서 새 자리(또는 제자리)로 미끄러지게 — 다음 render 가 지금 위치를 재고 transform 을 푼다.
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
