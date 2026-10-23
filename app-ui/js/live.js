// 라이브(단독 시청) 메뉴 — 재생 중이면 시청 화면을, 아니면 이어 볼 채널을 보여 준다.
// 시청 화면 자체는 viewer.js 가 그리고, 이 페이지는 "재생 중인 방송이 없을 때" 화면이다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';
import { liveGrid, emptyState, loadingState, recentChannels } from './cards.js';
import { liveIcon, liveModeSwitch } from './live-mode.js';
import { refreshIcon } from './page-header.js';

export { liveIcon };

export class LivePage {
  constructor(content, { isLoggedIn }) {
    this.isLoggedIn = isLoggedIn;
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'live';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">라이브</h1>
        ${liveModeSwitch('live')}
        <span class="page-subtitle">재생 중인 방송이 없습니다 — 이어 볼 채널을 고르세요</span>
        <div class="grow"></div>
        <button class="icon-btn round" data-action="refresh" title="새로고침" aria-label="새로고침">${refreshIcon}</button>
      </header>
      <div class="page-body"></div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.root.querySelector('[data-action="refresh"]').addEventListener('click', () => this.show(true));
    this.seq = 0;
  }

  async show(refresh = false) {
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    const [following, popular] = await Promise.allSettled([
      this.isLoggedIn() ? api.followingLives(refresh) : Promise.resolve([]),
      api.popularLives(24, refresh),
    ]);
    if (seq !== this.seq) return;
    const follow = following.status === 'fulfilled' ? following.value || [] : [];
    const pop = popular.status === 'fulfilled' ? popular.value || [] : [];
    const liveIds = new Map([...pop, ...follow].map((l) => [l.channelId, l]));
    const recent = recentChannels().slice(0, 6);
    let html = '';
    if (recent.length) {
      html += `<h2 class="section-title">최근 시청</h2><div class="recent-row">${recent.map((r) => {
        const live = liveIds.get(r.channelId);
        const avatar = safeUrl(r.channelImageUrl);
        return `<button class="recent-item${live ? ' is-live' : ''}" data-channel-id="${esc(r.channelId)}" data-channel-name="${esc(r.channelName)}" data-channel-image="${esc(avatar)}">
          ${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
          <span class="recent-text"><span class="ellipsis">${esc(r.channelName || r.channelId)}</span>
          <span class="recent-meta ellipsis">${live ? `<b>LIVE</b> ${esc(live.viewerCountText)}명` : '오프라인일 수 있음'}</span></span>
        </button>`;
      }).join('')}</div>`;
    }
    if (follow.length) html += `<h2 class="section-title">방송 중인 팔로잉</h2>${liveGrid(follow.slice(0, 12))}`;
    if (pop.length) html += `<h2 class="section-title">인기 라이브</h2>${liveGrid(pop.slice(0, 12))}`;
    this.body.innerHTML = html || emptyState('볼 수 있는 라이브가 없습니다');
  }
}
