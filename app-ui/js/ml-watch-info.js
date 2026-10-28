// 라이브 메뉴 단독 보기 — 영상 아래 채널 정보 · 다시보기 · 클립 · 커뮤니티.
// 치지직 웹의 방송 화면처럼 영상 바로 밑에 채널 카드가 보이고, 아래로 넘기면 탭이 이어진다.
// 무대 패인(iframe)은 옮기지 않는다 — 이 패널은 무대 옆(.ml-main 안 아래쪽)에 처음부터 있는 칸을 채운다.
//
// - 탭 막대는 무대 칸 위에 붙어 다니고, 채널 카드가 지나가면 작은 아바타·이름을 띄운다.
// - 목록은 끝에 다가가면 다음 쪽을 저절로 받는다(실패하면 "다시 시도" 단추).
// - 커뮤니티 사진은 앱 안 보기(라이트박스)로 크게 본다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc, toast, channelTone } from './dom.js';
import { compactCount } from './cards.js';
import { vodCard, clipCard, relativeDate } from './channel.js';

const TABS = [['info', '정보'], ['vod', '다시보기'], ['clip', '클립'], ['community', '커뮤니티']];
const svg = (d) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const ICON = {
  ext: svg('<path d="M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"/>'),
  home: svg('<path d="M4 11 12 4l8 7v9h-5v-6H9v6H4z"/>'),
  like: svg('<path d="M7 11v9H4v-9h3Zm0 0 4-7a2 2 0 0 1 3 2l-1 4h6a2 2 0 0 1 2 2.3l-1.2 6A2 2 0 0 1 17.8 20H7"/>'),
  comment: svg('<path d="M4 5h16v11H9l-5 4z"/>'),
  users: svg('<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0M16 4.5a3.5 3.5 0 0 1 0 7M18 14a6 6 0 0 1 3.5 6"/>'),
  eye: svg('<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>'),
  chart: svg('<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>'),
  clock: svg('<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>'),
  film: svg('<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M10 9.5v5l4-2.5z"/>'),
  clip: svg('<path d="M8 3h8l-1 6h4L9 21l2-8H6z"/>'),
  post: svg('<path d="M5 4h14v16H5z"/><path d="M8 9h8M8 13h8M8 17h5"/>'),
  close: svg('<path d="M6 6l12 12M18 6 6 18"/>'),
  prev: svg('<path d="M15 5l-7 7 7 7"/>'),
  next: svg('<path d="M9 5l7 7-7 7"/>'),
};
const ICON_VERIFIED = '<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="m12 2 2.4 1.8 3-.1 1 2.8 2.4 1.8-.9 2.9.9 2.9-2.4 1.8-1 2.8-3-.1L12 22l-2.4-1.8-3 .1-1-2.8-2.4-1.8.9-2.9-.9-2.9 2.4-1.8 1-2.8 3 .1Z"/><path fill="none" stroke="var(--mwi-check, #fff)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="m8.5 12.2 2.3 2.3 4.7-4.8"/></svg>';
const EMPTY = {
  vod: [ICON.film, '아직 다시보기가 없어요', '방송이 끝나면 여기에 다시보기가 올라옵니다'],
  clip: [ICON.clip, '아직 클립이 없어요', '시청자가 만든 클립이 여기에 모입니다'],
  community: [ICON.post, '커뮤니티 글이 없어요', '스트리머가 글을 올리면 여기에 보입니다'],
};

const chzzkUrl = (channelId, rest = '') => `https://chzzk.naver.com/${encodeURIComponent(channelId)}${rest}`;

/** `2026-10-26 07:12:33`(한국 시간) 또는 ISO → 밀리초. 못 읽으면 NaN. */
function parseKst(value) {
  if (!value) return NaN;
  const s = String(value);
  return Date.parse(/[zZ]|[+-]\d\d:?\d\d$/.test(s) ? s : `${s.replace(' ', 'T')}+09:00`);
}

/** 방송 경과 — `4시간 18분`. 못 읽으면 빈 글. */
export function liveDuration(openDate, now = Date.now()) {
  const t = parseKst(openDate);
  if (!Number.isFinite(t)) return '';
  const min = Math.max(0, Math.floor((now - t) / 60000));
  if (min < 60) return `${Math.max(1, min)}분`;
  const h = Math.floor(min / 60);
  return h < 48 ? `${h}시간 ${min % 60}분` : `${Math.floor(h / 24)}일`;
}

