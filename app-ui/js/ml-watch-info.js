// 라이브 메뉴 단독 보기 — 영상 아래 채널 정보 · 다시보기 · 클립 · 커뮤니티.
// 치지직 웹의 방송 화면처럼 영상 바로 밑에 채널 줄이 보이고, 아래로 넘기면 탭이 이어진다.
// 무대 패인(iframe)은 옮기지 않는다 — 이 패널은 무대 옆(.ml-main 안 아래쪽)에 처음부터 있는 칸을 채운다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc, toast } from './dom.js';
import { emptyState, loadingState, errorState, compactCount } from './cards.js';
import { vodCard, clipCard, relativeDate } from './channel.js';

const TABS = [['info', '정보'], ['vod', '다시보기'], ['clip', '클립'], ['community', '커뮤니티']];
const ICON_EXT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"/></svg>';
const ICON_LIKE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M7 11v9H4v-9h3Zm0 0 4-7a2 2 0 0 1 3 2l-1 4h6a2 2 0 0 1 2 2.3l-1.2 6A2 2 0 0 1 17.8 20H7"/></svg>';
const ICON_COMMENT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>';
/** 이보다 긴 글은 접어 두고 "더 보기"로 편다. */
const POST_CLAMP_CHARS = 280;

const chzzkUrl = (channelId, rest = '') => `https://chzzk.naver.com/${encodeURIComponent(channelId)}${rest}`;

export class WatchInfoPanel {
  /** @param {HTMLElement} el 무대 아래 칸(.ml-watch-info) */
  constructor(el) {
    this.el = el;
    this.channelId = null;
    this.detail = null;
    this.tab = 'info';
    this.seq = 0;
    this.pages = { vod: 0, clip: 0, community: 0 };
    el.addEventListener('click', (e) => this.onClick(e));
  }

  /** 단독 보기 채널이 바뀔 때(없으면 null) — 같은 채널이면 그대로 둔다. */
  show(channelId) {
    this.el.hidden = !channelId;
    if (channelId === this.channelId) return;
    this.channelId = channelId;
    this.detail = null;
    this.tab = 'info';
    this.el.closest('.ml-main')?.scrollTo({ top: 0 });
    if (!channelId) { this.el.innerHTML = ''; return; }
    this.load();
  }

  async load() {
    const seq = ++this.seq;
    const id = this.channelId;
    this.el.innerHTML = loadingState();
    try {
      const detail = await api.channelDetail(id);
      if (seq !== this.seq) return;
      this.detail = detail;
      this.render();
    } catch (e) {
      if (seq !== this.seq) return;
      this.el.innerHTML = errorState('채널 정보를 불러오지 못했습니다', e);
    }
  }

