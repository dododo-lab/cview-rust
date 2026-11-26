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

/**
 * 영상 단축키(맨 글자·화살표·Space)를 대상에게 넘길 키인가 — 입력란·선택 상자, 버튼·링크의 Space/Enter,
 * 시청 화면 위에 뜬 시트·패널·메뉴 안. 넘기지 않으면 선택 상자의 화살표가 볼륨을, 버튼의 Space 가 재생을 바꾼다.
 * 채널 추가 서랍(.ml-addbar)·머리줄 ⋯ 메뉴·패인 ⚙/⋯ 메뉴·화질 메뉴·후원 목록도 — 그 안의 ←/→·M·L·B 가 뒤의 플레이어로 갔다.
 */
export function keyBelongsToTarget(e) {
  if (e.defaultPrevented) return true;
  const t = e.target;
  if (!t || !t.closest) return false;
  if (t.isContentEditable || t.closest('input, textarea, select')) return true;
  if ((e.key === ' ' || e.key === 'Enter') && t.closest('button, a[href], [role="button"]')) return true;
  // 단독 보기·포커스 아래 채널 정보 칸 — 화살표는 카드 사이 이동이다(뒤 방송 되감기·볼륨까지 함께 움직였다, 감사 UX-H20).
  if (e.key && e.key.startsWith('Arrow') && t.closest('.ml-watch-info, .ml-focus-info')) return true;
  // .vt-menu(시청 화면 ⋯)·.ad-overlay(프리롤 광고) 안의 Space·화살표·M 도 뒤의 방송으로 새지 않게.
  return Boolean(t.closest('.sheet-scrim, .palette-scrim, .player-panel, .emoji-picker, .dp-panel, .ctx-menu, .ml-addbar, .ml-more-menu, .ml-pane-menu, .quality-menu, .vt-menu, .ad-overlay'));
}

/**
 * hidden 으로 여닫는 떠 있는 것(팔레트·메뉴·플레이어 패널)의 나가는 모션.
 * 상태는 바로 숨기고(열림 판단·토글이 hidden 을 그대로 본다), 같은 자리에 겉모습만 복제해 흐려지며 사라지게 한다.
 * 복제는 원본 바로 뒤에 둔다 — 그사이 다시 열면 CSS(`:not([hidden]) + .exit-ghost`)가 복제를 감춘다.
 */
const EXIT_GHOST_MAX_NODES = 400;

export function hideWithExit(el, ms = 160) {
  if (!el || el.hidden) return;
  // 안이 아주 큰 것(이모티콘 수백 칸·긴 검색 결과)은 복제·다시 그리기가 닫는 순간 프레임을 밀어 바로 닫는다.
  const big = el.getElementsByTagName('*').length > EXIT_GHOST_MAX_NODES;
  el.hidden = true;
  if (big || motionReduced() || !el.parentNode || !el.getClientRects) return;
  const next = el.nextElementSibling;
  if (next && next.classList.contains('exit-ghost')) next.remove();
  const ghost = el.cloneNode(true);
  ghost.hidden = false;
  ghost.removeAttribute('id');
  for (const n of ghost.querySelectorAll('[id]')) n.removeAttribute('id');
  ghost.setAttribute('aria-hidden', 'true');
  ghost.inert = true;
  ghost.classList.add('exit-ghost');
  el.after(ghost);
  const done = () => ghost.remove();
  ghost.addEventListener('animationend', (e) => { if (e.target === ghost) done(); }, { once: true });
  setTimeout(done, ms + 80);
}

