// 설정 › 정보 — 앱 머리말(아이콘·이름·버전·소개), 업데이트, 도움말(피드백·진단 보고서·사용 데이터), 아래 링크 띠.
// 업데이트 줄의 확인·설치는 settings.js(update-row · data-sys)가 맡는다 — 앱 메뉴 시트와 같은 update-check.js 흐름.
// 진단 보고서는 백엔드 diagnostic_report 가 만든다(개인 정보를 넣지 않는 규칙은 crates/cview-app/src/diagnostics.rs).
import { api } from './api.js';
import { esc, toast } from './dom.js';
import { IS_WINDOWS, kbd } from './platform.js';
import { showSheet } from './app-sheets.js';
import { currentAppInfo, versionText, dateText } from './update-check.js';

// 공개된 주소만 — 웹사이트(내려받기)는 README, 피드백은 저장소 이슈 양식(.github/ISSUE_TEMPLATE).
export const ABOUT_LINKS = {
  website: 'https://cv.dododo.app/',
  releases: 'https://github.com/dododo-lab/cview-rust/releases',
  source: 'https://github.com/dododo-lab/cview-rust',
  feedback: 'https://github.com/dododo-lab/cview-rust/issues/new/choose',
};
const REVEAL = IS_WINDOWS ? '탐색기에서 보기' : 'Finder에서 보기';
const REPORT_PRIVACY = '로그인 정보(쿠키·토큰)·계정 이름·채팅·팔로잉과 시청한 채널·파일 경로는 넣지 않습니다';

/**
 * 정보 탭 HTML. 검색 중이면 머리말과 링크 띠는 빼고 설정 줄 묶음만(다른 탭 결과와 함께 흐른다).
 * autoCheckHtml — 설정 스키마로 그린 '자동으로 업데이트 확인' 토글 줄. updateText — 업데이트 줄의 처음 상태 글.
 */
export function aboutHtml({ autoCheckHtml = '', updateText = '', searching = false } = {}) {
  const hero = searching ? '' : `<header class="about-hero" data-g="about">
      <span class="brand-mark big about-icon" aria-hidden="true"><img class="app-logo logo-light" src="img/logo-light.png" alt=""><img class="app-logo logo-dark" src="img/logo-dark.png" alt=""></span>
      <h2 class="about-name">CView</h2>
      <p class="about-version" data-about-text="version">버전 확인 중…</p>
      <p class="about-tagline">치지직 라이브를 위한 데스크톱 뷰어</p>
    </header>`;
  const foot = searching ? '' : `<footer class="about-foot" data-g="about">
      <nav class="about-links" aria-label="CView 링크">
        <a href="${ABOUT_LINKS.website}" data-ext title="cv.dododo.app">웹사이트</a>
        <a href="${ABOUT_LINKS.releases}" data-ext title="GitHub 릴리스">변경 내역</a>
        <a href="${ABOUT_LINKS.source}" data-ext title="GitHub 저장소 (GPL-3.0)">소스 코드</a>
      </nav>
      <span class="about-credit">개인이 만든 비공식 앱 · NAVER·치지직과 관련 없음</span>
    </footer>`;
  return `${hero}
    <section class="settings-group" data-g="about"><h2>업데이트</h2>
      <div class="setting update-row" data-search="업데이트 확인 설치 버전 다운로드 릴리스 새 버전">
        <span class="setting-label"><span class="update-version">CView</span><small class="update-status" role="status">${esc(updateText || '업데이트 서버의 최신 릴리스와 비교합니다')}</small></span>
        <span class="shortcut-edit setting-actions"><button class="link-btn" data-sys="update-page" hidden>릴리스 노트</button><button class="btn primary" data-sys="update-download" hidden>다운로드</button><button class="btn primary" data-sys="update-install" hidden>업데이트 설치</button><button class="btn" data-sys="update">업데이트 확인</button></span>
      </div>
      ${autoCheckHtml}
      <div class="setting" data-search="새로운 기능 변경 사항 릴리스 노트 바뀐 점">
        <span class="setting-label"><span data-about-text="notes">이 버전의 새로운 기능</span><small>이 버전에서 바뀐 점을 릴리스 노트로 봅니다</small></span>
        <button class="btn" data-about="notes" aria-haspopup="dialog" aria-label="이 버전의 새로운 기능 보기">보기…</button>
      </div>
    </section>
    <section class="settings-group" data-g="about"><h2>도움말</h2>
      <div class="setting" data-search="피드백 보내기 버그 제보 제안 이슈 github 문의">
        <span class="setting-label">피드백 보내기<small>버그 제보나 제안을 GitHub 이슈로 남겨 주세요. GitHub 계정이 필요합니다.</small></span>
        <button class="btn" data-about="feedback" aria-label="피드백 작성 — GitHub 이슈 열기">작성…</button>
      </div>
      <div class="setting" data-search="진단 보고서 복사 로그 버그 문제">
        <span class="setting-label">진단 보고서<small>앱 버전·운영체제·주요 설정·최근 디버그 로그를 복사합니다. ${REPORT_PRIVACY}. 이슈에 붙여 넣어 보내 주세요.</small></span>
        <span class="shortcut-edit setting-actions"><button class="link-btn" data-about="report-view" aria-haspopup="dialog" aria-label="진단 보고서 내용 보기">내용 보기</button><button class="btn" data-about="report-copy" aria-label="진단 보고서 복사">보고서 복사</button></span>
      </div>
      <div class="setting" data-search="사용 데이터 폴더 앱 데이터 저장 위치 finder 탐색기">
        <span class="setting-label">사용 데이터<small>설정·시청 기록·썸네일 캐시·디버그 로그가 담긴 앱 데이터 폴더입니다. 이 컴퓨터에만 저장됩니다.</small></span>
        <button class="btn" data-about="data" aria-label="사용 데이터 폴더를 ${REVEAL}">${REVEAL}</button>
      </div>
    </section>
    ${foot}`;
}

