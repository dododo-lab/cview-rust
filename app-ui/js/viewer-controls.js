// 시청 화면 컨트롤 — 원본 LiveStreamView 하단 바·OverlayVolumeSlider·StreamLoadingOverlay.
//
// PLY-01 화질 선택, PLY-02 재생/일시정지·라이브 복귀·지연 배지, PLY-09 스크린샷, PLY-11 전체 화면 2종,
// PLY-13 오류 오버레이 재시도, PLY-15 휠 볼륨·% 표시, PLY-17 북마크, 시청 화면 단축키.
import { screenshotFormat, screenshotExtension } from './screenshot-format.js';
import { api } from './api.js';
import { esc, toast, keyBelongsToTarget, hideWithExit, plainLetter } from './dom.js';
import { announcePolite } from './a11y.js';
import { matchPlainAction } from './shortcuts.js';
import { kbd } from './platform.js';
import { releaseForPip } from './background-saver.js';

const ICON = {
  fullscreenExit: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/></svg>',
  play: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5-13-7.5Z"/></svg>',
  pause: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
  camera: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>',
  bookmark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  chatFull: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M15 20h5v-5"/><path d="M14 9h4M14 12h4"/></svg>',
};

/**
 * 화질 목록 → 메뉴 항목(높이 내림차순, 같은 높이는 하나). 순수 함수.
 * 같은 높이면 HEVC → 높은 비트레이트 → 먼저 나온 것. 치지직 1080p 는 같은 대역폭의 HEVC·H.264 가 함께 오는데, 플레이어는
 * 이 웹뷰가 HEVC 를 하드웨어로 풀 수 있을 때만 HEVC 레벨을 남기므로(player.html `applyHevcPolicyToLevels`) 남아 있으면 그쪽이 맞다.
 */
export function qualityOptions(levels) {
  const best = new Map();
  const hevc = (l) => l.codec === 'HEVC';
  for (const l of levels || []) {
    if (!l || !l.height) continue;
    const prev = best.get(l.height);
    const better = !prev || (hevc(l) !== hevc(prev) ? hevc(l) : (l.bitrate || 0) > (prev.bitrate || 0));
    if (better) best.set(l.height, l);
  }
  return [...best.values()]
    .sort((a, b) => b.height - a.height)
    .map((l) => ({ index: l.index, label: `${l.height}p`, height: l.height, codec: l.codec || '' }));
}

/** 화질 항목의 코덱 표시 — 기본(H.264)이 아닌 HEVC 만 단다. */
export function codecTagHtml(codec) {
  return codec === 'HEVC' ? '<span class="codec-tag" title="H.265(HEVC)">HEVC</span>' : '';
}

/** 화질 배지 설명 — '화질 1080p HEVC (자동) · 지연 3.5초'. 코덱을 모르면 빼고, 높이를 모르면 '확인 중'. */
export function qualityBadgeLabel({ height = 0, codec = '', auto = true, latency = 0 } = {}) {
  const res = height ? `${height}p${codec ? ` ${codec}` : ''}` : '확인 중';
  return `화질 ${res}${auto ? ' (자동)' : ''} · 지연 ${latency ? `${latency.toFixed(1)}초` : '확인 중'}`;
}

/**
 * 전체 화면 켜기·끄기·바꾸기 — 같은 대상이면 끄고, 다른 대상(영상만 ↔ 영상+채팅)이 전체 화면이면 나왔다가 이 대상으로
 * 다시 들어간다. 예전엔 무엇이든 전체 화면이면 끄기만 해 '영상+채팅'으로 바로 넘어가지 못했다.
 * (exitFullscreen 은 전체 화면이 풀린 뒤 끝난다 — 그때 누른 지 1초 안팎이라 사용자 활성화가 남아 있다.)
 */
export function toggleFullscreenOn(target) {
  const cur = document.fullscreenElement;
  if (!target || !target.requestFullscreen) { if (cur) document.exitFullscreen().catch(() => {}); return; }
  if (!cur) { target.requestFullscreen().catch(() => {}); return; }
  if (cur === target) { document.exitFullscreen().catch(() => {}); return; }
  document.exitFullscreen()
    .then(() => { if (!document.fullscreenElement && target.isConnected) return target.requestFullscreen(); return null; })
    .catch(() => {});
}

