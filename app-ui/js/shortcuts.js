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
  ['toggleFullscreen', '전체 화면 전환'],
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

/**
 * 바꿀 수 없는 예약 키(M-8) — 시청 화면이 직접 받는 맨 글자 키와 메뉴 막대·시스템이 가진 조합.
 * 사용자 단축키가 이 키를 쓰면 matchPlainAction 이 먼저 가로채 Space·M 같은 기본 키가 말없이 죽거나,
 * 메뉴 가속기가 먼저 받아 사용자 단축키가 아예 오지 않는다. F·C·P 는 바꿀 수 있는 동작(전체 화면·채팅·PiP)의
 * 기본값이라 여기 두지 않고 아래 동작끼리 비교에서 걸린다(그 동작을 다른 키로 옮기면 F·C·P 를 쓸 수 있다).
 * 항목: [key, modifiers, 설명]
 */
const C = MOD.cmd;
const S = MOD.shift;
export const RESERVED = [
  ['space', 0, '재생/일시정지'], ['m', 0, '음소거'], ['l', 0, '라이브로 이동'], ['b', 0, '북마크(시청 화면)'],
  ['upArrow', 0, '볼륨 올리기'], ['downArrow', 0, '볼륨 내리기'],
  ['leftArrow', 0, '되감기'], ['rightArrow', 0, '앞으로 감기'], ['leftArrow', S, '되감기 60초'], ['rightArrow', S, '앞으로 60초'],
  ['escape', 0, '닫기'], ['tab', 0, '초점 이동'], ['return', 0, '누르기'],
  ...['1', '2', '3', '4', '5', '6', '7'].map((k) => [k, C, '메뉴 이동']),
  ['f', C, '검색'], ['k', C, '명령 팔레트'], ['[', C, '뒤로'], ['r', C, '새로고침'], ['\\', C, '사이드바 접기'], [',', C, '설정'],
  ['/', C, '키보드 단축키'], ['l', C | S, '다크/라이트 전환'], ['f', C | S, '영상 전체 화면'], ['f', C | MOD.control, '영상+채팅 전체 화면'],
  ['p', C | MOD.option, 'PiP'], ['w', C, '시청 화면 닫기'], ['k', C | S, '채팅 지우기'], ['j', C, '자동 스크롤'],
  ['n', C, '새 플레이어 창'], ['c', C | S, '채팅 창'], ['m', C | S, '멀티채팅 창'], ['t', C | S, '통계 창'],
  ['q', C, '앱 종료'], ['h', C, '앱 가리기'], ['m', C, '창 최소화'],
  ['c', C, '복사'], ['v', C, '붙여넣기'], ['x', C, '잘라내기'], ['a', C, '전체 선택'], ['z', C, '실행 취소'],
];

/** 예약 키면 그 설명, 아니면 null. 순수 함수. */
export function reservedOf(binding) {
  if (!binding || !binding.key) return null;
  const m = binding.modifiers || 0;
  const hit = RESERVED.find(([k, mods]) => k === binding.key && mods === m);
  return hit ? hit[2] : null;
}

/**
 * 겹치는지 — 예약 키(막아야 함)와 다른 동작(경고만)을 함께 본다(M-8).
 * @returns {null | { kind: 'reserved', label: string } | { kind: 'action', action: string, label: string }}
 */
export function conflictOf(action, binding) {
  const reserved = reservedOf(binding);
  if (reserved) return { kind: 'reserved', label: reserved };
  for (const [a, b] of Object.entries(bindings)) {
    if (a !== action && b && b.key === binding.key && (b.modifiers || 0) === (binding.modifiers || 0)) {
      return { kind: 'action', action: a, label: ACTIONS.find(([id]) => id === a)?.[1] || a };
    }
  }
  return null;
}

/**
 * 키보드 단축키 도움말의 "시청" 묶음 — 고정 키 + 지금 바인딩으로 만든다(M-9). 도움말이 실제와 어긋나지 않게
 * app.js 의 SHORTCUTS 고정 표 대신 이것을 쓴다. 바인딩이 없는 동작은 뺀다.
 * @returns {Array<[string, string]>} [키 표기, 설명]
 */
export function shortcutHelpRows() {
  const b = currentBindings();
  const bound = (action, label) => (b[action] && b[action].key ? [[bindingLabel(b[action]), label]] : []);
  return [
    ['Space', '재생/일시정지'],
    ['M', '음소거'],
    ['↑ ↓', '볼륨'],
    ['L', '라이브로 이동'],
    ['← →', '되감기 10초 (⇧ 60초)'],
    ...bound('toggleChat', '채팅 표시 방식'),
    ...bound('toggleFullscreen', '영상 전체 화면'),
    [kbd('⇧⌘F'), '영상 전체 화면'],
    [kbd('⌃⌘F'), '영상+채팅 전체 화면'],
    ...bound('togglePiP', 'PiP'),
    [kbd('⌥⌘P'), 'PiP (메뉴)'],
    ['B', '북마크'],
    ...bound('bookmarkStream', '북마크'),
    ...bound('screenshot', '스크린샷'),
    [kbd('⌘W / Esc'), '시청 화면 닫기'],
  ];
}

window.addEventListener('cview:settings-changed', (e) => {
  const s = e.detail && e.detail.settings;
  if (s && s.keyboard) applyKeyboardSettings(s.keyboard);
});
