// 화면 밖 탐색 페이지의 이미지 놓기 — 메모리 최적화(2026-11-24, MEM-22).
//
// 모든 페이지는 .content 에 붙은 채 [hidden](display:none)으로만 바뀐다. 홈 → 인기(라이브 최대 300카드) → 팔로잉 → 채널을
// 거쳐 라이브를 보면 수백 장의 <img> 가 숨은 채 남는데, macOS WKWebView 는 문서에 붙은 숨은 <img> 의 디코드된 비트맵
// (2배율 히어로 8 MB, 라이브 썸네일 장당 2~4 MB)을 메모리 압박이 올 때까지 놓지 않는다.
// 숨은 지 HIBERNATE_MS 가 지난 탐색 페이지는 <img src> 를 data-hib-src 로 옮겨 비트맵을 놓게 하고, 다시 보이기 직전에
// 되돌린다(디스크·웹뷰 캐시에서 바로 다시 그린다). DOM·스크롤 위치·목록 상태는 그대로 둔다.
// OS 메모리 압박(app://memory-pressure, memory_pressure.rs)이 오면 기다리지 않고 숨은 탐색 페이지를 바로 놓는다.

export const HIBERNATE_PAGES = new Set(['home', 'following', 'popular', 'search', 'categories', 'channel', 'library']);
export const HIBERNATE_MS = 15000;

const timers = new Map();

/** root 안 <img src> 를 놓는다(이미 놓은 것·data: 는 건너뜀). 놓은 수. */
export function hibernateImages(root) {
  let n = 0;
  for (const img of root.querySelectorAll('img[src]')) {
    const src = img.getAttribute('src');
    if (!src || src.startsWith('data:')) continue;
    img.dataset.hibSrc = src;
    img.removeAttribute('src');
    n += 1;
  }
  return n;
}

/** 놓았던 이미지를 되돌린다 — 숨어 있는 동안 화면이 새 주소를 넣은 이미지는 그대로 둔다. */
export function wakeImages(root) {
  for (const img of root.querySelectorAll('img[data-hib-src]')) {
    if (!img.hasAttribute('src')) img.setAttribute('src', img.dataset.hibSrc);
    delete img.dataset.hibSrc;
  }
}

function sleepNow(page) {
  hibernateImages(page.root);
  page.root.dataset.hibernated = '1';
}

/**
 * 페이지 표시가 바뀐 뒤(app.js navigate) 부른다 — 보이는 탐색 페이지는 이미지를 되돌리고, 숨은 것은 잠시 뒤 놓는다.
 * @param {Record<string, { root: HTMLElement }>} pages
 */
export function syncHibernation(pages, delay = HIBERNATE_MS) {
  for (const [id, page] of Object.entries(pages)) {
    if (!HIBERNATE_PAGES.has(id) || !page || !page.root) continue;
    if (page.root.hidden) {
      if (timers.has(id)) continue;
      timers.set(id, setTimeout(() => {
        timers.delete(id);
        if (page.root.hidden) sleepNow(page);
      }, delay));
    } else {
      clearTimeout(timers.get(id));
      timers.delete(id);
      if (page.root.dataset.hibernated) {
        wakeImages(page.root);
        delete page.root.dataset.hibernated;
      }
    }
  }
}

/** 메모리 압박 — 숨은 탐색 페이지를 기다리지 않고 놓는다(이미 놓은 뒤 새로 붙은 이미지도). */
export function hibernateHiddenNow(pages) {
  for (const [id, page] of Object.entries(pages)) {
    if (!HIBERNATE_PAGES.has(id) || !page || !page.root || !page.root.hidden) continue;
    clearTimeout(timers.get(id));
    timers.delete(id);
    sleepNow(page);
  }
}
