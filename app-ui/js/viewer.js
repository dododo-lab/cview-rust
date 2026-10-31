// 라이브 시청 화면 — 플레이어 영역 + 채팅 패널.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast, icons, imageSrc, streamEndedArt } from './dom.js';
import { PlayerBridge } from './player.js';
import { ChatPanel } from './chat.js';
import { StreamAlerts } from './donations.js';
import { installChatSplitter } from './splitter.js';
import { ViewerControls } from './viewer-controls.js';
import { PlayerPanel } from './player-panel.js';
import { ViewerExtras } from './viewer-extras.js';
import { uptimeText, compactCount } from './cards.js';
import { noteConnectorLimit } from './live-connector.js';
import { playPreroll, cancelPreroll } from './ads.js';

const PHASE_TEXT = {
  idle: '',
  loadingInfo: '방송 정보를 불러오는 중…',
  loadingManifest: '스트림 정보를 불러오는 중…',
  connecting: '연결 중…',
  playing: '',
  paused: '일시정지',
  buffering: '버퍼링 중…',
  reconnecting: '재연결 중…',
  streamEnded: '방송이 종료되었습니다',
  offline: '현재 방송 중인 라이브가 없습니다',
  error: '재생 오류',
};

export class Viewer {
  constructor(root, { isLoggedIn, onClose }) {
    this.root = root;
    this.onClose = onClose;
    this.channelId = null;
    this.startToken = 0;
    this.header = root.querySelector('.stage-top');
    this.overlay = root.querySelector('.player-overlay');
    this.overlayText = this.overlay.querySelector('span');
    this.overlay.querySelector('.spinner').insertAdjacentHTML('afterend', streamEndedArt);
    this.status = root.querySelector('.player-status');
    this.volume = root.querySelector('#volume');
    this.muteButton = root.querySelector('#mute');

    this.player = new PlayerBridge(root.querySelector('#player-frame'));
    this.alerts = new StreamAlerts(root.querySelector('.player-stage'));
    this.chat = new ChatPanel(root.querySelector('.chat'), { isLoggedIn, onHighlight: (m) => this.alerts.push(m) });

    this.player.onMessage((type, data) => this.onPlayerMessage(type, data));
    listen(Events.playerEvent, (ev) => this.onPlayerEvent(ev));

    root.querySelector('#viewer-close').addEventListener('click', () => this.close());
    // 다시 연결은 처음 열 때의 플레이어 설정(음소거로 시작 등)을 그대로 쓴다.
    root.querySelector('#reload').addEventListener('click', () => this.channelId && this.open(this.channelId, this.lastSettings));
    root.querySelector('#fullscreen').addEventListener('click', () => {
      const stage = root.querySelector('.player-stage');
      if (document.fullscreenElement) document.exitFullscreen();
      else if (stage.requestFullscreen) stage.requestFullscreen();
    });
    this.muteButton.addEventListener('click', () => this.toggleMute());
    this.volume.addEventListener('input', () => {
      const v = Number(this.volume.value) / 100;
      this.player.call('setVolume', v);
      if (v > 0 && this.player.isMuted()) this.setMuted(false);
    });
    this.volume.addEventListener('change', () => {
      api.playerSetVolume(Number(this.volume.value) / 100).catch(() => {});
    });
    this.muteButton.innerHTML = icons.muted;
    this.audioSuspended = false;
    this.mutedBeforeSuspend = false;
    installChatSplitter({ container: root, chat: root.querySelector('.chat'), key: 'viewer' });
    this.controls = new ViewerControls(this);
    this.panel = new PlayerPanel(this);
    this.extras = new ViewerExtras(this, { isLoggedIn });
  }

  /** 재생 중인 채널이 있는가(화면을 숨겨도 재생은 이어진다). */
  get isPlaying() {
    return Boolean(this.channelId);
  }

  /** 재생은 두고 화면만 숨긴다 — 다른 메뉴로 갈 때. */
  hide() {
    this.root.hidden = true;
  }

  reveal() {
    if (this.channelId) this.root.hidden = false;
  }

  /** 멀티라이브가 소리를 가져갈 때 잠시 음소거한다(버튼 상태는 그대로). */
  suspendAudio(on) {
    if (on === this.audioSuspended) return;
    this.audioSuspended = on;
    if (on) {
      this.mutedBeforeSuspend = this.player.isMuted();
      this.player.call('setMuted', true);
    } else {
      this.player.call('setMuted', this.mutedBeforeSuspend);
    }
  }

  get isOpen() {
    return !this.root.hidden;
  }