/** 지연(초) → 라이브 배지 상태. 4초 넘게 뒤처지면 "뒤처짐". */
/**
 * 볼륨 슬라이더 펼침 — 시청 화면·멀티라이브 패인 공용. 예전엔 CSS :hover 만으로 펼쳐(너비 0 → 0.2초에 걸쳐 펼침)
 * 아이콘에서 오른쪽으로 빨리 옮기거나 끌다가 조금만 벗어나도 다시 접혀 잡을 수 없었다(창이 키 창이 아니면
 * :focus-within 도 듣지 않는다). 들어오면 열고(.open), 나가면 잠시 기다렸다 닫고, 끄는 동안은 닫지 않는다.
 */
export function installVolumeReveal(group, range, { signal } = {}) {
  let timer = 0;
  let dragging = false;
  const open = () => { clearTimeout(timer); group.classList.add('open'); };
  const close = (delay = 600) => {
    clearTimeout(timer);
    timer = setTimeout(() => { if (!dragging && !group.matches(':hover')) group.classList.remove('open'); }, delay);
  };
  group.addEventListener('pointerenter', open);
  group.addEventListener('pointerleave', () => close());
  range.addEventListener('pointerdown', () => { dragging = true; open(); });
  const end = () => { if (dragging) { dragging = false; close(); } };
  window.addEventListener('pointerup', end, { signal });
  window.addEventListener('pointercancel', end, { signal });
  range.addEventListener('focus', open);
  range.addEventListener('blur', () => close());
  /** 휠·키보드로 바꿀 때 잠깐 펼쳐 값을 보여 준다. */
  const peek = () => { open(); close(1200); };
  return { open, close, peek, isOpen: () => group.classList.contains('open') };
}

export function liveBadgeState(latency) {
  if (!Number.isFinite(latency) || latency <= 0) return { behind: false, text: 'LIVE' };
  return { behind: latency > 8, text: latency > 8 ? `-${Math.round(latency)}초` : 'LIVE' };
}

