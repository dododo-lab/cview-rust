// 스플래시·부트 진행(SYS-10, 원본 SplashView/BootProgress)과 첫 실행 환영 안내(SYS-11, 원본 WelcomeOverlay).
const WELCOME_KEY = 'cview.welcomed.v1';

/** 스플래시 진행 단계 표시. */
export function splashStep(text, fraction) {
  const el = document.getElementById('splash');
  if (!el) return;
  el.querySelector('.splash-step').textContent = text;
  if (fraction != null) el.querySelector('.splash-bar span').style.width = `${Math.round(fraction * 100)}%`;
}

/** 스플래시를 걷는다(짧게 페이드). */
export function hideSplash() {
  const el = document.getElementById('splash');
  if (!el) return;
  splashStep('완료', 1);
  el.classList.add('leaving');
  setTimeout(() => el.remove(), 360);
}

const STEPS = [
  ['👋', 'CView 에 오신 걸 환영해요', '치지직 라이브를 데스크톱 앱으로 편하게 봅니다. 몇 가지만 알려 드릴게요.'],
  ['🔑', '로그인', '네이버로 로그인하면 팔로잉 방송·채팅 참여·알림을 쓸 수 있어요. 비밀번호는 네이버 로그인 창에만 입력됩니다.'],
  ['▦', '멀티라이브 · 멀티채팅', '여러 방송을 한 화면에 띄우고, 여러 채팅을 한곳에서 볼 수 있어요. 카드의 ＋ 버튼이나 ⌘4 로 시작합니다.'],
  ['⌘', '키보드', '⌘K 명령 팔레트, ⌘F 검색, ⌘⌥1~6 화면 전환, ⌘/ 단축키 목록. 설정에서 단축키를 바꿀 수 있어요.'],
  ['🔔', '알림 · 메뉴 막대', '팔로잉 방송 시작을 알려 드리고, 메뉴 막대 아이콘에서 방송 중 채널을 바로 열 수 있어요.'],
];

/** 환영 안내 — 처음 한 번, 또는 force 로 다시. */
export function showWelcome({ force = false, onLogin } = {}) {
  try { if (!force && localStorage.getItem(WELCOME_KEY)) return; } catch { /* 저장소 없음 */ }
  let i = 0;
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim welcome-scrim';
  const render = () => {
    const [icon, title, body] = STEPS[i];
    scrim.innerHTML = `<div class="sheet welcome" role="dialog" aria-modal="true" aria-label="환영 안내">
      <div class="welcome-icon">${icon}</div>
      <h2>${title}</h2><p>${body}</p>
      <div class="welcome-dots">${STEPS.map((_, k) => `<span class="${k === i ? 'on' : ''}"></span>`).join('')}</div>
      <div class="sheet-actions">
        <button class="btn ghost" data-w="skip">건너뛰기</button>
        ${i === 1 && onLogin ? '<button class="btn" data-w="login">지금 로그인</button>' : ''}
        <button class="btn primary" data-w="next">${i === STEPS.length - 1 ? '시작하기' : '다음'}</button>
      </div></div>`;
    scrim.querySelector('[data-w=next]').focus();
  };
  const close = () => {
    try { localStorage.setItem(WELCOME_KEY, String(Date.now())); } catch { /* 무시 */ }
    scrim.remove();
    document.removeEventListener('keydown', onKey, true);
  };
  const onKey = (e) => {
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'ArrowRight') { i = Math.min(STEPS.length - 1, i + 1); render(); }
    else if (e.key === 'ArrowLeft') { i = Math.max(0, i - 1); render(); }
  };
  scrim.addEventListener('click', (e) => {
    const b = e.target.closest('[data-w]');
    if (!b) return;
    if (b.dataset.w === 'skip') close();
    else if (b.dataset.w === 'login') { close(); onLogin(); }
    else if (i === STEPS.length - 1) close();
    else { i += 1; render(); }
  });
  document.addEventListener('keydown', onKey, true);
  document.body.append(scrim);
  render();
}

// 초기화가 중간에 멈춰도 화면을 가리지 않게 — 8초 뒤에는 무조건 걷는다.
setTimeout(hideSplash, 8000);