/** 절대 시각(밀리초) — 상대 시간에 마우스를 올리면 보이는 글. */
function absoluteTime(ms) {
  return Number.isFinite(ms)
    ? new Date(ms).toLocaleString('ko-KR', { year: 'numeric', month: 'long', day: 'numeric', weekday: 'short', hour: '2-digit', minute: '2-digit' })
    : '';
}

/** 글 본문 — 이스케이프 후 http(s) 주소만 바깥 브라우저로 여는 링크로. */
export function linkifyPost(text) {
  return esc(text).replace(/https?:\/\/[^\s<>"']+/g, (url) => {
    const clean = url.replace(/[).,!?]+$/, '');
    const tail = url.slice(clean.length);
    return `<a class="mwi-link" href="#" data-mwi-url="${clean}">${clean}</a>${tail}`;
  });
}

export class WatchInfoPanel {
  /** @param {HTMLElement} el 무대 아래 칸(.ml-watch-info) 또는 포커스 메인 아래 칸(.ml-focus-info, 스스로 스크롤) */
  constructor(el) {
    this.el = el;
    this.channelId = null;
    this.detail = null;
    this.tab = 'info';
    this.seq = 0;
    this.pages = { vod: 0, clip: 0, community: 0 };
    this.loading = false;
    this.postImages = new Map();
    this.observers = [];
    el.addEventListener('click', (e) => this.onClick(e));
    el.addEventListener('keydown', (e) => this.onTabKey(e));
  }

  get scroller() {
    return this.el.classList.contains('ml-focus-info') ? this.el : this.el.closest('.ml-main');
  }

  /** 단독 보기 채널이 바뀔 때(없으면 null) — 같은 채널이면 그대로 둔다. */
  show(channelId) {
    this.el.hidden = !channelId;
    if (channelId === this.channelId) return;
    this.channelId = channelId;
    this.detail = null;
    this.tab = 'info';
    this.disconnect();
    this.scroller?.scrollTo({ top: 0 });
    if (!channelId) { this.el.innerHTML = ''; return; }
    this.load();
  }

  /** 멀티라이브 상태의 시청자 수 — 카드는 처음 불러온 값이라 포커스 정보 칸에서는 상태가 올 때마다 맞춘다. */
  syncViewers(text) {
    const pill = this.detail?.live && text ? this.el.querySelector('.mwi-pill-live') : null;
    if (!pill) return;
    const html = `<i aria-hidden="true"></i>${esc(text)}명`;
    if (pill.innerHTML !== html) pill.innerHTML = html;
  }

  disconnect() {
    for (const o of this.observers) o.disconnect();
    this.observers = [];
  }

  async load() {
    const seq = ++this.seq;
    const id = this.channelId;
    this.el.innerHTML = `
      <div class="mwi-hero is-skeleton" aria-busy="true">
        <div class="mwi-avatar sk"></div>
        <div class="mwi-id"><span class="sk mwi-sk-line w40"></span><span class="sk mwi-sk-line w70"></span><span class="sk mwi-sk-line w30"></span></div>
      </div>`;
    try {
      const detail = await api.channelDetail(id);
      if (seq !== this.seq) return;
      this.detail = detail;
      this.render();
    } catch (e) {
      if (seq !== this.seq) return;
      this.el.innerHTML = this.failHtml('채널 정보를 불러오지 못했습니다', e, 'reload');
    }
  }