  showOverlay(text, isError = false, detail = '') {
    if (!text) {
      this.overlay.hidden = true;
      this.overlay.removeAttribute('title');
      return;
    }
    // 기술적인 원문(hls 오류 종류 등)은 화면 문구 대신 툴팁으로만(L-11).
    if (detail) this.overlay.title = detail;
    else this.overlay.removeAttribute('title');
    this.overlay.hidden = false;
    const ended = text === PHASE_TEXT.streamEnded || text === PHASE_TEXT.offline;
    this.overlay.classList.toggle('error', isError);
    this.overlay.classList.toggle('ended', ended);
    this.overlay.querySelector('.spinner').hidden = isError || ended;
    this.overlayText.textContent = text;
  }

  renderHeader(live) {
    this.live = live;
    this.followerCount = null;
    const avatar = safeUrl(live && live.channelImageUrl);
    this.header.querySelector('.viewer-avatar').innerHTML = avatar
      ? `<img class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<div class="avatar"></div>';
    this.header.classList.toggle('is-live', Boolean(live && live.isLive));
    this.header.querySelector('.title').textContent = live ? live.liveTitle || '' : '';
    this.renderMeta();
    clearInterval(this.uptimeTimer);
    // 방송 경과 시간은 분 단위라 30초마다 다시 그리면 충분하다.
    if (live && live.isLive && live.openDate) this.uptimeTimer = setInterval(() => this.renderMeta(), 30000);
  }

  /** 제목 아래 두 줄 — 채널(인증·팔로워·카테고리)과 방송 지표 칩(LIVE·시청자·경과·누적). */
  renderMeta() {
    const live = this.live;
    const meta = this.header.querySelector('.meta');
    if (!live) {
      meta.innerHTML = '';
      return;
    }
    const channel = [
      `<b class="ch-name">${esc(live.channelName || '')}</b>${live.verified ? '<span class="verified" title="인증된 채널">✔</span>' : ''}`,
      this.followerCount != null ? `팔로워 ${esc(compactCount(this.followerCount))}명` : '',
      live.categoryName ? esc(live.categoryName) : '',
    ].filter(Boolean).join('<i class="sep">·</i>');
    const uptime = live.isLive ? uptimeText(live.openDate) : '';
    const stats = live.isLive
      ? [
        '<span class="stat live">LIVE</span>',
        `<span class="stat" title="현재 시청자"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/></svg>${esc(live.viewerCountText)}명</span>`,
        uptime ? `<span class="stat" title="방송 경과 시간"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>${esc(uptime)}</span>` : '',
        live.accumulateCount > 0 ? `<span class="stat" title="누적 시청자">누적 ${esc(live.accumulateCountText)}명</span>` : '',
      ].filter(Boolean).join('')
      : '<span class="stat off">오프라인</span>';
    meta.innerHTML = `<div class="meta-channel ellipsis">${channel}</div><div class="meta-stats">${stats}</div>`;
  }

  /** 팔로워 수는 라이브 상세에 없어 채널 정보로 따로 채운다 — 늦게 와도 같은 채널일 때만. */
  async loadFollowers(channelId, token) {
    try {
      const info = await api.channelInfo(channelId);
      if (token !== this.startToken || !this.live) return;
      this.followerCount = info.followerCount;
      this.renderMeta();
    } catch { /* 팔로워 없이도 헤더는 충분하다 */ }
  }

  async open(channelId, settings) {
    const token = ++this.startToken;
    if (channelId !== this.channelId) this.cancelAd();
    this.channelId = channelId;
    // 성능 수집기가 player.net(채널 앞 6자)과 맞춘다
    const frame = document.getElementById('player-frame');
    if (frame) frame.dataset.perfChannel = channelId; // data-channel-id 는 카드 열기 속성이라 쓰지 않는다
    if (settings) this.lastSettings = settings;
    this.root.hidden = false;
    this.renderHeader(null);
    this.showOverlay(PHASE_TEXT.loadingInfo);
    this.status.textContent = '';
    if (settings && typeof settings.volumeLevel === 'number') {
      this.volume.value = String(Math.round(settings.volumeLevel * 100));
      // 채움 색(--fill)도 — 속성이 아니라 값을 바꿔 viewer-controls 의 감시자가 모른다(input 을 쏘면 음소거가 풀린다)
      this.volume.style.setProperty('--fill', `${this.volume.value}%`);
    }
    this.startMuted = settings ? Boolean(settings.startMuted) : false;
    try {
      const started = await api.playerStart(channelId);
      if (token !== this.startToken) return;
      this.renderHeader(started.live);
      this.loadFollowers(channelId, token);
      this.extras.loadRail(channelId);
      if (started.audioOnly) this.status.textContent = '오디오 전용 방송';
      noteConnectorLimit(started.connectorLimitedHeight);
      this.chat.connect(started.live.chatChannelId, started.live.channelId);
    } catch (e) {
      if (token !== this.startToken) return;
      // 방송 중이 아니어도 헤더·채팅은 보여 준다.
      try {
        const live = await api.liveDetail(channelId, true);
        if (token !== this.startToken) return;
        this.renderHeader(live);
        this.loadFollowers(channelId, token);
        if (live.chatChannelId) this.chat.connect(live.chatChannelId, live.channelId);
        this.showOverlay(live.isLive ? e.message : PHASE_TEXT.offline, live.isLive);
      } catch {
        this.showOverlay(e.message, true);
      }
    }
  }

  /**
   * 닫기. `silent` 면 onClose(뒤로 가기 등)를 부르지 않는다 — 채널을 멀티라이브로 넘기는 경우.
   * 기다리는 동안 다른 방송을 열면(open 이 토큰을 올림) 낡은 onClose 가 새 방송에서 뒤로 가지 않게 건너뛴다.
   */
  async close({ silent = false } = {}) {
    const token = ++this.startToken;
    this.cancelAd();
    this.root.hidden = true;
    this.channelId = null;
    clearInterval(this.uptimeTimer);
    this.alerts.clear();
    this.showOverlay('');
    await Promise.allSettled([api.playerStop(), this.chat.disconnect()]);
    if (!silent && token === this.startToken && this.onClose) this.onClose();
  }

  /** 새로 연 방송의 프리롤 — 영상 영역 안에만 띄우고, 그동안 이 플레이어 소리를 막는다(기다리지 않는다). */
  playAd(channelId) {
    const host = this.root.querySelector('.player-stage');
    playPreroll('live', channelId, { host, player: this.player, target: 'viewer' }).catch(() => {});
  }

  cancelAd() {
    cancelPreroll(this.root.querySelector('.player-stage'));
  }

  setMuted(muted) {
    this.player.call('setMuted', muted);
    this.muteButton.innerHTML = muted ? icons.muted : icons.volume;
    this.muteButton.title = muted ? '음소거 해제' : '음소거';
    this.muteButton.setAttribute('aria-label', this.muteButton.title);
  }

  toggleMute() {
    this.setMuted(!this.player.isMuted());
  }

  onPlayerMessage(type, data) {
    if (type === 'playStarted') {
      this.showOverlay('');
      // 페이지는 자동재생 정책 때문에 음소거로 시작한다 — 설정에 따라 바로 해제한다.
      this.player.call('setVolume', Number(this.volume.value) / 100);
      if (this.audioSuspended) {
        this.mutedBeforeSuspend = this.startMuted;
        this.player.call('setMuted', true);
      } else {
        this.setMuted(this.startMuted);
      }
    } else if (type === 'fatalError') {
      // 사용자에게는 쉬운 문구, 원문은 툴팁·콘솔에(L-11).
      const detail = [data.type, data.details].filter(Boolean).join(' · ');
      if (detail) console.warn('[viewer] 재생 오류:', detail, data);
      this.showOverlay('영상을 재생하지 못했습니다. 잠시 뒤 다시 시도해 주세요.', true, detail ? `오류 정보: ${detail}` : '');
    } else if (type === 'playbackState') {
      if (data.state === 'stalled') this.showOverlay(PHASE_TEXT.buffering);
      else this.showOverlay('');
    } else if (type === 'metrics' && typeof data.latency === 'number' && data.latency > 0) {
      const res = data.currentLevelHeight ? `${data.currentLevelHeight}p · ` : '';
      this.status.textContent = `${res}지연 ${data.latency.toFixed(1)}초`;
    }
  }

  onPlayerEvent(ev) {
    // 다른 메뉴로 가서 화면만 숨었어도 재생은 계속되므로 종료·오류는 받아 둔다(돌아왔을 때 낡은 화면이 남지 않게).
    if (!ev || !this.channelId) return;
    if (ev.type === 'phase') {
      if (ev.phase === 'playing' || ev.phase === 'idle') return;
      const text = PHASE_TEXT[ev.phase];
      if (ev.phase === 'error') this.showOverlay(ev.message || text, true);
      else if (text) this.showOverlay(text);
    } else if (ev.type === 'quality') {
      this.status.textContent = ev.resolution ? `${ev.name} (${ev.resolution})` : ev.name;
    } else if (ev.type === 'ended') {
      this.showOverlay(PHASE_TEXT.streamEnded);
    } else if (ev.type === 'error') {
      toast(ev.message);
    }
  }

  refreshAuth() {
    this.chat.updateInput();
  }
}
