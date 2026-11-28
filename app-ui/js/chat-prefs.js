// 채팅 설정 공유 — Rust 설정 `chat` 도메인(원본 ChatSettings JSON 키)을 모든 채팅 패널이 함께 본다.
// 차단 사용자·금지어·표시 옵션·후원 필터·자동 스크롤 등. 바꾸면 저장하고 패널에 알린다.
import { api, listen, Events } from './api.js';
import { toast } from './dom.js';

export const CHAT_DEFAULTS = Object.freeze({
  fontSize: 14,
  lineSpacing: 2,
  showTimestamp: true,
  showBadge: true,
  // 채팅 아바타 — 'photo'(프로필 사진, 없으면 랜덤 아이콘) · 'icon'(랜덤 아이콘만, 사진을 찾지 않음) · 'off'(숨김)
  chatAvatars: 'photo',
  // 멀티채팅·멀티라이브 통합 목록의 줄 모양 — 'compact'(촘촘: 한 줄에 태그·닉네임·글) · 'bubble'(아바타·말풍선)
  mergedChatStyle: 'compact',
  // 후원 목록 표시 — 'button'(머리의 후원 단추로 여는 목록, 위쪽 띠 없음) · 'strip'(예전처럼 머리 아래 후원 칩 띠·랭킹 줄도)
  donationList: 'button',
  highlightMentions: true,
  mentionNotify: true,
  nicknameColors: true,
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
  // 후원 읽어 주기 음량(0~1)·속도(200 = 보통) — 읽는 쪽(chat-extras.js)과 같은 기본값. 예전엔 기본값이 없어 설정 화면
  // 슬라이더가 빈 값(가운데 0.5·250)으로 그려지고 숫자도 비었다.
  ttsVolume: 0.8,
  ttsRate: 200,
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

let followingOtherWindows = false;
/** 다른 창·가져오기·초기화로 바뀐 채팅 설정을 이 창에도 — 저장할 때 옛 목록으로 덮어쓰지 않게(한 번만 건다). */
function followOtherWindows() {
  if (followingOtherWindows) return;
  followingOtherWindows = true;
  listen(Events.chatChanged, (chat) => {
    if (!chat || typeof chat !== 'object') return;
    prefs = { ...CHAT_DEFAULTS, ...chat };
    notify();
  }).catch(() => {});
}

export async function loadChatPrefs() {
  followOtherWindows();
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
  // 저장 실패를 조용히 삼키면 다음 실행에 설정이 되돌아가 있었다 — 알린다(감사 UX-M10). 화면 값은 이번 실행 동안 유지된다.
  try { await api.settingsUpdate('chat', patch); } catch (e) { console.warn('채팅 설정 저장 실패', e); toast(`채팅 설정을 저장하지 못했습니다 — ${e.message || e}`); }
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
  // '후원만 보기'는 후원 표시가 켜져 있을 때만 — 둘 다 걸면 모든 메시지가 막혀 채팅이 비었다(감사 UX-H23).
  if (p.showDonationsOnly && p.showDonation !== false && !isEvent) return false;
  if (p.chatFilterEnabled && (p.blockedWords || []).length) {
    const text = String(m.content || '').toLowerCase();
    if (p.blockedWords.some((w) => w && text.includes(String(w).toLowerCase()))) return false;
  }
  return true;
}

/** 내 닉네임 멘션인가(@닉네임 또는 닉네임 그대로). */
export function mentionsMe(m, nick = myNickname) {
  if (!nick || !m || !m.content || m.nickname === nick) return false;
  // 닉네임 바로 뒤가 글자·숫자·_ 면 다른 사람(@철수 ≠ @철수짱) — 예전엔 포함만 봐서 내 닉네임으로 시작하는
  // 더 긴 닉네임을 부른 말도 '나를 언급'으로 잡고 알림까지 띄웠다.
  const text = String(m.content);
  const tag = `@${nick}`;
  for (let i = text.indexOf(tag); i !== -1; i = text.indexOf(tag, i + 1)) {
    const next = text.slice(i + tag.length).match(/^./su)?.[0] || '';
    if (!next || !/[\p{L}\p{N}_]/u.test(next)) return true;
  }
  return false;
}

/** 차단한 사용자의 표시 이름(모르면 ID). */
export function blockedLabel(userId, p = prefs) {
  return (p.blockedUserNames && p.blockedUserNames[userId]) || userId;
}