  render() {
    const { channel: c, live } = this.detail;
    const avatar = safeUrl(c.channelImageUrl);
    const duration = live ? liveDuration(live.openDate) : '';
    const avatarImg = (kind) => (avatar
      ? `<img src="${esc(imageSrc(avatar, kind))}" alt="" referrerpolicy="no-referrer">`
      : `<span>${esc((c.channelName || '?').slice(0, 1))}</span>`);
    const meta = live ? [
      live.categoryName ? `<span class="mwi-pill mwi-pill-cat">${esc(live.categoryName)}</span>` : '',
      `<span class="mwi-pill mwi-pill-live" title="지금 시청자"><i aria-hidden="true"></i>${esc(live.viewerCountText || '0')}명</span>`,
      duration ? `<span class="mwi-pill" title="방송 시작 ${esc(absoluteTime(parseKst(live.openDate)))}">${ICON.clock}${esc(duration)}</span>` : '',
    ].join('') : '<span class="mwi-pill">오프라인</span>';
    this.disconnect();
    this.el.innerHTML = `
      <header class="mwi-hero" style="--tone: ${channelTone(this.channelId)}">
        <div class="mwi-avatar${live ? ' live' : ''}">${avatarImg('avatar-lg')}${live ? '<b class="mwi-live-tag">LIVE</b>' : ''}</div>
        <div class="mwi-id">
          <div class="mwi-name">
            <h2 class="ellipsis" title="${esc(c.channelName)}">${esc(c.channelName)}</h2>
            ${c.verifiedMark ? `<span class="mwi-verified" title="인증된 채널">${ICON_VERIFIED}</span>` : ''}
            <span class="mwi-followers">팔로워 <b>${esc(compactCount(c.followerCount || 0))}</b></span>
          </div>
          ${live ? `<p class="mwi-title" title="${esc(live.liveTitle || '')}">${esc(live.liveTitle || '')}</p>` : ''}
          <div class="mwi-meta">${meta}</div>
        </div>
        <div class="mwi-actions">
          <button class="mwi-btn" data-mwi="channel" title="CView 채널 화면으로">${ICON.home}<span>채널 홈</span></button>
          <button class="mwi-btn icon" data-mwi="external" title="치지직에서 열기" aria-label="치지직에서 열기">${ICON.ext}</button>
        </div>
      </header>
      <nav class="mwi-tabbar">
        <div class="mwi-mini" aria-hidden="true">${avatarImg('avatar')}<strong class="ellipsis">${esc(c.channelName)}</strong></div>
        <div class="segmented mwi-tabs" role="tablist" aria-label="채널 정보">
          ${TABS.map(([id, label]) => `<button class="seg${this.tab === id ? ' active' : ''}" data-mwi-tab="${id}" role="tab" aria-selected="${this.tab === id}" aria-controls="mwi-panel" tabindex="${this.tab === id ? 0 : -1}">${label}<span class="mwi-count" data-count="${id}"></span></button>`).join('')}
        </div>
      </nav>
      <div class="mwi-body" id="mwi-panel" role="tabpanel"></div>`;
    this.watchStuck();
    this.renderTab();
  }

  /** 채널 카드가 위로 지나가면 탭 막대에 작은 아바타·이름을 띄운다. */
  watchStuck() {
    const hero = this.el.querySelector('.mwi-hero');
    const bar = this.el.querySelector('.mwi-tabbar');
    if (!hero || !bar || !this.scroller) return;
    const io = new IntersectionObserver(([e]) => {
      bar.classList.toggle('stuck', !e.isIntersecting && e.boundingClientRect.top < (e.rootBounds?.top ?? 0));
    }, { root: this.scroller, threshold: 0 });
    io.observe(hero);
    this.observers.push(io);
  }

  // ── 탭 ────────────────────────────────────

  selectTab(id, focus = false) {
    if (!TABS.some(([t]) => t === id)) return;
    const changed = id !== this.tab;
    this.tab = id;
    for (const b of this.el.querySelectorAll('[data-mwi-tab]')) {
      const on = b.dataset.mwiTab === id;
      b.classList.toggle('active', on);
      b.setAttribute('aria-selected', String(on));
      b.tabIndex = on ? 0 : -1;
      if (on && focus) b.focus();
    }
    if (!changed) return;
    // 이미 탭 막대를 지나왔으면 새 내용이 막대 바로 밑에서 시작되게 올린다
    const bar = this.el.querySelector('.mwi-tabbar');
    const sc = this.scroller;
    if (bar && sc && bar.classList.contains('stuck')) {
      const top = bar.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop;
      sc.scrollTo({ top, behavior: 'smooth' });
    }
    this.renderTab();
  }

  onTabKey(e) {
    const tab = e.target.closest?.('[data-mwi-tab]');
    if (!tab || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    e.stopPropagation(); // 단독 보기 단축키(←/→ 되감기)로 가지 않게
    const ids = TABS.map(([t]) => t);
    const i = ids.indexOf(tab.dataset.mwiTab);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? ids.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + ids.length) % ids.length;
    this.selectTab(ids[next], true);
  }

