// 멀티라이브 패인 재생 컨트롤 — 치지직 플레이어식 아래 막대를 패인마다 작게.
//
// 재생/일시정지 · 소리(아이콘에 올리면 볼륨 펼침) · LIVE(지연 따라잡기) · 화질/지연 톱니 메뉴 · PiP · 크게 · 전체 화면.
// 단일 시청 화면(viewer-controls.js)의 화질 목록·LIVE 배지·PiP 코드를 같이 쓴다.
// - 들리는 패인은 Rust 가 setVolume(0/1) 로 정한다. 패인 볼륨은 그 위에 곱하는 배율이라
//   여러 소리 모드와 함께 써도 서로 덮어쓰지 않는다.
// - 마우스를 움직이면 나타나고 재생 중 2.5초 가만히 있으면 커서와 함께 숨는다. 일시정지·메뉴 열림이면 계속 보인다.
import { esc, toast } from './dom.js';
import { qualityOptions, liveBadgeState, togglePipFor, installVolumeReveal } from './viewer-controls.js';
import { ADJUST_RANGES, applyPicture, pictureFor, resetAdjust, setPicture } from './picture.js';

const IDLE_MS = 2500;

export const PANE_ICON = {
  play: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5-13-7.5Z"/></svg>',
  pause: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
  sound: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></svg>',
  muted: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="m16 9.5 5 5M21 9.5l-5 5"/></svg>',
  gear: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"/></svg>',
  pip: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><rect x="12" y="11" width="7" height="6" rx="1" fill="currentColor" stroke="none"/></svg>',
  expand: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/></svg>',
  fullscreen: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></svg>',
  more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5.5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/></svg>',
  chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  info: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5M12 7.6v.2"/></svg>',
  live: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="13" rx="2"/><path d="M8 21h8M10.5 9.5v5l4-2.5z"/></svg>',
  retry: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11.5a8 8 0 1 0-2.4 5.9"/><path d="M20 4.5v7h-7"/></svg>',
};

/** 패인 아래 막대·가운데 재생·메뉴 마크업. */
export function paneControlsHtml() {
  const I = PANE_ICON;
  return `
      <button class="ml-center-play" data-mp="play" aria-label="재생">${I.play}</button>
      <div class="spinner ml-buffer" aria-hidden="true"></div>
      <div class="ml-pane-bar">
        <button class="ml-pane-btn" data-mp="play" title="일시정지" aria-label="일시정지">${I.pause}</button>
        <div class="ml-vol">
          <button class="ml-pane-btn" data-ml="audio" title="이 패인 소리 켜기" aria-label="소리">${I.muted}</button>
          <input type="range" class="ml-vol-range" min="0" max="100" value="100" aria-label="패인 볼륨">
        </div>
        <button class="ml-live-pill" data-mp="live" title="라이브로 이동">LIVE</button>
        <span class="ml-pane-cap"></span>
        <div class="grow"></div>
        <button class="ml-pane-btn" data-mp="gear" title="화질·지연" aria-label="화질·지연" aria-haspopup="menu">${I.gear}</button>
        <button class="ml-pane-btn ml-sec" data-mp="pip" title="PiP" aria-label="PiP">${I.pip}</button>
        <button class="ml-pane-btn ml-sec" data-ml="single" title="이 채널만 크게 (더블클릭)" aria-label="이 채널만 크게">${I.expand}</button>
        <button class="ml-pane-btn" data-mp="fullscreen" title="이 패인 전체 화면" aria-label="전체 화면">${I.fullscreen}</button>
      </div>
      <div class="ml-pane-menu ml-gear-menu" role="menu" hidden></div>
      <div class="ml-pane-menu ml-more-pane" role="menu" hidden>
        <button class="ml-menu-item ml-watch-btn" data-ml="watch">${I.live}<span>라이브로 크게 보기</span></button>
        <button class="ml-menu-item" data-ml="select">${I.chat}<span>이 패인 채팅 보기</span></button>
        <button class="ml-menu-item" data-ml="info">${I.info}<span>채널 정보</span></button>
        <button class="ml-menu-item" data-ml="retry">${I.retry}<span>다시 연결</span></button>
        <button class="ml-menu-item ml-narrow-only" data-mp="pip">${I.pip}<span>PiP</span></button>
        <button class="ml-menu-item ml-narrow-only" data-ml="single">${I.expand}<span>이 채널만 크게</span></button>
      </div>`;
}

