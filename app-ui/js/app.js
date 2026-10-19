// CView 메인 — 사이드바 라우팅, 인증 상태, 시청 화면 연결.
import { api, listen, Events, isTauri } from './api.js';
import { toast, icons } from './dom.js';
import { bindCardOpen } from './cards.js';
import { FollowingPage, PopularPage, SearchPage, SettingsPage } from './pages.js';
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

// 사이드바 — Swift `AppRouter.SidebarItem` 라벨.
const ROUTES = [
  { id: 'home', title: '홈', icon: icons.home },
  { id: 'following', title: '팔로잉', icon: icons.following },
  { id: 'popular', title: '인기', icon: icons.popular },
  { id: 'live', title: '라이브', icon: liveIcon },
  { id: 'multilive', title: '멀티라이브', icon: multiLiveIcon },
  { id: 'library', title: '보관함', icon: libraryIcon },
  { id: 'search', title: '검색', icon: icons.search, section: '탐색' },
  { id: 'settings', title: '설정', icon: icons.settings, section: '앱' },
];

let auth = { loggedIn: false, state: 'loggedOut' };
const isLoggedIn = () => Boolean(auth && auth.loggedIn);

const pages = {};
let current = null;
let viewer = null;
let sidebar = null;

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
    sidebar.setFollowing(await api.followingLives(false));
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
  // 라이브 메뉴는 재생 중이면 시청 화면, 아니면 이어 볼 채널 목록이다.
  const showViewer = route === 'live' && viewer.isPlaying;
  for (const [id, page] of Object.entries(pages)) {
    page.root.hidden = showViewer || id !== route;
  }
  if (showViewer) {
    viewer.reveal();
    setAudioOwner('live');
    return;
  }
  viewer.hide();
  if (route === 'multilive') setAudioOwner('multilive');
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
  sidebar.setPlaying('live', false);
  api.multiLiveSetAudioSuspended(false).catch(() => {});
  audioOwner = null;
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
  let player = null;
  try {
    player = (await api.settingsGet()).player;
  } catch { /* 기본값 */ }
  viewer.open(channelId, player);
  sidebar.setPlaying('live', true);
  audioOwner = null;
  await navigate('live');
}

/** 카드·시청 화면의 "멀티라이브에 추가" — 멀티라이브 화면으로 옮겨 패인을 띄운다. */
async function addToMultiLive(channelId) {
  if (current !== 'multilive') await navigate('multilive');
  await pages.multilive.add(channelId);
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
}

async function main() {
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
  pages.library = new LibraryPage(content, { isLoggedIn, onOpenChannel: openChannelPage });
  pages.media = new MediaPage(content, { onBack: goBack, onOpenChannel: openChannelPage });
  pages.channel = new ChannelPage(content, { onBack: goBack, onPlay: openChannel, onAddMulti: addToMultiLive });
  viewer = new Viewer(document.getElementById('viewer'), { isLoggedIn, onClose: onViewerClosed });
  bindMultiLiveAddButtons(addToMultiLive);
  installViewerMultiLiveButton();

  for (const p of Object.values(pages)) bindCardOpen(p.root.querySelector('.page-body'), openChannel);
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
  window.addEventListener('cview:open-channel', (e) => openChannelPage(e.detail && e.detail.channelId));
  // 시청 화면 헤더(아바타·채널명)를 누르면 채널 상세
  document.querySelector('#viewer .viewer-header').addEventListener('click', (e) => {
    if (e.target.closest('.viewer-avatar, .viewer-title') && viewer.channelId) openChannelPage(viewer.channelId);
  });

  listen(Events.authChanged, onAuthChanged);
  window.addEventListener('cview:login', login);
  loadChatPrefs();
  listen(Events.loginFailed, (p) => p && p.message && toast(p.message));

  renderAccount();
  if (!isTauri) {
    toast('CView 앱(cargo run -p cview-app) 안에서 실행해야 데이터가 표시됩니다', 6000);
  } else {
    try {
      onAuthChanged(await api.authStatus());
    } catch { /* 상태 이벤트를 기다린다 */ }
  }
  api.settingsGet().then((s) => syncTheme(s && s.appearance && s.appearance.theme)).catch(() => {});
  navigate('home');
}

main();
