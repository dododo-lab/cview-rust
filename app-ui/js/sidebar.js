// 왼쪽 사이드바 — 브랜드/접기, 빠른 검색, 메뉴, 방송 중인 팔로잉, 계정 카드.
// 접힘 상태는 로컬에 기억하고, 좁은 창(<1100px)에서는 사용자가 펼치지 않은 한 아이콘 레일로 접는다.
import { esc, safeUrl, icons, imageSrc } from './dom.js';

const COLLAPSE_KEY = 'cview.sidebarCollapsed';
const NARROW = window.matchMedia('(max-width: 1100px)');
const LIVE_LIMIT = 7;
const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

function storedCollapsed() {
  try {
    const v = localStorage.getItem(COLLAPSE_KEY);
    return v === null ? null : v === '1';
  } catch {
    return null;
  }
}

function avatar(url, cls = 'avatar') {
  const src = safeUrl(url);
  return src
    ? `<img class="${cls}" src="${esc(imageSrc(src, 'avatar'))}" alt="" referrerpolicy="no-referrer">`
    : `<div class="${cls}"></div>`;
}

export class Sidebar {
  /**
   * @param {HTMLElement} root `.sidebar`
   * @param {{ routes: Array<{id,title,icon,section?}>, onNavigate: (id: string) => void }} opts
   */
  constructor(root, { routes, onNavigate }) {
    this.root = root;
    this.routes = routes;
    this.onNavigate = onNavigate;
    this.liveCount = null;
    this.playing = new Set();
    this.nav = root.querySelector('#nav');
    this.liveBox = root.querySelector('#sidebar-live');
    this.account = root.querySelector('#account');

    root.querySelector('.brand-row').insertAdjacentHTML('beforeend',
      `<button class="icon-btn sidebar-toggle" data-tip="사이드바 접기" aria-label="사이드바 접기">${icons.sidebar}</button>`);
    root.querySelector('.sidebar-toggle').addEventListener('click', () => this.toggle());
    root.querySelector('.quick-search').addEventListener('click', () => this.onNavigate('search'));
    root.querySelector('.quick-search kbd').textContent = isMac ? '⌘K' : 'Ctrl K';

    this.nav.addEventListener('click', (e) => {
      const item = e.target.closest('[data-route]');
      if (item) this.onNavigate(item.dataset.route);
    });
    document.addEventListener('keydown', (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        this.onNavigate('search');
      }
      if ((e.metaKey || e.ctrlKey) && e.key === '\\') {
        e.preventDefault();
        this.toggle();
      }
    });
    this.tip = document.createElement('div');
    this.tip.className = 'side-tip';
    this.tip.hidden = true;
    document.body.append(this.tip);
    root.addEventListener('mouseover', (e) => this.showTip(e.target.closest('[data-tip]')));
    root.addEventListener('mouseleave', () => { this.tip.hidden = true; });
    NARROW.addEventListener('change', () => this.applyCollapsed());
    this.applyCollapsed();
    this.renderNav();
  }

  get collapsed() {
    const stored = storedCollapsed();
    return stored === null ? NARROW.matches : stored;
  }

  applyCollapsed() {
    const c = this.collapsed;
    this.root.classList.toggle('collapsed', c);
    const t = this.root.querySelector('.sidebar-toggle');
    const label = c ? '사이드바 펼치기' : '사이드바 접기';
    t.dataset.tip = label;
    t.setAttribute('aria-label', label);
  }

  /** 접힌 레일에서만 이름 툴팁을 띄운다. */
  showTip(el) {
    if (!el || !this.root.classList.contains('collapsed')) {
      this.tip.hidden = true;
      return;
    }
    const r = el.getBoundingClientRect();
    this.tip.textContent = el.dataset.tip;
    this.tip.style.left = `${this.root.getBoundingClientRect().right + 8}px`;
    this.tip.style.top = `${r.top + r.height / 2}px`;
    this.tip.hidden = false;
  }

  toggle() {
    this.tip.hidden = true;
    try {
      localStorage.setItem(COLLAPSE_KEY, this.collapsed ? '0' : '1');
    } catch { /* 저장 불가 — 이번 세션만 */ }
    // 전환 동안만 .sb-anim — 배치가 바뀌는 칸을 잠깐 숨겼다 페이드인한다(app.css "접힌 사이드바").
    // 전환 동안 본문 너비가 매 프레임 바뀌므로, 무거운 크기 맞춤(시청 화면 채널 레일 등)은 끝난 뒤 한 번만 하도록
    // <html>.sidebar-animating 을 걸고 끝나면 'cview:sidebar-settled' 를 보낸다.
    clearTimeout(this.animTimer);
    this.root.classList.remove('sb-anim');
    void this.root.offsetWidth; // 연타해도 페이드인 애니메이션을 처음부터 다시
    this.root.classList.add('sb-anim');
    document.documentElement.classList.add('sidebar-animating');
    this.animTimer = setTimeout(() => {
      this.root.classList.remove('sb-anim');
      document.documentElement.classList.remove('sidebar-animating');
      window.dispatchEvent(new Event('cview:sidebar-settled'));
    }, 520);
    this.applyCollapsed();
  }

  renderNav() {
    // 검색은 위쪽 빠른 검색 버튼이 맡는다(같은 메뉴를 두 번 두지 않는다).
    this.nav.innerHTML = this.routes.filter((r) => r.id !== 'search').map((r) => {
      const count = r.id === 'following' ? this.liveCount : (this.badges && this.badges.get(r.id));
      const badge = count ? `<span class="nav-badge">${count > 99 ? '99+' : count}</span>` : '';
      return `
        ${r.section ? `<div class="nav-section">${esc(r.section)}</div>` : ''}
        <button class="nav-item${this.playing.has(r.id) ? ' playing' : ''}" data-route="${r.id}" data-tip="${esc(r.title)}">
          <span class="nav-icon">${r.icon}</span><span class="nav-label">${esc(r.title)}</span>${badge}
        </button>`;
    }).join('');
  }

  /** 메뉴 옆 숫자(멀티라이브 패인 수 등). 0 이면 지운다. */
  setBadge(route, count) {
    if (!this.badges) this.badges = new Map();
    if ((this.badges.get(route) || 0) === (count || 0)) return;
    if (count) this.badges.set(route, count); else this.badges.delete(route);
    const item = this.nav.querySelector(`[data-route="${route}"]`);
    if (!item) return;
    item.querySelector('.nav-badge')?.remove();
    if (count) item.insertAdjacentHTML('beforeend', `<span class="nav-badge">${count > 99 ? '99+' : count}</span>`);
  }

  /** 메뉴 옆 "재생 중" 표시(라이브 메뉴). */
  setPlaying(route, on) {
    if (on) this.playing.add(route); else this.playing.delete(route);
    const item = this.nav.querySelector(`[data-route="${route}"]`);
    if (item) item.classList.toggle('playing', on);
  }

  setActive(route) {
    this.root.querySelector('.quick-search').classList.toggle('active', route === 'search');
    for (const el of this.nav.querySelectorAll('.nav-item')) {
      const on = el.dataset.route === route;
      el.classList.toggle('active', on);
      if (on) el.setAttribute('aria-current', 'page');
      else el.removeAttribute('aria-current');
    }
  }

  /** 방송 중인 팔로잉. `null` 이면(비로그인·실패) 숨긴다. */
  setFollowing(lives) {
    const active = this.nav.querySelector('.nav-item.active')?.dataset.route;
    this.liveCount = lives ? lives.length : null;
    this.renderNav();
    if (active) this.setActive(active);
    if (!lives || !lives.length) {
      this.liveBox.hidden = !lives;
      this.liveBox.innerHTML = lives ? '<div class="nav-section">방송 중</div><p class="side-empty">지금 방송 중인 팔로잉 채널이 없어요</p>' : '';
      return;
    }
    const sorted = [...lives].sort((a, b) => (b.viewerCount || 0) - (a.viewerCount || 0));
    const more = sorted.length - LIVE_LIMIT;
    this.liveBox.hidden = false;
    this.liveBox.innerHTML = `
      <div class="nav-section">방송 중</div>
      ${sorted.slice(0, LIVE_LIMIT).map((l) => `
        <button class="side-live" data-channel-id="${esc(l.channelId)}" data-channel-name="${esc(l.channelName)}" data-channel-image="${esc(safeUrl(l.channelImageUrl))}" data-tip="${esc(l.channelName)} · ${esc(l.viewerCountText)}명">
          <span class="side-live-avatar">${avatar(l.channelImageUrl)}</span>
          <span class="side-live-text">
            <span class="side-live-name ellipsis">${esc(l.channelName)}</span>
            <span class="side-live-cat ellipsis">${esc(l.categoryName || l.liveTitle || '')}</span>
          </span>
          <span class="side-live-viewers"><i class="dot"></i>${esc(l.viewerCountText)}</span>
        </button>`).join('')}
      ${more > 0 ? `<button class="side-more" data-tip="팔로잉 ${more}명 더 보기">+${more}<span class="nav-label">명 더 보기</span></button>` : ''}`;
    this.liveBox.querySelector('.side-more')?.addEventListener('click', () => this.onNavigate('following'));
  }

  renderAccount(auth) {
    const el = this.account;
    if (auth && auth.loggedIn) {
      el.innerHTML = `
        <div class="account-avatar">${avatar(auth.profileImageUrl)}<i class="presence"></i></div>
        <div class="account-name ellipsis">${esc(auth.nickname || '로그인됨')}<small>네이버 계정</small></div>
        <button class="icon-btn account-action" data-action="logout" data-tip="로그아웃" aria-label="로그아웃">${icons.logout}</button>`;
    } else {
      const busy = auth && auth.state === 'loggingIn';
      const expired = auth && auth.state === 'expired';
      el.innerHTML = `
        <div class="account-avatar"><div class="avatar guest">${icons.user}</div></div>
        <div class="account-name">${expired ? '로그인 만료' : '비로그인'}<small>${busy ? '로그인 중…' : expired ? '다시 로그인해 주세요' : '팔로우·채팅 이용'}</small></div>
        <button class="btn primary account-login" data-action="login" data-tip="네이버 로그인" ${busy ? 'disabled' : ''}>
          <span class="login-icon">${icons.login}</span><span class="nav-label">로그인</span>
        </button>`;
    }
  }
}
