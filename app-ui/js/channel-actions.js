// 시청 화면 상단 바의 채널 동작 — 팔로우·즐겨찾기·링크 복사·치지직에서 열기.
// 라이브 단독 보기(ml-watch.js)와 예전 시청 화면(viewer.js)이 같이 쓴다. 모두 이미 있는 명령만 부른다:
//   팔로우 상태 following_channels_all(캐시) · 바꾸기 channel_follow · 즐겨찾기 favorites_list / favorite_set · open_external.
import { api } from './api.js';
import { toast } from './dom.js';

export const CHANNEL_ICON = {
  follow: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10Z"/></svg>',
  following: '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10Z"/></svg>',
  star: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>',
  starOn: '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>',
  link: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1"/></svg>',
  external: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M20 4l-8 8"/><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>',
  multi: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="8" height="7" rx="1.5"/><rect x="13" y="4" width="8" height="7" rx="1.5"/><rect x="3" y="13" width="8" height="7" rx="1.5"/><path d="M17 14v6M14 17h6"/></svg>',
  home: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 11 12 4l8 7v8a1 1 0 0 1-1 1h-4v-6h-6v6H5a1 1 0 0 1-1-1z"/></svg>',
};

/**
 * 이 창에서 바뀐 팔로우·즐겨찾기 — 채널 id → { following?, favorite? }. 팔로우 목록(following_channels_all)은 서버 캐시라
 * 바꾼 직후 1분 안에 다시 읽으면 바꾸기 전 상태가 돌아왔다(다시 누르면 같은 요청을 또 보냈다). 알게 된 마지막 값이 캐시보다 앞선다.
 * 어디서 바꾸든(상단 바·채팅 말풍선·채널 화면·카드 메뉴) channelSocialChanged 로 알리면 여기와 열린 상단 바가 함께 따라간다.
 */
const overrides = new Map();
export const CHANNEL_SOCIAL_EVENT = 'cview:channel-social';

/** 팔로우·즐겨찾기가 바뀌었다고 알린다 — { following } 또는 { favorite } 중 바뀐 것만. */
export function channelSocialChanged(channelId, change) {
  if (!channelId) return;
  overrides.set(channelId, { ...(overrides.get(channelId) || {}), ...change });
  window.dispatchEvent(new CustomEvent(CHANNEL_SOCIAL_EVENT, { detail: { channelId, ...change } }));
}

/** 로그아웃·다른 계정 — 팔로우 덮어쓰기는 그 계정 것이라 버린다(즐겨찾기는 이 기기 저장이라 남긴다). */
export function resetFollowOverrides() {
  for (const [id, o] of overrides) {
    if (!('following' in o)) continue;
    const { following, ...rest } = o;
    if (Object.keys(rest).length) overrides.set(id, rest); else overrides.delete(id);
  }
}

/** 라이브 주소 — 링크 복사·치지직에서 열기. 순수 함수. */
export function liveUrl(channelId) {
  return `https://chzzk.naver.com/live/${encodeURIComponent(channelId)}`;
}

/**
 * 채널의 팔로우·즐겨찾기 상태. 팔로우는 로그인했을 때만(아니면 null — 단추를 숨긴다).
 * 둘 다 캐시된 목록이라 시청을 시작할 때 한 번 불러도 서버를 더 두드리지 않는다.
 */
export async function loadChannelSocial(channelId, loggedIn) {
  const [follows, favs] = await Promise.allSettled([
    loggedIn ? api.followingChannelsAll(false) : Promise.resolve(null),
    api.favoritesList(),
  ]);
  const list = follows.status === 'fulfilled' ? follows.value : null;
  const o = overrides.get(channelId) || {};
  let following = Array.isArray(list) ? list.some((c) => c.channelId === channelId) : null;
  if (following !== null && typeof o.following === 'boolean') following = o.following;
  let favorite = favs.status === 'fulfilled' && Array.isArray(favs.value) ? favs.value.some((c) => c.channelId === channelId) : false;
  if (typeof o.favorite === 'boolean') favorite = o.favorite;
  return { following, favorite };
}

/** 팔로우 켜기·끄기 — 성공하면 새 값, 실패하면 null(알림은 여기서 띄운다). */
export async function setFollow(channelId, name, on) {
  try {
    await api.channelFollow(channelId, on);
    toast(on ? `${name || '채널'}을(를) 팔로우했습니다` : `${name || '채널'} 팔로우를 끊었습니다`, 1500);
    channelSocialChanged(channelId, { following: on });
    return on;
  } catch (e) {
    toast(e.message || '팔로우를 바꾸지 못했습니다');
    return null;
  }
}

/** 즐겨찾기 켜기·끄기 — 사이드바·홈 목록도 다시 읽게 알린다(app.js 가 CHANNEL_SOCIAL_EVENT 를 받는다). */
export async function setFavorite(channelId, name, image, on) {
  try {
    await api.favoriteSet(channelId, on, name || '', image || null);
    toast(on ? '즐겨찾기에 추가했습니다' : '즐겨찾기에서 뺐습니다', 1500);
    channelSocialChanged(channelId, { favorite: on }); // app.js 가 받아 사이드바·홈 즐겨찾기 순서를 다시 읽는다
    return on;
  } catch (e) {
    toast(e.message);
    return null;
  }
}

export function copyLiveLink(channelId) {
  const url = liveUrl(channelId);
  const done = () => toast('링크를 복사했습니다', 1200);
  const fail = () => toast('링크를 복사하지 못했습니다');
  try { navigator.clipboard.writeText(url).then(done, fail); } catch { fail(); }
}

export function openOnChzzk(channelId) {
  api.openExternal(liveUrl(channelId)).catch((e) => toast(e.message));
}
