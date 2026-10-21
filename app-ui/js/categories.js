// 카테고리 둘러보기 — 원본 CategoryBrowseView: 지금 방송 중인 라이브를 카테고리로 묶어 카드로 보이고,
// 고르면 그 카테고리 라이브만 본다. 자주 보는 카테고리는 고정한다 (NAV-02).
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';
import { liveGrid, emptyState, loadingState, compactCount, skeletonGrid, errorState } from './cards.js';
import { categoryGroups, arrangeLives } from './pages.js';

export const categoryIcon = '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="3" width="8" height="8" rx="2.5"/><rect x="13" y="3" width="8" height="8" rx="2.5" opacity=".6"/><rect x="3" y="13" width="8" height="8" rx="2.5" opacity=".6"/><rect x="13" y="13" width="8" height="8" rx="2.5"/></svg>';
const PIN_KEY = 'cview.pinnedCategories';

function pinned() {
  try { return JSON.parse(localStorage.getItem(PIN_KEY) || '[]'); } catch { return []; }
}

export class CategoriesPage {
  constructor(content) {
    this.acceptsParams = true;
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'categories';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">카테고리</h1>
        <span class="page-subtitle cat-sub">지금 방송 중인 카테고리</span>
        <div class="grow"></div>
        <button class="btn" data-action="refresh">새로고침</button>
      </header>
      <div class="page-body"></div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.sub = this.root.querySelector('.cat-sub');
    this.lives = [];
    this.category = '';
    this.seq = 0;
    this.root.querySelector('[data-action="refresh"]').addEventListener('click', () => this.show({ refresh: true }));
    this.body.addEventListener('click', (e) => {
      const pin = e.target.closest('[data-pin]');
      if (pin) {
        e.stopPropagation();
        const list = new Set(pinned());
        if (list.has(pin.dataset.pin)) list.delete(pin.dataset.pin); else list.add(pin.dataset.pin);
        try { localStorage.setItem(PIN_KEY, JSON.stringify([...list])); } catch { /* 무시 */ }
        this.render();
        return;
      }
      const c = e.target.closest('[data-category]');
      if (c) { this.category = c.dataset.category; this.render(); return; }
      if (e.target.closest('[data-cat-back]')) { this.category = ''; this.render(); }
    });
  }

  async show(params = {}) {
    if (params.category !== undefined) this.category = params.category;
    const seq = ++this.seq;
    if (!this.lives.length || params.refresh) this.body.innerHTML = skeletonGrid(12);
    try {
      const lives = await api.popularLives(300, Boolean(params.refresh));
      if (seq !== this.seq) return;
      this.lives = lives;
      this.render();
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('불러오지 못했습니다', e);
    }
  }

  render() {
    if (this.category) {
      const list = arrangeLives(this.lives, { category: this.category });
      this.sub.textContent = `${this.category} · 라이브 ${list.length}개`;
      this.body.innerHTML = `<button class="back-btn" data-cat-back>‹ 모든 카테고리</button>${list.length ? liveGrid(list) : emptyState('지금 이 카테고리 방송이 없습니다')}`;
      return;
    }
    const pins = new Set(pinned());
    const groups = categoryGroups(this.lives).sort((a, b) => (pins.has(b.name) - pins.has(a.name)) || (b.viewers - a.viewers));
    this.sub.textContent = `지금 방송 중인 카테고리 ${groups.length}개 · 상위 라이브 ${this.lives.length}개 기준`;
    this.body.innerHTML = groups.length ? `<div class="category-grid">${groups.map((g) => {
      const thumb = safeUrl(g.thumb);
      return `<button class="category-card" data-category="${esc(g.name)}">
        <span class="category-art">${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}</span>
        <span class="category-info"><strong class="ellipsis">${esc(g.name)}</strong><span>라이브 ${g.count} · ${esc(compactCount(g.viewers))}명</span></span>
        <span class="category-pin${pins.has(g.name) ? ' on' : ''}" data-pin="${esc(g.name)}" title="${pins.has(g.name) ? '고정 해제' : '위에 고정'}">★</span>
      </button>`;
    }).join('')}</div>` : emptyState('카테고리가 없습니다');
  }
}
