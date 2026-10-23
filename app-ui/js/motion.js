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
