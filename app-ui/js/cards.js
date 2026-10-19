// 라이브 카드 격자 / 채널 행 / 빈 상태.
import { esc, safeUrl } from './dom.js';

/** 라이브 카드 한 장 (LiveCardDto). */
export function liveCard(live) {
  const thumb = safeUrl(live.thumbnailUrl);
  const avatar = safeUrl(live.channelImageUrl);
  const category = live.categoryName ? `<span class="category"> · ${esc(live.categoryName)}</span>` : '';
  return `
    <article class="card" tabindex="0" data-channel-id="${esc(live.channelId)}" title="${esc(live.liveTitle)}">
      <div class="thumb">
        ${thumb ? `<img src="${esc(thumb)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}
        ${live.isLive ? '<span class="badge-live">LIVE</span>' : ''}
        ${live.adult ? '<span class="badge-adult">19</span>' : ''}
        <span class="badge-viewers">${esc(live.viewerCountText)}명 시청</span>
      </div>
      <div class="card-body">
        ${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<div class="avatar"></div>'}
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
    <div class="channel-row card" tabindex="0" data-channel-id="${esc(channel.channelId)}" data-open-live="${channel.openLive ? '1' : ''}">
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

/** 클릭/엔터로 채널을 연다. */
export function bindCardOpen(root, onOpen) {
  const open = (el) => {
    const card = el.closest('[data-channel-id]');
    if (card) onOpen(card.dataset.channelId, card.dataset);
  };
  root.addEventListener('click', (e) => open(e.target));
  root.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') open(e.target);
  });
}
