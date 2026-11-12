// 채팅 아바타 — 메시지 앞의 작은 원형 그림.
//
// 1) 사진: 치지직 채팅 메시지의 profile.profileImageUrl 은 지금 늘 빈 문자열로 온다(2026-11-12 실측). 그래서 사진은
//    시청자의 채널(userIdHash = 채널 ID)에서 Rust `chat_avatars` 로 천천히 찾는다 — 1.5초에 최대 3명(초당 2건),
//    가장 최근에 말한 사람부터, 창이 보일 때만. 찾은 결과(사진 없음 포함)는 여기와 Rust 양쪽에서 기억해 다시 묻지 않는다.
// 2) 랜덤 아이콘: 사진이 없거나 아직 모르거나 불러오기에 실패하면 사용자 ID 로 정해지는 동물·캐릭터 아이콘(14종 ×
//    색 12조합 × 장식 4종). 같은 사람은 늘 같은 아이콘이다. 외부 서비스 없이 여기서 만든 SVG 시트 한 장을 배경으로
//    깔고 줄마다 위치(background-position)만 준다(줄마다 노드 추가 없음, 이미지는 한 번만 래스터). 사진은 그 위에
//    겹쳐 그려(크기 고정) 바뀌어도 줄이 흔들리지 않는다.
// 설정 `chatAvatars`: 'photo'(기본, 사진 + 아이콘) · 'icon'(아이콘만 — 사진을 찾지 않는다) · 'off'(숨김).
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';

// ── 결정적 해시 ──────────────────────────────
const encoder = new TextEncoder();

