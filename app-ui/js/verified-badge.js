// 치지직 인증 마크 — 파트너 · 챌린저 · 공식 채널.
// 응답의 verifiedMarkType(PARTNER / CHALLENGER / OFFICIAL)을 Rust 가 badge("partner" …)로 넘긴다.
// 마크 그림은 치지직 웹이 쓰는 공식 이미지(ssl.pstatic.net)를 이미지 캐시(cvimg:)로 받아 쓴다.
import { esc, imageSrc } from './dom.js';

const BASE = 'https://ssl.pstatic.net/static/nng/glive/image/';
export const VERIFIED_MARKS = Object.freeze({
  partner: { url: `${BASE}icon_partner_mark.png`, label: '치지직 파트너' },
  challenger: { url: `${BASE}icon_challenger_mark.png`, label: '치지직 챌린저' },
  official: { url: `${BASE}icon_official_mark.png`, label: '공식 채널' },
});

/**
 * 마크 종류 — 문자열("partner" / "PARTNER")이나 DTO·원시 응답 객체를 받는다.
 * 객체는 badge → verifiedBadge → verifiedMarkType 순서로 보고, 종류 없이 verified/verifiedMark 만 참이면
 * 챌린저 제도 이전 인증이므로 파트너로 본다.
 */
export function badgeKind(src) {
  if (!src) return null;
  if (typeof src === 'string') {
    const k = src.trim().toLowerCase();
    return Object.prototype.hasOwnProperty.call(VERIFIED_MARKS, k) ? k : null;
  }
  const named = badgeKind(src.badge) || badgeKind(src.verifiedBadge) || badgeKind(src.verifiedMarkType);
  if (named) return named;
  return src.verified === true || src.verifiedMark === true ? 'partner' : null;
}

/** 이름 옆에 붙일 마크 `<img>` — 인증이 없으면 빈 문자열. `size`: '' | 'sm' | 'lg'. */
export function verifiedBadge(src, size = '') {
  const kind = badgeKind(src);
  if (!kind) return '';
  const mark = VERIFIED_MARKS[kind];
  return `<img class="vbadge vbadge-${kind}${size ? ` ${esc(size)}` : ''}" src="${esc(imageSrc(mark.url, 'static'))}" alt="${esc(mark.label)}" title="${esc(mark.label)}" draggable="false" decoding="async" referrerpolicy="no-referrer">`;
}

// 그림을 받지 못하면(오프라인 등) 깨진 아이콘 대신 숨긴다 — CSP 로 인라인 onerror 를 못 쓰므로 위임.
if (typeof document !== 'undefined') {
  document.addEventListener('error', (e) => {
    const t = e.target;
    if (t && t.classList && t.classList.contains('vbadge')) t.hidden = true;
  }, true);
}