  async renderTab(append = false) {
    const box = this.el.querySelector('.mwi-body');
    if (!box || !this.detail) return;
    if (this.tab === 'info') {
      box.innerHTML = this.infoHtml();
      return;
    }
    const kind = this.tab;
    if (!append) {
      this.pages[kind] = 0;
      if (kind === 'community') this.postImages.clear();
      box.innerHTML = this.skeletonHtml(kind);
    }
    const seq = this.seq;
    const id = this.channelId;
    const page = this.pages[kind];
    this.loading = true;
    try {
      const res = kind === 'vod' ? await api.channelVods(id, page, 24)
        : kind === 'clip' ? await api.channelClips(id, page, 24)
        : await api.channelPosts(id, page);
      if (seq !== this.seq || this.tab !== kind) return;
      const badge = this.el.querySelector(`[data-count="${kind}"]`);
      if (badge && res.totalCount > 0) badge.textContent = compactCount(res.totalCount);
      const html = kind === 'community' ? res.items.map((p) => this.postHtml(p)).join('')
        : res.items.map(kind === 'vod' ? vodCard : clipCard).join('');
      if (!append) {
        const wrap = kind === 'community' ? 'mwi-posts' : `grid mwi-grid${kind === 'clip' ? ' clip-grid' : ''}`;
        box.innerHTML = res.items.length || res.hasMore ? `<div class="${wrap}">${html}</div>` : this.emptyHtml(kind);
      } else {
        box.querySelector('.mwi-grid, .mwi-posts')?.insertAdjacentHTML('beforeend', html);
      }
      box.querySelector('.mwi-tail')?.remove();
      if (res.hasMore) {
        box.insertAdjacentHTML('beforeend', '<div class="mwi-tail" role="status" aria-label="더 불러오는 중"><span class="mwi-dots" aria-hidden="true"><i></i><i></i><i></i></span></div>');
        this.watchTail(box.querySelector('.mwi-tail'));
      } else if (append) {
        box.insertAdjacentHTML('beforeend', '<div class="mwi-tail end">모두 봤어요</div>');
      }
      if (kind === 'community') requestAnimationFrame(() => this.markOverflow(box));
    } catch (e) {
      if (seq !== this.seq || this.tab !== kind) return;
      if (!append) box.innerHTML = this.failHtml('불러오지 못했습니다', e, 'retry-tab');
      else {
        this.pages[kind] -= 1;
        const tail = box.querySelector('.mwi-tail');
        if (tail) tail.outerHTML = '<div class="mwi-tail"><button class="mwi-btn" data-mwi="more">다시 시도</button></div>';
      }
    } finally {
      if (seq === this.seq) this.loading = false;
    }
  }

  /** 목록 끝이 가까워지면(600px 앞) 다음 쪽. */
  watchTail(tail) {
    if (!tail || !this.scroller) return;
    const io = new IntersectionObserver(([e]) => {
      if (!e.isIntersecting || this.loading) return;
      io.disconnect();
      this.pages[this.tab] += 1;
      this.renderTab(true);
    }, { root: this.scroller, rootMargin: '0px 0px 600px 0px' });
    io.observe(tail);
    this.observers.push(io);
  }

  /** 접힌 글 중 실제로 넘치는 것만 "더 보기"를 보인다(글자 수가 아니라 보이는 줄 수로). */
  markOverflow(box) {
    for (const t of box.querySelectorAll('.mwi-post-text.clamped:not([data-measured])')) {
      t.dataset.measured = '1';
      const more = t.nextElementSibling;
      if (more?.dataset.mwi === 'expand') more.hidden = t.scrollHeight <= t.clientHeight + 2;
    }
  }

  // ── 조각 ───────────────────────────────────

