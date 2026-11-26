// 설정값 → 화면 동작. 원본 설정의 효과를 웹 화면에 반영한다.
// DSN-02 모션 등급, DSN-03 콤팩트, PLY-16 후원 알림 위치·크기, PLY-19 백그라운드 재생, SYS-15 자동 새로고침 주기.
import { api, listen, Events } from './api.js';
import { setImageZoom } from './dom.js';
import { syncTheme } from './theme.js';

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
  // 화면 크기 — 웹뷰 확대는 Rust 가 건다(ui_zoom_sync·설정 저장). 여기선 썸네일 단계를 고를 배율만 알린다.
  setImageZoom(Number(a.uiScale) || 1);
  // CSS 도 배율을 안다 — macOS 신호등은 확대와 상관없이 물리 위치에 고정이라 줄이면 접힌 사이드바 밖으로 나왔다(app.css, 감사 UX-H02).
  const scale = String(Number(a.uiScale) || 1);
  if (root.style.getPropertyValue('--ui-scale') !== scale) root.style.setProperty('--ui-scale', scale); // 같으면 쓰지 않는다(스타일 다시 계산)
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
  // 창이 뜰 때 저장된 화면 크기 — Rust 가 창을 만들자마자 걸어 둔다. 여기서는 웹뷰가 문서를 바꾸며 확대를 잃은 경우의 보조.
  if (window.__TAURI__) api.uiZoomSync().catch(() => {});
  // 다른 창·메뉴(⌘= ⌘- ⌘0)·가져오기·초기화로 외관이 바뀌면 모든 창에 온다 — 썸네일 배율·모션 등급을 맞춘다.
  listen(Events.appearanceChanged, (appearance) => {
    if (!appearance || typeof appearance !== 'object') return;
    // 테마도 — 메뉴 막대 팝오버·다른 창에서 바꾸거나 가져오기·초기화하면 이 창 화면은 예전 테마로 남았다.
    syncTheme(appearance.theme);
    apply({ ...(settings || {}), appearance }, ctx);
  });
  // 재생·일반 설정 — 다른 창(메인 창 설정·가져오기·초기화·메뉴 막대)에서 바꾸면 이 창도 따른다. 예전엔 연 때 읽은 값을 계속 써서
  // 플레이어 창이 '백그라운드 재생' 끔·후원 알림 위치·자동 새로고침 간격을 따르지 않았다.
  for (const [ev, key] of [[Events.playerChanged, 'player'], [Events.generalChanged, 'general']]) {
    listen(ev, (v) => {
      if (!v || typeof v !== 'object' || !settings) return;
      apply({ ...settings, [key]: v }, ctx);
    });
  }
  window.addEventListener('cview:settings-changed', (e) => {
    if (e.detail && e.detail.settings) apply(e.detail.settings, ctx);
  });
  // 백그라운드 재생 끄기 — 창이 가려지거나 최소화되면 단독 시청을 멈췄다가 돌아오면 이어서 (PLY-19).
  // 지금 라이브 시청의 기본 경로는 멀티라이브 단독 보기(무대 패인)다 — 예전엔 옛 시청 화면(ctx.viewer)만 멈춰,
  // 설정을 꺼도 단독 보기는 가려진 창에서 계속 받고 디코드했다. 무대 패인도 같은 규칙으로 멈췄다가 이어 본다.
  let pausedWatchSlot = null;
  document.addEventListener('visibilitychange', () => {
    const keep = !settings || !settings.player || settings.player.continuePlaybackInBackground !== false;
    const ml = ctx.multilive;
    if (document.visibilityState === 'hidden') {
      const slot = ml && ml.watchSlot;
      const pane = !keep && slot != null ? ml.panes.get(slot) : null;
      const video = pane && pane.sessionId ? pane.controls?.video : null;
      if (video && !video.paused) { pane.bridge.call('pause'); pausedWatchSlot = slot; }
    } else if (pausedWatchSlot != null) {
      const pane = ml && ml.panes.get(pausedWatchSlot);
      pausedWatchSlot = null;
      if (pane && pane.sessionId) { pane.bridge.call('play'); pane.bridge.call('seekToLiveEdge'); }
    }
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
