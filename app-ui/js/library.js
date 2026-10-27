// 보관함 — 원본 LibraryHubView / PopularClipsView / VODBrowseView / RecentFavoritesView / 북마크 목록.
// VOD-04 인기 클립(기간·정렬), VOD-01 팔로우 채널 다시보기·이어 보기·나중에 보기, NAV-08 허브,
// NAV-09 즐겨찾기·최근 시청(DB, 지우기), PLY-17 북마크 목록·삭제.
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { emptyState, loadingState, errorState } from './cards.js';
import { vodCard, clipCard, relativeDate, formatDuration } from './channel.js';
import { continueCard } from './media.js';
import { mountDownloads } from './downloads.js';
import { syncTabs, bindTabKeys } from './a11y.js';

export const libraryIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 4h4v16H4zM10 4h4v16h-4zM16.2 4.4l3.8-1 4 15.5-3.8 1z" transform="scale(.92) translate(1 1)"/></svg>';

const TABS = [
  ['clips', '인기 클립'],
  ['vods', '팔로우 다시보기'],
  ['continue', '이어 보기'],
  ['later', '나중에 보기'],
  ['favorites', '즐겨찾기'],
  ['recent', '최근 시청'],
  ['bookmarks', '북마크'],
  ['downloads', '다운로드'],
];
const CLIP_FILTERS = [['WITHIN_1_DAY', '오늘'], ['WITHIN_7_DAYS', '7일'], ['WITHIN_30_DAYS', '30일']];
const CLIP_ORDERS = [['POPULAR', '인기순'], ['RECOMMEND', '추천순']];

function channelRowSaved(c) {
  const avatar = safeUrl(c.channelImageUrl);
  return `
    <button class="recent-item" data-open-channel-id="${esc(c.channelId)}">
      ${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
      <span class="recent-text"><span class="ellipsis">${c.isFavorite ? '★ ' : ''}${esc(c.channelName || c.channelId)}</span>
      <span class="recent-meta ellipsis">${c.lastWatched ? `${esc(relativeDate(new Date(c.lastWatched).toISOString()))} 시청` : '즐겨찾기'}</span></span>
    </button>`;
}

