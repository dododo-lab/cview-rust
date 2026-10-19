// Tauri IPC 래퍼 — Rust `commands` 계층과 1:1.
// `withGlobalTauri` 로 주입된 window.__TAURI__ 를 쓴다(번들러 없음).
// Tauri 밖(일반 브라우저로 열기)에서는 명령이 실패하고 이벤트는 조용히 무시된다.

const tauri = window.__TAURI__;

export const isTauri = Boolean(tauri && tauri.core);

/** Rust 명령 호출. 실패하면 사용자용 문구(문자열)로 reject. */
export async function invoke(command, args = {}) {
  if (!isTauri) {
    throw new Error('CView 앱 안에서 실행해야 합니다');
  }
  try {
    return await tauri.core.invoke(command, args);
  } catch (e) {
    throw new Error(typeof e === 'string' ? e : (e && e.message) || String(e));
  }
}

/** 이벤트 구독. 해제 함수를 돌려준다. */
export async function listen(event, handler) {
  if (!isTauri) return () => {};
  return tauri.event.listen(event, (e) => handler(e.payload));
}

export const Events = Object.freeze({
  authChanged: 'auth://changed',
  loginFailed: 'auth://login-failed',
  chat: 'chat://event',
  playerCommand: 'player://command',
  playerEvent: 'player://event',
  multiLiveCommand: 'multilive://command',
  multiLiveState: 'multilive://state',
});

export const api = {
  authStatus: () => invoke('auth_status'),
  login: () => invoke('auth_login'),
  logout: () => invoke('auth_logout'),

  followingLives: (refresh = false) => invoke('following_lives', { refresh }),
  popularLives: (size = 50, refresh = false) => invoke('popular_lives', { size, refresh }),
  search: (keyword) => invoke('search', { keyword }),
  channelInfo: (channelId) => invoke('channel_info', { channelId }),
  liveDetail: (channelId, refresh = false) => invoke('live_detail', { channelId, refresh }),

  playerStart: (channelId) => invoke('player_start', { channelId }),
  playerStop: () => invoke('player_stop'),
  playerSetVolume: (volume) => invoke('player_set_volume', { volume }),
  playerMessage: (message) => invoke('player_message', { message }),

  multiLiveState: () => invoke('multilive_state'),
  multiLiveAdd: (channelId) => invoke('multilive_add', { channelId }),
  multiLiveRemove: (sessionId) => invoke('multilive_remove', { sessionId }),
  multiLiveSelect: (sessionId) => invoke('multilive_select', { sessionId }),
  multiLiveSetLayout: (layout) => invoke('multilive_set_layout', { layout }),
  multiLiveStageSize: (width, height) => invoke('multilive_stage_size', { width, height }),
  multiLiveStopAll: () => invoke('multilive_stop_all'),
  multiLiveSetFocusLayout: (direction, ratio) => invoke('multilive_set_focus_layout', { direction, ratio }),
  multiLiveSetAudioSuspended: (suspended) => invoke('multilive_set_audio_suspended', { suspended }),
  multiLivePlayerMessage: (slot, message) => invoke('multilive_player_message', { slot, message }),

  chatConnect: (chatChannelId, channelId) => invoke('chat_connect', { chatChannelId, channelId }),
  chatDisconnect: () => invoke('chat_disconnect'),
  chatSend: (text) => invoke('chat_send', { text }),

  settingsGet: () => invoke('settings_get'),
  settingsUpdate: (domain, patch) => invoke('settings_update', { domain, patch }),
};
