// 라이브 시청 화면 — 플레이어 영역 + 채팅 패널.
import { api, listen, Events } from './api.js';
import { esc, safeUrl, toast, icons, imageSrc, streamEndedArt } from './dom.js';
import { PlayerBridge } from './player.js';
import { ChatPanel } from './chat.js';
import { StreamAlerts } from './donations.js';
import { installChatSplitter } from './splitter.js';
import { ViewerControls, toggleFullscreenOn } from './viewer-controls.js';
import { PlayerPanel } from './player-panel.js';
import { ViewerExtras } from './viewer-extras.js';
import { uptimeText, compactCount } from './cards.js';
import { noteConnectorLimit } from './live-connector.js';
import { playPreroll, cancelPreroll } from './ads.js';
import { verifiedBadge } from './verified-badge.js';
import { liveHandlers } from './now-playing.js';
import { ViewerTopBar } from './viewer-topbar.js';

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
  constructor(root, { isLoggedIn, onClose, onAddMulti = null, onPopout = null, chatSession = null }) {
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
    this.chat = new ChatPanel(root.querySelector('.chat'), { isLoggedIn, onHighlight: (m) => this.alerts.push(m), session: chatSession, mentionNotify: true });

    this.player.onMessage((type, data) => this.onPlayerMessage(type, data));
    listen(Events.playerEvent, (ev) => this.onPlayerEvent(ev));

    root.querySelector('#viewer-close').addEventListener('click', () => this.close());
    // 다시 연결은 처음 열 때의 플레이어 설정(음소거로 시작 등)을 그대로 쓴다.
    root.querySelector('#reload').addEventListener('click', () => this.channelId && this.open(this.channelId, this.lastSettings));
    root.querySelector('#fullscreen').addEventListener('click', () => toggleFullscreenOn(root.querySelector('.player-stage')));
    this.muteButton.addEventListener('click', () => this.toggleMute());
    this.volume.addEventListener('input', () => {
      const v = Number(this.volume.value) / 100;
      this.player.call('setVolume', v);
      if (v > 0 && this.sessionMuted) this.setMuted(false);
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
    // 위 막대 동작 묶음(채널·보기·⋯) — 채팅 표시 방식 단추(extras)를 '보기' 묶음으로 옮기므로 그 뒤에 만든다.
    this.topbar = new ViewerTopBar(this, { isLoggedIn, onAddMulti, onPopout });
    // 시스템 '지금 재생 중'(macOS 섬·제어 센터)의 재생·일시정지 — 화면의 재생 버튼과 같은 길로.
    const fromSystem = (name) => () => {
      this.player.call(name);
      setTimeout(() => this.controls.renderStatus(), 150);
    };
    this.nowPlayingHandlers = liveHandlers({ play: fromSystem('play'), pause: fromSystem('pause') });
  }

  /** 시스템 '지금 재생 중' 정보 — 방송 제목·채널·썸네일. 방송이 없으면 지운다. */
  syncNowPlaying(live) {
    if (!live || !this.channelId) {
      this.player.setNowPlaying(null);
      return;
    }
    this.player.setNowPlaying({
      kind: 'live',
      title: live.liveTitle,
      channelName: live.channelName,
      channelId: this.channelId,
      category: live.categoryName || '',
      thumbnailUrl: live.thumbnailUrl,
      imageUrl: live.channelImageUrl,
    }, this.nowPlayingHandlers);
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
      this.mutedBeforeSuspend = Boolean(this.sessionMuted);
      this.player.call('setMuted', true);
    } else {
      this.player.call('setMuted', this.mutedBeforeSuspend);
    }
  }

  get isOpen() {
    return !this.root.hidden;
  }

  showOverlay(text, isError = false, detail = '') {
    // 안내막이 떠 있는 동안 무대 가운데 재생 단추·버퍼링 고리를 감춘다(app.css .has-overlay) — 두 겹으로 보이지 않게.
    this.overlay.parentElement?.classList.toggle('has-overlay', Boolean(text));
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
      `<b class="ch-name">${esc(live.channelName || '')}</b>${verifiedBadge(live)}`,
      this.followerCount != null ? `팔로워 ${esc(compactCount(this.followerCount))}명` : '',
      live.categoryName ? esc(live.categoryName) : '',
    ].filter(Boolean).join('<i class="sep">·</i>');
    const uptime = live.isLive ? uptimeText(live.openDate) : '';
    const stats = live.isLive
      ? [
        '<span class="stat live">LIVE</span>',
        `<span class="stat" title="현재 시청자"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/></svg>${esc(live.viewerCountText)}명</span>`,
        uptime ? `<span class="stat" title="방송 경과 시간"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>${esc(uptime)}</span>` : '',
        live.accumulateCount > 0 ? `<span class="stat stat-acc" title="누적 시청자">누적 ${esc(live.accumulateCountText)}명</span>` : '',
      ].filter(Boolean).join('')
      : '<span class="stat off">오프라인</span>';
    // 틀은 한 번만 만들고 칸 안만 바꾼다 — 30초마다(경과 시간)·팔로워가 오면 다시 그리는데, 통째로 바꾸면 지표 줄의
    // 화질·지연 배지(viewer-topbar.js)가 떼였다 붙어 키보드 초점이 body 로 떨어지고 위 막대가 숨었다.
    let line = meta.querySelector('.meta-channel');
    let chips = meta.querySelector('.meta-chips');
    if (!line || !chips) {
      meta.innerHTML = '<div class="meta-channel ellipsis"></div><div class="meta-stats"><span class="meta-chips"></span></div>';
      line = meta.querySelector('.meta-channel');
      chips = meta.querySelector('.meta-chips');
    }
    if (line.innerHTML !== channel) line.innerHTML = channel;
    if (chips.innerHTML !== stats) chips.innerHTML = stats;
    this.topbar?.attachBadge(); // 화질·지연 배지는 칩 묶음 옆(형제) — 칩만 다시 그려도 떼이지 않는다
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

  /**
   * 플레이어 문서는 처음 열 때 띄운다 — 시청 화면은 라이브 칸이 가득 찼을 때·보조 플레이어 창에서만 쓰는데, 예전엔
   * 부팅(채팅·통계 보조 창 포함)마다 player.html·hls.js(~800KB)를 숨은 iframe 에서 파싱·실행했다.
   * 문서를 바꿔도 contentWindow 는 같은 객체라 브리지 연결은 그대로고, 그 전에 온 명령은 ready 까지 줄 선다.
   */
  ensurePlayerPage() {
    const frame = this.player.iframe;
    if (frame.dataset.src && !frame.getAttribute('src')) frame.src = frame.dataset.src;
  }

  async open(channelId, settings) {
    const token = ++this.startToken;
    if (channelId !== this.channelId) this.cancelAd();
    this.channelId = channelId;
    this.audioOnly = false;
    // 성능 수집기가 player.net(채널 앞 6자)과 맞춘다
    const frame = document.getElementById('player-frame');
    if (frame) frame.dataset.perfChannel = channelId; // data-channel-id 는 카드 열기 속성이라 쓰지 않는다
    this.ensurePlayerPage();
    // 다시 연결·다시 시도는 처음 받은 설정 객체를 다시 넘긴다 — 그 볼륨은 연 뒤 바꾼 값이 아니라서
    // 슬라이더를 되돌리면 소리가 저장된 볼륨과 어긋났다. 같은 객체면 지금 슬라이더(=저장된 볼륨)를 둔다.
    const reopened = Boolean(settings) && settings === this.lastSettings;
    if (settings) this.lastSettings = settings;
    this.root.hidden = false;
    this.renderHeader(null);
    if (channelId !== this.topbar.loadedFor) { this.topbar.loadedFor = channelId; this.topbar.load(channelId); }
    this.syncNowPlaying(null);
    this.showOverlay(PHASE_TEXT.loadingInfo);
    this.controls.resetStream();
    this.status.textContent = '';
    if (settings && !reopened && typeof settings.volumeLevel === 'number') {
      this.volume.value = String(Math.round(settings.volumeLevel * 100));
      // 채움 색(--fill)도 — 속성이 아니라 값을 바꿔 viewer-controls 의 감시자가 모른다(input 을 쏘면 음소거가 풀린다)
      this.volume.style.setProperty('--fill', `${this.volume.value}%`);
    }
    this.startMuted = settings ? Boolean(settings.startMuted) : false;
    // 이번 시청의 음소거 — 재연결·화질 바꾸기·정지 복구로 페이지가 다시 시작해도(playStarted 가 또 온다)
    // 사용자가 바꾼 음소거를 지킨다. 예전엔 매번 '음소거로 시작' 설정으로 되돌아가 소리가 갑자기 났다.
    // 다시 연결·다시 시도(같은 설정 객체)도 같은 시청이다 — 예전엔 여기서도 설정값으로 되돌려, M 으로 끈 소리가 ⟳ 뒤에 났다.
    if (!reopened) this.sessionMuted = this.startMuted;
    // 녹화 중에 다시 연결하거나 다른 채널을 열면 재생 시작(Rust)이 녹화를 끝낸다 — 닫기처럼 먼저 끝내고 저장 위치를
    // 알린다. 예전엔 녹화 표시만 조용히 사라져 파일이 어디 있는지 알 수 없었다.
    if (this.panel.recording) {
      await this.panel.resetRecording();
      if (token !== this.startToken) return;
    }
    try {
      const started = await api.playerStart(channelId);
      if (token !== this.startToken) return;
      this.renderHeader(started.live);
      this.syncNowPlaying(started.live);
      this.loadFollowers(channelId, token);
      this.extras.loadRail(channelId);
      this.audioOnly = Boolean(started.audioOnly);
      if (this.audioOnly) this.status.textContent = '오디오 전용 방송';
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
        // 채팅 채널이 없으면 앞 채널 채팅을 끊는다 — 예전엔 머리줄은 새 채널인데 앞 채널 채팅이 계속 오고 보낼 수도 있었다.
        if (live.chatChannelId) this.chat.connect(live.chatChannelId, live.channelId);
        else if (this.chat.active) this.chat.disconnect();
        this.showOverlay(live.isLive ? e.message : PHASE_TEXT.offline, live.isLive);
      } catch {
        if (token !== this.startToken) return;
        if (this.chat.active) this.chat.disconnect();
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
    // 전체 화면이던 무대·시청 화면을 닫으면 숨긴 요소가 전체 화면으로 남아 검은 화면만 보였다 — 먼저 빠져나온다.
    const fs = document.fullscreenElement;
    if (fs && (fs === this.root || this.root.contains(fs)) && document.exitFullscreen) document.exitFullscreen().catch(() => {});
    this.root.hidden = true;
    this.channelId = null;
    this.topbar.loadedFor = null;
    this.topbar.closeMenu();
    this.syncNowPlaying(null);
    clearInterval(this.uptimeTimer);
    this.alerts.clear();
    const recording = this.panel.resetRecording();
    this.showOverlay('');
    // 녹화를 먼저 끝내야 저장 경로를 알릴 수 있다(재생 정지도 녹화를 끝내지만 경로는 돌려주지 않는다).
    await recording;
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
    this.sessionMuted = muted;
    this.player.call('setMuted', muted);
    this.muteButton.innerHTML = muted ? icons.muted : icons.volume;
    this.muteButton.title = muted ? '음소거 해제' : '음소거';
    this.muteButton.setAttribute('aria-label', this.muteButton.title);
  }

  /** 사용자가 고른 음소거(sessionMuted)를 뒤집는다. 실제 video.muted 는 광고 중(setAdHold)·첫 프레임 전에 앱이
   *  잠시 켜 두므로 그것을 읽으면 그 사이에는 음소거할 수 없었다(누를 때마다 '해제'만 되어 광고 뒤 소리가 났다). */
  toggleMute() {
    this.setMuted(!this.sessionMuted);
  }

  onPlayerMessage(type, data) {
    if (type === 'playStarted') {
      this.showOverlay('');
      // 페이지는 자동재생 정책 때문에 음소거로 시작한다 — 설정에 따라 바로 해제한다.
      this.player.call('setVolume', Number(this.volume.value) / 100);
      if (this.audioSuspended) {
        this.mutedBeforeSuspend = this.sessionMuted;
        this.player.call('setMuted', true);
      } else {
        this.setMuted(this.sessionMuted);
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
      // 플레이어 metrics 는 videoHeight 를 보낸다(currentLevelHeight 는 예전 이름이라 해상도가 늘 비었다).
      const h = data.videoHeight || data.currentLevelHeight;
      const res = h ? `${h}p · ` : '';
      // 오디오 전용은 2초마다 오는 metrics 가 덮어 사라졌다 — 앞에 붙여 둔다.
      this.status.textContent = `${this.audioOnly ? '오디오 전용 · ' : res}지연 ${data.latency.toFixed(1)}초`;
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
      this.player.setNowPlaying(null);
    } else if (ev.type === 'error') {
      toast(ev.message);
    }
  }

  /** @param {boolean} [changed] 로그인 상태가 바뀌었는가 — 바뀌었을 때만 위 막대 팔로우 단추를 다시 읽는다. */
  refreshAuth(changed = false) {
    this.chat.updateInput();
    // 위 막대 팔로우 단추 — 로그인하면 나타나고 로그아웃하면 숨는다(지금 채널 상태를 다시 읽는다).
    if (changed && this.channelId) this.topbar.load(this.channelId);
  }
}
