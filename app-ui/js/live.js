// 라이브(단독 시청) 메뉴 — 재생 중이면 시청 화면을, 아니면 이어 볼 채널을 보여 준다.
// 시청 화면 자체는 viewer.js 가 그리고, 이 페이지는 "재생 중인 방송이 없을 때" 화면이다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';
import { liveGrid, emptyState, errorState, skeletonGrid, recentChannels } from './cards.js';
import { liveIcon, liveModeSwitch } from './live-mode.js';
import { refreshIcon } from './page-header.js';
import { swapContent } from './motion.js';

export { liveIcon };

export class LivePage {
  /** `modeSwitch` — 보조 창(?w=player)에는 전환 버튼을 두지 않는다(이벤트를 메인 창에서만 건다). */
  constructor(content, { isLoggedIn, modeSwitch = true }) {
    this.isLoggedIn = isLoggedIn;
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'live';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">라이브</h1>
        ${modeSwitch ? liveModeSwitch('live') : ''}
        <div class="grow" data-tauri-drag-region></div>
        <button class="icon-btn round" data-action="refresh" title="새로고침" aria-label="새로고침">${refreshIcon}</button>
      </header>
      <div class="live-scroll">
        <p class="page-subtitle live-lead">볼 방송을 고르세요 — 최근 본 채널과 지금 방송 중인 채널</p>
        <div class="page-body"></div>
      </div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.root.querySelector('[data-action="refresh"]').addEventListener('click', () => this.show(true));
    this.seq = 0;
    this.loaded = false;
  }

  async show(refresh = false) {
    const seq = ++this.seq;
    // 처음만 스켈레톤(인기·팔로잉과 같게) — 다시 불러올 때는 보던 목록·스크롤·초점을 그대로 둔다.
    if (!this.loaded) this.body.innerHTML = skeletonGrid(8);
    const loggedIn = this.isLoggedIn();
    const [following, popular] = await Promise.allSettled([
      loggedIn ? api.followingLives(refresh) : Promise.resolve([]),
      api.popularLives(24, refresh),
    ]);
    if (seq !== this.seq) return;
    const followFailed = following.status === 'rejected';
    const popFailed = popular.status === 'rejected';
    // 불러올 수 있는 쪽이 모두 실패 — 빈 목록 대신 실패 화면과 "다시 시도"(인기 화면과 같게).
    if (popFailed && (followFailed || !loggedIn)) {
      this.loaded = false;
      this.body.innerHTML = errorState('불러오지 못했습니다', popular.reason);
      return;
    }
    this.loaded = true;
    const follow = followFailed ? [] : following.value || [];
    const pop = popFailed ? [] : popular.value || [];
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
          <span class="recent-meta ellipsis">${live ? `<b>LIVE</b> ${esc(live.viewerCountText)}명` : (followFailed || popFailed ? '방송 상태를 확인하지 못함' : '오프라인일 수 있음')}</span></span>
        </button>`;
      }).join('')}</div>`;
    }
    // 한쪽만 실패하면 그 섹션에 짧게 알린다. 로그아웃이면 팔로우 섹션 자리에 로그인 안내.
    const note = (text, action) => `<p class="live-note" role="status">${text}${action}</p>`;
    const retry = '<button class="link-btn" data-retry>다시 시도</button>';
    const followTitle = '<h2 class="section-title">방송 중인 팔로우 채널</h2>';
    if (!loggedIn) html += followTitle + note('로그인하면 팔로우한 채널의 방송이 여기에 나옵니다', '<button class="link-btn" data-action="login">로그인</button>');
    else if (followFailed) html += followTitle + note('팔로우 채널을 불러오지 못했습니다', retry);
    else if (follow.length) html += followTitle + liveGrid(follow.slice(0, 12));
    if (popFailed) html += `<h2 class="section-title">인기 라이브</h2>${note('인기 라이브를 불러오지 못했습니다', retry)}`;
    else if (pop.length) html += `<h2 class="section-title">인기 라이브</h2>${liveGrid(pop.slice(0, 12))}`;
    swapContent(this.body, html || emptyState('볼 수 있는 라이브가 없습니다'));
  }
}
