// 사이드바 아래 채널 목록 — 지금 보는 메뉴에 맞춰 다른 목록을 보여 준다.
// 순수 함수만 둔다(데이터는 app.js 가 모아 넘긴다). 결과가 null 이면 목록을 숨긴다.
//
//   홈·검색·통계·설정 등   방송 중          즐겨찾기 먼저, 다음은 시청자 많은 순
//   팔로잉                 방금 시작        팔로잉 중 방송을 막 시작한 순
//   인기                   인기 라이브      전체 인기 라이브 시청자 많은 순(로그인 없이도)
//   카테고리               방송 분야        팔로잉이 방송 중인 카테고리(로그인 없으면 인기 카테고리) — 누르면 그 카테고리로
//   라이브·멀티라이브      시청자 많은 순    팔로잉 방송 중 시청자 많은 순(즐겨찾기 우선 없이)
//   멀티채팅               채팅 추가        아직 열지 않은 팔로잉 — 누르면 멀티채팅에 채팅을 더한다
//   보관함                 최근 본 채널      최근 시청 기록(로그인 없이도) — 방송 중이면 시청자 수, 아니면 본 때
import { compactCount } from './cards.js';
import { categoryGroups } from './pages.js';
import { badgeKind } from './verified-badge.js';

export const SIDEBAR_LIST_LIMIT = 7;

const KIND_BY_ROUTE = {
  following: 'fresh',
  popular: 'popular',
  categories: 'categories',
  live: 'viewers',
  multilive: 'viewers',
  multichat: 'multichat',
  library: 'recent',
};

/** 메뉴 → 목록 종류. 정해 두지 않은 메뉴(홈·검색·통계·설정·채널 화면 등)는 'favorites'. */
export function sidebarListKind(route) {
  return KIND_BY_ROUTE[route] || 'favorites';
}

/** 그 목록 종류가 더 불러와야 하는 데이터. */
export function sidebarListNeeds(kind) {
  return { popular: kind === 'popular' || kind === 'categories', recent: kind === 'recent' };
}

const byViewers = (a, b) => (b.viewerCount || 0) - (a.viewerCount || 0);
const openedAt = (l) => {
  const t = Date.parse(l.openDate || '');
  return Number.isFinite(t) ? t : -Infinity;
};

