// 앱 메뉴의 시트 — 단축키 도움말·앱 정보·업데이트 확인. app.js 에서 떼어 냈다(동작 그대로).
import { api } from './api.js';
import { toast, dismiss, esc } from './dom.js';
import { checkForUpdate, currentVersionText, versionText, updateDetailHtml, downloadTarget, openDownload, canInstall, installUpdate, progressHtml, renderProgress } from './update-check.js';
import { shortcutHelpRows } from './shortcuts.js';
import { kbd } from './platform.js';

// '시청' 묶음은 바꿀 수 있는 단축키를 따르므로 열 때마다 만든다(shortcuts.js shortcutHelpRows).
const shortcutGroups = () => [
  ['탐색', [['⌘1 ~ ⌘7', '홈 · 팔로잉 · 인기 · 멀티라이브 · 라이브 · 보관함 · 카테고리'], ['⌘K', '명령 팔레트'], ['⌘F', '검색'], ['⌘[  /  ⌥←', '뒤로'], ['⌘R', '새로고침'], ['⌘\\', '사이드바 접기'], ['⇧⌘L', '다크/라이트 전환'], ['⌘,', '설정']]],
  ['시청', shortcutHelpRows()],
  ['채팅', [['⇧⌘K', '채팅 지우기'], ['⌘J', '자동 스크롤'], [':이름', '이모티콘 자동완성'], ['@닉네임', '멘션 자동완성'], ['우클릭', '메시지 메뉴'], ['더블클릭', '프로필']]],
];

export function showSheet(html, cls = '') {
  document.querySelector('.sheet-scrim.app-sheet')?.remove();
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim app-sheet';
  scrim.innerHTML = `<div class="sheet ${cls}" role="dialog" tabindex="-1">${html}<div class="sheet-actions"><button class="btn primary" data-close>닫기</button></div></div>`;
  document.body.append(scrim);
  scrim.querySelector('.sheet').focus();
  const close = () => dismiss(scrim);
  scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-close]')) close(); });
  scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); close(); } });
  return scrim;
}

export function showShortcuts() {
  showSheet(`<h2>키보드 단축키</h2><div class="shortcut-cols">${shortcutGroups().map(([g, list]) => `
    <section><h3>${g}</h3>${list.map(([k, d]) => `<div class="shortcut-row"><kbd>${kbd(k)}</kbd><span>${kbd(d)}</span></div>`).join('')}</section>`).join('')}</div>`, 'wide');
}

export async function showAbout() {
  let info = null;
  try { info = await api.appInfo(); } catch { /* 앱 밖 */ }
  showSheet(`<div class="about"><span class="brand-mark big"><img class="app-logo logo-light" src="img/logo-light.png" alt=""><img class="app-logo logo-dark" src="img/logo-dark.png" alt=""></span><h2>CView</h2>
    <p>치지직 라이브·채팅 데스크톱 앱 (Rust/Tauri 포팅판)</p>
    <p class="muted">버전 ${info ? versionText(info) : '-'} · ${info ? `${info.os} ${info.arch}` : ''}</p>
    <p class="muted small">재생: hls.js (Apache-2.0) · 셸: Tauri (MIT/Apache-2.0)</p></div>`);
}

/** 앱 메뉴 "업데이트 확인…" — 시트를 먼저 띄우고 결과로 채운다. 새 버전이면 "업데이트 설치" 로 받아 바꾸고 다시 연다. */
export async function showUpdateCheck() {
  const scrim = showSheet(`<div class="update-sheet"><h2>업데이트 확인</h2><div class="update-body"><p class="muted">업데이트 서버를 확인하는 중…</p></div></div>`);
  const body = scrim.querySelector('.update-body');
  const actions = scrim.querySelector('.sheet-actions');
  const closeBtn = actions.querySelector('[data-close]');
  let u = null;
  const setButtons = (html) => {
    actions.querySelectorAll('[data-update]').forEach((b) => b.remove());
    closeBtn.insertAdjacentHTML('beforebegin', html);
  };
  const run = async () => {
    setButtons('');
    body.innerHTML = '<p class="muted">업데이트 서버를 확인하는 중…</p>';
    const current = await currentVersionText();
    try { u = await checkForUpdate(); } catch (er) {
      if (!scrim.isConnected) return;
      body.innerHTML = `<p class="update-summary">업데이트를 확인하지 못했습니다</p><p class="muted small">${esc(er.message || String(er))}</p>`;
      setButtons('<button class="btn" data-update="retry">다시 시도</button>');
      return;
    }
    if (!scrim.isConnected) return;
    body.innerHTML = updateDetailHtml(u, current);
    const buttons = [];
    if (u.url && (u.newer || !u.downloadUrl)) buttons.push('<button class="btn" data-update="page">릴리스 페이지</button>');
    if (canInstall(u)) buttons.push('<button class="btn primary" data-update="install">업데이트 설치</button>');
    else if (u.newer && downloadTarget(u)) buttons.push('<button class="btn primary" data-update="download">다운로드</button>');
    closeBtn.classList.toggle('primary', !u.newer);
    setButtons(buttons.join(''));
  };
  const install = async () => {
    actions.querySelectorAll('[data-update]').forEach((b) => { b.disabled = true; });
    body.querySelector('.update-error')?.remove();
    body.insertAdjacentHTML('beforeend', progressHtml());
    const progress = body.querySelector('.update-progress');
    try {
      await installUpdate((st) => renderProgress(progress, st));
    } catch (er) {
      const msg = er.message || String(er);
      if (!scrim.isConnected) { toast(`업데이트를 설치하지 못했습니다 — ${msg}`); return; }
      progress.remove();
      body.insertAdjacentHTML('beforeend', `<p class="update-error small">설치하지 못했습니다 — ${esc(msg)}</p>`);
      const fallback = downloadTarget(u) ? '<button class="btn" data-update="download">직접 다운로드</button>' : '';
      setButtons(`${fallback}<button class="btn primary" data-update="install">다시 시도</button>`);
    }
  };
  scrim.onclick = (e) => {
    const b = e.target.closest('[data-update]');
    if (!b || b.disabled) return;
    if (b.dataset.update === 'retry') run();
    else if (b.dataset.update === 'install') install();
    else if (b.dataset.update === 'page') api.openExternal(u.url).catch((er) => toast(er.message));
    else if (b.dataset.update === 'download') openDownload(u).catch((er) => toast(er.message));
  };
  run();
}
