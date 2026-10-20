// 사용자 단축키 — 원본 KeyboardShortcutSettings·GeneralSettingsTab "키보드 단축키" (SET-12).
// 바인딩은 설정 keyboard 도메인(`{ bindings: [동작, {key, modifiers}, …] }`)에 있다. 수식 키가 있는 바인딩은
// 메뉴 막대가 가속기로 받고(셸이 메뉴를 다시 만든다), 맨 글자 바인딩은 여기서 시청 화면 키로 받는다.
import { api } from './api.js';
import { kbd } from './platform.js';

export const MOD = { cmd: 1, shift: 2, option: 4, control: 8 };
export const ACTIONS = [
  ['togglePlay', '재생/일시정지', 'Space'],
  ['toggleMute', '음소거 전환', 'M'],
  ['volumeUp', '볼륨 올리기', '↑'],
  ['volumeDown', '볼륨 내리기', '↓'],
  ['toggleFullscreen', '전체화면 전환'],
  ['toggleChat', '채팅 표시 방식 바꾸기'],
  ['togglePiP', 'PiP 전환'],
  ['screenshot', '스크린샷'],
  ['bookmarkStream', '스트림 북마크'],
];
/** 메뉴가 소유한 고정 단축키(바꿀 수 없음). */
export const FIXED = new Set(['togglePlay', 'toggleMute', 'volumeUp', 'volumeDown']);
const DEFAULTS = {
  toggleFullscreen: { key: 'f', modifiers: 0 },
  toggleChat: { key: 'c', modifiers: 0 },
  togglePiP: { key: 'p', modifiers: 0 },
  screenshot: { key: 's', modifiers: MOD.cmd },
  bookmarkStream: { key: 'b', modifiers: MOD.cmd },
};
const KEY_NAMES = { ' ': 'space', ArrowUp: 'upArrow', ArrowDown: 'downArrow', ArrowLeft: 'leftArrow', ArrowRight: 'rightArrow', Enter: 'return', Escape: 'escape', Tab: 'tab', Backspace: 'delete' };
const KEY_LABELS = { space: 'Space', upArrow: '↑', downArrow: '↓', leftArrow: '←', rightArrow: '→', return: '⏎', escape: 'Esc', tab: '⇥', delete: '⌫' };

let bindings = { ...DEFAULTS };
const listeners = new Set();

/** `[동작, 바인딩, …]` → 객체. 순수 함수. */
export function pairsToMap(pairs) {
  const out = {};
  if (Array.isArray(pairs)) for (let i = 0; i + 1 < pairs.length; i += 2) out[pairs[i]] = pairs[i + 1];
  return out;
}

export function mapToPairs(map) {
  return Object.keys(map).sort().flatMap((k) => [k, map[k]]);
}

/** 키 이벤트 → 바인딩. 수식 키만 누른 경우 null. 순수 함수. */
export function bindingFromEvent(e) {
  if (['Meta', 'Shift', 'Alt', 'Control'].includes(e.key)) return null;
  const key = KEY_NAMES[e.key] || (e.code && e.code.startsWith('Key') ? e.code.slice(3).toLowerCase() : e.key.toLowerCase());
  const modifiers = (e.metaKey ? MOD.cmd : 0) | (e.shiftKey ? MOD.shift : 0) | (e.altKey ? MOD.option : 0) | (e.ctrlKey ? MOD.control : 0);
  return { key, modifiers };
}

export function bindingLabel(b) {
  if (!b || !b.key) return '없음';
  const m = b.modifiers || 0;
  return kbd(`${m & MOD.control ? '⌃' : ''}${m & MOD.option ? '⌥' : ''}${m & MOD.shift ? '⇧' : ''}${m & MOD.cmd ? '⌘' : ''}`) + (KEY_LABELS[b.key] || b.key.toUpperCase());
}

export function currentBindings() {
  return { ...bindings };
}

export function onShortcuts(fn) {
  listeners.add(fn);
}

export function applyKeyboardSettings(keyboard) {
  const map = pairsToMap(keyboard && keyboard.bindings);
  bindings = { ...DEFAULTS };
  for (const k of Object.keys(DEFAULTS)) if (map[k]) bindings[k] = map[k];
  for (const fn of listeners) fn(bindings);
}

export async function loadShortcuts() {
  try { applyKeyboardSettings((await api.settingsGet()).keyboard); } catch { /* 기본값 */ }
}

/** 시청 화면 키 처리용 — 맨 글자(수식 키 없음) 바인딩만 맞춘다. 수식 키 바인딩은 메뉴 몫. */
export function matchPlainAction(e) {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  const b = bindingFromEvent(e);
  if (!b) return null;
  for (const [action, bind] of Object.entries(bindings)) {
    if (bind && !bind.modifiers && bind.key === b.key && !(e.shiftKey)) return action;
  }
  return null;
}

/** 저장 — 바꾼 바인딩을 전체 배열로 보낸다(셸이 메뉴를 다시 만든다). */
export async function saveBinding(action, binding) {
  const s = await api.settingsGet();
  const map = pairsToMap(s.keyboard && s.keyboard.bindings);
  if (binding) map[action] = binding;
  else delete map[action];
  const next = await api.settingsUpdate('keyboard', { bindings: mapToPairs(map) });
  applyKeyboardSettings(next.keyboard);
  return next;
}

/** 다른 동작과 겹치는지. */
export function conflictOf(action, binding) {
  for (const [a, b] of Object.entries(bindings)) {
    if (a !== action && b && b.key === binding.key && (b.modifiers || 0) === (binding.modifiers || 0)) return a;
  }
  return null;
}

window.addEventListener('cview:settings-changed', (e) => {
  const s = e.detail && e.detail.settings;
  if (s && s.keyboard) applyKeyboardSettings(s.keyboard);
});
