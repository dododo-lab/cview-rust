// 통합 검색 추천어 (NAV-06) — 입력하는 동안 최근 검색어와 내 채널(즐겨찾기·최근 시청·팔로우)에서 맞는 것을 보인다.
// 원본 SearchState.autocomplete_suggestions 처럼 기기 안 목록만 쓴다(검색 서버에 글자마다 묻지 않는다).
// ↑/↓ 로 고르고 Enter 로 연다. 고르지 않은 Enter 는 입력한 그대로 검색한다. Esc·초점을 잃으면 닫는다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc } from './dom.js';

export const SUGGEST_LIMIT = 8;

/** 비교용 — 대소문자·앞뒤·사이 공백을 무시한다. */
const norm = (s) => String(s || '').toLowerCase().replace(/\s+/g, '');

/**
 * 추천어 목록 — 순수 함수(테스트 가능). 앞에서부터 맞는 것이 먼저, 같은 이름은 한 번만.
 * recent: 최근 검색어(문자열), channels: { id, name, image, live, sub }. 반환: { kind: 'query' | 'channel', ... }
 */
export function buildSuggestions(query, { recent = [], channels = [] } = {}, limit = SUGGEST_LIMIT) {
  const q = norm(query);
  if (!q) return [];
  const rank = (text) => {
    const t = norm(text);
    if (!t) return -1;
    if (t === q) return 0;
    if (t.startsWith(q)) return 1;
    return t.includes(q) ? 2 : -1;
  };
  const out = [];
  const seenQuery = new Set([q]); // 입력한 그대로는 추천하지 않는다(Enter 와 같다)
  const queries = recent
    .map((r, i) => ({ r, i, k: rank(r) }))
    .filter((x) => x.k > 0 && !seenQuery.has(norm(x.r)))
    .sort((a, b) => a.k - b.k || a.i - b.i);
  for (const { r } of queries) {
    if (seenQuery.has(norm(r))) continue;
    seenQuery.add(norm(r));
    out.push({ kind: 'query', text: r });
  }
  const seenChannel = new Set();
  const chans = channels
    .map((c, i) => ({ c, i, k: rank(c.name) }))
    .filter((x) => x.k >= 0 && x.c.id)
    .sort((a, b) => a.k - b.k || Number(Boolean(b.c.live)) - Number(Boolean(a.c.live)) || a.i - b.i);
  const picked = [];
  for (const { c } of chans) {
    if (seenChannel.has(c.id)) continue;
    seenChannel.add(c.id);
    picked.push({ kind: 'channel', id: c.id, text: c.name, image: c.image || '', live: Boolean(c.live), sub: c.sub || '' });
  }
  // 채널이 먼저 보이도록 검색어는 절반까지만(채널이 적으면 나머지를 검색어로 채운다).
  const half = Math.ceil(limit / 2);
  const qs = out.slice(0, Math.max(half, limit - picked.length));
  return [...picked.slice(0, limit - qs.length), ...qs].slice(0, limit);
}

/** 내 채널 후보 — 즐겨찾기·최근 시청(기기 안)과 팔로우(로그인했을 때, 공유 캐시). */
async function loadChannels(isLoggedIn) {
  const [fav, recent, follow] = await Promise.allSettled([
    api.favoritesList(),
    api.recentChannels(30),
    isLoggedIn() ? api.followingChannelsAll() : Promise.resolve([]),
  ]);
  const map = new Map();
  const add = (c, sub) => {
    if (!c || !c.channelId || map.has(c.channelId)) return;
    map.set(c.channelId, { id: c.channelId, name: c.channelName, image: c.channelImageUrl, live: Boolean(c.openLive || c.isLive), sub });
  };
  for (const c of fav.status === 'fulfilled' ? fav.value || [] : []) add(c, '즐겨찾기');
  for (const c of follow.status === 'fulfilled' ? follow.value || [] : []) add(c, '팔로우');
  for (const c of recent.status === 'fulfilled' ? recent.value || [] : []) add(c, '최근 시청');
  return [...map.values()];
}

