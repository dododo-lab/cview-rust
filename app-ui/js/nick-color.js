// 닉네임 색 — 원본 CViewUI/ChatNicknamePalette·ChatAuthorColor 와 같은 규칙.
// - 역할이 있으면 역할색(스트리머 초록·매니저 파랑), 일반 시청자는 닉네임 해시색.
// - 해시: djb2(5381, ×33 + 바이트) 를 닉네임 UTF-8 바이트에 64비트로 — 같은 이름은 실행·테마가 바뀌어도 같은 색.
// - 팔레트 9색은 역할색과 겹치지 않게 고른 원본 값(다크·라이트 각각 4.5:1 이상). 색은 CSS(--nc-0…8)에 있다.
export const NICK_PALETTE_SIZE = 9;

const encoder = new TextEncoder();
const cache = new Map();

/** 닉네임 → 팔레트 번호(0…8). 원본 `ChatNicknamePalette.color(for:)` 와 같은 해시. 순수 함수. */
export function nickPaletteIndex(nickname) {
  const key = String(nickname || '');
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  let h = 5381n;
  const mask = (1n << 64n) - 1n;
  for (const b of encoder.encode(key)) h = (((h << 5n) + h) + BigInt(b)) & mask;
  const idx = Number(h % BigInt(NICK_PALETTE_SIZE));
  if (cache.size > 4096) cache.clear();
  cache.set(key, idx);
  return idx;
}

/** 닉네임 span 에 붙일 클래스 — 일반 시청자만 해시색(역할·내 메시지는 기존 색 유지). */
export function nickColorClass(nickname, role, isMe = false) {
  if (isMe || (role && role !== 'viewer')) return '';
  return ` nc nc-${nickPaletteIndex(nickname)}`;
}