/** 그린 뒤 버전 글을 채운다 — 머리말 '버전 0.1.103 (빌드 103)', 새로운 기능 줄 '0.1.103의 새로운 기능'. */
export async function fillAbout(root) {
  const info = await currentAppInfo();
  if (!info || !root.isConnected) return;
  const v = root.querySelector('[data-about-text="version"]');
  if (v) v.textContent = `버전 ${versionText(info)}`;
  const notes = root.querySelector('[data-about-text="notes"]');
  if (notes) notes.textContent = `${info.version}의 새로운 기능`;
}

/** 정보 탭의 누름 — 처리했으면 true. 링크(a[data-ext])는 웹뷰가 아니라 기본 브라우저로 연다. */
export function onAboutClick(e) {
  const link = e.target.closest('a[data-ext]');
  if (link) {
    e.preventDefault();
    api.openExternal(link.href).catch((er) => toast(er.message || String(er)));
    return true;
  }
  const b = e.target.closest('button[data-about]');
  if (!b) return false;
  if (b.getAttribute('aria-disabled') === 'true') return true;
  const act = b.dataset.about;
  if (act === 'notes') showReleaseNotes();
  else if (act === 'feedback') api.openExternal(ABOUT_LINKS.feedback).catch((er) => toast(er.message || String(er)));
  else if (act === 'report-copy') copyReport(b);
  else if (act === 'report-view') viewReport(b);
  else if (act === 'data') revealData();
  return true;
}

/**
 * 클립보드에 글을 넣는다. WebKit(WKWebView)은 누름 처리 중에 시작한 쓰기만 받아, 보고서를 IPC 로 받는 동안 사용자 동작이
 * 풀린다 — 그래서 내용을 Promise 로 넘기는 ClipboardItem 을 먼저 쓰고, 안 되면 다 받은 뒤 writeText 로.
 */
async function copyText(textPromise) {
  const clip = navigator.clipboard;
  if (clip && typeof clip.write === 'function' && typeof window.ClipboardItem === 'function') {
    try {
      const blob = textPromise.then((t) => new Blob([t], { type: 'text/plain' }));
      blob.catch(() => {}); // ClipboardItem 을 못 만들어 아무도 기다리지 않을 때 처리 안 된 거부로 남지 않게
      await clip.write([new window.ClipboardItem({ 'text/plain': blob })]);
      return;
    } catch { /* 아래 writeText 로 */ }
  }
  const text = await textPromise;
  if (!clip || typeof clip.writeText !== 'function') throw new Error('클립보드를 쓸 수 없습니다');
  await clip.writeText(text);
}

/** 누르는 동안 다시 눌리지 않게 — disabled 는 초점을 잃게 해 aria-disabled 로 막는다. */
async function busy(b, run) {
  b.setAttribute('aria-disabled', 'true');
  try { return await run(); } finally { b.removeAttribute('aria-disabled'); }
}

function copyReport(b) {
  return busy(b, async () => {
    const report = api.diagnosticReport();
    report.catch(() => {}); // 실패는 아래에서 한 번만 알린다
    try {
      await copyText(report);
      toast('진단 보고서를 복사했습니다 — 이슈에 붙여 넣어 주세요', 3500);
    } catch {
      let text = null;
      try { text = await report; } catch (er) { toast(`진단 보고서를 만들지 못했습니다 — ${er.message || er}`); return; }
      showReport(text, { copyFailed: true });
    }
  });
}

function viewReport(b) {
  return busy(b, async () => {
    try { showReport(await api.diagnosticReport()); } catch (er) { toast(`진단 보고서를 만들지 못했습니다 — ${er.message || er}`); }
  });
}

