// CView 메인 — 사이드바 라우팅, 인증 상태, 시청 화면 연결.
import { CHANNEL_SOCIAL_EVENT, channelSocialChanged, resetFollowOverrides } from './channel-actions.js';
import { api, listen, listenHere, Events, isTauri } from './api.js';
import { toast, icons } from './dom.js';
import { showShortcuts, showAbout, showUpdateCheck } from './app-sheets.js';
import { installUpdateNotifier } from './update-notify.js';
import { installImageFade, installSegmentGlider, refreshSegmentGliders, installMicroMotion } from './motion.js';
import { installLiveMotion } from './live-motion.js';
import { installHoverPreview } from './hover-preview.js';
import { setAdAudioHooks } from './ads.js';
import { noteConnectorLimit } from './live-connector.js';
import { bindCardOpen, recentChannels, fallbackAvatar, RECENT_CLEARED_EVENT } from './cards.js';
import { FollowingPage, PopularPage, SearchPage } from './pages.js';
import { SettingsPage } from './settings.js';
import { Viewer } from './viewer.js';
import { togglePipFor } from './viewer-controls.js';
import { HomePage } from './home.js';
import { Sidebar } from './sidebar.js';
import { buildSidebarList, sidebarListKind, sidebarListNeeds } from './sidebar-lists.js';
import { setTheme, syncTheme, resolvedTheme } from './theme.js';
import { loadChatPrefs, setMyNickname, chatPrefs, updateChatPrefs } from './chat-prefs.js';
import { MultiLivePage, multiLiveIcon, bindMultiLiveAddButtons } from './multilive.js';
import { syncHibernation, hibernateHiddenNow } from './page-hibernate.js';
import { LivePage, liveIcon } from './live.js';
import { LIVE_MODES, liveModeSwitch, bindLiveModeSwitch } from './live-mode.js';
import { decoratePageHeader } from './page-header.js';
import { ChannelPage } from './channel.js';
import { MediaPage } from './media.js';
import { LibraryPage, libraryIcon } from './library.js';
import { StatsPage, statsIcon } from './stats.js';
import { loadShortcuts } from './shortcuts.js';
import { InsightPanel } from './insight-panel.js';
import { installA11y } from './a11y.js';
import { splashStep, hideSplash, showWelcome } from './onboarding.js';
import { needsNotice, showNotice, showNoticeReview, showLicenses, openProjectLink, noticeOpen } from './notice.js';
import { HUBS, saveWorkspace, loadWorkspace, installSidebarExtras, hubFlash } from './workspace.js';
import { CategoriesPage, categoryIcon } from './categories.js';
import { CommandPalette } from './palette.js';
import { ChatOnlyPage } from './chat-only.js';
import { MultiChatPage, multiChatIcon } from './multichat.js';
import { installPrefsEffects } from './prefs-effects.js';
import { installBackgroundSaver, setMenuHidden } from './background-saver.js';
import { installPerfCollector } from './perf-collector.js';
import { showMenu, refreshChatAuth, reconnectFailedChats } from './chat.js';
import { installUiHealth } from './ui-health.js';
import { installSidebarUsage } from './sidebar-usage.js';
import { closeUserPop } from './chat-user-pop.js';
import { kbd, IS_WINDOWS, noteSaveDirs } from './platform.js';
import { installWindowControls } from './window-controls.js';
import { withRefreshVeil, cancelRefreshVeil } from './refresh-veil.js';
import { initSharpness, flushSharpnessNotice } from './sharpness.js';
import { installTrayState, syncTray } from './tray-state.js';

// 사이드바 — Swift `AppRouter.SidebarItem` 라벨. `sidebar: false` 는 사이드바에 없고 다른 메뉴 속에서 여는 화면
// (멀티라이브는 라이브 메뉴의 한 보기 — 머리줄 전환 버튼으로 오간다). ⌘K 팔레트에는 그대로 나온다.
const ROUTES = [
  { id: 'home', title: '홈', icon: icons.home },
  { id: 'following', title: '팔로잉', icon: icons.following },
  { id: 'popular', title: '방송&클립', icon: icons.popular },
  { id: 'categories', title: '카테고리', icon: categoryIcon },
  { id: 'live', title: '라이브', icon: liveIcon },
  { id: 'multilive', title: '멀티라이브', icon: multiLiveIcon, sidebar: false },
  { id: 'multichat', title: '멀티채팅', icon: multiChatIcon },
  { id: 'library', title: '보관함', icon: libraryIcon },
  { id: 'stats', title: '시청 리포트', icon: statsIcon, sidebar: false },
  { id: 'search', title: '검색', icon: icons.search, section: '탐색' },
  { id: 'settings', title: '설정', icon: icons.settings, section: '앱' },
];

/** 화면별 메뉴 단축키(menu.rs GO 와 같게) — 팔레트 힌트. */
const ROUTE_KEYS = { home: '⌘1', following: '⌘2', popular: '⌘3', categories: '⌘4', live: '⌘5', multichat: '⌘6', library: '⌘7', multilive: '⌘8', search: '⌘F', settings: '⌘,' };

/** 보조 창 종류 — player · chat · multichat · stats (없으면 메인 창). */
const AUX = new URLSearchParams(location.search).get('w');

let auth = { loggedIn: false, state: 'loggedOut' };
const isLoggedIn = () => Boolean(auth && auth.loggedIn);

const pages = {};
let current = null;
let viewer = null;
let sidebar = null;
let insights = null;

function renderNav() {
  sidebar = new Sidebar(document.querySelector('.sidebar'), { routes: ROUTES, onNavigate: (route) => (route === 'live' ? openLiveMenu() : navigate(route)) });
  closeFloatingOnSheet();
}

// 사이드바 아래 채널 목록 — 메뉴마다 다른 목록(sidebar-lists.js). 팔로잉은 Rust FollowingStore 가 60초 안의 결과를
// 재사용하므로 주기 호출이 가볍다. 인기·최근 본 채널은 그 목록을 쓰는 메뉴에 들어갈 때만 불러온다.
const SIDEBAR_FOLLOWING_MS = 120000;
// 갱신이 실패했으면(네트워크 끊김 등) 2분을 다 기다리지 않고 이 간격으로 다시 해 본다.
const SIDEBAR_FOLLOWING_RETRY_MS = 20000;
const SIDEBAR_EXTRA_MS = 60000;
let followingTimer = 0;
const sideData = { following: null, favorites: new Set(), popular: null, popularAt: 0, recent: null, recentAt: 0 };
let sideListSeq = 0;
function sidebarRoute() {
  return LIVE_MODES.includes(current) ? 'live' : current;
}
function paintSidebarList() {
  const multichatIds = new Set(((pages.multichat && pages.multichat.channels) || []).map((c) => c.channelId));
  sidebar.setList(buildSidebarList(sidebarRoute(), { ...sideData, multichatIds }));
}
/** 지금 메뉴의 목록을 바로 그리고, 그 목록에 필요한 인기·최근 기록이 없거나 오래됐으면 불러와 다시 그린다. */
async function refreshSidebarList() {
  paintSidebarList();
  const seq = ++sideListSeq;
  const need = sidebarListNeeds(sidebarListKind(sidebarRoute()));
  const now = Date.now();
  const jobs = [];
  if (need.popular && now - sideData.popularAt > SIDEBAR_EXTRA_MS) {
    jobs.push(api.popularLives(50, false).then((l) => { sideData.popular = l; sideData.popularAt = Date.now(); }).catch(() => {}));
  }
  // 최근 본 채널은 방금 본 채널이 바로 올라오게 메뉴에 들어올 때마다 다시 읽는다(로컬 DB라 가볍다).
  if (need.recent) {
    jobs.push(api.recentChannels(20).then((l) => { sideData.recent = l; sideData.recentAt = Date.now(); }).catch(() => {
      if (!sideData.recent) sideData.recent = recentChannels().map((r) => ({ ...r, lastWatched: r.at }));
    }));
  }
  if (!jobs.length) return;
  await Promise.all(jobs);
  if (seq === sideListSeq) paintSidebarList();
}
let sideFollowSeq = 0;
async function refreshSidebarFollowing() {
  clearTimeout(followingTimer);
  const seq = ++sideFollowSeq;
  // 보조 창(?w=…)은 사이드바를 숨긴다(app.css :root[data-window] .sidebar) — 2분마다 팔로잉 목록을 받아 그릴 곳이 없다.
  if (AUX) return;
  let failed = false;
  if (!isLoggedIn()) {
    sideData.following = null;
    sidebar.setLiveCount(null);
  } else {
    try {
      // 즐겨찾기 채널을 위로 (DAT-02) — 홈 등 기본 목록
      const [lives, favs] = await Promise.all([api.followingLives(false), api.favoritesList().catch(() => [])]);
      // 그 사이 로그아웃했거나 더 새 갱신이 시작됐으면 버린다 — 예전엔 로그아웃 직후 늦게 온 이전 계정 목록이
      // 로그아웃 상태 사이드바에 다시 그려져 다음 주기(2분)까지 남았다.
      if (seq !== sideFollowSeq || !isLoggedIn()) return;
      sideData.following = lives;
      sideData.favorites = new Set(favs.map((f) => f.channelId));
    } catch {
      // 한 번 실패했다고(잠깐의 네트워크 끊김) 보던 목록을 지우지 않는다 — 예전엔 사이드바 팔로잉 목록과 개수가
      // 사라진 채 다음 주기(2분)까지, 네트워크가 돌아온 뒤에도 비어 있었다. 받은 적이 없을 때만 비운다.
      failed = true;
    }
    sidebar.setLiveCount(sideData.following ? sideData.following.length : null);
    // 홈 맨 위 "방송 중인 팔로잉"도 같은 주기로 고친다 — 바뀐 칸만(홈 전체를 다시 그리지 않아 깜박이지 않는다)
    if (!failed && sideData.following && pages.home) {
      try { pages.home.updateLiveFollowing(sideData.following, sideData.favorites); } catch (e) { console.warn('홈 팔로잉 갱신 실패', e); }
    }
  }
  paintSidebarList();
  // 여러 호출이 겹쳐도 주기 타이머는 하나만(예전엔 겹친 호출마다 타이머가 늘었다).
  clearTimeout(followingTimer);
  followingTimer = setTimeout(refreshSidebarFollowing, failed ? SIDEBAR_FOLLOWING_RETRY_MS : SIDEBAR_FOLLOWING_MS);
}

// 단독 라이브와 멀티라이브는 동시에 소리를 내지 않는다 — 마지막으로 연 쪽이 소리를 가진다.
// 멀티라이브 매니저(Rust)는 메인 창 것 하나뿐이다. 보조 창(플레이어·채팅·멀티채팅·통계)이 여기에 음소거·접기를
// 보내면 메인 창은 자기 소리 주인을 그대로 들고 있어 창을 닫아도 되돌리지 않았다(메인 멀티라이브가 계속 무음·정지).
const mlAudioSuspended = (on) => { if (!AUX) api.multiLiveSetAudioSuspended(on).catch(() => {}); };
let audioOwner = null;
function setAudioOwner(owner) {
  if (owner === audioOwner) return;
  audioOwner = owner;
  if (owner === 'live') {
    viewer.suspendAudio(false);
    mlAudioSuspended(true);
  } else if (owner === 'multilive') {
    viewer.suspendAudio(true);
    mlAudioSuspended(false);
  }
}

// 광고가 나오는 동안은 보던 방송 소리를 잠시 끄고, 끝나면 소리 주인 규칙대로 되돌린다.
// 광고가 나오는 영상 영역(target)의 소리는 ads.js 가 그 플레이어에서 따로 막는다 — 시청 화면 광고면 시청 화면은 건드리지 않는다.
setAdAudioHooks({
  hold(target) {
    if (target !== 'viewer') viewer.suspendAudio(true);
    mlAudioSuspended(true);
  },
  release(target) {
    if (target !== 'viewer') viewer.suspendAudio(audioOwner === 'multilive');
    mlAudioSuspended(audioOwner === 'live');
  },
});

