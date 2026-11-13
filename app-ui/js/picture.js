// 화면 색 보정 — 밝기·대비·채도. 멀티라이브 패인이 채널마다 기억한다(이 기기 localStorage `cview.picture.v1`).
// 같은 채널은 멀티라이브·넘기기·다시 실행에도 같은 모습이다(시청 화면은 기존 고급 패널의 전역 보정을 그대로 쓴다).
//
// [선명도 통합 2026-11-14] 예전 '선명한 화면'(채널별 `sharpen`·처음 보는 채널 기본값 `__default.sharpen`)은
// 업스케일과 함께 전역 '선명도'(sharpness.js)로 옮겼다. 여기에는 그 이전·구버전 호환용 읽기·쓰기만 남는다:
//   - legacySharpenState(): 이전 계산용 예전 값(채널 켬·끔 수, 기본값, 새 판 스냅숏)
//   - writeSharpnessMirror(): 새 판에서 선명도를 바꾸면 구버전이 읽는 `__default.sharpen` 과 스냅숏
//     `__default.sharpness` 를 써 두고, 채널 값 `sharpen` 은 `sharpenBeforeEnhance` 로 이름만 바꿔 보존한다
//     (구버전으로 되돌리면 모든 채널이 기본값 = 새 판에서 고른 선명도를 따른다). 구버전 setPicture·setSharpenDefault 는
//     객체를 펼쳐 다시 쓰므로 이 키들이 살아남는다.

const KEY = 'cview.picture.v1';
const DEFAULT_KEY = '__default';

export const ADJUST_RANGES = [
  ['brightness', '밝기', 0.5, 1.5, 0.05],
  ['contrast', '대비', 0.5, 1.5, 0.05],
  ['saturation', '채도', 0, 2, 0.05],
];

function load() {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || '{}');
    return v && typeof v === 'object' ? v : {};
  } catch {
    return {};
  }
}

function save(all) {
  try { localStorage.setItem(KEY, JSON.stringify(all)); } catch { /* 저장 못 해도 이번 실행은 그대로 */ }
}

const num = (v, fallback) => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);

/** 채널의 색 보정(없으면 기본값). */
export function pictureFor(channelId) {
  const all = load();
  const c = (channelId && all[channelId]) || {};
  return {
    brightness: num(c.brightness, 1),
    contrast: num(c.contrast, 1),
    saturation: num(c.saturation, 1),
  };
}

/** 채널의 색 보정 일부를 바꾼다. */
export function setPicture(channelId, patch) {
  if (!channelId) return;
  const all = load();
  all[channelId] = { ...(all[channelId] || {}), ...patch };
  save(all);
}

/** 밝기·대비·채도를 기본값으로. */
export function resetAdjust(channelId) {
  setPicture(channelId, { brightness: 1, contrast: 1, saturation: 1 });
}

export function isNeutral(p) {
  return p.brightness === 1 && p.contrast === 1 && p.saturation === 1;
}

/**
 * 플레이어(iframe 다리)에 채널 색 보정을 건다. 선명도는 sharpness.js(setSharpness)가 따로 건다.
 * @param {{ call: (name: string, ...args: unknown[]) => unknown }} bridge
 */
export function applyPicture(bridge, p) {
  if (!bridge) return;
  bridge.call('applyVideoAdjust', {
    enabled: !isNeutral(p),
    brightness: p.brightness,
    contrast: p.contrast,
    saturation: p.saturation,
    hue: 0,
    gamma: 1,
  });
}

/** 예전 '선명한 화면' 값(선명도 이전 계산용) — 채널 값이 있는 것만 센다. */
export function legacySharpenState() {
  const all = load();
  let on = 0;
  let off = 0;
  for (const [k, v] of Object.entries(all)) {
    if (k === DEFAULT_KEY || !v || typeof v !== 'object' || typeof v.sharpen !== 'boolean') continue;
    if (v.sharpen) on += 1; else off += 1;
  }
  const d = all[DEFAULT_KEY] || {};
  return {
    on,
    off,
    def: typeof d.sharpen === 'boolean' ? d.sharpen : undefined,
    snapshot: d.sharpness && typeof d.sharpness === 'object' ? d.sharpness : null,
  };
}

/** 새 판에서 고른 선명도를 구버전이 읽는 값으로 써 둔다(위 머리 주석). */
export function writeSharpnessMirror({ level, enlargedOnly, d, u }) {
  const all = load();
  const def = { ...(all[DEFAULT_KEY] || {}) };
  def.sharpen = Boolean(d);
  def.sharpness = { level, enlargedOnly: Boolean(enlargedOnly), d: Boolean(d), u: Boolean(u) };
  all[DEFAULT_KEY] = def;
  for (const [k, v] of Object.entries(all)) {
    if (k === DEFAULT_KEY || !v || typeof v !== 'object' || typeof v.sharpen !== 'boolean') continue;
    v.sharpenBeforeEnhance = v.sharpen;
    delete v.sharpen;
  }
  save(all);
}
