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

/** 이벤트 구독. 해제 함수를 돌려준다. (Tauri 2 의 전역 listen 은 다른 창으로 보낸 emit_to 도 받는다.) */
export async function listen(event, handler) {
  if (!isTauri) return () => {};
  return tauri.event.listen(event, (e) => handler(e.payload));
}

/** 이 창으로 보낸 이벤트만 구독 — 플레이어·멀티라이브 명령처럼 창마다 따로인 것(보조 창). */
export async function listenHere(event, handler) {
  if (!isTauri) return () => {};
  const here = tauri.webviewWindow && tauri.webviewWindow.getCurrentWebviewWindow
    ? tauri.webviewWindow.getCurrentWebviewWindow() : null;
  if (!here) return tauri.event.listen(event, (e) => handler(e.payload));
  return here.listen(event, (e) => handler(e.payload));
}

export const Events = Object.freeze({
  authChanged: 'auth://changed',
  loginFailed: 'auth://login-failed',
  chat: 'chat://event',
  playerCommand: 'player://command',
  playerEvent: 'player://event',
  multiLiveCommand: 'multilive://command',
  multiLiveState: 'multilive://state',
  downloadUpdate: 'download://update',
  deepLink: 'deeplink://open',
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
  multiLiveWatch: (sessionId) => invoke('multilive_watch', { sessionId }),
  multiLivePark: (parked) => invoke('multilive_park', { parked }),
  multiLiveSetLayout: (layout) => invoke('multilive_set_layout', { layout }),
  multiLiveStageSize: (width, height) => invoke('multilive_stage_size', { width, height }),
  multiLiveStopAll: () => invoke('multilive_stop_all'),
  multiLiveSetFocusLayout: (direction, ratio) => invoke('multilive_set_focus_layout', { direction, ratio }),
  multiLiveSetAudioSuspended: (suspended) => invoke('multilive_set_audio_suspended', { suspended }),
  multiLiveReorder: (draggedId, targetId) => invoke('multilive_reorder', { draggedId, targetId }),
  multiLiveAudio: (sessionId, toggleMode) => invoke('multilive_audio', { sessionId, toggleMode }),
  multiLiveSetRatios: (horizontal, vertical) => invoke('multilive_set_ratios', { horizontal, vertical }),
  insightsOverview: () => invoke('insights_overview'),
  dataHealth: () => invoke('data_health'),
  dataBackupNow: () => invoke('data_backup_now'),
  dataCleanup: (kind) => invoke('data_cleanup', { kind }),
  presetList: () => invoke('preset_list'),
  presetSave: (name, channels) => invoke('preset_save', { name, channels }),
  presetDelete: (id) => invoke('preset_delete', { id }),
  multiLivePlayerMessage: (slot, message) => invoke('multilive_player_message', { slot, message }),

  vodPlay: (videoNo) => invoke('vod_play', { videoNo }),
  vodProgressSave: (videoNo, position, duration, title, channelName, thumbnailUrl) => invoke('vod_progress_save', { videoNo, position, duration, title, channelName, thumbnailUrl }),
  vodProgressRemove: (videoNo) => invoke('vod_progress_remove', { videoNo }),
  vodContinueList: (limit = 20) => invoke('vod_continue_list', { limit }),
  vodWatchLaterSet: (videoNo, on, title, channelName, thumbnailUrl, duration) => invoke('vod_watch_later_set', { videoNo, on, title, channelName, thumbnailUrl, duration }),
  vodWatchLaterList: () => invoke('vod_watch_later_list'),
  followingChannelsAll: (refresh = false) => invoke('following_channels_all', { refresh }),
  followingVods: (channels = 24, perChannel = 3) => invoke('following_vods', { channels, perChannel }),
  channelDetail: (channelId) => invoke('channel_detail', { channelId }),
  channelVods: (channelId, page = 0, size = 24) => invoke('channel_vods', { channelId, page, size }),
  channelClips: (channelId, page = 0, size = 24) => invoke('channel_clips', { channelId, page, size }),
  popularClips: (filter, order) => invoke('popular_clips', { filter, order }),
  searchVideos: (keyword, offset = 0, size = 24) => invoke('search_videos', { keyword, offset, size }),
  clipPlay: (clipUid) => invoke('clip_play', { clipUid }),
  vodDetail: (videoNo) => invoke('vod_detail', { videoNo }),
  channelFollow: (channelId, follow) => invoke('channel_follow', { channelId, follow }),
  favoriteSet: (channelId, favorite, channelName, channelImageUrl) => invoke('favorite_set', { channelId, favorite, channelName, channelImageUrl }),
  favoritesList: () => invoke('favorites_list'),
  recentChannels: (limit = 20) => invoke('recent_channels', { limit }),
  recentChannelsClear: () => invoke('recent_channels_clear'),
  memoSet: (channelId, memo) => invoke('memo_set', { channelId, memo }),
  bookmarkAdd: (channelId, note) => invoke('bookmark_add', { channelId, note }),
  bookmarksList: () => invoke('bookmarks_list'),
  bookmarkDelete: (id) => invoke('bookmark_delete', { id }),
  saveFile: (kind, fileName, dataBase64, text) => invoke('save_file', { kind, fileName, dataBase64, text }),
  revealPath: (path) => invoke('reveal_path', { path }),
  openPath: (path) => invoke('open_path', { path }),
  openNotificationSettings: () => invoke('open_notification_settings'),
  downloadList: () => invoke('download_list'),
  downloadStart: (kind, target, title, channelName) => invoke('download_start', { kind, target, title, channelName }),
  downloadCancel: (id) => invoke('download_cancel', { id }),
  downloadClear: (ids) => invoke('download_clear', { ids }),
  openExternal: (url) => invoke('open_external', { url }),

  chatEmoticons: (channelId) => invoke('chat_emoticons', { channelId }),
  chatConnect: (chatChannelId, channelId, session = null) => invoke('chat_connect', { chatChannelId, channelId, session }),
  chatDisconnect: (session = null) => invoke('chat_disconnect', { session }),
  chatSend: (text, session = null) => invoke('chat_send', { text, session }),

  appInfo: () => invoke('app_info'),
  updateCheck: () => invoke('update_check'),
  windowOpen: (kind, params) => invoke('window_open', { kind, params: params || null }),
  windowCloseSelf: () => invoke('window_close_self'),
  oauthStatus: () => invoke('oauth_status'),
  oauthSetCredentials: (clientId, clientSecret) => invoke('oauth_set_credentials', { clientId, clientSecret }),
  oauthClearCredentials: () => invoke('oauth_clear_credentials'),
  oauthConnect: () => invoke('oauth_connect'),
  notifySend: (title, body) => invoke('notify_send', { title, body }),
  notifyPermission: (request) => invoke('notify_permission', { request }),
  deeplinkTakePending: () => invoke('deeplink_take_pending'),
  multiLiveRetry: (sessionId = null) => invoke('multilive_retry', { sessionId }),
  settingsExport: () => invoke('settings_export'),
  settingsImport: (json) => invoke('settings_import', { json }),
  settingsReset: (domain) => invoke('settings_reset', { domain }),
  networkPreset: (preset) => invoke('network_preset', { preset }),
  playerRecordStart: () => invoke('player_record_start'),
  playerRecordStop: () => invoke('player_record_stop'),
  playerRecordStatus: () => invoke('player_record_status'),
  equalizerPresets: () => invoke('equalizer_presets'),
  settingsGet: () => invoke('settings_get'),
  settingsUpdate: (domain, patch) => invoke('settings_update', { domain, patch }),
};