/** FNV-1a 32비트(UTF-8 바이트) — 실행·기기가 바뀌어도 같은 값. 순수 함수. */
export function avatarSeed(key) {
  let h = 0x811c9dc5;
  for (const b of encoder.encode(String(key ?? ''))) {
    h ^= b;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  // 끝자리만 다른 ID 도 고르게 퍼지게 한 번 더 섞는다(murmur3 finalizer)
  h ^= h >>> 16; h = Math.imul(h, 0x85ebca6b) >>> 0;
  h ^= h >>> 13; h = Math.imul(h, 0xc2b2ae35) >>> 0;
  h ^= h >>> 16;
  return h >>> 0;
}

export const AVATAR_SPECIES = Object.freeze(['cat', 'dog', 'bear', 'rabbit', 'fox', 'frog', 'panda', 'chick', 'owl', 'penguin', 'mouse', 'ghost', 'robot', 'alien']);
// [배경, 몸, 포인트] — 배경은 옅게, 몸은 진하게(두 테마 모두에서 원 안이 또렷하게).
export const AVATAR_PALETTES = Object.freeze([
  ['#ffe0c2', '#f28c38', '#ffc4d6'], // 살구
  ['#cdefe3', '#2fb383', '#ffe59a'], // 민트
  ['#cfe6ff', '#4a90e2', '#ffd1dc'], // 하늘
  ['#e3d9ff', '#8b6cf0', '#ffe08a'], // 라벤더
  ['#ffd6e0', '#ef6f91', '#fff1b8'], // 장미
  ['#fff3b0', '#e0a800', '#ff9e7a'], // 레몬
  ['#c9f2f2', '#1f9ea3', '#ffc2a8'], // 청록
  ['#ffd3cc', '#e9573f', '#ffe7a3'], // 산호
  ['#e2f5c4', '#6aab2e', '#ffc9de'], // 연두
  ['#dce3ec', '#5b6b82', '#ffd27a'], // 돌
  ['#ecdccf', '#9a6b4f', '#f7c8a8'], // 코코아
  ['#eed8f5', '#a855c4', '#bff0e0'], // 포도
]);
export const AVATAR_EXTRAS = Object.freeze(['none', 'blush', 'shine', 'none']);

/** 사용자 키 → 아이콘 조합 { species, palette(번호), extra }. 순수 함수 — 같은 키는 늘 같은 조합. */
export function avatarFor(key) {
  const h = avatarSeed(key);
  return {
    species: AVATAR_SPECIES[h % AVATAR_SPECIES.length],
    palette: (h >>> 8) % AVATAR_PALETTES.length,
    extra: AVATAR_EXTRAS[(h >>> 20) % AVATAR_EXTRAS.length],
  };
}

// ── 아이콘 그림 ─────────────────────────────
// 색 자리표시: var(--av-fg)(몸) · var(--av-ac)(포인트) · var(--av-ink)(눈·코) — 시트를 만들 때 실제 색으로 바꾼다.
const EYES = (y = 19, dx = 4.2, r = 1.7) => `<circle cx="${18 - dx}" cy="${y}" r="${r}" fill="var(--av-ink)"/><circle cx="${18 + dx}" cy="${y}" r="${r}" fill="var(--av-ink)"/>`;
const SPECIES_SVG = {
  cat: `<path d="M7.5 17 8.6 5.5 16 11.5ZM28.5 17 27.4 5.5 20 11.5Z" fill="var(--av-fg)"/><path d="M10 13.5 10.6 8.6 14 11.6ZM26 13.5 25.4 8.6 22 11.6Z" fill="var(--av-ac)"/><ellipse cx="18" cy="20.5" rx="11.5" ry="10" fill="var(--av-fg)"/>${EYES(19.5)}<path d="M16.6 23h2.8L18 24.6Z" fill="var(--av-ac)"/><path d="M7 22.5H3M7.2 25 3.4 26.6M29 22.5h4M28.8 25l3.8 1.6" stroke="var(--av-ink)" stroke-width=".9" stroke-linecap="round" opacity=".45"/>`,
  dog: `<ellipse cx="18" cy="19.5" rx="10.5" ry="10.5" fill="var(--av-fg)"/><ellipse cx="7.8" cy="17" rx="3.6" ry="7" transform="rotate(18 7.8 17)" fill="var(--av-ac)"/><ellipse cx="28.2" cy="17" rx="3.6" ry="7" transform="rotate(-18 28.2 17)" fill="var(--av-ac)"/><ellipse cx="18" cy="24.5" rx="5.6" ry="4.3" fill="#fff" opacity=".88"/>${EYES(18)}<ellipse cx="18" cy="22.6" rx="2" ry="1.5" fill="var(--av-ink)"/><path d="M17 26.2q1 1.6 2 0" stroke="var(--av-ink)" stroke-width=".9" fill="none" stroke-linecap="round"/>`,
  bear: `<circle cx="9.5" cy="10.5" r="4.4" fill="var(--av-fg)"/><circle cx="26.5" cy="10.5" r="4.4" fill="var(--av-fg)"/><circle cx="9.5" cy="10.5" r="2.2" fill="var(--av-ac)"/><circle cx="26.5" cy="10.5" r="2.2" fill="var(--av-ac)"/><circle cx="18" cy="20" r="11" fill="var(--av-fg)"/><ellipse cx="18" cy="24.3" rx="5.2" ry="3.8" fill="var(--av-ac)"/>${EYES(18.4)}<ellipse cx="18" cy="23" rx="1.8" ry="1.3" fill="var(--av-ink)"/>`,
  rabbit: `<ellipse cx="13" cy="9" rx="3.4" ry="8" fill="var(--av-fg)"/><ellipse cx="23" cy="9" rx="3.4" ry="8" fill="var(--av-fg)"/><ellipse cx="13" cy="9.5" rx="1.6" ry="5.6" fill="var(--av-ac)"/><ellipse cx="23" cy="9.5" rx="1.6" ry="5.6" fill="var(--av-ac)"/><ellipse cx="18" cy="23" rx="10.5" ry="9.2" fill="var(--av-fg)"/>${EYES(22)}<path d="M16.8 25h2.4L18 26.4Z" fill="var(--av-ac)"/>`,
  fox: `<path d="M6.5 7.5 15 13 9 20ZM29.5 7.5 21 13 27 20Z" fill="var(--av-fg)"/><path d="M8.6 10.4 12.6 13 9.8 16.4ZM27.4 10.4 23.4 13l2.8 3.4Z" fill="var(--av-ink)" opacity=".7"/><path d="M7 15q11-9 22 0 0 9-11 16Q7 24 7 15Z" fill="var(--av-fg)"/><path d="M9.5 19q4 1 8.5 6 4.5-5 8.5-6-2 7-8.5 11-6.5-4-8.5-11Z" fill="#fff" opacity=".9"/>${EYES(18.2, 4.6, 1.6)}<ellipse cx="18" cy="27.2" rx="1.7" ry="1.3" fill="var(--av-ink)"/>`,
  frog: `<circle cx="11" cy="12.5" r="5.2" fill="var(--av-fg)"/><circle cx="25" cy="12.5" r="5.2" fill="var(--av-fg)"/><ellipse cx="18" cy="22" rx="13" ry="9.2" fill="var(--av-fg)"/><circle cx="11" cy="12.5" r="3.2" fill="#fff"/><circle cx="25" cy="12.5" r="3.2" fill="#fff"/><circle cx="11.4" cy="12.9" r="1.7" fill="var(--av-ink)"/><circle cx="25.4" cy="12.9" r="1.7" fill="var(--av-ink)"/><path d="M12 23.5q6 4.5 12 0" stroke="var(--av-ink)" stroke-width="1.3" fill="none" stroke-linecap="round"/><circle cx="9" cy="22" r="1.6" fill="var(--av-ac)"/><circle cx="27" cy="22" r="1.6" fill="var(--av-ac)"/>`,
  panda: `<circle cx="9.5" cy="10.5" r="4.4" fill="var(--av-ink)"/><circle cx="26.5" cy="10.5" r="4.4" fill="var(--av-ink)"/><circle cx="18" cy="20" r="11" fill="#fff"/><ellipse cx="13.4" cy="19" rx="2.9" ry="3.9" transform="rotate(30 13.4 19)" fill="var(--av-ink)"/><ellipse cx="22.6" cy="19" rx="2.9" ry="3.9" transform="rotate(-30 22.6 19)" fill="var(--av-ink)"/><circle cx="13.8" cy="18.6" r="1" fill="#fff"/><circle cx="22.2" cy="18.6" r="1" fill="#fff"/><ellipse cx="18" cy="24" rx="1.9" ry="1.3" fill="var(--av-ink)"/><path d="M8 24.5q1 3 3 4M28 24.5q-1 3-3 4" stroke="var(--av-fg)" stroke-width="2.2" fill="none" stroke-linecap="round"/>`,
  chick: `<path d="M16 8.5q1-4 3-2-1 1.5 0 3" stroke="var(--av-fg)" stroke-width="1.6" fill="none" stroke-linecap="round"/><circle cx="18" cy="20" r="11.5" fill="var(--av-fg)"/>${EYES(18, 4.2, 1.6)}<path d="M15.6 21.5h4.8L18 24.6Z" fill="var(--av-ac)"/><path d="M7.5 22q-2 2 0 4.5M28.5 22q2 2 0 4.5" stroke="var(--av-ink)" stroke-width=".9" fill="none" opacity=".35" stroke-linecap="round"/>`,
  owl: `<path d="M8 9 13 12.5 9 15ZM28 9 23 12.5 27 15Z" fill="var(--av-fg)"/><ellipse cx="18" cy="20.5" rx="11" ry="11" fill="var(--av-fg)"/><path d="M11 27q7 5 14 0-7-3-14 0Z" fill="var(--av-ac)" opacity=".8"/><circle cx="13.4" cy="18" r="4.4" fill="#fff"/><circle cx="22.6" cy="18" r="4.4" fill="#fff"/><circle cx="13.4" cy="18.3" r="2.2" fill="var(--av-ink)"/><circle cx="22.6" cy="18.3" r="2.2" fill="var(--av-ink)"/><path d="M16.6 21.6h2.8L18 24.2Z" fill="var(--av-ac)"/>`,
  penguin: `<ellipse cx="18" cy="20" rx="11.5" ry="12" fill="var(--av-fg)"/><path d="M18 14q-2.2-3.2-6-2-3.6 2.4-3 8 .6 8 9 10.5 8.4-2.5 9-10.5.6-5.6-3-8-3.8-1.2-6 2Z" fill="#fff"/>${EYES(18.4, 3.8, 1.6)}<path d="M15.8 21h4.4L18 23.4Z" fill="var(--av-ac)"/><ellipse cx="12" cy="23.3" rx="1.5" ry="1" fill="var(--av-ac)" opacity=".8"/><ellipse cx="24" cy="23.3" rx="1.5" ry="1" fill="var(--av-ac)" opacity=".8"/>`,
  mouse: `<circle cx="9" cy="12" r="6" fill="var(--av-fg)"/><circle cx="27" cy="12" r="6" fill="var(--av-fg)"/><circle cx="9" cy="12" r="3.6" fill="var(--av-ac)"/><circle cx="27" cy="12" r="3.6" fill="var(--av-ac)"/><ellipse cx="18" cy="21.5" rx="9.5" ry="9" fill="var(--av-fg)"/>${EYES(20, 3.8, 1.6)}<circle cx="18" cy="24.2" r="1.5" fill="var(--av-ac)"/><path d="M10.5 24.5H5.5M10.8 26.8 6 28M25.5 24.5h5M25.2 26.8 30 28" stroke="var(--av-ink)" stroke-width=".8" stroke-linecap="round" opacity=".4"/>`,
  ghost: `<path d="M8 30V17a10 10 0 0 1 20 0v13l-3.3-2.4L21.3 30 18 27.6 14.7 30l-3.4-2.4Z" fill="var(--av-fg)"/><ellipse cx="14.2" cy="17.5" rx="1.9" ry="2.6" fill="var(--av-ink)"/><ellipse cx="21.8" cy="17.5" rx="1.9" ry="2.6" fill="var(--av-ink)"/><ellipse cx="18" cy="23" rx="1.8" ry="1.4" fill="var(--av-ink)" opacity=".75"/>`,
  robot: `<path d="M18 6.5v4" stroke="var(--av-ink)" stroke-width="1.4" stroke-linecap="round"/><circle cx="18" cy="6" r="2" fill="var(--av-ac)"/><rect x="7.5" y="10.5" width="21" height="19" rx="5" fill="var(--av-fg)"/><rect x="5" y="17" width="2.6" height="6" rx="1.2" fill="var(--av-ink)" opacity=".55"/><rect x="28.4" y="17" width="2.6" height="6" rx="1.2" fill="var(--av-ink)" opacity=".55"/><rect x="10.5" y="14" width="15" height="7.5" rx="3" fill="var(--av-ink)"/><circle cx="14.5" cy="17.75" r="1.7" fill="var(--av-ac)"/><circle cx="21.5" cy="17.75" r="1.7" fill="var(--av-ac)"/><path d="M13.5 25.3h9" stroke="var(--av-ink)" stroke-width="1.4" stroke-linecap="round" stroke-dasharray="1.6 1.4"/>`,
  alien: `<path d="M11 9.5 8.5 4.5M25 9.5l2.5-5" stroke="var(--av-fg)" stroke-width="1.4" stroke-linecap="round"/><circle cx="8.3" cy="4.3" r="1.8" fill="var(--av-ac)"/><circle cx="27.7" cy="4.3" r="1.8" fill="var(--av-ac)"/><path d="M18 8c7.6 0 12 4.8 12 10.6C30 25.6 23 31.5 18 31.5S6 25.6 6 18.6C6 12.8 10.4 8 18 8Z" fill="var(--av-fg)"/><ellipse cx="12.8" cy="19" rx="3.3" ry="4.6" transform="rotate(-28 12.8 19)" fill="var(--av-ink)"/><ellipse cx="23.2" cy="19" rx="3.3" ry="4.6" transform="rotate(28 23.2 19)" fill="var(--av-ink)"/><circle cx="12" cy="17.4" r=".9" fill="#fff"/><circle cx="22.4" cy="17.4" r=".9" fill="#fff"/><path d="M16 26.4q2 1.2 4 0" stroke="var(--av-ink)" stroke-width="1" fill="none" stroke-linecap="round"/>`,
};
const EXTRA_SVG = {
  blush: '<ellipse cx="10.6" cy="23.2" rx="2.3" ry="1.4" fill="#ff7a9c" opacity=".55"/><ellipse cx="25.4" cy="23.2" rx="2.3" ry="1.4" fill="#ff7a9c" opacity=".55"/>',
  shine: '<path d="M30 3.5l.9 2.3 2.3.9-2.3.9-.9 2.3-.9-2.3-2.3-.9 2.3-.9Z" fill="#fff" opacity=".95"/>',
};

const INK = '#24292f';
const CELL = 40; // 칸 하나(36 그림 + 바깥 여백 — 귀·더듬이가 원에 잘리지 않게)

function paint(svg, fg, ac) {
  return svg.replaceAll('var(--av-fg)', fg).replaceAll('var(--av-ac)', ac).replaceAll('var(--av-ink)', INK);
}

/**
 * 아이콘 시트 — 열은 종류(14), 줄은 색(12). 한 장의 SVG 이미지라 브라우저가 한 번만 만들고 래스터한다.
 * 사람마다 다른 SVG 이미지를 쓰면 붐비는 채팅에서 이미지마다 메인 스레드가 문서를 만들고 래스터해
 * 프레임을 넘겼다(P04 4배 감속: 20ms 초과 3% → 38%). 줄마다 SVG 노드를 넣는 것도 같은 이유로 버렸다.
 */
export function avatarSheetSvg() {
  const cells = [];
  AVATAR_PALETTES.forEach(([bg, fg, ac], row) => {
    AVATAR_SPECIES.forEach((sp, col) => {
      cells.push(`<g transform="translate(${col * CELL} ${row * CELL})"><rect width="${CELL}" height="${CELL}" fill="${bg}"/><g transform="translate(2 1.5)">${paint(SPECIES_SVG[sp], fg, ac)}</g></g>`);
    });
  });
  const w = AVATAR_SPECIES.length * CELL;
  const h = AVATAR_PALETTES.length * CELL;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w * 2}" height="${h * 2}">${cells.join('')}</svg>`;
}

