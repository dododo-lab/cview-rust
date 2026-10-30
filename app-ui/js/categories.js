// 카테고리 둘러보기 — 원본 CategoryBrowseView: 지금 방송 중인 라이브를 카테고리로 묶어 카드로 보이고,
// 고르면 그 카테고리 라이브만 본다. 자주 보는 카테고리는 고정한다 (NAV-02).
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';
import { liveGrid, emptyState, loadingState, compactCount, skeletonGrid, errorState } from './cards.js';
import { categoryGroups, arrangeLives } from './pages.js';
import { refreshIcon } from './page-header.js';
import { swapContent } from './motion.js';

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
        <button class="icon-btn round" data-action="refresh" title="새로고침" aria-label="새로고침">${refreshIcon}</button>
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
        const name = pin.dataset.pin;
        this.render();
        // 다시 그린 뒤에도 같은 별에 초점을 둔다(키보드로 고정·해제를 이어 할 수 있게)
        if (document.activeElement === document.body || !document.activeElement?.isConnected) {
          [...this.body.querySelectorAll('[data-pin]')].find((b) => b.dataset.pin === name)?.focus({ preventScroll: true });
        }
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
    // 목록 ↔ 한 카테고리를 오갈 때만 떠오르고, 같은 화면을 다시 그릴 때(자동 새로고침·고정)는 그대로
    const view = this.category || '';
    const enter = view !== this.view;
    this.view = view;
    if (this.category) {
      const list = arrangeLives(this.lives, { category: this.category });
      this.sub.textContent = `${this.category} · 라이브 ${list.length}개`;
      swapContent(this.body, `<button class="back-btn" data-cat-back>‹ 모든 카테고리</button>${list.length ? liveGrid(list) : emptyState('지금 이 카테고리 방송이 없습니다')}`, enter);
      return;
    }
    const pins = new Set(pinned());
    const groups = categoryGroups(this.lives).sort((a, b) => (pins.has(b.name) - pins.has(a.name)) || (b.viewers - a.viewers));
    this.sub.textContent = `지금 방송 중인 카테고리 ${groups.length}개 · 상위 라이브 ${this.lives.length}개 기준`;
    swapContent(this.body, groups.length ? `<div class="category-grid">${groups.map((g) => {
      const thumb = safeUrl(g.thumb);
      const on = pins.has(g.name);
      // 고정 별은 카드(상세로 이동)와 형제인 독립 단추 — 안에 두면 초점을 받지 못하고 Enter 가 상세로 갔다
      return `<div class="category-item">
        <button class="category-card" data-category="${esc(g.name)}">
          <span class="category-art">${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}</span>
          <span class="category-info"><strong class="ellipsis">${esc(g.name)}</strong><span>라이브 ${g.count} · ${esc(compactCount(g.viewers))}명</span></span>
        </button>
        <button class="category-pin${on ? ' on' : ''}" data-pin="${esc(g.name)}" aria-pressed="${on}" aria-label="${esc(g.name)} ${on ? '고정 해제' : '위에 고정'}" title="${on ? '고정 해제' : '위에 고정'}">★</button>
      </div>`;
    }).join('')}</div>` : emptyState('카테고리가 없습니다'), enter);
  }
}