/** "12분 전" / "2시간 전" / "3일 전". 알 수 없으면 ''. */
export function agoShort(ms, now = Date.now()) {
  if (!Number.isFinite(ms)) return '';
  const m = Math.max(0, Math.floor((now - ms) / 60000));
  if (m < 1) return '방금';
  if (m < 60) return `${m}분 전`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}시간 전`;
  const d = Math.floor(h / 24);
  return d < 30 ? `${d}일 전` : `${Math.floor(d / 30)}달 전`;
}

function liveItem(l, extra = {}) {
  return {
    channelId: l.channelId,
    name: l.channelName,
    image: l.channelImageUrl,
    badge: badgeKind(l),
    sub: l.categoryName || l.liveTitle || '',
    meta: l.viewerCountText || compactCount(l.viewerCount || 0),
    live: true,
    tip: `${l.channelName} · ${l.viewerCountText || compactCount(l.viewerCount || 0)}명`,
    ...extra,
  };
}

function finish(title, all, { empty, more = null, limit = SIDEBAR_LIST_LIMIT }) {
  const items = all.slice(0, limit);
  const rest = all.length - items.length;
  return { title, items, empty, more: more && rest > 0 ? { ...more, count: rest } : null };
}

const followMore = (count) => ({ route: 'following', tip: `팔로잉 ${count}명 더 보기` });

/**
 * 메뉴에 맞는 사이드바 목록.
 * @param {string} route 지금 메뉴
 * @param {{ following: Array|null, favorites?: Set<string>, popular?: Array|null, recent?: Array|null, multichatIds?: Set<string> }} data
 *   following 이 null 이면 비로그인(또는 실패)
 * @returns {{ title, items, empty, more, kind } | null}
 */
export function buildSidebarList(route, data, now = Date.now()) {
  const kind = sidebarListKind(route);
  const following = data.following;
  const favs = data.favorites || new Set();
  const list = (() => {
    switch (kind) {
      case 'popular': {
        if (!data.popular) return null;
        return finish('인기 라이브', [...data.popular].sort(byViewers).map((l) => liveItem(l)), { empty: '지금 인기 라이브를 불러오지 못했어요' });
      }
      case 'categories': {
        const mine = Boolean(following && following.length);
        const lives = mine ? following : data.popular;
        if (!lives) return following ? { title: '팔로잉 방송 분야', items: [], empty: '지금 방송 중인 팔로잉 채널이 없어요', more: null } : null;
        // 묶음마다 시청자가 가장 많은 채널의 아바타를 대표로 쓴다.
        const top = new Map();
        for (const l of [...lives].sort(byViewers)) if (!top.has(l.categoryName || '기타')) top.set(l.categoryName || '기타', l);
        const groups = mine ? categoryGroups(lives).sort((a, b) => (b.count - a.count) || (b.viewers - a.viewers)) : categoryGroups(lives);
        return finish(mine ? '팔로잉 방송 분야' : '인기 카테고리', groups.map((g) => ({
          category: g.name,
          name: g.name,
          image: top.get(g.name)?.channelImageUrl,
          sub: mine ? `팔로잉 ${g.count}명 방송 중` : `라이브 ${g.count}개`,
          meta: compactCount(g.viewers),
          live: true,
          tip: `${g.name} · ${mine ? `팔로잉 ${g.count}명` : `라이브 ${g.count}개`}`,
        })), { empty: '지금 방송 중인 카테고리가 없어요' });
      }
      case 'recent': {
        if (!data.recent) return null;
        const onAir = new Map([...(data.popular || []), ...(following || [])].map((l) => [l.channelId, l]));
        return finish('최근 본 채널', data.recent.map((c) => {
          const l = onAir.get(c.channelId);
          if (l) return liveItem({ ...l, channelName: c.channelName || l.channelName, channelImageUrl: l.channelImageUrl || c.channelImageUrl });
          const ago = agoShort(Number(c.lastWatched), now);
          return {
            channelId: c.channelId, name: c.channelName, image: c.channelImageUrl,
            sub: '오프라인', meta: ago, live: false, openPage: true,
            tip: `${c.channelName}${ago ? ` · ${ago} 시청` : ''}`,
          };
        }), { empty: '아직 본 채널이 없어요' });
      }
      default:
        break;
    }
    if (!following) return null;
    const noneLive = '지금 방송 중인 팔로잉 채널이 없어요';
    switch (kind) {
      case 'fresh':
        return finish('방금 시작', [...following].sort((a, b) => (openedAt(b) - openedAt(a)) || byViewers(a, b)).map((l) => {
          const ago = agoShort(openedAt(l), now);
          return liveItem(l, { meta: ago || l.viewerCountText, dot: !ago, tip: `${l.channelName}${ago ? ` · ${ago} 시작` : ''} · ${l.viewerCountText}명` });
        }), { empty: noneLive, more: followMore(0) });
      case 'viewers':
        return finish('시청자 많은 순', [...following].sort(byViewers).map((l) => liveItem(l)), { empty: noneLive, more: followMore(0) });
      case 'multichat': {
        const open = data.multichatIds || new Set();
        const rest = following.filter((l) => !open.has(l.channelId)).sort(byViewers);
        return finish('채팅 추가', rest.map((l) => liveItem(l, { action: 'multichat', tip: `${l.channelName} 채팅을 멀티채팅에 추가` })), {
          empty: following.length ? '방송 중인 팔로잉 채팅을 모두 열었어요' : noneLive,
        });
      }
      default:
        return finish('방송 중', [...following].sort((a, b) => (favs.has(b.channelId) - favs.has(a.channelId)) || byViewers(a, b))
          .map((l) => liveItem(l, { favorite: favs.has(l.channelId) })), { empty: noneLive, more: followMore(0) });
    }
  })();
  if (list && list.more) list.more.tip = `팔로잉 ${list.more.count}명 더 보기`;
  return list && { ...list, kind };
}
