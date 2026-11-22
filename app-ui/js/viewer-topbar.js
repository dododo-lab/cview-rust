// 예전 시청 화면(#viewer — 보조 플레이어 창·라이브 칸이 가득 찼을 때)의 위쪽 막대 동작 묶음.
// 라이브 단독 보기(ml-watch.js) 상단 바와 같은 짜임으로 맞춘다:
//   [채널 정보·지표·화질/지연 배지] [바로 전환 레일] … [채널: 팔로우·즐겨찾기·링크] | [보기: 채팅 표시·새 창] [⋯] [×]
// ⋯ 메뉴 — 채널(채널 정보·치지직에서 열기·멀티라이브에 추가) / 방송(북마크·스크린샷·다시 연결) / 보기(PiP·영상+채팅 전체 화면).
// 좁은 무대(@container vstage, 640px 이하)에서는 채널·보기 묶음이 ⋯ 로 접힌다(CSS).
import { esc, fitPopover, hideWithExit } from './dom.js';
import { kbd } from './platform.js';
import { currentBindings, bindingLabel, onShortcuts } from './shortcuts.js';
import { bindMenuKeys, focusFirstMenuItem } from './ml-pane-controls.js';
import { qualityBadgeLabel } from './viewer-controls.js';
import { CHANNEL_ICON, CHANNEL_SOCIAL_EVENT, loadChannelSocial, setFollow, setFavorite, copyLiveLink, openOnChzzk } from './channel-actions.js';

const ICON = {
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  popout: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="13" height="13" rx="2"/><path d="M10 4h10v10M20 4l-8 8"/></svg>',
  more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5.5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/></svg>',
  info: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5M12 7.6v.2"/></svg>',
  retry: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11.5a8 8 0 1 0-2.4 5.9"/><path d="M20 4.5v7h-7"/></svg>',
  camera: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>',
  bookmark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  pip: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><rect x="12" y="11" width="7" height="6" rx="1" fill="currentColor" stroke="none"/></svg>',
  chatFull: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M15 20h5v-5"/><path d="M14 9h4M14 12h4"/></svg>',
};