  infoHtml() {
    const { channel: c, live } = this.detail;
    const duration = live ? liveDuration(live.openDate) : '';
    const tiles = [
      [ICON.users, '팔로워', compactCount(c.followerCount || 0)],
      live && [ICON.eye, '지금 시청', compactCount(live.viewerCount || 0)],
      live && live.accumulateCount > 0 && [ICON.chart, '누적 시청', compactCount(live.accumulateCount)],
      duration && [ICON.clock, '방송 시간', duration],
    ].filter(Boolean);
    const tags = live ? (live.tags || []).filter(Boolean).slice(0, 12) : [];
    const desc = c.channelDescription;
    return `
      <div class="mwi-tiles">${tiles.map(([icon, k, v]) => `<div class="mwi-tile"><span class="mwi-tile-icon" aria-hidden="true">${icon}</span><span class="mwi-tile-text"><small>${esc(k)}</small><strong>${esc(v)}</strong></span></div>`).join('')}</div>
      <section class="mwi-card">
        <h3>채널 소개</h3>
        <p class="mwi-desc${desc ? '' : ' empty'}">${desc ? linkifyPost(desc) : '아직 채널 소개가 없습니다'}</p>
        ${tags.length ? `<div class="mwi-tags">${tags.map((t) => `<span>#${esc(t)}</span>`).join('')}</div>` : ''}
      </section>`;
  }

  skeletonHtml(kind) {
    if (kind === 'community') {
      const one = '<div class="mwi-post is-skeleton"><div class="mwi-post-head"><span class="sk avatar"></span><div class="mwi-post-who"><span class="sk mwi-sk-line w40"></span><span class="sk mwi-sk-line w20"></span></div></div><span class="sk mwi-sk-line w90"></span><span class="sk mwi-sk-line w70"></span><span class="sk mwi-sk-block"></span></div>';
      return `<div class="mwi-posts" aria-busy="true">${one.repeat(3)}</div>`;
    }
    const card = '<div class="sk-card"><div class="sk thumb-sk"></div><div class="sk line"></div><div class="sk line short"></div></div>';
    return `<div class="grid mwi-grid${kind === 'clip' ? ' clip-grid' : ''}" aria-busy="true">${card.repeat(kind === 'clip' ? 6 : 4)}</div>`;
  }

  emptyHtml(kind) {
    const [icon, title, sub] = EMPTY[kind];
    return `<div class="mwi-empty"><span class="mwi-empty-icon" aria-hidden="true">${icon}</span><strong>${esc(title)}</strong><span>${esc(sub)}</span></div>`;
  }

  failHtml(title, err, action) {
    const raw = String((err && err.message) || err || '');
    return `<div class="mwi-empty error" role="alert"><strong>${esc(title)}</strong>${raw ? `<span>${esc(raw)}</span>` : ''}<button class="mwi-btn" data-mwi="${action}">다시 시도</button></div>`;
  }

