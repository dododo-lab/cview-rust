// 화면 보정 — 선명한 화면(치지직과 같은 SVG 언샤프 마스크)·밝기·대비·채도. 시청 화면과 멀티라이브 패인이 같이 쓴다.
//
// 채널마다 기억한다(이 기기 localStorage `cview.picture.v1`): 같은 채널은 시청 화면·멀티라이브·넘기기·다시 실행에도
// 같은 모습이다. 처음 보는 채널의 선명한 화면은 마지막 "모든 패인에 적용"이 정한 기본값을 따른다(처음엔 꺼짐).
// 밝기·대비·채도는 멀티라이브 패인에서 채널별로 조절한다(시청 화면은 기존 고급 패널의 전역 보정을 그대로 쓴다).

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

/** 채널의 화면 보정(없으면 기본값). */
export function pictureFor(channelId) {
  const all = load();
  const d = all[DEFAULT_KEY] || {};
  const c = (channelId && all[channelId]) || {};
  return {
    sharpen: typeof c.sharpen === 'boolean' ? c.sharpen : Boolean(d.sharpen),
    brightness: num(c.brightness, 1),
    contrast: num(c.contrast, 1),
    saturation: num(c.saturation, 1),
  };
}

/** 채널의 화면 보정 일부를 바꾼다. */
export function setPicture(channelId, patch) {
  if (!channelId) return;
  const all = load();
  all[channelId] = { ...(all[channelId] || {}), ...patch };
  save(all);
}

/** 처음 보는 채널의 선명한 화면 기본값("모든 패인에 적용"). */
export function setSharpenDefault(on) {
  const all = load();
  all[DEFAULT_KEY] = { ...(all[DEFAULT_KEY] || {}), sharpen: Boolean(on) };
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
 * 플레이어(iframe 다리)에 건다. `adjust` 가 거짓이면 선명한 화면만 건다(시청 화면 — 색 보정은 고급 패널 몫).
 * @param {{ call: (name: string, ...args: unknown[]) => unknown }} bridge
 */
export function applyPicture(bridge, p, { adjust = true } = {}) {
  if (!bridge) return;
  bridge.call('setSharpen', Boolean(p.sharpen), 1);
  if (adjust) {
    bridge.call('applyVideoAdjust', {
      enabled: !isNeutral(p),
      brightness: p.brightness,
      contrast: p.contrast,
      saturation: p.saturation,
      hue: 0,
      gamma: 1,
    });
  }
}
