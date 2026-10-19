// CView 메인 — 사이드바 라우팅, 인증 상태, 시청 화면 연결.
import { api, listen, Events, isTauri } from './api.js';
import { esc, safeUrl, toast, icons } from './dom.js';
import { bindCardOpen } from './cards.js';
import { HomePage, FollowingPage, PopularPage, SearchPage, SettingsPage } from './pages.js';
import { Viewer } from './viewer.js';

// 사이드바 — Swift `AppRouter.SidebarItem` 라벨.
const ROUTES = [
  { id: 'home', title: '홈', icon: icons.home },
  { id: 'following', title: '팔로잉', icon: icons.following },
  { id: 'popular', title: '인기', icon: icons.popular },
  { id: 'search', title: '검색', icon: icons.search, section: '탐색' },
  { id: 'settings', title: '설정', icon: icons.settings, section: '앱' },
];

let auth = { loggedIn: false, state: 'loggedOut' };
const isLoggedIn = () => Boolean(auth && auth.loggedIn);

const pages = {};
let current = null;
let viewer = null;

function renderNav() {
  const nav = document.getElementById('nav');
  nav.innerHTML = ROUTES.map((r) => `
    ${r.section ? `<div class="nav-section">${esc(r.section)}</div>` : ''}
    <button class="nav-item" data-route="${r.id}">${r.icon}<span>${esc(r.title)}</span></button>`).join('');
  nav.addEventListener('click', (e) => {
    const item = e.target.closest('[data-route]');
    if (item) navigate(item.dataset.route);
  });
}

async function navigate(route) {
  if (viewer && viewer.isOpen) await viewer.close();
  current = route;
  for (const el of document.querySelectorAll('.nav-item')) {
    el.classList.toggle('active', el.dataset.route === route);
  }
  for (const [id, page] of Object.entries(pages)) {
    page.root.hidden = id !== route;
  }
  pages[route].show();
}

function renderAccount() {
  const el = document.getElementById('account');
  if (isLoggedIn()) {
    const avatar = safeUrl(auth.profileImageUrl);
    el.innerHTML = `
      ${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '<div class="avatar"></div>'}
      <div class="account-name ellipsis">${esc(auth.nickname || '로그인됨')}<small>네이버 계정</small></div>
      <button class="btn ghost" data-action="logout" title="로그아웃">로그아웃</button>`;
  } else {
    const busy = auth.state === 'loggingIn';
    el.innerHTML = `
      <div class="avatar"></div>
      <div class="account-name">비로그인<small>${busy ? '로그인 중…' : '네이버 로그인이 필요합니다'}</small></div>
      <button class="btn primary" data-action="login" ${busy ? 'disabled' : ''}>로그인</button>`;
  }
}

function onAuthChanged(status) {
  const wasLoggedIn = isLoggedIn();
  auth = status || { loggedIn: false, state: 'loggedOut' };
  renderAccount();
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
}

async function main() {
  renderNav();
  const content = document.querySelector('.content');
  const page = (id) => content.querySelector(`[data-page="${id}"]`);
  pages.home = new HomePage(page('home'), { isLoggedIn });
  pages.following = new FollowingPage(page('following'), { isLoggedIn });
  pages.popular = new PopularPage(page('popular'));
  pages.search = new SearchPage(page('search'));
  pages.settings = new SettingsPage(page('settings'), {
    getAuth: () => auth,
    onLogin: login,
    onLogout: logout,
  });
  viewer = new Viewer(document.getElementById('viewer'), { isLoggedIn });

  for (const p of Object.values(pages)) bindCardOpen(p.root.querySelector('.page-body'), openChannel);
  document.addEventListener('click', (e) => {
    const action = e.target.closest('[data-action]');
    if (!action) return;
    if (action.dataset.action === 'login' && !action.closest('[data-page="settings"]')) login();
    if (action.dataset.action === 'logout' && !action.closest('[data-page="settings"]')) logout();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && viewer.isOpen && !document.fullscreenElement) viewer.close();
  });

  listen(Events.authChanged, onAuthChanged);
  listen(Events.loginFailed, (p) => p && p.message && toast(p.message));

  renderAccount();
  if (!isTauri) {
    toast('CView 앱(cargo run -p cview-app) 안에서 실행해야 데이터가 표시됩니다', 6000);
  } else {
    try {
      onAuthChanged(await api.authStatus());
    } catch { /* 상태 이벤트를 기다린다 */ }
  }
  navigate('home');
}

main();
