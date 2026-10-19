// 시청 화면 컨트롤 — 원본 LiveStreamView 하단 바·OverlayVolumeSlider·StreamLoadingOverlay.
//
// PLY-01 화질 선택, PLY-02 재생/일시정지·라이브 복귀·지연 배지, PLY-09 스크린샷, PLY-11 전체화면 2종,
// PLY-13 오류 오버레이 재시도, PLY-15 휠 볼륨·% 표시, PLY-17 북마크, 시청 화면 단축키.
import { api } from './api.js';
import { esc, toast } from './dom.js';
import { matchPlainAction } from './shortcuts.js';

const ICON = {
  play: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5-13-7.5Z"/></svg>',
  pause: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
  camera: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>',
  bookmark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  chatFull: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M15 20h5v-5"/><path d="M14 9h4M14 12h4"/></svg>',
};

/** 화질 목록 → 메뉴 항목(높이 내림차순, 같은 높이는 높은 비트레이트 하나). 순수 함수. */
export function qualityOptions(levels) {
  const best = new Map();
  for (const l of levels || []) {
    if (!l || !l.height) continue;
    const prev = best.get(l.height);
    if (!prev || (l.bitrate || 0) > (prev.bitrate || 0)) best.set(l.height, l);
  }
  return [...best.values()]
    .sort((a, b) => b.height - a.height)
    .map((l) => ({ index: l.index, label: `${l.height}p`, height: l.height }));
}

/** 지연(초) → 라이브 배지 상태. 4초 넘게 뒤처지면 "뒤처짐". */
export function liveBadgeState(latency) {
  if (!Number.isFinite(latency) || latency <= 0) return { behind: false, text: 'LIVE' };
  return { behind: latency > 8, text: latency > 8 ? `-${Math.round(latency)}초` : 'LIVE' };
}

function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

export class ViewerControls {
  /** @param {import('./viewer.js').Viewer} viewer */
  constructor(viewer) {
    this.viewer = viewer;
    const root = viewer.root;
    this.root = root;
    this.stage = root.querySelector('.player-stage');
    this.bar = root.querySelector('.player-controls');
    this.levels = [];
    this.manualIndex = -1;
    this.currentHeight = 0;
    this.latency = 0;

    // 왼쪽: 재생/일시정지 · 라이브
    const mute = root.querySelector('#mute');
    mute.insertAdjacentHTML('beforebegin', `
      <button class="icon-btn" id="ctl-play" title="일시정지 (Space)" aria-label="일시정지">${ICON.pause}</button>
      <button class="live-chip" id="ctl-live" title="라이브로 이동">LIVE</button>`);
    // 오른쪽: 화질 · 스크린샷 · 북마크 · 전체화면(영상+채팅)
    const reload = root.querySelector('#reload');
    reload.insertAdjacentHTML('beforebegin', `
      <div class="quality-wrap">
        <button class="quality-btn" id="ctl-quality" title="화질" aria-haspopup="menu">자동</button>
        <div class="quality-menu" role="menu" hidden></div>
      </div>
      <button class="icon-btn" id="ctl-shot" title="스크린샷 (⌘S)" aria-label="스크린샷">${ICON.camera}</button>
      <button class="icon-btn" id="ctl-bookmark" title="북마크" aria-label="북마크">${ICON.bookmark}</button>`);
    root.querySelector('#fullscreen').insertAdjacentHTML('afterend',
      `<button class="icon-btn" id="ctl-fullscreen-chat" title="영상+채팅 전체화면 (⌃⌘F)" aria-label="영상과 채팅 전체화면">${ICON.chatFull}</button>`);
    root.querySelector('#fullscreen').title = '영상 전체화면 (⇧⌘F)';

    this.playButton = root.querySelector('#ctl-play');
    this.liveButton = root.querySelector('#ctl-live');
    this.qualityButton = root.querySelector('#ctl-quality');
    this.qualityMenu = root.querySelector('.quality-menu');
    this.osd = document.createElement('div');
    this.osd.className = 'player-osd';
    this.osd.hidden = true;
    this.stage.append(this.osd);

    // 오류·종료 오버레이의 다시 시도
    const overlay = root.querySelector('.player-overlay');
    overlay.insertAdjacentHTML('beforeend', '<button class="btn primary overlay-retry" hidden>다시 시도</button>');
    this.retry = overlay.querySelector('.overlay-retry');
    this.retry.addEventListener('click', () => viewer.channelId && viewer.open(viewer.channelId));
    // 오버레이 표시·문구가 바뀔 때만 버튼을 맞춘다(버튼 자체는 감시하지 않는다 — 자기 변경으로 다시 깨지 않게).
    const label = overlay.querySelector('span');
    const sync = () => {
      const ended = /종료/.test(label.textContent);
      const shown = !overlay.hidden && (overlay.classList.contains('error') || ended);
      if (this.retry.hidden !== !shown) this.retry.hidden = !shown;
      const text = ended ? '다시 확인' : '다시 시도';
      if (this.retry.textContent !== text) this.retry.textContent = text;
    };
    new MutationObserver(sync).observe(overlay, { attributes: true, attributeFilter: ['hidden', 'class'] });
    new MutationObserver(sync).observe(label, { childList: true, characterData: true, subtree: true });

    this.playButton.addEventListener('click', () => this.togglePlay());
    this.liveButton.addEventListener('click', () => this.goLive());
    this.qualityButton.addEventListener('click', (e) => { e.stopPropagation(); this.toggleQualityMenu(); });
    this.qualityMenu.addEventListener('click', (e) => {
      const item = e.target.closest('[data-level]');
      if (item) this.chooseLevel(Number(item.dataset.level));
    });
    document.addEventListener('click', () => { this.qualityMenu.hidden = true; });
    root.querySelector('#ctl-shot').addEventListener('click', () => this.screenshot());
    root.querySelector('#ctl-bookmark').addEventListener('click', () => this.bookmark());
    root.querySelector('#ctl-fullscreen-chat').addEventListener('click', () => this.fullscreen(true));
    this.stage.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    this.stage.addEventListener('dblclick', () => this.fullscreen(false));

    viewer.player.onMessage((type, data) => this.onMessage(type, data));
    document.addEventListener('keydown', (e) => this.onKey(e));
    this.installOverlay();
  }

