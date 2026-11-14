// 후원 목록 — 채팅 머리의 후원 단추(새 후원 수 배지)와, 누르면 채팅 위에 펼쳐지는 목록 패널.
//
// 예전엔 채팅 머리 바로 아래에 후원·구독 칩 띠(.chat-events)와 주간 랭킹 줄(.donation-rank)이 늘 붙어 채팅 읽는 자리를
// 한두 줄 차지했다. 이제는 머리에 단추 하나만 두고, 새 후원이 오면 배지 수가 늘고 단추가 잠깐 반짝인다.
// - 패널: 합계(치즈·건수), 종류 거르기(전체·후원·미션·구독), 최신순·금액순 정렬, 주간 랭킹, 목록(시간·후원자·금액·종류·메시지).
//   항목을 누르면 채팅의 그 줄로 옮겨 잠깐 강조한다(줄이 목록에서 밀려났으면 알린다).
// - 키보드: 단추(Enter·Space)로 열면 첫 항목에 초점, ↑/↓·Home/End 로 항목 이동, Tab 은 패널 안에서 돈다(trapFocus),
//   Esc·✕ 로 닫으면 단추로 초점을 돌려준다. 바깥을 누르면 닫힌다.
// - 보관: 채팅에 연결한 뒤 들어온 후원·구독(입장 때 받은 지난 기록 포함) 최근 DONATION_LIST_MAX 건. 연결이 바뀌면 비운다.
//   배지는 지금 막 온 것(지난 기록 제외)만 센다.
// - 표시 규칙은 채팅 카드와 같다(donations.js) — 익명은 '익명', 비공개 금액은 금액 칸 없음, 색 단계 ev-lv-1~6.
// - 설정 chat.donationList: 'button'(기본 — 위쪽 띠 없음) · 'strip'(예전처럼 위쪽 띠·랭킹 줄도 보임).
// - 멀티라이브 멀티채팅 보기(통합·그리드·포커스)에서는 ml-multichat.js 가 모든 패인의 후원을 채널 이름과 함께 넣는다(feed).
import { esc, renderContent, toast, hideWithExit } from './dom.js';
import { trapFocus } from './a11y.js';
import { eventMeta, donorName, donationKindLabel, formatCheese, EVENT_ICONS, ICON_CHEESE } from './donations.js';
import { chatPrefs, onChatPrefs } from './chat-prefs.js';
import { chatFollower } from './chat-follow.js';

export const DONATION_LIST_MAX = 300;
/** 예전 위쪽 띠에 보이던 칩 수. */
const STRIP_MAX = 12;
export const DONATION_FILTERS = [['all', '전체'], ['cheese', '후원'], ['mission', '미션'], ['sub', '구독']];
export const DONATION_SORTS = [['recent', '최신순'], ['amount', '금액순']];
const FLASH_MS = 2200;
const ICON_CLOSE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>';

/** 표시 방식 — 'button'(기본, 위쪽 띠 없음) · 'strip'(위쪽 띠도). */
export function donationListMode(p = chatPrefs()) {
  return p && p.donationList === 'strip' ? 'strip' : 'button';
}

/** 거르기 종류 — 'cheese'(채팅·영상·파티 후원) · 'mission' · 'sub'(구독·구독 선물). 후원·구독이 아니면 null. 순수 함수. */
export function donationCategory(m) {
  if (!m) return null;
  if (m.kind === 'subscription') return 'sub';
  if (m.kind !== 'donation') return null;
  return String(m.donationType || '').toLowerCase() === 'mission' ? 'mission' : 'cheese';
}

/** 공개된 치즈 금액(비공개·구독이면 0). 순수 함수. */
export function cheeseOf(m) {
  return m && m.kind === 'donation' && Number.isFinite(m.donationAmount) && m.donationAmount > 0 ? m.donationAmount : 0;
}

/**
 * 보이는 목록 — entries 는 최신이 앞(seq 큰 것). 금액순은 치즈가 많은 순, 같으면(비공개·구독 포함) 최신이 앞. 순수 함수.
 * @param {{m: object, seq: number}[]} entries
 */
