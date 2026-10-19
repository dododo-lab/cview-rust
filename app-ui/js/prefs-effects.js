// 설정값 → 화면 동작. 원본 설정의 효과를 웹 화면에 반영한다.
// DSN-02 모션 등급, DSN-03 콤팩트, PLY-16 후원 알림 위치·크기, PLY-19 백그라운드 재생, SYS-15 자동 새로고침 주기.
import { api } from './api.js';

let settings = null;
let refreshTimer = 0;
let pausedByBackground = false;

/** 알림 위치·크기 CSS 변수 — 순수 계산(테스트 가능). */
export function alertVars(player = {}) {
  const clamp = (v, lo, hi, d) => (Number.isFinite(Number(v)) ? Math.min(hi, Math.max(lo, Number(v))) : d);
  return {
    '--alert-x': String(clamp(player.streamAlertAnchorX, 0.05, 0.95, 0.5)),
    '--alert-y': String(clamp(player.streamAlertAnchorY, 0, 0.95, 0.08)),
    '--alert-scale': String(clamp(player.streamAlertScale, 0.5, 2, 1)),
  };
}

function apply(s, ctx) {
  settings = s;
  const root = document.documentElement;
  const a = s.appearance || {};
  root.classList.toggle('compact', Boolean(a.compactMode));
  root.dataset.motion = a.motionQuality || 'balanced';
  // 투명도 (SYS-22) — 시스템이면 macOS "투명도 줄이기"를 따른다.
  root.dataset.transparency = a.transparencyMode || 'system';
  for (const [k, v] of Object.entries(alertVars(s.player || {}))) root.style.setProperty(k, v);
  scheduleRefresh(ctx);
}

function scheduleRefresh(ctx) {
  clearInterval(refreshTimer);
  const g = (settings && settings.general) || {};
  if (!g.autoRefreshEnabled) return;
  const sec = Math.min(600, Math.max(30, Number(g.autoRefreshInterval) || 60));
  refreshTimer = setInterval(() => {
    if (document.visibilityState !== 'visible' || !navigator.onLine) return;
    ctx.refreshCurrent();
  }, sec * 1000);
}

/**
 * @param {{ viewer: import('./viewer.js').Viewer, refreshCurrent: () => void }} ctx
 */
export async function installPrefsEffects(ctx) {
  try { apply(await api.settingsGet(), ctx); } catch { /* 앱 밖 */ }
  window.addEventListener('cview:settings-changed', (e) => {
    if (e.detail && e.detail.settings) apply(e.detail.settings, ctx);
  });
  // 백그라운드 재생 끄기 — 창이 가려지거나 최소화되면 단독 시청을 멈췄다가 돌아오면 이어서 (PLY-19).
  document.addEventListener('visibilitychange', () => {
    const keep = !settings || !settings.player || settings.player.continuePlaybackInBackground !== false;
    const v = ctx.viewer;
    if (keep || !v.isPlaying) return;
    if (document.visibilityState === 'hidden') {
      const video = v.player.iframe.contentDocument && v.player.iframe.contentDocument.getElementById('video');
      if (video && !video.paused) { v.player.call('pause'); pausedByBackground = true; }
    } else if (pausedByBackground) {
      pausedByBackground = false;
      v.player.call('play');
      v.player.call('seekToLiveEdge');
    }
  });
}