/**
 * 버튼 옆에 absolute 로 뜨는 메뉴를 경계(잘리는 패널 또는 창) 안에 들인다 — 연 직후(보이는 상태에서) 부른다.
 * 1) 메뉴가 경계보다 넓으면 경계 폭으로 줄인다(항목 글자는 줄바꿈된다 — 잘리지 않는다).
 * 2) CSS 정렬(보통 버튼 오른쪽 끝)이 넘치면 반대쪽 정렬(버튼 왼쪽 끝)로 뒤집고, 그래도 넘치면 경계 안으로 민다.
 * 3) 아래가 모자라면 높이를 남은 공간으로 줄여 안에서 스크롤한다. 아래 공간이 너무 좁고 위가 더 넓으면 위로 연다.
 * 위치는 left·right 만 바꾼다 — 여닫는 모션(motion.css mo-pop-down)이 translate·scale 을 쓴다.
 * 매번 인라인 값을 지우고 CSS 자리에서 다시 잰다(패널 폭·화면 크기가 바뀌었을 수 있다). 레이아웃은 메뉴를 열 때만 읽는다.
 * @param {HTMLElement} menu
 * @param {Element|null} [bound] 메뉴를 자르는 상자(넘침을 숨기는 패널). 없으면 창. 창과 겹친 부분만 쓴다.
 * @param {number} [margin] 경계에서 띄울 거리(px)
 */
export function fitPopover(menu, bound = null, margin = 8) {
  if (!menu || menu.hidden) return;
  const s = menu.style;
  s.left = ''; s.right = ''; s.top = ''; s.bottom = ''; s.minWidth = ''; s.maxWidth = ''; s.maxHeight = ''; s.overflowY = ''; s.transformOrigin = '';
  delete menu.dataset.fit;
  const op = menu.offsetParent;
  if (!op) return;
  // 숨김을 풀면 바로 여는 모션(mo-pop-down: translate 0 -4px · scale .97)이 시작된다 — 그 첫 프레임을 재면 폭은 97%,
  // 아래 끝은 (4px + 높이 3%) 위로 재져 높이 상한이 걸리지 않고 다 열린 뒤 메뉴가 경계 밖으로 나갔다.
  // 재는 동안만 모션을 끄고(돌고 있을 때만 — 창 크기 변경으로 다시 잴 때 다 열린 메뉴가 다시 튀지 않게) 되돌린다.
  const playing = typeof menu.getAnimations === 'function' && menu.getAnimations().some((a) => a.playState === 'running');
  const prevAnimation = s.animation;
  if (playing) s.animation = 'none';
  try { measureAndFit(menu, op, bound, margin); } finally { if (playing) s.animation = prevAnimation; }
}

