// 플랫폼 — macOS 기호 단축키(⌘⌥⇧⌃)를 Windows 표기로 바꾼다.
// 대응(index.html 의 키 보정과 메뉴 가속기와 같음): ⌘ → Ctrl, ⌥ → Alt, ⇧ → Shift, ⌃⌘ → Ctrl+Shift+Alt.
export const IS_WINDOWS = document.documentElement.dataset.platform === 'windows';

/** 문장 속 단축키 기호 묶음을 이 플랫폼 표기로. macOS 는 그대로. 순수 함수(플랫폼은 인자로 바꿀 수 있다). */
export function kbd(text, windows = IS_WINDOWS) {
  if (!windows || !text) return text;
  return String(text).replace(/[⌃⌥⇧⌘]+/g, (m) => {
    const ctrlCmd = m.includes('⌃') && m.includes('⌘');
    const parts = [];
    if (m.includes('⌘') || m.includes('⌃')) parts.push('Ctrl');
    if (m.includes('⌥') || ctrlCmd) parts.push('Alt');
    if (m.includes('⇧') || ctrlCmd) parts.push('Shift');
    return `${parts.join('+')}+`;
  });
}