  /**
   * 치지직 플레이어식 오버레이 — 컨트롤 막대를 영상 위 아래쪽 그라데이션으로 옮기고,
   * 마우스를 움직이면 나타났다가 재생 중 2.5초 가만히 있으면 커서와 함께 숨긴다.
   * 일시정지·메뉴가 열려 있으면 계속 보인다. 일시정지면 가운데 큰 재생 표시.
   */
  installOverlay() {
    const stage = this.stage;
    const bar = this.bar;
    bar.classList.add('chzzk-bar');
    // 볼륨: 아이콘에 마우스를 올리면 슬라이더가 펼쳐진다
    const mute = bar.querySelector('#mute');
    const volume = bar.querySelector('#volume');
    const vol = document.createElement('div');
    vol.className = 'vol-group';
    mute.before(vol);
    vol.append(mute, volume);
    const syncVolumeFill = () => volume.style.setProperty('--fill', `${volume.value}%`);
    volume.addEventListener('input', syncVolumeFill);
    new MutationObserver(syncVolumeFill).observe(volume, { attributes: true, attributeFilter: ['value'] });
    syncVolumeFill();
    stage.append(bar);
    this.center = document.createElement('button');
    this.center.className = 'stage-center-play';
    this.center.setAttribute('aria-label', '재생');
    this.center.innerHTML = ICON.play;
    this.center.addEventListener('click', () => this.togglePlay());
    stage.append(this.center);

    // 위쪽 오버레이(채널·제목·바로 전환·창 동작)도 아래 막대와 함께 나타나고 숨는다.
    const top = stage.querySelector('.stage-top');
    const menuOpen = () => !this.qualityMenu.hidden || Boolean(this.root.querySelector('.player-panel:not([hidden])'));
    const hide = () => {
      if (stage.classList.contains('paused') || menuOpen() || bar.matches(':hover') || top?.matches(':hover')) { this.wake(); return; }
      stage.classList.remove('ctl-visible');
      stage.classList.add('idle');
    };
    this.wake = () => {
      stage.classList.add('ctl-visible');
      stage.classList.remove('idle');
      clearTimeout(this.idleTimer);
      this.idleTimer = setTimeout(hide, 2500);
    };
    stage.addEventListener('mousemove', this.wake);
    stage.addEventListener('mousedown', this.wake);
    stage.addEventListener('mouseleave', () => { clearTimeout(this.idleTimer); this.idleTimer = setTimeout(hide, 400); });
    bar.addEventListener('focusin', this.wake);
    top?.addEventListener('focusin', this.wake);
    // 영상은 iframe 안이라 그 문서의 마우스 움직임도 받는다(같은 출처).
    const iframe = stage.querySelector('iframe');
    const hook = () => {
      try {
        const doc = iframe.contentDocument;
        // 치지직처럼 무대는 테마와 상관없이 검정 — 영상 위아래·좌우 여백도 같은 색.
        iframe.contentWindow?.setStageBackgroundColor?.('#000');
        if (!doc || doc.__cviewWake) return;
        doc.__cviewWake = true;
        doc.addEventListener('mousemove', this.wake);
        doc.addEventListener('mousedown', this.wake);
        const style = doc.createElement('style');
        style.textContent = 'html.cview-idle, html.cview-idle * { cursor: none !important; }';
        doc.head.append(style);
      } catch { /* 다른 출처면 무시 */ }
    };
    iframe.addEventListener('load', hook);
    hook();
    new MutationObserver(() => {
      try { iframe.contentDocument.documentElement.classList.toggle('cview-idle', stage.classList.contains('idle')); } catch { /* 무시 */ }
    }).observe(stage, { attributes: true, attributeFilter: ['class'] });
    this.wake();
  }