/** 보고서 내용 시트 — 복사 전에 무엇이 들어가는지 보여 준다. 클립보드에 넣지 못했을 때도 여기서 직접 복사한다. */
function showReport(text, { copyFailed = false } = {}) {
  const note = copyFailed
    ? `클립보드에 넣지 못했습니다. 아래 글을 모두 골라(${kbd('⌘A')}) 복사해(${kbd('⌘C')}) 주세요.`
    : `복사하면 아래 글이 그대로 들어갑니다. ${REPORT_PRIVACY}.`;
  const scrim = showSheet(`<div class="report-sheet"><h2>진단 보고서</h2><p class="muted small report-note">${esc(note)}</p>
    <textarea class="report-text" readonly rows="14" spellcheck="false" aria-label="진단 보고서 내용">${esc(text)}</textarea></div>`, 'wide');
  const actions = scrim.querySelector('.sheet-actions');
  actions.insertAdjacentHTML('afterbegin', '<button class="btn" data-report-copy>복사</button>');
  const area = scrim.querySelector('.report-text');
  if (copyFailed) { area.focus(); area.select(); }
  actions.querySelector('[data-report-copy]').addEventListener('click', () => {
    copyText(Promise.resolve(text)).then(
      () => toast('진단 보고서를 복사했습니다 — 이슈에 붙여 넣어 주세요', 3500),
      () => { area.focus(); area.select(); toast(`복사하지 못했습니다 — ${kbd('⌘C')} 로 복사해 주세요`); },
    );
  });
}

/** "이 버전의 새로운 기능" — 지금 버전의 릴리스 노트를 시트로. GitHub 릴리스 페이지로도 갈 수 있다. */
async function showReleaseNotes() {
  const info = await currentAppInfo();
  const title = info ? `${info.version}의 새로운 기능` : '이 버전의 새로운 기능';
  const scrim = showSheet(`<div class="update-sheet notes-sheet"><h2>${esc(title)}</h2><div class="update-body" aria-live="polite"><p class="muted">릴리스 노트를 받는 중…</p></div></div>`, 'notes');
  const body = scrim.querySelector('.update-body');
  const actions = scrim.querySelector('.sheet-actions');
  const closeBtn = actions.querySelector('[data-close]');
  let page = ABOUT_LINKS.releases;
  // 누른 단추([다시 시도])를 지우면 초점이 문서로 빠져 Esc·Tab 이 시트에 닿지 않는다 — 그때는 시트로 옮긴다.
  const setButtons = (html) => {
    const old = [...actions.querySelectorAll('[data-notes]')];
    const lost = old.includes(document.activeElement);
    old.forEach((x) => x.remove());
    closeBtn.insertAdjacentHTML('beforebegin', html);
    if (lost) scrim.querySelector('.sheet').focus();
  };
  const run = async () => {
    setButtons('');
    body.innerHTML = '<p class="muted">릴리스 노트를 받는 중…</p>';
    let r;
    try { r = await api.releaseNotes(); } catch (er) {
      if (!scrim.isConnected) return;
      body.innerHTML = `<p class="update-summary">릴리스 노트를 받지 못했습니다</p><p class="muted small">${esc(er.message || String(er))}</p>`;
      setButtons('<button class="btn" data-notes="page">GitHub에서 보기</button><button class="btn" data-notes="retry">다시 시도</button>');
      return;
    }
    if (!scrim.isConnected) return;
    page = r.pageUrl || ABOUT_LINKS.releases;
    const date = dateText(r.publishedAt);
    body.innerHTML = r.notes
      ? `${date ? `<p class="muted small">${esc(date)} 게시</p>` : ''}<pre class="update-notes release-notes" tabindex="0" aria-label="릴리스 노트">${esc(r.notes.trim())}</pre>`
      : '<p class="update-summary">이 버전의 릴리스 노트가 없습니다</p><p class="muted small">개발 빌드이거나 아직 게시하지 않은 버전입니다. 지난 버전의 변경 내역은 GitHub에서 볼 수 있습니다.</p>';
    setButtons('<button class="btn" data-notes="page">GitHub에서 보기</button>');
  };
  scrim.addEventListener('click', (e) => {
    const b = e.target.closest('[data-notes]');
    if (!b) return;
    if (b.dataset.notes === 'retry') run();
    else api.openExternal(page).catch((er) => toast(er.message || String(er)));
  });
  run();
}

async function revealData() {
  const info = await currentAppInfo();
  try {
    if (!info || !info.dataDir) throw new Error('폴더 위치를 알 수 없습니다');
    await api.revealPath(info.dataDir);
  } catch (er) { toast(`사용 데이터 폴더를 열지 못했습니다 — ${er.message || er}`); }
}
