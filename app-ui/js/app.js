// CView 메인 — 사이드바 라우팅, 인증 상태, 시청 화면 연결.
import { api, listen, Events, isTauri } from './api.js';
import { toast, icons } from './dom.js';
import { showShortcuts, showAbout, showUpdateCheck } from './app-sheets.js';
import { installImageFade, installSegmentGlider, refreshSegmentGliders, installMicroMotion } from './motion.js';
import { installHoverPreview } from './hover-preview.js';
import { bindCardOpen } from './cards.js';
import { FollowingPage, PopularPage, SearchPage } from './pages.js';
import { SettingsPage } from './settings.js';
import { Viewer } from './viewer.js';
import { HomePage } from './home.js';
import { Sidebar } from './sidebar.js';
import { setTheme, syncTheme, resolvedTheme } from './theme.js';
import { loadChatPrefs, setMyNickname, chatPrefs, updateChatPrefs } from './chat-prefs.js';
import { MultiLivePage, multiLiveIcon, bindMultiLiveAddButtons } from './multilive.js';
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
import { HUBS, saveWorkspace, loadWorkspace, installSidebarExtras, hubFlash } from './workspace.js';
import { CategoriesPage, categoryIcon } from './categories.js';
import { CommandPalette } from './palette.js';
import { ChatOnlyPage } from './chat-only.js';
import { MultiChatPage, multiChatIcon } from './multichat.js';
import { installPrefsEffects } from './prefs-effects.js';
import { installBackgroundSaver, setMenuHidden } from './background-saver.js';
import { installPerfCollector } from './perf-collector.js';
import { showMenu, refreshChatAuth } from './chat.js';
import { kbd } from './platform.js';
import { installWindowControls } from './window-controls.js';

// 사이드바 — Swift `AppRouter.SidebarItem` 라벨. `sidebar: false` 는 사이드바에 없고 다른 메뉴 속에서 여는 화면
// (멀티라이브는 라이브 메뉴의 한 보기 — 머리줄 전환 버튼으로 오간다). ⌘K 팔레트에는 그대로 나온다.
const ROUTES = [
  { id: 'home', title: '홈', icon: icons.home },
  { id: 'following', title: '팔로잉', icon: icons.following },
  { id: 'popular', title: '인기', icon: icons.popular },
  { id: 'categories', title: '카테고리', icon: categoryIcon },
  { id: 'live', title: '라이브', icon: liveIcon },
  { id: 'multilive', title: '멀티라이브', icon: multiLiveIcon, sidebar: false },
  { id: 'multichat', title: '멀티채팅', icon: multiChatIcon },
  { id: 'library', title: '보관함', icon: libraryIcon },
  { id: 'stats', title: '통계', icon: statsIcon },
  { id: 'search', title: '검색', icon: icons.search, section: '탐색' },
  { id: 'settings', title: '설정', icon: icons.settings, section: '앱' },
];

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
}

// 사이드바 "방송 중" 목록 — Rust FollowingStore 가 60초 안의 결과를 재사용하므로 주기 호출이 가볍다.
const SIDEBAR_FOLLOWING_MS = 120000;
let followingTimer = 0;
async function refreshSidebarFollowing() {
  clearTimeout(followingTimer);
  if (!isLoggedIn()) {
    sidebar.setFollowing(null);
    return;
  }
  try {
    // 즐겨찾기 채널을 위로 (DAT-02)
    const [lives, favs] = await Promise.all([api.followingLives(false), api.favoritesList().catch(() => [])]);
    const fav = new Set(favs.map((f) => f.channelId));
    sidebar.setFollowing([...lives].sort((a, b) => (fav.has(b.channelId) - fav.has(a.channelId)) || ((b.viewerCount || 0) - (a.viewerCount || 0))));
  } catch {
    sidebar.setFollowing(null);
  }
  // 여러 호출이 겹쳐도 주기 타이머는 하나만(예전엔 겹친 호출마다 타이머가 늘었다).
  clearTimeout(followingTimer);
  followingTimer = setTimeout(refreshSidebarFollowing, SIDEBAR_FOLLOWING_MS);
}

