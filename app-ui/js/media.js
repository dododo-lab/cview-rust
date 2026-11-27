// 다시보기·클립 플레이어 — 원본 VODPlayerView / ClipPlayerView / ClipFilmstripDock.
// VOD-02 VOD 플레이어(시크·속도·화질·이어 보기), VOD-03 클립 플레이어(다음 클립 독).
// 다시보기는 WebView 기본 HLS(<video src=m3u8>)로 재생한다 — CDN CORS 를 피하고 macOS 기본 컨트롤·PiP 를
// 쓴다. 클립은 원본처럼 치지직 임베드 페이지가 주 경로다(대부분 ABR_HLS 라 인키 재생이 막힌다).
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc, keyBelongsToTarget, plainLetter } from './dom.js';
import { emptyState, loadingState, compactCount, errorState } from './cards.js';
import { formatDuration, relativeDate, clipCard } from './channel.js';
import { startDownload } from './downloads.js';
import { previewAttr } from './hover-preview.js';
import { playPreroll, cancelPreroll } from './ads.js';
import { applyNowPlaying, clearNowPlaying, vodHandlers } from './now-playing.js';

const ICON_BACK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M15 5l-7 7 7 7"/></svg>';
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const SAVE_EVERY_MS = 10000;

export class MediaPage {
  constructor(content, { onBack, onOpenChannel }) {
    this.onBack = onBack;
    this.onOpenChannel = onOpenChannel;
    this.acceptsParams = true;
    this.root = document.createElement('section');
    this.root.className = 'page page-media';
    this.root.dataset.page = 'media';
    this.root.hidden = true;
    this.root.innerHTML = '<div class="page-body"></div>';
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.seq = 0;
    this.info = null;
    this.saveTimer = 0;
    this.body.addEventListener('click', (e) => this.onClick(e));
    document.addEventListener('keydown', (e) => this.onKey(e));
    this.body.addEventListener('change', (e) => this.onChange(e));
    // 화면을 떠나면 멈추고 위치를 저장한다.
    // 진행 중인 열기(vodPlay 응답 대기)도 무효로 — 늦은 응답이 떠난 영상의 지금 재생 중 정보를 다시 넣지 않게.
    new MutationObserver(() => { if (this.root.hidden) { this.seq++; this.leave(); } })
      .observe(this.root, { attributes: true, attributeFilter: ['hidden'] });
  }

  get video() {
    return this.body.querySelector('video');
  }

  async show(params = {}) {
    const seq = ++this.seq;
    this.leave();
    this.kind = params.kind;
    this.body.innerHTML = loadingState();
    try {
      if (params.kind === 'vod') await this.openVod(Number(params.id), seq);
      else if (params.kind === 'clip') await this.openClip(String(params.id), seq);
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('재생할 수 없습니다', e, '<button class="btn" data-media="back">뒤로</button>');
    }
  }