  get video() {
    const doc = this.viewer.player.iframe.contentDocument;
    return doc && doc.getElementById('video');
  }

  onMessage(type, data) {
    if (type === 'levels') {
      this.levels = qualityOptions(data.levels);
      this.manualIndex = typeof data.current === 'number' ? data.current : -1;
      this.renderQualityMenu();
    } else if (type === 'metrics') {
      if (typeof data.latency === 'number') this.latency = data.latency;
      if (data.currentLevelHeight) this.currentHeight = data.currentLevelHeight;
      this.renderStatus();
    } else if (type === 'playStarted' || type === 'playbackState') {
      this.renderStatus();
    } else if (type === 'screenshot') {
      this.saveScreenshot(data);
    }
  }

  renderStatus() {
    const v = this.video;
    const paused = !v || v.paused;
    this.stage.classList.toggle('paused', Boolean(v && v.paused && v.readyState > 0));
    if (this.stage.classList.contains('paused') && this.wake) this.wake();
    this.playButton.innerHTML = paused ? ICON.play : ICON.pause;
    this.playButton.title = paused ? '재생 (Space)' : '일시정지 (Space)';
    const badge = liveBadgeState(this.latency);
    this.liveButton.textContent = badge.text;
    this.liveButton.classList.toggle('behind', badge.behind || paused);
    const auto = this.manualIndex < 0;
    const height = (v && v.videoHeight) || this.currentHeight;
    this.qualityButton.textContent = height ? `${auto ? '자동 · ' : ''}${height}p` : '자동';
  }

  togglePlay() {
    const v = this.video;
    if (!v) return;
    this.viewer.player.call(v.paused ? 'play' : 'pause');
    setTimeout(() => this.renderStatus(), 150);
  }

  goLive() {
    const v = this.video;
    if (v && v.paused) this.viewer.player.call('play');
    if (v && v.playbackRate !== 1) { this.viewer.player.call('setRate', 1); if (this.viewer.panel) this.viewer.panel.rate = 1; }
    this.viewer.player.call('seekToLiveEdge');
    this.flash('라이브로 이동');
  }

  toggleQualityMenu() {
    if (!this.qualityMenu.hidden) {
      this.qualityMenu.hidden = true;
      return;
    }
    this.viewer.player.call('getLevels');
    this.renderQualityMenu();
    this.qualityMenu.hidden = false;
  }

  renderQualityMenu() {
    const items = [{ index: -1, label: '자동' }, ...this.levels];
    this.qualityMenu.innerHTML = items.map((o) => `
      <button class="quality-item${o.index === this.manualIndex ? ' active' : ''}" role="menuitemradio" data-level="${o.index}">
        ${esc(o.label)}${o.height >= 1080 ? '<span class="hd">HD</span>' : ''}
      </button>`).join('') || '<div class="quality-empty">화질 정보를 불러오는 중…</div>';
  }

  chooseLevel(index) {
    this.qualityMenu.hidden = true;
    const run = (s) => this.viewer.player.run(s);
    if (index < 0) {
      run('if (typeof setLockTopLevel === "function") setLockTopLevel(false); if (hls) { hls.currentLevel = -1; hls.loadLevel = -1; hls.autoLevelCapping = -1; }');
      this.flash('화질 자동');
    } else {
      run(`if (typeof setLockTopLevel === "function") setLockTopLevel(false); requestManualLevel(${Number(index)});`);
      const opt = this.levels.find((l) => l.index === index);
      this.flash(`화질 ${opt ? opt.label : ''}`);
    }
    this.manualIndex = index;
    setTimeout(() => this.viewer.player.call('getLevels'), 600);
    setTimeout(() => this.renderStatus(), 2500);
    this.renderStatus();
  }

  screenshot() {
    if (!this.viewer.isPlaying) return;
    this.viewer.player.call('takeScreenshot', 'png');
  }