// ── 라이브 ↔ 멀티라이브 연동 (원본 시청/멀티 허브 모드) ──────────────
// - 멀티라이브에 있는 채널을 라이브로 보면 같은 세션(플레이어·채팅 소켓)을 무대에 올린다 — 다시 연결 없음.
//   무대가 있는 동안 매니저가 나머지 패인의 로딩을 멈추고, 멀티라이브로 돌아가면 배치 그대로 되살린다.
// - 멀티라이브에 없는 채널을 라이브 시청 화면(별도 플레이어)으로 보는 동안 멀티라이브 패인은 접어 둔다
//   (일시정지 → 2.5초 뒤 로딩 정지). 같은 채널이 두 곳에서 동시에 재생되지 않는다.
const isWatching = () => Boolean(pages.multilive && pages.multilive.watchSlot != null);
let multiLiveParked = false;
function setMultiLiveParked(on) {
  if (AUX) return; // 메인 창 멀티라이브만 접는다(위 mlAudioSuspended 와 같은 까닭)
  const hasSessions = Boolean(pages.multilive && (pages.multilive.state.sessions || []).length);
  if (on && !hasSessions) return;
  if (on === multiLiveParked) return;
  multiLiveParked = on;
  api.multiLivePark(on).catch(() => {});
}
// 창 가림 — 메인 창이 가려지면(최소화·다른 창 뒤·다른 Space, `document.hidden`) 선택·PiP·들리는 패인 밖의
// 멀티라이브 패인 디코드를 멈추고, 다시 보이면 바로 되돌린다(원본 창 가림 디코드 정지). 보이지 않는 패인을
// 계속 디코드하면 WebKit 웹 콘텐츠 프로세스 메모리가 가려진 시간만큼 쌓였다(5패인 27시간 190MB → 1.1~1.4GB).
// 창은 보여도 멀티라이브 화면을 떠나 다른 메뉴(홈·팔로잉·설정…)를 보는 동안도 같다 — 예전엔 보이지 않는 패인을 그대로
// 디코드해, 4패인을 뒤에 둔 채 홈을 보면 멀티라이브 화면을 볼 때와 거의 같은 CPU 를 썼다(사용률 측정 32.5 → 14.8,
// 코어 하나 = 100, tools/usage-bench). 소리는 이어진다 — 선택·들리는 패인은 계속 재생한다(화질 상한만, setOffscreen).
// 잠깐 다른 창·메뉴로 넘어갔다 오는 정도로는 멈추지 않게 조금 기다린다. 보조 창에는 멀티라이브가 없다.
const OCCLUDE_DELAY_MS = 5000;
// null — 아직 보낸 적 없음. 페이지를 다시 불러오면(웹 콘텐츠 복구 등) 매니저에 남은 가림 상태를 첫 값으로 덮는다.
let windowOccluded = null;
let occludeTimer = 0;
function sendWindowOccluded(on) {
  if (on === windowOccluded) return;
  windowOccluded = on;
  // 계속 재생하는 패인(선택·들리는)은 보이지 않는 동안 화질 상한 — 나머지는 아래 명령으로 Rust 가 멈춘다.
  if (pages.multilive) pages.multilive.setOffscreen(on);
  api.multiLiveSetWindowOccluded(on).catch(() => { windowOccluded = null; });
}
/**
 * 멈출 멀티라이브 패인이 화면 밖에 있나 — 패인이 있고, 창이 가려졌거나 멀티라이브(단독 보기 무대 포함) 화면이 숨었다.
 * 패인이 없으면 보낼 까닭이 없다(빈 멀티라이브로 메뉴를 옮길 때마다 명령이 나가지 않게).
 */
function multiLiveOffscreen() {
  const ml = pages.multilive;
  if (!ml || !(ml.state.sessions || []).length) return false;
  return document.hidden || ml.root.hidden;
}
/**
 * 화면 밖이 되면 OCCLUDE_DELAY_MS 뒤에 가림을, 화면에 돌아오면 바로 풀기를 보낸다. 화면 밖인 동안 다시 불려도
 * (다른 메뉴로 옮기기·창 숨김 토글·멀티라이브 상태 알림) 기다리던 타이머를 새로 걸지 않는다 — 처음 떠난 때부터 잰다.
 */
function syncWindowOcclusion() {
  if (!multiLiveOffscreen()) {
    clearTimeout(occludeTimer);
    occludeTimer = 0;
    sendWindowOccluded(false);
    return;
  }
  if (windowOccluded === true || occludeTimer) return;
  occludeTimer = setTimeout(() => {
    occludeTimer = 0;
    if (multiLiveOffscreen()) sendWindowOccluded(true);
  }, OCCLUDE_DELAY_MS);
}
if (!AUX) {
  document.addEventListener('visibilitychange', syncWindowOcclusion);
  syncWindowOcclusion();
}
function multiLiveSessionFor(channelId) {
  return (pages.multilive.state.sessions || []).find((x) => x.channelId === channelId && x.slot != null) || null;
}
// 채널을 무대에 올리는 중(openChannel·enterWatch) — 그 사이 오는 상태 알림이 화면을 멀티라이브로 끌고 가지 않게.
let pendingWatch = 0;
async function enterWatch(sessionId, { push = true } = {}) {
  openSeq++; // 칩·패인으로 무대를 바꿔도 아직 추가 중인 앞선 열기가 나중에 무대를 빼앗지 않게
  pendingWatch++;
  try {
    if (viewer.isPlaying) await viewer.close({ silent: true });
    try {
      pages.multilive.render(await api.multiLiveWatch(sessionId));
    } catch (e) { toast(e.message); return; }
    syncLiveIndicators();
    await navigate('live', {}, { push });
  } finally { pendingWatch--; syncLiveIndicators(); }
}
async function exitWatch() {
  try { pages.multilive.render(await api.multiLiveWatch(null)); } catch { /* 앱 밖 */ }
  syncLiveIndicators();
}
/** 전체 화면 요소가 숨은 화면(roots 중 hidden) 안에 있으면 전체 화면을 끝낸다. 끝냈으면 true. */
function exitHiddenFullscreen(roots, doc = document) {
  const fs = doc.fullscreenElement;
  if (!fs || !doc.exitFullscreen) return false;
  if (!roots.some((r) => r && r.hidden && (r === fs || r.contains(fs)))) return false;
  doc.exitFullscreen().catch(() => {});
  return true;
}
/** 라이브 메뉴인데 목록(LivePage) 대신 무대·예전 시청 화면이 보이는 중 — 숨은 목록은 새로 불러오지 않는다. */
function liveListHidden() {
  return current === 'live' && Boolean((viewer && viewer.isPlaying) || isWatching());
}
/** 라이브 메뉴 단독 보기가 앞에 떠 있으면 그 도구(ml-watch.js) — 메뉴 동작·단축키가 예전 시청 화면 대신 이리로 간다. */
function activeWatch() {
  return current === 'live' && !viewer.isOpen && isWatching() ? pages.multilive.watch : null;
}
// 단독 보기를 닫은 뒤 보던 화면으로 돌아갈지 — 멀티라이브 상태 알림(남은 패인이 있으면 멀티라이브로)보다 먼저 본다.
/** 고지를 물어야 하는데 아직 동의하지 않았다(고지를 DOM 에 붙이기 전부터) — 트레이 동작 막기에 쓴다. */
let noticePending = false;
let watchCloseBack = false;
/**
 * 단독 보기 닫기(닫기 단추·Esc·⌘W) — 예전 시청 화면처럼 보던 화면(인기·카테고리 등)으로 돌아간다.
 * 둘러보다 연 채널은 멀티라이브에서도 뺀다. 멀티라이브에 넣어 둔 채널은 닫기 단추면 빼고,
 * Esc·⌘W 면 그대로 두고 멀티라이브 보기로 간다(배치를 잃지 않게). 닫기 단추는 머리줄에 남은 채널이 있으면 그리로.
 */
async function closeWatch({ remove = false } = {}) {
  const id = pages.multilive.state.watchId;
  if (!id) return;
  const transient = browsedWatch.has(id);
  if (!transient && !remove) { await navigate('multilive'); return; }
  browsedWatch.delete(id);
  // 닫기 단추 — 머리줄에 다른 채널이 남아 있으면 탭을 닫듯 옆 채널로 바꿔 보고 라이브 화면에 머문다.
  // 옆 채널을 먼저 무대에 올리고 뺀다(무대가 비는 순간 멀티라이브로 끌려가지 않게).
  if (remove) {
    const list = (pages.multilive.state.sessions || []).filter((x) => x.slot != null);
    const at = list.findIndex((x) => x.id === id);
    const next = list[at + 1] || list[at - 1];
    if (at >= 0 && next) {
      await enterWatch(next.id, { push: false });
      await pages.multilive.run(api.multiLiveRemove(id));
      return;
    }
  }
  watchCloseBack = transient;
  await pages.multilive.run(api.multiLiveRemove(id));
  // 매니저는 지운 세션을 무대로 기억한 채 남는다 — 그대로 두면 다음 패인 선택이 그 패인을 무대로 올렸다.
  await exitWatch();
  // 상태 알림이 오지 않았거나 늦으면 여기서 마무리한다.
  setTimeout(() => {
    if (!watchCloseBack) return;
    watchCloseBack = false;
    if (current === 'live' && !viewer.isPlaying && !isWatching()) goBack();
  }, 300);
}
/**
 * 사이드바 라이브 메뉴 — 단독 재생 중이면 점, 멀티라이브 패인이 있으면 그 수.
 * 둘러보다 연 채널(임시 보기)은 멀티라이브에 넣은 패인이 아니라 세지 않는다.
 */
function syncLiveIndicators() {
  const playing = Boolean(viewer && viewer.isPlaying) || isWatching();
  const sessions = (pages.multilive && pages.multilive.state.sessions) || [];
  const count = sessions.filter((x) => !browsedWatch.has(x.id)).length;
  sidebar.setPlaying('live', playing);
  if (pendingWatch) return; // 여는 중 — 막 넣은 패인이 임시 보기로 정해지기 전에 숫자가 튀지 않게(끝나면 다시 맞춘다)
  sidebar.setBadge('live', count);
  sidebar.setNavLabel('live', playing || count ? ['라이브', playing && '재생 중', count && `멀티라이브 ${count}개`].filter(Boolean).join(', ') : '');
}

// 사이드바 라이브 메뉴를 누르면 마지막으로 본 보기(라이브·멀티라이브)로 간다.
let lastLiveMode = 'live';
/** 라이브 메뉴로 들어가는 모든 길(사이드바·메뉴 막대·팔레트·허브 링크)이 같은 곳으로 — 마지막으로 본 보기. */
function openLiveMenu() {
  return navigate(lastLiveMode);
}

/**
 * 머리줄 [라이브 | 멀티라이브] 전환.
 * 멀티라이브 → 라이브: 포커스 레이아웃이면 메인 패인, 아니면 선택한 패인의 채널을 같은 세션 그대로(다시 연결 없이)
 * 라이브 무대에 올린다. 나머지 채널은 라이브 머리줄의 채널 고르기로 바꿔 본다.
 * 멀티라이브가 비어 있으면 예전처럼 라이브 화면(재생 중인 방송·이어 볼 채널)으로 간다.
 * 예전 시청 화면에서 보던 방송이 있으면 그 화면으로만 돌아간다(멀티라이브 패인으로 바꾸지 않는다).
 * 두 보기는 한 메뉴라 서로를 뒤로 가기 기록에 쌓지 않는다(⌘[ 는 라이브 메뉴 전의 화면으로).
 */
function switchLiveMode(mode) {
  if (mode === 'live' && current === 'multilive' && !isWatching() && !viewer.isPlaying) {
    const st = pages.multilive.state;
    const sessions = (st.sessions || []).filter((x) => x.slot != null);
    const mainId = (st.layout === 'focus' && st.focusMainId) || st.selectedId;
    const pick = sessions.find((x) => x.id === mainId) || sessions[0];
    if (pick) return enterWatch(pick.id, { push: false });
  }
  return navigate(mode, {}, { push: false });
}

// 내비게이션 스택 — 원본 AppRouter. 화면마다 파라미터(채널 ID 등)를 가진다.
const backStack = [];
let currentParams = {};
const BACK_MAX = 50;

