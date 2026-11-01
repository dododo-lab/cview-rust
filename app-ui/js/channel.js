// 채널 상세 — 원본 ChannelInfoView / ChannelInfoHeaderView / ChannelVODClipTab / ChannelMemoSheet.
// NAV-01 헤더·방송 중 배지·정보/다시보기/클립 탭, VOD-05 채널 VOD·클립 탭, DAT-01 메모, DAT-02 즐겨찾기.
import { api } from './api.js';
import { esc, safeUrl, toast, channelTone, imageSrc, dismiss } from './dom.js';
import { emptyState, loadingState, compactCount, errorState } from './cards.js';
import { previewAttr } from './hover-preview.js';
import { verifiedBadge } from './verified-badge.js';

const ICON_STAR = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"/></svg>';
const ICON_STAR_LINE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"/></svg>';
const ICON_NOTE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M5 4h10l4 4v12H5z"/><path d="M8 12h8M8 16h5"/></svg>';
const ICON_EXT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"/></svg>';
const ICON_BACK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M15 5l-7 7 7 7"/></svg>';

/** 초 → `1:02:03` / `12:34`. */
export function formatDuration(sec) {
  const s = Math.max(0, Math.floor(Number(sec) || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${r}` : `${m}:${r}`;
}

/** ISO 날짜 → `3일 전` 같은 상대 시간. */
export function relativeDate(iso, now = Date.now()) {
  const t = Date.parse(iso || '');
  if (!Number.isFinite(t)) return '';
  const min = Math.max(0, Math.floor((now - t) / 60000));
  if (min < 60) return `${Math.max(1, min)}분 전`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h}시간 전`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d}일 전`;
  const mo = Math.floor(d / 30);
  return mo < 12 ? `${mo}개월 전` : `${Math.floor(mo / 12)}년 전`;
}

/** 다시보기 카드. */
export function vodCard(v) {
  const thumb = safeUrl(v.thumbnailImageUrl || v.videoImageUrl);
  return `
    <article class="card media-card" tabindex="0" data-vod-no="${esc(v.videoNo)}" title="${esc(v.videoTitle)}">
      <div class="thumb"${v.adult ? '' : previewAttr('vod', v.videoNo)}>
        ${thumb ? `<img src="${esc(imageSrc(thumb, 'vod'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        <span class="badge-duration">${esc(formatDuration(v.duration))}</span>
      </div>
      <div class="card-body"><div class="card-text">
        <div class="card-title">${esc(v.videoTitle || '제목 없음')}</div>
        <div class="card-meta ellipsis">${v.channel ? `${esc(v.channel.channelName)} · ` : ''}조회 ${esc(compactCount(v.readCount || 0))} · ${esc(relativeDate(v.publishDate))}</div>
      </div></div>
    </article>`;
}

/** 클립 카드(세로 썸네일). */
export function clipCard(c) {
  const thumb = safeUrl(c.thumbnailImageUrl);
  return `
    <article class="card media-card clip" tabindex="0" data-clip-uid="${esc(c.clipUID || c.clipUid)}" title="${esc(c.clipTitle)}">
      <div class="thumb"${c.adult ? '' : previewAttr('clip', c.clipUID || c.clipUid)}>
        ${thumb ? `<img src="${esc(imageSrc(thumb, 'clip'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        <span class="badge-duration">${esc(formatDuration(c.duration))}</span>
      </div>
      <div class="card-body"><div class="card-text">
        <div class="card-title">${esc(c.clipTitle || '제목 없음')}</div>
        <div class="card-meta ellipsis">${c.channel ? `${esc(c.channel.channelName)} · ` : ''}조회 ${esc(compactCount(c.readCount || 0))}</div>
      </div></div>
    </article>`;
}

export class ChannelPage {
  constructor(content, { onBack, onPlay, onAddMulti }) {
    this.onBack = onBack;
    this.onPlay = onPlay;
    this.onAddMulti = onAddMulti;
    this.root = document.createElement('section');
    this.acceptsParams = true;
    this.root.className = 'page page-channel';
    this.root.dataset.page = 'channel';
    this.root.hidden = true;
    this.root.innerHTML = '<div class="page-body"></div>';
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.channelId = null;
    this.tab = 'info';
    this.detail = null;
    this.seq = 0;
    this.pages = { vod: 0, clip: 0 };
    this.body.addEventListener('click', (e) => this.onClick(e));
  }

  async show(params = {}) {
    if (!params.channelId) return;
    const changed = params.channelId !== this.channelId;
    this.channelId = params.channelId;
    if (changed) this.tab = params.tab || 'info';
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    try {
      const detail = await api.channelDetail(this.channelId);
      if (seq !== this.seq) return;
      this.detail = detail;
      this.render();
    } catch (e) {
      if (seq !== this.seq) return;
      this.body.innerHTML = errorState('채널 정보를 불러오지 못했습니다', e);
    }
  }

  render() {
    const d = this.detail;
    const c = d.channel;
    const avatar = safeUrl(c.channelImageUrl);
    const live = d.live;
    this.body.innerHTML = `
      <button class="back-btn" data-ch="back">${ICON_BACK}<span>뒤로</span></button>
      <header class="channel-hero" style="--tone: ${channelTone(this.channelId)}">
        <div class="channel-hero-bg">${avatar ? `<img src="${esc(imageSrc(avatar, 'avatar-lg'))}" alt="" referrerpolicy="no-referrer">` : ''}</div>
        <div class="channel-hero-main">
          <div class="channel-avatar${live ? ' live' : ''}">
            ${avatar ? `<img class="avatar" src="${esc(imageSrc(avatar, 'avatar-lg'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
            ${live ? '<span class="strip-live">LIVE</span>' : ''}
          </div>
          <div class="channel-id">
            <h1>${esc(c.channelName)}${verifiedBadge(c, 'lg')}</h1>
            <p>팔로워 ${esc(compactCount(c.followerCount || 0))}명${live ? ` · <b>${esc(live.viewerCountText)}명 시청 중</b>` : ''}</p>
          </div>
          <div class="grow"></div>
          <div class="channel-actions">
            ${live ? '<button class="btn primary lg" data-ch="play">▶ 라이브 보기</button><button class="btn" data-ch="multi">멀티라이브에 추가</button>' : ''}
            <button class="icon-btn round fav${d.isFavorite ? ' on' : ''}" data-ch="favorite" title="${d.isFavorite ? '즐겨찾기 해제' : '즐겨찾기'}">${d.isFavorite ? ICON_STAR : ICON_STAR_LINE}</button>
            <button class="icon-btn round" data-ch="memo" title="메모">${ICON_NOTE}</button>
            <button class="icon-btn round" data-ch="notify" title="이 채널 알림 설정">🔔</button>
            <button class="icon-btn round" data-ch="external" title="치지직에서 열기">${ICON_EXT}</button>
          </div>
        </div>
        ${live ? `<div class="channel-live-line"><span class="badge-live">LIVE</span> ${esc(live.liveTitle || '')}${live.categoryName ? ` · <span>${esc(live.categoryName)}</span>` : ''}</div>` : ''}
      </header>
      <div class="chips channel-tabs" role="tablist">
        ${[['info', '정보'], ['vod', '다시보기'], ['clip', '클립']].map(([id, label]) => `<button class="chip${this.tab === id ? ' active' : ''}" data-tab="${id}" role="tab">${label}</button>`).join('')}
      </div>
      <div class="channel-tab-body"></div>`;
    this.renderTab();
  }

  async renderTab(append = false) {
    const box = this.body.querySelector('.channel-tab-body');
    if (!box) return;
    const d = this.detail;
    if (this.tab === 'info') {
      const desc = d.channel.channelDescription;
      box.innerHTML = `
        <section class="settings-group channel-info">
          <h3>채널 소개</h3>
          <p class="channel-desc">${desc ? esc(desc) : '소개가 없습니다'}</p>
        </section>
        <section class="settings-group channel-info">
          <h3>내 메모</h3>
          <p class="channel-desc">${d.memo ? esc(d.memo) : '메모가 없습니다 — 오른쪽 위 메모 버튼으로 남길 수 있어요'}</p>
        </section>`;
      return;
    }
    const kind = this.tab;
    if (!append) {
      this.pages[kind] = 0;
      box.innerHTML = loadingState();
    }
    const seq = this.seq;
    try {
      const page = kind === 'vod'
        ? await api.channelVods(this.channelId, this.pages[kind], 24)
        : await api.channelClips(this.channelId, this.pages[kind], 24);
      if (seq !== this.seq || this.tab !== kind) return;
      const cards = page.items.map(kind === 'vod' ? vodCard : clipCard).join('');
      if (!append) {
        box.innerHTML = page.items.length
          ? `<div class="grid ${kind === 'clip' ? 'clip-grid' : ''}">${cards}</div>`
          : emptyState(kind === 'vod' ? '다시보기가 없습니다' : '클립이 없습니다');
      } else {
        box.querySelector('.grid').insertAdjacentHTML('beforeend', cards);
      }
      box.querySelector('.load-more')?.remove();
      if (page.hasMore) box.insertAdjacentHTML('beforeend', '<button class="btn load-more" data-ch="more">더 보기</button>');
    } catch (e) {
      if (seq !== this.seq || this.tab !== kind) return; // 그사이 다른 탭 — 그 탭을 오류로 덮지 않는다
      if (!append) box.innerHTML = errorState('불러오지 못했습니다', e);
      else { this.pages[kind] -= 1; toast(e.message); } // 다음 "더 보기"가 같은 쪽을 다시 받게
    }
  }

  async onClick(e) {
    const tab = e.target.closest('[data-tab]');
    if (tab) {
      this.tab = tab.dataset.tab;
      for (const b of this.body.querySelectorAll('[data-tab]')) b.classList.toggle('active', b === tab);
      this.renderTab();
      return;
    }
    const vod = e.target.closest('[data-vod-no]');
    if (vod) {
      window.dispatchEvent(new CustomEvent('cview:open-vod', { detail: { videoNo: Number(vod.dataset.vodNo) } }));
      return;
    }
    const clip = e.target.closest('[data-clip-uid]');
    if (clip) {
      window.dispatchEvent(new CustomEvent('cview:open-clip', { detail: { clipUid: clip.dataset.clipUid } }));
      return;
    }
    const action = e.target.closest('[data-ch]');
    if (!action) return;
    const c = this.detail && this.detail.channel;
    switch (action.dataset.ch) {
      case 'back': this.onBack(); break;
      case 'play': this.onPlay(this.channelId); break;
      case 'multi': this.onAddMulti(this.channelId); break;
      case 'external': api.openExternal(`https://chzzk.naver.com/${encodeURIComponent(this.channelId)}`).catch((er) => toast(er.message)); break;
      case 'favorite': {
        const next = !this.detail.isFavorite;
        try {
          await api.favoriteSet(this.channelId, next, c.channelName, c.channelImageUrl || null);
          this.detail.isFavorite = next;
          toast(next ? '즐겨찾기에 추가했습니다' : '즐겨찾기에서 뺐습니다', 1500);
          this.render();
        } catch (er) { toast(er.message); }
        break;
      }
      case 'memo': this.editMemo(); break;
      case 'notify': this.editNotify(action); break;
      case 'more':
        // 받는 동안 두 번 눌러 쪽이 건너뛰거나 순서가 섞이지 않게
        if (action.disabled) break;
        action.disabled = true;
        this.pages[this.tab] += 1;
        await this.renderTab(true);
        if (action.isConnected) action.disabled = false;
        break;
      default: break;
    }
  }

  /** 채널별 알림 (SET-13) — 방송 시작·카테고리·제목 변경. 전역 스위치는 설정 › 일반. */
  async editNotify(anchor) {
    document.querySelector('.notify-pop')?.remove();
    let all = {};
    try { all = (await api.settingsGet()).channelNotifications?.settings || {}; } catch { /* 기본값 */ }
    const id = this.channelId;
    const name = this.detail?.channelName || this.detail?.channel?.channelName || id;
    const cur = all[id] || { channelId: id, channelName: name, notifyOnLive: true, notifyOnCategoryChange: true, notifyOnTitleChange: true };
    const pop = document.createElement('div');
    pop.className = 'notify-pop';
    const row = (k, l) => `<label class="setting"><span class="setting-label">${l}</span><span class="toggle"><input type="checkbox" data-nk="${k}" ${cur[k] ? 'checked' : ''}><span></span></span></label>`;
    pop.innerHTML = `<strong>${esc(name)} 알림</strong>${row('notifyOnLive', '방송 시작')}${row('notifyOnCategoryChange', '카테고리 변경')}${row('notifyOnTitleChange', '제목 변경')}<small class="muted">팔로우한 채널만 · 전체 켜기/끄기는 설정 › 일반</small>`;
    document.body.append(pop);
    const r = anchor.getBoundingClientRect();
    pop.style.top = `${r.bottom + 8}px`;
    pop.style.left = `${Math.max(12, Math.min(window.innerWidth - 292, r.right - 280))}px`;
    pop.addEventListener('change', async (e) => {
      const k = e.target.dataset.nk;
      if (!k) return;
      cur[k] = e.target.checked;
      cur.channelName = name;
      try {
        const fresh = (await api.settingsGet()).channelNotifications?.settings || {};
        await api.settingsUpdate('channelNotifications', { settings: { ...fresh, [id]: cur } });
      } catch (er) { toast(er.message); }
    });
    setTimeout(() => document.addEventListener('pointerdown', function off(ev) {
      if (!pop.contains(ev.target)) { pop.remove(); document.removeEventListener('pointerdown', off); }
    }), 0);
  }

  editMemo() {
    const dialog = document.createElement('div');
    dialog.className = 'sheet-scrim';
    dialog.innerHTML = `
      <form class="sheet" role="dialog" aria-label="채널 메모">
        <h2>${esc(this.detail.channel.channelName)} 메모</h2>
        <textarea maxlength="500" placeholder="이 채널에 대해 기억할 것을 적어 두세요">${esc(this.detail.memo || '')}</textarea>
        <div class="sheet-actions"><button type="button" class="btn ghost" data-sheet="cancel">취소</button><button class="btn primary">저장</button></div>
      </form>`;
    document.body.append(dialog);
    const area = dialog.querySelector('textarea');
    area.focus();
    const close = () => dismiss(dialog);
    dialog.addEventListener('click', (e) => { if (e.target === dialog || e.target.closest('[data-sheet="cancel"]')) close(); });
    dialog.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
    dialog.querySelector('form').addEventListener('submit', async (e) => {
      e.preventDefault();
      try {
        await api.memoSet(this.channelId, area.value);
        this.detail.memo = area.value.trim();
        toast('메모를 저장했습니다', 1400);
        close();
        if (this.tab === 'info') this.renderTab();
      } catch (er) { toast(er.message); }
    });
  }
}
