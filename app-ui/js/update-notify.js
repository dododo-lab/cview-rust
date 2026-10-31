// 새 버전 알림 — 실행 직후와 몇 시간마다 업데이트 서버를 조용히 확인하고, 새 버전이 있으면 앱 안에서 알린다.
// · 사이드바 아래(계정 위)에 "새 버전" 줄이 남는다 — 누르면 업데이트 확인 시트(설치 단추).
// · 버전마다 한 번 알림(토스트, 앱이 뒤에 있으면 시스템 알림). 창을 띄우거나 포커스를 가져가지 않는다.
// · 설정 › 일반 › "업데이트 자동 확인"(general.autoCheckUpdates)을 끄면 확인도 알림도 하지 않는다.
// 확인 결과는 update-check.js 와 같이 써서 설정 › 앱 줄에도 바로 보인다.
import { api, isTauri } from './api.js';
import { toast, esc } from './dom.js';
import { checkForUpdate, lastUpdateResult } from './update-check.js';

const FIRST_DELAY_MS = 15_000; // 실행 직후 화면·방송 로딩과 겹치지 않게 조금 늦게
const INTERVAL_MS = 6 * 60 * 60 * 1000; // 6시간마다
const RETRY_MS = 30 * 60 * 1000; // 실패하면 30분 뒤 다시
const NOTIFIED_KEY = 'cview.updateNotified'; // 알림을 띄운 버전 — 같은 버전은 다시 띄우지 않는다
const HIDDEN_KEY = 'cview.updatePillHidden'; // 사이드바 줄을 닫은 버전

let timer = null;
let lastCheckAt = 0;
let checking = false;
let onOpen = () => {};

function readKey(key) {
  try { return localStorage.getItem(key); } catch { return null; }
}
function writeKey(key, value) {
  try { localStorage.setItem(key, value); } catch { /* 저장 못 해도 이번 실행 동안은 동작 */ }
}

async function autoCheckEnabled() {
  try {
    const s = await api.settingsGet();
    return s?.general?.autoCheckUpdates !== false;
  } catch { return true; }
}

async function notificationsEnabled() {
  try {
    const s = await api.settingsGet();
    return s?.general?.notificationsEnabled !== false;
  } catch { return false; }
}

const PILL_ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11"/><path d="m7 10 5 5 5-5"/><path d="M5 20h14"/></svg>';

/** 사이드바 "새 버전" 줄 — 새 버전이 있고 사용자가 그 버전 줄을 닫지 않았으면 보인다. */
export function renderUpdatePill(u = lastUpdateResult()) {
  const pill = document.getElementById('update-pill');
  if (!pill) return;
  const show = Boolean(u && u.newer && u.latest && readKey(HIDDEN_KEY) !== u.latest);
  pill.hidden = !show;
  if (!show) return;
  pill.innerHTML = `<button class="update-pill-main" type="button" data-tip="새 버전 ${esc(u.latest)} — 눌러서 업데이트" aria-label="새 버전 ${esc(u.latest)} 업데이트"><span class="update-pill-icon">${PILL_ICON}<i class="update-pill-dot"></i></span>
    <span class="update-pill-text nav-label"><b>새 버전 ${esc(u.latest)}</b><small>눌러서 업데이트</small></span></button>
    <button class="update-pill-close" type="button" title="이 버전 알림 숨기기" aria-label="이 버전 알림 숨기기">✕</button>`;
}

function announce(u) {
  if (!u || !u.newer || !u.latest) return;
  if (readKey(NOTIFIED_KEY) === u.latest) return;
  writeKey(NOTIFIED_KEY, u.latest);
  toast(`새 버전 ${u.latest} 이 나왔습니다`, 10000, { label: '업데이트', run: () => onOpen() });
  // 앱이 뒤에 있으면(다른 앱을 보는 중) 시스템 알림도 — 알림만 뜨고 창은 앞으로 오지 않는다.
  if (document.hidden || !document.hasFocus()) {
    notificationsEnabled().then((on) => {
      if (on) api.notifySend('CView 업데이트', `새 버전 ${u.latest} 을 설치할 수 있습니다 — 설정 › 앱에서 업데이트하세요`).catch(() => {});
    });
  }
}

/** 조용히 한 번 확인한다. 실패해도 알리지 않는다(다음 주기에 다시). */
export async function checkQuietly({ force = false } = {}) {
  if (checking) return;
  if (!force && !(await autoCheckEnabled())) { schedule(INTERVAL_MS); return; }
  checking = true;
  let ok = false;
  try {
    const u = await checkForUpdate();
    ok = true;
    lastCheckAt = Date.now();
    renderUpdatePill(u);
    announce(u);
  } catch { /* 네트워크 없음 등 — 조용히 넘어간다 */ } finally {
    checking = false;
    schedule(ok ? INTERVAL_MS : RETRY_MS);
  }
}

function schedule(ms) {
  clearTimeout(timer);
  timer = setTimeout(() => checkQuietly(), ms);
}

/**
 * 앱 시작 때 한 번 부른다. openUpdate 는 줄·토스트를 누르면 여는 업데이트 확인 시트.
 * 보조 창(플레이어·채팅 창)에서는 부르지 않는다 — 알림이 창마다 겹치지 않게.
 */
export function installUpdateNotifier(openUpdate) {
  if (!isTauri) return;
  onOpen = openUpdate;
  const pill = document.getElementById('update-pill');
  if (pill) {
    pill.addEventListener('click', (e) => {
      if (e.target.closest('.update-pill-close')) {
        const u = lastUpdateResult();
        if (u && u.latest) writeKey(HIDDEN_KEY, u.latest);
        pill.hidden = true;
        return;
      }
      if (e.target.closest('.update-pill-main')) onOpen();
    });
  }
  // 수동 확인(메뉴·설정)으로 결과가 바뀌어도 줄을 맞춘다.
  window.addEventListener('cview:update-result', () => renderUpdatePill());
  // 잠자기에서 깨어나 오래 지났으면(타이머가 밀렸으면) 창이 다시 보일 때 확인한다.
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden && lastCheckAt && Date.now() - lastCheckAt > INTERVAL_MS) checkQuietly();
  });
  schedule(FIRST_DELAY_MS);
}