  postHtml(p) {
    const avatar = safeUrl(p.authorImageUrl);
    const color = /^#[0-9a-f]{3,8}$/i.test(p.authorTitleColor || '') ? p.authorTitleColor : '';
    const images = p.images.map(safeUrl).filter(Boolean);
    if (images.length) this.postImages.set(String(p.postId), images);
    const shown = images.slice(0, 4);
    return `
      <article class="mwi-post" data-post-id="${esc(p.postId)}">
        <header class="mwi-post-head">
          ${avatar ? `<img class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
          <div class="mwi-post-who">
            <div class="mwi-post-name">
              <strong class="ellipsis">${esc(p.authorName || '알 수 없음')}</strong>
              ${p.authorVerified ? `<span class="mwi-verified sm">${ICON_VERIFIED}</span>` : ''}
              ${p.authorTitle ? `<span class="mwi-post-title"${color ? ` style="--title-color: ${color}"` : ''}>${esc(p.authorTitle)}</span>` : ''}
            </div>
            <time datetime="${esc(p.createdAt || '')}" title="${esc(absoluteTime(Date.parse(p.createdAt || '')))}">${esc(relativeDate(p.createdAt))}</time>
          </div>
          <button class="mwi-btn icon ghost" data-mwi="post" title="치지직에서 글 보기" aria-label="치지직에서 글 보기">${ICON.ext}</button>
        </header>
        ${p.content ? `<p class="mwi-post-text clamped">${linkifyPost(p.content)}</p><button class="mwi-more-text" data-mwi="expand">더 보기</button>` : ''}
        ${shown.length ? `<div class="mwi-post-images n${shown.length}">${shown.map((u, i) => `<button class="mwi-post-img" data-mwi="image" data-index="${i}" aria-label="사진 ${i + 1} 크게 보기"><img src="${esc(imageSrc(u, 'static'))}" alt="" loading="lazy" referrerpolicy="no-referrer">${i === 3 && images.length > 4 ? `<b>+${images.length - 4}</b>` : ''}</button>`).join('')}</div>` : ''}
        <footer class="mwi-post-foot">
          <span class="mwi-react" title="추천 ${esc(p.likeCount)}">${ICON.like}${esc(compactCount(p.likeCount))}</span>
          <span class="mwi-react" title="댓글 ${esc(p.commentCount)}">${ICON.comment}${esc(compactCount(p.commentCount))}</span>
        </footer>
      </article>`;
  }

  // ── 사진 크게 보기 ─────────────────────────

  openLightbox(images, index) {
    document.querySelector('.mwi-lightbox')?.remove();
    const back = document.activeElement;
    const box = document.createElement('div');
    box.className = 'mwi-lightbox';
    box.setAttribute('role', 'dialog');
    box.setAttribute('aria-modal', 'true');
    box.setAttribute('aria-label', '사진 보기');
    const many = images.length > 1;
    box.innerHTML = `
      <img alt="" referrerpolicy="no-referrer">
      <button class="mwi-lb-btn close" data-lb="close" aria-label="닫기 (Esc)">${ICON.close}</button>
      ${many ? `<button class="mwi-lb-btn prev" data-lb="prev" aria-label="이전 사진">${ICON.prev}</button><button class="mwi-lb-btn next" data-lb="next" aria-label="다음 사진">${ICON.next}</button><span class="mwi-lb-count"></span>` : ''}`;
    const img = box.querySelector('img');
    const count = box.querySelector('.mwi-lb-count');
    let i = index;
    const show = () => {
      img.src = imageSrc(images[i], 'static');
      if (count) count.textContent = `${i + 1} / ${images.length}`;
    };
    const step = (d) => { i = (i + d + images.length) % images.length; show(); };
    // 단독 보기 단축키(Space·←/→·Esc)보다 먼저 받는다 — 캡처 단계
    const onKey = (e) => {
      const handled = e.key === 'Escape' || (many && (e.key === 'ArrowLeft' || e.key === 'ArrowRight'));
      if (!handled) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') close();
      else step(e.key === 'ArrowRight' ? 1 : -1);
    };
    function close() {
      box.classList.add('closing');
      document.removeEventListener('keydown', onKey, true);
      setTimeout(() => box.remove(), 160);
      back?.focus?.({ preventScroll: true });
    }
    box.addEventListener('click', (e) => {
      const b = e.target.closest('[data-lb]');
      if (b) {
        e.stopPropagation();
        if (b.dataset.lb === 'close') close();
        else step(b.dataset.lb === 'next' ? 1 : -1);
        return;
      }
      if (e.target === box) close();
    });
    document.addEventListener('keydown', onKey, true);
    document.body.append(box);
    show();
    box.querySelector('[data-lb="close"]').focus();
  }

  // ── 동작 ───────────────────────────────────

  async onClick(e) {
    const tab = e.target.closest('[data-mwi-tab]');
    if (tab) { this.selectTab(tab.dataset.mwiTab); return; }
    const link = e.target.closest('[data-mwi-url]');
    if (link) {
      e.preventDefault();
      api.openExternal(link.dataset.mwiUrl).catch((er) => toast(er.message));
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
    e.stopPropagation(); // 앱 전체 "다시 시도"(화면 통째로 다시 불러오기)까지 올라가지 않게
    const postId = action.closest('[data-post-id]')?.dataset.postId;
    switch (action.dataset.mwi) {
      case 'channel':
        window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: this.channelId } }));
        break;
      case 'external':
        api.openExternal(chzzkUrl(this.channelId)).catch((er) => toast(er.message));
        break;
      case 'post':
        if (postId) api.openExternal(chzzkUrl(this.channelId, `/community/detail/${encodeURIComponent(postId)}`)).catch((er) => toast(er.message));
        break;
      case 'image': {
        const images = this.postImages.get(postId);
        if (images) this.openLightbox(images, Number(action.dataset.index) || 0);
        break;
      }
      case 'expand': {
        const text = action.previousElementSibling;
        const clamped = text.classList.toggle('clamped');
        action.textContent = clamped ? '더 보기' : '접기';
        break;
      }
      case 'more':
        if (this.loading) break;
        action.closest('.mwi-tail')?.remove();
        this.pages[this.tab] += 1;
        this.renderTab(true);
        break;
      case 'retry-tab': this.renderTab(); break;
      case 'reload': this.load(); break;
      default: break;
    }
  }
}