  async openVod(videoNo, seq) {
    // 재생 정보부터 — 예전엔 광고를 먼저 다 본 뒤에야 성인·구독자 전용·삭제된 영상이라 '재생할 수 없습니다'가 떴다.
    // 실패하면 광고 없이 바로 오류(show 의 catch).
    const info = await api.vodPlay(videoNo);
    if (seq !== this.seq) return;
    // 치트키가 없으면 치지직처럼 재생 전에 프리롤 광고를 본다(그 사이 다른 영상을 열면 그만).
    if (!(await playPreroll('vod', videoNo))) {
      // 광고를 그만뒀다 — 다른 영상에 밀려났거나(그쪽이 화면을 맡는다) 광고 중에 다른 화면으로 옮겨 이 화면이
      // 숨으며(leave) 그만둔 것이다. 이미 떠난 화면에서 뒤로 가기를 또 부르면 사용자가 고른 화면에서 한 번 더
      // 뒤로 밀려났다 — 이 화면이 아직 보일 때만 돌아간다.
      if (seq === this.seq && !this.root.hidden && this.onBack) this.onBack();
      return;
    }
    if (seq !== this.seq) return;
    this.info = info;
    const qualities = info.qualities || [];
    this.body.innerHTML = `
      <button class="back-btn" data-media="back">${ICON_BACK}<span>뒤로</span></button>
      <div class="media-layout">
        <div class="media-main">
          <div class="media-stage"><video controls playsinline autoplay preload="auto"></video></div>
          <div class="media-bar">
            <div class="media-title-box">
              <h1 class="media-title">${esc(info.title)}</h1>
              <div class="media-meta">
                ${info.channelId ? `<button class="link-btn" data-media="channel" data-channel-id="${esc(info.channelId)}" data-channel-name="${esc(info.channelName || '')}">${esc(info.channelName)}</button> · ` : `${esc(info.channelName)} · `}
                조회 ${esc(compactCount(info.readCount || 0))}${info.publishDate ? ` · ${esc(relativeDate(info.publishDate))}` : ''} · ${esc(formatDuration(info.duration))}
              </div>
            </div>
            <div class="grow"></div>
            ${qualities.length > 1 ? `<label class="media-select">화질 <select class="select" data-media-select="quality">
              <option value="">자동</option>${qualities.map((q, i) => `<option value="${i}">${esc(q.resolution || q.name)}</option>`).join('')}
            </select></label>` : ''}
            <label class="media-select">속도 <select class="select" data-media-select="speed">
              ${SPEEDS.map((s) => `<option value="${s}" ${s === 1 ? 'selected' : ''}>${s}x</option>`).join('')}
            </select></label>
            <button class="btn${info.watchLater ? ' primary' : ''}" data-media="later">${info.watchLater ? '나중에 보기 ✓' : '나중에 보기'}</button>
            <button class="btn" data-media="download" title="다시보기를 파일로 받기">받기</button>
            <button class="btn ghost" data-media="external">치지직에서 열기</button>
          </div>
        </div>
      </div>`;
    const video = this.video;
    video.src = info.streamUrl;
    // 시스템 '지금 재생 중'(macOS 섬·제어 센터) — 이 영상은 앱 문서에 있으므로 앱 창의 mediaSession 에 넣는다.
    applyNowPlaying(window, {
      kind: 'vod',
      title: info.title,
      channelName: info.channelName,
      thumbnailUrl: info.thumbnailUrl,
    }, vodHandlers(video));
    if (info.resumeAt > 0) {
      const resume = () => { video.currentTime = info.resumeAt; };
      video.addEventListener('loadedmetadata', resume, { once: true });
      // 처음부터 보고 싶으면 바로 되돌릴 수 있게(감사 UX-M07). 메타데이터 전에 눌러도 — 예전엔 남은 이어 보기 처리가
      // 나중에 다시 이어 보기 위치로 옮겼다.
      toast(`${formatDuration(info.resumeAt)}부터 이어서 재생합니다`, 5000, { label: '처음부터', run: () => { video.removeEventListener('loadedmetadata', resume); video.currentTime = 0; } });
    }
    // 실패하면 2.6초 토스트만 뜨고 검은 화면이 남았다(감사 UX-C06) — 무대 안에 다시 시도·치지직에서 열기를 둔다.
    video.addEventListener('error', () => {
      if (seq === this.seq) this.showStageError(video);
    });
    this.savedAt = null;
    this.saveTimer = setInterval(() => this.saveProgress(), SAVE_EVERY_MS);
    video.addEventListener('pause', () => this.saveProgress());
  }