async function navigate(route, params = {}, { push = true, back = false } = {}) {
  // 예전 통계 진입점(메뉴 막대·팔레트·작업 공간)은 홈 리포트로 모은다. 보조 통계 창은 그대로 지원한다.
  if (route === 'stats' && AUX !== 'stats') { route = 'home'; params = { ...params, view: 'report' }; }
  // 인기 클립은 보관함에서 방송&클립 › 클립 탭으로 옮겼다 — 예전 경로(저장된 작업 공간·뒤로 가기 기록·옛 진입점)도 그리로.
  if (route === 'library' && params && params.tab === 'clips') { route = 'popular'; params = { tab: 'clips' }; }
  if (!pages[route]) return;
  cancelRefreshVeil(); // 새로고침 덮개는 그 화면 것 — 옮기면 바로 걷는다
  // 같은 화면을 같은 값으로 다시 연 것이면 기록을 쌓지 않는다. 화면이 기록에 더 남기는 값(채널 화면의 탭 — routeParams)이
  // 있어도 새로 넘긴 값이 지금 값의 일부면 같은 화면이다 — 예전엔 뒤로 가기로 돌아온 채널을 아바타·말풍선으로 다시 열면
  // 기록이 하나 더 쌓여 ⌘[ 를 두 번 눌러야 했다.
  const now = current === route ? liveParams() : null;
  const same = Boolean(now) && (JSON.stringify(currentParams) === JSON.stringify(params)
    || Object.keys(params).every((k) => JSON.stringify(now[k]) === JSON.stringify(params[k])) && Object.keys(params).length > 0);
  if (push && current && !same) {
    backStack.push({ route: current, params: liveParams() });
    if (backStack.length > BACK_MAX) backStack.shift();
  }
  current = route;
  // 방송&클립은 고른 탭·기간을 스스로 기억한다 — 연 때의 탭·기간을 경로에 남기면 ⌘R·재연결·작업 공간 복원이 사용자가 바꾼 탭을 되돌렸다.
  currentParams = route === 'popular' ? {} : route === 'home' ? { view: params.view || pages.home.view } : params;
  if (route === 'multilive') keepTransient(); // 멀티라이브 화면에서 본 패인은 남긴다
  if (LIVE_MODES.includes(route)) lastLiveMode = route;
  sidebar.setActive(LIVE_MODES.includes(route) ? 'live' : route);
  refreshSidebarList();
  const watching = isWatching();
  if (insights) insights.show(route === 'live' && viewer && (viewer.isPlaying || watching) ? 'live' : route);
  if (!AUX) saveWorkspace(route, currentParams, viewer && viewer.isPlaying ? viewer.channelId : null);
  // 라이브 메뉴는 재생 중이면 시청 화면, 아니면 이어 볼 채널 목록이다.
  // 멀티라이브에 있는 채널을 라이브로 볼 때(시청 모드)는 멀티라이브 화면을 단독 무대로 보여 준다.
  const showViewer = route === 'live' && viewer.isPlaying;
  const showWatch = route === 'live' && !showViewer && watching;
  for (const [id, page] of Object.entries(pages)) {
    // 홈 안 리포트의 표시 여부는 HomePage가 고른다. 먼저 숨기면 같은 홈 안 이동에서도 스크롤이 초기화된다.
    if (id === 'stats' && page.embedded && route === 'home' && !showViewer) continue;
    page.root.hidden = showViewer || id !== (showWatch ? 'multilive' : route);
  }
  // 숨긴 화면 안의 요소가 전체 화면이면 풀어 준다 — 단독 보기 '영상+채팅 전체 화면'(⌃⌘F)에서 ⌘W·× 로 닫으면
  // 숨은 멀티라이브 화면이 전체 화면으로 남아 검은 화면만 보였다(시청 화면 viewer.close 는 이미 푼다).
  exitHiddenFullscreen(Object.values(pages).map((p) => p.root));
  // 숨은 탐색 페이지는 잠시 뒤 이미지 비트맵을 놓고, 보이게 된 페이지는 그리기 전에 되돌린다(page-hibernate.js).
  syncHibernation(pages);
  // 멀티라이브 화면 자체인가 — 라이브 단독 보기(showWatch)도 같은 화면을 쓰므로 그때는 멀티라이브 안내를 띄우지 않는다.
  if (pages.multilive) pages.multilive.onMultiRoute = route === 'multilive';
  refreshSegmentGliders(); // 숨어 있는 동안 고른 칸이 바뀐 세그먼트(전환 버튼 등)의 판을 제자리로
  syncWindowOcclusion(); // 멀티라이브 화면을 떠나면(잠시 뒤) 보이지 않는 패인 디코드를 멈추고, 돌아오면 바로 되돌린다
  if (showViewer) {
    setMenuHidden(viewer.player.iframe, false);
    viewer.reveal();
    setAudioOwner('live');
    setMultiLiveParked(true);
    return;
  }
  viewer.hide();
  // 다른 메뉴로 옮겨 뒤에서 소리만 나는 라이브 시청 화면은 화질을 낮춰 보이지 않는 1080p 디코딩을 줄인다(PiP 중이면 그대로).
  if (viewer.isPlaying) setMenuHidden(viewer.player.iframe, true);
  if (showWatch) {
    setMultiLiveParked(false);
    setAudioOwner('multilive');
    pages.multilive.show();
    return;
  }
  if (route === 'multilive') {
    if (watching) await exitWatch();
    setMultiLiveParked(false);
    setAudioOwner('multilive');
    pages.multilive.reviveChatIfDropped();
  }
  // 파라미터는 받겠다고 표시한 화면(채널 상세 등)에만 넘긴다 — 목록 화면의 show(refresh) 와 겹치지 않게.
  if (pages[route].acceptsParams) pages[route].show(params, { back });
  else pages[route].show();
}

/** 뒤로 가기 (`⌘[`, 마우스 뒤로 버튼). 스택이 비면 홈. */
function goBack() {
  const prev = backStack.pop();
  if (prev) navigate(prev.route, prev.params, { push: false, back: true });
  // 보조 창은 자기 화면 하나로 열린다(기록 없음) — 비었을 때 홈으로 가면 채팅·통계 창이 사이드바 없는 홈이 되고 되돌아갈 길도
  // 없었다(마우스 뒤로 단추 한 번에). 보조 창 안에서 쌓인 기록(통계 → 채널 → 뒤로)은 그대로 돌아간다.
  else if (!AUX && current !== 'home') navigate('home', {}, { push: false });
}

function openChannelPage(channelId) {
  if (channelId) navigate('channel', { channelId });
}

function onViewerClosed() {
  if (AUX === 'player') { api.windowCloseSelf().catch(() => {}); return; }
  syncLiveIndicators();
  setMenuHidden(viewer.player.iframe, false);
  mlAudioSuspended(false);
  audioOwner = null;
  setMultiLiveParked(false);
  if (current === 'live') goBack();
}

function renderAccount() {
  sidebar.renderAccount(auth);
}

function onAuthChanged(status) {
  const wasLoggedIn = isLoggedIn();
  const wasExpired = Boolean(auth && auth.state === 'expired');
  auth = status || { loggedIn: false, state: 'loggedOut' };
  // 서버가 저장된 로그인을 받아 주지 않는다(재부팅 사이 세션 만료 등) — 시작 때든 쓰는 도중이든 한 번 알린다.
  if (auth.state === 'expired' && !wasExpired) toast('로그인이 만료되었습니다. 다시 로그인해 주세요', 6000);
  setMyNickname(auth.loggedIn ? auth.nickname : '');
  renderAccount();
  // 시작 때는 닉네임 없이 홈을 먼저 그린다 — 서버 확인으로 닉네임이 오면 인사말만 고친다.
  if (pages.home) pages.home.refreshGreetingName();
  if (wasLoggedIn !== isLoggedIn()) refreshSidebarFollowing();
  if (wasLoggedIn !== isLoggedIn()) resetFollowOverrides();
  if (viewer) viewer.refreshAuth(wasLoggedIn !== isLoggedIn());
  // 시청 상단 바의 팔로우 단추는 로그인했을 때만 — 로그인·로그아웃하면 지금 채널 상태를 다시 읽는다(예전엔 그대로 남았다).
  if (wasLoggedIn !== isLoggedIn() && pages.multilive) pages.multilive.watch.refreshAuth();
  if (wasLoggedIn !== isLoggedIn()) refreshChatAuth();
  if (wasLoggedIn !== isLoggedIn() && current && current !== 'search' && !(viewer && viewer.isOpen) && !liveListHidden()) {
    pages[current].show();
  }
  if (current === 'settings') pages.settings.render();
}

async function login() {
  try {
    await api.login();
    toast('네이버 로그인 창에서 로그인해 주세요');
  } catch (e) {
    toast(e.message);
  }
}

async function logout() {
  try {
    onAuthChanged(await api.logout());
    toast('로그아웃했습니다');
  } catch (e) {
    toast(e.message);
  }
}

/**
 * 다른 메뉴(홈·팔로잉·인기·검색·카테고리·사이드바·팔레트·채널 화면…)에서 라이브를 재생 — 늘 멀티라이브의
 * 라이브 단독 보기(머리줄 채널 고르기·오른쪽 채팅)로 연다. 예전 시청 화면은 보조 플레이어 창과 라이브 단독 보기가
 * 가득 찼을 때만 쓴다.
 * 재생으로 들어온 채널은 "둘러본 채널"(browsedWatch) — 머리줄 채널 고르기에 차례로 쌓여 다시 연결 없이 바꿔 본다.
 * 라이브 단독 보기 상한이 차면 가장 오래전에 둘러본 채널부터 바꿔 낀다(예전엔 하나만 두고 다음 재생이 늘 바꿔 껴서
 * 머리줄에 채널이 하나만 남았다). 멀티라이브에 직접 추가하거나 멀티라이브 화면으로 가면 남긴다(사이드바 수에 센다).
 */
const browsedWatch = new Map(); // 세션 id → 채널 id, 연 차례
const browsingChannels = new Set(); // 둘러보려고 넣는 중인 채널 id — 넣은 직후 그리기에서 '직접 넣음'으로 보이지 않게
function keepTransient(channelId = null) {
  let changed = false;
  for (const [id, ch] of browsedWatch) {
    if (channelId && ch !== channelId) continue;
    browsedWatch.delete(id);
    changed = true;
  }
  if (changed) syncLiveIndicators();
}
/** 자리를 내줄 둘러본 채널 — 가장 오래전에 연 것, 되도록 지금 보는 채널은 빼고. */
function oldestBrowsed() {
  const st = pages.multilive.state;
  const live = new Set((st.sessions || []).map((x) => x.id));
  for (const id of [...browsedWatch.keys()]) if (!live.has(id)) browsedWatch.delete(id);
  const ids = [...browsedWatch.keys()];
  return ids.find((id) => id !== st.watchId) || ids[0] || null;
}
async function openChannel(channelId) {
  // 채팅·멀티채팅·통계 창의 '방송 보기'는 플레이어 창으로 — 예전엔 그 창 안에서 재생해 메인 창 채팅 세션(main)을
  // 가져갔고, 창을 닫아도 재생·시청 기록·채팅 연결이 남았다(채팅 창 '▶ 영상 보기'와 같게).
  if (AUX && AUX !== 'player') {
    await api.windowOpen('player', { channelId }).catch((er) => toast(er.message));
    return;
  }
  if (AUX || !pages.multilive) {
    await openInViewer(channelId, { ad: true });
    return;
  }
  pendingWatch++;
  const seq = ++openSeq;
  try { await openChannelWatch(channelId, seq); } finally { pendingWatch--; syncLiveIndicators(); }
}
// 가장 나중에 누른 열기만 무대에 올린다 — 예전엔 A(상세가 느림)를 누르고 곧 B 를 누르면 B 가 먼저 오른 뒤 늦게 끝난
// A 가 무대를 빼앗고, 둘 다 프리롤 광고를 틀었다. 늦은 A 는 둘러본 채널로만 남는다(상한이 차면 먼저 빠짐).
let openSeq = 0;
async function openChannelWatch(channelId, seq = openSeq) {
  // 이미 멀티라이브에 있는 채널 — 같은 플레이어를 라이브 무대로(원본 commitExistingWatch).
  const existing = multiLiveSessionFor(channelId);
  if (existing) { await enterWatch(existing.id); return; }
  // 새로 여는 방송 — 치트키가 없으면 치지직처럼 프리롤 광고를 그 영상 영역 안에서 먼저 본다(무대에 올린 뒤).
  noteConnectorLimit();
  const st = pages.multilive.state;
  // 라이브 단독 보기 상한 — 멀티라이브 상한과 따로다(지금 보기가 멀티라이브여도 라이브로 여는 추가는 이쪽).
  if ((st.canAddLive ?? st.canAdd) === false) {
    // 가득 참 — 가장 오래전에 둘러본 채널을 먼저 빼고, 없으면(모두 직접 넣은 채널) 예전 시청 화면으로
    const prev = oldestBrowsed();
    if (!prev) { await openInViewer(channelId, { ad: true }); return; }
    browsedWatch.delete(prev);
    await pages.multilive.run(api.multiLiveRemove(prev));
  }
  browsingChannels.add(channelId);
  let s;
  try {
    await pages.multilive.add(channelId, { quiet: true, live: true, ad: false });
    s = multiLiveSessionFor(channelId);
    if (s) browsedWatch.set(s.id, channelId);
  } finally { browsingChannels.delete(channelId); }
  if (!s) return; // 추가 실패 — run 이 이미 알렸다
  if (seq !== openSeq) return; // 그새 다른 채널을 눌렀다
  await enterWatch(s.id);
  // 무대에 올리는 동안 다른 채널을 눌렀으면 광고를 틀지 않는다 — 예전엔 검사가 enterWatch 앞에만 있어,
  // 늦은 A 가 무대 밖에서 프리롤을 틀어 B 의 광고와 겹쳤다. (enterWatch 가 한 번 올린다.)
  if (openSeq !== seq + 1) return;
  pages.multilive.playPaneAd(channelId);
}