export class PaneControls {
  /**
   * @param {{ el: HTMLElement, bridge: import('./player.js').PlayerBridge }} pane
   * @param {{ makeAudible: () => void }} hooks
   */
  constructor(pane, hooks) {
    this.pane = pane;
    this.el = pane.el;
    this.bridge = pane.bridge;
    this.hooks = hooks;
    this.levels = [];
    this.manualIndex = -1;
    this.latency = 0;
    this.gain = 1;
    this.audible = false;
    this.playButton = this.el.querySelector('.ml-pane-bar [data-mp="play"]');
    this.liveButton = this.el.querySelector('[data-mp="live"]');
    this.gearMenu = this.el.querySelector('.ml-gear-menu');
    this.moreMenu = this.el.querySelector('.ml-more-pane');
    this.range = this.el.querySelector('.ml-vol-range');
    this.audioButton = this.el.querySelector('.ml-vol [data-ml="audio"]');

    this.el.addEventListener('click', (e) => this.onClick(e));
    this.gearMenu.addEventListener('input', (e) => this.onAdjustInput(e));
    this.range.addEventListener('input', () => this.setGain(Number(this.range.value) / 100));
    this.volumeReveal = installVolumeReveal(this.el.querySelector('.ml-vol'), this.range);
    this.syncRangeFill();
    this.bridge.onMessage((type, data) => this.onMessage(type, data));
    // 플레이어 문서가 새로 뜨면 볼륨 배율을 다시 걸고, 영상 위 휠을 받는다.
    this.bridge.iframe.addEventListener('load', () => { this.patchVolume(); this.hookWheel(); });
    this.el.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    this.hookWheel();

    const wake = () => this.wake();
    this.el.addEventListener('mousemove', wake);
    this.el.addEventListener('mousedown', wake);
    this.el.addEventListener('mouseleave', () => { clearTimeout(this.idleTimer); this.idleTimer = setTimeout(() => this.hide(), 400); });
    this.el.addEventListener('focusin', wake);
    // 패인 밖을 누르거나 Esc 면 메뉴를 닫는다.
    document.addEventListener('pointerdown', (e) => { if (this.menuOpen && !this.el.contains(e.target)) this.closeMenus(); });
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && this.menuOpen) this.closeMenus(); });
  }

  get video() {
    try { return this.bridge.iframe.contentDocument?.getElementById('video') || null; } catch { return null; }
  }

  // ── 자동 숨김 ─────────────────────────────

  get menuOpen() { return !this.gearMenu.hidden || !this.moreMenu.hidden; }

  wake() {
    this.el.classList.add('ctl-visible');
    this.el.classList.remove('idle');
    clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => this.hide(), IDLE_MS);
  }

  hide() {
    const hovering = Boolean(this.el.querySelector('.ml-pane-bar:hover, .ml-pane-top:hover, .ml-pane-menu:hover')) || this.volumeReveal.isOpen();
    if (this.el.classList.contains('paused') || this.menuOpen || hovering) { this.wake(); return; }
    this.el.classList.remove('ctl-visible');
    this.el.classList.toggle('idle', this.el.matches(':hover'));
  }

  closeMenus() {
    this.gearMenu.hidden = true;
    this.moreMenu.hidden = true;
  }

  // ── 상태 ─────────────────────────────────

  onMessage(type, data) {
    if (type === 'levels') {
      this.levels = qualityOptions(data.levels);
      this.manualIndex = typeof data.current === 'number' ? data.current : this.manualIndex;
      if (!this.gearMenu.hidden) this.renderGear();
    } else if (type === 'metrics') {
      if (typeof data.latency === 'number') this.latency = data.latency;
    } else if (type === 'playbackState') {
      this.el.classList.toggle('buffering', data.state === 'stalled');
    } else if (type === 'playStarted' || type === 'ready') {
      this.patchVolume();
      this.applyPicture();
    }
  }

  /** 주기적으로(페이지가 보일 때) 영상 상태를 막대에 맞춘다. */
  sync(audible) {
    this.audible = audible;
    const v = this.video;
    const paused = Boolean(v && v.paused && v.readyState > 0);
    if (paused !== this.el.classList.contains('paused')) {
      this.el.classList.toggle('paused', paused);
      if (paused) this.wake();
    }
    this.el.classList.toggle('buffering', Boolean(v && !v.paused && v.readyState > 0 && v.readyState < 3));
    const playing = v && !v.paused;
    // 아이콘은 바뀔 때만 다시 그린다(700ms 마다 SVG 를 다시 파싱하지 않게).
    if (this.playButton.dataset.playing !== String(Boolean(playing))) {
      this.playButton.dataset.playing = String(Boolean(playing));
      this.playButton.innerHTML = playing ? PANE_ICON.pause : PANE_ICON.play;
      this.playButton.title = playing ? '일시정지' : '재생';
    }
    const badge = liveBadgeState(this.latency);
    // 700ms 마다 패인 수만큼 돈다 — 같은 값을 다시 쓰면(텍스트 노드 교체·title) 매번 스타일·레이아웃이 무효화된다.
    if (this.liveButton.textContent !== badge.text) this.liveButton.textContent = badge.text;
    this.liveButton.classList.toggle('behind', badge.behind || !playing);
    const muted = !audible || this.gain === 0;
    const icon = muted ? PANE_ICON.muted : PANE_ICON.sound;
    if (this.audioButton.dataset.icon !== String(muted)) {
      this.audioButton.innerHTML = icon;
      this.audioButton.dataset.icon = String(muted);
    }
    const audioTitle = audible && this.gain > 0 ? '소리 끄기' : '이 패인 소리 켜기';
    if (this.audioButton.title !== audioTitle) this.audioButton.title = audioTitle;
  }

  // ── 동작 ─────────────────────────────────

  onClick(e) {
    const menuItem = e.target.closest('.ml-pane-menu [data-ml], .ml-pane-menu [data-mp]');
    const action = e.target.closest('[data-mp]');
    if (!e.target.closest('.ml-pane-menu') && !e.target.closest('[data-mp="gear"], [data-mp="more"]')) this.closeMenus();
    // 화면 보정(선명한 화면·밝기 등)은 여러 번 만지므로 메뉴를 닫지 않는다.
    if (menuItem && !e.target.closest('[data-keep-open]')) setTimeout(() => this.closeMenus(), 0);
    if (!action) return;
    e.stopPropagation();
    switch (action.dataset.mp) {
      case 'play': this.togglePlay(); break;
      case 'live': this.goLive(); break;
      case 'gear': this.toggleMenu(this.gearMenu); break;
      case 'more': this.toggleMenu(this.moreMenu); break;
      case 'pip': togglePipFor(this.video); break;
      case 'fullscreen': this.toggleFullscreen(); break;
      case 'level': this.chooseLevel(Number(action.dataset.level)); break;
      case 'sharpen': this.setSharpen(!pictureFor(this.channelId).sharpen); break;
      case 'sharpen-all': this.hooks.applySharpenAll?.(pictureFor(this.channelId).sharpen); break;
      case 'adjust-reset': resetAdjust(this.channelId); this.applyPicture(); this.renderGear(); break;
      default: break;
    }
  }

  toggleMenu(menu) {
    const open = menu.hidden;
    this.closeMenus();
    if (!open) return;
    if (menu === this.gearMenu) {
      this.bridge.call('getLevels');
      this.renderGear();
    }
    menu.hidden = false;
    this.wake();
  }

  togglePlay() {
    const v = this.video;
    if (!v) return;
    this.bridge.call(v.paused ? 'play' : 'pause');
    setTimeout(() => this.sync(this.audible), 150);
  }

  goLive() {
    const v = this.video;
    if (v && v.paused) this.bridge.call('play');
    if (v && v.playbackRate !== 1) this.bridge.call('setRate', 1);
    this.bridge.call('seekToLiveEdge');
    toast('라이브로 이동', 1000);
  }

  toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else if (this.el.requestFullscreen) this.el.requestFullscreen();
  }

  renderGear() {
    const items = [{ index: -1, label: '자동' }, ...this.levels];
    const v = this.video;
    const height = v && v.videoHeight;
    const lat = Number.isFinite(this.latency) && this.latency > 0 ? `${this.latency.toFixed(1)}초` : '—';
    this.gearMenu.innerHTML = `
      <div class="ml-menu-head">화질${height ? ` <span>${height}p</span>` : ''}</div>
      ${items.map((o) => `<button class="ml-menu-item ml-level${o.index === this.manualIndex ? ' active' : ''}" role="menuitemradio" data-mp="level" data-level="${o.index}"><span>${esc(o.label)}</span></button>`).join('')}
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head">지연 <span>${lat}</span></div>
      <button class="ml-menu-item" data-mp="live"><span>라이브로 이동 (따라잡기)</span></button>
      ${this.pictureMenuHtml()}`;
  }

  // ── 화면 보정(선명한 화면·밝기·대비·채도) — 채널마다 기억(picture.js) ──

  pictureMenuHtml() {
    if (!this.channelId) return '';
    const p = pictureFor(this.channelId);
    const sliders = ADJUST_RANGES.map(([key, label, min, max, step]) => `
      <label class="ml-adjust-row" data-keep-open><span>${label}</span>
        <input type="range" data-adj="${key}" min="${min}" max="${max}" step="${step}" value="${p[key]}" aria-label="${label}">
        <output>${Math.round(p[key] * 100)}%</output></label>`).join('');
    return `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head">화면</div>
      <button class="ml-menu-item ml-check" role="menuitemcheckbox" aria-checked="${p.sharpen}" data-mp="sharpen" data-keep-open title="치지직과 같은 언샤프 마스크로 윤곽을 또렷하게 합니다"><span>선명한 화면</span><span class="ml-switch${p.sharpen ? ' on' : ''}"></span></button>
      <button class="ml-menu-item" data-mp="sharpen-all"><span>모든 패인에 ${p.sharpen ? '선명하게' : '선명하게 끄기'}</span></button>
      ${sliders}
      <button class="ml-menu-item" data-mp="adjust-reset" data-keep-open><span>보정 기본값</span></button>`;
  }

  /** 이 패인에 지금 채널을 알린다 — 바뀌면 그 채널의 화면 보정을 건다. */
  setChannel(channelId) {
    if (this.channelId === channelId) return;
    this.channelId = channelId || null;
    this.applyPicture();
  }

  applyPicture() {
    if (this.channelId) applyPicture(this.bridge, pictureFor(this.channelId));
  }

  setSharpen(on) {
    setPicture(this.channelId, { sharpen: Boolean(on) });
    this.applyPicture();
    if (!this.gearMenu.hidden) this.renderGear();
  }

  onAdjustInput(e) {
    const input = e.target.closest('[data-adj]');
    if (!input || !this.channelId) return;
    const value = Number(input.value);
    setPicture(this.channelId, { [input.dataset.adj]: value });
    const out = input.parentElement.querySelector('output');
    if (out) out.textContent = `${Math.round(value * 100)}%`;
    this.applyPicture();
  }

  chooseLevel(index) {
    this.closeMenus();
    if (index < 0) {
      // 자동 — 매니저가 건 패인 화질 상한(applyAdaptiveLevelCap)은 그대로 둔다.
      this.bridge.run('if (typeof setLockTopLevel === "function") setLockTopLevel(false); if (hls) { hls.currentLevel = -1; hls.loadLevel = -1; if (typeof applyAdaptiveLevelCap === "function") applyAdaptiveLevelCap(); }');
    } else {
      this.bridge.run(`if (typeof setLockTopLevel === "function") setLockTopLevel(false); requestManualLevel(${Number(index)});`);
    }
    this.manualIndex = index;
    setTimeout(() => this.bridge.call('getLevels'), 600);
  }

  // ── 패인 볼륨(배율) ───────────────────────

  /** 페이지의 setVolume 을 감싸 Rust 가 정한 0/1 에 패인 배율을 곱한다. */
  patchVolume() {
    let w;
    try { w = this.bridge.iframe.contentWindow; } catch { return; }
    if (!w || typeof w.setVolume !== 'function') return;
    // 치지직처럼 무대는 테마와 상관없이 검정.
    w.setStageBackgroundColor?.('#000');
    if (!w.__cviewPaneGain) {
      const original = w.setVolume;
      w.__cviewPaneGain = { original, raw: w.video ? w.video.volume : 1 };
      w.setVolume = (v) => {
        w.__cviewPaneGain.raw = v;
        original(v * this.gain);
      };
    }
    w.__cviewPaneGain.original(w.__cviewPaneGain.raw * this.gain);
  }

  /**
   * 소리 버튼 — 소리 나는 패인이면 끄고(배율 0), 볼륨 0 이면 이전 값으로 되돌려 켠다. 처리했으면 true.
   * 예전엔 하나만 듣기 모드에서 이 버튼이 "이 패인 선택"뿐이라, 이미 들리는 패인을 눌러도 아무 일이 없었다.
   */
  toggleMute() {
    if (this.gain === 0) {
      const g = this.lastGain > 0 ? this.lastGain : 1;
      this.range.value = String(Math.round(g * 100));
      this.setGain(g);
      return true;
    }
    if (!this.audible) return false;
    this.lastGain = this.gain;
    this.range.value = '0';
    this.setGain(0);
    return true;
  }

  /** 패인(영상 위 포함) 휠 — 패인 볼륨 ±5. */
  onWheel(e) {
    if (e.target.closest && e.target.closest('.ml-pane-menu')) return;
    e.preventDefault();
    const next = Math.min(100, Math.max(0, Number(this.range.value) + (e.deltaY > 0 ? -5 : 5)));
    if (String(next) === this.range.value) return;
    this.range.value = String(next);
    this.setGain(next / 100);
    this.volumeReveal.peek();
    this.wake();
  }

  hookWheel() {
    try {
      const doc = this.bridge.iframe.contentDocument;
      if (!doc || doc.__cviewPaneWheel) return;
      doc.__cviewPaneWheel = true;
      doc.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    } catch { /* 준비 전 */ }
  }

  setGain(gain) {
    this.gain = Math.max(0, Math.min(1, gain));
    this.syncRangeFill();
    if (this.gain > 0 && !this.audible) this.hooks.makeAudible();
    this.patchVolume();
    this.sync(this.audible || this.gain > 0);
  }

  syncRangeFill() {
    this.range.style.setProperty('--fill', `${this.range.value}%`);
  }
}
