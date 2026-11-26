// 처음 실행 고지 — 비영리·비공식·오픈소스 안내와 동의(앱 메뉴 › 도움말 › 이용 고지 다시 보기로 다시 연다), 오픈소스 라이선스 시트.
// 동의한 문구 버전은 Rust 일반 설정(general.noticeAcceptedVersion, notice_accept 명령)에 남는다. 문구를 고치면 NOTICE_VERSION 을
// 올린다 — 다음 실행에 다시 묻는다. 기존 환영 안내(onboarding.js)는 동의한 뒤에 이어서 뜬다.
import { api } from './api.js';
import { dismiss, esc, toast } from './dom.js';
import { IS_WINDOWS } from './platform.js';
import { trapFocus } from './a11y.js';
import { showSheet } from './app-sheets.js';

/** 고지 문구 버전 — 문구(아래 noticeBody)를 바꾸면 올린다. 저장된 동의 버전이 이보다 낮으면 다시 묻는다. */
export const NOTICE_VERSION = 1;
/** 문의 메일 — 도움말 메뉴(menu.rs CONTACT_EMAIL)와 같다. */
export const CONTACT_EMAIL = 'do@dododo.app';
export const REPO_URL = 'https://github.com/dododo-lab/cview-rust';
export const LICENSE_URL = `${REPO_URL}/blob/main/LICENSE`;
/** 저장소 LICENSE(GNU GPL v3)와 Cargo.toml `license = "GPL-3.0-only"`. */
export const LICENSE_NAME = 'GNU GPL v3.0 (GPL-3.0-only)';

/**
 * 고지를 물어야 하는가 — 설정의 동의 버전이 지금 문구보다 낮을 때. 필드가 없으면(이 기능 이전 백엔드·모의 IPC) 묻지 않는다.
 * 순수 함수.
 */
export function needsNotice(general, version = NOTICE_VERSION) {
  const v = general && general.noticeAcceptedVersion;
  return typeof v === 'number' && v < version;
}

/** 첫 문장 — macOS '이 앱은', Windows '이 프로그램은'(운영체제의 관례 낱말). 순수 함수. */
export function noticeLead(windows = IS_WINDOWS) {
  return `${windows ? '이 프로그램은' : '이 앱은'} 개인이 비영리로 만든 오픈소스 프로젝트입니다.`;
}

/** 고지 항목 — [제목, 본문]. 문구를 바꾸면 NOTICE_VERSION 을 올린다. */
export const NOTICE_ITEMS = [
  ['비공식 프로젝트', 'CView는 NAVER(네이버)·치지직(CHZZK)과 관련이 없는 비공식 클라이언트입니다. 네이버가 만들거나 승인·후원하지 않았으며, 이 앱에 관한 문의는 네이버가 아니라 아래 문의처로 해 주세요.'],
  ['비영리', '이용료를 받지 않으며 영리를 목적으로 하지 않습니다.'],
  ['오픈소스', `소스 코드는 ${LICENSE_NAME} 라이선스로 GitHub(dododo-lab/cview-rust)에 공개되어 있습니다. 누구나 살펴보고 고쳐 다시 배포할 수 있습니다.`],
  ['상표와 콘텐츠', '치지직·CHZZK·NAVER 등의 이름과 상표는 각 소유자의 것입니다. 방송·채팅·클립 등 콘텐츠의 권리는 각 권리자에게 있으며, 치지직 이용약관과 운영 정책은 이 앱에서도 그대로 적용됩니다.'],
  ['보증 없음', '이 소프트웨어는 있는 그대로 제공되며 어떤 보증도 하지 않습니다. 치지직 서비스가 바뀌면 일부 기능이 예고 없이 동작하지 않을 수 있고, 사용에 따른 책임은 사용자에게 있습니다.'],
  ['데이터', '로그인 정보·설정·시청 기록은 이 컴퓨터에 저장하며, 설정에서 직접 켜지 않는 한 개발자에게 보내지 않습니다. 업데이트 확인을 위해 GitHub와 업데이트 서버(cv.dododo.app)에 접속합니다.'],
];

