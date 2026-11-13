// 멀티라이브 패인 재생 컨트롤 — 치지직 플레이어식 아래 막대를 패인마다 작게.
//
// 재생/일시정지 · 소리(아이콘에 올리면 볼륨 펼침) · LIVE(지연 따라잡기) · 화질/지연 톱니 메뉴 · PiP · 크게 · 전체 화면.
// 단일 시청 화면(viewer-controls.js)의 화질 목록·LIVE 배지·PiP 코드를 같이 쓴다.
// - 들리는 패인은 Rust 가 setVolume(0/1) 로 정한다. 패인 볼륨은 그 위에 곱하는 배율이라
//   여러 소리 모드와 함께 써도 서로 덮어쓰지 않는다.
// - 마우스를 움직이면 나타나고 재생 중 2.5초 가만히 있으면 커서와 함께 숨는다. 일시정지·메뉴 열림이면 계속 보인다.
// - 모든 패인 ⚙ 에 '선명도'(끔·약·중·강 + 크게 볼 때만 적용, sharpness.js) — 예전 '선명한 화면'과 '업스케일'을
//   합친 전역 설정이다. 단독 보기 무대 패인만 확대(업스케일)를 허용하고, 그리드 패인은 윤곽만 또렷하게 한다.
// - 라이브 단독 보기의 무대 패인(setStage)은 ⚙ 에 예전 시청 화면 고급 패널의 재생 도구를 더 보인다 —
//   배속·화면 꽉 채우기·자동 밝기·색 보정·픽셀 그대로 확대·이퀄라이저(Windows). 배속 말고는 전역 플레이어
//   설정(player-tools.js)이라 시청 화면과 같은 값을 쓰고, 무대에 오를 때·새 재생마다 다시 걸고 내려오면 되돌린다.
//   그리드 패인은 작고 여러 개라 화질·지연·선명도·채널별 색 보정만 둔다.
import { esc, toast, hideWithExit } from './dom.js';
import { qualityOptions, liveBadgeState, togglePipFor, installVolumeReveal, toggleFullscreenOn } from './viewer-controls.js';
import { ADJUST_RANGES, applyPicture, pictureFor, resetAdjust, setPicture } from './picture.js';
import {
  SHARPNESS_LEVELS, SHARPNESS_HELP, ENLARGED_ONLY_LABEL, ENLARGED_ONLY_HELP,
  sharpnessState, sharpnessReady, sharpnessArgs, setSharpness, sharpnessNote, sharpnessShort,
} from './sharpness.js';
import { watchBarHtml, watchBarEndHtml, WATCH_ICON } from './ml-watch.js';
import {
  RATES, RATE_NOTE, EQ_SUPPORTED, EQ_FREQS, freqLabel, loadEqPresets, loadPlayerSettings, savePlayerSettings,
  applyPlayerTools, clearPlayerTools, eqWanted, eqBands, eqOnPatch,
} from './player-tools.js';

const IDLE_MS = 2500;

// 단독 보기 ⚙ 의 전역 플레이어 설정 스위치 — [설정 키, 이름, 설명]
const STAGE_TOOLS = [
  ['autoPictureEnhancement', '자동 밝기·색 보정', '어둡거나 뿌연 장면의 밝기·대비·채도를 천천히 맞춥니다'],
  ['sharpPixelScaling', '픽셀 그대로 확대', '확대할 때 픽셀을 부드럽게 하지 않습니다 — 선명도가 처리하는 동안에는 보이지 않습니다'],
];
const GLOBAL_NOTE = '플레이어 설정 — 시청 화면과 단독 보기에 함께 적용';
// 다시 그린 메뉴에서 같은 항목으로 초점을 되돌릴 때 보는 속성
const FOCUS_KEYS = ['data-mp', 'data-level', 'data-rate', 'data-sharpness', 'data-key', 'data-adj', 'data-eq-band', 'data-eq-preamp', 'data-eq-preset'];

/** 메뉴 안에서 보이는(누를 수 있는) 항목 — 숨은 좁은 화면 전용 항목은 뺀다. */
function menuItems(menu) {
  return [...menu.querySelectorAll('button:not(:disabled), input, select')].filter((el) => el.getClientRects().length > 0);
}

