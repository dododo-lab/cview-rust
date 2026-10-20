// 백그라운드 절약 — 창이 오래 안 보이면(최소화·⌘H·완전히 가려짐) 재생 중인 모든 플레이어의
// 자동 화질에 낮은 높이 상한을 건다. 소리는 계속 나야 해서 재생은 멈추지 않고, 보이지 않는 1080p
// 디코딩만 줄인다(perf/baseline.md (f)). 다시 보이면 바로 상한을 푼다. "최고 화질 유지" 고정은 건드리지 않는다.
const HIDDEN_DELAY_MS = 15000;
const BACKGROUND_HEIGHT = 360;

// 메뉴를 옮겨 화면에서 빠진 플레이어(예: 멀티라이브를 보는 동안의 라이브 시청 화면) — 창이 다시 보여도 상한을 유지한다.
const menuHidden = new WeakSet();

/** PiP 로 떠 있는 영상 — 메인 창에서 빠져도 사용자는 보고 있으니 상한을 걸지 않는다. */
function inPip(iframe) {
  try {
    const d = iframe.contentDocument;
    const v = d && d.getElementById('video');
    return !!v && (v.webkitPresentationMode === 'picture-in-picture' || d.pictureInPictureElement === v);
  } catch { return false; }
}

/** 화면에서 빠진 플레이어에 백그라운드 상한을 바로 건다/푼다(PiP 중이면 걸지 않는다). */
export function setMenuHidden(iframe, on) {
  if (!iframe) return;
  if (on) menuHidden.add(iframe); else menuHidden.delete(iframe);
  const cap = on && !inPip(iframe) ? BACKGROUND_HEIGHT : 0;
  try { iframe.contentWindow?.setBackgroundSaver?.(cap); } catch { /* 준비 전 */ }
}

function players() {
  const out = [];
  for (const f of document.querySelectorAll('iframe')) {
    if (menuHidden.has(f) || inPip(f)) continue;
    try {
      const w = f.contentWindow;
      if (w && typeof w.setBackgroundSaver === 'function') out.push(w);
    } catch { /* 다른 출처 */ }
  }
  return out;
}

export function installBackgroundSaver() {
  let timer = null;
  let applied = false;
  const apply = (height) => {
    for (const w of players()) {
      try { w.setBackgroundSaver(height); } catch { /* 페이지가 아직 준비 전 */ }
    }
  };
  document.addEventListener('visibilitychange', () => {
    clearTimeout(timer);
    if (document.hidden) {
      timer = setTimeout(() => { applied = true; apply(BACKGROUND_HEIGHT); }, HIDDEN_DELAY_MS);
    } else if (applied) {
      applied = false;
      apply(0);
    }
  });
}