/** 예전 시청 화면으로 — 보조 플레이어 창, 멀티라이브가 가득 찼을 때. */
async function openInViewer(channelId, { ad = false } = {}) {
  if (isWatching()) await exitWatch();
  let player = null;
  try {
    player = (await api.settingsGet()).player;
  } catch { /* 기본값 */ }
  viewer.open(channelId, player);
  syncLiveIndicators();
  audioOwner = null;
  // 새로 여는 방송의 프리롤은 시청 화면 영상 영역 안에만 — 끝날 때까지 기다리지 않는다.
  if (ad) viewer.playAd(channelId);
  await navigate('live');
}

/**
 * 채널 추가 사이드바의 줄(썸네일·이름) — 멀티라이브 패인으로 넣어 같은 화면의 라이브 단독 보기로 올린다.
 * openChannel 은 멀티라이브에 없는 채널을 예전 시청 화면으로 열어 머리줄·채팅 배치가 통째로 바뀌었다.
 * 자리가 없을 때만 예전 시청 화면으로 연다.
 */
async function watchFromDrawer(channelId) {
  await openChannel(channelId);
}

/**
 * 카드·시청 화면·채널 추가 사이드바의 "멀티라이브에 추가" — 멀티라이브 화면으로 옮겨 패인을 띄우고 선택한다.
 * 선택은 포커스 메인으로 올리므로 마지막 배치(포커스면 그 메인 자리)로 재생되고, 소리·채팅이 이 채널로 따라온다.
 * quiet — 사이드바처럼 줄에 진행 표시가 따로 있으면 '추가하는 중' 안내를 띄우지 않는다.
 */
async function addToMultiLive(channelId, { quiet = false } = {}) {
  keepTransient(channelId); // 직접 넣은 채널은 다음 재생 때 빼지 않는다
  // 라이브 시청 화면의 채널이면 넘겨준다 — 시청 화면을 닫고(같은 채널 두 번 재생 없음) 볼륨을 이어받아 선택 패인으로.
  const handoff = viewer.isPlaying && viewer.channelId === channelId;
  const muted = handoff && Boolean(viewer.sessionMuted);
  if (handoff) {
    // 패인은 설정의 볼륨을 기본으로 튼다 — 시청 화면 볼륨을 먼저 저장해 같은 크기로 이어받는다.
    // (예전엔 이 값을 패인 배율에도 넣어 볼륨이 제곱으로 줄었다: 30% → 9%.)
    await api.playerSetVolume(Number(viewer.volume.value) / 100).catch(() => {});
    await viewer.close({ silent: true });
  }
  if (current !== 'multilive') await navigate('multilive');
  await pages.multilive.add(channelId, { quiet });
  const s = multiLiveSessionFor(channelId);
  if (!s) return;
  // 포커스 배치면 메인으로도 올린다 — 이미 있던 채널은 Rust 가 선택만 해(올리지 않음) 선택 id 가 같아 건너뛰었고,
  // 소리·채팅은 옮겨 가는데 작은 서브 칸에 남았다(새 채널의 '+' 는 메인으로 올라간다).
  const st = pages.multilive.state || {};
  const notMain = st.layout === 'focus' && st.focusMainId && st.focusMainId !== s.id;
  if (s.id !== st.selectedId || notMain) await pages.multilive.run(api.multiLiveSelect(s.id));
  const pane = pages.multilive.panes.get(s.slot);
  // 음소거였으면 패인도 끈 채로(소리 버튼을 누르면 원래 크기로).
  if (pane && muted) {
    pane.controls.lastGain = 1;
    pane.controls.range.value = '0';
    pane.controls.setGain(0);
  }
  syncLiveIndicators();
}

/** 카드 우클릭 메뉴 — 원본 LiveCardActionMenu (NAV-10). */
function installCardMenu() {
  // 메뉴 키·Shift+F10 — macOS 에는 메뉴 키가 없고 WKWebView 는 Shift+F10 을 contextmenu 로 바꾸지 않아, 키보드로는 카드·사이드바
  // 줄의 채널 메뉴(채팅만 열기·즐겨찾기·링크 복사)를 열 수 없었다(멀티라이브 칩·팔로잉 띠는 이미 직접 받는다).
  // Windows(WebView2)는 브라우저가 contextmenu 를 내므로 여기서 또 내지 않는다.
  if (!IS_WINDOWS) {
    document.addEventListener('keydown', (e) => {
      if (e.defaultPrevented || !(e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))) return;
      const t = e.target;
      const card = t && t.closest && t.closest('[data-channel-id]');
      if (!card || t.closest('.chat, .ml-stage, input, textarea, select, [contenteditable="true"]')) return;
      e.preventDefault();
      const r = (t.getBoundingClientRect ? t : card).getBoundingClientRect();
      t.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + Math.min(24, r.width / 2), clientY: r.bottom - 4 }));
    });
  }
  document.addEventListener('contextmenu', async (e) => {
    const card = e.target.closest('[data-channel-id]');
    if (!card || e.target.closest('.chat') || card.closest('.ml-stage')) return;
    e.preventDefault();
    const id = card.dataset.channelId;
    // 이름 칸이 없는 줄도 보이는 이름을 쓴다 — 예전엔 빈 이름으로 즐겨찾기 행이 생겨 보관함에 채널 id 만 보였다.
    const name = card.dataset.channelName || card.querySelector('.dense-name b, .card-title-name, .channel-name')?.textContent.trim() || '';
    const image = card.dataset.channelImage || null;
    let isFav = false;
    try { isFav = (await api.favoritesList()).some((f) => f.channelId === id); } catch { /* 무시 */ }
    const url = `https://chzzk.naver.com/live/${id}`;
    showMenu(e.clientX, e.clientY, [
      ['play', '재생'],
      ['multi', '멀티라이브에 추가'],
      ['channel', '채널 정보'],
      ['chat', '채팅만 열기'],
      ['-'],
      ['fav', isFav ? '즐겨찾기 해제' : '즐겨찾기 추가'],
      ['copy', '링크 복사'],
      ['open', '치지직에서 열기'],
    ], async (a) => {
      if (a === 'play') openChannel(id);
      else if (a === 'multi') addToMultiLive(id);
      else if (a === 'channel') openChannelPage(id);
      else if (a === 'chat') { if (viewer.isPlaying) viewer.close(); navigate('chat', { channelId: id }); }
      else if (a === 'fav') {
        // 알림을 받아 사이드바·홈을 다시 읽는다(아래 CHANNEL_SOCIAL_EVENT) — 시청 상단 바 별도 함께 따라간다.
        try { await api.favoriteSet(id, !isFav, name, image); toast(isFav ? '즐겨찾기에서 뺐습니다' : '즐겨찾기에 추가했습니다', 1500); channelSocialChanged(id, { favorite: !isFav }); } catch (er) { toast(er.message); }
      } else if (a === 'copy') navigator.clipboard.writeText(url).then(() => toast('링크를 복사했습니다', 1200), () => toast('링크를 복사하지 못했습니다'));
      else if (a === 'open') api.openExternal(url).catch((er) => toast(er.message));
    });
  });
}

/** ⌘K 커맨드 팔레트 (NAV-12) — 사이드바의 ⌘K(검색 이동)보다 먼저 받는다. */
let palette = null;
function installPalette() {
  palette = new CommandPalette({
    isLoggedIn,
    onOpenChannel: openChannelPage,
    onPlay: openChannel,
    onSearch: (q) => goRoute('search', { query: q }),
    actions: () => [
      // 화면 목록(ROUTES)에 이미 '검색'이 있다 — 따로 더하면 팔레트에 두 번 떴다. 검색만 설명을 붙인다.
      // 힌트는 그 화면의 실제 단축키(menu.rs GO) — 예전엔 모두 '이동'이었다(감사 UX-H09).
      ...ROUTES.map((r) => ({ group: '화면', label: r.title, hint: ROUTE_KEYS[r.id] ? kbd(ROUTE_KEYS[r.id]) : '이동', run: () => (AUX ? openInMainWindow(r.id) : r.id === 'live' ? openLiveMenu() : navigate(r.id)) })),
      { group: '보관함', label: '보관함 개요', hint: '한눈에', run: () => goRoute('library', { tab: 'overview' }) },
      { group: '방송&클립', label: '인기 방송', hint: '방송&클립', run: () => goRoute('popular', { tab: 'live' }) },
      { group: '방송&클립', label: '인기 클립', hint: '방송&클립', run: () => goRoute('popular', { tab: 'clips' }) },
      { group: '보관함', label: '이어 보기', hint: '다시보기', run: () => goRoute('library', { tab: 'continue' }) },
      { group: '보관함', label: '즐겨찾기', hint: '채널', run: () => goRoute('library', { tab: 'favorites' }) },
      { group: '보관함', label: '북마크', hint: '방송 장면', run: () => goRoute('library', { tab: 'bookmarks' }) },
      { group: '홈', label: '데이터 관리 · 백업', hint: '시청 리포트', run: () => goRoute('stats', { tab: 'data' }) },
      { group: '동작', label: resolvedTheme() === 'dark' ? '라이트 테마로 전환' : '다크 테마로 전환', hint: '테마', run: () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark') },
      { group: '동작', label: '뒤로 가기', hint: kbd('⌘['), run: goBack },
      { group: '동작', label: '사이드바 접기/펼치기', hint: kbd('⌘\\'), run: () => sidebar.toggle() },
      { group: '동작', label: '현재 화면 새로고침', hint: kbd('⌘R'), run: () => reloadCurrent({ veil: true }) },
      ...(isLoggedIn() ? [{ group: '동작', label: '로그아웃', hint: '계정', run: logout }] : [{ group: '동작', label: '네이버 로그인', hint: '계정', run: login }]),
    ],
  });
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (palette.isOpen) palette.close(); else palette.open();
    }
  }, true);
}

// ── 메뉴·단축키·정보·오프라인 (SYS-01·02·03·09·12) ─────────────────

// Esc 는 맨 위의 것 하나만 닫는다 — 메뉴·시트·입력란이 먼저 받고, 아무것도 없을 때만 시청 화면을 닫는다.
const ESC_POPUPS = ':is(.quality-menu, .vt-menu, .player-panel, .emoji-picker, .chat-tray, .dp-panel, .chat-autocomplete, .ml-pane-menu, .ml-more-menu, .ml-watch-overflow):not([hidden], .exit-ghost)';
const ESC_LAYERS = '.sheet-scrim:not(.leaving), .palette-scrim:not([hidden], .exit-ghost)';
/** 시트(.sheet-scrim)가 열리면 떠 있던 팝오버·메뉴를 닫는다 — 팝오버 층(z 120)이 시트(z 80)보다 높아 확인 창 위에 남았다(감사 UX-H06).
 *  층 순서를 바꾸면 시트 안에서 여는 메뉴가 시트 밑에 깔리므로, 순서는 두고 여는 순간 닫는다. */