/** 장식(볼터치·반짝이) 한 장 — 아이콘 위에 겹친다. */
function extraSvg(name) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -1.5 ${CELL} ${CELL}" width="80" height="80">${EXTRA_SVG[name]}</svg>`;
}

function blobUrl(svg) {
  try { return URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' })); } catch { return `data:image/svg+xml,${encodeURIComponent(svg)}`; }
}

let sheetReady = false;
/** 시트·장식 그림을 문서 변수(--cv-av-sheet 등)로 한 번 건다. */
function ensureSheet() {
  if (sheetReady || typeof document === 'undefined') return;
  sheetReady = true;
  const root = document.documentElement.style;
  root.setProperty('--cv-av-sheet', `url("${blobUrl(avatarSheetSvg())}")`);
  for (const name of Object.keys(EXTRA_SVG)) root.setProperty(`--cv-av-${name}`, `url("${blobUrl(extraSvg(name))}")`);
}

/** 사용자 키 → 줄에 붙일 { cls, style }. 순수 함수 — 같은 키는 늘 같은 값. */
export function avatarStyle(key) {
  const a = avatarFor(key);
  const col = AVATAR_SPECIES.indexOf(a.species);
  const pct = (i, n) => `${+(i / (n - 1) * 100).toFixed(4)}%`;
  return {
    cls: EXTRA_SVG[a.extra] ? ` av-x-${a.extra}` : '',
    style: `background-position:${pct(col, AVATAR_SPECIES.length)} ${pct(a.palette, AVATAR_PALETTES.length)}`,
  };
}

// ── 사진 찾기 ──────────────────────────────
const BATCH = 3;
const INTERVAL_MS = 1500;
const QUEUE_MAX = 120;
const KNOWN_MAX = 5000;
/** uid → 사진 주소('' = 없음·실패). */
const known = new Map();
/** 물어볼 uid(넣은 순서 — 최근이 끝). */
const queue = new Set();
let pumpTimer = 0;
let backoff = 1;

function remember(uid, url) {
  if (known.size >= KNOWN_MAX) known.delete(known.keys().next().value);
  known.set(uid, url || '');
}

/** 사진을 물어볼 만한 사용자인가 — 치지직 userIdHash(영숫자 해시)만. */
export function isLookupId(uid) {
  return typeof uid === 'string' && /^[A-Za-z0-9_-]{6,64}$/.test(uid) && uid !== 'unknown';
}

function enqueue(uid) {
  if (!isLookupId(uid) || known.has(uid)) return;
  queue.delete(uid); // 다시 말하면 맨 뒤(가장 최근)로
  queue.add(uid);
  if (queue.size > QUEUE_MAX) queue.delete(queue.values().next().value);
  schedule();
}

function schedule() {
  if (pumpTimer || !queue.size) return;
  pumpTimer = setTimeout(pump, INTERVAL_MS * backoff);
}

async function pump() {
  pumpTimer = 0;
  if (!queue.size) return;
  if (typeof document !== 'undefined' && document.hidden) return; // 다시 보이면 visibilitychange 가 이어 간다
  const ids = [...queue].slice(-BATCH).reverse();
  for (const id of ids) queue.delete(id);
  try {
    const urls = await api.chatAvatars(ids);
    backoff = 1;
    ids.forEach((id, i) => {
      const url = safeUrl(Array.isArray(urls) ? urls[i] : '');
      // Rust 가 모른다(실패)고 null 을 준 경우도 여기선 "없음"으로 — 다음 실행까지 다시 묻지 않는다.
      remember(id, url);
      if (known.get(id)) attachPhotos(id, known.get(id));
    });
  } catch {
    backoff = Math.min(8, backoff * 2); // 오프라인·명령 없음 — 천천히
  }
  schedule();
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => { if (!document.hidden) schedule(); });
  // 사진을 못 불러오면 지우고(아래 아이콘이 보인다) 그 사람은 아이콘으로 기억한다. error 는 버블되지 않아 캡처로.
  document.addEventListener('error', (e) => {
    const img = e.target;
    if (!(img instanceof HTMLImageElement) || !img.classList.contains('av-photo') || !img.isConnected) return;
    const box = img.closest('[data-av]');
    if (box && box.dataset.av) remember(box.dataset.av, '');
    img.remove();
  }, true);
}

