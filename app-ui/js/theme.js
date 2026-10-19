// 화면 테마 — 시스템 / 다크 / 라이트 (원본 AppearanceSettings.theme).
//
// 값은 Rust 설정(appearance.theme)이 원본이고, 첫 페인트 전에 쓰려고 localStorage 에도 둔다
// (index.html 의 첫 스크립트가 읽는다). 바꾸면 Rust 가 창 타이틀바 테마도 맞춘다.
import { invoke } from './api.js';

const KEY = 'cview.theme';
export const THEMES = ['system', 'dark', 'light'];
export const THEME_LABELS = { system: '시스템', dark: '다크', light: '라이트' };
const listeners = new Set();
const lightQuery = window.matchMedia('(prefers-color-scheme: light)');

export function currentTheme() {
  const t = document.documentElement.dataset.theme;
  return THEMES.includes(t) ? t : 'dark';
}

/** 시스템이면 실제로 보이는 쪽(dark | light). */
export function resolvedTheme(theme = currentTheme()) {
  if (theme !== 'system') return theme;
  return lightQuery.matches ? 'light' : 'dark';
}

function apply(theme) {
  document.documentElement.dataset.theme = theme;
  try { localStorage.setItem(KEY, theme); } catch { /* 저장 불가 — 이번 실행만 */ }
  for (const fn of listeners) fn(theme, resolvedTheme(theme));
}

/** 설정에서 읽은 값으로 맞춘다(저장하지 않는다). */
export function syncTheme(theme) {
  if (THEMES.includes(theme) && theme !== currentTheme()) apply(theme);
}

/** 사용자가 고른 테마 — 적용하고 설정·창 테마에 저장한다. */
export async function setTheme(theme) {
  if (!THEMES.includes(theme)) return;
  apply(theme);
  try {
    await invoke('appearance_set_theme', { theme });
  } catch { /* 앱 밖(브라우저 미리보기) */ }
}

export function onThemeChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

lightQuery.addEventListener('change', () => {
  if (currentTheme() === 'system') for (const fn of listeners) fn('system', resolvedTheme());
});