function bookmarkRow(b) {
  const thumb = safeUrl(b.thumbnailUrl);
  return `
    <div class="bookmark-row" data-bookmark-id="${esc(b.id)}">
      <span class="bookmark-thumb">${thumb ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(thumb, 'live'))}" alt="" referrerpolicy="no-referrer">` : ''}</span>
      <span class="bookmark-text">
        <strong class="ellipsis">${esc(b.liveTitle || '제목 없음')}</strong>
        <span class="ellipsis">${esc(b.channelName)}${b.categoryName ? ` · ${esc(b.categoryName)}` : ''} · 방송 ${esc(formatDuration(b.liveUptime))} 지점 · ${esc(relativeDate(new Date(b.capturedAt).toISOString()))}</span>
      </span>
      <button class="btn ghost" data-lib="bookmark-channel" data-channel-id="${esc(b.channelId)}">채널</button>
      <button class="icon-btn" data-lib="bookmark-delete" title="삭제">✕</button>
    </div>`;
}

export class LibraryPage {
  constructor(content, { isLoggedIn, onOpenChannel }) {
    this.isLoggedIn = isLoggedIn;
    this.onOpenChannel = onOpenChannel;
    this.acceptsParams = true;
    this.tab = 'clips';
    this.clipFilter = 'WITHIN_7_DAYS';
    this.clipOrder = 'POPULAR';
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'library';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">보관함</h1>
        <span class="page-subtitle">클립 · 다시보기 · 즐겨찾기 · 북마크</span>
      </header>
      <div class="chips lib-tabs" role="tablist" aria-label="보관함">${TABS.map(([id, label]) => `<button class="chip" data-tab="${id}" role="tab">${label}</button>`).join('')}</div>
      <div class="lib-filters"></div>
      <div class="page-body"></div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.filters = this.root.querySelector('.lib-filters');
    this.tabs = this.root.querySelector('.lib-tabs');
    this.seq = 0;
    bindTabKeys(this.tabs);
    this.root.addEventListener('click', (e) => this.onClick(e));
  }

  show(params = {}) {
    if (params.tab) this.tab = params.tab;
    syncTabs(this.tabs, this.tab, this.body, 'lib');
    this.load();
  }

  renderFilters() {
    if (this.tab !== 'clips') {
      this.filters.innerHTML = this.tab === 'recent'
        ? '<button class="btn ghost" data-lib="recent-clear">최근 시청 기록 지우기</button>' : '';
      return;
    }
    this.filters.innerHTML = `
      <div class="segmented">${CLIP_FILTERS.map(([v, l]) => `<button class="seg${v === this.clipFilter ? ' active' : ''}" data-clip-filter="${v}">${l}</button>`).join('')}</div>
      <div class="segmented">${CLIP_ORDERS.map(([v, l]) => `<button class="seg${v === this.clipOrder ? ' active' : ''}" data-clip-order="${v}">${l}</button>`).join('')}</div>`;
  }

  async load() {
    const seq = ++this.seq;
    if (this.unmountDownloads) { this.unmountDownloads(); this.unmountDownloads = null; }
    this.renderFilters();
    this.body.innerHTML = loadingState();
    const needsLogin = this.tab === 'vods';
    if (needsLogin && !this.isLoggedIn()) {
      this.body.innerHTML = emptyState('로그인이 필요합니다', '팔로우한 채널의 다시보기를 모아 보여 드립니다', '<button class="btn primary" data-action="login">로그인</button>');
      return;
    }
    let html = '';
    try {
      switch (this.tab) {
        case 'clips': {
          const list = await api.popularClips(this.clipFilter, this.clipOrder);
          html = list.length ? `<div class="grid clip-grid">${list.map(clipCard).join('')}</div>` : emptyState('클립이 없습니다');
          break;
        }
        case 'vods': {
          const list = await api.followingVods(24, 3);
          html = list.length ? `<div class="grid">${list.map(vodCard).join('')}</div>` : emptyState('최근 다시보기가 없습니다');
          break;
        }
        case 'continue': {
          const list = await api.vodContinueList(40);
          html = list.length ? `<div class="grid">${list.map(continueCard).join('')}</div>` : emptyState('이어 볼 다시보기가 없습니다', '다시보기를 보다 멈추면 여기에 남습니다');
          break;
        }
        case 'later': {
          const list = await api.vodWatchLaterList();
          html = list.length ? `<div class="grid">${list.map(continueCard).join('')}</div>` : emptyState('나중에 볼 다시보기가 없습니다', '다시보기 화면의 "나중에 보기"로 담을 수 있어요');
          break;
        }
        case 'favorites': {
          const list = await api.favoritesList();
          html = list.length ? `<div class="recent-row">${list.map(channelRowSaved).join('')}</div>` : emptyState('즐겨찾기한 채널이 없습니다', '채널 화면의 ☆ 버튼으로 추가할 수 있어요');
          break;
        }
        case 'recent': {
          const list = await api.recentChannels(60);
          html = list.length ? `<div class="recent-row">${list.map(channelRowSaved).join('')}</div>` : emptyState('최근 시청 기록이 없습니다');
          break;
        }
        case 'downloads': {
          const box = document.createElement('div');
          box.className = 'dl-box';
          this.body.innerHTML = '';
          this.body.append(box);
          const unmount = await mountDownloads(box);
          if (seq !== this.seq) { unmount(); return; } // 붙이는 사이 다른 탭으로 — 남은 구독을 바로 푼다
          this.unmountDownloads = unmount;
          return;
        }
        case 'bookmarks': {
          const list = await api.bookmarksList();
          html = list.length ? `<div class="bookmark-list">${list.map(bookmarkRow).join('')}</div>` : emptyState('북마크가 없습니다', '시청 중 북마크 버튼(또는 B 키)으로 지금 장면을 남길 수 있어요');
          break;
        }
        default: break;
      }
    } catch (e) {
      html = errorState('불러오지 못했습니다', e);
    }
    if (seq === this.seq) this.body.innerHTML = html;
  }

  async onClick(e) {
    const tab = e.target.closest('[data-tab]');
    if (tab) {
      this.tab = tab.dataset.tab;
      this.show();
      return;
    }
    const f = e.target.closest('[data-clip-filter]');
    if (f) { this.clipFilter = f.dataset.clipFilter; this.load(); return; }
    const o = e.target.closest('[data-clip-order]');
    if (o) { this.clipOrder = o.dataset.clipOrder; this.load(); return; }
    const vod = e.target.closest('[data-vod-no]');
    if (vod) { window.dispatchEvent(new CustomEvent('cview:open-vod', { detail: { videoNo: Number(vod.dataset.vodNo) } })); return; }
    const clip = e.target.closest('[data-clip-uid]');
    if (clip) { window.dispatchEvent(new CustomEvent('cview:open-clip', { detail: { clipUid: clip.dataset.clipUid } })); return; }
    const ch = e.target.closest('[data-open-channel-id]');
    if (ch) { this.onOpenChannel(ch.dataset.openChannelId); return; }
    const a = e.target.closest('[data-lib]');
    if (!a) return;
    if (a.dataset.lib === 'recent-clear') {
      // 시청 기록 전체 삭제는 되돌릴 수 없어 한 번 더 눌러야 지운다.
      if (a.dataset.armed !== '1') {
        const label = a.textContent;
        a.dataset.armed = '1';
        a.textContent = '한 번 더 누르면 모두 지웁니다';
        setTimeout(() => { if (a.dataset.armed === '1') { a.dataset.armed = ''; a.textContent = label; } }, 3000);
        return;
      }
      a.dataset.armed = '';
      try { await api.recentChannelsClear(); toast('최근 시청 기록을 지웠습니다', 1500); this.load(); } catch (er) { toast(er.message); }
    } else if (a.dataset.lib === 'bookmark-delete') {
      // 바로 지우지 않고 숨긴 뒤, 되돌리기 시간이 지나면 지운다.
      const row = a.closest('[data-bookmark-id]');
      row.hidden = true;
      let undone = false;
      toast('북마크를 지웠습니다', 5000, { label: '되돌리기', run: () => { undone = true; row.hidden = false; } });
      setTimeout(async () => {
        if (undone) return;
        try { await api.bookmarkDelete(row.dataset.bookmarkId); row.remove(); } catch (er) { row.hidden = false; toast(er.message); }
      }, 5200);
    } else if (a.dataset.lib === 'bookmark-channel') {
      this.onOpenChannel(a.dataset.channelId);
    }
  }
}