  async openClip(clipUid, seq) {
    const res = await api.clipPlay(clipUid);
    if (seq !== this.seq) return;
    const d = res.detail;
    this.info = { clipUid, ...d };
    const channelId = d.optionalProperty?.ownerChannel?.channelId || d.ownerChannel?.channelId || d.channel?.channelId || d.ownerChannelId || '';
    const channelName = d.optionalProperty?.ownerChannel?.channelName || d.ownerChannel?.channelName || d.channel?.channelName || '';
    this.body.innerHTML = `
      <button class="back-btn" data-media="back">${ICON_BACK}<span>뒤로</span></button>
      <div class="media-layout clip">
        <div class="media-main">
          <div class="media-stage vertical"><iframe class="clip-embed" src="https://chzzk.naver.com/embed/clip/${encodeURIComponent(clipUid)}" title="클립" allow="autoplay; fullscreen; picture-in-picture" allowfullscreen></iframe></div>
          <div class="media-bar">
            <div class="media-title-box">
              <h1 class="media-title">${esc(d.clipTitle || '')}</h1>
              <div class="media-meta">
                ${channelId ? `<button class="link-btn" data-media="channel" data-channel-id="${esc(channelId)}" data-channel-name="${esc(channelName || '')}">${esc(channelName || '채널')}</button> · ` : ''}조회 ${esc(compactCount(d.readCount || 0))} · ${esc(formatDuration(d.duration))}
              </div>
            </div>
            <div class="grow"></div>
            <button class="btn" data-media="download" title="클립을 파일로 받기">받기</button>
            <button class="btn ghost" data-media="external">치지직에서 열기</button>
          </div>
        </div>
        <section class="media-side" aria-label="다른 클립"><h2 class="section-title">다른 클립</h2><div class="media-dock">${loadingState()}</div></section>
      </div>`;
    this.loadDock(channelId, clipUid, seq);
  }

  async loadDock(channelId, currentUid, seq) {
    const dock = this.body.querySelector('.media-dock');
    try {
      const list = channelId
        ? (await api.channelClips(channelId, null, 20)).items
        : await api.popularClips('WITHIN_7_DAYS', 'POPULAR');
      if (seq !== this.seq || !dock) return;
      const others = list.filter((c) => c.clipUID !== currentUid).slice(0, 16);
      dock.innerHTML = others.length ? `<div class="grid clip-grid dock-grid">${others.map(clipCard).join('')}</div>` : emptyState('다른 클립이 없습니다');
    } catch {
      if (seq === this.seq && dock) dock.innerHTML = emptyState('목록을 불러오지 못했습니다');
    }
  }

  saveProgress() {
    const v = this.video;
    const info = this.info;
    if (!v || !info || this.kind !== 'vod' || !Number.isFinite(v.currentTime) || v.currentTime < 1) return;
    // 위치가 그대로면(일시정지한 채 두기) 다시 쓰지 않는다 — 10초마다(시간당 360번) 같은 값을 DB 에 커밋했다.
    if (this.savedAt != null && Math.abs(v.currentTime - this.savedAt) < 0.5) return;
    this.savedAt = v.currentTime;
    api.vodProgressSave(info.videoNo, v.currentTime, v.duration || info.duration, info.title, info.channelName, info.thumbnailUrl || null)
      .catch(() => {});
  }

  /**
   * 다시보기·클립 키 — Space 재생/멈춤, ←/→ 10초(⇧ 60초), ↑/↓ 볼륨, M 소리, F 전체 화면. 예전엔 영상에 초점이 있을 때만
   * 기본 컨트롤이 받아 화면 어디서나 쓰던 라이브와 달랐다(감사 UX-M07). 영상 자체에 초점이 있으면 기본 컨트롤에 맡긴다.
   */
  onKey(e) {
    if (this.root.hidden || e.metaKey || e.ctrlKey || e.altKey || keyBelongsToTarget(e)) return;
    const v = this.video;
    if (!v || e.target === v || document.querySelector('.ad-overlay')) return;
    // 글자 키는 물리 키로도 — 한글 입력 상태에서 K·M·F 가 'ㅏ'·'ㅡ'·'ㄹ' 로 와 듣지 않았다(시청 화면과 같게).
    const k = e.key.length === 1 ? (plainLetter(e) || e.key) : e.key;
    if (k === ' ' || k === 'k') { e.preventDefault(); if (v.paused) v.play().catch(() => {}); else v.pause(); }
    else if (k === 'ArrowLeft' || k === 'ArrowRight') {
      e.preventDefault();
      const d = (e.shiftKey ? 60 : 10) * (k === 'ArrowLeft' ? -1 : 1);
      v.currentTime = Math.max(0, Math.min(v.duration || Infinity, v.currentTime + d));
    } else if (k === 'ArrowUp' || k === 'ArrowDown') {
      e.preventDefault();
      v.volume = Math.max(0, Math.min(1, Math.round((v.volume + (k === 'ArrowUp' ? 0.05 : -0.05)) * 100) / 100));
      if (v.volume > 0) v.muted = false;
    } else if (k === 'm') v.muted = !v.muted;
    else if (k === 'f') {
      if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
      else v.requestFullscreen?.().catch(() => {});
    }
  }

