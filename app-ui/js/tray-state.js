// 메뉴 막대·알림 영역 아이콘 메뉴(Rust tray.rs)와 macOS 드롭다운 팝오버(tray_popover.rs)에 보일
// '지금 시청 중'·음소거·재생·볼륨·PiP 상태 — 바뀔 때만 Rust 에 알린다.
//
// - 지금 시청 중: 시스템 '지금 재생 중'(now-playing.js)에 올린 것과 같은 정보다. applyNowPlaying/clearNowPlaying 이
//   여기에 알리고, 가장 최근에 올린 문서가 이긴다(멀티라이브는 소리 나는 패인 하나만 올린다). 문서가 사라지면(iframe 제거)
//   다음 확인 때 뺀다.
// - 음소거·재생·볼륨·PiP: 메뉴 막대 팝오버 미니 컨트롤러와 같은 대상을 app.js 가 알려 준다.
// - 메인 창만 알린다(보조 창의 플레이어는 메뉴 대상이 아니다). 숨은 창에서도 느린 확인(2초)만 돈다.

const CHECK_MS = 2000;
const IS_MAIN = typeof location !== 'undefined'
  && !/menubar\.html$/i.test(location.pathname || '')
  && !new URLSearchParams(location.search).get('w');

/** 문서(창) → 지금 재생 중 정보. 넣은 순서가 곧 최근 순서다(다시 넣으면 끝으로 옮긴다). */
const playing = new Map();
let mutedSource = () => null;
let playbackSource = () => null;
let lastSent = '';
let timer = 0;

function alive(win) {
  try { return !!win && !win.closed; } catch { return false; }
}

function cleanStr(v) {
  return typeof v === 'string' ? v.trim() : '';
}

function buildNowPlaying(now) {
  if (!now) return null;
  const out = {
    kind: now.kind === 'vod' ? 'vod' : 'live',
    title: String(now.title || ''),
    channelName: String(now.channelName || ''),
  };
  const channelId = cleanStr(now.channelId);
  const category = cleanStr(now.category);
  const thumbnailUrl = cleanStr(now.thumbnailUrl);
  const imageUrl = cleanStr(now.imageUrl);
  if (channelId) out.channelId = channelId;
  if (category) out.category = category;
  if (thumbnailUrl) out.thumbnailUrl = thumbnailUrl;
  if (imageUrl) out.imageUrl = imageUrl;
  return out;
}

/** Rust 로 보낼 상태. 순수에 가깝다(살아 있는 문서만 본다). */
export function trayStateSnapshot() {
  let now = null;
  for (const [win, info] of playing) {
    if (!alive(win)) { playing.delete(win); continue; }
    now = info;
  }
  let muted = null;
  try {
    const m = mutedSource();
    muted = typeof m === 'boolean' ? m : null;
  } catch { muted = null; }

  const out = {
    nowPlaying: buildNowPlaying(now),
    // 시청 중인 것이 없으면 음소거도 두지 않는다(메뉴에 대상 없는 항목이 남지 않게).
    muted: now ? muted : null,
  };
  if (now) {
    try {
      const extra = playbackSource() || null;
      if (extra) {
        if (typeof extra.paused === 'boolean') out.paused = extra.paused;
        if (typeof extra.volume === 'number' && Number.isFinite(extra.volume)) {
          out.volume = Math.max(0, Math.min(100, Math.round(extra.volume)));
        }
        if (typeof extra.pip === 'boolean') out.pip = extra.pip;
      }
    } catch { /* 무시 */ }
  }
  return out;
}

function send() {
  if (!IS_MAIN) return;
  const invoke = typeof window !== 'undefined' && window.__TAURI__?.core?.invoke;
  if (!invoke) return;
  const state = trayStateSnapshot();
  const key = JSON.stringify(state);
  if (key === lastSent) return;
  // 실패해도(명령이 없는 옛 셸·검사용 모의 IPC) 같은 상태를 다시 보내지 않는다 — 놀고 있는 동안 IPC 를 만들지 않게.
  lastSent = key;
  invoke('tray_set_state', { state }).catch(() => {});
}

/** 곧 다시 알린다(연달아 불러도 한 번). */
export function syncTray() {
  if (!IS_MAIN) return;
  queueMicrotask(send);
}

function sameInfo(a, b) {
  return Boolean(a && b)
    && a.kind === b.kind
    && a.title === b.title
    && a.channelName === b.channelName
    && cleanStr(a.channelId) === cleanStr(b.channelId)
    && cleanStr(a.category) === cleanStr(b.category)
    && cleanStr(a.thumbnailUrl) === cleanStr(b.thumbnailUrl)
    && cleanStr(a.imageUrl) === cleanStr(b.imageUrl);
}

/** now-playing.js — 이 문서의 지금 재생 중 정보를 넣거나(info) 뺀다(null). */
export function noteNowPlaying(win, info) {
  if (!IS_MAIN || !win) return;
  // 멀티라이브는 1초마다 같은 정보를 다시 넣는다 — 이미 맨 끝(가장 최근)에 같은 내용이면 할 일이 없다.
  const prev = playing.get(win);
  if (info && prev && [...playing.keys()].at(-1) === win && sameInfo(prev, info)) return;
  if (!info && !prev) return;
  playing.delete(win);
  if (info) playing.set(win, info);
  syncTray();
}

/** app.js — 음소거·재생 상태 함수를 걸고 주기 확인을 시작한다. */
export function installTrayState({ muted, playback } = {}) {
  if (!IS_MAIN) return;
  if (typeof muted === 'function') mutedSource = muted;
  if (typeof playback === 'function') playbackSource = playback;
  clearInterval(timer);
  timer = setInterval(send, CHECK_MS);
  syncTray();
}
