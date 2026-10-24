// 시청 화면 부가 기능 — 원본 ChatOverlayView/ChatDisplayMode, LiveChannelHeaderRail, LiveRewindRail(키), StreamAlertOverlayView 위치 조정.
// PLY-12 채팅 표시 모드(사이드·오버레이·숨김), PLY-14 헤더 채널 레일(다른 방송 바로 전환),
// PLY-03 라이브 되감기(←/→ 10초, ⇧ 60초 — 재생 목록이 허용하는 범위 안), PLY-16 후원 알림 위치 끌어 맞추기.
import { api } from './api.js';
import { esc, safeUrl, imageSrc, keyBelongsToTarget } from './dom.js';
import { chatPrefs, onChatPrefs, updateChatPrefs } from './chat-prefs.js';

const MODES = ['side', 'overlay', 'hidden'];
const MODE_LABEL = { side: '채팅: 옆', overlay: '채팅: 영상 위', hidden: '채팅: 숨김' };
const ICON_CHAT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>';

/** 되감기 목표 시각 — 탐색 가능 구간 [start, end] 안으로 자른다. 순수 함수. */
export function seekTarget(current, delta, start, end) {
  if (!Number.isFinite(current) || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) return null;
  return Math.min(end - 0.5, Math.max(start + 0.5, current + delta));
}