  /** 다시보기 재생 실패 안내 — 영상 위에 문구와 '다시 시도'·'치지직에서 열기'(onClick 의 retry·external). */
  showStageError(video) {
    const stage = video.closest('.media-stage');
    if (!stage || stage.querySelector('.media-error')) return;
    stage.insertAdjacentHTML('beforeend', `
      <div class="media-error" role="alert">
        <p>영상을 불러오지 못했습니다</p>
        <div class="error-actions"><button class="btn" data-media="retry">다시 시도</button><button class="btn ghost" data-media="external">치지직에서 열기</button></div>
      </div>`);
  }

  leave() {
    cancelPreroll();
    clearInterval(this.saveTimer);
    clearNowPlaying(window);
    // 클립은 치지직 임베드 iframe 이라 화면을 떠나도 소리가 계속 났다 — 비운다.
    for (const f of this.root.querySelectorAll('iframe.clip-embed')) f.src = 'about:blank';
    const v = this.video;
    if (v) {
      this.saveProgress();
      v.pause();
      v.removeAttribute('src');
      v.load();
    }
  }

  async onClick(e) {
    const clip = e.target.closest('[data-clip-uid]');
    if (clip) {
      window.dispatchEvent(new CustomEvent('cview:open-clip', { detail: { clipUid: clip.dataset.clipUid } }));
      return;
    }
    const a = e.target.closest('[data-media]');
    if (!a) return;
    switch (a.dataset.media) {
      case 'back': this.onBack(); break;
      case 'retry': {
        const v = this.video;
        a.closest('.media-error')?.remove();
        if (!v || !this.info) break;
        // 보던 자리에서 다시 — 예전엔 처음(0:00)부터 재생해, 10초 뒤 이어 보기 위치까지 처음으로 덮었다.
        const at = Number.isFinite(v.currentTime) ? v.currentTime : 0;
        const rate = v.playbackRate || 1;
        // 다시보기는 재생 주소를 새로 받는다 — 주소에 서명이 붙어 있어, 오래 멈췄다가 만료되면 같은 주소로는 계속 실패했다.
        const seq = this.seq;
        if (this.kind === 'vod' && this.info.videoNo != null) {
          try {
            const fresh = await api.vodPlay(this.info.videoNo);
            if (seq !== this.seq) break;
            this.info = { ...this.info, ...fresh };
          } catch { /* 옛 주소로라도 다시 해 본다 */ }
        }
        // 고른 화질 그대로 — 예전엔 늘 '자동' 주소를 다시 넣어, 화질 칸은 1080p 인데 자동으로 재생됐다.
        const sel = this.body ? this.body.querySelector('[data-media-select="quality"]') : null;
        const q = sel && sel.value !== '' ? (this.info.qualities || [])[Number(sel.value)] : null;
        const src = (q && q.url) || this.info.streamUrl;
        if (!src) break;
        v.addEventListener('loadedmetadata', () => { if (at >= 1) v.currentTime = at; v.playbackRate = rate; }, { once: true });
        v.src = src; v.load(); v.play().catch(() => {});
        break;
      }
      case 'channel': this.onOpenChannel(a.dataset.channelId); break;
      case 'external': {
        const url = this.kind === 'vod'
          ? `https://chzzk.naver.com/video/${this.info.videoNo}`
          : `https://chzzk.naver.com/clips/${this.info.clipUid}`;
        api.openExternal(url).catch((er) => toast(er.message));
        break;
      }
      case 'download': {
        const i = this.info;
        if (this.kind === 'vod') startDownload('vod', i.videoNo, i.title, i.channelName);
        else startDownload('clip', i.clipUid, i.clipTitle, i.optionalProperty?.ownerChannel?.channelName || i.ownerChannel?.channelName || i.channel?.channelName || '');
        break;
      }
      case 'later': {
        const i = this.info;
        // 요청 중엔 다시 누르지 않게 — 예전엔 빠르게 두 번 누르면 둘 다 '담기'로 가 해제가 되지 않았다.
        if (a.disabled) break;
        const next = !i.watchLater;
        a.disabled = true;
        try {
          await api.vodWatchLaterSet(i.videoNo, next, i.title, i.channelName, i.thumbnailUrl || null, i.duration);
          i.watchLater = next;
          a.classList.toggle('primary', next);
          a.textContent = next ? '나중에 보기 ✓' : '나중에 보기';
        } catch (er) { toast(er.message); } finally { a.disabled = false; }
        break;
      }
      default: break;
    }
  }