/**
 * 화면(.page) 폭 단계 클래스 — w-lt670·w-lt770·w-lt870(사이드바 232px 를 뺀 예전 화면 폭 900·1000·1100 기준).
 * 인사이트 패널이 열려 본문만 좁아질 때도 좁은 배치가 걸리게(app.css). 숨은 화면은 폭이 0 이라 건드리지 않는다.
 */
function watchPageWidths(root) {
  const STEPS = [670, 770, 870];
  // 잰 폭은 모아 두었다가 다음 프레임에 클래스로 — 크기 감시 콜백 안에서 바로 바꾸면 뒤이은 다른 감시(채팅 분할기 등)의
  // 크기 읽기가 강제 레이아웃이 됐다(perf P01). 같은 단계면 건드리지 않는다.
  const pending = new Map();
  let raf = 0;
  const flush = () => {
    raf = 0;
    for (const [el, w] of pending) {
      for (const s of STEPS) { const on = w < s; if (el.classList.contains(`w-lt${s}`) !== on) el.classList.toggle(`w-lt${s}`, on); }
    }
    pending.clear();
  };
  const ro = new ResizeObserver((entries) => {
    for (const { target, borderBoxSize, contentRect } of entries) {
      if (!contentRect.width) continue;
      const box = borderBoxSize && borderBoxSize[0];
      pending.set(target, box ? box.inlineSize : contentRect.width);
    }
    if (pending.size && !raf) raf = requestAnimationFrame(flush);
  });
  for (const p of root.querySelectorAll(':scope > .page')) ro.observe(p);
}

function closeFloatingOnSheet() {
  new MutationObserver((records) => {
    const opened = records.some((r) => [...r.addedNodes].some((n) => n.nodeType === 1 && n.classList.contains('sheet-scrim')));
    if (!opened) return;
    closeUserPop();
    sidebar?.closeAccountMenu?.();
    for (const m of document.querySelectorAll('body > :is(.ctx-menu, .design-menu)')) m.remove();
  }).observe(document.body, { childList: true });
}

/** 시청 화면 밖의 Esc — 떠 있는 패널·메뉴만 닫는다(입력란 초점·화면 이동은 그대로). 예전엔 시청 화면에서만 닫혔다(감사 UX-H24). */
function closePopupsOnEscape(e) {
  if (e.defaultPrevented) return;
  const popups = document.querySelectorAll(ESC_POPUPS);
  for (const p of popups) p.hidden = true;
  const layers = document.querySelectorAll('body > :is(.ctx-menu, .design-menu)');
  for (const m of layers) m.remove();
  if (popups.length || layers.length) e.preventDefault();
}

// 팝업 바깥에서 마지막으로 초점을 받은 요소 — 제 Esc 처리가 없는 팝업(플레이어 설정·멘션 모음 등)을 여기서 닫을 때
// 초점을 돌려줄 곳. 예전엔 숨기기만 해 초점이 body 로 떨어졌고, 다음 Tab 은 문서 처음부터, 한 번 더 Esc 는 시청 화면을 닫았다.
let focusBeforePopup = null;
document.addEventListener('focusin', (e) => {
  const t = e.target;
  if (t instanceof Element && !t.closest(ESC_POPUPS)) focusBeforePopup = t;
});
function escapeTaken(e) {
  if (e.defaultPrevented) return true;
  const popups = document.querySelectorAll(ESC_POPUPS);
  if (popups.length) {
    const active = document.activeElement;
    const inside = [...popups].some((p) => p.contains(active));
    for (const p of popups) p.hidden = true;
    const back = focusBeforePopup;
    if (inside && back && back.isConnected && back.getClientRects().length && !back.closest('[inert], [hidden]')) back.focus({ preventScroll: true });
    return true;
  }
  // 채널 추가 사이드바 — 안에 초점이 없을 때의 Esc 는 사이드바만 닫는다(라이브 단독 보기를 닫지 않게).
  const drawer = !pages.multilive.root.hidden && pages.multilive.addBar.isOpen;
  if (drawer) { pages.multilive.addBar.close(); return true; }
  const layer = document.querySelector('.ctx-menu, .design-menu');
  if (layer) { layer.remove(); return true; } // 초점을 받지 않는 메뉴 — 제 Esc 처리가 불리지 않는다
  if (document.querySelector(ESC_LAYERS)) return true;
  const t = e.target;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) { t.blur(); return true; }
  return false;
}

function activeChat() {
  if (viewer.isOpen) return viewer.chat;
  if (current === 'multilive' || activeWatch()) return pages.multilive.chat;
  if (current === 'chat' && pages.chat) return pages.chat.chat; // 채팅만 보기·채팅 창 — ⇧⌘K 지우기·다시 연결
  return null;
}

// 영상이 주인공인 화면은 새로고침 덮개로 흐리지 않는다(다시 불러올 목록이 없고 영상 흐림은 무겁다).
const NO_REFRESH_VEIL = new Set(['multilive', 'multichat', 'media', 'chat']);
/** 지금 화면의 경로 인자 — 화면 안에서 바꾼 것(검색창에 새로 낸 검색어·고른 카테고리)을 덧씌운다. 예전엔 들어올 때의
 *  인자만 써서 ⌘R·네트워크 재연결·'다시 시도'·뒤로 가기가 검색어와 카테고리를 처음 값으로 되돌렸다. */
function liveParams() {
  const page = pages[current];
  return page && page.routeParams ? { ...currentParams, ...page.routeParams() } : currentParams;
}

/** 지금 화면을 다시 불러온다. `veil` 은 사용자가 직접 고른 새로고침(⌘R·메뉴·팔레트) — 로고 덮개(refresh-veil.js)를 띄운다. */
function reloadCurrent({ veil = false } = {}) {
  const page = pages[current];
  // 라이브 메뉴에서 무대·시청 화면이 앞에 있으면 뒤에 숨은 목록은 다시 불러오지 않는다.
  if (!page || liveListHidden()) return undefined;
  const run = () => (page.acceptsParams ? page.show({ ...liveParams(), refresh: true }) : page.show(true));
  return veil && !NO_REFRESH_VEIL.has(current) ? withRefreshVeil(page.root, run) : run();
}

const MENU_ACTIONS = {
  'nav-home': () => navigate('home'),
  'nav-following': () => navigate('following'),
  'nav-popular': () => navigate('popular'),
  'nav-multilive': () => navigate('multilive'),
  'nav-live': () => openLiveMenu(),
  'nav-library': () => navigate('library'),
  'nav-categories': () => navigate('categories'),
  'nav-multichat': () => navigate('multichat'),
  search: () => navigate('search'),
  palette: () => (palette.isOpen ? palette.close() : palette.open()),
  back: () => goBack(),
  refresh: () => reloadCurrent({ veil: true }),
  'toggle-sidebar': () => sidebar.toggle(),
  'theme-toggle': () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark'),
  'zoom-in': () => zoomStep(1),
  'zoom-out': () => zoomStep(-1),
  'zoom-reset': () => zoomStep(0),
  settings: () => navigate('settings'),
  about: () => showAbout(),
  'check-update': () => showUpdateCheck(),
  shortcuts: () => showShortcuts(),
  'report-issue': () => api.openExternal('https://github.com/dododo-lab/cview-rust/issues').catch((e) => toast(e.message)),
  'notification-settings': () => api.openNotificationSettings().catch((e) => toast(e.message)),
  'open-logs': async () => { try { const i = await api.appInfo(); await api.revealPath(i.dataDir); } catch (e) { toast(e.message); } },
  'play-pause': () => (viewer.isOpen ? viewer.controls.togglePlay() : activeWatch()?.togglePlay()),
  // 메뉴 막대·알림 영역 메뉴의 '음소거'도 여기로 온다 — 바뀐 상태를 아이콘 메뉴에 바로 알린다.
  mute: () => {
    if (viewer.isOpen) viewer.toggleMute(); else activeWatch()?.toggleMute();
    syncTray();
  },
  'go-live': () => (viewer.isOpen ? viewer.controls.goLive() : activeWatch()?.goLive()),
  screenshot: () => (activeWatch() ? activeWatch().screenshot() : viewer.isPlaying && viewer.controls.screenshot()),
  bookmark: () => (activeWatch() ? activeWatch().bookmark() : viewer.isPlaying && viewer.controls.bookmark()),
  'window-player': async () => {
    const w = activeWatch();
    if (w && w.channelId) {
      // 단독 보기의 채널을 새 창으로 — 같은 채널이 두 곳에서 재생되지 않게 이 창에서는 닫는다.
      const id = w.channelId;
      await closeWatch({ remove: true });
      api.windowOpen('player', { channelId: id }).catch((er) => toast(er.message));
    } else if (viewer.isPlaying) {
      const id = viewer.channelId;
      viewer.close();
      api.windowOpen('player', { channelId: id }).catch((er) => toast(er.message));
    } else api.windowOpen('player').catch((er) => toast(er.message));
  },
  'window-chat': () => {
    const id = viewer.isOpen ? viewer.channelId : (activeWatch()?.channelId || (current === 'chat' ? pages.chat.channelId : null));
    api.windowOpen('chat', id ? { channelId: id } : null).catch((er) => toast(er.message));
  },
  'window-multichat': () => api.windowOpen('multichat').catch((er) => toast(er.message)),
  'window-stats': () => api.windowOpen('stats').catch((er) => toast(er.message)),
  pip: () => {
    const w = activeWatch();
    if (w) w.togglePip();
    else if (viewer.isPlaying) viewer.controls.togglePip();
    else toast('재생 중인 방송이 없습니다');
  },
  welcome: () => showWelcome({ force: true, onLogin: login }),
  // 도움말 › 프로젝트 — 이용 고지·오픈소스 라이선스는 시트, 문의는 메일 앱, 저장소는 기본 브라우저(notice.js).
  notice: () => showNoticeReview(),
  licenses: () => showLicenses(),
  contact: () => openProjectLink('contact'),
  'github-repo': () => openProjectLink('repo'),
  fullscreen: () => (viewer.isOpen ? viewer.controls.fullscreen(false) : activeWatch()?.fullscreen(false)),
  'fullscreen-chat': () => (viewer.isOpen ? viewer.controls.fullscreen(true) : activeWatch()?.fullscreen(true)),
  // 보조 창(채팅·멀티채팅·통계, 방송 없는 플레이어 창)은 닫을 시청 화면이 없으면 창을 닫는다 — ⌘W 를 이 메뉴가 가져가서
  // 예전엔 이 창들을 단축키로 닫을 수 없었다.
  'close-viewer': () => (viewer.isOpen ? viewer.close() : activeWatch() ? closeWatch() : AUX && api.windowCloseSelf().catch(() => {})),
  // 멀티채팅(화면·창)은 모든 채널 채팅을 지운다 — 예전엔 대상이 없어 ⇧⌘K 가 아무 일도 안 했다(감사 UX-C12)
  'chat-clear': () => {
    if (current === 'multichat' && pages.multichat) {
      const mc = pages.multichat;
      for (const c of mc.channels || []) c.panel?.clear(true);
      if (mc.layout === 'merged' && mc.mergedList) { mc.lines?.reset?.(); mc.mergedList.innerHTML = '<div class="empty">채팅을 지웠습니다</div>'; }
      return;
    }
    activeChat()?.clear(true);
  },
  'chat-reconnect': () => activeChat()?.reconnect(),
  'chat-autoscroll': () => {
    const on = !chatPrefs().autoScroll;
    updateChatPrefs({ autoScroll: on });
    toast(on ? '자동 스크롤 켬' : '자동 스크롤 끔', 1200);
  },
};

/** 화면 크기 한 단계(메뉴·단축키) — 저장·모든 창 적용은 Rust, 화면은 설정 변경 알림으로 따라온다. */
async function zoomStep(dir) {
  if (!isTauri) return;
  try {
    // 모든 창의 화면(썸네일 배율·설정 슬라이더)은 Rust 가 보내는 settings://appearance 로 따라온다.
    const settings = await api.uiZoomStep(dir);
    const scale = Number(settings && settings.appearance && settings.appearance.uiScale) || 1;
    toast(`화면 크기 ${Math.round(scale * 100)}%`, 1200);
  } catch (e) { toast(e.message); }
}

