// 왼쪽 사이드바 — 브랜드/접기, 빠른 검색, 메뉴, 방송 중인 팔로잉, 계정 카드.
// 접힘 상태는 로컬에 기억하고, 좁은 창(<1100px)에서는 사용자가 펼치지 않은 한 아이콘 레일로 접는다.
import { esc, safeUrl, icons, imageSrc, keepFocus } from './dom.js';
import { setTheme, currentTheme, onThemeChange, THEMES, THEME_LABELS } from './theme.js';

const COLLAPSE_KEY = 'cview.sidebarCollapsed';
const NARROW = window.matchMedia('(max-width: 1100px)');
const REDUCED_MOTION = window.matchMedia('(prefers-reduced-motion: reduce)');
const LIVE_LIMIT = 7;
const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const ICON_KEYBOARD = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M7 10h.01M11 10h.01M15 10h.01M7 14h10"/></svg>';
const ICON_INFO = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>';

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
    if (!el || !this.root.classList.contains('collapsed') || (this.accountMenu && el.matches('.account-trigger'))) {
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
    this.closeAccountMenu();
    try {
      localStorage.setItem(COLLAPSE_KEY, this.collapsed ? '0' : '1');
    } catch { /* 저장 불가 — 이번 세션만 */ }
    // 전환 동안만 .sb-anim — 배치가 바뀌는 칸을 잠깐 숨겼다 페이드인한다(app.css "접힌 사이드바").
    // 전환 동안 본문 너비가 매 프레임 바뀌므로, 무거운 크기 맞춤(시청 화면 채널 레일 등)은 끝난 뒤 한 번만 하도록
    // <html>.sidebar-animating 을 걸고 끝나면 'cview:sidebar-settled' 를 보낸다.
    clearTimeout(this.animTimer);
    this.finishSlide();
    const heavy = this.playingInContent();
    this.root.classList.remove('sb-anim');
    if (!heavy) {
      void this.root.offsetWidth; // 연타해도 페이드인 애니메이션을 처음부터 다시
      this.root.classList.add('sb-anim');
    }
    document.documentElement.classList.add('sidebar-animating');
    this.animTimer = setTimeout(() => {
      this.root.classList.remove('sb-anim');
      document.documentElement.classList.remove('sidebar-animating');
      window.dispatchEvent(new Event('cview:sidebar-settled'));
    }, 520);
    if (heavy) this.slideOver();
    else this.applyCollapsed();
  }

  /** 본문에 보이는 영상(라이브 플레이어·멀티라이브 패인 iframe, 클립 등)이 있나. */
  playingInContent() {
    if (REDUCED_MOTION.matches || document.documentElement.dataset.motion === 'reduced') return false;
    const content = document.querySelector('.content');
    return Boolean(content) && [...content.querySelectorAll('iframe, video')].some((el) => el.offsetParent !== null);
  }

  /**
   * 영상 시청 중 접기·펼치기 — 합성(GPU)만 쓰는 전환.
   * 영상(특히 멀티라이브 여러 패인)이 재생 중이면 메인 스레드가 바빠, 너비·패딩처럼 레이아웃을 바꾸는 전환은
   * 프레임마다 밀려 뚝뚝 끊긴다. 그래서 여기서는 실제 사이드바를 곧바로 최종 배치로 바꾸고,
   * 움직임은 transform·opacity 만으로 만든다(WebKit 은 이것을 메인 스레드 없이 돌린다).
   * - 접기: 펼친 모습의 복제본이 왼쪽으로 밀려나며 사라지고, 그 밑에서 아이콘 레일이 나타난다.
   *   본문은 시작에 한 번만 넓어진다(왼쪽은 아직 복제본 밑).
   * - 펼치기: 펼친 사이드바가 본문 위로 왼쪽에서 밀려 들어오고 레일 복제본은 사라진다.
   *   그동안 오른쪽 음수 여백으로 차지하는 폭을 유지했다가, 끝에 한 번만 본문이 좁아진다.
   */
  slideOver() {
    const el = this.root;
    const app = el.parentElement;
    const r0 = el.getBoundingClientRect();
    const ghost = el.cloneNode(true);
    ghost.removeAttribute('id');
    for (const n of ghost.querySelectorAll('[id]')) n.removeAttribute('id');
    ghost.classList.remove('sb-anim');
    ghost.classList.add('sb-instant', 'sb-ghost');
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    Object.assign(ghost.style, { left: `${r0.left}px`, top: `${r0.top}px`, width: `${r0.width}px`, height: `${r0.height}px` });
    const scrollTop = el.scrollTop;

    el.classList.add('sb-instant');
    this.applyCollapsed();
    const w1 = el.getBoundingClientRect().width;
    const collapsing = w1 < r0.width;
    const shift = Math.abs(r0.width - w1);
    app.classList.add('sb-overlay');
    if (!collapsing) el.style.marginRight = `${-shift}px`; // 펼치는 동안 본문 폭 유지
    document.body.append(ghost);
    ghost.scrollTop = scrollTop;

    const css = getComputedStyle(el);
    const timing = {
      duration: (parseFloat(css.getPropertyValue('--sb-dur')) || 0.34) * 1000,
      easing: css.getPropertyValue('--sb-ease').trim() || 'ease-out',
    };
    const anims = collapsing
      ? [
        // 두 겹이 겹쳐 보이는 시간을 줄이려고 복제본은 70% 지점까지 사라지고, 레일은 30% 부터 나타난다.
        ghost.animate([{ transform: 'translateX(0)', opacity: 1 }, { opacity: 0, offset: 0.7 }, { transform: `translateX(${-shift}px)`, opacity: 0 }], timing),
        el.animate([{ opacity: 0 }, { opacity: 0, offset: 0.3 }, { opacity: 1 }], { ...timing, easing: 'ease-out' }),
      ]
      : [
        el.animate([{ transform: `translateX(${-shift}px)` }, { transform: 'translateX(0)' }], timing),
        ghost.animate([{ opacity: 1 }, { opacity: 0 }], { duration: timing.duration * 0.35, easing: 'ease-out', fill: 'forwards' }),
      ];
    this.slide = { ghost, anims };
    Promise.all(anims.map((a) => a.finished)).then(() => {
      if (this.slide && this.slide.ghost === ghost) this.finishSlide();
    }, () => {});
  }

  /** 진행 중인 영상용 전환을 끝 상태로 정리한다(연타·완료 공용). */
  finishSlide() {
    const slide = this.slide;
    if (!slide) return;
    this.slide = null;
    for (const a of slide.anims) a.cancel();
    slide.ghost.remove();
    this.root.style.marginRight = '';
    this.root.classList.remove('sb-instant');
    this.root.parentElement.classList.remove('sb-overlay');
  }

  renderNav() {
    // 검색은 위쪽 빠른 검색 버튼이 맡는다(같은 메뉴를 두 번 두지 않는다). `sidebar: false` 는 다른 메뉴 속 화면.
    this.nav.innerHTML = this.routes.filter((r) => r.id !== 'search' && r.sidebar !== false).map((r) => {
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
    // 팔로잉 폴링(2분)마다 메뉴·방송 중 목록을 다시 그린다 — Tab 으로 고르던 항목의 초점을 지킨다
    keepFocus(this.root, () => this.paintFollowing(lives));
  }

  paintFollowing(lives) {
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
    this.closeAccountMenu();
    this.auth = auth;
    if (auth && auth.loggedIn) {
      // 아바타·이름 전체가 계정 메뉴(말풍선) 버튼 — 로그아웃은 말풍선 맨 아래로 옮겼다.
      el.innerHTML = `
        <button class="account-trigger" data-tip="계정 메뉴" aria-label="계정 메뉴" aria-haspopup="menu" aria-expanded="false">
          <span class="account-avatar">${avatar(auth.profileImageUrl)}<i class="presence"></i></span>
          <span class="account-name ellipsis">${esc(auth.nickname || '로그인됨')}<small>네이버 계정</small></span>
          <span class="account-chevron" aria-hidden="true">${icons.chevronRight}</span>
        </button>`;
      el.querySelector('.account-trigger').addEventListener('click', (e) => {
        e.stopPropagation();
        if (this.accountMenu) this.closeAccountMenu();
        else this.openAccountMenu(e.detail === 0);
      });
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

  /**
   * 계정 말풍선 메뉴 — 사이드바가 세로 스크롤 칸이라 잘리지 않게 <body> 에 띄우고 위치를 잡는다.
   * 펼친 사이드바: 계정 카드 위로, 꼬리는 아래(아바타 가운데)를 가리킨다.
   * 접힌 레일: 레일 오른쪽으로, 꼬리는 왼쪽(아바타 가운데)을 가리키고 아래쪽을 아바타에 맞춘다.
   * @param {boolean} viaKeyboard 키보드로 열었으면 첫 항목에 초점
   */
  openAccountMenu(viaKeyboard = false) {
    const auth = this.auth;
    const trigger = this.account.querySelector('.account-trigger');
    if (!auth || !auth.loggedIn || !trigger) return;
    this.tip.hidden = true;
    const icon = (id) => (this.routes.find((r) => r.id === id) || {}).icon || '';
    const item = (key, label, svg, cls = '') =>
      `<button class="ap-item${cls}" role="menuitem" ${key}><span class="ap-icon">${svg}</span><span class="ap-label">${label}</span></button>`;
    const pop = document.createElement('div');
    pop.className = 'account-pop';
    pop.setAttribute('role', 'menu');
    pop.setAttribute('aria-label', '계정 메뉴');
    pop.innerHTML = `
      <div class="ap-head">
        ${avatar(auth.profileImageUrl)}
        <div class="ap-who"><b class="ellipsis">${esc(auth.nickname || '로그인됨')}</b><small>네이버 계정${auth.method === 'oauth' ? ' · 공식 로그인' : ''}</small></div>
      </div>
      <div class="ap-sep"></div>
      ${item('data-ap-route="following"', '팔로잉', icon('following'))}
      ${item('data-ap-route="library"', '보관함', icon('library'))}
      ${item('data-ap-route="stats"', '시청 통계', icon('stats'))}
      <div class="ap-sep"></div>
      <div class="ap-theme"><span>테마</span>
        <div class="segmented" role="group" aria-label="테마">${THEMES.map((t) => `<button class="seg" data-ap-theme="${t}">${THEME_LABELS[t]}</button>`).join('')}</div>
      </div>
      ${item('data-ap-route="settings"', '설정', icon('settings'))}
      ${item('data-ap-menu="shortcuts"', '키보드 단축키', ICON_KEYBOARD)}
      ${item('data-ap-menu="about"', 'CView 정보', ICON_INFO)}
      <div class="ap-sep"></div>
      ${item('data-action="logout"', '로그아웃', icons.logout, ' danger')}`;
    document.body.append(pop);
    this.accountMenu = pop;
    trigger.setAttribute('aria-expanded', 'true');
    this.account.classList.add('menu-open');

    const markTheme = () => pop.querySelectorAll('[data-ap-theme]').forEach((b) => {
      const on = b.dataset.apTheme === currentTheme();
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    });
    markTheme();
    const offTheme = onThemeChange(markTheme);

    this.placeAccountMenu();
    pop.addEventListener('click', (e) => {
      const t = e.target.closest('[data-ap-theme]');
      if (t) { setTheme(t.dataset.apTheme); return; } // 테마는 고른 뒤에도 말풍선을 열어 둔다
      const b = e.target.closest('.ap-item');
      if (!b) return;
      this.closeAccountMenu();
      // 로그아웃(data-action)은 app.js 의 문서 클릭 처리가 맡는다.
      if (b.dataset.apRoute) this.onNavigate(b.dataset.apRoute);
      else if (b.dataset.apMenu) window.dispatchEvent(new CustomEvent('cview:menu', { detail: b.dataset.apMenu }));
    });
    pop.addEventListener('keydown', (e) => {
      const items = [...pop.querySelectorAll('.ap-item, .seg')];
      const i = items.indexOf(document.activeElement);
      if (e.key === 'Escape') { e.preventDefault(); this.closeAccountMenu(); trigger.focus(); }
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const n = items.length;
        items[(i + (e.key === 'ArrowDown' ? 1 : -1) + n) % n].focus();
      } else if (e.key === 'Tab') this.closeAccountMenu();
    });
    const outside = (e) => {
      if (!pop.contains(e.target) && !trigger.contains(e.target)) this.closeAccountMenu();
    };
    const place = () => this.placeAccountMenu();
    const close = () => this.closeAccountMenu();
    setTimeout(() => document.addEventListener('mousedown', outside, true), 0);
    window.addEventListener('resize', place);
    window.addEventListener('blur', close);
    this.root.addEventListener('scroll', place, { passive: true });
    this.accountMenuCleanup = () => {
      offTheme();
      document.removeEventListener('mousedown', outside, true);
      window.removeEventListener('resize', place);
      window.removeEventListener('blur', close);
      this.root.removeEventListener('scroll', place);
    };
    if (viaKeyboard) pop.querySelector('.ap-item')?.focus();
  }

  placeAccountMenu() {
    const pop = this.accountMenu;
    const av = this.account.querySelector('.account-avatar');
    if (!pop || !av) return;
    const a = av.getBoundingClientRect();
    const w = pop.offsetWidth;
    const h = pop.offsetHeight;
    const gap = 12;
    const cx = a.left + a.width / 2;
    const cy = a.top + a.height / 2;
    let left;
    let top;
    if (this.root.classList.contains('collapsed')) {
      pop.dataset.side = 'right';
      left = this.root.getBoundingClientRect().right + gap;
      top = Math.max(8, Math.min(a.bottom + 8 - h, window.innerHeight - h - 8));
      pop.style.setProperty('--tail', `${Math.max(16, Math.min(h - 16, cy - top))}px`);
    } else {
      pop.dataset.side = 'top';
      const card = this.account.getBoundingClientRect();
      left = Math.max(8, Math.min(card.left, window.innerWidth - w - 8));
      top = Math.max(8, card.top - gap - h);
      pop.style.setProperty('--tail', `${Math.max(16, Math.min(w - 16, cx - left))}px`);
    }
    pop.style.left = `${left}px`;
    pop.style.top = `${top}px`;
  }

  closeAccountMenu() {
    if (!this.accountMenu) return;
    this.accountMenuCleanup?.();
    this.accountMenu.remove();
    this.accountMenu = null;
    this.account.classList.remove('menu-open');
    this.account.querySelector('.account-trigger')?.setAttribute('aria-expanded', 'false');
  }
}
