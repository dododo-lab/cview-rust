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

// 채팅 연결·해제 요청 순번 — Tauri 비동기 명령은 보낸 순서대로 돈다는 보장이 없어, 패인을 바꾸며 보낸
// 해제→연결이 뒤집혀 돌면 방금 연 채팅이 끊겼다. 요청마다 늘어나는 순번을 붙이면 Rust 가 늦게 온 오래된 요청을 버린다.
// 시각 기반이라 창이 여럿이거나 창을 다시 불러와도 나중 요청이 더 크다.
let chatOp = 0;
function nextChatOp() {
  chatOp = Math.max(chatOp + 1, Date.now() * 1000);
  return chatOp;
}

/** 이벤트 구독. 해제 함수를 돌려준다. (Tauri 2 의 전역 listen 은 다른 창으로 보낸 emit_to 도 받는다.) */
export async function listen(event, handler) {
  if (!isTauri) return () => {};
  return tauri.event.listen(event, (e) => {
    // 디버그 로그 수집기가 켜져 있을 때만 있는 계수기(IPC 이벤트 수).
    if (globalThis.__cviewPerf) globalThis.__cviewPerf.ipc += 1;
    handler(e.payload);
  });
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
  networkPresetChanged: 'settings://network-preset',
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
  // live — 라이브 단독 보기에 담는 추가(라이브 단독 보기 상한을 따른다)
  multiLiveAdd: (channelId, live = false) => invoke('multilive_add', { channelId, live }),
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
  channelPosts: (channelId, page = 0) => invoke('channel_posts', { channelId, page }),
  popularClips: (filter, order) => invoke('popular_clips', { filter, order }),
  searchVideos: (keyword, offset = 0, size = 24) => invoke('search_videos', { keyword, offset, size }),
  clipPlay: (clipUid) => invoke('clip_play', { clipUid }),
  vodDetail: (videoNo) => invoke('vod_detail', { videoNo }),
  // 썸네일 호버 미리보기 주소 — { url, format: 'hls'|'mp4', startAt, live } 또는 null(미리보기 없음)
  previewSource: (kind, id) => invoke('preview_source', { kind, id: String(id) }),
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
  chatConnect: (chatChannelId, channelId, session = null) => invoke('chat_connect', { chatChannelId, channelId, session, op: nextChatOp() }),
  chatDisconnect: (session = null) => invoke('chat_disconnect', { session, op: nextChatOp() }),
  chatSend: (text, session = null) => invoke('chat_send', { text, session }),

  appInfo: () => invoke('app_info'),
  perfLog: (kind, data) => invoke('perf_log', { kind, data }),
  perfLogStatus: () => invoke('perf_log_status'),
  imageCacheStatus: () => invoke('image_cache_status'),
  imageCacheClear: () => invoke('image_cache_clear'),
  updateCheck: () => invoke('update_check'),
  updateInstall: () => invoke('update_install'),
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