/** 파일 이름용 시각 — 20261024_153012. */
export function stamp() {
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
    this.codec = '';

    // 왼쪽: 재생/일시정지 · 라이브
    const mute = root.querySelector('#mute');
    mute.insertAdjacentHTML('beforebegin', `
      <button class="icon-btn" id="ctl-play" title="일시정지 (Space)" aria-label="일시정지">${ICON.pause}</button>
      <button class="live-chip at-edge" id="ctl-live" title="라이브로 이동">LIVE</button>`);
    // 오른쪽: 화질 · 스크린샷 · 북마크 · 전체 화면(영상+채팅)
    const reload = root.querySelector('#reload');
    reload.insertAdjacentHTML('beforebegin', `
      <div class="quality-wrap">
        <button class="quality-btn" id="ctl-quality" title="화질" aria-haspopup="menu" aria-expanded="false" aria-controls="ctl-quality-menu">자동</button>
        <div class="quality-menu" id="ctl-quality-menu" role="menu" aria-label="화질" hidden></div>
      </div>
      <button class="icon-btn" id="ctl-shot" title="${kbd('스크린샷 (⌘S)')}" aria-label="스크린샷">${ICON.camera}</button>
      <button class="icon-btn" id="ctl-bookmark" title="북마크" aria-label="북마크">${ICON.bookmark}</button>`);
    root.querySelector('#fullscreen').insertAdjacentHTML('afterend',
      `<button class="icon-btn" id="ctl-fullscreen-chat" title="${kbd('영상+채팅 전체 화면 (⌃⌘F)')}" aria-label="영상과 채팅 전체 화면">${ICON.chatFull}</button>`);
    root.querySelector('#fullscreen').title = kbd('영상 전체 화면 (⇧⌘F)');
    // 전체 화면이면 단추 그림·이름을 '끝내기'로 — 예전엔 들어가도 같은 그림이라 상태를 알 수 없었다(감사 UX-M07)
    const fsButton = root.querySelector('#fullscreen');
    const fsEnter = fsButton.innerHTML;
    // 영상+채팅 전체 화면 단추도 같다 — 예전엔 영상 전체 화면 단추만 바뀌어, ⌃⌘F 로 들어간 뒤 이 단추는 '들어가기'로 남았다.
    const fsChat = root.querySelector('#ctl-fullscreen-chat');
    document.addEventListener('fullscreenchange', () => {
      const on = document.fullscreenElement === this.stage;
      fsButton.innerHTML = on ? ICON.fullscreenExit : fsEnter;
      fsButton.title = kbd(on ? '전체 화면 끝내기 (⇧⌘F · Esc)' : '영상 전체 화면 (⇧⌘F)');
      fsButton.setAttribute('aria-label', on ? '전체 화면 끝내기' : '영상 전체 화면');
      const chatOn = document.fullscreenElement === this.root;
      fsChat.innerHTML = chatOn ? ICON.fullscreenExit : ICON.chatFull;
      fsChat.title = kbd(chatOn ? '전체 화면 끝내기 (⌃⌘F · Esc)' : '영상+채팅 전체 화면 (⌃⌘F)');
      fsChat.setAttribute('aria-label', chatOn ? '전체 화면 끝내기' : '영상과 채팅 전체 화면');
    });

    this.playButton = root.querySelector('#ctl-play');
    this.liveButton = root.querySelector('#ctl-live');
    this.qualityButton = root.querySelector('#ctl-quality');
    this.qualityMenu = root.querySelector('.quality-menu');
    this.osd = document.createElement('div');
    this.osd.className = 'player-osd';
    // 보이는 OSD 는 꾸밈 — 낭독은 늘 있는 자리(announcePolite)로 한다(숨었다 나타나는 live 영역은 읽히지 않았다).
    this.osd.setAttribute('aria-hidden', 'true');
    this.osd.hidden = true;
    this.stage.append(this.osd);

    // 오류·종료 오버레이의 다시 시도
    const overlay = root.querySelector('.player-overlay');
    overlay.insertAdjacentHTML('beforeend', '<button class="btn primary overlay-retry" hidden>다시 시도</button>');
    this.retry = overlay.querySelector('.overlay-retry');
    this.retry.addEventListener('click', () => viewer.channelId && viewer.open(viewer.channelId, viewer.lastSettings));
    // 오버레이 표시·문구가 바뀔 때만 버튼을 맞춘다(버튼 자체는 감시하지 않는다 — 자기 변경으로 다시 깨지 않게).
    const label = overlay.querySelector('span');
    const sync = () => {
      const ended = overlay.classList.contains('ended') || /종료/.test(label.textContent);
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
    document.addEventListener('click', () => this.closeQualityMenu());
    // 메뉴 키보드 — ↑↓ 로 항목 이동, Esc 로 닫고 연 버튼으로 초점을 돌려준다(L-7).
    this.qualityMenu.addEventListener('keydown', (e) => {
      const items = [...this.qualityMenu.querySelectorAll('[role="menuitemradio"]')];
      const i = items.indexOf(document.activeElement);
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        this.closeQualityMenu(true);
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        e.stopPropagation();
        if (!items.length) return;
        const next = e.key === 'ArrowDown' ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
        items[next].focus();
      } else if (e.key === 'Home' || e.key === 'End') {
        e.preventDefault();
        e.stopPropagation();
        items[e.key === 'Home' ? 0 : items.length - 1]?.focus();
      } else if (e.key === 'Tab') {
        this.closeQualityMenu();
      }
    });
    root.querySelector('#ctl-shot').addEventListener('click', () => this.screenshot());
    root.querySelector('#ctl-bookmark').addEventListener('click', () => this.bookmark());
    root.querySelector('#ctl-fullscreen-chat').addEventListener('click', () => this.fullscreen(true));
    this.stage.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    // 영상 자리만 — 컨트롤·패널·메뉴를 빠르게 두 번 눌러도 전체 화면이 되지 않게(멀티라이브 패인과 같게).
    this.stage.addEventListener('dblclick', (e) => {
      if (e.target.closest('.player-controls, .chzzk-bar, .player-panel, .stage-top, .quality-menu, .vt-menu, .overlay-retry, button')) return;
      this.fullscreen(false);
    });

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
    this.volumeReveal = installVolumeReveal(vol, volume);
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
    // 버퍼링 — 영상 가운데 로고 로더(motion.css). 재생 중 데이터가 모자라면 보이고 다시 재생되면 숨는다.
    const buffer = document.createElement('div');
    buffer.className = 'spinner stage-buffer';
    buffer.setAttribute('aria-hidden', 'true');
    stage.append(buffer);

    // 위쪽 오버레이(채널·제목·바로 전환·창 동작)도 아래 막대와 함께 나타나고 숨는다.
    const top = stage.querySelector('.stage-top');
    const menuOpen = () => !this.qualityMenu.hidden || Boolean(this.root.querySelector('.player-panel:not([hidden]), .vt-menu:not([hidden])'));
    const hide = () => {
      // 시청 화면이 숨어 있으면(시작 직후·다른 메뉴) 막대를 숨기고 멈춘다 — 예전엔 정지 상태(영상 없음 포함)면
      // 2.5초마다 다시 깨워 앱이 끝날 때까지 돌았다. 다시 보이면 마우스 움직임·초점·정지 표시(renderStatus)가 깨운다.
      if (stage.closest('[hidden]')) { stage.classList.remove('ctl-visible'); stage.classList.add('idle'); return; }
      // 키보드 초점이 막대 안에 있으면 숨기지 않는다(마우스 클릭으로 생긴 초점은 :focus-visible 이 아니라 숨는다).
      const keyFocus = stage.querySelector(':is(button, input, select):focus-visible');
      if (stage.classList.contains('paused') || menuOpen() || bar.matches(':hover') || top?.matches(':hover') || keyFocus || this.volumeReveal.isOpen()) { this.wake(); return; }
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
    // 영상 위 투명 클릭 차단층(H-4) — 멀티라이브 .ml-pane-hit 처럼. 영상을 누르면 초점이 iframe 으로 넘어가
    // 부모 문서의 단축키(Space·M·방향키·F·C·P·L)와 Esc 가 모두 죽고, 무대의 dblclick(전체 화면)도 받지 못했다.
    // iframe 바로 뒤에 두고 z-index 를 주지 않아 오류 오버레이(다시 시도)·막대·가운데 재생 버튼보다는 아래에 깔린다.
    const hit = document.createElement('div');
    hit.className = 'player-hit';
    hit.setAttribute('aria-hidden', 'true');
    iframe.after(hit);
    // 누르면 남아 있던 버튼 초점도 내려놓는다 — Space 가 방금 누른 버튼을 다시 누르지 않고 재생/일시정지로 가게.
    hit.addEventListener('mousedown', () => {
      const a = document.activeElement;
      if (a && a !== document.body && stage.contains(a)) a.blur();
    });
    const hook = () => {
      try {
        const doc = iframe.contentDocument;
        // 치지직처럼 무대는 테마와 상관없이 검정 — 영상 위아래·좌우 여백도 같은 색.
        iframe.contentWindow?.setStageBackgroundColor?.('#000');
        if (!doc || doc.__cviewWake) return;
        doc.__cviewWake = true;
        doc.addEventListener('mousemove', this.wake);
        doc.addEventListener('mousedown', this.wake);
        // 휠도 영상(iframe) 위에서 받는다 — 거품이 부모 문서로 오지 않아 영상 위에서는 볼륨이 바뀌지 않았다.
        doc.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
        // 그래도 초점이 iframe 에 들어가면(전체 화면 전환 등) 키와 두 번 누르기를 부모로 넘긴다(H-4).
        doc.addEventListener('keydown', (e) => {
          const t = e.target;
          if (t && t.closest && t.closest('input, textarea, select, [contenteditable]')) return;
          const clone = new KeyboardEvent('keydown', {
            key: e.key, code: e.code, repeat: e.repeat, bubbles: true, cancelable: true,
            metaKey: e.metaKey, ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey,
          });
          if (!document.dispatchEvent(clone)) e.preventDefault();
        });
        doc.addEventListener('dblclick', () => this.fullscreen(false));
        // 미디어 이벤트는 거품이 일지 않아 캡처로 받는다.
        const setBuffering = (on) => stage.classList.toggle('buffering', on);
        doc.addEventListener('waiting', () => setBuffering(true), true);
        for (const t of ['playing', 'canplay', 'pause', 'emptied', 'ended']) doc.addEventListener(t, () => setBuffering(false), true);
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

  /** 새로 열 때 — 앞 방송의 화질 목록·고른 화질·지연을 비운다(다음 playStarted·metrics 가 채운다). */
  resetStream() {
    this.levels = [];
    this.manualIndex = -1;
    this.currentHeight = 0;
    this.latency = 0;
  }

  get video() {
    const doc = this.viewer.player.iframe.contentDocument;
    return doc && doc.getElementById('video');
  }

  onMessage(type, data) {
    if (type === 'levels') {
      this.levels = qualityOptions(data.levels);
      if (typeof data.videoCodec === 'string') this.codec = data.videoCodec;
      // manual — 고른 화질(-1 자동). 예전 판 플레이어(current 만 보냄)는 current 로.
      this.manualIndex = typeof data.manual === 'number' ? data.manual : (typeof data.current === 'number' ? data.current : -1);
      this.renderQualityMenu();
    } else if (type === 'metrics') {
      if (typeof data.latency === 'number') this.latency = data.latency;
      if (data.currentLevelHeight) this.currentHeight = data.currentLevelHeight;
      if (typeof data.videoCodec === 'string') this.codec = data.videoCodec;
      this.renderStatus();
    } else if (type === 'playStarted' || type === 'playbackState') {
      // 재생이 (다시) 시작되면 화질 상태를 플레이어에서 다시 읽는다 — 다시 연결·채널 전환·정지 복구는 hls 를 새로 만들어
      // 고른 화질이 '자동'으로 풀리는데, 단추·배지는 메뉴를 열 때까지 옛 '720p' 로 남았다.
      if (type === 'playStarted') this.viewer.player.call('getLevels');
      this.renderStatus();
    } else if (type === 'screenshot') {
      this.saveScreenshot(data);
    }
  }

  renderStatus() {
    const v = this.video;
    const paused = !v || v.paused;
    this.stage.classList.toggle('paused', Boolean(v && v.paused && v.readyState > 0));
    // 버퍼링 표시는 미디어 이벤트가 켜고 끄지만, 이벤트를 놓쳐도 데이터가 충분하면 지운다.
    if (v && v.readyState >= 3) this.stage.classList.remove('buffering');
    if (this.stage.classList.contains('paused') && this.wake) this.wake();
    if (this.playButton.dataset.paused !== String(paused)) {
      this.playButton.dataset.paused = String(paused);
      this.playButton.innerHTML = paused ? ICON.play : ICON.pause;
      this.playButton.title = paused ? '재생 (Space)' : '일시정지 (Space)';
      // 스크린리더도 지금 누르면 무엇이 되는지 알게(M-7) — title 만 바뀌고 aria-label 은 "일시정지"로 고정이었다.
      this.playButton.setAttribute('aria-label', paused ? '재생' : '일시정지');
    }
    const badge = liveBadgeState(this.latency);
    this.liveButton.textContent = badge.text;
    this.liveButton.classList.toggle('behind', badge.behind || paused);
    // 라이브 끝이면 숨긴다 — "LIVE" 는 위쪽 방송 정보 칩이 이미 보여 준다. 늦어지면 "-N초"(누르면 따라잡기)로 나타난다.
    // 멈췄거나 배속을 바꿨으면 늘 보인다 — 안내("LIVE 를 누르면 1배속")가 가리키는 단추가 사라져 있었다(감사 UX-H13).
    // 배속은 사용자가 고른 값으로 본다 — 플레이어가 지연을 따라잡느라 잠깐 1.03배로 돌면(video.playbackRate) 예전엔
    // 'LIVE' 칩이 저절로 떴다 사라지기를 되풀이했다.
    this.liveButton.classList.toggle('at-edge', !badge.behind && !paused && (this.viewer.panel?.rate ?? 1) === 1);
    const auto = this.manualIndex < 0;
    // 직접 고른 화질은 고른 높이로 — 바꾼 직후엔 영상이 아직 앞 화질이라 720p 를 골라도 1080p 로 보였다.
    const chosen = auto ? null : this.levels.find((l) => l.index === this.manualIndex);
    const height = (chosen && chosen.height) || (v && v.videoHeight) || this.currentHeight;
    this.qualityButton.textContent = height ? `${auto ? '자동 · ' : ''}${height}p` : '자동';
    // 코덱은 지금 받는 레벨 것 — 직접 고른 직후엔 아직 앞 레벨이라 고른 항목의 코덱을 쓴다.
    const codec = (chosen && chosen.codec) || this.codec || '';
    this.viewer.topbar?.syncBadge({ height, latency: this.latency, auto, codec });
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

  /** 화질 메뉴 — opener 는 누른 단추(아래 막대 화질 단추 또는 위 막대 화질·지연 배지). 닫으면 그 단추로 초점을 돌려준다. */
  toggleQualityMenu(opener = null) {
    if (!this.qualityMenu.hidden) {
      this.closeQualityMenu(true);
      return;
    }
    this.qualityOpener = opener;
    this.viewer.topbar?.closeMenu();
    if (this.viewer.panel && !this.viewer.panel.el.hidden) hideWithExit(this.viewer.panel.el); // 플레이어 설정 패널과 겹치지 않게
    this.viewer.player.call('getLevels');
    this.renderQualityMenu();
    this.qualityMenu.hidden = false;
    (this.qualityOpener || this.qualityButton).setAttribute('aria-expanded', 'true');
    // 열면 고른 항목(없으면 첫 항목)으로 초점(L-7).
    const first = this.qualityMenu.querySelector('[aria-checked="true"]') || this.qualityMenu.querySelector('[role="menuitemradio"]');
    first?.focus();
  }

  /** 화질 메뉴 닫기 — refocus 면 연 버튼으로 초점을 돌려준다(Esc·다시 누르기). 메뉴 안에 초점이 있었어도 돌려준다. */
  closeQualityMenu(refocus = false) {
    if (this.qualityMenu.hidden) return;
    const inside = this.qualityMenu.contains(document.activeElement);
    hideWithExit(this.qualityMenu);
    this.qualityButton.setAttribute('aria-expanded', 'false');
    const opener = this.qualityOpener || this.qualityButton;
    opener.setAttribute('aria-expanded', 'false');
    this.qualityOpener = null;
    if (refocus || inside) opener.focus();
  }

  renderQualityMenu() {
    const items = [{ index: -1, label: '자동' }, ...this.levels];
    // 다시 그리면 초점이 사라지므로, 메뉴 안에 초점이 있었으면 같은 자리 항목으로 돌려놓는다.
    const focused = this.qualityMenu.contains(document.activeElement) ? document.activeElement.dataset.level : null;
    this.qualityMenu.innerHTML = items.map((o) => `
      <button class="quality-item${o.index === this.manualIndex ? ' active' : ''}" role="menuitemradio" aria-checked="${o.index === this.manualIndex}" tabindex="-1" data-level="${o.index}">
        <span class="q-name">${esc(o.label)}${codecTagHtml(o.codec)}</span>${o.height >= 1080 ? '<span class="hd">HD</span>' : ''}
      </button>`).join('') || '<div class="quality-empty">화질 정보를 불러오는 중…</div>';
    if (focused != null) this.qualityMenu.querySelector(`[data-level="${CSS.escape(focused)}"]`)?.focus();
  }

  chooseLevel(index) {
    this.closeQualityMenu();
    const run = (s) => this.viewer.player.run(s);
    if (index < 0) {
      // 자동 — 상한을 -1 로 지우지 않고 앱 화질 상한·백그라운드 절약 상한을 다시 적용한다.
      run('if (typeof setLockTopLevel === "function") setLockTopLevel(false); if (typeof clearUserManualLevel === "function") clearUserManualLevel(); if (hls) { hls.loadLevel = -1; if (typeof applyAdaptiveLevelCap === "function") applyAdaptiveLevelCap(); }');
      this.flash('화질 자동');
    } else {
      run(`if (typeof setLockTopLevel === "function") setLockTopLevel(false); if (typeof pickManualLevel === "function") pickManualLevel(${Number(index)}); else requestManualLevel(${Number(index)});`);
      const opt = this.levels.find((l) => l.index === index);
      this.flash(`화질 ${opt ? opt.label : ''}`);
    }
    this.manualIndex = index;
    setTimeout(() => this.viewer.player.call('getLevels'), 600);
    setTimeout(() => this.renderStatus(), 2500);
    this.renderStatus();
  }

  async screenshot() {
    if (!this.viewer.isPlaying) return;
    this.viewer.player.call('takeScreenshot', await screenshotFormat());
  }

  async saveScreenshot(data) {
    if (!data || !data.ok) {
      toast('스크린샷을 찍지 못했습니다 — 영상이 아직 준비되지 않았습니다');
      return;
    }
    const name = this.viewer.live?.channelName || 'CView';
    try {
      const path = await api.saveFile('screenshot', `${name}_${stamp()}.${screenshotExtension(data)}`, data.base64, null);
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
    toggleFullscreenOn(withChat ? this.root : this.stage);
  }

  onWheel(e) {
    if (!this.viewer.isPlaying || this.root.hidden) return;
    // 무대 위에 뜬 목록(⚙ 패널·⋯ 메뉴·화질 메뉴)은 스크롤한다 — 예전엔 목록이 움직이지 않고 볼륨이 바뀌어 음소거까지 풀렸다
    // (멀티라이브 칸 메뉴는 ml-pane-controls 가 같은 식으로 뺀다).
    if (e.target && e.target.closest && e.target.closest('.player-panel, .vt-menu, .quality-menu')) return;
    e.preventDefault();
    const range = this.root.querySelector('#volume');
    const step = e.deltaY > 0 ? -5 : 5;
    range.value = String(Math.min(100, Math.max(0, Number(range.value) + step)));
    range.dispatchEvent(new Event('input'));
    clearTimeout(this.wheelSave);
    this.wheelSave = setTimeout(() => range.dispatchEvent(new Event('change')), 400);
    this.volumeReveal?.peek();
    this.wake?.();
    this.flash(`볼륨 ${range.value}%`);
  }

  flash(text) {
    announcePolite(text);
    this.osd.textContent = text;
    this.osd.hidden = false;
    this.osd.classList.remove('fade');
    void this.osd.offsetWidth;
    this.osd.classList.add('fade');
    clearTimeout(this.osdTimer);
    this.osdTimer = setTimeout(() => { this.osd.hidden = true; }, 1300);
  }

  onKey(e) {
    if (this.root.hidden || !this.viewer.isPlaying || keyBelongsToTarget(e)) return;
    const mod = e.metaKey || e.ctrlKey;
    const letter = plainLetter(e);
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
    } else if (letter === 'm' && !mod) {
      this.viewer.toggleMute();
      this.flash(this.viewer.sessionMuted ? '음소거' : '음소거 해제');
    } else if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !mod) {
      e.preventDefault();
      range.value = String(Math.min(100, Math.max(0, Number(range.value) + (e.key === 'ArrowUp' ? 5 : -5))));
      range.dispatchEvent(new Event('input'));
      range.dispatchEvent(new Event('change'));
      this.volumeReveal?.peek();
      this.flash(`볼륨 ${range.value}%`);
    } else if (e.metaKey && e.shiftKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      this.fullscreen(false);
    } else if (e.metaKey && e.ctrlKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      this.fullscreen(true);
    } else if (letter === 'l' && !mod) {
      this.goLive();
    } else if (letter === 'b' && !mod && !e.altKey && !e.shiftKey) {
      // 단독 무대(ml-watch)와 같게 맨 글자 B 도 북마크(M-9 — 두 화면의 키를 맞춘다). 예약 키라 사용자 단축키와 겹치지 않는다.
      this.bookmark();
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
      if (next === 'picture-in-picture') releaseForPip(v);
      v.webkitSetPresentationMode(next);
      return next === 'inline' ? 'PiP 끔' : 'PiP';
    }
    const doc = v.ownerDocument;
    if (doc.pictureInPictureEnabled || document.pictureInPictureEnabled) {
      if (doc.pictureInPictureElement) await doc.exitPictureInPicture();
      else { releaseForPip(v); await v.requestPictureInPicture(); }
      return '';
    }
    toast('이 환경에서는 PiP 를 쓸 수 없습니다');
  } catch (er) { toast(`PiP 실패: ${er.message}`); }
  return '';
}