// 단독 라이브와 멀티라이브는 동시에 소리를 내지 않는다 — 마지막으로 연 쪽이 소리를 가진다.
let audioOwner = null;
function setAudioOwner(owner) {
  if (owner === audioOwner) return;
  audioOwner = owner;
  if (owner === 'live') {
    viewer.suspendAudio(false);
    api.multiLiveSetAudioSuspended(true).catch(() => {});
  } else if (owner === 'multilive') {
    viewer.suspendAudio(true);
    api.multiLiveSetAudioSuspended(false).catch(() => {});
  }
}

// ── 라이브 ↔ 멀티라이브 연동 (원본 시청/멀티 허브 모드) ──────────────
// - 멀티라이브에 있는 채널을 라이브로 보면 같은 세션(플레이어·채팅 소켓)을 무대에 올린다 — 다시 연결 없음.
//   무대가 있는 동안 매니저가 나머지 패인의 로딩을 멈추고, 멀티라이브로 돌아가면 배치 그대로 되살린다.
// - 멀티라이브에 없는 채널을 라이브 시청 화면(별도 플레이어)으로 보는 동안 멀티라이브 패인은 접어 둔다
//   (일시정지 → 2.5초 뒤 로딩 정지). 같은 채널이 두 곳에서 동시에 재생되지 않는다.
const isWatching = () => Boolean(pages.multilive && pages.multilive.watchSlot != null);
let multiLiveParked = false;
function setMultiLiveParked(on) {
  const hasSessions = Boolean(pages.multilive && (pages.multilive.state.sessions || []).length);
  if (on && !hasSessions) return;
  if (on === multiLiveParked) return;
  multiLiveParked = on;
  api.multiLivePark(on).catch(() => {});
}
function multiLiveSessionFor(channelId) {
  return (pages.multilive.state.sessions || []).find((x) => x.channelId === channelId && x.slot != null) || null;
}
// 채널을 무대에 올리는 중(openChannel·enterWatch) — 그 사이 오는 상태 알림이 화면을 멀티라이브로 끌고 가지 않게.
let pendingWatch = 0;
async function enterWatch(sessionId, { push = true } = {}) {
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
/** 라이브 메뉴인데 목록(LivePage) 대신 무대·예전 시청 화면이 보이는 중 — 숨은 목록은 새로 불러오지 않는다. */
function liveListHidden() {
  return current === 'live' && Boolean((viewer && viewer.isPlaying) || isWatching());
}
/** 라이브 메뉴 단독 보기가 앞에 떠 있으면 그 도구(ml-watch.js) — 메뉴 동작·단축키가 예전 시청 화면 대신 이리로 간다. */
function activeWatch() {
  return current === 'live' && !viewer.isOpen && isWatching() ? pages.multilive.watch : null;
}
// 단독 보기를 닫은 뒤 보던 화면으로 돌아갈지 — 멀티라이브 상태 알림(남은 패인이 있으면 멀티라이브로)보다 먼저 본다.
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

async function navigate(route, params = {}, { push = true } = {}) {
  if (!pages[route]) return;
  if (push && current && !(current === route && JSON.stringify(currentParams) === JSON.stringify(params))) {
    backStack.push({ route: current, params: currentParams });
    if (backStack.length > BACK_MAX) backStack.shift();
  }
  current = route;
  currentParams = params;
  if (route === 'multilive') keepTransient(); // 멀티라이브 화면에서 본 패인은 남긴다
  if (LIVE_MODES.includes(route)) lastLiveMode = route;
  sidebar.setActive(LIVE_MODES.includes(route) ? 'live' : route);
  const watching = isWatching();
  if (insights) insights.show(route === 'live' && viewer && (viewer.isPlaying || watching) ? 'live' : route);
  if (!AUX) saveWorkspace(route, params, viewer && viewer.isPlaying ? viewer.channelId : null);
  // 라이브 메뉴는 재생 중이면 시청 화면, 아니면 이어 볼 채널 목록이다.
  // 멀티라이브에 있는 채널을 라이브로 볼 때(시청 모드)는 멀티라이브 화면을 단독 무대로 보여 준다.
  const showViewer = route === 'live' && viewer.isPlaying;
  const showWatch = route === 'live' && !showViewer && watching;
  for (const [id, page] of Object.entries(pages)) {
    page.root.hidden = showViewer || id !== (showWatch ? 'multilive' : route);
  }
  refreshSegmentGliders(); // 숨어 있는 동안 고른 칸이 바뀐 세그먼트(전환 버튼 등)의 판을 제자리로
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
  if (pages[route].acceptsParams) pages[route].show(params);
  else pages[route].show();
}

/** 뒤로 가기 (`⌘[`, 마우스 뒤로 버튼). 스택이 비면 홈. */
function goBack() {
  const prev = backStack.pop();
  if (prev) navigate(prev.route, prev.params, { push: false });
  else if (current !== 'home') navigate('home', {}, { push: false });
}

function openChannelPage(channelId) {
  if (channelId) navigate('channel', { channelId });
}

function onViewerClosed() {
  if (AUX === 'player') { api.windowCloseSelf().catch(() => {}); return; }
  syncLiveIndicators();
  setMenuHidden(viewer.player.iframe, false);
  api.multiLiveSetAudioSuspended(false).catch(() => {});
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
  if (wasLoggedIn !== isLoggedIn()) refreshSidebarFollowing();
  if (viewer) viewer.refreshAuth();
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
  if (AUX || !pages.multilive) { await openInViewer(channelId); return; }
  pendingWatch++;
  try { await openChannelWatch(channelId); } finally { pendingWatch--; syncLiveIndicators(); }
}
async function openChannelWatch(channelId) {
  // 이미 멀티라이브에 있는 채널 — 같은 플레이어를 라이브 무대로(원본 commitExistingWatch).
  const existing = multiLiveSessionFor(channelId);
  if (existing) { await enterWatch(existing.id); return; }
  const st = pages.multilive.state;
  // 라이브 단독 보기 상한 — 멀티라이브 상한과 따로다(지금 보기가 멀티라이브여도 라이브로 여는 추가는 이쪽).
  if ((st.canAddLive ?? st.canAdd) === false) {
    // 가득 참 — 가장 오래전에 둘러본 채널을 먼저 빼고, 없으면(모두 직접 넣은 채널) 예전 시청 화면으로
    const prev = oldestBrowsed();
    if (!prev) { await openInViewer(channelId); return; }
    browsedWatch.delete(prev);
    await pages.multilive.run(api.multiLiveRemove(prev));
  }
  await pages.multilive.add(channelId, { quiet: true, live: true });
  const s = multiLiveSessionFor(channelId);
  if (!s) return; // 추가 실패 — run 이 이미 알렸다
  browsedWatch.set(s.id, channelId);
  await enterWatch(s.id);
}

/** 예전 시청 화면으로 — 보조 플레이어 창, 멀티라이브가 가득 찼을 때. */
async function openInViewer(channelId) {
  if (isWatching()) await exitWatch();
  let player = null;
  try {
    player = (await api.settingsGet()).player;
  } catch { /* 기본값 */ }
  viewer.open(channelId, player);
  syncLiveIndicators();
  audioOwner = null;
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
  const muted = handoff && viewer.player.isMuted();
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
  if (s.id !== pages.multilive.state.selectedId) await pages.multilive.run(api.multiLiveSelect(s.id));
  const pane = pages.multilive.panes.get(s.slot);
  // 음소거였으면 패인도 끈 채로(소리 버튼을 누르면 원래 크기로).
  if (pane && muted) {
    pane.controls.lastGain = 1;
    pane.controls.range.value = '0';
    pane.controls.setGain(0);
  }
  syncLiveIndicators();
}

function installViewerMultiLiveButton() {
  const close = document.getElementById('viewer-close');
  const button = document.createElement('button');
  button.className = 'icon-btn';
  button.title = '멀티라이브에 추가';
  button.innerHTML = multiLiveIcon;
  button.addEventListener('click', () => {
    const channelId = viewer.channelId;
    if (channelId) addToMultiLive(channelId);
  });
  close.before(button);
  // 새 플레이어 창으로 분리 (PLY-18, ⌘N) — 보조 창 안에서는 숨긴다.
  if (!AUX) {
    const pop = document.createElement('button');
    pop.className = 'icon-btn';
    pop.title = kbd('새 창으로 분리 (⌘N)');
    pop.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="13" height="13" rx="2"/><path d="M10 4h10v10M20 4l-8 8"/></svg>';
    pop.addEventListener('click', () => MENU_ACTIONS['window-player']());
    button.before(pop);
  }
}

/** 카드 우클릭 메뉴 — 원본 LiveCardActionMenu (NAV-10). */
function installCardMenu() {
  document.addEventListener('contextmenu', async (e) => {
    const card = e.target.closest('[data-channel-id]');
    if (!card || e.target.closest('.chat') || card.closest('.ml-stage')) return;
    e.preventDefault();
    const id = card.dataset.channelId;
    const name = card.dataset.channelName || '';
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
        try { await api.favoriteSet(id, !isFav, name, image); toast(isFav ? '즐겨찾기에서 뺐습니다' : '즐겨찾기에 추가했습니다', 1500); refreshSidebarFollowing(); } catch (er) { toast(er.message); }
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
    onSearch: (q) => navigate('search', { query: q }),
    actions: () => [
      ...ROUTES.map((r) => ({ group: '화면', label: r.title, hint: '이동', run: () => (r.id === 'live' ? openLiveMenu() : navigate(r.id)) })),
      { group: '화면', label: '검색', hint: '라이브·채널·다시보기', run: () => navigate('search') },
      { group: '보관함', label: '인기 클립', hint: '보관함', run: () => navigate('library', { tab: 'clips' }) },
      { group: '보관함', label: '이어 보기', hint: '다시보기', run: () => navigate('library', { tab: 'continue' }) },
      { group: '보관함', label: '즐겨찾기', hint: '채널', run: () => navigate('library', { tab: 'favorites' }) },
      { group: '보관함', label: '북마크', hint: '방송 장면', run: () => navigate('library', { tab: 'bookmarks' }) },
      { group: '통계', label: '시청 통계', hint: '통계', run: () => navigate('stats') },
      { group: '통계', label: '데이터 관리 · 백업', hint: '저장소', run: () => navigate('stats', { tab: 'data' }) },
      { group: '동작', label: resolvedTheme() === 'dark' ? '라이트 테마로 전환' : '다크 테마로 전환', hint: '테마', run: () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark') },
      { group: '동작', label: '뒤로 가기', hint: kbd('⌘['), run: goBack },
      { group: '동작', label: '사이드바 접기/펼치기', hint: kbd('⌘\\'), run: () => sidebar.toggle() },
      { group: '동작', label: '현재 화면 새로고침', hint: kbd('⌘R'), run: () => pages[current] && (pages[current].acceptsParams ? pages[current].show(currentParams) : pages[current].show(true)) },
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
const ESC_POPUPS = ':is(.quality-menu, .player-panel, .emoji-picker, .chat-tray, .chat-autocomplete, .ml-pane-menu, .ml-more-menu):not([hidden], .exit-ghost)';
const ESC_LAYERS = '.sheet-scrim:not(.leaving), .palette-scrim:not([hidden], .exit-ghost)';
function escapeTaken(e) {
  if (e.defaultPrevented) return true;
  const popups = document.querySelectorAll(ESC_POPUPS);
  if (popups.length) { for (const p of popups) p.hidden = true; return true; }
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
  return null;
}

const MENU_ACTIONS = {
  'nav-home': () => navigate('home'),
  'nav-following': () => navigate('following'),
  'nav-popular': () => navigate('popular'),
  'nav-multilive': () => navigate('multilive'),
  'nav-live': () => openLiveMenu(),
  'nav-library': () => navigate('library'),
  'nav-categories': () => navigate('categories'),
  search: () => navigate('search'),
  palette: () => (palette.isOpen ? palette.close() : palette.open()),
  back: () => goBack(),
  // 라이브 메뉴에서 무대·시청 화면이 앞에 있으면 뒤에 숨은 목록은 다시 불러오지 않는다.
  refresh: () => pages[current] && !liveListHidden() && (pages[current].acceptsParams ? pages[current].show({ ...currentParams, refresh: true }) : pages[current].show(true)),
  'toggle-sidebar': () => sidebar.toggle(),
  'theme-toggle': () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark'),
  settings: () => navigate('settings'),
  about: () => showAbout(),
  'check-update': () => showUpdateCheck(),
  shortcuts: () => showShortcuts(),
  'report-issue': () => api.openExternal('https://github.com/dododo-lab/cview-rust/issues').catch((e) => toast(e.message)),
  'notification-settings': () => api.openNotificationSettings().catch((e) => toast(e.message)),
  'open-logs': async () => { try { const i = await api.appInfo(); await api.revealPath(i.dataDir); } catch (e) { toast(e.message); } },
  'play-pause': () => (viewer.isOpen ? viewer.controls.togglePlay() : activeWatch()?.togglePlay()),
  mute: () => (viewer.isOpen ? viewer.toggleMute() : activeWatch()?.toggleMute()),
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
  fullscreen: () => (viewer.isOpen ? viewer.controls.fullscreen(false) : activeWatch()?.fullscreen(false)),
  'fullscreen-chat': () => (viewer.isOpen ? viewer.controls.fullscreen(true) : activeWatch()?.fullscreen(true)),
  'close-viewer': () => (viewer.isOpen ? viewer.close() : activeWatch() && closeWatch()),
  'chat-clear': () => activeChat()?.clear(true),
  'chat-reconnect': () => activeChat()?.reconnect(),
  'chat-autoscroll': () => {
    const on = !chatPrefs().autoScroll;
    updateChatPrefs({ autoScroll: on });
    toast(on ? '자동 스크롤 켬' : '자동 스크롤 끔', 1200);
  },
};

function installMenuAndShortcuts() {
  // 메뉴 이벤트는 모든 창에 간다 — 앞에 있는 창만 실행한다.
  listen('menu://action', (p) => {
    const run = p && MENU_ACTIONS[p.id];
    if (run && document.hasFocus()) run();
  });
  // 메뉴가 없는 환경(브라우저 미리보기 등)과 ⌘/ 보조.
  document.addEventListener('keydown', (e) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    if (e.altKey && e.metaKey) {
      const hub = HUBS.find(([k]) => e.code === `Digit${k}`);
      if (hub) { e.preventDefault(); if (hub[1] === 'live') openLiveMenu(); else navigate(hub[1]); hubFlash(hub[2]); return; }
    }
    if (e.key === '/') { e.preventDefault(); showShortcuts(); }
    else if (e.key === ',') { e.preventDefault(); navigate('settings'); }
  });
  // 오프라인 배너 (SYS-12)
  const banner = document.createElement('div');
  banner.className = 'offline-banner';
  banner.innerHTML = '<span>오프라인입니다 — 네트워크가 돌아오면 자동으로 다시 불러옵니다</span>';
  banner.hidden = navigator.onLine !== false;
  document.querySelector('.content').prepend(banner);
  window.addEventListener('offline', () => { banner.hidden = false; });
  window.addEventListener('online', () => {
    banner.hidden = true;
    toast('다시 연결되었습니다', 1500);
    if (pages[current] && !viewer.isOpen) MENU_ACTIONS.refresh();
  });
}

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
  installImageFade(); // 첫 화면 그림보다 먼저 — 놓친 load 로 썸네일이 투명하게 남지 않게
  installSegmentGlider(); // 모든 세그먼트의 고른 칸 판이 미끄러지게
  installMicroMotion();
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
  pages.multilive = new MultiLivePage(content, { isLoggedIn, onAddChannel: addToMultiLive, onOpenChannel: watchFromDrawer });
  pages.live = new LivePage(content, { isLoggedIn, modeSwitch: !AUX });
  pages.categories = new CategoriesPage(content);
  pages.multichat = new MultiChatPage(content, { isLoggedIn, onPlay: openChannel });
  pages.chat = new ChatOnlyPage(content, { isLoggedIn, onBack: goBack, onPlay: openChannel, session: AUX === 'chat' ? `win-${Date.now()}` : null, standalone: AUX === 'chat' });
  pages.library = new LibraryPage(content, { isLoggedIn, onOpenChannel: openChannelPage });
  pages.stats = new StatsPage(content, { onOpenChannel: openChannelPage });
  pages.media = new MediaPage(content, { onBack: goBack, onOpenChannel: openChannelPage });
  pages.channel = new ChannelPage(content, { onBack: goBack, onPlay: openChannel, onAddMulti: addToMultiLive });
  viewer = new Viewer(document.getElementById('viewer'), { isLoggedIn, onClose: onViewerClosed });
  if (!AUX) insights = new InsightPanel(content, { isLoggedIn });
  bindMultiLiveAddButtons(addToMultiLive);
  installViewerMultiLiveButton();
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

  for (const p of Object.values(pages)) {
    const body = p.root.querySelector('.page-body');
    if (body) bindCardOpen(body, openChannel);
  }
  bindCardOpen(document.getElementById('sidebar-live'), openChannel);
  document.addEventListener('click', (e) => {
    const action = e.target.closest('[data-action]');
    if (!action) return;
    if (action.dataset.action === 'login' && !action.closest('[data-page="settings"]')) login();
    if (action.dataset.action === 'logout' && !action.closest('[data-page="settings"]')) logout();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !document.fullscreenElement) {
      if (viewer.isOpen && !escapeTaken(e)) viewer.close();
      else if (activeWatch() && !escapeTaken(e)) closeWatch();
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
  window.addEventListener('cview:menu', (e) => { const run = MENU_ACTIONS[e.detail]; if (run) run(); });
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
    else if (page.acceptsParams) page.show(currentParams);
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
  loadChatPrefs();
  loadShortcuts();
  listen(Events.loginFailed, (p) => p && p.message && toast(p.message));

  renderAccount();
  splashStep('계정·설정 확인', 0.55);
  if (!isTauri) toast('CView 앱(cargo run -p cview-app) 안에서 실행해야 데이터가 표시됩니다', 6000);
  // 계정 상태와 설정은 서로 기다릴 필요가 없어 함께 부른다(예전엔 차례로 기다렸다).
  const [authRes, settingsRes] = await Promise.allSettled(isTauri ? [api.authStatus(), api.settingsGet()] : []);
  if (authRes && authRes.status === 'fulfilled') onAuthChanged(authRes.value);
  const settings = settingsRes && settingsRes.status === 'fulfilled' ? settingsRes.value : null;
  syncTheme(settings && settings.appearance && settings.appearance.theme);
  // 스플래시는 hideSplash 가 최소 0.4초를 지킨다 — 따로 더 기다리지 않는다.
  hideSplash();
  if (!AUX) showWelcome({ onLogin: login });
  if (AUX) { openAuxWindow(); return; }
  // 작업 공간 복원 (NAV-16) — "실행 시 창 복원"이 켜져 있으면 마지막 화면으로. 방송은 자동 재생하지 않고 묻는다.
  const ws = settings?.general?.restoreWindowOnLaunch !== false ? loadWorkspace() : null;
  navigate('home', {}, { push: false });
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

main();
