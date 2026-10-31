// 다시보기·클립 플레이어 — 원본 VODPlayerView / ClipPlayerView / ClipFilmstripDock.
// VOD-02 VOD 플레이어(시크·속도·화질·이어 보기), VOD-03 클립 플레이어(다음 클립 독).
// 다시보기는 WebView 기본 HLS(<video src=m3u8>)로 재생한다 — CDN CORS 를 피하고 macOS 기본 컨트롤·PiP 를
// 쓴다. 클립은 원본처럼 치지직 임베드 페이지가 주 경로다(대부분 ABR_HLS 라 인키 재생이 막힌다).
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { emptyState, loadingState, compactCount, errorState } from './cards.js';
import { formatDuration, relativeDate, clipCard } from './channel.js';
import { startDownload } from './downloads.js';
import { previewAttr } from './hover-preview.js';

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
    this.body.addEventListener('change', (e) => this.onChange(e));
    // 화면을 떠나면 멈추고 위치를 저장한다.
    new MutationObserver(() => { if (this.root.hidden) this.leave(); })
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
    const info = await api.vodPlay(videoNo);
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
                ${info.channelId ? `<button class="link-btn" data-media="channel" data-channel-id="${esc(info.channelId)}">${esc(info.channelName)}</button> · ` : `${esc(info.channelName)} · `}
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
    if (info.resumeAt > 0) {
      video.addEventListener('loadedmetadata', () => { video.currentTime = info.resumeAt; }, { once: true });
      toast(`${formatDuration(info.resumeAt)}부터 이어서 재생합니다`, 2200);
    }
    video.addEventListener('error', () => {
      if (seq === this.seq) toast('영상을 불러오지 못했습니다');
    });
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
                ${channelId ? `<button class="link-btn" data-media="channel" data-channel-id="${esc(channelId)}">${esc(channelName || '채널')}</button> · ` : ''}조회 ${esc(compactCount(d.readCount || 0))} · ${esc(formatDuration(d.duration))}
              </div>
            </div>
            <div class="grow"></div>
            <button class="btn" data-media="download" title="클립을 파일로 받기">받기</button>
            <button class="btn ghost" data-media="external">치지직에서 열기</button>
          </div>
        </div>
        <aside class="media-side"><h2 class="section-title">다른 클립</h2><div class="media-dock">${loadingState()}</div></aside>
      </div>`;
    this.loadDock(channelId, clipUid, seq);
  }

  async loadDock(channelId, currentUid, seq) {
    const dock = this.body.querySelector('.media-dock');
    try {
      const list = channelId
        ? (await api.channelClips(channelId, 0, 20)).items
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
    api.vodProgressSave(info.videoNo, v.currentTime, v.duration || info.duration, info.title, info.channelName, info.thumbnailUrl || null)
      .catch(() => {});
  }

  leave() {
    clearInterval(this.saveTimer);
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
        const next = !i.watchLater;
        try {
          await api.vodWatchLaterSet(i.videoNo, next, i.title, i.channelName, i.thumbnailUrl || null, i.duration);
          i.watchLater = next;
          a.classList.toggle('primary', next);
          a.textContent = next ? '나중에 보기 ✓' : '나중에 보기';
        } catch (er) { toast(er.message); }
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
      const at = v.currentTime;
      const rate = v.playbackRate;
      v.src = q ? q.url : this.info.streamUrl;
      v.addEventListener('loadedmetadata', () => { v.currentTime = at; v.playbackRate = rate; v.play().catch(() => {}); }, { once: true });
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
        ${thumb ? `<img src="${esc(imageSrc(thumb, 'static'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        <span class="badge-duration">${esc(formatDuration(item.duration))}</span>
        ${pct ? `<span class="progress-bar"><i style="width:${pct}%"></i></span>` : ''}
      </div>
      <div class="card-body"><div class="card-text">
        <div class="card-title">${esc(item.title || '제목 없음')}</div>
        <div class="card-meta ellipsis">${esc(item.channelName || '')}${item.position ? ` · ${esc(formatDuration(item.position))}까지 봄` : ''}</div>
      </div></div>
    </article>`;
}
