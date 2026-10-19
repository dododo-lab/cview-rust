// 후원·구독 표시 — 원본 ChatEventCard / ChatDonationRankingHeader / StreamAlertItem.
//
// - 채팅 줄: 후원은 종류(채팅·영상·미션)와 치즈 금액, 금액 구간별 강조. 구독은 개월 수와 티어.
// - 주간 후원 랭킹: 채팅 패널 위 칩 줄(`chat_donation_ranking`).
// - 스트림 알림: 시청 화면 위에 새 후원·구독을 잠깐 띄운다(설정에서 끌 수 있음, 기본 켬).
import { invoke } from './api.js';
import { esc, safeUrl } from './dom.js';

const ALERT_KEY = 'cview.streamAlerts';
const ALERT_MS = 6500;
const ALERT_MAX = 3;

export const ICON_CHEESE = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 15.5 12.6 4a1 1 0 0 1 1.4-.1l6.6 6.1c.3.2.4.5.4.8v6.7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5v-2Zm6 1.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm6.5-3a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z"/></svg>';
export const ICON_STAR = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"/></svg>';
const ICON_TROPHY = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 3h10v2h3v3a4 4 0 0 1-4 4h-.3A5 5 0 0 1 13 14.9V18h3v3H8v-3h3v-3.1A5 5 0 0 1 8.3 12H8a4 4 0 0 1-4-4V5h3V3Zm0 4H6v1a2 2 0 0 0 1 1.7V7Zm10 0v2.7A2 2 0 0 0 18 8V7h-1Z"/></svg>';

const KIND_LABEL = { chat: '채팅 후원', video: '영상 후원', mission: '미션 후원', party: '파티 후원' };

/** 치즈 금액 — `1,000치즈`. 잘못된 값이면 빈 문자열. */
export function formatCheese(amount) {
  if (typeof amount !== 'number' || !Number.isFinite(amount) || amount <= 0) return '';
  return `${amount.toLocaleString('ko-KR')}치즈`;
}

/** 강조 구간 — 1만 이상 2, 10만 이상 3. */
export function donationTier(amount) {
  if (amount >= 100000) return 3;
  if (amount >= 10000) return 2;
  return 1;
}

export function donationKindLabel(type) {
  return KIND_LABEL[String(type || '').toLowerCase()] || '후원';
}

export function streamAlertsEnabled() {
  try { return localStorage.getItem(ALERT_KEY) !== '0'; } catch { return true; }
}

export function setStreamAlertsEnabled(on) {
  try { localStorage.setItem(ALERT_KEY, on ? '1' : '0'); } catch { /* 이번 실행만 */ }
}

/** 후원·구독 줄의 머리(종류 배지·금액). 일반 줄이면 `null`. */
export function eventHead(m) {
  if (m.kind === 'donation') {
    const amount = formatCheese(m.donationAmount);
    return {
      classes: ['donation', `tier-${donationTier(m.donationAmount || 0)}`],
      head: `<span class="event-kind">${ICON_CHEESE}${esc(donationKindLabel(m.donationType))}</span>${amount ? `<span class="amount">${esc(amount)}</span>` : ''}`,
    };
  }
  if (m.kind === 'subscription') {
    const months = m.subscriptionMonths ? `${m.subscriptionMonths}개월` : '';
    return {
      classes: ['subscription'],
      head: `<span class="event-kind">${ICON_STAR}구독</span>${months ? `<span class="amount">${esc(months)}</span>` : ''}${m.subscriptionTier ? `<span class="event-tier">${esc(m.subscriptionTier)}</span>` : ''}`,
    };
  }
  return null;
}

/** 주간 후원 랭킹 칩 줄. */
export class DonationRanking {
  constructor(chatRoot) {
    this.el = document.createElement('div');
    this.el.className = 'donation-rank';
    this.el.hidden = true;
    chatRoot.querySelector('.chat-header').after(this.el);
    this.seq = 0;
  }

  async load(channelId) {
    const seq = ++this.seq;
    this.el.hidden = true;
    if (!channelId) return;
    let list = [];
    try {
      list = await invoke('chat_donation_ranking', { channelId, count: 10 });
    } catch { /* 랭킹 비공개·미지원 채널 */ }
    if (seq !== this.seq || !list.length) return;
    this.el.innerHTML = `
      <div class="donation-rank-head">${ICON_TROPHY}이번 주 후원 랭킹</div>
      <div class="donation-rank-list">${list.map((r) => {
        const avatar = safeUrl(r.profileImageUrl);
        return `<span class="rank-chip ${r.rank === 1 ? 'top1' : ''}" title="${esc(r.nickname)} · ${esc(formatCheese(r.amount))}">
          <span class="rank-no">${esc(r.rank)}</span>
          ${avatar ? `<img class="avatar" src="${esc(avatar)}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
          <span class="rank-name">${esc(r.nickname)}</span>
          <span class="rank-amount">${esc((r.amount || 0).toLocaleString('ko-KR'))}</span>
        </span>`;
      }).join('')}</div>`;
    this.el.hidden = false;
  }

  clear() {
    this.seq += 1;
    this.el.hidden = true;
  }
}

/** 플레이어 위 후원·구독 알림. */
export class StreamAlerts {
  constructor(stage) {
    this.el = document.createElement('div');
    this.el.className = 'stream-alerts';
    stage.append(this.el);
  }

  push(m) {
    if (!streamAlertsEnabled()) return;
    const isSub = m.kind === 'subscription';
    const what = isSub
      ? `<b>${esc(m.subscriptionMonths ? `${m.subscriptionMonths}개월` : '')}</b> 구독`
      : `<b>${esc(formatCheese(m.donationAmount))}</b> ${esc(donationKindLabel(m.donationType))}`;
    const item = document.createElement('div');
    item.className = `stream-alert ${isSub ? 'subscription' : 'donation'}`;
    item.innerHTML = `
      <span class="stream-alert-icon">${isSub ? ICON_STAR : ICON_CHEESE}</span>
      <span class="stream-alert-text">
        <span class="stream-alert-title">${esc(m.nickname || '익명')}님이 ${what}</span>
        ${m.content ? `<span class="stream-alert-msg">${esc(m.content)}</span>` : ''}
      </span>`;
    this.el.append(item);
    while (this.el.children.length > ALERT_MAX) this.el.firstElementChild.remove();
    setTimeout(() => {
      item.classList.add('leaving');
      setTimeout(() => item.remove(), 400);
    }, ALERT_MS);
  }

  clear() {
    this.el.innerHTML = '';
  }
}
