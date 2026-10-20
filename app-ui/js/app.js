// CView 메인 — 사이드바 라우팅, 인증 상태, 시청 화면 연결.
import { api, listen, Events, isTauri } from './api.js';
import { toast, icons } from './dom.js';
import { bindCardOpen } from './cards.js';
import { FollowingPage, PopularPage, SearchPage } from './pages.js';
import { SettingsPage } from './settings.js';
import { Viewer } from './viewer.js';
import { HomePage } from './home.js';
import { Sidebar } from './sidebar.js';
import { setTheme, syncTheme, resolvedTheme, onThemeChange } from './theme.js';
import { loadChatPrefs, setMyNickname } from './chat-prefs.js';
import { MultiLivePage, multiLiveIcon, bindMultiLiveAddButtons } from './multilive.js';
import { LivePage, liveIcon } from './live.js';
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
import { showMenu } from './chat.js';

// 사이드바 — Swift `AppRouter.SidebarItem` 라벨.
const ROUTES = [
  { id: 'home', title: '홈', icon: icons.home },
  { id: 'following', title: '팔로잉', icon: icons.following },
  { id: 'popular', title: '인기', icon: icons.popular },
  { id: 'categories', title: '카테고리', icon: categoryIcon },
  { id: 'live', title: '라이브', icon: liveIcon },
  { id: 'multilive', title: '멀티라이브', icon: multiLiveIcon },
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

const ICON_SUN = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>';
const ICON_MOON = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M20.5 14.4A8.5 8.5 0 0 1 9.6 3.5a8.5 8.5 0 1 0 10.9 10.9Z"/></svg>';

function renderNav() {
  sidebar = new Sidebar(document.querySelector('.sidebar'), { routes: ROUTES, onNavigate: navigate });
  installThemeToggle();
}

/** 사이드바 위쪽 테마 빠른 전환 — 지금 보이는 테마의 반대로 바꾼다. */
function installThemeToggle() {
  const button = document.createElement('button');
  button.className = 'icon-btn theme-toggle';
  const render = () => {
    const dark = resolvedTheme() === 'dark';
    button.innerHTML = dark ? ICON_SUN : ICON_MOON;
    const label = dark ? '라이트 테마로' : '다크 테마로';
    button.title = label;
    button.setAttribute('aria-label', label);
    button.dataset.tip = label;
  };
  button.addEventListener('click', () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark'));
  onThemeChange(render);
  render();
  const toggle = document.querySelector('.sidebar .sidebar-toggle');
  if (toggle) toggle.before(button);
  else document.querySelector('.sidebar .brand-row').append(button);
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
async function enterWatch(sessionId) {
  if (viewer.isPlaying) await viewer.close({ silent: true });
  try {
    pages.multilive.render(await api.multiLiveWatch(sessionId));
  } catch (e) { toast(e.message); return; }
  syncLiveIndicators();
  await navigate('live');
}
async function exitWatch() {
  try { pages.multilive.render(await api.multiLiveWatch(null)); } catch { /* 앱 밖 */ }
  syncLiveIndicators();
}
/** 사이드바 — 라이브는 재생 중 점, 멀티라이브는 패인 수. */
function syncLiveIndicators() {
  sidebar.setPlaying('live', Boolean(viewer && viewer.isPlaying) || isWatching());
  sidebar.setBadge('multilive', (pages.multilive && (pages.multilive.state.sessions || []).length) || 0);
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
  sidebar.setActive(route);
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
  if (showViewer) {
    setMenuHidden(viewer.player.iframe, false);
    viewer.reveal();
    setAudioOwner('live');
    setMultiLiveParked(true);
    return;
  }
  viewer.hide();
  if (showWatch) {
    setMultiLiveParked(false);
    setAudioOwner('multilive');
    pages.multilive.show();
    return;
  }
  if (route === 'multilive') {
    if (watching) await exitWatch();
    // 소리만 꺼진 채 뒤에서 도는 라이브 시청 화면은 화질을 낮춰 디코딩을 줄인다.
    if (viewer.isPlaying) setMenuHidden(viewer.player.iframe, true);
    setMultiLiveParked(false);
    setAudioOwner('multilive');
    pages.multilive.chatKey = null;
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
  auth = status || { loggedIn: false, state: 'loggedOut' };
  setMyNickname(auth.loggedIn ? auth.nickname : '');
  renderAccount();
  if (wasLoggedIn !== isLoggedIn()) refreshSidebarFollowing();
  if (viewer) viewer.refreshAuth();
  if (wasLoggedIn !== isLoggedIn() && current && current !== 'search' && !(viewer && viewer.isOpen)) {
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

async function openChannel(channelId) {
  // 이미 멀티라이브에 있는 채널 — 같은 플레이어를 라이브 무대로(원본 commitExistingWatch).
  const existing = !AUX && pages.multilive ? multiLiveSessionFor(channelId) : null;
  if (existing) { await enterWatch(existing.id); return; }
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

/** 카드·시청 화면의 "멀티라이브에 추가" — 멀티라이브 화면으로 옮겨 패인을 띄운다. */
async function addToMultiLive(channelId) {
  // 라이브 시청 화면의 채널이면 넘겨준다 — 시청 화면을 닫고(같은 채널 두 번 재생 없음) 볼륨을 이어받아 선택 패인으로.
  const handoff = viewer.isPlaying && viewer.channelId === channelId;
  const volume = handoff ? (viewer.player.isMuted() ? 0 : Number(viewer.volume.value) / 100) : null;
  if (handoff) await viewer.close({ silent: true });
  if (current !== 'multilive') await navigate('multilive');
  await pages.multilive.add(channelId);
  const s = multiLiveSessionFor(channelId);
  if (!s) return;
  if (s.id !== pages.multilive.state.selectedId) await pages.multilive.run(api.multiLiveSelect(s.id));
  const pane = pages.multilive.panes.get(s.slot);
  if (pane && volume != null) {
    pane.controls.range.value = String(Math.round(volume * 100));
    pane.controls.setGain(volume);
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
    pop.title = '새 창으로 분리 (⌘N)';
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
      } else if (a === 'copy') navigator.clipboard.writeText(url).then(() => toast('링크를 복사했습니다', 1200));
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
      ...ROUTES.map((r) => ({ group: '화면', label: r.title, hint: '이동', run: () => navigate(r.id) })),
      { group: '화면', label: '검색', hint: '라이브·채널·다시보기', run: () => navigate('search') },
      { group: '보관함', label: '인기 클립', hint: '보관함', run: () => navigate('library', { tab: 'clips' }) },
      { group: '보관함', label: '이어 보기', hint: '다시보기', run: () => navigate('library', { tab: 'continue' }) },
      { group: '보관함', label: '즐겨찾기', hint: '채널', run: () => navigate('library', { tab: 'favorites' }) },
      { group: '보관함', label: '북마크', hint: '방송 장면', run: () => navigate('library', { tab: 'bookmarks' }) },
      { group: '통계', label: '시청 통계', hint: '통계', run: () => navigate('stats') },
      { group: '통계', label: '데이터 관리 · 백업', hint: '저장소', run: () => navigate('stats', { tab: 'data' }) },
      { group: '동작', label: resolvedTheme() === 'dark' ? '라이트 테마로 전환' : '다크 테마로 전환', hint: '테마', run: () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark') },
      { group: '동작', label: '뒤로 가기', hint: '⌘[', run: goBack },
      { group: '동작', label: '사이드바 접기/펼치기', hint: '⌘\\', run: () => sidebar.toggle() },
      { group: '동작', label: '현재 화면 새로고침', hint: '⌘R', run: () => pages[current] && (pages[current].acceptsParams ? pages[current].show(currentParams) : pages[current].show(true)) },
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

const SHORTCUTS = [
  ['탐색', [['⌘1 ~ ⌘7', '홈 · 팔로잉 · 인기 · 멀티라이브 · 라이브 · 보관함 · 카테고리'], ['⌘K', '명령 팔레트'], ['⌘F', '검색'], ['⌘[  /  ⌥←', '뒤로'], ['⌘R', '새로고침'], ['⌘\\', '사이드바 접기'], ['⇧⌘L', '다크/라이트 전환'], ['⌘,', '설정']]],
  ['시청', [['Space', '재생/일시정지'], ['M', '음소거'], ['↑ ↓', '볼륨'], ['L', '라이브로 이동'], ['B / ⇧⌘B', '북마크'], ['⌘S', '스크린샷'], ['⇧⌘F', '영상 전체화면'], ['⌃⌘F', '영상+채팅 전체화면'], ['⌘W / Esc', '시청 화면 닫기']]],
  ['채팅', [['⇧⌘K', '채팅 지우기'], ['⌘J', '자동 스크롤'], [':이름', '이모티콘 자동완성'], ['@닉네임', '멘션 자동완성'], ['우클릭', '메시지 메뉴'], ['더블클릭', '프로필']]],
];

function showSheet(html, cls = '') {
  document.querySelector('.sheet-scrim.app-sheet')?.remove();
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim app-sheet';
  scrim.innerHTML = `<div class="sheet ${cls}" role="dialog" tabindex="-1">${html}<div class="sheet-actions"><button class="btn primary" data-close>닫기</button></div></div>`;
  document.body.append(scrim);
  scrim.querySelector('.sheet').focus();
  const close = () => scrim.remove();
  scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-close]')) close(); });
  scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  return scrim;
}

function showShortcuts() {
  showSheet(`<h2>키보드 단축키</h2><div class="shortcut-cols">${SHORTCUTS.map(([g, list]) => `
    <section><h3>${g}</h3>${list.map(([k, d]) => `<div class="shortcut-row"><kbd>${k}</kbd><span>${d}</span></div>`).join('')}</section>`).join('')}</div>`, 'wide');
}

/** "0.1.1 (빌드 3)" — 빌드 번호가 없거나 0 이면 버전만. */
function versionText(info) {
  return info.build && info.build !== '0' ? `${info.version} (빌드 ${info.build})` : info.version;
}

async function showAbout() {
  let info = null;
  try { info = await api.appInfo(); } catch { /* 앱 밖 */ }
  showSheet(`<div class="about"><span class="brand-mark big"><img class="app-logo logo-light" src="img/logo-light.png" alt=""><img class="app-logo logo-dark" src="img/logo-dark.png" alt=""></span><h2>CView</h2>
    <p>치지직 라이브·채팅 데스크톱 앱 (Rust/Tauri 포팅판)</p>
    <p class="muted">버전 ${info ? versionText(info) : '-'} · ${info ? `${info.os} ${info.arch}` : ''}</p>
    <p class="muted small">재생: hls.js (Apache-2.0) · 셸: Tauri (MIT/Apache-2.0)</p></div>`);
}

function activeChat() {
  if (viewer.isOpen) return viewer.chat;
  if (current === 'multilive') return pages.multilive.chat;
  return null;
}

const MENU_ACTIONS = {
  'nav-home': () => navigate('home'),
  'nav-following': () => navigate('following'),
  'nav-popular': () => navigate('popular'),
  'nav-multilive': () => navigate('multilive'),
  'nav-live': () => navigate('live'),
  'nav-library': () => navigate('library'),
  'nav-categories': () => navigate('categories'),
  search: () => navigate('search'),
  palette: () => (palette.isOpen ? palette.close() : palette.open()),
  back: () => goBack(),
  refresh: () => pages[current] && (pages[current].acceptsParams ? pages[current].show({ ...currentParams, refresh: true }) : pages[current].show(true)),
  'toggle-sidebar': () => sidebar.toggle(),
  'theme-toggle': () => setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark'),
  settings: () => navigate('settings'),
  about: () => showAbout(),
  shortcuts: () => showShortcuts(),
  'report-issue': () => api.openExternal('https://github.com/dododo-lab/cview-rust/issues').catch((e) => toast(e.message)),
  'notification-settings': () => api.openNotificationSettings().catch((e) => toast(e.message)),
  'open-logs': async () => { try { const i = await api.appInfo(); await api.revealPath(i.dataDir); } catch (e) { toast(e.message); } },
  'play-pause': () => viewer.isOpen && viewer.controls.togglePlay(),
  mute: () => viewer.isOpen && viewer.toggleMute(),
  'go-live': () => viewer.isOpen && viewer.controls.goLive(),
  screenshot: () => viewer.isPlaying && viewer.controls.screenshot(),
  bookmark: () => viewer.isPlaying && viewer.controls.bookmark(),
  'window-player': () => {
    if (viewer.isPlaying) {
      const id = viewer.channelId;
      viewer.close();
      api.windowOpen('player', { channelId: id }).catch((er) => toast(er.message));
    } else api.windowOpen('player').catch((er) => toast(er.message));
  },
  'window-chat': () => {
    const id = viewer.isOpen ? viewer.channelId : (current === 'chat' ? pages.chat.channelId : null);
    api.windowOpen('chat', id ? { channelId: id } : null).catch((er) => toast(er.message));
  },
  'window-multichat': () => api.windowOpen('multichat').catch((er) => toast(er.message)),
  'window-stats': () => api.windowOpen('stats').catch((er) => toast(er.message)),
  pip: () => (viewer.isPlaying ? viewer.controls.togglePip() : toast('재생 중인 방송이 없습니다')),
  welcome: () => showWelcome({ force: true, onLogin: login }),
  fullscreen: () => viewer.isOpen && viewer.controls.fullscreen(false),
  'fullscreen-chat': () => viewer.isOpen && viewer.controls.fullscreen(true),
  'close-viewer': () => viewer.isOpen && viewer.close(),
  'chat-clear': () => activeChat()?.clear(true),
  'chat-reconnect': () => activeChat()?.reconnect(),
  'chat-autoscroll': async () => {
    const { chatPrefs, updateChatPrefs } = await import('./chat-prefs.js');
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
      if (hub) { e.preventDefault(); navigate(hub[1]); hubFlash(hub[2]); return; }
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
  trackFullscreen();
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
  pages.multilive = new MultiLivePage(content, { isLoggedIn });
  pages.live = new LivePage(content, { isLoggedIn });
  pages.categories = new CategoriesPage(content);
  pages.multichat = new MultiChatPage(content, { isLoggedIn });
  pages.chat = new ChatOnlyPage(content, { isLoggedIn, onBack: goBack, onPlay: openChannel, session: AUX === 'chat' ? `win-${Date.now()}` : null, standalone: AUX === 'chat' });
  pages.library = new LibraryPage(content, { isLoggedIn, onOpenChannel: openChannelPage });
  pages.stats = new StatsPage(content, { onOpenChannel: openChannelPage });
  pages.media = new MediaPage(content, { onBack: goBack, onOpenChannel: openChannelPage });
  pages.channel = new ChannelPage(content, { onBack: goBack, onPlay: openChannel, onAddMulti: addToMultiLive });
  viewer = new Viewer(document.getElementById('viewer'), { isLoggedIn, onClose: onViewerClosed });
  if (!AUX) insights = new InsightPanel(content, { isLoggedIn });
  bindMultiLiveAddButtons(addToMultiLive);
  installViewerMultiLiveButton();
  installBackgroundSaver();

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
    if (e.key === 'Escape' && viewer.isOpen && !document.fullscreenElement) viewer.close();
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
  listen(Events.multiLiveState, () => setTimeout(() => {
    syncLiveIndicators();
    // 라이브 무대의 채널이 닫혔으면 멀티라이브(남은 패인이 있으면) 또는 이전 화면으로.
    if (current === 'live' && !viewer.isPlaying && !isWatching()) {
      if ((pages.multilive.state.sessions || []).length) navigate('multilive', {}, { push: false });
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
    onRefresh: () => MENU_ACTIONS.refresh && MENU_ACTIONS.refresh(),
  });
  installCardMenu();
  installA11y();
  installPalette();
  installMenuAndShortcuts();
  installPrefsEffects({
    viewer,
    refreshCurrent: () => {
      if (viewer.isOpen || palette.isOpen) return;
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
  splashStep('계정 확인', 0.55);
  if (!isTauri) {
    toast('CView 앱(cargo run -p cview-app) 안에서 실행해야 데이터가 표시됩니다', 6000);
  } else {
    try {
      onAuthChanged(await api.authStatus());
    } catch { /* 상태 이벤트를 기다린다 */ }
  }
  let settings = null;
  splashStep('설정 불러오기', 0.8);
  try { settings = await api.settingsGet(); } catch { /* 기본값 */ }
  syncTheme(settings && settings.appearance && settings.appearance.theme);
  setTimeout(() => { hideSplash(); if (!AUX) showWelcome({ onLogin: login }); }, 120);
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
    case 'following': navigate(HUB_ROUTE[p.mode] || 'following'); break;
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
