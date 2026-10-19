// 작업 공간 — 원본 Persistence/WorkspaceSnapshot·AppState+StoreRecovery(NAV-16),
// MainShell/MenuHubSegment(NAV-15 ⌘⌥1~6 허브 전환), Components/SidebarQuickActions·SidebarWidgets·SidebarPlaybackInsights(NAV-13).
import { api } from './api.js';
import { esc, toast } from './dom.js';

const KEY = 'cview.workspace';
const RESTORABLE = new Set(['home', 'following', 'popular', 'categories', 'multilive', 'multichat', 'library', 'stats', 'search', 'settings', 'channel', 'media']);

/** ⌘⌥ 숫자 → 허브 화면. */
export const HUBS = [
  ['1', 'home', '탐색'],
  ['2', 'live', '시청'],
  ['3', 'multilive', '멀티'],
  ['4', 'search', '검색'],
  ['5', 'popular', '인기'],
  ['6', 'categories', '카테고리'],
];

/** 저장할 스냅숏인지 — 일회성 화면·큰 파라미터는 빼고 화면·작은 파라미터만. 순수 함수. */
export function snapshotFor(route, params, playingChannelId) {
  if (!RESTORABLE.has(route) && route !== 'live') return null;
  const safe = {};
  for (const [k, v] of Object.entries(params || {})) {
    if (['string', 'number', 'boolean'].includes(typeof v) && String(v).length < 200) safe[k] = v;
  }
  return { route: route === 'live' ? 'home' : route, params: safe, channelId: playingChannelId || null, at: Date.now() };
}

export function saveWorkspace(route, params, playingChannelId) {
  const snap = snapshotFor(route, params, playingChannelId);
  if (!snap) return;
  try { localStorage.setItem(KEY, JSON.stringify(snap)); } catch { /* 무시 */ }
}

export function loadWorkspace() {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) || 'null');
    return s && RESTORABLE.has(s.route) ? s : null;
  } catch { return null; }
}

/** 사이드바 빠른 동작·오늘 시청 위젯. */
export function installSidebarExtras({ onRandomLive, onQueueMulti, onRefresh }) {
  const nav = document.getElementById('nav');
  const box = document.createElement('div');
  box.className = 'side-extras';
  box.innerHTML = `
    <div class="side-widget" title="이 기기에서 오늘 본 시간 · 연속 시청 일수 (통계에서 자세히)"><span class="side-widget-label nav-label">오늘 시청</span><strong class="side-widget-value">—</strong></div>
    <div class="side-quick">
      <button class="side-quick-btn" data-q="random" title="팔로잉 중 아무 방송이나 보기">🎲<span class="nav-label">아무거나</span></button>
      <button class="side-quick-btn" data-q="multi" title="팔로잉 상위 4개를 멀티라이브로">▦<span class="nav-label">멀티로</span></button>
      <button class="side-quick-btn" data-q="refresh" title="새로 고침 (⌘R)">↻<span class="nav-label">새로 고침</span></button>
    </div>`;
  nav.after(box);
  box.addEventListener('click', (e) => {
    const b = e.target.closest('[data-q]');
    if (!b) return;
    if (b.dataset.q === 'random') onRandomLive();
    else if (b.dataset.q === 'multi') onQueueMulti();
    else onRefresh();
  });
  const value = box.querySelector('.side-widget-value');
  const widget = box.querySelector('.side-widget');
  const update = async () => {
    try {
      const d = await api.insightsOverview();
      const today = d.daily.length ? d.daily[d.daily.length - 1].seconds : 0;
      const m = Math.round(today / 60);
      value.textContent = m >= 60 ? `${Math.floor(m / 60)}시간 ${m % 60}분` : `${m}분`;
      widget.title = `오늘 ${value.textContent} · 연속 ${d.streak}일 · 최근 7일 ${Math.round(d.last7 / 60)}분`;
      widget.dataset.streak = d.streak ? `🔥${d.streak}` : '';
    } catch { widget.hidden = true; }
  };
  widget.addEventListener('click', () => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'stats' } })));
  update();
  setInterval(update, 5 * 60 * 1000);
  return { refresh: update };
}

export function hubFlash(label) {
  toast(`허브 · ${esc(label)}`, 900);
}
