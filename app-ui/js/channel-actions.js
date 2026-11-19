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
  return {
    following: Array.isArray(list) ? list.some((c) => c.channelId === channelId) : null,
    favorite: favs.status === 'fulfilled' && Array.isArray(favs.value) ? favs.value.some((c) => c.channelId === channelId) : false,
  };
}

/** 팔로우 켜기·끄기 — 성공하면 새 값, 실패하면 null(알림은 여기서 띄운다). */
export async function setFollow(channelId, name, on) {
  try {
    await api.channelFollow(channelId, on);
    toast(on ? `${name || '채널'}을(를) 팔로우했습니다` : `${name || '채널'} 팔로우를 끊었습니다`, 1500);
    window.dispatchEvent(new CustomEvent('cview:channel-social', { detail: { channelId, following: on } }));
    return on;
  } catch (e) {
    toast(e.message || '팔로우를 바꾸지 못했습니다');
    return null;
  }
}

/** 즐겨찾기 켜기·끄기 — 사이드바 목록도 다시 읽게 알린다. */
export async function setFavorite(channelId, name, image, on) {
  try {
    await api.favoriteSet(channelId, on, name || '', image || null);
    toast(on ? '즐겨찾기에 추가했습니다' : '즐겨찾기에서 뺐습니다', 1500);
    window.dispatchEvent(new CustomEvent('cview:channel-social', { detail: { channelId, favorite: on } }));
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