  async saveScreenshot(data) {
    if (!data || !data.ok) {
      toast('스크린샷을 찍지 못했습니다 — 영상이 아직 준비되지 않았습니다');
      return;
    }
    const name = (this.root.querySelector('.viewer-title .meta')?.textContent || 'CView').split('·')[0].trim();
    try {
      const path = await api.saveFile('screenshot', `${name}_${stamp()}.png`, data.base64, null);
      this.flash('스크린샷 저장됨');
      toast(`스크린샷을 저장했습니다 — ${path}`, 3500);
    } catch (e) {
      toast(e.message);
    }
  }

  async bookmark() {
    const id = this.viewer.channelId;
    if (!id) return;
    try {
      await api.bookmarkAdd(id, null);
      this.flash('북마크 추가');
    } catch (e) {
      toast(e.message);
    }
  }

  fullscreen(withChat) {
    if (document.fullscreenElement) {
      document.exitFullscreen();
      return;
    }
    const target = withChat ? this.root : this.stage;
    if (target.requestFullscreen) target.requestFullscreen();
  }

  onWheel(e) {
    if (!this.viewer.isPlaying || this.root.hidden) return;
    e.preventDefault();
    const range = this.root.querySelector('#volume');
    const step = e.deltaY > 0 ? -5 : 5;
    range.value = String(Math.min(100, Math.max(0, Number(range.value) + step)));
    range.dispatchEvent(new Event('input'));
    clearTimeout(this.wheelSave);
    this.wheelSave = setTimeout(() => range.dispatchEvent(new Event('change')), 400);
    this.flash(`볼륨 ${range.value}%`);
  }

  flash(text) {
    this.osd.textContent = text;
    this.osd.hidden = false;
    this.osd.classList.remove('fade');
    void this.osd.offsetWidth;
    this.osd.classList.add('fade');
    clearTimeout(this.osdTimer);
    this.osdTimer = setTimeout(() => { this.osd.hidden = true; }, 1300);
  }

  onKey(e) {
    if (this.root.hidden || !this.viewer.isPlaying) return;
    const t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    const mod = e.metaKey || e.ctrlKey;
    const range = this.root.querySelector('#volume');
    // 사용자 단축키(맨 글자) — 수식 키 바인딩은 메뉴가 받는다 (SET-12).
    const act = matchPlainAction(e);
    if (act) {
      e.preventDefault();
      this.runAction(act);
      return;
    }
    if (e.key === ' ' && !mod) {
      e.preventDefault();
      this.togglePlay();
    } else if (e.key.toLowerCase() === 'm' && !mod) {
      this.viewer.toggleMute();
      this.flash(this.viewer.player.isMuted() ? '음소거' : '음소거 해제');
    } else if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !mod) {
      e.preventDefault();
      range.value = String(Math.min(100, Math.max(0, Number(range.value) + (e.key === 'ArrowUp' ? 5 : -5))));
      range.dispatchEvent(new Event('input'));
      range.dispatchEvent(new Event('change'));
      this.flash(`볼륨 ${range.value}%`);
    } else if (e.metaKey && e.shiftKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      this.fullscreen(false);
    } else if (e.metaKey && e.ctrlKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      this.fullscreen(true);
    } else if (e.key.toLowerCase() === 'l' && !mod) {
      this.goLive();
    }
  }

  runAction(action) {
    switch (action) {
      case 'toggleFullscreen': this.fullscreen(false); break;
      case 'toggleChat': this.viewer.extras?.cycleMode(); break;
      case 'togglePiP': this.togglePip(); break;
      case 'screenshot': this.screenshot(); break;
      case 'bookmarkStream': this.bookmark(); break;
      default: break;
    }
  }

  /** PiP — 단일 시청 화면의 영상. */
  async togglePip() {
    const label = await togglePipFor(this.video);
    if (label) this.flash(label);
  }
}

/** PiP 켜고 끄기 — WKWebView 는 webkitSetPresentationMode, 그 밖은 표준 API. 멀티라이브 패인도 쓴다. */
export async function togglePipFor(v) {
  if (!v) return '';
  try {
    if (typeof v.webkitSetPresentationMode === 'function') {
      const next = v.webkitPresentationMode === 'picture-in-picture' ? 'inline' : 'picture-in-picture';
      v.webkitSetPresentationMode(next);
      return next === 'inline' ? 'PiP 끔' : 'PiP';
    }
    const doc = v.ownerDocument;
    if (doc.pictureInPictureEnabled || document.pictureInPictureEnabled) {
      if (doc.pictureInPictureElement) await doc.exitPictureInPicture();
      else await v.requestPictureInPicture();
      return '';
    }
    toast('이 환경에서는 PiP 를 쓸 수 없습니다');
  } catch (er) { toast(`PiP 실패: ${er.message}`); }
  return '';
}