/** 고지 본문 HTML — 첫 문장·항목·문의처. 처음 실행 창과 다시 보기 시트가 함께 쓴다. */
export function noticeBody(windows = IS_WINDOWS) {
  return `<p class="notice-lead" data-notice-lead>${esc(noticeLead(windows))}</p>
    <dl class="notice-list">${NOTICE_ITEMS.map(([t, d]) => `<div><dt>${esc(t)}</dt><dd>${esc(d)}</dd></div>`).join('')}</dl>
    <p class="notice-contact">문의: <button type="button" class="link-btn" data-notice-link="contact">${CONTACT_EMAIL}</button>
      · 저장소: <button type="button" class="link-btn" data-notice-link="repo">github.com/dododo-lab/cview-rust</button></p>`;
}

/** 고지 안의 링크 — 메일 앱·기본 브라우저로만 연다(웹뷰는 그대로). 처리했으면 true. */
function onNoticeLink(e) {
  const b = e.target.closest('[data-notice-link]');
  if (!b) return false;
  openProjectLink(b.dataset.noticeLink);
  return true;
}

/** 문의·저장소·라이선스 전문 — 도움말 메뉴와 정보 시트가 함께 쓴다. */
export function openProjectLink(kind) {
  const url = kind === 'contact' ? `mailto:${CONTACT_EMAIL}` : kind === 'license' ? LICENSE_URL : REPO_URL;
  return api.openExternal(url).catch((er) => toast(kind === 'contact'
    ? `메일 앱을 열지 못했습니다 — ${CONTACT_EMAIL} 로 보내 주세요`
    : (er.message || String(er))));
}

let open = null;
/** 처음 실행 고지가 떠 있는가 — 떠 있는 동안 메뉴 동작·단축키를 막는다(app.js). */
export function noticeOpen() { return Boolean(open && open.isConnected); }

/**
 * 처음 실행 고지 — 동의하면 기록하고 Promise 를 푼다(이어서 환영 안내). 동의하지 않으면 앱을 끝낸다.
 * Esc·바깥 누름으로는 닫히지 않는다(어느 쪽인지 사용자가 골라야 한다). 처음 초점은 대화 상자 자체 — 화면 낭독기가 제목과
 * 첫 문장을 읽고, Enter 한 번으로 동의되지 않는다.
 */
export function showNotice({ windows = IS_WINDOWS } = {}) {
  if (noticeOpen()) return open.done;
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim notice-scrim';
  scrim.innerHTML = `<div class="sheet notice" role="dialog" aria-modal="true" tabindex="-1" aria-labelledby="notice-title" aria-describedby="notice-lead-text">
      <h2 id="notice-title">CView 이용 안내</h2>
      <div class="notice-body">${noticeBody(windows).replace('data-notice-lead', 'id="notice-lead-text" data-notice-lead')}</div>
      <p class="notice-ask">위 내용을 읽고 동의하면 CView를 시작합니다. 동의하지 않으면 ${windows ? '프로그램' : '앱'}을 종료합니다.</p>
      <div class="sheet-actions">
        <button type="button" class="btn" data-notice="decline">동의하지 않음 (종료)</button>
        <button type="button" class="btn primary" data-notice="accept">동의하고 시작</button>
      </div></div>`;
  const sheet = scrim.querySelector('.sheet');
  // 뒤의 앱은 누르거나 초점을 줄 수 없게 — 떠 있는 동안 body 의 다른 자식은 inert.
  const inerted = [...document.body.children].filter((el) => !el.inert);
  let resolve;
  const done = new Promise((r) => { resolve = r; });
  scrim.done = done;
  const onKey = (e) => {
    if (!scrim.isConnected) return;
    if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); return; }
    // 앱 단축키(⌘K 명령 팔레트·⌘F 검색 등)가 고지 뒤에서 돌지 않게 — 글 고르기·복사(⌘A·⌘C)만 둔다.
    if ((e.metaKey || e.ctrlKey) && !['a', 'c'].includes(e.key.toLowerCase())) e.stopImmediatePropagation();
  };
  const finish = () => {
    window.removeEventListener('keydown', onKey, true);
    inerted.forEach((el) => { el.inert = false; });
    release();
    dismiss(scrim);
    open = null;
    resolve();
  };
  scrim.addEventListener('click', async (e) => {
    if (onNoticeLink(e)) return;
    const b = e.target.closest('[data-notice]');
    if (!b || b.getAttribute('aria-disabled') === 'true') return;
    if (b.dataset.notice === 'accept') {
      b.setAttribute('aria-disabled', 'true');
      try { await api.noticeAccept(NOTICE_VERSION); } catch (er) {
        // 기록하지 못해도 이번 실행은 이어 간다 — 다음 실행에 다시 묻는다.
        toast(`동의 기록을 저장하지 못했습니다 — 다음 실행 때 다시 묻습니다 (${er.message || er})`, 5000);
      }
      finish();
    } else {
      b.setAttribute('aria-disabled', 'true');
      try { await api.appQuit(); } catch {
        b.removeAttribute('aria-disabled');
        toast(`${windows ? '프로그램' : '앱'}을 종료하지 못했습니다 — 창을 직접 닫아 주세요`, 5000);
      }
    }
  });
  document.body.append(scrim);
  inerted.forEach((el) => { el.inert = true; });
  const release = trapFocus(sheet, { restore: false });
  // 창(window) 캡처 — 문서 캡처에 먼저 걸린 앱 단축키(⌘K 등)보다 앞에서 막는다. Tab 은 건드리지 않아 trapFocus 가 가둔다.
  window.addEventListener('keydown', onKey, true);
  sheet.focus();
  open = scrim;
  return done;
}

