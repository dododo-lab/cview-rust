// 커맨드 팔레트(⌘K) — 원본 CommandPaletteView / CViewActionRegistry: 화면 이동·동작·채널을 한 곳에서
// 찾아 실행한다. 일치가 없으면 "검색" 으로 넘긴다 (NAV-12).
import { api } from './api.js';
import { esc, safeUrl, imageSrc, hideWithExit } from './dom.js';

/** 간단한 일치 점수(높을수록 앞) — 앞부분 일치 > 단어 시작 > 포함. 순수 함수. */
export function matchScore(text, query) {
  const t = String(text || '').toLowerCase();
  const q = String(query || '').trim().toLowerCase();
  if (!q) return 1;
  if (t.startsWith(q)) return 3;
  if (t.split(/\s+/).some((w) => w.startsWith(q))) return 2;
  return t.includes(q) ? 1 : 0;
}

export class CommandPalette {
  /**
   * @param {{ actions: () => Array<{id,label,hint?,group,run:Function}>, onOpenChannel: (id)=>void,
   *           onPlay: (id)=>void, onSearch: (q)=>void, isLoggedIn: () => boolean }} opts
   */
  constructor(opts) {
    this.opts = opts;
    this.el = document.createElement('div');
    this.el.className = 'palette-scrim';
    this.el.hidden = true;
    this.el.innerHTML = `
      <div class="palette" role="dialog" aria-label="명령 팔레트">
        <div class="palette-input"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg>
          <input type="text" placeholder="화면, 동작, 채널을 찾거나 검색어를 입력하세요" autocomplete="off" spellcheck="false"><kbd>esc</kbd></div>
        <div class="palette-list" role="listbox"></div>
      </div>`;
    document.body.append(this.el);
    this.input = this.el.querySelector('input');
    this.list = this.el.querySelector('.palette-list');
    this.items = [];
    this.index = 0;
    this.channels = [];
    this.input.addEventListener('input', () => this.render());
    this.input.addEventListener('keydown', (e) => this.onKey(e));
    this.el.addEventListener('mousedown', (e) => { if (e.target === this.el) this.close(); });
    this.list.addEventListener('click', (e) => {
      const row = e.target.closest('[data-i]');
      if (row) this.exec(Number(row.dataset.i));
    });
    this.list.addEventListener('mousemove', (e) => {
      const row = e.target.closest('[data-i]');
      if (row && Number(row.dataset.i) !== this.index) { this.index = Number(row.dataset.i); this.highlight(); }
    });
  }

  get isOpen() {
    return !this.el.hidden;
  }

  async open() {
    this.el.hidden = false;
    this.input.value = '';
    this.input.focus();
    this.render();
    // 채널 후보: 방송 중 팔로잉 + 즐겨찾기 + 최근 시청
    const [live, fav, recent] = await Promise.allSettled([
      this.opts.isLoggedIn() ? api.followingLives(false) : Promise.resolve([]),
      api.favoritesList(),
      api.recentChannels(20),
    ]);
    const map = new Map();
    for (const l of live.status === 'fulfilled' ? live.value : []) map.set(l.channelId, { id: l.channelId, name: l.channelName, image: l.channelImageUrl, live: true, sub: l.liveTitle });
    for (const c of [...(fav.status === 'fulfilled' ? fav.value : []), ...(recent.status === 'fulfilled' ? recent.value : [])]) {
      if (!map.has(c.channelId)) map.set(c.channelId, { id: c.channelId, name: c.channelName, image: c.channelImageUrl, live: false, sub: c.isFavorite ? '즐겨찾기' : '최근 시청' });
    }
    this.channels = [...map.values()];
    if (this.isOpen) this.render();
  }

  close() {
    hideWithExit(this.el);
  }

  render() {
    const q = this.input.value.trim();
    const actions = this.opts.actions()
      .map((a) => ({ ...a, score: Math.max(matchScore(a.label, q), matchScore(a.hint, q) - 1) }))
      .filter((a) => a.score > 0);
    const channels = this.channels
      .map((c) => ({ c, score: matchScore(c.name, q) }))
      .filter((x) => x.score > 0)
      .sort((a, b) => (b.score - a.score) || (b.c.live - a.c.live))
      .slice(0, 8)
      .map(({ c }) => ({
        group: '채널', label: c.name, hint: c.sub || '', image: c.image, live: c.live,
        run: () => (c.live ? this.opts.onPlay(c.id) : this.opts.onOpenChannel(c.id)),
      }));
    const items = [...actions.sort((a, b) => b.score - a.score).slice(0, q ? 8 : 14), ...channels];
    if (q) items.push({ group: '검색', label: `"${q}" 검색`, hint: '라이브·채널·다시보기', run: () => this.opts.onSearch(q) });
    this.items = items;
    this.index = 0;
    let lastGroup = '';
    this.list.innerHTML = items.map((it, i) => {
      const head = it.group !== lastGroup ? `<div class="palette-group">${esc(it.group)}</div>` : '';
      lastGroup = it.group;
      const img = safeUrl(it.image);
      return `${head}<div class="palette-item${i === 0 ? ' active' : ''}" role="option" data-i="${i}">
        ${it.group === '채널' ? (img ? `<img class="avatar" src="${esc(imageSrc(img, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>') : ''}
        <span class="palette-label">${esc(it.label)}${it.live ? ' <b class="live-dot">LIVE</b>' : ''}</span>
        <span class="palette-hint ellipsis">${esc(it.hint || '')}</span></div>`;
    }).join('') || '<div class="tray-empty">일치하는 항목이 없습니다</div>';
  }

  highlight() {
    for (const el of this.list.querySelectorAll('.palette-item')) el.classList.toggle('active', Number(el.dataset.i) === this.index);
    this.list.querySelector('.palette-item.active')?.scrollIntoView({ block: 'nearest' });
  }

  onKey(e) {
    if (e.key === 'Escape') { e.preventDefault(); this.close(); return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!this.items.length) return;
      this.index = (this.index + (e.key === 'ArrowDown' ? 1 : -1) + this.items.length) % this.items.length;
      this.highlight();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      this.exec(this.index);
    }
  }

  exec(i) {
    const it = this.items[i];
    if (!it) return;
    this.close();
    it.run();
  }
}
