// 작은 DOM 도우미.
import { motionReduced } from './motion.js';

/**
 * 시트·팝업 닫기 — leaving 을 달아 나가는 애니메이션(motion.css)이 끝나면 지운다.
 * 모션을 줄였거나 애니메이션이 없으면 바로 지운다. 두 번 불러도 한 번만 지운다.
 */
export function dismiss(el, ms = 200) {
  if (!el || !el.isConnected || el.classList.contains('leaving')) return;
  if (motionReduced()) { el.remove(); return; }
  el.classList.add('leaving');
  const done = () => el.remove();
  el.addEventListener('animationend', (e) => { if (e.target === el) done(); }, { once: true });
  setTimeout(done, ms + 60);
}

/** HTML 이스케이프. */
export function esc(value) {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** http(s) URL 만 통과시킨다(이미지 src 등). */
export function safeUrl(value) {
  const s = String(value ?? '');
  return /^https?:\/\//i.test(s) ? s : '';
}

// ── 화면 이미지 ──────────────────────────────
// 썸네일·아바타는 Rust 의 디스크·메모리 캐시(cvimg: 프로토콜)를 거치고, CDN 에는 칸에 맞는 크기만 요청한다.
// - live: 라이브 썸네일 image_720(1280×720, ~150KB) → image_405(720×405, ~60KB). 카드 칸은 약 330×185.
// - hero: 홈 큰 슬라이드 — 720 그대로.
// - avatar / avatar-lg: 네이버 이미지 원본(최대 1344² PNG) → ?type=f120_120_na(~24KB) / f240_240_na.
// - static: 카테고리 포스터·다시보기·이모티콘 등 — 크기는 그대로, 캐시만.
const IMAGE_HOSTS = /(^|\.)(pstatic\.net|akamaized\.net|naver\.net|naver\.com)$/i;
const CVIMG_BASE = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  ? 'http://cvimg.localhost/'
  : 'cvimg://localhost/';
const HAS_TAURI = typeof window !== 'undefined' && Boolean(window.__TAURI__);

function sizedForBox(url, kind) {
  if (kind === 'live') {
    return url.replace(/\/image_(\d+)\.jpg(?=$|\?)/, (m, n) => (Number(n) > 405 ? '/image_405.jpg' : m));
  }
  if (kind === 'avatar' || kind === 'avatar-lg') {
    try {
      const u = new URL(url);
      if (/phinf\.pstatic\.net$/i.test(u.host) && !u.searchParams.has('type')) {
        u.searchParams.set('type', kind === 'avatar' ? 'f120_120_na' : 'f240_240_na');
        return u.toString();
      }
    } catch { /* 그대로 */ }
  }
  return url;
}

/** `<img src>` 에 넣을 주소 — 칸에 맞는 크기로 바꾸고, 앱 안이면 캐시(cvimg:)를 거친다. */
export function imageSrc(value, kind = 'static') {
  const url = safeUrl(value);
  if (!url) return '';
  const sized = sizedForBox(url, kind);
  if (!HAS_TAURI || !sized.startsWith('https://')) return sized;
  let host = '';
  try { host = new URL(sized).host; } catch { return sized; }
  if (!IMAGE_HOSTS.test(host)) return sized;
  return `${CVIMG_BASE}${encodeURIComponent(`${kind}/${sized}`)}`;
}

export function $(selector, root = document) {
  return root.querySelector(selector);
}

export function h(html) {
  const t = document.createElement('template');
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
}

/**
 * 토스트 센터 — 원본 ToastCenter: 여러 개가 아래에서 쌓이고(최대 4), 동작 버튼을 붙일 수 있다 (SYS-13).
 * @param {string} message
 * @param {number} [ms]
 * @param {{ label: string, run: () => void }} [action]
 */
export function toast(message, ms = 2600, action = null) {
  let stack = document.getElementById('toast-stack');
  if (!stack) {
    stack = document.createElement('div');
    stack.id = 'toast-stack';
    stack.className = 'toast-stack';
    stack.setAttribute('role', 'status');
    stack.setAttribute('aria-live', 'polite');
    document.body.append(stack);
  }
  // 같은 문구가 이미 떠 있으면 시간만 늘린다.
  const same = [...stack.children].find((t) => t.dataset.msg === message);
  if (same) { same.remove(); }
  const el = document.createElement('div');
  el.className = 'toast';
  el.dataset.msg = message;
  el.innerHTML = `<span class="toast-text"></span>${action ? '<button class="toast-action"></button>' : ''}`;
  el.querySelector('.toast-text').textContent = message;
  if (action) {
    const b = el.querySelector('.toast-action');
    b.textContent = action.label;
    b.addEventListener('click', () => { dismiss(el); action.run(); });
  }
  stack.append(el);
  while (stack.children.length > 4) stack.firstElementChild.remove();
  setTimeout(() => dismiss(el), action ? Math.max(ms, 5000) : ms);
}

/**
 * 방송 종료 그림 — 꺼진 화면에 달·Zz. 라이브 시청 화면과 멀티라이브 패인의 상태 오버레이에 넣고,
 * 오버레이에 `ended` 클래스가 있을 때만 보인다. 색은 currentColor — CSS(.stream-ended-art)가 정한다.
 */
export const streamEndedArt = `<div class="stream-ended-art" aria-hidden="true"><svg viewBox="0 0 120 96" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
  <rect x="10" y="10" width="100" height="64" rx="9" fill="currentColor" fill-opacity=".08"/>
  <path d="M46 86h28M60 74v12"/>
  <path d="M66 30a13 13 0 1 0 12 18 10 10 0 0 1-12-18Z" fill="currentColor" fill-opacity=".85" stroke="none"/>
  <path d="M36 32h8l-8 9h8M80 24h5l-5 6h5" stroke-width="2.4" opacity=".7"/>
  <circle cx="99" cy="20" r="3" fill="currentColor" fill-opacity=".35" stroke="none"/>
</svg></div>`;

export const icons = {
  home: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 10.5 12 4l8.5 6.5"/><path d="M5.5 9v10a1 1 0 0 0 1 1H10v-5.5h4V20h3.5a1 1 0 0 0 1-1V9"/></svg>',
  following: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20s-7-4.3-7-9.6A4 4 0 0 1 12 8a4 4 0 0 1 7 2.4C19 15.7 12 20 12 20Z"/></svg>',
  popular: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3c.5 3.2 4.5 5.3 4.5 10a4.5 4.5 0 0 1-9 0c0-2.2 1-3.6 2.2-4.8.3 1.6 1 2.6 2 3.1C11.4 9 11.2 5.7 12 3Z"/></svg>',
  search: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg>',
  settings: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"/></svg>',
  close: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>',
  volume: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h4l5-4v14l-5-4H4V9Zm12.5 3a4.5 4.5 0 0 0-2-3.7v7.4a4.5 4.5 0 0 0 2-3.7Z"/></svg>',
  muted: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h4l5-4v14l-5-4H4V9Z"/><path d="m16 9 5 6m0-6-5 6" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  fullscreen: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></svg>',
  sidebar: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="15" rx="3"/><path d="M9.5 4.5v15"/></svg>',
  logout: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M15 4.5h3a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5h-3"/><path d="M10 16.5 5.5 12 10 7.5M5.5 12H15"/></svg>',
  login: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4.5H6A1.5 1.5 0 0 0 4.5 6v12A1.5 1.5 0 0 0 6 19.5h3"/><path d="M14 7.5 18.5 12 14 16.5M18.5 12H9"/></svg>',
  user: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8.5" r="3.5"/><path d="M5 19.5a7 7 0 0 1 14 0"/></svg>',
  play: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.5-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5Z"/></svg>',
  chevronLeft: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 5-7 7 7 7"/></svg>',
  chevronRight: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 5 7 7-7 7"/></svg>',
  send: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 20.5 21 12 3 3.5V10l12 2-12 2v6.5Z"/></svg>',
};

/** 채널 프로필 톤 (DSN-04) — 채널 ID 로 정하는 고정 색상(같은 채널은 늘 같은 색). 순수 함수. */
export function channelTone(id) {
  let h = 2166136261;
  for (const ch of String(id || '')) { h ^= ch.codePointAt(0); h = Math.imul(h, 16777619) >>> 0; }
  return `hsl(${h % 360} 78% 58%)`;
}