/** 메뉴의 첫 항목으로 초점을 옮긴다(키보드로 열었을 때). */
export function focusFirstMenuItem(menu) {
  menuItems(menu)[0]?.focus({ preventScroll: true });
}

/**
 * 메뉴 키보드 — ↑↓ 로 항목 사이를 돌고 Home/End 로 처음·끝. 슬라이더(range) 위의 ↑↓ 는 값 조절이라 그대로 둔다.
 * 머리줄 ⋯ 메뉴·채팅 보기 메뉴·패인 ⚙/⋯ 메뉴가 같이 쓴다.
 */
export function bindMenuKeys(menu) {
  menu.addEventListener('keydown', (e) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key) || e.target.matches?.('input[type="range"], select')) return;
    const items = menuItems(menu);
    if (!items.length) return;
    e.preventDefault();
    const i = items.indexOf(document.activeElement);
    let next;
    if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = items.length - 1;
    else if (i < 0) next = e.key === 'ArrowDown' ? 0 : items.length - 1;
    else next = (i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next].focus({ preventScroll: true });
  });
}

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
      <span class="ml-paused-label" aria-hidden="true">일시정지됨</span>
      <div class="spinner ml-buffer" aria-hidden="true"></div>
      <div class="ml-pane-bar">
        <button class="ml-pane-btn" data-mp="play" title="일시정지" aria-label="일시정지">${I.pause}</button>
        <div class="ml-vol">
          <button class="ml-pane-btn" data-ml="audio" title="이 패인 소리 켜기" aria-label="소리">${I.muted}</button>
          <input type="range" class="ml-vol-range" min="0" max="100" value="100" aria-label="패인 볼륨">
        </div>
        <button class="ml-live-pill at-edge" data-mp="live" title="라이브로 이동">LIVE</button>
        <span class="ml-pane-cap"></span>
        <div class="grow"></div>${watchBarHtml()}
        <button class="ml-pane-btn" data-mp="gear" title="화질·지연" aria-label="화질·지연" aria-haspopup="menu" aria-expanded="false">${I.gear}</button>
        <button class="ml-pane-btn ml-sec" data-mp="pip" title="PiP" aria-label="PiP">${I.pip}</button>
        <button class="ml-pane-btn ml-sec" data-ml="single" title="이 채널만 크게 (더블클릭)" aria-label="이 채널만 크게">${I.expand}</button>
        <button class="ml-pane-btn" data-mp="fullscreen" title="이 패인 전체 화면" aria-label="전체 화면">${I.fullscreen}</button>${watchBarEndHtml()}
      </div>
      <div class="ml-pane-menu ml-gear-menu" role="menu" hidden></div>
      <div class="ml-pane-menu ml-more-pane" role="menu" hidden>
        <button class="ml-menu-item ml-watch-btn" data-ml="watch">${I.live}<span>라이브로 크게 보기</span></button>
        <button class="ml-menu-item" data-ml="select">${I.chat}<span>이 패인 채팅 보기</span></button>
        <button class="ml-menu-item" data-ml="info">${I.info}<span>채널 정보</span></button>
        <button class="ml-menu-item" data-ml="retry">${I.retry}<span>다시 연결</span></button>
        <button class="ml-menu-item ml-narrow-only" data-mp="pip">${I.pip}<span>PiP</span></button>
        <button class="ml-menu-item ml-narrow-only" data-ml="single">${I.expand}<span>이 채널만 크게</span></button>
        <button class="ml-menu-item ml-watch-only" data-mw="chatmode">${WATCH_ICON.chat}<span>채팅 표시 방식</span></button>
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
    this.stage = false; // 라이브 단독 보기의 무대 패인인가(multilive.render 가 정한다)
    this.rate = 1; // 이 재생의 배속 — 저장하지 않는다(새 재생·LIVE·무대에서 내려오면 1)
    this.playerSettings = null; // 무대일 때 쓰는 전역 플레이어 설정
    this.settingsSeq = 0;
    this.eqApplied = false;
    this.eq = { freqs: EQ_FREQS, presets: [] };
    this.sharpMsg = null; // 이 패인 플레이어가 마지막으로 알린 선명도 경로('sharpness')
    this.sharpSent = null; // 준비된 플레이어 문서에 마지막으로 보낸 setSharpness 인자(같으면 다시 보내지 않는다)
    this.playButton = this.el.querySelector('.ml-pane-bar [data-mp="play"]');
    this.liveButton = this.el.querySelector('[data-mp="live"]');
    this.gearMenu = this.el.querySelector('.ml-gear-menu');
    this.moreMenu = this.el.querySelector('.ml-more-pane');
    this.range = this.el.querySelector('.ml-vol-range');
    this.audioButton = this.el.querySelector('.ml-vol [data-ml="audio"]');

    this.el.addEventListener('click', (e) => this.onClick(e));
    bindMenuKeys(this.gearMenu);
    bindMenuKeys(this.moreMenu);
    this.gearMenu.addEventListener('input', (e) => this.onAdjustInput(e));
    this.gearMenu.addEventListener('change', (e) => this.onMenuChange(e));
    this.range.addEventListener('input', () => this.setGain(Number(this.range.value) / 100));
    this.volumeReveal = installVolumeReveal(this.el.querySelector('.ml-vol'), this.range);
    this.syncRangeFill();
    this.bridge.onMessage((type, data) => this.onMessage(type, data));
    // 선명도는 전역 — 어느 화면(다른 패인 ⚙·시청 화면·설정)에서 바꿔도 이 패인에 바로 다시 건다.
    window.addEventListener('cview:sharpness', () => {
      this.applySharpness();
      if (!this.gearMenu.hidden) this.renderGear();
      this.syncGearTitle();
    });
    sharpnessReady().then(() => { this.applySharpness(); this.syncGearTitle(); });
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
    document.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || !this.menuOpen) return;
      // 메뉴 안에서 Esc 면 연 버튼으로 초점을 돌려준다(메뉴가 숨으며 초점이 body 로 떨어진다)
      const back = this.el.contains(document.activeElement) && document.activeElement.closest('.ml-pane-menu') ? this.menuButton : null;
      e.preventDefault();
      this.closeMenus();
      back?.focus({ preventScroll: true });
    });
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
    const hovering = Boolean(this.el.querySelector('.ml-pane-bar:hover, .ml-pane-top:hover, .ml-pane-menu:hover, :is(button, input, select):focus-visible')) || this.volumeReveal.isOpen();
    if (this.el.classList.contains('paused') || this.menuOpen || hovering) { this.wake(); return; }
    this.el.classList.remove('ctl-visible');
    this.el.classList.toggle('idle', this.el.matches(':hover'));
  }

  closeMenus() {
    hideWithExit(this.gearMenu);
    hideWithExit(this.moreMenu);
    for (const b of this.el.querySelectorAll('[data-mp="gear"], [data-mp="more"]')) b.setAttribute('aria-expanded', 'false');
  }

  // ── 상태 ─────────────────────────────────

  onMessage(type, data) {
    if (type === 'levels') {
      this.levels = qualityOptions(data.levels);
      // manual — 고른 화질(-1 자동). current 는 지금 재생 중인 화질이라 자동에서도 0 이상이다.
      this.manualIndex = typeof data.manual === 'number' ? data.manual : (typeof data.current === 'number' ? data.current : this.manualIndex);
      if (!this.gearMenu.hidden) this.renderGear();
    } else if (type === 'metrics') {
      if (typeof data.latency === 'number') this.latency = data.latency;
    } else if (type === 'playbackState') {
      this.el.classList.toggle('buffering', data.state === 'stalled');
    } else if (type === 'sharpness') {
      this.sharpMsg = data;
      this.syncSharpnessNote();
    } else if (type === 'playStarted' || type === 'ready') {
      this.patchVolume();
      this.applyPicture();
      this.applySharpness(true); // 새 플레이어 문서·새 재생 — 앞 문서에 보낸 값은 사라졌다
      if (this.stage) {
        // 새 재생·새 플레이어 문서는 1배속(예전 시청 화면과 같다) — 메뉴가 고른 배속과 어긋나지 않게 맞춘다.
        if (this.rate !== 1) this.normalRate();
        this.applyTools();
      }
    } else if (type === 'equalizer' && this.stage && data && data.ok === false) {
      toast('이퀄라이저를 켤 수 없습니다 — 이 방송은 오디오 처리를 쓸 수 없습니다');
    }
  }

  /**
   * 슬롯에 다른 세션이 들어왔을 때 — 앞 채널의 볼륨 배율(0 이면 새 채널도 무음)·되돌릴 볼륨·수동 화질·지연을 넘기지 않는다.
   * 화면 보정은 채널마다 기억하므로 setChannel 이 따로 건다.
   */
  reset() {
    const manual = this.manualIndex >= 0;
    this.gain = 1;
    this.lastGain = undefined;
    this.manualIndex = -1;
    this.latency = 0;
    this.levels = [];
    this.audible = false;
    this.rate = 1; // 새 채널의 플레이어는 1배속으로 시작한다(loadSource)
    this.range.value = '100';
    this.syncRangeFill();
    this.closeMenus();
    this.el.classList.remove('audible', 'paused', 'buffering');
    // 앞 채널에서 고른 화질 고정을 푼다(자동 — 매니저의 화질 상한은 그대로).
    if (manual) this.bridge.run('try { if (typeof setLockTopLevel === "function") setLockTopLevel(false); if (hls) { hls.currentLevel = -1; hls.loadLevel = -1; if (typeof applyAdaptiveLevelCap === "function") applyAdaptiveLevelCap(); } } catch (e) {}');
    this.patchVolume();
  }

  /** 주기적으로(페이지가 보일 때) 영상 상태를 막대에 맞춘다. audible 은 Rust 가 정한 '들리는 패인'. */
  sync(audible) {
    this.audible = audible;
    // 소리 배지·초록 아이콘은 실제로 소리가 날 때만(볼륨 0 으로 끈 패인은 '소리 남'이 아니다).
    this.el.classList.toggle('audible', Boolean(audible) && this.gain > 0);
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
    // 라이브 끝이면 숨긴다 — "LIVE" 는 위쪽 방송 정보 칩이 이미 보여 준다. 늦어지면 "-N초"(누르면 따라잡기)로 나타난다.
    this.liveButton.classList.toggle('at-edge', !badge.behind);
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
    const menuItem = e.target.closest('.ml-pane-menu [data-ml], .ml-pane-menu [data-mp], .ml-pane-menu [data-mw]');
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
      case 'sharpness': this.chooseSharpness(Number(action.dataset.sharpness)); break;
      case 'sharpness-enlarged': this.toggleEnlargedOnly(); break;
      case 'adjust-reset': resetAdjust(this.channelId); this.applyPicture(); this.renderGear(); break;
      case 'rate': this.setRate(Number(action.dataset.rate)); break;
      case 'tool': this.toggleTool(action.dataset.key); break;
      case 'eq': this.toggleEq(); break;
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
      // 무대 패인 — 다른 곳(설정·시청 화면)에서 바꾼 플레이어 설정을 다시 읽는다. 값이 같으면 다시 그리지 않는다.
      if (this.stage) this.refreshSettings();
    }
    menu.hidden = false;
    this.menuButton = this.el.querySelector(`[data-mp="${menu === this.gearMenu ? 'gear' : 'more'}"]`);
    this.menuButton?.setAttribute('aria-expanded', 'true');
    // 키보드로 열었으면 첫 항목으로 — 마우스로 열었으면 초점은 그대로(:focus-visible 이 아니다)
    if (this.menuButton?.matches(':focus-visible')) focusFirstMenuItem(menu);
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
    this.normalRate();
    this.bridge.call('seekToLiveEdge');
    toast('라이브로 이동', 1000);
  }

  /** 1배속으로 — LIVE(따라잡기)·새 재생·무대에서 내려올 때. 지연 따라잡기 배속(1.03)도 이것으로 풀린다. */
  normalRate() {
    const v = this.video;
    if (this.rate !== 1 || (v && v.playbackRate !== 1)) this.bridge.call('setRate', 1);
    this.rate = 1;
  }

  toggleFullscreen() {
    toggleFullscreenOn(this.el);
  }

  renderGear() {
    const focusSel = this.focusedItemSelector();
    const items = [{ index: -1, label: '자동' }, ...this.levels];
    const v = this.video;
    // 직접 고른 화질은 고른 높이로(바꾼 직후 영상은 아직 앞 화질이다)
    const chosen = this.manualIndex >= 0 ? this.levels.find((l) => l.index === this.manualIndex) : null;
    const height = (chosen && chosen.height) || (v && v.videoHeight);
    const lat = Number.isFinite(this.latency) && this.latency > 0 ? `${this.latency.toFixed(1)}초` : '—';
    this.gearMenu.innerHTML = `
      <div class="ml-menu-head">화질${height ? ` <span>${height}p</span>` : ''}</div>
      ${items.map((o) => `<button class="ml-menu-item ml-level${o.index === this.manualIndex ? ' active' : ''}" role="menuitemradio" data-mp="level" data-level="${o.index}"><span>${esc(o.label)}</span></button>`).join('')}
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head">지연 <span>${lat}</span></div>
      <button class="ml-menu-item" data-mp="live"><span>라이브로 이동 (따라잡기)</span></button>
      ${this.stage ? this.playMenuHtml() : ''}
      ${this.sharpnessMenuHtml()}
      ${this.pictureMenuHtml()}
      ${this.stage ? this.stageToolsHtml() : ''}
      ${this.stage && EQ_SUPPORTED ? this.eqMenuHtml() : ''}`;
    this.gearMenu.classList.toggle('ml-stage-tools', this.stage);
    // 열린 채 다시 그리면(화질 목록·설정이 늦게 옴) 키보드 초점이 body 로 떨어졌다 — 같은 항목으로 되돌린다.
    if (focusSel) this.gearMenu.querySelector(focusSel)?.focus({ preventScroll: true });
  }

  focusedItemSelector() {
    const a = document.activeElement;
    if (!a || !this.gearMenu.contains(a)) return null;
    const parts = FOCUS_KEYS.filter((k) => a.hasAttribute(k)).map((k) => `[${k}="${CSS.escape(a.getAttribute(k))}"]`);
    return parts.length ? parts.join('') : null;
  }

  // ── 단독 보기 무대 — 배속·화면 비율·자동 보정/업스케일·이퀄라이저(예전 시청 화면 고급 패널과 같은 설정) ──

  /** multilive.render 가 매번 부른다 — 무대에 오르면 저장된 도구를 걸고, 내려오면 되돌린다. */
  setStage(on) {
    const stage = Boolean(on);
    if (this.stage === stage) return;
    this.stage = stage;
    // 무대에 오르고 내릴 때 패인 크기·자리가 크게 바뀐다 — 열린 메뉴는 닫는다(항목도 달라진다).
    if (this.menuOpen) this.closeMenus();
    this.applySharpness(); // 무대는 확대를 허용하고, 그리드는 윤곽만(선명도 자체는 끄지 않는다)
    if (!stage) {
      if (this.rate !== 1) this.normalRate(); // 지연 따라잡기(1.03)는 그리드에서도 그대로 둔다
      clearPlayerTools(this.bridge, { eq: this.eqApplied });
      this.eqApplied = false;
      return;
    }
    this.applyTools(); // 앞서 읽은 값이 있으면 바로
    this.refreshSettings();
  }

  /** 전역 플레이어 설정을 다시 읽어 건다(무대에 오를 때·⚙ 를 열 때). */
  async refreshSettings() {
    const seq = this.settingsSeq;
    const [p] = await Promise.all([loadPlayerSettings(), EQ_SUPPORTED ? this.loadEq() : null]);
    // 읽는 사이 무대에서 내려왔거나 메뉴에서 값을 바꿨으면 그 값을 덮지 않는다.
    if (!this.stage || seq !== this.settingsSeq) return;
    const changed = JSON.stringify(p) !== JSON.stringify(this.playerSettings);
    this.playerSettings = p;
    if (!changed) return;
    this.applyTools();
    if (!this.gearMenu.hidden) this.renderGear();
  }

  async loadEq() {
    this.eq = await loadEqPresets();
  }

  /** 저장된 도구를 이 패인 플레이어에 건다(무대일 때만). 플레이어 문서가 아직이면 ready 에서 다시 온다. */
  applyTools() {
    if (!this.stage || !this.playerSettings) return;
    const p = this.playerSettings;
    const eqOn = applyPlayerTools(this.bridge, p);
    if (!eqOn && this.eqApplied) this.bridge.call('setEqualizerEnabled', false);
    this.eqApplied = eqOn && this.bridge.ready;
  }

  /** 메뉴에서 바꾼 값 — 바로 걸고 저장한다(저장 응답을 기다리면 스위치가 늦게 바뀐다). */
  async savePatch(patch) {
    this.settingsSeq += 1;
    this.playerSettings = { ...(this.playerSettings || {}), ...patch };
    this.applyTools();
    if (!this.gearMenu.hidden) this.renderGear();
    const seq = this.settingsSeq;
    try {
      const saved = await savePlayerSettings(patch);
      if (seq === this.settingsSeq && this.stage) this.playerSettings = saved;
    } catch (e) {
      toast(e.message);
    }
  }

  setRate(rate) {
    if (!RATES.includes(rate)) return;
    this.rate = rate;
    this.bridge.call('setRate', rate);
    this.hooks.flash?.(`${rate}배속`);
  }

  toggleTool(key) {
    const p = this.playerSettings || {};
    if (key === 'aspectRatio') this.savePatch({ aspectRatio: p.aspectRatio === 'fill' ? null : 'fill' });
    else if (STAGE_TOOLS.some(([k]) => k === key)) this.savePatch({ [key]: !p[key] });
  }

  toggleEq() {
    if (!EQ_SUPPORTED) return;
    const p = this.playerSettings || {};
    this.savePatch(eqWanted(p) ? { equalizerPreset: null } : eqOnPatch(p, this.eq.presets));
  }

  playMenuHtml() {
    const p = this.playerSettings || {};
    const fill = p.aspectRatio === 'fill';
    return `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head" title="${esc(RATE_NOTE)}">재생 속도 <span>${this.rate}x</span></div>
      <div class="ml-rate-row" role="group" aria-label="재생 속도">${RATES.map((r) => `<button class="ml-menu-item ml-rate${r === this.rate ? ' active' : ''}" role="menuitemradio" aria-checked="${r === this.rate}" data-mp="rate" data-rate="${r}" aria-label="${r}배속">${r}x</button>`).join('')}</div>
      <p class="ml-menu-note">LIVE 를 누르면 1배속으로 돌아갑니다</p>
      <button class="ml-menu-item ml-check" role="menuitemcheckbox" aria-checked="${fill}" data-mp="tool" data-key="aspectRatio" data-keep-open title="${esc(GLOBAL_NOTE)}"><span>화면 꽉 채우기(비율 무시)</span><span class="ml-switch${fill ? ' on' : ''}"></span></button>`;
  }

  stageToolsHtml() {
    const p = this.playerSettings || {};
    return `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head" title="${esc(GLOBAL_NOTE)}">화면 도구 <span>플레이어 설정</span></div>${STAGE_TOOLS.map(([key, label, help]) => `
      <button class="ml-menu-item ml-check" role="menuitemcheckbox" aria-checked="${Boolean(p[key])}" data-mp="tool" data-key="${key}" data-keep-open title="${esc(`${help} · ${GLOBAL_NOTE}`)}"><span>${label}</span><span class="ml-switch${p[key] ? ' on' : ''}"></span></button>`).join('')}`;
  }

  /** 10밴드 이퀄라이저 — Windows(WebView2)에서만. 꺼져 있으면 스위치만(꺼진 슬라이더는 초점을 받지 못해 화살표 이동이 막힌다). */
  eqMenuHtml() {
    const p = this.playerSettings || {};
    const on = eqWanted(p);
    const head = `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head">이퀄라이저</div>
      <button class="ml-menu-item ml-check" role="menuitemcheckbox" aria-checked="${on}" data-mp="eq" data-keep-open title="${esc(GLOBAL_NOTE)}"><span>이퀄라이저</span><span class="ml-switch${on ? ' on' : ''}"></span></button>`;
    if (!on) return head;
    const { freqs, presets } = this.eq;
    const bands = eqBands(p, presets);
    const pre = Number(p.equalizerPreAmp) || 0;
    const known = presets.some((pr) => pr.name === p.equalizerPreset);
    return `${head}
      <label class="ml-adjust-row ml-eq-row" data-keep-open><span>프리셋</span>
        <select data-eq-preset aria-label="이퀄라이저 프리셋">
          ${presets.map((pr) => `<option value="${esc(pr.name)}"${pr.name === p.equalizerPreset ? ' selected' : ''}>${esc(pr.name)}</option>`).join('')}
          ${known ? '' : `<option selected>${esc(p.equalizerPreset)}</option>`}
        </select></label>
      <label class="ml-adjust-row ml-eq-row" data-keep-open><span>프리앰프</span>
        <input type="range" data-eq-preamp min="-12" max="12" step="0.5" value="${pre}" aria-label="프리앰프">
        <output>${pre}dB</output></label>
      <div class="ml-eq-bands" data-keep-open>${bands.map((g, i) => `
        <label class="ml-eq-band"><output>${g > 0 ? '+' : ''}${g}</output><input type="range" min="-12" max="12" step="0.5" value="${g}" data-eq-band="${i}" aria-label="${freqLabel(freqs[i])}Hz"><span>${freqLabel(freqs[i])}</span></label>`).join('')}</div>`;
  }

  /** 이퀄라이저 슬라이더를 끄는 동안 — 바로 듣게 걸기만 하고 저장은 놓을 때(change). */
  onEqInput(t) {
    const out = t.parentElement.querySelector('output');
    const v = Number(t.value);
    if (t.dataset.eqBand !== undefined) {
      if (out) out.textContent = `${v > 0 ? '+' : ''}${v}`;
      this.bridge.call('setEqualizerBand', Number(t.dataset.eqBand), v);
    } else if (t.dataset.eqPreamp !== undefined) {
      if (out) out.textContent = `${v}dB`;
      this.bridge.call('setEqualizerPreAmp', v);
    }
  }

  onMenuChange(e) {
    const t = e.target;
    if (!this.stage || !EQ_SUPPORTED || !t.closest) return;
    if (t.dataset.eqBand !== undefined) {
      const bands = [...this.gearMenu.querySelectorAll('[data-eq-band]')].map((x) => Number(x.value));
      this.savePatch({ equalizerBands: bands, equalizerPreset: '사용자 지정' });
    } else if (t.dataset.eqPreamp !== undefined) {
      this.savePatch({ equalizerPreAmp: Number(t.value) });
    } else if (t.dataset.eqPreset !== undefined) {
      const pr = this.eq.presets.find((x) => x.name === t.value);
      if (pr) this.savePatch({ equalizerPreset: pr.name, equalizerBands: pr.gains, equalizerPreAmp: pr.preAmp });
    }
  }

  // ── 선명도(전역, sharpness.js) — 예전 '선명한 화면'·'업스케일'을 합친 하나 ──

  /** 이 패인 플레이어에 선명도를 건다 — 무대 패인만 확대 허용. 준비된 문서에 같은 값을 이미 보냈으면 건너뛴다. */
  applySharpness(force = false) {
    const args = sharpnessArgs(this.stage);
    const key = JSON.stringify(args);
    if (!force && key === this.sharpSent) return;
    if (!this.bridge.ready) { this.sharpSent = null; return; } // 문서가 준비되면 ready 에서 다시 온다
    this.bridge.call('setSharpness', ...args);
    this.sharpSent = key;
  }

  chooseSharpness(level) {
    if (!Number.isInteger(level) || level < 0 || level > 3) return;
    const s = sharpnessState();
    if (s.level === level) return;
    setSharpness(level, s.enlargedOnly);
    const name = SHARPNESS_LEVELS[level].label;
    toast(`선명도 '${name}' — 모든 라이브 화면에 적용했어요`, 1500);
  }

  toggleEnlargedOnly() {
    const s = sharpnessState();
    if (!s.level) return; // 끔이면 쓸 일이 없다(aria-disabled)
    setSharpness(s.level, !s.enlargedOnly);
  }

  sharpnessMenuHtml() {
    const s = sharpnessState();
    const off = !s.level;
    return `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head" title="${esc(SHARPNESS_HELP)}">선명도 <span>모든 라이브 화면</span></div>
      <div class="ml-rate-row ml-sharp-row" role="group" aria-label="선명도">${SHARPNESS_LEVELS.map((l) => `<button class="ml-menu-item ml-sharp${l.value === s.level ? ' active' : ''}" role="menuitemradio" aria-checked="${l.value === s.level}" data-mp="sharpness" data-sharpness="${l.value}" data-keep-open aria-label="선명도 ${l.label}" title="${esc(l.help)}">${l.label}</button>`).join('')}</div>
      <button class="ml-menu-item ml-check${off ? ' is-disabled' : ''}" role="menuitemcheckbox" aria-checked="${s.enlargedOnly}" aria-disabled="${off}" data-mp="sharpness-enlarged" data-keep-open title="${esc(ENLARGED_ONLY_HELP)}"><span>${ENLARGED_ONLY_LABEL}</span><span class="ml-switch${s.enlargedOnly ? ' on' : ''}"></span></button>
      <p class="ml-menu-note ml-sharpness-note" aria-live="polite">${esc(sharpnessNote(this.sharpMsg))}</p>`;
  }

  /** 상태 줄만 고친다(메뉴를 다시 그리지 않아 초점이 그대로다). */
  syncSharpnessNote() {
    const note = this.gearMenu.querySelector('.ml-sharpness-note');
    const text = sharpnessNote(this.sharpMsg);
    if (note && note.textContent !== text) note.textContent = text;
    this.syncGearTitle();
  }

  /** ⚙ 단추 title — 열지 않아도 지금 선명도가 무엇을 하는지 보인다. */
  syncGearTitle() {
    const btn = this.el.querySelector('.ml-pane-bar [data-mp="gear"]');
    if (!btn) return;
    const title = `화질·지연·선명도 (선명도: ${sharpnessShort(this.sharpMsg)})`;
    if (btn.title !== title) btn.title = title;
  }

  // ── 색 보정(밝기·대비·채도) — 채널마다 기억(picture.js) ──

  pictureMenuHtml() {
    if (!this.channelId) return '';
    const p = pictureFor(this.channelId);
    const sliders = ADJUST_RANGES.map(([key, label, min, max, step]) => `
      <label class="ml-adjust-row" data-keep-open><span>${label}</span>
        <input type="range" data-adj="${key}" min="${min}" max="${max}" step="${step}" value="${p[key]}" aria-label="${label}">
        <output>${Math.round(p[key] * 100)}%</output></label>`).join('');
    return `
      <div class="ml-menu-sep"></div>
      <div class="ml-menu-head">색 보정 <span>이 채널</span></div>
      ${sliders}
      <button class="ml-menu-item" data-mp="adjust-reset" data-keep-open><span>보정 기본값</span></button>`;
  }

  /** 이 패인에 지금 채널을 알린다 — 바뀌면 그 채널의 색 보정을 건다. */
  setChannel(channelId) {
    if (this.channelId === channelId) return;
    this.channelId = channelId || null;
    this.applyPicture();
  }

  applyPicture() {
    if (this.channelId) applyPicture(this.bridge, pictureFor(this.channelId));
  }

  onAdjustInput(e) {
    if (this.stage && e.target.matches?.('[data-eq-band], [data-eq-preamp]')) { this.onEqInput(e.target); return; }
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

  /**
   * 패인(영상 위 포함) 휠 — 이미 소리 나는 패인의 볼륨만 ±5.
   * 들리지 않는 패인 위에서는 아무것도 하지 않는다 — 트랙패드로 스치기만 해도 setGain → makeAudible → 선택으로
   * 소리·채팅·포커스 메인이 한꺼번에 바뀌었다. 선택은 클릭으로만 한다.
   */
  onWheel(e) {
    if (e.target.closest && e.target.closest('.ml-pane-menu')) return;
    if (!this.audible) return;
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
