// 플랫폼 — macOS 기호 단축키(⌘⌥⇧⌃)를 Windows 표기로 바꾸고 OS 기본 저장 폴더 경로를 관리한다.
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
/** 'TRAY_LABEL 로' — 조사까지(알림 영역으로 · 메뉴 막대로). */
export const TRAY_TO = IS_WINDOWS ? '알림 영역으로' : '메뉴 막대로';

/** Tauri `app_info` 가 알려 준 실제 OS 저장 폴더(도착 전·브라우저 모의 환경에서는 null). */
const resolvedDirs = { recording: null, screenshot: null, download: null };

export function noteSaveDirs(info) {
  if (!info || typeof info !== 'object') return;
  if (info.recordingDir) resolvedDirs.recording = String(info.recordingDir);
  if (info.screenshotDir) resolvedDirs.screenshot = String(info.screenshotDir);
  if (info.downloadDir) resolvedDirs.download = String(info.downloadDir);
}

/** 사용자 폴더 아래 경로 표기 — Windows 동영상 폴더는 'Movies' 대신 'Videos' 로 맞춘다. */
export function userPath(...parts) {
  if (IS_WINDOWS) {
    const winParts = parts.map((p, idx) => (idx === 0 && p === 'Movies' ? 'Videos' : p));
    return `%USERPROFILE%\\${winParts.join('\\')}`;
  }
  return `~/${parts.join('/')}`;
}

/** 실제 저장 경로(Tauri 경로 API 값 우선, 아직 없으면 플랫폼 기본 표기). */
export function saveDirPath(kind) {
  if (kind === 'recording') return resolvedDirs.recording || userPath('Movies', 'CView');
  if (kind === 'screenshot') return resolvedDirs.screenshot || userPath('Pictures', 'CView');
  return resolvedDirs.download || userPath('Downloads', 'CView');
}

/** 비밀 값을 두는 곳 — macOS 키체인, Windows 자격 증명 관리자(keyring). */
export const SECRET_STORE_LABEL = IS_WINDOWS ? 'Windows 자격 증명 관리자' : '키체인';