function measureAndFit(menu, op, bound, margin) {
  const s = menu.style;
  const b = bound ? bound.getBoundingClientRect() : { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
  const lo = Math.max(b.left, 0) + margin;
  const hi = Math.min(b.right, window.innerWidth) - margin;
  const top = Math.max(b.top, 0) + margin;
  const bottom = Math.min(b.bottom, window.innerHeight) - margin;
  const avail = Math.max(0, hi - lo);
  let r = menu.getBoundingClientRect();
  if (r.width > avail + 0.5) {
    s.minWidth = '0';
    s.maxWidth = `${avail}px`;
    r = menu.getBoundingClientRect();
  }
  const fits = (x) => x >= lo - 0.5 && x + r.width <= hi + 0.5;
  if (!fits(r.left)) {
    const o = op.getBoundingClientRect();
    // 반대쪽 정렬 — 왼쪽으로 넘치면 버튼 묶음(offsetParent) 왼쪽 끝에서, 오른쪽으로 넘치면 오른쪽 끝에 맞춰 연다
    const flip = r.left < lo ? o.left : o.right - r.width;
    const x = fits(flip) ? flip : Math.min(Math.max(r.left, lo), hi - r.width);
    s.left = `${Math.round(x - o.left - op.clientLeft)}px`;
    s.right = 'auto';
    s.transformOrigin = x + r.width / 2 > (o.left + o.right) / 2 ? 'top right' : 'top left';
    menu.dataset.fit = fits(flip) ? 'flip' : 'shift';
  }
  if (r.bottom > bottom + 0.5) {
    const o = op.getBoundingClientRect();
    const gap = Math.max(0, r.top - o.bottom); // 버튼 묶음과 메뉴 사이(보통 6px)
    const below = Math.floor(bottom - r.top);
    const above = Math.floor(o.top - gap - top);
    // 아래가 너무 좁고(메뉴도 다 못 들이고 120px 도 안 됨) 위가 더 넓으면 위로 연다
    const up = below < Math.min(120, r.height) && above > below;
    if (up) {
      s.top = 'auto';
      s.bottom = `${Math.round(op.clientHeight + gap)}px`;
      s.transformOrigin = `bottom ${s.transformOrigin.split(' ')[1] || 'right'}`;
      menu.dataset.fitV = 'up';
    } else delete menu.dataset.fitV;
    const room = up ? above : below;
    if (r.height > room + 0.5) {
      s.maxHeight = `${Math.max(0, room)}px`;
      s.overflowY = 'auto';
    }
  } else delete menu.dataset.fitV;
}

const FOCUS_KEYS = ['data-route', 'data-channel-id', 'data-dl-id', 'data-dl'];

/** 초점 든 요소를 다시 찾을 선택자 — 자신과 조상의 식별 data-* 로(없으면 null). */
function focusPath(el, root) {
  const parts = [];
  for (let n = el; n && n !== root; n = n.parentElement) {
    const k = FOCUS_KEYS.filter((x) => n.hasAttribute(x)).map((x) => `[${x}="${CSS.escape(n.getAttribute(x))}"]`).join('');
    if (k) parts.unshift(k);
  }
  return parts.length ? parts.join(' ') : null;
}

/**
 * 안을 innerHTML 로 다시 그리는 동안 키보드 초점을 지킨다 — 초점 든 버튼이 문서에서 떨어지면
 * 같은 식별(data-route·data-channel-id·data-dl-id·data-dl)의 새 버튼에 다시 준다.
 */
export function keepFocus(root, paint) {
  const a = document.activeElement;
  const sel = a && a !== root && root.contains(a) ? focusPath(a, root) : null;
  paint();
  if (!sel || a.isConnected) return;
  root.querySelector(sel)?.focus({ preventScroll: true });
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
//   고해상도(Retina·화면 크기 확대)에서 칸이 물리 픽셀로 405 보다 크면 480·540·720 으로 올린다(N 은 세로 픽셀).
// - hero: 홈 큰 슬라이드(화면당 한 장) — 720, 칸이 물리 픽셀로 더 크면(Retina·4K) 1080·1280.
// 물리 픽셀 = CSS 픽셀 × 기기 배율 × 화면 크기(setImageZoom). 단계는 thumbnail_tier.rs 화이트리스트 안에서만 고른다.
// - avatar / avatar-lg: 네이버 이미지 원본(최대 1344² PNG) → ?type=f120_120_na(~24KB) / f240_240_na.
// - avatar-sm: 채팅 아바타(34px) → ?type=f60_60_na(~7KB, 2026-11-12 실측 200 — f48·f64·f80 은 404).
// - vod: 다시보기 썸네일(가로) 원본 1280~1920px(~75~120KB) → ?type=w640(640×360, ~50KB). 카드 칸은 약 330×185.
// - clip: 클립 썸네일(세로 칸 약 170×300). nng-phinf 는 늘 1080×1920(250~360KB) → ?type=w640(640×1138, ~100~130KB).
//   video-phinf 클립은 가로·세로가 섞여 있어(가로 원본을 줄이면 세로 칸에서 흐려진다) 그대로 둔다.
//   이미지 서버마다 받는 단계가 다르다(2026-10-31 실측: video-phinf w320·w400·w480·w640·w750, nng-phinf w640·w750).
//   모르는 단계면 404 → Rust 이미지 서비스가 원본으로 한 번 더 받는다.
// - static: 이모티콘·배지·사진 등 — 크기는 그대로, 캐시만.
const IMAGE_HOSTS = /(^|\.)(pstatic\.net|akamaized\.net|naver\.net|naver\.com)$/i;
const CVIMG_BASE = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  ? 'http://cvimg.localhost/'
  : 'cvimg://localhost/';
const HAS_TAURI = typeof window !== 'undefined' && Boolean(window.__TAURI__);

// 화면 크기(설정 "외관 → 화면 크기") — 웹뷰 확대가 devicePixelRatio 에 들어가는지는 엔진마다 다르다.
// - WebView2(Windows, Chromium): 확대가 devicePixelRatio 에 이미 들어 있다 — 그대로 쓴다(곱하면 두 번).
//   확대는 다시 불러오기 뒤에도 남으므로 "처음 본 배율"을 확대 전 값으로 기억하면 안 된다.
// - WKWebView(macOS): devicePixelRatio 는 화면 배율(1·2) 그대로라 확대 배율을 곱한다. 다만 값이 2 를 넘거나
//   정수가 아니면(맥 화면 배율은 1·2 뿐) 확대가 이미 들어간 것으로 보고 곱하지 않는다.
let uiZoom = 1;
const winDpr = () => (typeof window !== 'undefined' && window.devicePixelRatio) || 1;
const ON_WINDOWS = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
/** devicePixelRatio 가 화면 크기 확대를 이미 품고 있는가. 순수 함수(검사용으로 내보낸다). */
export function dprIncludesZoom(dpr, windows = ON_WINDOWS) {
  if (windows) return true;
  return dpr > 2.01 || Math.abs(dpr - Math.round(dpr)) > 0.01;
}
/** 화면 크기 배율을 알린다(prefs-effects.js). */
export function setImageZoom(zoom) {
  uiZoom = Number(zoom) > 0 ? Number(zoom) : 1;
}
/** CSS 픽셀 → 물리 픽셀 배율. */
export function pixelScale() {
  const d = winDpr();
  return dprIncludesZoom(d) ? d : d * uiZoom;
}

/** 본문 칸 폭(CSS 픽셀) 추정 — 사이드바(약 220)와 넓은 창의 인사이트 칸(약 280)을 뺀다. 그리기 전에 쓰므로 재지 않는다. */
function contentWidth() {
  const w = (typeof window !== 'undefined' && window.innerWidth) || 1280;
  return Math.max(260, w - 220 - (w >= 1241 ? 280 : 0) - 48);
}
/** 카드 격자(minmax(260px, 1fr), 간격 16) 한 칸의 폭. */
export function cardWidth(content = contentWidth()) {
  const cols = Math.max(1, Math.floor((content + 16) / (260 + 16)));
  return (content - 16 * (cols - 1)) / cols;
}
const LIVE_TIERS = [405, 480, 540, 720];
const HERO_TIERS = [720, 1080, 1280]; // 1280 = 2276×1280 — 4K 를 2배율로 쓰면 히어로 칸이 물리 픽셀로 5천 픽셀을 넘는다
/** 세로 물리 픽셀 need 를 덮는 가장 작은 단계(10% 모자란 것까지는 받아 준다). 없으면 가장 큰 단계. */
export function pickTier(need, tiers) {
  return tiers.find((t) => t >= need * 0.9) ?? tiers[tiers.length - 1];
}
function retier(url, tier) {
  return url.replace(/\/image_(\d+)\.jpg(?=$|\?)/, (m, n) => (Number(n) === tier ? m : `/image_${tier}.jpg`));
}

function sizedForBox(url, kind) {
  // 작은 썸네일(통계 최근 시청 64px 등) — 라이브 썸네일 가장 작은 단계. 예전엔 원본('static', 1080p)을 받아 디코드했다.
  if (kind === 'thumb') return retier(url, LIVE_TIERS[0]);
  if (kind === 'live') {
    return retier(url, pickTier((cardWidth() * 9) / 16 * pixelScale(), LIVE_TIERS));
  }
  if (kind === 'hero') {
    return retier(url, pickTier((contentWidth() * 0.85 * 9) / 16 * pixelScale(), HERO_TIERS));
  }
  if (kind === 'vod' || kind === 'clip') {
    try {
      const u = new URL(url);
      const nng = /^nng-phinf\.pstatic\.net$/i.test(u.host);
      const video = /^video-phinf\.pstatic\.net$/i.test(u.host);
      if ((nng || (video && kind === 'vod')) && !u.search) {
        // 다시보기 칸이 물리 픽셀로 640 을 넘으면(Retina) 두 서버 모두 받는 w750 — 그보다 큰 단계는 서버마다 달라 쓰지 않는다.
        u.searchParams.set('type', kind === 'vod' && cardWidth() * pixelScale() > 680 ? 'w750' : 'w640');
        return u.toString();
      }
    } catch { /* 그대로 */ }
    return url;
  }
  if (kind === 'avatar' || kind === 'avatar-lg' || kind === 'avatar-sm') {
    try {
      const u = new URL(url);
      if (/phinf\.pstatic\.net$/i.test(u.host) && !u.searchParams.has('type')) {
        // 채팅 아바타(34px)는 2배율 이상이면 68px 이 필요해 f60 이 흐리다 — f120.
        const sm = pixelScale() >= 1.75 ? 'f120_120_na' : 'f60_60_na';
        u.searchParams.set('type', { avatar: 'f120_120_na', 'avatar-sm': sm }[kind] || 'f240_240_na');
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
  // 'thumb' 은 크기만 고르는 화면 쪽 종류 — 캐시(Rust ImageKind)는 정적 이미지로 둔다.
  const cacheKind = kind === 'thumb' ? 'static' : kind;
  return `${CVIMG_BASE}${encodeURIComponent(`${cacheKind}/${sized}`)}`;
}

/**
 * 이모티콘 주소 — 네이버 이미지 서버(https)만. 이모티콘 표는 채팅 메시지에 실려 오므로, 다른 호스트를 받으면
 * 보낸 사람이 정한 서버로 시청자 모두의 IP·시청 시각이 새는 추적 픽셀이 된다. 나머지는 ''.
 */
export function emojiUrl(value) {
  const url = safeUrl(value);
  if (!url.startsWith('https://')) return '';
  try { return IMAGE_HOSTS.test(new URL(url).hostname) ? url : ''; } catch { return ''; }
}

/** `{:name:}` 이모티콘 치환 (치지직 채팅 본문 규칙). 나머지는 이스케이프. 채팅·후원 알림이 함께 쓴다. */
export function renderContent(content, emojis, showEmoji = true) {
  const map = emojis || {};
  return String(content ?? '')
    .split(/(\{:[^{}:]+:\})/g)
    .map((part) => {
      const m = /^\{:([^{}:]+):\}$/.exec(part);
      const url = m && Object.hasOwn(map, m[1]) && emojiUrl(map[m[1]]);
      if (!url) return esc(part);
      return showEmoji ? `<img decoding="async" class="emoji" src="${esc(imageSrc(url, 'static'))}" alt="${esc(m[1])}" referrerpolicy="no-referrer">` : `<span class="emoji-text">:${esc(m[1])}:</span>`;
    })
    .join('');
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
  // 같은 문구가 이미 떠 있으면 새것으로 바꾼다. 단 '되돌리기' 같은 동작이 달린 알림은 지우지 않는다 — 연달아 두 개를 지우면
  // 첫 알림의 되돌리기가 사라진 채 시간이 지나 그대로 지워졌다(deferDelete, 감사 UX-C10).
  const same = !action && [...stack.children].find((t) => t.dataset.msg === message && !t.querySelector('.toast-action'));
  if (same) { same.remove(); }
  const el = document.createElement('div');
  el.className = 'toast';
  el.dataset.msg = message;
  const life = action ? Math.max(ms, 5000) : ms;
  // 되돌리기 같은 동작이 있는 알림은 남은 시간을 아래 줄이 줄어드는 것으로 보인다
  el.innerHTML = `<span class="toast-text"></span>${action ? `<button class="toast-action"></button><i class="toast-timer" style="animation-duration:${life}ms"></i>` : ''}`;
  el.querySelector('.toast-text').textContent = message;
  if (action) {
    const b = el.querySelector('.toast-action');
    b.textContent = action.label;
    b.addEventListener('click', () => { dismiss(el); action.run(); });
  }
  stack.append(el);
  // 넘치면 오래된 것부터 — 동작 없는 알림을 먼저 내보낸다.
  while (stack.children.length > 4) {
    const plain = [...stack.children].find((t) => !t.querySelector('.toast-action'));
    (plain || stack.firstElementChild).remove();
  }
  setTimeout(() => dismiss(el), life);
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

/** 두 시각의 로컬 달력 날짜 차이(일) — '오늘'·'어제'를 24시간 단위가 아니라 날짜로 가른다(서머타임 날도 반올림으로 맞다). */
export function calendarDaysBetween(t, now) {
  const a = new Date(t);
  const b = new Date(now);
  return Math.round((new Date(b.getFullYear(), b.getMonth(), b.getDate()) - new Date(a.getFullYear(), a.getMonth(), a.getDate())) / 86400000);
}