  render() {
    const { channel: c, live } = this.detail;
    const avatar = safeUrl(c.channelImageUrl);
    const liveLine = live
      ? `<p class="mwi-live ellipsis" title="${esc(live.liveTitle || '')}">${esc(live.liveTitle || '')}</p>
         <p class="mwi-sub ellipsis">${[
           live.categoryName ? `<span class="mwi-cat">${esc(live.categoryName)}</span>` : '',
           live.viewerCountText ? `${esc(live.viewerCountText)}명 시청` : '',
           `팔로워 ${esc(compactCount(c.followerCount || 0))}명`,
         ].filter(Boolean).join('<i class="sep">·</i>')}</p>`
      : `<p class="mwi-sub">팔로워 ${esc(compactCount(c.followerCount || 0))}명</p>`;
    this.el.innerHTML = `
      <header class="mwi-head">
        ${avatar ? `<img class="avatar${live ? ' live' : ''}" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
        <div class="mwi-id">
          <h2 class="ellipsis">${esc(c.channelName)}${c.verifiedMark ? '<span class="verified" title="인증된 채널">✔</span>' : ''}</h2>
          ${liveLine}
        </div>
        <div class="mwi-actions">
          <button class="btn" data-mwi="channel">채널 보기</button>
          <button class="icon-btn round" data-mwi="external" title="치지직에서 열기" aria-label="치지직에서 열기">${ICON_EXT}</button>
        </div>
      </header>
      <div class="chips mwi-tabs" role="tablist" aria-label="채널 정보">
        ${TABS.map(([id, label]) => `<button class="chip${this.tab === id ? ' active' : ''}" data-mwi-tab="${id}" role="tab" aria-selected="${this.tab === id}">${label}</button>`).join('')}
      </div>
      <div class="mwi-body" role="tabpanel"></div>`;
    this.renderTab();
  }

  async renderTab(append = false) {
    const box = this.el.querySelector('.mwi-body');
    if (!box || !this.detail) return;
    if (this.tab === 'info') {
      const desc = this.detail.channel.channelDescription;
      box.innerHTML = `<p class="mwi-desc">${desc ? esc(desc) : '채널 소개가 없습니다'}</p>`;
      return;
    }
    const kind = this.tab;
    if (!append) {
      this.pages[kind] = 0;
      box.innerHTML = loadingState();
    }
    const seq = this.seq;
    const id = this.channelId;
    const page = this.pages[kind];
    try {
      const res = kind === 'vod' ? await api.channelVods(id, page, 24)
        : kind === 'clip' ? await api.channelClips(id, page, 24)
        : await api.channelPosts(id, page);
      if (seq !== this.seq || this.tab !== kind) return;
      const html = kind === 'community' ? res.items.map((p) => this.postHtml(p)).join('')
        : res.items.map(kind === 'vod' ? vodCard : clipCard).join('');
      if (!append) {
        const wrap = kind === 'community' ? 'mwi-posts' : `grid${kind === 'clip' ? ' clip-grid' : ''}`;
        box.innerHTML = res.items.length || res.hasMore
          ? `<div class="${wrap}">${html}</div>`
          : emptyState({ vod: '다시보기가 없습니다', clip: '클립이 없습니다', community: '커뮤니티 글이 없습니다' }[kind]);
      } else {
        box.querySelector('.grid, .mwi-posts')?.insertAdjacentHTML('beforeend', html);
      }
      box.querySelector('.load-more')?.remove();
      if (res.hasMore) box.insertAdjacentHTML('beforeend', '<button class="btn load-more" data-mwi="more">더 보기</button>');
    } catch (e) {
      if (seq !== this.seq || this.tab !== kind) return;
      if (!append) box.innerHTML = errorState('불러오지 못했습니다', e);
      else { this.pages[kind] -= 1; toast(e.message); }
    }
  }

  postHtml(p) {
    const avatar = safeUrl(p.authorImageUrl);
    const long = p.content.length > POST_CLAMP_CHARS;
    const color = /^#[0-9a-f]{3,8}$/i.test(p.authorTitleColor || '') ? p.authorTitleColor : '';
    const images = p.images.map(safeUrl).filter(Boolean).slice(0, 4);
    return `
      <article class="mwi-post" data-post-id="${esc(p.postId)}">
        <header class="mwi-post-head">
          ${avatar ? `<img class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
          <strong class="ellipsis">${esc(p.authorName || '알 수 없음')}</strong>
          ${p.authorVerified ? '<span class="verified">✔</span>' : ''}
          ${p.authorTitle ? `<span class="mwi-post-title"${color ? ` style="--title-color: ${color}"` : ''}>${esc(p.authorTitle)}</span>` : ''}
          <time>${esc(relativeDate(p.createdAt))}</time>
        </header>
        ${p.content ? `<p class="mwi-post-text${long ? ' clamped' : ''}">${esc(p.content)}</p>${long ? '<button class="link-btn" data-mwi="expand">더 보기</button>' : ''}` : ''}
        ${images.length ? `<div class="mwi-post-images n${images.length}">${images.map((u) => `<button class="mwi-post-img" data-mwi="post" title="치지직에서 글 보기"><img src="${esc(imageSrc(u, 'static'))}" alt="" loading="lazy" referrerpolicy="no-referrer"></button>`).join('')}</div>` : ''}
        <footer class="mwi-post-foot">
          <span title="추천">${ICON_LIKE}${esc(compactCount(p.likeCount))}</span>
          <span title="댓글">${ICON_COMMENT}${esc(compactCount(p.commentCount))}</span>
          <button class="link-btn" data-mwi="post">치지직에서 보기</button>
        </footer>
      </article>`;
  }

  async onClick(e) {
    const tab = e.target.closest('[data-mwi-tab]');
    if (tab) {
      if (tab.dataset.mwiTab === this.tab) return;
      this.tab = tab.dataset.mwiTab;
      for (const b of this.el.querySelectorAll('[data-mwi-tab]')) {
        const on = b === tab;
        b.classList.toggle('active', on);
        b.setAttribute('aria-selected', String(on));
      }
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
    const action = e.target.closest('[data-mwi]');
    if (!action || !this.channelId) return;
    switch (action.dataset.mwi) {
      case 'channel':
        window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: this.channelId } }));
        break;
      case 'external':
        api.openExternal(chzzkUrl(this.channelId)).catch((er) => toast(er.message));
        break;
      case 'post': {
        const id = action.closest('[data-post-id]')?.dataset.postId;
        if (id) api.openExternal(chzzkUrl(this.channelId, `/community/detail/${encodeURIComponent(id)}`)).catch((er) => toast(er.message));
        break;
      }
      case 'expand': {
        const text = action.previousElementSibling;
        const clamped = text.classList.toggle('clamped');
        action.textContent = clamped ? '더 보기' : '접기';
        break;
      }
      case 'more':
        // 받는 동안 두 번 눌러 쪽이 건너뛰지 않게
        if (action.disabled) break;
        action.disabled = true;
        this.pages[this.tab] += 1;
        await this.renderTab(true);
        if (action.isConnected) action.disabled = false;
        break;
      default: break;
    }
  }
}