// 보조 창(?w=…)은 사이드바 없이 한 화면만 그린다 — 화면 이동 메뉴·단축키를 그 창에서 따르면 채팅·통계 창이 사이드바 없는
// 홈이나(보조 창은 멀티라이브를 그리지 않아) 빈 멀티라이브로 바뀌고, 그 화면의 단추가 메인 창 멀티라이브를 바꿨다.
// 이 동작들은 메인 창을 앞으로 가져와 그 창에서 연다.
const MAIN_ROUTE_ACTIONS = {
  'nav-home': 'home', 'nav-following': 'following', 'nav-popular': 'popular', 'nav-multilive': 'multilive',
  'nav-live': 'live', 'nav-library': 'library', 'nav-categories': 'categories', 'nav-multichat': 'multichat',
  search: 'search', settings: 'settings',
};
function openInMainWindow(route, params = {}) {
  api.trayControl({ kind: 'open-route', route, params }).catch((e) => toast(e.message));
}
/** 화면 이동 — 보조 창이면 메인 창에서(명령 팔레트도 같은 규칙, MAIN_ROUTE_ACTIONS 참고). */
function goRoute(route, params = {}) {
  if (AUX) openInMainWindow(route, params); else navigate(route, params);
}

function installMenuAndShortcuts() {
  // 메뉴 이벤트는 모든 창에 간다 — 앞에 있는 창만 실행한다.
  listen('menu://action', (p) => {
    const run = p && MENU_ACTIONS[p.id];
    // 처음 실행 고지가 떠 있는 동안은 메뉴 막대 동작도 받지 않는다(동의 전에 앱을 쓰지 않게).
    if (!run || !document.hasFocus() || noticeOpen()) return;
    if (AUX && MAIN_ROUTE_ACTIONS[p.id]) { openInMainWindow(MAIN_ROUTE_ACTIONS[p.id]); return; }
    run();
  });
  // 메뉴가 없는 환경(브라우저 미리보기 등)과 ⌘/ 보조.
  document.addEventListener('keydown', (e) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    if (e.altKey && e.metaKey) {
      const hub = HUBS.find(([k]) => e.code === `Digit${k}`);
      if (hub) {
        e.preventDefault();
        if (AUX) { openInMainWindow(hub[1]); return; }
        if (hub[1] === 'live') openLiveMenu(); else navigate(hub[1]);
        hubFlash(hub[2]);
        return;
      }
    }
    // 화면 크기 — macOS 는 메뉴 단축키가 먼저 가져간다. 메뉴 막대가 없는 Windows 는 여기서.
    if (IS_WINDOWS && !e.altKey && (e.key === '+' || (!e.shiftKey && ['=', '-', '0'].includes(e.key)))) {
      e.preventDefault(); zoomStep(e.key === '0' ? 0 : e.key === '-' ? -1 : 1); return;
    }
    if (e.key === '/') { e.preventDefault(); showShortcuts(); }
    else if (e.key === ',') { e.preventDefault(); if (AUX) openInMainWindow('settings'); else navigate('settings'); }
  });
  // 오프라인 배너 (SYS-12)
  const banner = document.createElement('div');
  banner.className = 'offline-banner';
  banner.innerHTML = '<span>오프라인입니다 — 네트워크가 돌아오면 자동으로 다시 불러옵니다</span>';
  banner.hidden = navigator.onLine !== false;
  document.querySelector('.content').prepend(banner);
  window.addEventListener('offline', () => {
    banner.hidden = false;
    if (isTauri) api.playerNetworkPath(false).catch(() => {});
  });
  // 네트워크가 흔들리면(VPN·Wi-Fi 전환) online 이 짧은 간격으로 여러 번 온다 — 복구 작업은 잠잠해진 뒤 한 번만.
  let onlineTimer = 0;
  window.addEventListener('offline', () => clearTimeout(onlineTimer));
  window.addEventListener('online', () => {
    banner.hidden = true;
    if (isTauri) api.playerNetworkPath(true).catch(() => {});
    clearTimeout(onlineTimer);
    onlineTimer = setTimeout(() => {
      if (navigator.onLine === false) return;
      toast('다시 연결되었습니다', 1500);
      // 자동 복구 — 덮개 없이. 실패는 화면이 스스로 안내한다(처리되지 않은 거부로 남기지 않는다).
      // 재생·채팅·편집 화면은 스스로 복구한다(영상 오류 안내·실패한 채팅만 다시 연결) — 다시 불러오면 보던 다시보기가
      // 광고부터 다시 열리고, 채팅 창 목록이 지워지고, 설정에 치던 글이 사라졌다. 목록 화면만 새로 받는다.
      if (pages[current] && !viewer.isOpen && autoReloadOnOnline(current)) Promise.resolve(reloadCurrent()).catch(() => {});
      // 끊긴 동안 실패한 사이드바 팔로잉·채팅도 바로 다시 — 다음 주기(2분)·수동 "다시 연결"을 기다리지 않게.
      refreshSidebarFollowing();
      reconnectFailedChats();
    }, ONLINE_SETTLE_MS);
  });
}

/** 네트워크가 돌아왔을 때 이 화면을 다시 불러올까 — 목록 화면은 늘, 재생·채팅 화면은 열다 실패해 비어 있을 때만,
 *  멀티라이브·멀티채팅·설정은 하지 않는다(스스로 복구하거나 다시 받을 것이 없다). */
function autoReloadOnOnline(route) {
  if (route === 'multichat' || route === 'multilive' || route === 'settings') return false;
  if (route === 'media') return Boolean(pages.media.body.querySelector('.empty.error'));
  if (route === 'chat') return !pages.chat.chat.active;
  return true;
}

/** online 이벤트가 잠잠해지기를 기다리는 시간. */
const ONLINE_SETTLE_MS = 800;

/** 전체 화면이면 신호등 자리를 비우지 않는다. */
function trackFullscreen() {
  const win = window.__TAURI__ && window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow
    ? window.__TAURI__.window.getCurrentWindow() : null;
  if (!win) return;
  const sync = async () => {
    try { document.documentElement.classList.toggle('is-fullscreen', await win.isFullscreen()); } catch { /* 무시 */ }
  };
  win.onResized(sync);
  sync();
}