export class ViewerExtras {
  /** @param {import('./viewer.js').Viewer} viewer */
  constructor(viewer, { isLoggedIn }) {
    this.viewer = viewer;
    this.isLoggedIn = isLoggedIn;
    const root = viewer.root;
    // 채팅 표시 모드 버튼
    const close = root.querySelector('#viewer-close');
    close.insertAdjacentHTML('beforebegin', `<button class="icon-btn" id="ctl-chatmode" title="채팅 표시 방식" aria-label="채팅 표시 방식">${ICON_CHAT}</button>`);
    this.modeButton = root.querySelector('#ctl-chatmode');
    this.modeButton.addEventListener('click', () => this.cycleMode());
    onChatPrefs(() => this.applyMode());
    this.applyMode();

    // 헤더 채널 레일
    this.rail = document.createElement('div');
    this.rail.className = 'viewer-rail';
    root.querySelector('.viewer-title').after(this.rail);
    this.rail.addEventListener('click', (e) => {
      const b = e.target.closest('[data-rail-channel]');
      if (b) window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: b.dataset.railChannel } }));
      else if (e.target.closest('.rail-more')) window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'following' } }));
    });
    // 좁으면 들어가는 만큼만 보이고 나머지는 "+N" 으로 접는다.
    // 사이드바 접기·펼치기 동안엔 매 프레임 재지 않고(레일 항목을 다 펼쳐 재느라 프레임마다 레이아웃을 두 번 했다) 끝난 뒤 한 번.
    new ResizeObserver(() => {
      if (!document.documentElement.classList.contains('sidebar-animating')) this.fitRail();
    }).observe(this.rail.parentElement);
    window.addEventListener('cview:sidebar-settled', () => this.fitRail());

    document.addEventListener('keydown', (e) => this.onKey(e));
  }

  // ── 채팅 표시 모드 (PLY-12) ────────────────

  get mode() {
    const m = chatPrefs().displayMode;
    return MODES.includes(m) ? m : 'side';
  }

  applyMode() {
    const root = this.viewer.root;
    const m = this.mode;
    for (const x of MODES) root.classList.toggle(`chat-mode-${x}`, x === m);
    const p = chatPrefs();
    root.style.setProperty('--overlay-w', `${Math.min(560, Math.max(240, Number(p.overlayWidth) || 340))}px`);
    root.style.setProperty('--overlay-bg', String(Math.min(0.95, Math.max(0.1, Number(p.overlayBackgroundOpacity) || 0.5))));
    this.modeButton.title = `${MODE_LABEL[m]} — 눌러서 바꾸기`;
    this.modeButton.classList.toggle('active', m !== 'side');
  }

  cycleMode() {
    const next = MODES[(MODES.indexOf(this.mode) + 1) % MODES.length];
    updateChatPrefs({ displayMode: next });
    this.viewer.controls.flash(MODE_LABEL[next]);
  }

  // ── 헤더 채널 레일 (PLY-14) ────────────────

  async loadRail(currentId) {
    this.rail.innerHTML = '';
    let lives = [];
    try {
      lives = this.isLoggedIn() ? await api.followingLives(false) : await api.popularLives(20, false);
    } catch { return; }
    if (this.viewer.channelId !== currentId) return;
    const others = lives.filter((l) => l.channelId !== currentId).slice(0, 10);
    this.rail.innerHTML = others.map((l) => {
      const avatar = safeUrl(l.channelImageUrl);
      return `<button class="rail-item" data-rail-channel="${esc(l.channelId)}" title="${esc(l.channelName)} · ${esc(l.liveTitle || '')}">${avatar ? `<img class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : `<span class="avatar">${esc((l.channelName || '?').slice(0, 1))}</span>`}</button>`;
    }).join('') + '<button class="rail-more" hidden></button>';
    this.fitRail();
  }

  fitRail() {
    const items = [...this.rail.querySelectorAll('.rail-item')];
    const more = this.rail.querySelector('.rail-more');
    if (!more) return;
    // 한 번 재고 계산한다(예전엔 하나 숨길 때마다 scrollWidth 를 읽어 항목 수만큼 레이아웃을 다시 했다).
    for (const b of items) b.hidden = false;
    more.hidden = true;
    const top = this.rail.parentElement;
    const overflow = top.scrollWidth - top.clientWidth;
    if (overflow <= 0 || !items.length) return;
    const gap = parseFloat(getComputedStyle(this.rail).columnGap) || 6;
    const widths = items.map((b) => b.getBoundingClientRect().width + gap);
    const moreWidth = 44; // "+N" 알약 대략 폭(간격 포함)
    let freed = 0;
    let hiddenCount = 0;
    for (let i = items.length - 1; i >= 0 && freed < overflow + moreWidth; i--) {
      freed += widths[i];
      hiddenCount++;
    }
    for (let i = items.length - hiddenCount; i < items.length; i++) items[i].hidden = true;
    more.hidden = false;
    more.textContent = `+${hiddenCount}`;
    more.title = `팔로잉 라이브 ${hiddenCount}개 더 — 팔로잉 화면에서 보기`;
  }

  // ── 라이브 되감기 키 (PLY-03, 부분) ────────

  onKey(e) {
    const v = this.viewer;
    if (v.root.hidden || !v.isPlaying || e.metaKey || e.ctrlKey || e.altKey || keyBelongsToTarget(e)) return;
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const video = v.controls.video;
    if (!video || !video.seekable || !video.seekable.length) return;
    e.preventDefault();
    const step = (e.shiftKey ? 60 : 10) * (e.key === 'ArrowLeft' ? -1 : 1);
    const start = video.seekable.start(0);
    const end = video.seekable.end(video.seekable.length - 1);
    const target = seekTarget(video.currentTime, step, start, end);
    if (target === null) return;
    if (Math.abs(target - video.currentTime) < 1) {
      v.controls.flash(step < 0 ? '더 되감을 수 없어요' : '라이브 끝입니다');
      return;
    }
    v.player.call('seek', target);
    const behind = Math.max(0, Math.round(end - target));
    v.controls.flash(behind > 2 ? `라이브보다 ${behind}초 뒤` : '라이브');
  }

  // ── 후원 알림 위치 조정 (PLY-16) ────────────

  /** 견본 알림을 띄운다 — 끌어 놓으면 그 자리가 저장된다(실제 알림도 바로 끌 수 있다). */
  startAlertPositioning() {
    this.viewer.alerts.preview();
  }
}