export class ViewerTopBar {
  /**
   * @param {import('./viewer.js').Viewer} viewer
   * @param {{ isLoggedIn: () => boolean, onAddMulti?: ((channelId: string) => void) | null, onPopout?: (() => void) | null }} opts
   */
  constructor(viewer, { isLoggedIn, onAddMulti = null, onPopout = null }) {
    this.viewer = viewer;
    this.isLoggedIn = isLoggedIn;
    this.onAddMulti = onAddMulti;
    this.onPopout = onPopout;
    this.social = null;
    const root = viewer.root;
    this.stage = root.querySelector('.player-stage');
    const top = root.querySelector('.stage-top');
    const close = root.querySelector('#viewer-close');
    const chatMode = root.querySelector('#ctl-chatmode');
    // 채팅 표시 방식(viewer-extras.js)은 '보기' 묶음으로 옮긴다.
    close.insertAdjacentHTML('beforebegin', `
      <div class="vt-group" role="group" aria-label="채널">
        <button class="vt-follow" data-vt="follow" hidden><span class="vt-ico">${CHANNEL_ICON.follow}</span><span class="vt-label">팔로우</span></button>
        <button class="icon-btn" data-vt="fav" aria-pressed="false" title="즐겨찾기에 추가" aria-label="즐겨찾기">${CHANNEL_ICON.star}</button>
        <button class="icon-btn" data-vt="copy" title="방송 링크 복사" aria-label="방송 링크 복사">${CHANNEL_ICON.link}</button>
      </div>
      <span class="vt-sep" aria-hidden="true"></span>
      <div class="vt-group vt-view" role="group" aria-label="보기"></div>
      <button class="icon-btn" data-vt="more" id="viewer-more" title="더 보기" aria-label="더 보기" aria-haspopup="menu" aria-expanded="false" aria-controls="viewer-more-menu">${ICON.more}</button>`);
    const view = top.querySelector('.vt-view');
    if (chatMode) view.append(chatMode);
    if (onPopout) view.insertAdjacentHTML('beforeend', `<button class="icon-btn" data-vt="popout" title="${kbd('새 창으로 분리 (⌘N)')}" aria-label="새 창으로 분리">${ICON.popout}</button>`);
    this.moreButton = top.querySelector('[data-vt="more"]');

    // 지표 줄 끝의 화질·지연 배지 — 누르면 아래 막대의 화질 메뉴.
    this.badge = document.createElement('button');
    this.badge.className = 'vt-q-badge';
    this.badge.hidden = true;
    this.badge.setAttribute('aria-haspopup', 'menu');
    this.badge.setAttribute('aria-controls', 'ctl-quality-menu');
    this.badge.innerHTML = '<span class="vt-q-res"></span><span class="vt-q-lat"></span>';
    this.badge.setAttribute('aria-expanded', 'false');
    this.badge.addEventListener('click', (e) => { e.stopPropagation(); viewer.controls.toggleQualityMenu(this.badge); });

    // ⋯ 메뉴 — stage-top 은 overflow:hidden 이라 무대(.player-stage)에 둔다.
    this.menu = document.createElement('div');
    this.menu.className = 'vt-menu';
    this.menu.id = 'viewer-more-menu';
    this.menu.setAttribute('role', 'menu');
    this.menu.setAttribute('aria-label', '더 보기');
    this.menu.hidden = true;
    const key = (k) => `<kbd class="vt-key" data-key-of="${k}"></kbd>`;
    const item = (vt, icon, label, { narrow = false, k = '', extra = '' } = {}) => `<button class="vt-item${narrow ? ' vt-narrow' : ''}" role="menuitem" data-vt="${vt}"${extra}>${icon}<span>${esc(label)}</span>${k}</button>`;
    this.menu.innerHTML = `
      <div class="vt-label" aria-hidden="true">채널</div>
      ${item('channel', ICON.info, '채널 정보')}
      ${item('follow', CHANNEL_ICON.follow, '팔로우', { narrow: true, extra: ' hidden' })}
      ${item('fav', CHANNEL_ICON.star, '즐겨찾기에 추가', { narrow: true })}
      ${item('copy', CHANNEL_ICON.link, '방송 링크 복사', { narrow: true })}
      ${item('external', CHANNEL_ICON.external, '치지직에서 열기')}
      ${onAddMulti ? item('multi', CHANNEL_ICON.multi, '멀티라이브에 추가') : ''}
      <div class="vt-sep-line"></div>
      <div class="vt-label" aria-hidden="true">방송</div>
      ${item('bookmark', ICON.bookmark, '북마크', { k: key('bookmark') })}
      ${item('shot', ICON.camera, '스크린샷', { k: key('screenshot') })}
      ${item('retry', ICON.retry, '다시 연결')}
      <div class="vt-sep-line"></div>
      <div class="vt-label" aria-hidden="true">보기</div>
      ${item('chatmode', ICON.chat, '채팅 표시 방식', { narrow: true, k: key('chat') })}
      ${onPopout ? item('popout', ICON.popout, '새 창으로 분리', { narrow: true, k: key('popout') }) : ''}
      ${item('pip', ICON.pip, 'PiP', { k: key('pip') })}
      ${item('fullchat', ICON.chatFull, '영상+채팅 전체 화면', { k: key('fullchat') })}`;
    this.stage.append(this.menu);
    bindMenuKeys(this.menu);
    // 앱 전체 Esc(app.js escapeTaken)가 메뉴를 바로 숨겨도 단추의 펼침 상태가 따라오게.
    new MutationObserver(() => { if (this.menu.hidden) this.moreButton.setAttribute('aria-expanded', 'false'); })
      .observe(this.menu, { attributes: true, attributeFilter: ['hidden'] });

    root.addEventListener('click', (e) => this.onClick(e));
    document.addEventListener('pointerdown', (e) => {
      if (!this.menu.hidden && !this.menu.contains(e.target) && !this.moreButton.contains(e.target)) this.closeMenu();
    });
    this.menu.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); this.closeMenu(true); } else if (e.key === 'Tab') this.closeMenu();
    });
    window.addEventListener('resize', () => fitPopover(this.menu, this.stage));
    onShortcuts(() => this.applyShortcutHints());
    this.applyShortcutHints();
    // 팔로우 단추·배지가 늦게 나타나도(캐시가 비어 늦게 온 팔로우 상태, 시청 중 로그인) 레일 접기를 다시 잰다 —
    // stage-top 은 overflow:hidden 이라 제 폭이 그대로여서 viewer-extras 의 ResizeObserver 가 울리지 않고, 오른쪽 ⋯·× 가 밖으로 밀렸다.
    this.refit = new ResizeObserver(() => this.viewer.extras?.fitRail());
    for (const g of top.querySelectorAll('.vt-group')) this.refit.observe(g);
    this.refit.observe(this.badge);
    window.addEventListener(CHANNEL_SOCIAL_EVENT, (e) => {
      const d = e.detail || {};
      if (!this.social || d.channelId !== this.viewer.channelId) return;
      if (typeof d.following === 'boolean') this.social.following = d.following;
      if (typeof d.favorite === 'boolean') this.social.favorite = d.favorite;
      this.renderSocial();
    });
  }

  /** 헤더를 다시 그린 뒤(viewer.renderMeta) 지표 줄 끝에 배지를 다시 붙인다. */
  attachBadge() {
    const stats = this.viewer.header.querySelector('.meta-stats');
    if (!stats || this.badge.parentElement === stats) return;
    // 칩 묶음(.meta-chips, display: contents)의 형제 — 자리는 CSS order 로 경과 뒤·누적 앞(넘치면 누적이 먼저 빠진다).
    stats.append(this.badge);
  }

  /** 화질·지연 — 플레이어 지표가 올 때마다(viewer-controls renderStatus). 바뀔 때만 쓴다. */
  syncBadge({ height = 0, latency = 0, auto = true, codec = '' } = {}) {
    const lat = Number.isFinite(latency) && latency > 0 ? Math.round(latency * 10) / 10 : 0;
    const k = `${height}|${lat}|${auto}|${codec}`;
    this.attachBadge();
    if (this.badge.dataset.key === k) return;
    this.badge.dataset.key = k;
    this.badge.hidden = !height && !lat;
    this.badge.querySelector('.vt-q-res').textContent = height ? `${height}p` : '';
    this.badge.querySelector('.vt-q-lat').textContent = lat ? `${lat.toFixed(1)}초` : '';
    this.badge.classList.toggle('behind', lat > 8);
    const label = qualityBadgeLabel({ height, codec, auto, latency: lat });
    this.badge.title = `${label} — 눌러서 화질 메뉴`;
    this.badge.setAttribute('aria-label', `${label}, 화질 메뉴`);
  }

  async load(channelId) {
    this.social = null;
    this.renderSocial();
    this.badge.dataset.key = '';
    this.syncBadge();
    if (!channelId) return;
    const s = await loadChannelSocial(channelId, this.isLoggedIn());
    if (channelId !== this.viewer.channelId) return;
    this.social = s;
    this.renderSocial();
  }

  renderSocial() {
    const s = this.social;
    const name = this.viewer.live?.channelName || '채널';
    for (const b of this.viewer.root.querySelectorAll('[data-vt="follow"]')) {
      const known = Boolean(s) && typeof s.following === 'boolean';
      b.hidden = !known;
      if (!known) continue;
      const on = s.following;
      b.classList.toggle('on', on);
      b.disabled = Boolean(this.busy);
      const hint = on ? `${name} 팔로잉 중 — 눌러서 팔로우 끊기` : `${name} 팔로우`;
      b.title = hint;
      if (b.classList.contains('vt-item')) {
        b.querySelector('span').textContent = on ? '팔로우 끊기' : '팔로우';
      } else {
        b.setAttribute('aria-label', on ? `팔로잉 — ${name} 팔로우 끊기` : `팔로우 — ${name}`);
        b.querySelector('.vt-label').textContent = on ? '팔로잉' : '팔로우';
        b.querySelector('.vt-ico').innerHTML = on ? CHANNEL_ICON.following : CHANNEL_ICON.follow;
      }
    }
    for (const b of this.viewer.root.querySelectorAll('[data-vt="fav"]')) {
      const on = Boolean(s && s.favorite);
      b.classList.toggle('on', on);
      b.title = on ? '즐겨찾기에서 빼기' : '즐겨찾기에 추가';
      if (b.classList.contains('vt-item')) {
        b.querySelector('span').textContent = b.title;
      } else {
        b.setAttribute('aria-pressed', String(on));
        if (b.dataset.icon !== String(on)) { b.innerHTML = on ? CHANNEL_ICON.starOn : CHANNEL_ICON.star; b.dataset.icon = String(on); }
      }
    }
  }

  applyShortcutHints() {
    const b = currentBindings();
    const label = (x) => (x && x.key ? bindingLabel(x) : '');
    const keys = {
      chat: label(b.toggleChat), pip: label(b.togglePiP), screenshot: label(b.screenshot) || kbd('⌘S'),
      bookmark: 'B', popout: kbd('⌘N'), fullchat: kbd('⌃⌘F'),
    };
    for (const k of this.menu.querySelectorAll('[data-key-of]')) k.textContent = keys[k.dataset.keyOf] || '';
  }

  get isOpen() { return !this.menu.hidden; }

  toggleMenu() {
    if (this.isOpen) { this.closeMenu(true); return; }
    this.viewer.controls.closeQualityMenu();
    this.menu.hidden = false;
    this.moreButton.setAttribute('aria-expanded', 'true');
    fitPopover(this.menu, this.stage);
    if (this.moreButton.matches(':focus-visible')) focusFirstMenuItem(this.menu);
    this.viewer.controls.wake?.();
  }

  closeMenu(refocus = false) {
    if (this.menu.hidden) return;
    const inside = this.menu.contains(document.activeElement);
    hideWithExit(this.menu);
    this.moreButton.setAttribute('aria-expanded', 'false');
    if (refocus || inside) this.moreButton.focus({ preventScroll: true });
  }

  async onClick(e) {
    const b = e.target.closest('[data-vt]');
    if (!b) return;
    const v = this.viewer;
    const id = v.channelId;
    const act = b.dataset.vt;
    if (act === 'more') { e.stopPropagation(); this.toggleMenu(); return; }
    if (b.closest('.vt-menu')) this.closeMenu();
    if (!id) return;
    switch (act) {
      case 'follow': {
        const s = this.social;
        if (!s || typeof s.following !== 'boolean' || this.busy) break;
        this.busy = true;
        this.renderSocial();
        const next = await setFollow(id, v.live?.channelName, !s.following);
        this.busy = false;
        if (next !== null && id === v.channelId) s.following = next;
        this.renderSocial();
        break;
      }
      case 'fav': {
        const s = this.social || (this.social = { following: null, favorite: false });
        const next = await setFavorite(id, v.live?.channelName, v.live?.channelImageUrl, !s.favorite);
        if (next !== null && id === v.channelId) s.favorite = next;
        this.renderSocial();
        break;
      }
      case 'copy': copyLiveLink(id); break;
      case 'external': openOnChzzk(id); break;
      case 'channel': window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: id } })); break;
      case 'multi': this.onAddMulti?.(id); break;
      case 'popout': this.onPopout?.(); break;
      case 'bookmark': v.controls.bookmark(); break;
      case 'shot': v.controls.screenshot(); break;
      case 'retry': v.open(id, v.lastSettings); break;
      case 'chatmode': v.extras.cycleMode(); break;
      case 'pip': v.controls.togglePip(); break;
      case 'fullchat': v.controls.fullscreen(true); break;
      default: break;
    }
  }
}
