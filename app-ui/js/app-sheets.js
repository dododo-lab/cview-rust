// 앱 메뉴의 시트 — 단축키 도움말·앱 정보·업데이트 확인. app.js 에서 떼어 냈다(동작 그대로).
import { api } from './api.js';
import { toast, dismiss, esc } from './dom.js';
import { checkForUpdate, currentVersionText, versionText, updateDetailHtml, downloadTarget, openDownload, canInstall, installUpdate, progressHtml, renderProgress } from './update-check.js';
import { shortcutHelpRows } from './shortcuts.js';
import { kbd } from './platform.js';
import { trapFocus } from './a11y.js';
import { HUBS } from './workspace.js';

// '시청' 묶음은 바꿀 수 있는 단축키를 따르므로 열 때마다 만든다(shortcuts.js shortcutHelpRows).
const shortcutGroups = () => [
  ['탐색', [['⌘1 ~ ⌘7', '홈 · 팔로잉 · 방송&클립 · 카테고리 · 라이브 · 멀티채팅 · 보관함(사이드바 순서)'], ['⌘8', '멀티라이브'], ['⌘K', '명령 팔레트'], ['⌘F', '검색'], ['⌘[  /  ⌥←', '뒤로'], ['⌘R', '새로고침'], ['⌘\\', '사이드바 접기'], ['⇧⌘L', '다크/라이트 전환'], ['⌘=  /  ⌘-  /  ⌘0', '화면 크기 확대 · 축소 · 실제 크기'], ['⌘,', '설정'], [`⌥⌘1 ~ ⌥⌘${HUBS.length}`, `작업 공간 — ${HUBS.map(([, , label]) => label).join(' · ')}`]]],
  ['시청', shortcutHelpRows()],
  ['다시보기', [['Space', '재생 / 멈춤'], ['←  /  →', '10초 뒤로 · 앞으로 (⇧ 60초)'], ['↑  /  ↓', '볼륨'], ['M', '소리 끄기 / 켜기'], ['F', '전체 화면']]],
  ['채팅', [['⇧⌘K', '채팅 지우기'], ['⌘J', '자동 스크롤'], [':이름', '이모티콘 자동완성'], ['@닉네임', '멘션 자동완성'], ['우클릭', '메시지 메뉴'], ['더블클릭', '프로필']]],
];

export function showSheet(html, cls = '') {
  // 앞 시트가 떠 있으면 먼저 풀어 초점을 그 시트를 연 자리로 돌려 둔다 — 새 시트가 그 자리를 이어받는다.
  // 예전엔 그냥 지워, 새 시트의 '연 자리'가 문서(body)가 되어 닫은 뒤 초점이 처음으로 떨어졌다.
  const prev = document.querySelector('.sheet-scrim.app-sheet');
  if (prev) { prev.releaseFocus?.(); prev.remove(); }
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim app-sheet';
  scrim.innerHTML = `<div class="sheet ${cls}" role="dialog" aria-modal="true" tabindex="-1">${html}<div class="sheet-actions"><button class="btn primary" data-close>닫기</button></div></div>`;
  const sheet = scrim.querySelector('.sheet');
  // 이름은 시트 제목(h2)에서 — 화면 낭독기가 '대화 상자'만 읽었다.
  const title = sheet.querySelector('h2');
  if (title) { title.id ||= `sheet-title-${Date.now().toString(36)}`; sheet.setAttribute('aria-labelledby', title.id); }
  document.body.append(scrim);
  // 초점을 가두고, 닫으면 연 자리(메뉴 단추 등)로 돌려준다.
  const release = trapFocus(sheet);
  scrim.releaseFocus = release;
  sheet.focus();
  const close = () => { release(); dismiss(scrim); };
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
  const scrim = showSheet(`<div class="about"><span class="brand-mark big"><img class="app-logo logo-light" src="img/logo-light.png" alt=""><img class="app-logo logo-dark" src="img/logo-dark.png" alt=""></span><h2>CView</h2>
    <p>치지직 라이브·채팅 데스크톱 앱 (Rust/Tauri 포팅판)</p>
    <p class="muted">버전 ${info ? versionText(info) : '-'} · ${info ? `${info.os} ${info.arch}` : ''}</p>
    <p class="about-oss">개인이 비영리로 만든 오픈소스 프로젝트입니다. NAVER·치지직과 관련 없는 비공식 앱입니다.</p>
    <p class="muted small">라이선스: GNU GPL v3.0 (GPL-3.0-only) · 재생: hls.js (Apache-2.0) · 셸: Tauri (MIT/Apache-2.0)</p>
    <nav class="about-sheet-links" aria-label="CView 프로젝트">${ABOUT_MENU.map(([id, label]) => `<button type="button" class="link-btn" data-about-menu="${id}">${label}</button>`).join('')}</nav></div>`);
  // 도움말 메뉴와 같은 동작(app.js MENU_ACTIONS) — 문의는 메일 앱, 저장소는 브라우저, 라이선스·고지는 시트.
  scrim.addEventListener('click', (e) => {
    const b = e.target.closest('[data-about-menu]');
    if (b) window.dispatchEvent(new CustomEvent('cview:menu', { detail: b.dataset.aboutMenu }));
  });
}

/** 정보 시트 아래 링크 — 도움말 메뉴(menu.rs HELP)의 프로젝트 항목과 같은 id·이름. */
const ABOUT_MENU = [
  ['contact', '문의하기 (do@dododo.app)'],
  ['github-repo', 'GitHub 저장소'],
  ['licenses', '오픈소스 라이선스'],
  ['notice', '이용 고지'],
];

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
