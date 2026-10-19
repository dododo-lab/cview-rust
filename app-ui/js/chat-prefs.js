// 채팅 설정 공유 — Rust 설정 `chat` 도메인(원본 ChatSettings JSON 키)을 모든 채팅 패널이 함께 본다.
// 차단 사용자·금지어·표시 옵션·후원 필터·자동 스크롤 등. 바꾸면 저장하고 패널에 알린다.
import { api } from './api.js';

export const CHAT_DEFAULTS = Object.freeze({
  fontSize: 14,
  lineSpacing: 2,
  showTimestamp: true,
  showBadge: true,
  highlightMentions: true,
  mentionNotify: true,
  highlightRoles: true,
  maxVisibleMessages: 300,
  emoticonEnabled: true,
  showDonation: true,
  showDonationsOnly: false,
  autoScroll: true,
  chatFilterEnabled: false,
  blockedWords: [],
  blockedUsers: [],
  // 앱 전용 — 차단한 사용자 이름(blockedUsers 는 원본처럼 사용자 ID 문자열 목록), 하이라이트 대상
  blockedUserNames: {},
  highlightedUsers: [],
});

let prefs = { ...CHAT_DEFAULTS };
let myNickname = '';
const listeners = new Set();

function notify() {
  for (const fn of listeners) {
    try { fn(prefs); } catch (e) { console.error(e); }
  }
}

export function chatPrefs() {
  return prefs;
}

export function onChatPrefs(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export async function loadChatPrefs() {
  try {
    const s = await api.settingsGet();
    prefs = { ...CHAT_DEFAULTS, ...(s && s.chat ? s.chat : {}) };
  } catch { /* 기본값 */ }
  notify();
  return prefs;
}

export async function updateChatPrefs(patch) {
  prefs = { ...prefs, ...patch };
  notify();
  try { await api.settingsUpdate('chat', patch); } catch (e) { console.warn('채팅 설정 저장 실패', e); }
}

export function setMyNickname(nick) {
  myNickname = nick || '';
}

export function getMyNickname() {
  return myNickname;
}

/** 차단·금지어·후원 필터를 통과하는가. 순수 함수(테스트 가능). */
export function passesFilters(m, p = prefs) {
  if (!m) return false;
  if (m.kind === 'system') return true;
  if (m.userId && (p.blockedUsers || []).includes(m.userId)) return false;
  const isEvent = m.kind === 'donation' || m.kind === 'subscription';
  if (!p.showDonation && isEvent) return false;
  if (p.showDonationsOnly && !isEvent) return false;
  if (p.chatFilterEnabled && (p.blockedWords || []).length) {
    const text = String(m.content || '').toLowerCase();
    if (p.blockedWords.some((w) => w && text.includes(String(w).toLowerCase()))) return false;
  }
  return true;
}

/** 내 닉네임 멘션인가(@닉네임 또는 닉네임 그대로). */
export function mentionsMe(m, nick = myNickname) {
  if (!nick || !m || !m.content || m.nickname === nick) return false;
  return String(m.content).includes(`@${nick}`);
}

/** 차단한 사용자의 표시 이름(모르면 ID). */
export function blockedLabel(userId, p = prefs) {
  return (p.blockedUserNames && p.blockedUserNames[userId]) || userId;
}
