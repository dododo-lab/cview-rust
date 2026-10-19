// 라이브 시청 화면 — 플레이어 영역 + 채팅 패널.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast, icons } from './dom.js';
import { PlayerBridge } from './player.js';
import { ChatPanel } from './chat.js';

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
  error: '재생 오류',
};

export class Viewer {
  constructor(root, { isLoggedIn, onClose }) {
    this.root = root;
    this.onClose = onClose;
    this.channelId = null;
    this.startToken = 0;
    this.header = root.querySelector('.viewer-header');
    this.overlay = root.querySelector('.player-overlay');
    this.overlayText = this.overlay.querySelector('span');
    this.status = root.querySelector('.player-status');
    this.volume = root.querySelector('#volume');
    this.muteButton = root.querySelector('#mute');

    this.player = new PlayerBridge(root.querySelector('#player-frame'));
    this.chat = new ChatPanel(root.querySelector('.chat'), { isLoggedIn });

    this.player.onMessage((type, data) => this.onPlayerMessage(type, data));
    listen(Events.playerEvent, (ev) => this.onPlayerEvent(ev));

    root.querySelector('#viewer-close').addEventListener('click', () => this.close());
    root.querySelector('#reload').addEventListener('click', () => this.channelId && this.open(this.channelId));
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
  }

  get isOpen() {
    return !this.root.hidden;
  }

  showOverlay(text, isError = false) {
    if (!text) {
      this.overlay.hidden = true;
      return;
    }
    this.overlay.hidden = false;
    this.overlay.classList.toggle('error', isError);
    this.overlay.querySelector('.spinner').hidden = isError || text === PHASE_TEXT.streamEnded;
    this.overlayText.textContent = text;
  }

  renderHeader(live) {
    const avatar = safeUrl(live && live.channelImageUrl);
    const title = live ? live.liveTitle : '';
    const meta = live
      ? [live.channelName, live.categoryName, live.isLive ? `${live.viewerCountText}명 시청 중` : '오프라인']
        .filter(Boolean).map(esc).join(' · ')
      : '';
    this.header.querySelector('.viewer-avatar').innerHTML = avatar
      ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '<div class="avatar"></div>';
    this.header.querySelector('.title').textContent = title || '';
    this.header.querySelector('.meta').innerHTML = meta;
  }

  async open(channelId, settings) {
    const token = ++this.startToken;
    this.channelId = channelId;
    this.root.hidden = false;
    this.renderHeader(null);
    this.showOverlay(PHASE_TEXT.loadingInfo);
    this.status.textContent = '';
    if (settings && typeof settings.volumeLevel === 'number') {
      this.volume.value = String(Math.round(settings.volumeLevel * 100));
    }
    this.startMuted = settings ? Boolean(settings.startMuted) : false;
    try {
      const started = await api.playerStart(channelId);
      if (token !== this.startToken) return;
      this.renderHeader(started.live);
      if (started.audioOnly) this.status.textContent = '오디오 전용 방송';
      this.chat.connect(started.live.chatChannelId, started.live.channelId);
    } catch (e) {
      if (token !== this.startToken) return;
      // 방송 중이 아니어도 헤더·채팅은 보여 준다.
      try {
        const live = await api.liveDetail(channelId, true);
        if (token !== this.startToken) return;
        this.renderHeader(live);
        if (live.chatChannelId) this.chat.connect(live.chatChannelId, live.channelId);
        this.showOverlay(live.isLive ? e.message : '현재 방송 중인 라이브가 없습니다', live.isLive);
      } catch {
        this.showOverlay(e.message, true);
      }
    }
  }

  async close() {
    this.startToken += 1;
    this.root.hidden = true;
    this.channelId = null;
    this.showOverlay('');
    await Promise.allSettled([api.playerStop(), this.chat.disconnect()]);
    if (this.onClose) this.onClose();
  }

  setMuted(muted) {
    this.player.call('setMuted', muted);
    this.muteButton.innerHTML = muted ? icons.muted : icons.volume;
    this.muteButton.title = muted ? '음소거 해제' : '음소거';
  }

  toggleMute() {
    this.setMuted(!this.player.isMuted());
  }

  onPlayerMessage(type, data) {
    if (type === 'playStarted') {
      this.showOverlay('');
      // 페이지는 자동재생 정책 때문에 음소거로 시작한다 — 설정에 따라 바로 해제한다.
      this.player.call('setVolume', Number(this.volume.value) / 100);
      this.setMuted(this.startMuted);
    } else if (type === 'fatalError') {
      this.showOverlay(`재생 오류: ${data.details || data.type || ''}`, true);
    } else if (type === 'playbackState') {
      if (data.state === 'stalled') this.showOverlay(PHASE_TEXT.buffering);
      else this.showOverlay('');
    } else if (type === 'metrics' && typeof data.latency === 'number' && data.latency > 0) {
      const res = data.currentLevelHeight ? `${data.currentLevelHeight}p · ` : '';
      this.status.textContent = `${res}지연 ${data.latency.toFixed(1)}초`;
    }
  }

  onPlayerEvent(ev) {
    if (!ev || !this.isOpen) return;
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