export function viewDonations(entries, { filter = 'all', sort = 'recent' } = {}) {
  const list = filter === 'all' ? entries.slice() : entries.filter((e) => donationCategory(e.m) === filter);
  if (sort === 'amount') list.sort((a, b) => cheeseOf(b.m) - cheeseOf(a.m) || b.seq - a.seq);
  return list;
}

/** 합계 — 건수·공개된 치즈 합·종류별 건수. 순수 함수. */
export function donationTotals(entries) {
  const kinds = { cheese: 0, mission: 0, sub: 0 };
  let cheese = 0;
  for (const e of entries) {
    cheese += cheeseOf(e.m);
    const c = donationCategory(e.m);
    if (c) kinds[c] += 1;
  }
  return { count: entries.length, cheese, kinds };
}

/** 합계 한 줄 — '12,300치즈 · 5건'(치즈가 없으면 건수만). 순수 함수. */
export function totalsText({ count, cheese }) {
  const won = formatCheese(cheese);
  return `${won ? `${won} · ` : ''}${count}건`;
}

function timeText(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

const entryKey = (e) => `${e.src ? e.src.key : ''}|${e.m.id}`;
let ids = 0;

export class DonationList {
  /** @param {import('./chat.js').ChatPanel} panel */
  constructor(panel) {
    this.panel = panel;
    this.root = panel.root;
    this.header = panel.header;
    this.id = `dpl${++ids}`;
    this.entries = [];
    this.seq = 0;
    this.unseen = 0;
    this.filter = 'all';
    this.sort = 'recent';
    this.isOpen = false;
    /** 다른 곳이 채우는 중이면 그 이름(멀티라이브 'ml') — 이 패널 자신의 줄은 받지 않고, 연결이 바뀌어도 비우지 않는다. */
    this.external = null;
    /** 채팅 줄 찾기 — 기본은 이 패널 목록. 멀티라이브 멀티채팅은 통합 목록·칸에서 찾는다(setExternal). */
    this.findLine = (entry) => this.panel.list.querySelector(`.chat-line[data-id="${CSS.escape(entry.m.id)}"]`);
    this.presentKeys = () => new Set([...this.panel.list.querySelectorAll('.chat-line.ev[data-id]')].map((l) => `|${l.dataset.id}`));

    // ── 머리 단추 ──
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'icon-btn chat-hbtn dp-btn';
    btn.setAttribute('aria-haspopup', 'dialog');
    btn.setAttribute('aria-expanded', 'false');
    btn.setAttribute('aria-controls', `${this.id}-panel`);
    btn.innerHTML = `${ICON_CHEESE.replace('<svg ', '<svg aria-hidden="true" ')}<span class="dp-badge" aria-hidden="true" hidden></span>`;
    this.button = btn;
    this.badge = btn.querySelector('.dp-badge');
    const tools = this.header.querySelector('.chat-tools');
    if (tools) tools.before(btn); else this.header.append(btn);
    btn.addEventListener('click', () => (this.isOpen ? this.close() : this.open()));
    btn.addEventListener('animationend', () => btn.classList.remove('ping'));

    // ── 예전 위쪽 띠(설정 'strip' 일 때만) ──
    this.strip = document.createElement('div');
    this.strip.className = 'chat-events';
    this.strip.hidden = true;
    this.stripHtml = '';

    // ── 패널 ──
    const el = document.createElement('div');
    el.className = 'dp-panel';
    el.id = `${this.id}-panel`;
    el.hidden = true;
    el.setAttribute('role', 'dialog');
    el.setAttribute('aria-modal', 'true');
    el.setAttribute('aria-labelledby', `${this.id}-title`);
    el.innerHTML = `
      <div class="dp-head">
        <strong class="dp-title" id="${this.id}-title">후원 목록</strong>
        <span class="dp-sum"></span>
        <button type="button" class="icon-btn dp-close" title="닫기 (Esc)" aria-label="후원 목록 닫기">${ICON_CLOSE}</button>
      </div>
      <div class="dp-rank-slot"></div>
      <div class="dp-controls">
        <div class="chips dp-filter" role="group" aria-label="종류 거르기">${DONATION_FILTERS.map(([v, l]) => `<button type="button" class="chip" data-dp-filter="${v}" aria-pressed="false"><span>${l}</span><span class="dp-count"></span></button>`).join('')}</div>
        <div class="segmented dp-sort" role="group" aria-label="정렬">${DONATION_SORTS.map(([v, l]) => `<button type="button" class="seg" data-dp-sort="${v}" aria-pressed="false">${l}</button>`).join('')}</div>
      </div>
      <ul class="dp-list" aria-labelledby="${this.id}-title"></ul>
      <p class="dp-empty" hidden></p>
      <p class="dp-cap" hidden>최근 ${DONATION_LIST_MAX}건까지 보관합니다</p>`;
    this.el = el;
    this.sumEl = el.querySelector('.dp-sum');
    this.listEl = el.querySelector('.dp-list');
    this.emptyEl = el.querySelector('.dp-empty');
    this.capEl = el.querySelector('.dp-cap');
    this.rankSlot = el.querySelector('.dp-rank-slot');
    this.root.append(el);
    el.addEventListener('click', (e) => this.onClick(e));
    el.addEventListener('keydown', (e) => this.onListKey(e));

    this.onOutside = (e) => {
      if (!this.el.contains(e.target) && !this.button.contains(e.target)) this.close({ restore: false });
    };
    this.onKey = (e) => {
      if (e.key !== 'Escape' || !this.isOpen) return;
      // 위에 뜬 시트·메뉴·팔레트가 있으면 그것이 먼저 닫힌다.
      if (document.querySelector('.sheet-scrim:not(.leaving), .ctx-menu, .palette-scrim:not([hidden], .exit-ghost)')) return;
      e.preventDefault();
      e.stopPropagation(); // 시청 화면 닫기(app.js Esc)·줄 고르기 해제까지 가지 않게
      this.close();
    };
    // 밖에서 hidden 으로 닫았을 때(app.js 의 Esc 처리 등) 상태를 맞춘다.
    new MutationObserver(() => { if (this.el.hidden && this.isOpen) this.close({ restore: false }); })
      .observe(el, { attributes: true, attributeFilter: ['hidden'] });
    this.ro = new ResizeObserver(() => { if (this.isOpen) this.position(); });

    this.mode = null;
    this.applyMode();
    this.renderButton();
    panel.cleanups.push(onChatPrefs(() => {
      this.applyMode();
      if (this.isOpen) this.renderList();
    }));
    panel.cleanups.push(() => this.close({ restore: false }));
  }

  /** 표시 방식(설정)에 맞춰 위쪽 띠·랭킹 줄을 머리 아래 또는 패널 안에 둔다. */
  applyMode() {
    const mode = donationListMode();
    if (mode === this.mode) return;
    this.mode = mode;
    const rank = this.panel.ranking && this.panel.ranking.el;
    if (mode === 'strip') {
      if (rank) this.header.after(rank);
      (rank || this.header).after(this.strip);
    } else {
      if (rank) this.rankSlot.append(rank);
      this.strip.remove();
    }
    this.root.classList.toggle('dp-strip-mode', mode === 'strip');
    this.renderStrip();
  }

  /**
   * 채팅 줄 묶음에서 후원·구독을 모은다.
   * @param {object[]} messages
   * @param {{ fresh?: Set<object>|null, src?: {key: string, name: string, hue?: number}|null }} [opts]
   *   fresh — 지금 막 온 메시지 집합(없으면 모두 새것). 지난 기록은 목록에는 넣고 배지에는 세지 않는다.
   *   src — 출처 채널(멀티라이브 멀티채팅). 목록 항목에 채널 이름을 붙인다.
   */
  add(messages, { fresh = null, src = null } = {}) {
    let got = false;
    let ping = false;
    for (const m of messages || []) {
      if (!m || (m.kind !== 'donation' && m.kind !== 'subscription') || !eventMeta(m)) continue;
      this.entries.unshift({ m, seq: ++this.seq, src });
      got = true;
      if ((!fresh || fresh.has(m)) && !this.isOpen) { this.unseen += 1; ping = true; }
    }
    if (!got) return;
    if (this.entries.length > DONATION_LIST_MAX) this.entries.length = DONATION_LIST_MAX;
    this.renderButton(ping);
    this.renderStrip();
    if (this.isOpen) this.renderList();
  }

  /** 비운다 — 채팅 연결(채널)이 바뀌었을 때. */
  reset() {
    this.entries = [];
    this.unseen = 0;
    this.renderButton();
    this.renderStrip();
    if (this.isOpen) this.renderList();
  }

  /**
   * 다른 곳이 목록을 채우게 한다(멀티라이브 멀티채팅) — name 이 null 이면 다시 이 패널의 줄로. 바뀌면 비운다.
   * @param {string|null} name
   * @param {{ findLine?: (entry) => Element|null, presentKeys?: () => Set<string> }} [hooks]
   */
  setExternal(name, hooks = {}) {
    if ((this.external || null) === (name || null)) return;
    this.external = name || null;
    this.findLine = hooks.findLine || ((entry) => this.panel.list.querySelector(`.chat-line[data-id="${CSS.escape(entry.m.id)}"]`));
    this.presentKeys = hooks.presentKeys || (() => new Set([...this.panel.list.querySelectorAll('.chat-line.ev[data-id]')].map((l) => `|${l.dataset.id}`)));
    this.reset();
  }

  // ── 그리기 ──

  renderButton(ping = false) {
    const n = this.unseen;
    this.badge.hidden = n === 0;
    this.badge.textContent = n > 99 ? '99+' : String(n);
    this.button.classList.toggle('has-new', n > 0);
    const label = n ? `후원 목록 — 새 후원 ${n}건` : `후원 목록${this.entries.length ? ` (${this.entries.length}건)` : ''}`;
    this.button.title = label;
    this.button.setAttribute('aria-label', label);
    if (ping) {
      // 같은 프레임에 두 번 와도 한 번만 다시 시작한다. 모션 줄이기면 CSS 가 애니메이션을 끈다.
      this.button.classList.remove('ping');
      cancelAnimationFrame(this.pingRaf);
      this.pingRaf = requestAnimationFrame(() => this.button.classList.add('ping'));
    }
  }

  renderStrip() {
    const on = this.mode === 'strip' && this.entries.length > 0;
    this.strip.hidden = !on;
    if (!on) return;
    const html = this.entries.slice(0, STRIP_MAX).map(({ m }) => {
      const e = eventMeta(m);
      return `<span class="event-chip ev-${e.tone} ev-tier-${e.tier} ev-lv-${e.level}" title="${esc(`${e.title} · ${m.content || ''}`)}"><span class="event-chip-icon">${EVENT_ICONS[e.icon] || ''}</span><b>${esc(donorName(m))}</b>${e.value ? `<span class="event-chip-value">${esc(e.value)}</span>` : ''}</span>`;
    }).join('');
    // 후원이 올 때만 다시 쓴다(채팅 묶음마다 칩을 새로 만들면 버린 SVG 가 쌓였다).
    if (html === this.stripHtml) return;
    this.stripHtml = html;
    this.strip.innerHTML = html;
  }

  itemHtml(entry, present) {
    const { m, src } = entry;
    const e = eventMeta(m);
    const name = donorName(m);
    const kind = m.kind === 'donation' ? donationKindLabel(m.donationType) : e.title;
    const detail = e.detail ? ` · ${e.detail}` : '';
    const body = [
      e.extra ? `<span class="dp-extra">${esc(e.extra)}</span>` : '',
      m.content && String(m.content).trim() ? renderContent(m.content, m.emojis, chatPrefs().emoticonEnabled) : '',
    ].filter(Boolean).join(' ');
    const here = present.has(entryKey(entry));
    const hue = src && Number.isFinite(Number(src.hue)) ? ` style="--lh:${Number(src.hue)}"` : '';
    return `<li class="dp-row"><button type="button" class="dp-item ev-${e.tone} ev-lv-${e.level}${here ? '' : ' gone'}" data-dp="${entry.seq}" tabindex="-1"${here ? '' : ' aria-disabled="true"'} title="${here ? '채팅에서 이 후원 보기' : '채팅 목록에서 이미 지나간 후원입니다'}">
      <span class="dp-icon" aria-hidden="true">${EVENT_ICONS[e.icon] || ''}</span>
      <span class="dp-main">
        <span class="dp-top"><b class="dp-nick${name === '익명' ? ' anon' : ''}">${esc(name)}</b>${e.value ? `<span class="dp-value">${esc(e.value)}</span>` : ''}</span>
        <span class="dp-meta"><span class="dp-kind">${esc(kind + detail)}</span>${src ? `<span class="dp-src"${hue}>${esc(src.name)}</span>` : ''}<time class="dp-time">${esc(timeText(m.timestamp))}</time>${here ? '' : '<span class="dp-gone">지나감</span>'}</span>
        ${body ? `<span class="dp-msg">${body}</span>` : ''}
      </span>
    </button></li>`;
  }

  renderList() {
    const focusedSeq = this.el.contains(document.activeElement) ? document.activeElement.closest('.dp-item')?.dataset.dp : null;
    const all = donationTotals(this.entries);
    const view = viewDonations(this.entries, { filter: this.filter, sort: this.sort });
    this.sumEl.textContent = this.entries.length ? `합계 ${totalsText(donationTotals(view))}` : '';
    for (const b of this.el.querySelectorAll('[data-dp-filter]')) {
      const v = b.dataset.dpFilter;
      const on = v === this.filter;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
      b.querySelector('.dp-count').textContent = String(v === 'all' ? all.count : all.kinds[v]);
    }
    for (const b of this.el.querySelectorAll('[data-dp-sort]')) {
      const on = b.dataset.dpSort === this.sort;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
    const present = view.length ? this.presentKeys() : new Set();
    this.listEl.innerHTML = view.map((x) => this.itemHtml(x, present)).join('');
    this.listEl.hidden = view.length === 0;
    this.emptyEl.hidden = view.length > 0;
    this.emptyEl.textContent = this.entries.length
      ? '이 종류의 후원이 아직 없습니다'
      : '아직 후원이 없습니다 — 채팅에 들어온 후원·구독이 여기에 모입니다';
    this.capEl.hidden = this.entries.length < DONATION_LIST_MAX;
    // 항목은 하나만 Tab 으로 닿는다(↑/↓ 로 옮긴다). 다시 그려도 보던 항목에 초점을 남긴다.
    const items = [...this.listEl.querySelectorAll('.dp-item')];
    const keep = focusedSeq && items.find((b) => b.dataset.dp === focusedSeq);
    (keep || items[0])?.setAttribute('tabindex', '0');
    if (keep) keep.focus({ preventScroll: true });
  }

  /** 패널 자리 — 채팅 머리 바로 아래(머리가 두 줄로 접혀도)부터 채팅 아래쪽 8px 까지. */
  position() {
    const rr = this.root.getBoundingClientRect();
    const hr = this.header.getBoundingClientRect();
    const top = Math.max(0, Math.round(hr.bottom - rr.top) + 6);
    this.el.style.top = `${top}px`;
    this.el.style.maxHeight = `${Math.max(120, Math.round(rr.height) - top - 8)}px`;
  }

  // ── 열고 닫기 ──

  open() {
    if (this.isOpen) return;
    this.isOpen = true;
    this.unseen = 0;
    // 같은 자리에 뜨는 멘션 모음은 닫는다.
    if (this.panel.tray) this.panel.tray.hidden = true;
    this.el.hidden = false;
    this.button.setAttribute('aria-expanded', 'true');
    this.button.classList.add('open');
    this.renderButton();
    this.renderList();
    this.position();
    this.ro.observe(this.root);
    this.ro.observe(this.header);
    this.releaseTrap = trapFocus(this.el, { restore: false });
    document.addEventListener('pointerdown', this.onOutside, true);
    document.addEventListener('keydown', this.onKey, true);
    const first = this.listEl.querySelector('.dp-item[tabindex="0"]') || this.el.querySelector('.dp-close');
    first.focus({ preventScroll: true });
  }

  /** @param {{ restore?: boolean }} [opts] restore — 단추로 초점을 돌려준다(패널 안에 초점이 있었거나 초점이 갈 곳을 잃었을 때만). */
  close({ restore = true } = {}) {
    if (!this.isOpen) return;
    this.isOpen = false;
    // WebKit(macOS WKWebView)은 마우스로 누른 단추에 초점을 주지 않고, 누르는 순간 패널 항목의 초점을 body 로 떨어뜨린다 —
    // ✕·머리 단추를 마우스로 눌러 닫을 때 초점은 '패널 안'이 아니라 body 다. 그대로 두면 초점이 사라지므로 이때도 돌려준다.
    const at = document.activeElement;
    const had = this.el.contains(at) || !at || at === document.body;
    if (!this.el.hidden) hideWithExit(this.el);
    this.button.setAttribute('aria-expanded', 'false');
    this.button.classList.remove('open');
    this.ro.disconnect();
    this.releaseTrap?.();
    this.releaseTrap = null;
    document.removeEventListener('pointerdown', this.onOutside, true);
    document.removeEventListener('keydown', this.onKey, true);
    this.renderButton();
    if (restore && had && this.button.isConnected) this.button.focus({ preventScroll: true });
  }

  onClick(e) {
    if (e.target.closest('.dp-close')) { this.close(); return; }
    const f = e.target.closest('[data-dp-filter]');
    if (f) { this.filter = f.dataset.dpFilter; this.renderList(); return; }
    const s = e.target.closest('[data-dp-sort]');
    if (s) { this.sort = s.dataset.dpSort; this.renderList(); return; }
    const item = e.target.closest('.dp-item');
    if (item) this.jump(Number(item.dataset.dp));
  }

  onListKey(e) {
    const item = e.target.closest && e.target.closest('.dp-item');
    if (!item || e.metaKey || e.ctrlKey || e.altKey) return;
    const items = [...this.listEl.querySelectorAll('.dp-item')];
    const i = items.indexOf(item);
    const j = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: items.length - 1 }[e.key];
    if (j === undefined) return;
    e.preventDefault();
    const next = items[Math.max(0, Math.min(items.length - 1, j))];
    if (!next) return;
    item.setAttribute('tabindex', '-1');
    next.setAttribute('tabindex', '0');
    next.focus();
  }

  /** 채팅의 그 줄로 — 패널을 닫고 줄을 목록 가운데로 옮겨 잠깐 강조한다. */
  jump(seq) {
    const entry = this.entries.find((x) => x.seq === seq);
    let line = entry && this.findLine(entry);
    if (!line || !line.isConnected) { toast('채팅 목록에서 이미 지나간 후원입니다', 1800); return; }
    this.close({ restore: false });
    // 검색·이 사람만 보기로 숨은 줄이면 거르기를 푼다(단일 채팅).
    if (!this.external && line.offsetParent === null) {
      if (this.panel.searchQuery) this.panel.closeSearch?.();
      if (this.panel.soloUid) this.panel.setSolo(null);
      line = this.findLine(entry) || line;
    }
    const list = line.closest('.chat-list');
    if (line.offsetParent === null || !list) { toast('지금 보기에서는 이 후원 줄이 숨겨져 있습니다', 1800); this.button.focus({ preventScroll: true }); return; }
    // 바닥 따라가기를 끈다 — 사용자 손이 닿지 않은 스크롤은 따라가기(chat-follow.js)가 곧바로 바닥으로 되돌렸다.
    // 단일 채팅은 '새 메시지' 단추로, 멀티라이브 칸은 잠시 뒤(resumeMs) 다시 바닥을 따라간다.
    const follower = chatFollower(list);
    follower.stuck = false;
    follower.touchedAt = performance.now();
    const lr = list.getBoundingClientRect();
    const r = line.getBoundingClientRect();
    list.scrollTop += (r.top - lr.top) - Math.max(0, (list.clientHeight - r.height) / 2);
    // 이미 비추는 중이면 애니메이션을 처음부터 다시(클래스를 뗐다 붙이기 사이에 스타일을 한 번 계산한다).
    if (line.classList.contains('dp-flash')) { line.classList.remove('dp-flash'); void line.offsetWidth; }
    line.classList.add('dp-flash');
    clearTimeout(line.dpFlashTimer);
    line.dpFlashTimer = setTimeout(() => line.classList.remove('dp-flash'), FLASH_MS);
    // 키보드 사용자는 채팅 목록에서 이어 간다 — 사용자 ID 가 있는 줄이면 그 줄을 고른다(chat-actions 줄 고르기).
    // 초점을 받지 않는 목록(멀티라이브 통합·칸)이면 단추로 돌려준다(숨은 패널에 초점이 남지 않게).
    if (list.tabIndex >= 0) list.focus({ preventScroll: true });
    else this.button.focus({ preventScroll: true });
    if (!this.external && line.dataset.uid) this.panel.selectLine?.(line);
  }
}