async function main() {
  installUiHealth(); // 멈춤 감시 신호·오류 기록 — 무엇보다 먼저(아래에서 실패해도 신호는 간다)
  installImageFade(); // 첫 화면 그림보다 먼저 — 놓친 load 로 썸네일이 투명하게 남지 않게
  document.addEventListener('error', fallbackAvatar, true); // 모든 화면의 아바타 — 못 받으면 첫 글자·바탕 원(cards.js)
  installSegmentGlider(); // 모든 세그먼트의 고른 칸 판이 미끄러지게
  installMicroMotion();
  installLiveMotion();
  installHoverPreview(); // 재생 썸네일 위에 잠시 머물면 소리 없이 미리보기
  trackFullscreen();
  installWindowControls(); // Windows — 제목 막대 대신 화면 오른쪽 위의 메뉴·최소화·최대화·닫기
  splashStep('화면 준비', 0.2);
  renderNav();
  const content = document.querySelector('.content');
  const page = (id) => content.querySelector(`[data-page="${id}"]`);
  pages.home = new HomePage(page('home'), { isLoggedIn, getAuth: () => auth });
  pages.following = new FollowingPage(page('following'), { isLoggedIn });
  pages.popular = new PopularPage(page('popular'));
  pages.search = new SearchPage(page('search'));
  pages.settings = new SettingsPage(page('settings'), {
    getAuth: () => auth,
    onLogin: login,
    onLogout: logout,
  });
  pages.multilive = new MultiLivePage(content, { isLoggedIn, onAddChannel: addToMultiLive, onOpenChannel: watchFromDrawer, onSessionsChanged: () => syncWindowOcclusion() });
  // 둘러본 채널 — 지난 구성 저장에서 뺀다. 넣는 중(browsedWatch 에 들기 전)인 채널도 둘러본 채널이다.
  pages.multilive.isTransient = (sessionId, channelId) => browsedWatch.has(sessionId) || (channelId != null && browsingChannels.has(channelId));
  pages.live = new LivePage(content, { isLoggedIn, modeSwitch: !AUX });
  pages.categories = new CategoriesPage(content);
  pages.multichat = new MultiChatPage(content, { isLoggedIn, onPlay: openChannel });
  // 채팅 창(?w=chat, 420px)에서 '▶ 영상 보기'는 플레이어 창을 연다 — 같은 창에 시청 화면을 열면 영상이 80px 로 눌리고 채팅이 겹쳤다.
  const chatOnlyPlay = AUX === 'chat' ? (id) => api.windowOpen('player', { channelId: id }).catch((er) => toast(er.message)) : openChannel;
  pages.chat = new ChatOnlyPage(content, { isLoggedIn, onBack: goBack, onPlay: chatOnlyPlay, session: AUX === 'chat' ? `win-${Date.now()}` : null, standalone: AUX === 'chat' });
  pages.library = new LibraryPage(content, { isLoggedIn, onOpenChannel: openChannelPage });
  pages.stats = new StatsPage(AUX === 'stats' ? content : pages.home.panels, { onOpenChannel: openChannelPage, embedded: AUX !== 'stats' });
  if (AUX !== 'stats') pages.home.attachReport(pages.stats);
  pages.media = new MediaPage(content, { onBack: goBack, onOpenChannel: openChannelPage });
  watchPageWidths(content);
  // 보조 창(플레이어 창 등)에는 사이드바가 없다 — 채널 화면·카드의 '멀티라이브에 추가'로 그 창이 멀티라이브로 바뀌면
  // 보던 방송이 닫히고 돌아올 길이 ⌘[ 뿐이었다(감사 UX-H15 남은 부분). 시청 화면 ⋯ 처럼 보조 창에선 뺀다.
  pages.channel = new ChannelPage(content, { onBack: goBack, onPlay: openChannel, onAddMulti: AUX ? null : addToMultiLive });
  // 위 막대 ⋯ 의 '멀티라이브에 추가'·'새 창으로 분리'(보조 창 안에서는 없다) — viewer-topbar.js
  viewer = new Viewer(document.getElementById('viewer'), {
    isLoggedIn, onClose: onViewerClosed,
    // 보조 창(플레이어 창 등)엔 사이드바가 없어 멀티라이브로 가면 돌아올 길이 없다 — 주석대로 보조 창에선 뺀다(감사 UX-H15).
    onAddMulti: AUX ? null : (id) => addToMultiLive(id),
    onPopout: AUX ? null : () => MENU_ACTIONS['window-player'](),
    // 플레이어 창의 채팅은 창마다 따로 — 예전엔 메인 창의 기본 세션을 함께 써서, 창을 닫아도 연결이 남았고(창 정리가
    // 기본 세션은 건드리지 않는다) 메인 창의 '채팅만 열기'가 같은 세션을 다른 채널로 바꿔 이 창의 채팅을 멈췄다.
    chatSession: AUX === 'player' ? `pw-${Date.now()}` : null,
  });
  if (!AUX) insights = new InsightPanel(content, { isLoggedIn });
  if (!AUX) bindMultiLiveAddButtons(addToMultiLive);
  if (!AUX) {
    document.querySelector('.viewer-header .page-title').insertAdjacentHTML('afterend', liveModeSwitch('live'));
    bindLiveModeSwitch(switchLiveMode);
  }
  // 화면 머리줄 — 메뉴 아이콘 칩 + 제목 위·설명 아래(page-header.js). 멀티라이브(스스로 꾸민다)·시청 화면은 한 줄.
  // 라이브 목록도 한 줄 — 전환 버튼이 세 화면(목록·무대·멀티라이브)에서 같은 높이에 있게(설명은 본문 첫 줄로).
  for (const r of ROUTES) {
    if (pages[r.id] && r.id !== 'multilive') decoratePageHeader(pages[r.id].root.querySelector(':scope > .page-header'), r.icon, { compact: r.id === 'live' });
  }
  decoratePageHeader(document.querySelector('.viewer-header'), liveIcon, { compact: true });
  installBackgroundSaver();
  installPerfCollector();
  // 메뉴 막대·알림 영역 메뉴와 macOS 팝오버의 '지금 시청 중'·재생 제어(tray-state.js, tray_popover.rs).
  // 메인 창이 숨어 있거나 다른 화면에 있어도 시청 화면·단독 보기·멀티라이브·다시보기를 일관되게 제어한다.
  if (!AUX) {
    const videoPip = (v) => Boolean(v && (v.webkitPresentationMode === 'picture-in-picture' || v.ownerDocument?.pictureInPictureElement === v));
    const trayPane = () => {
      const w = activeWatch();
      if (w?.pane) return w.pane;
      const ml = pages.multilive;
      if (ml && ml.nowPlayingSlot != null) return ml.panes.get(ml.nowPlayingSlot) || null;
      return null;
    };
    const readTrayMuted = () => {
      if (viewer.isOpen || (viewer.isPlaying && !trayPane())) return Boolean(viewer.sessionMuted);
      const pane = trayPane();
      if (pane?.controls?.el) return !pane.controls.el.classList.contains('audible');
      if (current === 'media' && pages.media?.video) return Boolean(pages.media.video.muted || pages.media.video.volume === 0);
      return null;
    };
    const readTrayPlayback = () => {
      if (viewer.isOpen || (viewer.isPlaying && !trayPane())) {
        const v = viewer.controls?.video;
        return {
          paused: v ? Boolean(v.paused) : false,
          volume: viewer.sessionMuted ? 0 : Math.round(Number(viewer.volume?.value ?? 100)),
          pip: v ? videoPip(v) : false,
        };
      }
      const pane = trayPane();
      if (pane?.controls) {
        const c = pane.controls;
        const v = c.video;
        const audible = c.el?.classList.contains('audible');
        return {
          paused: v ? Boolean(v.paused) : false,
          volume: audible ? Math.round(Number(c.range?.value ?? 100)) : 0,
          pip: v ? videoPip(v) : false,
        };
      }
      if (current === 'media' && pages.media?.video) {
        const v = pages.media.video;
        return {
          paused: Boolean(v.paused),
          volume: v.muted ? 0 : Math.round((Number(v.volume) || 0) * 100),
          pip: videoPip(v),
        };
      }
      return null;
    };
    installTrayState({ muted: readTrayMuted, playback: readTrayPlayback });

    listenHere(Events.trayControl, (p) => {
      if (!p || !p.kind) return;
      // 처음 실행 고지가 떠 있으면 화면을 옮기거나 방송을 여는 동작은 하지 않는다 — 예전엔 메뉴 막대 '멀티라이브 4채널'이
      // 고지 뒤에서 4개 방송을 소리와 함께 틀고, '업데이트 확인…' 시트가 고지 위에 떠 Esc 로도 닫히지 않았다.
      // (채널 열기·로그인 등 딥 링크 경로는 Rust 대기열이 이미 동의 뒤로 미룬다.)
      if ((noticeOpen() || noticePending) && TRAY_BLOCKED_BY_NOTICE.has(p.kind)) return;
      const pane = trayPane();
      switch (p.kind) {
        case 'play-pause':
          if (viewer.isOpen || (viewer.isPlaying && !pane)) viewer.controls.togglePlay();
          else if (activeWatch()) activeWatch().togglePlay();
          else if (pane?.controls) pane.controls.togglePlay();
          else if (current === 'media' && pages.media?.video) {
            const v = pages.media.video;
            if (v.paused) v.play().catch(() => {}); else v.pause();
          }
          setTimeout(syncTray, 120);
          break;
        case 'mute':
          if (viewer.isOpen || (viewer.isPlaying && !pane)) viewer.toggleMute();
          else if (activeWatch()) activeWatch().toggleMute();
          else if (pane?.controls) {
            if (!pane.controls.toggleMute()) pages.multilive.makeAudible(pane);
          } else if (current === 'media' && pages.media?.video) {
            pages.media.video.muted = !pages.media.video.muted;
          }
          syncTray();
          setTimeout(syncTray, 120);
          break;
        case 'volume': {
          const val = Math.max(0, Math.min(100, Math.round(Number(p.value) || 0)));
          if (viewer.isOpen || (viewer.isPlaying && !pane)) {
            viewer.volume.value = String(val);
            viewer.player.call('setVolume', val / 100);
            if (val > 0 && viewer.sessionMuted) viewer.setMuted(false);
            else if (val === 0 && !viewer.sessionMuted) viewer.setMuted(true);
            api.playerSetVolume(val / 100).catch(() => {});
          } else if (pane?.controls) {
            const c = pane.controls;
            if (val > 0 && !c.el.classList.contains('audible')) pages.multilive.makeAudible(pane);
            c.range.value = String(val);
            c.setGain(val / 100);
          } else if (current === 'media' && pages.media?.video) {
            const v = pages.media.video;
            v.volume = val / 100;
            v.muted = val === 0;
          }
          syncTray();
          break;
        }
        case 'pip':
          if (activeWatch()) activeWatch().togglePip();
          else if (viewer.isOpen || (viewer.isPlaying && !pane)) viewer.controls.togglePip();
          else if (pane?.controls?.video) togglePipFor(pane.controls.video);
          else if (current === 'media' && pages.media?.video) togglePipFor(pages.media.video);
          setTimeout(syncTray, 160);
          break;
        case 'navigate': {
          const params = p.params && typeof p.params === 'object' ? p.params : {};
          // 보조 창의 ⌘5·⌥⌘숫자·팔레트 '라이브'는 라이브 메뉴(마지막으로 본 라이브·멀티라이브) — 예전엔 늘 단독 라이브로 가
          // 마지막 보기를 덮었다.
          if (p.route === 'live' && !Object.keys(params).length) openLiveMenu();
          else if (p.route) navigate(p.route, params);
          break;
        }
        case 'show-now-playing':
          // 메뉴 막대·트레이 '지금 재생 중' — 재생 중인 화면으로(라이브 시청·단독 보기 → 라이브, 멀티라이브 칸 → 멀티라이브).
          // 다시보기 화면이면 이미 그 화면이라 그대로 둔다.
          if (current === 'media') break;
          if (viewer.isPlaying || isWatching()) { if (current !== 'live') navigate('live'); }
          else if (pane && current !== 'multilive') navigate('multilive');
          break;
        case 'queue-multi':
          if (Array.isArray(p.channelIds)) queueMulti(p.channelIds);
          break;
        case 'check-update':
          showUpdateCheck();
          break;
        // 알림 영역 메뉴 › 도움말(tray.rs help_menu_action) — 오픈소스 라이선스·이용 고지 시트.
        case 'menu':
          // 앱 메뉴(menu.rs needs_main_window) — 초점 있는 창이 없을 때 Rust 가 메인 창을 보이고 이리로 보낸다.
          if (MAIN_MENU_IDS.has(p.id) && !noticeOpen()) MENU_ACTIONS[p.id]();
          break;
        default:
          break;
      }
    });
  }
  // 사이드바 아래 앱 사용률(누르면 설정 › 성능) — 메인 창만. 명령이 없으면 그리지 않는다.
  if (!AUX) installSidebarUsage();

  for (const p of Object.values(pages)) {
    const body = p.root.querySelector('.page-body');
    if (body) bindCardOpen(body, openChannel);
  }
  // 사이드바 아래 목록 — 멀티채팅 메뉴에서는 채팅 추가, 카테고리 줄은 그 카테고리로, 나머지는 방송 열기.
  const sideList = document.getElementById('sidebar-live');
  bindCardOpen(sideList, async (channelId, data) => {
    if (data.sideAction === 'multichat' && pages.multichat) {
      await pages.multichat.add(channelId);
      paintSidebarList();
      return;
    }
    openChannel(channelId);
  });
  sideList.addEventListener('click', (e) => {
    const cat = e.target.closest('[data-side-category]');
    if (cat) navigate('categories', { category: cat.dataset.sideCategory });
  });
  document.addEventListener('click', (e) => {
    const action = e.target.closest('[data-action]');
    if (!action) return;
    if (action.dataset.action === 'login' && !action.closest('[data-page="settings"]')) login();
    if (action.dataset.action === 'logout' && !action.closest('[data-page="settings"]')) logout();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !document.fullscreenElement) {
      if (viewer.isOpen) { if (!escapeTaken(e)) viewer.close(); }
      else if (activeWatch()) { if (!escapeTaken(e)) closeWatch(); }
      else closePopupsOnEscape(e);
    }
    const typing = e.target && (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA');
    if ((e.metaKey && e.key === '[') || (e.altKey && e.key === 'ArrowLeft' && !typing)) {
      e.preventDefault();
      goBack();
    }
  });
  window.addEventListener('mouseup', (e) => { if (e.button === 3) goBack(); });
  window.addEventListener('cview:open-vod', (e) => e.detail && navigate('media', { kind: 'vod', id: e.detail.videoNo }));
  window.addEventListener('cview:open-clip', (e) => e.detail && navigate('media', { kind: 'clip', id: e.detail.clipUid }));
  window.addEventListener('cview:menu', (e) => { const run = MENU_ACTIONS[e.detail]; if (run && !noticeOpen()) run(); });
  window.addEventListener('cview:play', (e) => e.detail && openChannel(e.detail.channelId));
  window.addEventListener('cview:ml-watch', (e) => e.detail && enterWatch(e.detail.sessionId));
  window.addEventListener('cview:ml-watch-close', (e) => closeWatch(e.detail || {}));
  // 실패 화면의 "다시 시도" — 지금 화면을 다시 불러온다.
  document.addEventListener('click', (e) => {
    if (!e.target.closest('[data-retry]')) return;
    const page = pages[current];
    if (!page) return;
    // 검색처럼 입력으로 연 화면은 경로 매개변수에 검색어가 없다 — 제 retry 가 마지막 입력으로 다시 한다
    if (page.retry) page.retry();
    else if (page.acceptsParams) page.show(liveParams());
    else page.show(true);
  });
  // 라이브 무대의 채널이 닫힌 "순간"에만 화면을 옮긴다 — 조건만 보면 목록 화면에 머물 때도(1초마다 오는 알림)
  // 멀티라이브로 끌려가고, 채널을 여는 도중(무대에 오르기 전)에도 멀티라이브가 번쩍 보였다.
  let wasWatching = false;
  listen(Events.multiLiveState, () => setTimeout(() => {
    syncLiveIndicators();
    const watching = isWatching();
    const ended = wasWatching && !watching;
    wasWatching = watching;
    if (pendingWatch) return;
    // 라이브 무대의 채널이 닫혔으면 멀티라이브(남은 패인이 있으면) 또는 이전 화면으로.
    if (ended && current === 'live' && !viewer.isPlaying) {
      if (watchCloseBack) { watchCloseBack = false; goBack(); }
      else if ((pages.multilive.state.sessions || []).length) navigate('multilive', {}, { push: false });
      else goBack();
    }
  }, 0));
  // OS 메모리 압박(memory_pressure.rs) — 숨은 탐색 페이지의 이미지를 바로 놓는다.
  if (!AUX) listenHere('app://memory-pressure', () => hibernateHiddenNow(pages));
  window.addEventListener('cview:navigate', (e) => e.detail && navigate(e.detail.route, e.detail.params || {}));
  window.addEventListener('cview:multi-queue', (e) => e.detail && queueMulti(e.detail.channelIds || []));
  installSidebarExtras({
    onRandomLive: async () => {
      try {
        const lives = isLoggedIn() ? await api.followingLives(false) : await api.popularLives(30, false);
        if (!lives.length) { toast('지금 방송 중인 채널이 없습니다'); return; }
        const pick = lives[Math.floor(Math.random() * lives.length)];
        toast(`${pick.channelName} 방송을 엽니다`, 1500);
        openChannel(pick.channelId);
      } catch (er) { toast(er.message); }
    },
    onQueueMulti: async () => {
      try {
        const lives = isLoggedIn() ? await api.followingLives(false) : await api.popularLives(10, false);
        queueMulti([...lives].sort((a, b) => b.viewerCount - a.viewerCount).slice(0, 4).map((l) => l.channelId));
      } catch (er) { toast(er.message); }
    },
    onRefresh: () => MENU_ACTIONS.refresh(),
  });
  installCardMenu();
  installA11y();
  installPalette();
  installMenuAndShortcuts();
  installPrefsEffects({
    viewer,
    multilive: pages.multilive,
    refreshCurrent: () => {
      if (viewer.isOpen || palette.isOpen || liveListHidden()) return;
      if (['home', 'following', 'popular', 'categories', 'live'].includes(current)) pages[current].show(current === 'categories' ? {} : false);
      refreshSidebarFollowing();
    },
  });
  window.addEventListener('cview:open-channel', (e) => openChannelPage(e.detail && e.detail.channelId));
  // 영상 위 채널 정보(아바타·채널명)를 누르면 채널 상세
  document.querySelector('#viewer .stage-top').addEventListener('click', (e) => {
    if (e.target.closest('.viewer-avatar, .viewer-title') && viewer.channelId) openChannelPage(viewer.channelId);
  });

  listen(Events.authChanged, onAuthChanged);
  window.addEventListener('cview:login', login);
  // 팔로우·즐겨찾기가 어디서든 바뀌면(상단 바·채팅 말풍선·채널 화면·카드 메뉴) 사이드바·홈 팔로잉(즐겨찾기 순서)을 다시 읽는다.
  window.addEventListener(CHANNEL_SOCIAL_EVENT, () => refreshSidebarFollowing());
  window.addEventListener(RECENT_CLEARED_EVENT, () => {
    sideData.recent = [];
    sideData.recentAt = Date.now();
    paintSidebarList();
    if (pages.home) { pages.home.dbRecent = []; pages.home.loadedAt = 0; }
  });
  loadChatPrefs();
  loadShortcuts();
  listen(Events.loginFailed, (p) => p && p.message && toast(p.message));

  renderAccount();
  splashStep('계정·설정 확인', 0.35);
  if (!isTauri) toast('CView 앱(cargo run -p cview-app) 안에서 실행해야 데이터가 표시됩니다', 6000);
  // 계정 상태와 설정은 서로 기다릴 필요가 없어 함께 부른다(예전엔 차례로 기다렸다).
  // 계정은 서버 확인 없이 저장된 로그인만 본다(quick) — 서버 확인(/users/me) 한 번을 기다린 뒤에야 홈 요청이
  // 시작되던 것을 없앤다. 닉네임·만료 판정은 홈을 띄운 뒤 아래의 일반 확인과 auth://changed 가 채운다.
  const [authRes, settingsRes] = await Promise.allSettled(isTauri ? [api.authStatus(true), api.settingsGet()] : []);
  if (authRes && authRes.status === 'fulfilled') onAuthChanged(authRes.value);
  const settings = settingsRes && settingsRes.status === 'fulfilled' ? settingsRes.value : null;
  syncTheme(settings && settings.appearance && settings.appearance.theme);
  // 선명도(예전 선명한 화면 + 업스케일) — 받은 설정으로 정하고, 새 판 첫 실행이면 예전 값에서 한 번 옮긴다.
  // 보조 창도 같은 값을 쓴다(안내 토스트는 메인 창에서만 — 시작 화면이 걷힌 뒤 flushSharpnessNotice).
  initSharpness(settings && settings.player, { notice: !AUX });
  // 보조 창은 홈이 없다 — 스플래시(최소 0.4초)만 걷고 제 화면으로.
  // 고지에 동의하기 전 — 보조 창(메뉴 막대 '채팅' 등)은 열지 않는다. 메인 창의 고지에서 동의해야 쓸 수 있다
  // (예전엔 고지를 띄운 채 채팅 창을 열고 그 창의 '영상 보기'로 방송까지 볼 수 있었다).
  // 닫기 전에 메인 창(고지)을 앞으로 — 트레이로 숨겨 둔 메인 창이면 창이 아무 안내 없이 사라지기만 했다.
  if (AUX && settings && needsNotice(settings.general)) {
    try { await api.trayControl({ kind: 'show-main' }); } catch { /* 앱 밖 */ }
    api.windowCloseSelf().catch(() => {});
    return;
  }
  if (AUX) { hideSplash(); openAuxWindow(); return; }
  // 작업 공간 복원 (NAV-16) — "실행 시 창 복원"이 켜져 있으면 마지막 화면으로. 방송은 자동 재생하지 않고 묻는다.
  const ws = settings?.general?.restoreWindowOnLaunch !== false ? loadWorkspace() : null;
  // 홈은 로딩 화면 뒤에서 먼저 불러 둔다 — 스켈레톤이 아니라 채워진 홈이 바로 보이게.
  navigate('home', {}, { push: false });
  if (isTauri) {
    api.authStatus().then(onAuthChanged).catch(() => {});
    api.appInfo().then(noteSaveDirs).catch(() => {});
  }
  // 고지를 물어야 하면 홈 데이터를 기다리지 않고 바로 고지를 띄운다 — 홈은 고지 뒤에서 채워진다. 예전엔 느린 망에서
  // 8초 안전 타이머(onboarding.js)가 시작 화면을 먼저 걷어, 그림을 기다리는 동안 고지 없이 홈을 누를 수 있었다.
  const askNotice = needsNotice(settings && settings.general);
  noticePending = askNotice; // 고지를 띄우기 전 틈(홈 이동 등)에도 트레이 동작을 막는다
  if (!askNotice) await loadHomeBehindSplash();
  hideSplash();
  // 처음 실행(또는 고지 문구가 바뀐 뒤) — 비영리·비공식·오픈소스 고지에 동의해야 이어 간다. 동의하지 않으면 앱을 끝낸다.
  // 환영 안내는 동의한 뒤에 뜬다(두 창이 겹치지 않게).
  if (askNotice) await showNotice();
  noticePending = false;
  showWelcome({ onLogin: login });
  flushSharpnessNotice(); // 선명도 이전 안내(한 번) — 시작 화면 뒤에 가려지지 않게 여기서
  // 새 버전 알림 — 메인 창에서만. 조용히 확인하고 사이드바 줄·토스트로 알린다(창·포커스는 건드리지 않음).
  installUpdateNotifier(() => showUpdateCheck());
  // 딥 링크 (SYS-06) — 준비 전에 온 링크가 있으면 작업 공간 복원보다 우선한다.
  listen(Events.deepLink, (p) => { api.deeplinkTakePending().catch(() => {}); openDeepLink(p); });
  let pendingLinks = [];
  try { pendingLinks = await api.deeplinkTakePending(); } catch { /* 없음 */ }
  if (pendingLinks.length) { for (const p of pendingLinks) openDeepLink(p); return; }
  if (ws && ws.route !== 'home') navigate(ws.route, ws.params || {});
  if (ws && ws.channelId) {
    toast('지난번에 보던 방송이 있어요', 8000, { label: '이어 보기', run: () => openChannel(ws.channelId) });
  }
}

