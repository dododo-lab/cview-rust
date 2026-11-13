// 채팅 입력부 — 이모티콘 선택기(CHT-01), 자동완성(:이모티콘·@멘션, CHT-02), 남은 글자 수, 입력 기록(↑/↓).
// 예전엔 ChatPanel 안에 있어 멀티채팅 통합 입력(multichat.js)에는 이것들이 하나도 없었다. 이제 두 곳이 같은 입력부를 쓴다.
// 이모티콘·닉네임 목록은 입력부가 갖지 않고 `source()` 가 돌려준 채팅 패널에서 그때그때 읽는다 — 통합 입력은 '보낼 채널'이
// 바뀌면 그 채널 패널의 이모티콘·최근 닉네임으로 바뀐다.
import { esc, safeUrl, toast, imageSrc, hideWithExit } from './dom.js';

// 글자 수 표시 — 입력 제한(index.html maxlength, Rust MAX_CHAT_LENGTH 100)에 가까워지면 남은 글자를 보인다.
const COUNT_FROM = 80;
// 입력 기록(↑/↓)으로 되부를 수 있는 보낸 메시지 수
const HISTORY_MAX = 30;
let SEQ = 0;

const SMILE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0"/><circle cx="9" cy="10" r=".6" fill="currentColor"/><circle cx="15" cy="10" r=".6" fill="currentColor"/></svg>';

/**
 * @typedef {object} ComposerSource 이모티콘·닉네임을 읽어 올 채팅 패널(ChatPanel 과 같은 모양)
 * @property {() => object[]} packs 이모티콘 팩(채널 팩 + 채팅에서 본 이모티콘)
 * @property {() => {id: string, name: string, url: string}[]} allEmoticons
 * @property {Map<string, number>} recentNicks 닉네임 → 마지막 시각
 * @property {boolean} [emoticonError] 불러오기 실패
 * @property {string|null} [channelId]
 * @property {(channelId: string) => void} [loadEmoticons]
 */

/**
 * 입력창 앞에 `@닉네임 ` 을 넣는다 — 보낼 수 없으면 알리고, 100자를 넘기면 넣지 않는다. input 이벤트를 보내
 * 그 입력창의 ChatComposer 가 글자 수·자동완성을 다시 본다(입력부를 직접 잡지 못하는 멀티라이브 통합 목록이 쓴다).
 * @returns {boolean} 넣었는가
 */
export function mentionInto(input, nick) {
  if (!input || input.disabled) { toast('지금은 채팅을 보낼 수 없습니다'); return false; }
  const next = `${`@${nick} ${input.value}`.trimEnd()} `;
  const max = input.maxLength > 0 ? input.maxLength : 100;
  if ([...next].length > max) { toast(`채팅은 ${max}자까지 쓸 수 있어요`, 1500); return false; }
  input.value = next;
  input.focus();
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
}

