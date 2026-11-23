// 플랫폼 — macOS 기호 단축키(⌘⌥⇧⌃)를 Windows 표기로 바꾼다.
// 대응(index.html 의 키 보정과 메뉴 가속기와 같음): ⌘ → Ctrl, ⌥ → Alt, ⇧ → Shift, ⌃⌘ → Ctrl+Shift+Alt.
export const IS_WINDOWS = document.documentElement.dataset.platform === 'windows';

/** 문장 속 단축키 기호 묶음을 이 플랫폼 표기로. macOS 는 그대로. 순수 함수(플랫폼은 인자로 바꿀 수 있다). */
export function kbd(text, windows = IS_WINDOWS) {
  if (!windows || !text) return text;
  // 뒤에 키 이름이 붙는 묶음(⇧⌘F)은 "Ctrl+Shift+F", 띄어 쓴 설명("⇧ 60초")은 "Shift 60초" — 예전엔 "Shift+ 60초"(감사 UX-M12).
  // 문장 끝(bindingLabel 처럼 키를 뒤에 붙이는 쪽)은 + 를 남긴다.
  return String(text).replace(/[⌃⌥⇧⌘]+(?=(\s?))/g, (m, space) => {
    const ctrlCmd = m.includes('⌃') && m.includes('⌘');
    const parts = [];
    if (m.includes('⌘') || m.includes('⌃')) parts.push('Ctrl');
    if (m.includes('⌥') || ctrlCmd) parts.push('Alt');
    if (m.includes('⇧') || ctrlCmd) parts.push('Shift');
    return space ? parts.join('+') : `${parts.join('+')}+`;
  });
}

/** 파일 위치 열기 단추 이름 — macOS 'Finder에서 보기', Windows '탐색기에서 보기'(감사 UX-M12). */
export const REVEAL_LABEL = IS_WINDOWS ? '탐색기에서 보기' : 'Finder에서 보기';

/** 상태 아이콘 자리 이름 — macOS '메뉴 막대', Windows '알림 영역'(작업 표시줄 오른쪽, 감사 UX-M12). */
export const TRAY_LABEL = IS_WINDOWS ? '알림 영역' : '메뉴 막대';

/** 사용자 폴더 아래 경로 표기 — macOS '~/Downloads/CView', Windows '%USERPROFILE%\Downloads\CView'(Rust content.rs default_save_dir 와 같은 곳). */
export function userPath(...parts) {
  return IS_WINDOWS ? `%USERPROFILE%\\${parts.join('\\')}` : `~/${parts.join('/')}`;
}

/** 비밀 값을 두는 곳 — macOS 키체인, Windows 자격 증명 관리자(keyring). */
export const SECRET_STORE_LABEL = IS_WINDOWS ? 'Windows 자격 증명 관리자' : '키체인';
