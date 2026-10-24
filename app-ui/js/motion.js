// 모션 도우미 — 썸네일 페이드인 표시(is-loaded).
//
// CSS(motion.css)는 썸네일을 받기 전까지 투명하게 두고 is-loaded 가 붙으면 0.3초에 걸쳐 보인다.
// - 받기가 끝나면(load) — 실패(error)해도 붙인다. 영영 투명하게 남는 그림이 없게.
// - 캐시에 있어 이미 끝난 그림은 MutationObserver(그리기 전 마이크로태스크)에서 바로 붙인다 — 목록을
//   다시 그릴 때 한 프레임 깜박이지 않게.
const FADE_SCOPE = '.thumb, .queue-thumb, .hero-media, .ml-addrow-thumb, .bookmark-thumb, .category-art, .ml-pick-thumb';

function mark(img) {
  if (img.complete) img.classList.add('is-loaded');
}

function scan(node) {
  if (node.nodeType !== 1) return;
  if (node.tagName === 'IMG') { mark(node); return; }
  if (node.firstElementChild) for (const img of node.getElementsByTagName('img')) mark(img);
}

export function installImageFade(root = document) {
  const done = (e) => { if (e.target instanceof HTMLImageElement) e.target.classList.add('is-loaded'); };
  root.addEventListener('load', done, true);
  root.addEventListener('error', done, true);
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'attributes') {
        // src 를 바꿔 끼운 그림 — 새 그림을 받는 동안만 다시 숨긴다(캐시에 있으면 그대로).
        const img = r.target;
        if (img.closest(FADE_SCOPE)) img.classList.toggle('is-loaded', img.complete);
      } else {
        for (const n of r.addedNodes) scan(n);
      }
    }
  }).observe(root.documentElement || root, { childList: true, subtree: true, attributes: true, attributeFilter: ['src'] });
  for (const img of document.images) mark(img);
}

/** 모션을 줄였는가 — 설정 "절약" 또는 시스템 설정. */
export function motionReduced() {
  return document.documentElement.dataset.motion === 'reduced'
    || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
}

/**
 * 세그먼트(.segmented) — 고른 칸 아래 판(.seg-glider)이 미끄러져 따라간다. 모든 세그먼트에 자동으로 붙는다.
 * - 판은 transform(translate)·크기만 바꾼다. 처음 놓을 때·모션 줄이기면 애니메이션 없이 바로.
 * - 고른 칸이 바뀌면(class) 다시 놓고, 세그먼트 크기가 바뀌면(사이드바 접기·창 크기) ResizeObserver 로 맞춘다.
 * - 세그먼트를 통째로 다시 그리면(innerHTML) 새 세그먼트를 찾아 붙인다.
 */
export function installSegmentGlider() {
  const tracked = new WeakSet();
  const ro = new ResizeObserver((entries) => {
    for (const e of entries) {
      if (!e.target.isConnected) { ro.unobserve(e.target); continue; }
      place(e.target, false);
    }
  });
  function place(box, animate = true) {
    const active = box.querySelector(':scope > .seg.active');
    let g = box.querySelector(':scope > .seg-glider');
    if (!active || !active.offsetWidth) { if (g) g.style.opacity = '0'; return; }
    box.classList.add('has-glider'); // position:relative — 아래 offset 이 이 상자 기준이 되게 먼저 단다
    if (!g) {
      g = document.createElement('span');
      g.className = 'seg-glider';
      g.setAttribute('aria-hidden', 'true');
      box.prepend(g);
      animate = false;
    }
    const instant = !animate || g.style.opacity === '0' || motionReduced();
    if (instant) g.classList.add('instant');
    g.style.width = `${active.offsetWidth}px`;
    g.style.height = `${active.offsetHeight}px`;
    g.style.translate = `${active.offsetLeft}px ${active.offsetTop}px`;
    g.style.opacity = '1';
    if (instant) requestAnimationFrame(() => requestAnimationFrame(() => g.classList.remove('instant')));
  }
  function track(box) {
    if (!tracked.has(box)) { tracked.add(box); ro.observe(box); }
    place(box, false);
  }
  function scan(node) {
    if (node.nodeType !== 1) return;
    if (node.classList.contains('segmented')) track(node);
    for (const b of node.getElementsByClassName('segmented')) track(b);
  }
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'attributes') {
        const box = r.target.parentElement;
        if (box && box.classList.contains('segmented') && r.target.classList.contains('seg')) {
          if (!tracked.has(box)) track(box); else place(box);
        }
      } else {
        const box = r.target.classList?.contains('segmented') ? r.target : null;
        if (box) track(box);
        for (const n of r.addedNodes) scan(n);
      }
    }
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['class'] });
  scan(document.body);
}

/**
 * 작은 손맛 — 새로고침 버튼은 누르면 아이콘이 한 바퀴 돈다(모든 [data-action="refresh"]).
 * 애니메이션이 끝나면 클래스를 떼어 다음에 다시 돈다.
 */
export function installMicroMotion() {
  document.addEventListener('click', (e) => {
    const btn = e.target.closest?.('[data-action="refresh"]');
    const svg = btn && btn.querySelector('svg');
    if (!svg || motionReduced()) return;
    svg.classList.remove('spin-once');
    void svg.getBoundingClientRect(); // 같은 버튼을 빠르게 다시 눌러도 처음부터
    svg.classList.add('spin-once');
    svg.addEventListener('animationend', () => svg.classList.remove('spin-once'), { once: true });
  }, true);
}