  onChange(e) {
    const sel = e.target.closest('[data-media-select]');
    if (!sel) return;
    const v = this.video;
    if (!v) return;
    if (sel.dataset.mediaSelect === 'speed') {
      v.playbackRate = Number(sel.value) || 1;
    } else if (sel.dataset.mediaSelect === 'quality') {
      const q = sel.value === '' ? null : this.info.qualities[Number(sel.value)];
      // 오류 막을 걷는다 — 예전엔 다른 화질로 재생이 돼도 검은 오류 막이 영상을 덮은 채 남았다.
      v.closest('.media-stage')?.querySelector('.media-error')?.remove();
      const at = v.currentTime;
      const rate = v.playbackRate;
      // 첫 로드 중(메타데이터 전)에 바꾸면 위치를 옮기지 않는다 — 남아 있는 이어 보기 처리가 새 주소에서 위치를 맞춘다.
      // 예전엔 그 뒤에 0 으로 되돌려 이어 보기 위치를 잃고, 주기 저장이 처음부터 본 위치로 저장 값을 덮었다.
      const keepPosition = v.readyState >= 1;
      // 멈춘 채 화질을 바꾸면 멈춘 채로 — 예전엔 새 주소를 불러온 뒤 늘 재생했다.
      const wasPaused = keepPosition && v.paused;
      v.src = q ? q.url : this.info.streamUrl;
      v.addEventListener('loadedmetadata', () => { if (keepPosition) v.currentTime = at; v.playbackRate = rate; if (!wasPaused) v.play().catch(() => {}); }, { once: true });
    }
  }
}

/** 썸네일 URL(카드) — 이어 보기 카드용. */
export function continueCard(item) {
  const thumb = safeUrl(item.thumbnailUrl);
  const pct = item.duration > 0 ? Math.min(100, Math.round((item.position / item.duration) * 100)) : 0;
  return `
    <article class="card media-card" tabindex="0" data-vod-no="${esc(item.videoNo)}" title="${esc(item.title)}">
      <div class="thumb"${previewAttr('vod', item.videoNo)}>
        ${thumb ? `<img src="${esc(imageSrc(thumb, 'vod'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        <span class="badge-duration">${esc(formatDuration(item.duration))}</span>
        ${pct ? `<span class="progress-bar"><i style="width:${pct}%"></i></span>` : ''}
      </div>
      <div class="card-body"><div class="card-text">
        <div class="card-title">${esc(item.title || '제목 없음')}</div>
        <div class="card-meta ellipsis">${esc(item.channelName || '')}${item.position ? ` · ${esc(formatDuration(item.position))}까지 봄` : ''}</div>
      </div></div>
    </article>`;
}