/** 검색 입력란에 붙는 추천어 목록. */
export class SearchSuggest {
  constructor(input, { recent, isLoggedIn = () => false, onQuery, onChannel }) {
    this.input = input;
    this.recent = recent;
    this.isLoggedIn = isLoggedIn;
    this.onQuery = onQuery;
    this.onChannel = onChannel;
    this.items = [];
    this.index = -1;
    this.channels = null;
    this.loading = null;
    this.timer = 0;
    const form = input.closest('form');
    this.list = document.createElement('div');
    this.list.className = 'search-suggest';
    this.list.id = 'search-suggest-list';
    this.list.setAttribute('role', 'listbox');
    this.list.setAttribute('aria-label', '추천 검색어');
    this.list.hidden = true;
    form.append(this.list);
    input.setAttribute('role', 'combobox');
    input.setAttribute('aria-autocomplete', 'list');
    input.setAttribute('aria-controls', this.list.id);
    input.setAttribute('aria-expanded', 'false');
    input.addEventListener('input', () => {
      clearTimeout(this.timer);
      this.timer = setTimeout(() => this.update(), 80);
    });
    input.addEventListener('keydown', (e) => this.onKey(e));
    input.addEventListener('blur', () => setTimeout(() => this.close(), 0));
    form.addEventListener('submit', () => this.close());
    // 누르는 동안 입력란이 초점을 잃어 목록이 먼저 닫히지 않게.
    this.list.addEventListener('mousedown', (e) => e.preventDefault());
    this.list.addEventListener('click', (e) => {
      const row = e.target.closest('[data-sg]');
      if (row) this.pick(Number(row.dataset.sg));
    });
  }

  /** 화면을 다시 열 때 — 채널 후보를 다음 입력에 새로 읽는다(즐겨찾기·팔로우가 그새 바뀌었을 수 있다). */
  reset() {
    this.channels = null;
    this.loading = null;
    this.close();
  }

  async ensureChannels() {
    if (this.channels) return this.channels;
    if (!this.loading) this.loading = loadChannels(this.isLoggedIn).then((c) => { this.channels = c; return c; }, () => { this.channels = []; return []; });
    return this.loading;
  }

  async update() {
    const value = this.input.value;
    if (!value.trim() || document.activeElement !== this.input) { this.close(); return; }
    // 기기 안 목록이 오기 전에도 최근 검색어는 바로 보인다.
    this.render(buildSuggestions(value, { recent: this.recent(), channels: this.channels || [] }));
    if (this.channels) return;
    await this.ensureChannels();
    if (this.input.value !== value || document.activeElement !== this.input) return; // 그새 더 쳤다 — 새 입력이 다시 그린다
    this.render(buildSuggestions(value, { recent: this.recent(), channels: this.channels }));
  }

  render(items) {
    this.items = items;
    this.index = -1;
    if (!items.length) { this.close(); return; }
    this.list.innerHTML = items.map((it, i) => {
      const icon = it.kind === 'channel'
        ? (safeUrl(it.image) ? `<img class="sg-avatar" src="${esc(imageSrc(safeUrl(it.image), 'avatar'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : '<span class="sg-avatar sg-blank" aria-hidden="true"></span>')
        : '<span class="sg-icon" aria-hidden="true">↺</span>';
      const meta = it.kind === 'channel' ? `${it.live ? '<span class="sg-live">LIVE</span>' : ''}<small>${esc(it.sub || '채널')}</small>` : '<small>최근 검색</small>';
      return `<div class="sg-item" id="search-suggest-${i}" role="option" aria-selected="false" data-sg="${i}">${icon}<span class="sg-text ellipsis">${esc(it.text)}</span>${meta}</div>`;
    }).join('');
    this.list.hidden = false;
    this.input.setAttribute('aria-expanded', 'true');
    this.input.removeAttribute('aria-activedescendant');
  }

  get open() { return !this.list.hidden; }

  close() {
    clearTimeout(this.timer);
    if (this.list.hidden) return;
    this.list.hidden = true;
    this.index = -1;
    this.input.setAttribute('aria-expanded', 'false');
    this.input.removeAttribute('aria-activedescendant');
  }

  highlight() {
    [...this.list.children].forEach((el, i) => { el.classList.toggle('active', i === this.index); el.setAttribute('aria-selected', String(i === this.index)); });
    if (this.index >= 0) {
      this.input.setAttribute('aria-activedescendant', `search-suggest-${this.index}`);
      this.list.children[this.index]?.scrollIntoView?.({ block: 'nearest' });
    } else this.input.removeAttribute('aria-activedescendant');
  }

  onKey(e) {
    if (e.isComposing || e.keyCode === 229) return; // 한글 조합 중 화살표·Enter 는 조합을 끝내는 키
    if (!this.open) {
      if (e.key === 'ArrowDown' && this.input.value.trim()) { e.preventDefault(); this.update(); }
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = this.items.length;
      this.index = e.key === 'ArrowDown' ? (this.index + 1) % n : (this.index <= 0 ? n - 1 : this.index - 1);
      this.highlight();
    } else if (e.key === 'Enter' && this.index >= 0) {
      e.preventDefault();
      this.pick(this.index);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation(); // 화면 닫기(Esc)까지 가지 않게 — 목록만 닫는다
      this.close();
    }
  }

  pick(i) {
    const it = this.items[i];
    this.close();
    if (!it) return;
    if (it.kind === 'channel') this.onChannel(it.id);
    else { this.input.value = it.text; this.onQuery(it.text); }
  }
}
