// 라이브 카드 격자 / 채널 행 / 빈 상태.
import { esc, safeUrl } from './dom.js';

/** 라이브 카드 한 장 (LiveCardDto). */
export function liveCard(live) {
  const thumb = safeUrl(live.thumbnailUrl);
  const avatar = safeUrl(live.channelImageUrl);
  const uptime = uptimeText(live.openDate);
  const category = live.categoryName ? `<span class="category"> · ${esc(live.categoryName)}</span>` : '';
  return `
    <article class="card" tabindex="0" data-channel-id="${esc(live.channelId)}" data-channel-name="${esc(live.channelName)}" data-channel-image="${esc(avatar)}" title="${esc(live.liveTitle)}">
      <div class="thumb">
        ${thumb ? `<img src="${esc(thumb)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        ${live.isLive ? '<span class="badge-live">LIVE</span>' : ''}
        ${live.adult ? '<span class="badge-adult">19</span>' : ''}
        <span class="badge-viewers"><i class="dot"></i>${esc(live.viewerCountText)}명</span>
        ${live.isLive ? '<button class="card-ml-add" data-ml-add title="멀티라이브에 추가" aria-label="멀티라이브에 추가"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 6v12M6 12h12"/></svg></button>' : ''}
        ${uptime ? `<span class="badge-uptime">${esc(uptime)}</span>` : ''}
      </div>
      <div class="card-body">
        <span class="card-avatar" data-open-channel title="채널 정보">${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<div class="avatar"></div>'}</span>
        <div class="card-text">
          <div class="card-title">${esc(live.liveTitle || '제목 없음')}</div>
          <div class="card-meta ellipsis">${esc(live.channelName)}${category}</div>
        </div>
      </div>
    </article>`;
}

export function liveGrid(lives) {
  return `<div class="grid">${lives.map(liveCard).join('')}</div>`;
}

/** 채널 행 (ChannelDto). */
export function channelRow(channel) {
  const avatar = safeUrl(channel.channelImageUrl);
  return `
    <div class="channel-row card" tabindex="0" data-channel-id="${esc(channel.channelId)}" data-channel-name="${esc(channel.channelName)}" data-channel-image="${esc(avatar)}" data-open-live="${channel.openLive ? '1' : ''}">
      ${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<div class="avatar"></div>'}
      <div class="card-text">
        <div class="ellipsis"><strong>${esc(channel.channelName)}</strong>${channel.verified ? '<span class="verified">✔</span>' : ''}${channel.openLive ? '<span class="live-dot">LIVE</span>' : ''}</div>
        <div class="card-meta">팔로워 ${esc(channel.followerCountText)}명</div>
      </div>
    </div>`;
}

export function emptyState(title, detail = '', action = '') {
  return `<div class="empty"><strong>${esc(title)}</strong>${detail ? `<span>${esc(detail)}</span>` : ''}${action}</div>`;
}

export function loadingState() {
  return '<div class="empty"><div class="spinner"></div></div>';
}

/** 방송 경과 시간 — `openDate`(RFC3339) 기준 "2시간 5분" / "35분". 알 수 없으면 ''. */
export function uptimeText(openDate, now = Date.now()) {
  const t = Date.parse(openDate || '');
  if (!Number.isFinite(t)) return '';
  const min = Math.floor((now - t) / 60000);
  if (min < 1) return '방금 시작';
  if (min < 60) return `${min}분`;
  const h = Math.floor(min / 60);
  return `${h}시간${min % 60 ? ` ${min % 60}분` : ''}`;
}

/** 한국어 수 축약 — 12,345 → "1.2만" (Rust `format_viewer_count` 근사 — 합계 표시용). */
export function compactCount(n) {
  if (n >= 10000) return `${(Math.floor(n / 1000) / 10).toFixed(1).replace(/\.0$/, '')}만`;
  if (n >= 1000) return `${(Math.floor(n / 100) / 10).toFixed(1).replace(/\.0$/, '')}천`;
  return String(n);
}

// ── 최근 시청 (로컬) ─────────────────────────────────────
const RECENT_KEY = 'cview.recentChannels';
const RECENT_MAX = 16;

export function recentChannels() {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
    return Array.isArray(list) ? list.filter((r) => r && r.channelId) : [];
  } catch {
    return [];
  }
}

export function rememberChannel({ channelId, channelName = '', channelImage = '' }) {
  if (!channelId) return;
  const entry = { channelId, channelName, channelImageUrl: safeUrl(channelImage), at: Date.now() };
  const list = [entry, ...recentChannels().filter((r) => r.channelId !== channelId)].slice(0, RECENT_MAX);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch { /* 저장 불가 — 무시 */ }
}

/** 클릭/엔터로 채널을 연다. */
export function bindCardOpen(root, onOpen) {
  const open = (el) => {
    const card = el.closest('[data-channel-id]');
    if (card && el.closest('[data-open-channel]')) {
      window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: card.dataset.channelId } }));
      return;
    }
    if (!card) return;
    rememberChannel(card.dataset);
    onOpen(card.dataset.channelId, card.dataset);
  };
  root.addEventListener('click', (e) => open(e.target));
  root.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') open(e.target);
  });
}
