// 플레이어 도구 — 시청 화면 고급 패널(player-panel.js)과 라이브 단독 보기 패인 ⚙(ml-pane-controls.js)가 같이 쓴다.
//
// - 화면 비율 강제·자동 밝기·색 보정·픽셀 그대로 확대·이퀄라이저는 전역 플레이어 설정(`player`)에 저장하고,
//   새 재생(playStarted)·새 플레이어 문서(ready)·단독 보기에 들어올 때마다 `applyPlayerTools` 로 다시 건다.
// - 업스케일은 선명한 화면과 함께 '선명도'(sharpness.js)로 합쳤다 — 여기서는 setPictureEnhancement 에 auto 만 보낸다
//   (upscale 키를 보내면 플레이어가 예전 방식으로 돌아간다).
// - 배속은 저장하지 않는다 — 라이브는 1배속이 기본이고, 새 재생·LIVE(따라잡기)마다 1배속으로 돌아간다.
//   플레이어(player.html)는 사용자 배속이 1 이 아니면 지연 따라잡기(1.03배)를 걸지 않는다.
import { api } from './api.js';

export const RATES = [0.5, 0.75, 1, 1.25, 1.5, 2];
export const RATE_NOTE = '라이브는 1배속이 기본이에요. 바꾸면 지연이 늘거나 줄 수 있고, LIVE 를 누르면 1배속으로 돌아갑니다.';

// macOS WKWebView 는 MSE 영상의 오디오를 Web Audio 그래프로 넘기지 않는다(소스 노드 출력이 0 — 실측).
// 그래프는 한 번 만들면 되돌릴 수 없어 켜는 순간 그 재생의 소리가 사라지므로 여기서는 막는다.
export const EQ_SUPPORTED = !/Mac OS X|Macintosh/.test(navigator.userAgent);
export const EQ_FREQS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const FLAT = { name: '평탄', gains: new Array(10).fill(0), preAmp: 0 };

/** 10밴드 주파수 표시 — 31 → "31", 1000 → "1k". */
export function freqLabel(hz) {
  return hz >= 1000 ? `${hz / 1000}k` : String(hz);
}

/** 저장된 플레이어 설정(앱 밖·실패면 빈 객체). */
export async function loadPlayerSettings() {
  try { return (await api.settingsGet()).player || {}; } catch { return {}; }
}

/** 플레이어 설정 일부를 저장하고 저장된 전체를 돌려준다(실패는 던진다). */
export async function savePlayerSettings(patch) {
  return (await api.settingsUpdate('player', patch)).player || {};
}

let eqPresets = null;
/** 이퀄라이저 주파수·프리셋(한 번만 묻는다). 실패하면 기본 주파수·빈 프리셋. */
export function loadEqPresets() {
  if (!eqPresets) {
    eqPresets = Promise.resolve()
      .then(() => api.equalizerPresets())
      .then(([freqs, presets]) => ({ freqs: freqs && freqs.length === 10 ? freqs : EQ_FREQS, presets: presets || [] }))
      .catch(() => { eqPresets = null; return { freqs: EQ_FREQS, presets: [] }; });
  }
  return eqPresets;
}

/** 이 설정이면 이퀄라이저를 켜는가(이 플랫폼에서 쓸 수 있을 때만). */
export function eqWanted(p) {
  return EQ_SUPPORTED && Boolean(p && p.equalizerPreset);
}

/** 저장된 밴드(10개가 아니면 기본 프리셋 → 평탄). */
export function eqBands(p, presets = []) {
  if (p && Array.isArray(p.equalizerBands) && p.equalizerBands.length === 10) return p.equalizerBands;
  return (presets[0] || FLAT).gains;
}

/** 이퀄라이저를 켤 때 저장할 값 — 이전 밴드가 있으면 그대로, 없으면 첫 프리셋. */
export function eqOnPatch(p, presets = []) {
  const pr = presets[0] || FLAT;
  return { equalizerPreset: (p && p.equalizerPreset) || pr.name, equalizerBands: eqBands(p, presets) };
}

/**
 * 저장된 도구 설정을 플레이어에 건다. 밝기·대비 같은 색 보정은 화면마다 다르게 다뤄 여기서 건드리지 않는다
 * (시청 화면은 고급 패널의 전역 보정, 멀티라이브 패인은 채널별 picture.js).
 * @param {{ call: (name: string, ...args: unknown[]) => unknown }} bridge
 * @returns {boolean} 이퀄라이저를 켰는가
 */
export function applyPlayerTools(bridge, p) {
  if (!bridge || !p) return false;
  bridge.call('setPictureEnhancement', { auto: Boolean(p.autoPictureEnhancement) });
  bridge.call('setForcedAspectRatio', p.aspectRatio === 'fill');
  bridge.call('setSharpPixelScaling', Boolean(p.sharpPixelScaling));
  if (!eqWanted(p)) return false;
  bridge.call('setEqualizerEnabled', true, eqBands(p), p.equalizerPreAmp || 0);
  return true;
}

/** 건 도구를 기본으로 되돌린다(단독 보기에서 그리드로 돌아간 패인). `eq` 면 이퀄라이저도 평탄하게 끈다. 선명도는 그대로(그리드 규칙으로 다시 건다). */
export function clearPlayerTools(bridge, { eq = false } = {}) {
  if (!bridge) return;
  bridge.call('setPictureEnhancement', { auto: false });
  bridge.call('setForcedAspectRatio', false);
  bridge.call('setSharpPixelScaling', false);
  if (eq) bridge.call('setEqualizerEnabled', false);
}