const dateText = (ms) => {
  if (!Number.isFinite(ms) || ms <= 0) return '';
  try { return new Date(ms).toLocaleDateString('ko-KR', { year: 'numeric', month: 'long', day: 'numeric' }); } catch { return ''; }
};

/** 도움말 › 이용 고지 다시 보기 — 같은 문구를 시트로(닫기·Esc 로 닫힌다). 동의한 날을 함께 보인다. */
export async function showNoticeReview({ windows = IS_WINDOWS } = {}) {
  let general = null;
  try { general = (await api.settingsGet())?.general || null; } catch { /* 앱 밖 */ }
  const when = dateText(Number(general?.noticeAcceptedAt));
  const status = general && general.noticeAcceptedVersion >= NOTICE_VERSION
    ? `${when ? `${when}에 ` : ''}동의했습니다.`
    : '';
  const scrim = showSheet(`<div class="notice notice-review"><h2>CView 이용 안내</h2>
    <div class="notice-body">${noticeBody(windows)}</div>
    ${status ? `<p class="muted small notice-status">${esc(status)}</p>` : ''}</div>`, 'notice-sheet');
  scrim.addEventListener('click', onNoticeLink);
  return scrim;
}

/** 함께 쓰는 오픈소스 — [이름, 라이선스, 설명]. */
export const THIRD_PARTY = [
  ['hls.js', 'Apache-2.0', 'Copyright (c) 2017 Dailymotion · 라이브·다시보기 재생, 이 앱에 맞게 일부 수정한 판'],
  ['Tauri', 'MIT 또는 Apache-2.0', '데스크톱 앱 셸·웹뷰'],
  ['Rust 라이브러리', '각 라이브러리의 라이선스(주로 MIT·Apache-2.0)', '네트워크·채팅·저장소 등. 전체 목록은 저장소의 Cargo.lock 에 있습니다'],
];

/** 도움말 › 오픈소스 라이선스 — CView 자체 라이선스와 함께 쓰는 오픈소스. */
export function showLicenses() {
  const scrim = showSheet(`<div class="licenses-sheet"><h2>오픈소스 라이선스</h2>
    <p>CView는 오픈소스 프로젝트입니다. 소스 코드는 <strong>${esc(LICENSE_NAME)}</strong> 라이선스로 공개되어 있으며, 이 라이선스에 따라 누구나 사용·수정·재배포할 수 있습니다. 수정한 판을 배포할 때도 같은 라이선스로 소스를 공개해야 합니다.</p>
    <p class="licenses-links"><button type="button" class="link-btn" data-notice-link="license">라이선스 전문 보기</button> · <button type="button" class="link-btn" data-notice-link="repo">GitHub 저장소</button></p>
    <h3>함께 쓰는 오픈소스</h3>
    <ul class="licenses-list">${THIRD_PARTY.map(([n, l, d]) => `<li><span class="licenses-name">${esc(n)}</span> <span class="licenses-license">${esc(l)}</span><small>${esc(d)}</small></li>`).join('')}</ul></div>`, 'licenses');
  scrim.addEventListener('click', onNoticeLink);
  return scrim;
}