function photoHtml(url) {
  const src = imageSrc(url, 'avatar-sm');
  return src ? `<img class="av-photo" src="${esc(src)}" alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer" width="36" height="36">` : '';
}

/** 찾은 사진을 이미 그려진 그 사람의 아바타에 덮는다(아이콘 위에 겹쳐 크기 그대로). */
function attachPhotos(uid, url) {
  const html = photoHtml(url);
  if (!html) return;
  for (const box of document.querySelectorAll(`.chat-avatar[data-av="${CSS.escape(uid)}"]`)) {
    if (!box.querySelector('.av-photo')) box.insertAdjacentHTML('beforeend', html);
  }
}

/**
 * 메시지 한 줄의 아바타 HTML. mode — 'photo'(기본) · 'icon' · 'off'(빈 문자열).
 */
export function avatarHtml(m, mode = 'photo') {
  if (mode === 'off') return '';
  const uid = m && typeof m.userId === 'string' ? m.userId : '';
  const anonymous = !uid || uid === 'unknown' || m.anonymous;
  const key = anonymous ? `nick:${m?.nickname || ''}` : uid;
  ensureSheet();
  const icon = avatarStyle(key);
  let photo = '';
  if (mode !== 'icon' && !anonymous) {
    const given = safeUrl(m.profileImageUrl);
    if (given && !known.has(uid)) remember(uid, given);
    const url = known.get(uid);
    if (url) photo = photoHtml(url);
    else if (url === undefined) enqueue(uid);
  }
  const av = anonymous ? '' : ` data-av="${esc(uid)}"`;
  return `<span class="chat-avatar av${icon.cls}"${av} style="${icon.style}" aria-hidden="true">${photo}</span>`;
}

/** 검사용 — 기억·대기열 상태. */
export function __avatarState() {
  return { known: known.size, queue: [...queue] };
}
