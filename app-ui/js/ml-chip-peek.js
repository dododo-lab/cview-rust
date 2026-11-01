// 라이브 머리줄 채널 칩 미리보기 말풍선 — 칩에 마우스를 잠시 올리면 칩 아래로 말풍선이 열려 그 채널의
// 썸네일·제목·시청자를 보여 주고, 곧이어 소리 없는 영상 미리보기(hover-preview.js previewInto)를 깐다.
//
// - 지연: 처음엔 OPEN_MS 머물러야 열린다. 열린 채로 옆 칩으로 옮기면 바로 바뀐다(훑어보기).
// - 지금 보는 칩(active)은 이미 무대에 있으니 열지 않는다.
// - 닫기: 칩 줄을 벗어나면(말풍선 위로 옮겨도 닫는다 — 영상을 가리는 일이 없게), 누르면, 칩 줄을 넘기면,
//   칩 줄이 다시 그려지거나 숨으면, 창이 가려지면.
// - 말풍선은 body 에 position: fixed — 칩 줄이 가로로 넘기는(overflow) 칸이라 그 안에 두면 잘린다.
import { esc, safeUrl, imageSrc } from './dom.js';
import { previewInto, closePreviewIn } from './hover-preview.js';
import { verifiedBadge } from './verified-badge.js';

const OPEN_MS = 350;
const CLOSE_MS = 90; // 칩 사이 틈(2px)을 지나는 동안 닫혔다 열리며 깜박이지 않게
const WIDTH = 288;
const MARGIN = 8;

const VIEWERS_ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>';

export class ChipPeek {
  /**
   * @param {HTMLElement} picker 칩 줄(.ml-watch-picker)
   * @param {(sessionId: string) => any} sessionOf 칩의 세션(MultiLivePaneDto)
   */
  constructor(picker, sessionOf) {
    this.picker = picker;
    this.sessionOf = sessionOf;
    this.chip = null; // 말풍선이 가리키는 칩(열렸거나 열 예정)
    this.openTimer = 0;
    this.closeTimer = 0;
    this.el = document.createElement('div');
    this.el.className = 'ml-peek';
    this.el.setAttribute('role', 'tooltip');
    this.el.id = 'ml-peek';
    this.el.hidden = true;
    document.body.append(this.el);

    picker.addEventListener('pointerover', (e) => {
      if (e.pointerType === 'touch') return;
      const chip = e.target instanceof Element ? e.target.closest('.ml-watch-chip') : null;
      if (chip) this.hover(chip);
    });
    picker.addEventListener('pointerout', (e) => {
      const to = e.relatedTarget;
      if (to instanceof Node && this.chip && this.chip.contains(to)) return;
      this.scheduleClose();
    });
    // 키보드로 칩에 오면 바로(영상 없이도) 보여 준다.
    picker.addEventListener('focusin', (e) => {
      const chip = e.target instanceof Element ? e.target.closest('.ml-watch-chip') : null;
      if (chip && chip.matches(':focus-visible')) this.hover(chip, 0);
    });
    picker.addEventListener('focusout', () => this.scheduleClose());
    picker.addEventListener('pointerdown', () => this.close(), true);
    picker.addEventListener('scroll', () => this.close(), { passive: true });
    window.addEventListener('blur', () => this.close());
    window.addEventListener('resize', () => this.close());
    document.addEventListener('visibilitychange', () => { if (document.hidden) this.close(); });
  }

  hover(chip, delay = OPEN_MS) {
    clearTimeout(this.closeTimer);
    if (chip === this.chip) return;
    clearTimeout(this.openTimer);
    if (chip.classList.contains('active')) { this.close(); return; }
    if (this.chip) this.chip.removeAttribute('aria-describedby');
    this.chip = chip;
    // 이미 열려 있으면 훑어보기 — 기다리지 않고 옮긴다.
    if (!this.el.hidden || delay === 0) this.show(chip);
    else this.openTimer = setTimeout(() => this.show(chip), delay);
  }

  scheduleClose() {
    clearTimeout(this.openTimer);
    clearTimeout(this.closeTimer);
    this.closeTimer = setTimeout(() => this.close(), CLOSE_MS);
  }

  show(chip) {
    const s = chip.isConnected && this.picker.offsetParent !== null ? this.sessionOf(chip.dataset.sessionId) : null;
    if (!s) { this.close(); return; }
    const thumb = safeUrl(s.thumbnailUrl);
    const avatar = safeUrl(s.profileImageUrl);
    const name = s.channelName || s.channelId;
    const title = s.liveTitle || '';
    const viewers = s.viewerCountText ? `<span class="ml-peek-viewers">${VIEWERS_ICON}${esc(s.viewerCountText)}명</span>` : '';
    this.el.innerHTML = `
      <div class="ml-peek-thumb">
        ${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" referrerpolicy="no-referrer">` : `<div class="ml-peek-blank">${avatar ? `<img src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : ''}</div>`}
        <div class="ml-peek-badges"><span class="ml-peek-live">LIVE</span>${viewers}</div>
      </div>
      <div class="ml-peek-body">
        <div class="ml-peek-title">${esc(title || name)}</div>
        <div class="ml-peek-name">${avatar ? `<img src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : ''}<span class="ellipsis">${esc(name)}</span>${verifiedBadge(s.verifiedBadge, 'sm')}<em>클릭해서 크게 보기</em></div>
      </div>`;
    chip.setAttribute('aria-describedby', this.el.id);
    const wasHidden = this.el.hidden;
    this.el.hidden = false;
    this.place(chip);
    if (wasHidden) {
      // 처음 열 때만 떠오르는 움직임 — 옆 칩으로 옮길 때는 자리만 미끄러진다.
      this.el.classList.remove('in');
      void this.el.offsetWidth;
      this.el.classList.add('in');
    }
    previewInto(this.el.querySelector('.ml-peek-thumb'), 'live', s.channelId);
  }

  /** 칩 바로 아래 가운데 — 창 밖으로 나가면 안쪽으로 밀고, 꼬리는 칩 가운데를 계속 가리킨다. */
  place(chip) {
    const r = chip.getBoundingClientRect();
    const w = Math.min(WIDTH, window.innerWidth - MARGIN * 2);
    const mid = r.left + r.width / 2;
    const left = Math.max(MARGIN, Math.min(mid - w / 2, window.innerWidth - MARGIN - w));
    this.el.style.width = `${w}px`;
    this.el.style.left = `${Math.round(left)}px`;
    this.el.style.top = `${Math.round(r.bottom + 10)}px`;
    this.el.style.setProperty('--tail-x', `${Math.round(Math.max(18, Math.min(w - 18, mid - left)))}px`);
  }

  close() {
    clearTimeout(this.openTimer);
    clearTimeout(this.closeTimer);
    const thumb = this.el.querySelector('.ml-peek-thumb');
    if (thumb) closePreviewIn(thumb);
    if (this.chip) this.chip.removeAttribute('aria-describedby');
    this.chip = null;
    if (this.el.hidden) return;
    this.el.hidden = true;
    this.el.classList.remove('in');
    this.el.innerHTML = '';
  }
}