// 고지가 떠 있는 동안 무시하는 트레이·메뉴 막대 동작 — 화면 이동·멀티라이브 추가·업데이트 시트. 재생 제어는 그대로 둔다.
const MAIN_MENU_IDS = new Set(['licenses', 'notice', 'about', 'check-update', 'settings', 'notification-settings']);
const TRAY_BLOCKED_BY_NOTICE = new Set(['navigate', 'show-now-playing', 'queue-multi', 'check-update']);

// 시작 로딩 화면이 홈 데이터를 기다리는 한도 — 네트워크가 느려도 이만큼 지나면 홈(스켈레톤)을 먼저 보여 주고
// 나머지는 홈이 스스로 채운다. 첫 화면 그림은 데이터가 온 뒤 조금만 더 기다린다(늦는 그림은 홈에서 떠오른다).
const STARTUP_DATA_MAX_MS = 8000;
const STARTUP_SLOW_MS = 3500;
const STARTUP_IMAGES_MAX_MS = 1500;
const STARTUP_LABELS = { following: '팔로잉 방송', popular: '인기 라이브', recent: '최근 시청' };

/** 앱 시작 — 홈의 첫 데이터(팔로잉·인기·최근 시청)와 첫 화면 썸네일이 준비될 때까지 로딩 화면에 진행을 보여 준다. */
async function loadHomeBehindSplash() {
  const home = pages.home;
  // 오프라인이면 기다릴 것이 없다 — 오프라인 배너와 홈의 실패 안내가 바로 보이게.
  if (!home || !isTauri || navigator.onLine === false) return;
  const pending = new Set(isLoggedIn() ? ['following', 'popular', 'recent'] : ['popular', 'recent']);
  const total = pending.size;
  let slow = false;
  const label = () => `${[...pending].map((k) => STARTUP_LABELS[k]).join(' · ')} 불러오는 중…`;
  const progress = () => 0.45 + 0.4 * ((total - pending.size) / total);
  home.onLoadStep = (name) => {
    if (!pending.delete(name)) return;
    if (pending.size) splashStep(slow ? '네트워크가 느려요 — 조금만 기다려 주세요' : label(), progress());
  };
  splashStep(label(), progress());
  const slowTimer = setTimeout(() => { slow = true; splashStep('네트워크가 느려요 — 조금만 기다려 주세요', progress()); }, STARTUP_SLOW_MS);
  const timeout = (ms) => new Promise((resolve) => setTimeout(() => resolve('timeout'), ms));
  const dataResult = await Promise.race([home.firstRender, timeout(STARTUP_DATA_MAX_MS)]);
  clearTimeout(slowTimer);
  home.onLoadStep = null;
  if (dataResult === 'timeout') return;
  // 첫 화면에 보이는 썸네일 — 막 그린 홈 위에서 그림이 하나씩 떠오르지 않게 잠깐 더 기다린다.
  splashStep('화면 그리는 중…', 0.9);
  // 배치(크기·위치)가 정해진 뒤 고른다 — 두 프레임: 첫 프레임의 ResizeObserver 가 홈 맨 위 "방송 중인 팔로잉"의 접힌
  // 칸(칸 수에 맞춰 숨김)을 고친 뒤여야 한다. 한 프레임 뒤에 고르면 곧 숨겨질 칸의 아바타(lazy — 숨으면 받지 않는다)까지
  // 기다려 시작 화면이 그림 상한(1.5초)을 다 채웠다.
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
  const images = home.aboveFoldImages().filter((img) => !img.complete);
  if (!images.length) return;
  const loaded = images.map((img) => new Promise((resolve) => {
    img.addEventListener('load', resolve, { once: true });
    img.addEventListener('error', resolve, { once: true });
  }));
  await Promise.race([Promise.all(loaded), timeout(STARTUP_IMAGES_MAX_MS)]);
}

/** 보조 창 첫 화면 (PLY-18·CHT-23·MUL-05·DAT-05). */
function openAuxWindow() {
  const q = new URLSearchParams(location.search);
  const channelId = q.get('channelId');
  if (AUX === 'player') { if (channelId) openChannel(channelId); else navigate('live', {}, { push: false }); }
  else if (AUX === 'chat') navigate('chat', channelId ? { channelId } : {}, { push: false });
  else if (AUX === 'multichat') navigate('multichat', {}, { push: false });
  else if (AUX === 'stats') navigate('stats', {}, { push: false });
}

/** `cview://` 링크 열기 — 원본 DeepLinkRouter.apply. */
const HUB_ROUTE = { 탐색: 'following', 시청: 'live', 멀티: 'multilive', 검색: 'search', 인기: 'popular', 카테고리: 'categories' };
function openDeepLink(p) {
  if (!p || !p.kind) return;
  switch (p.kind) {
    case 'live':
      if (p.mode === 'multi') addToMultiLive(p.channelId);
      else if (p.mode === 'chatonly') navigate('chat', { channelId: p.channelId });
      else openChannel(p.channelId);
      break;
    case 'channel': openChannelPage(p.channelId); break;
    case 'following': if (HUB_ROUTE[p.mode] === 'live') openLiveMenu(); else navigate(HUB_ROUTE[p.mode] || 'following'); break;
    case 'popular': navigate('popular', p.tab ? { tab: p.tab } : {}); break;
    case 'multilive': navigate('multilive'); break;
    case 'home': navigate('home'); break;
    case 'search': navigate('search', p.query ? { query: p.query } : {}); break;
    case 'clip': navigate('media', { kind: 'clip', id: p.uid }); break;
    case 'login': if (!isLoggedIn()) login(); break;
    default: break;
  }
}

/** 여러 채널을 멀티라이브로 차례로 연다(NAV-05 스마트 큐·사이드바 빠른 동작). */
async function queueMulti(channelIds) {
  if (!channelIds.length) return;
  if (current !== 'multilive') await navigate('multilive');
  for (const id of channelIds) {
    try { await pages.multilive.add(id); } catch { /* 한도 초과 등은 add 가 알린다 */ }
  }
}

// 시작 중 예외가 나면 로딩 화면이 걷히지 않고 영원히 남았다(누구도 거부를 받지 않았다) — 걷고 다시 불러오기를 권한다.
if (window.__cviewBoot?.ready() !== false) {
  main().catch((e) => {
    console.error('CView 시작 실패', e);
    hideSplash();
    toast('화면을 준비하지 못했습니다', 600000, { label: '다시 불러오기', run: () => location.reload() });
  });
}