export class ChatComposer {
  /**
   * @param {HTMLFormElement} form 입력 폼(.chat-input) — 안의 첫 text input 을 쓴다
   * @param {{ source: () => ComposerSource|null, host?: HTMLElement }} opts host — 글자 수를 보일 때 has-count 를 붙일 상자
   */
  constructor(form, { source, host = form }) {
    this.form = form;
    this.host = host;
    this.source = source;
    this.input = form.querySelector('input[type="text"], input:not([type])');
    this.cleanups = [];
    SEQ += 1;
    this.uid = `cc${SEQ}`;
    this.pickerPack = 0;

    this.emojiButton = document.createElement('button');
    this.emojiButton.type = 'button';
    this.emojiButton.className = 'icon-btn chat-emoji-btn';
    this.emojiButton.title = '이모티콘';
    this.emojiButton.setAttribute('aria-label', '이모티콘');
    this.emojiButton.setAttribute('aria-expanded', 'false');
    this.emojiButton.innerHTML = SMILE;
    this.input.before(this.emojiButton);
    this.picker = document.createElement('div');
    this.picker.className = 'emoji-picker';
    this.picker.setAttribute('role', 'dialog');
    this.picker.setAttribute('aria-label', '이모티콘 고르기');
    this.picker.hidden = true;
    form.append(this.picker);
    this.emojiButton.addEventListener('click', (e) => { e.stopPropagation(); this.togglePicker(); });
    this.picker.addEventListener('click', (e) => this.onPickerClick(e));
    this.picker.addEventListener('input', () => this.renderPicker());
    // Esc 로 닫고 입력창으로 돌아간다(예전엔 검색칸에 초점이 있어도 Esc 가 먹지 않았다).
    this.picker.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); this.closePicker(); this.input.focus(); return; }
      this.onPickerKey(e);
    });
    const onDocClick = (e) => {
      if (!this.picker.contains(e.target) && !this.emojiButton.contains(e.target)) this.closePicker();
    };
    document.addEventListener('click', onDocClick);
    this.cleanups.push(() => document.removeEventListener('click', onDocClick));

    this.autocomplete = document.createElement('div');
    this.autocomplete.className = 'chat-autocomplete';
    this.autocomplete.hidden = true;
    // 콤보박스 — 화면 읽기가 후보 목록과 고른 후보를 알 수 있게.
    this.autocomplete.id = `${this.uid}-ac`;
    this.autocomplete.setAttribute('role', 'listbox');
    this.autocomplete.setAttribute('aria-label', '자동 완성');
    this.input.setAttribute('role', 'combobox');
    this.input.setAttribute('aria-autocomplete', 'list');
    this.input.setAttribute('aria-controls', this.autocomplete.id);
    this.input.setAttribute('aria-expanded', 'false');
    if (!this.input.hasAttribute('aria-label')) this.input.setAttribute('aria-label', '채팅 입력');
    // 남은 글자 수 — 제한에 가까울 때만 보인다.
    this.counter = document.createElement('span');
    this.counter.className = 'chat-count';
    this.counter.hidden = true;
    this.counter.setAttribute('aria-live', 'polite');
    this.input.after(this.counter);
    this.history = [];
    this.historyAt = -1;
    form.append(this.autocomplete);
    this.input.addEventListener('input', () => { this.updateAutocomplete(); this.renderCount(); this.historyAt = -1; });
    this.input.addEventListener('keydown', (e) => this.onInputKey(e));
    this.autocomplete.addEventListener('mousedown', (e) => {
      const item = e.target.closest('[data-ac]');
      if (item) { e.preventDefault(); this.applyAutocomplete(item.dataset.ac); }
    });
  }

  destroy() {
    for (const fn of this.cleanups.splice(0)) fn();
  }

  /** 보낼 수 있는가에 맞춰 이모티콘 단추도 함께 켜고 끈다(입력·보내기 단추는 쓰는 쪽이). */
  setEnabled(on) {
    this.emojiButton.disabled = !on;
    if (!on) { this.closePicker(); this.setAutocompleteOpen(false); }
  }

  /** 자동완성이 열려 있으면 Enter 는 후보 고르기 — 쓰는 쪽 submit 이 보내지 않게 묻는다. */
  get choosing() {
    return !this.autocomplete.hidden;
  }

  // ── 글자 수·기록 ─────────────────────────

  /** 보낸 메시지를 입력 기록에 — 같은 말을 연달아 보내면 한 번만. */
  remember(text) {
    if (this.history[this.history.length - 1] !== text) this.history.push(text);
    if (this.history.length > HISTORY_MAX) this.history.shift();
    this.historyAt = -1;
  }

  /** ↑/↓ 입력 기록 — 입력창이 비었거나 기록을 넘기는 중일 때만(쓰던 글의 커서 이동은 그대로). */
  recall(dir) {
    if (!this.history.length) return false;
    const browsing = this.historyAt >= 0;
    if (!browsing && (dir > 0 || this.input.value)) return false;
    let at = browsing ? this.historyAt + dir : this.history.length - 1;
    if (at < 0) at = 0;
    if (at >= this.history.length) { this.historyAt = -1; this.input.value = ''; this.renderCount(); return true; }
    this.historyAt = at;
    this.input.value = this.history[at];
    const end = this.input.value.length;
    this.input.setSelectionRange(end, end);
    this.renderCount();
    return true;
  }

  /** 남은 글자 수 — 제한에 가까울 때만. */
  renderCount() {
    const max = this.input.maxLength > 0 ? this.input.maxLength : 100;
    const n = [...this.input.value].length;
    const show = n >= COUNT_FROM;
    this.counter.hidden = !show;
    this.host.classList.toggle('has-count', show);
    if (!show) return;
    this.counter.textContent = `${n}/${max}`;
    this.counter.classList.toggle('full', n >= max);
    this.counter.setAttribute('aria-label', n >= max ? `${max}자까지 쓸 수 있어요` : `${max - n}자 남음`);
  }

  /** 입력 제한(maxlength 100 = 백엔드 MAX_CHAT_LENGTH) 안인가 — 넘으면 알리고 넣지 않는다. 멘션·이모티콘을 코드로 넣으면
   *  maxlength 가 막지 않아, 100자를 넘긴 글이 보낼 때 뒤가 잘렸다(글자 수 표시도 갱신되지 않았다). */
  fitsInput(value) {
    const max = this.input.maxLength > 0 ? this.input.maxLength : 100;
    if ([...value].length <= max) return true;
    toast(`채팅은 ${max}자까지 쓸 수 있어요`, 1500);
    return false;
  }

  /** @닉네임 을 입력 앞에 넣는다(줄 메뉴·닉네임 말풍선의 '멘션'). */
  mention(nick) {
    mentionInto(this.input, nick);
  }

  insertAtCaret(text) {
    const el = this.input;
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const next = el.value.slice(0, start) + text + el.value.slice(end);
    if (!this.fitsInput(next)) return;
    el.value = next;
    const pos = start + text.length;
    el.focus();
    el.setSelectionRange(pos, pos);
    this.renderCount();
  }

  // ── 이모티콘 선택기 (CHT-01) ─────────────

  packs() {
    const s = this.source();
    return s ? s.packs() : [];
  }

  allEmoticons() {
    const s = this.source();
    return s ? s.allEmoticons() : [];
  }

  closePicker() {
    if (this.picker.hidden) return;
    hideWithExit(this.picker);
    this.emojiButton.setAttribute('aria-expanded', 'false');
  }

  togglePicker() {
    if (!this.picker.hidden) { this.closePicker(); return; }
    this.emojiButton.setAttribute('aria-expanded', 'true');
    // 틀은 한 번만 만든다 — 다시 열면 직전 검색어·스크롤이 그대로다(예전엔 열 때마다 innerHTML 을 새로 써 지워졌다).
    if (!this.picker.querySelector('.emoji-search')) {
      this.picker.innerHTML = `
        <input type="search" class="emoji-search" placeholder="이모티콘 검색" autocomplete="off" aria-label="이모티콘 검색">
        <div class="emoji-tabs"></div>
        <div class="emoji-grid" role="group" aria-label="이모티콘"></div>`;
    }
    // 불러오지 못했던 채널이면 다시 묻는다(열 때 한 번).
    const s = this.source();
    if (s && s.emoticonError && s.channelId && !(s.emoticons || []).length && s.loadEmoticons) s.loadEmoticons(s.channelId);
    const grid = this.picker.querySelector('.emoji-grid');
    const scroll = grid.scrollTop;
    this.picker.hidden = false;
    this.renderPicker();
    grid.scrollTop = scroll;
    this.picker.querySelector('.emoji-search').focus();
  }

  /** 선택기 키보드 — 칸 사이를 ←→↑↓ 로(줄 너비는 칸 위치로 잰다), 검색칸에서 ↓ 는 첫 칸, 첫 줄에서 ↑ 는 검색칸. */
  onPickerKey(e) {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
    const search = this.picker.querySelector('.emoji-search');
    const cells = [...this.picker.querySelectorAll('.emoji-cell')];
    if (!cells.length) return;
    if (e.target === search) {
      if (e.key !== 'ArrowDown') return;
      e.preventDefault();
      cells[0].focus();
      return;
    }
    const i = cells.indexOf(e.target.closest('.emoji-cell'));
    if (i < 0) return;
    e.preventDefault();
    const top = cells[0].offsetTop;
    let cols = cells.findIndex((c) => c.offsetTop !== top);
    if (cols < 0) cols = cells.length;
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -cols, ArrowDown: cols }[e.key];
    const j = i + step;
    if (j < 0) { if (e.key === 'ArrowUp') search.focus(); return; }
    cells[Math.min(cells.length - 1, j)].focus();
  }

  renderPicker() {
    const tabs = this.picker.querySelector('.emoji-tabs');
    const grid = this.picker.querySelector('.emoji-grid');
    if (!tabs || !grid) return;
    const packs = this.packs();
    if (!packs.length) {
      // 네트워크 실패와 '목록 없음'을 가른다 — 예전엔 실패해도 '이 채널은 주지 않습니다'라고 했다.
      const s = this.source();
      grid.innerHTML = s && s.emoticonError
        ? '<div class="tray-empty">이모티콘 목록을 불러오지 못했습니다 — 닫았다 다시 열면 다시 시도합니다</div>'
        : '<div class="tray-empty">이 채널은 이모티콘 목록을 주지 않습니다 — 채팅에 이모티콘이 보이면 여기에 모입니다</div>';
      tabs.innerHTML = '';
      return;
    }
    const q = (this.picker.querySelector('.emoji-search').value || '').trim().toLowerCase();
    if (this.pickerPack >= packs.length) this.pickerPack = 0;
    tabs.hidden = Boolean(q);
    tabs.innerHTML = packs.map((p, i) => {
      const icon = safeUrl(p.emoticonPackImageUrl);
      return `<button type="button" class="emoji-tab${i === this.pickerPack ? ' active' : ''}" data-pack="${i}" title="${esc(p.emoticonPackName)}">${icon ? `<img src="${esc(imageSrc(icon, 'static'))}" alt="" referrerpolicy="no-referrer">` : esc(p.emoticonPackName.slice(0, 2))}</button>`;
    }).join('');
    const items = q
      ? this.allEmoticons().filter((e) => e.id.toLowerCase().includes(q) || String(e.name).toLowerCase().includes(q)).slice(0, 120)
      : (packs[this.pickerPack].emoticons || []).map((e) => ({ id: e.emoticonId, name: e.emoticonName || e.emoticonId, url: e.imageUrl }));
    grid.innerHTML = items.length
      ? items.map((e) => `<button type="button" class="emoji-cell" data-emoji="${esc(e.id)}" title="${esc(e.name)}"><img src="${esc(imageSrc(e.url, 'static'))}" alt="${esc(e.name)}" loading="lazy" referrerpolicy="no-referrer"></button>`).join('')
      : '<div class="tray-empty">찾는 이모티콘이 없습니다</div>';
  }

  onPickerClick(e) {
    const tab = e.target.closest('[data-pack]');
    if (tab) { this.pickerPack = Number(tab.dataset.pack); this.renderPicker(); return; }
    const cell = e.target.closest('[data-emoji]');
    if (cell) this.insertAtCaret(`{:${cell.dataset.emoji}:}`);
  }

  // ── 자동완성 (CHT-02) ────────────────────

  /** 커서 앞 토큰(`:abc` 또는 `@abc`). */
  currentToken() {
    const el = this.input;
    const upto = el.value.slice(0, el.selectionStart ?? el.value.length);
    const m = /(^|\s)([:@])([^\s:@]{1,30})$/.exec(upto);
    return m ? { kind: m[2], query: m[3], start: upto.length - m[3].length - 1 } : null;
  }

  updateAutocomplete() {
    const tok = this.currentToken();
    if (!tok) { this.setAutocompleteOpen(false); return; }
    const q = tok.query.toLowerCase();
    let items = [];
    if (tok.kind === ':') {
      items = this.allEmoticons().filter((e) => e.id.toLowerCase().startsWith(q) || String(e.name).toLowerCase().includes(q)).slice(0, 8)
        .map((e) => ({ value: `{:${e.id}:}`, html: `<img loading="lazy" decoding="async" src="${esc(imageSrc(e.url, 'static'))}" alt="" referrerpolicy="no-referrer"><span>${esc(e.name)}</span>` }));
    } else {
      const nicks = this.source()?.recentNicks || new Map();
      items = [...nicks.entries()].sort((a, b) => b[1] - a[1]).map(([n]) => n)
        .filter((n) => n.toLowerCase().includes(q)).slice(0, 8)
        .map((n) => ({ value: `@${n} `, html: `<span>@${esc(n)}</span>` }));
    }
    this.acItems = items;
    this.acIndex = 0;
    this.acToken = tok;
    this.autocomplete.innerHTML = items.map((it, i) => `<div class="ac-item${i === 0 ? ' active' : ''}" id="${this.uid}-ac${i}" role="option" aria-selected="${i === 0}" data-ac="${esc(it.value)}">${it.html}</div>`).join('');
    this.setAutocompleteOpen(items.length > 0);
  }

  setAutocompleteOpen(open) {
    this.autocomplete.hidden = !open;
    this.input.setAttribute('aria-expanded', String(open));
    if (open) this.input.setAttribute('aria-activedescendant', `${this.uid}-ac${this.acIndex || 0}`);
    else this.input.removeAttribute('aria-activedescendant');
  }

  applyAutocomplete(value) {
    const tok = this.acToken;
    const el = this.input;
    if (!tok) return;
    const caret = el.selectionStart ?? el.value.length;
    const next = el.value.slice(0, tok.start) + value + el.value.slice(caret);
    if (!this.fitsInput(next)) { this.setAutocompleteOpen(false); return; }
    el.value = next;
    const pos = tok.start + value.length;
    el.setSelectionRange(pos, pos);
    this.setAutocompleteOpen(false);
    el.focus();
    this.renderCount();
  }

  onInputKey(e) {
    if (this.autocomplete.hidden) {
      if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.shiftKey && !e.altKey && !e.metaKey && !e.isComposing
        && this.recall(e.key === 'ArrowUp' ? -1 : 1)) e.preventDefault();
      return;
    }
    const items = this.autocomplete.querySelectorAll('.ac-item');
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      this.acIndex = (this.acIndex + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items.forEach((el, i) => { el.classList.toggle('active', i === this.acIndex); el.setAttribute('aria-selected', String(i === this.acIndex)); });
      this.input.setAttribute('aria-activedescendant', `${this.uid}-ac${this.acIndex}`);
      items[this.acIndex]?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      this.applyAutocomplete(this.acItems[this.acIndex].value);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      this.setAutocompleteOpen(false);
    }
  }
}
