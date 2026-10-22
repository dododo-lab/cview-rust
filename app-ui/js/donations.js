// 후원·구독 표시 — 원본 ChatEventCard / ChatDonationRankingHeader / StreamAlertItem.
//
// - 채팅 줄: 후원은 종류(채팅·영상·미션)와 치즈 금액, 금액 구간별 강조. 구독은 개월 수와 티어.
// - 주간 후원 랭킹: 채팅 패널 위 칩 줄(`chat_donation_ranking`).
// - 스트림 알림: 시청 화면 위에 새 후원·구독을 잠깐 띄운다(설정에서 끌 수 있음, 기본 켬).
//   떠 있는 알림을 끌면 그 자리로 옮기고(영상 크기 대비 비율로 저장), 더블클릭하면 기본 위치로 돌린다.
import { api, invoke } from './api.js';
import { esc, safeUrl, imageSrc, toast } from './dom.js';

const ALERT_KEY = 'cview.streamAlerts';
const ALERT_MS = 6500;
const ALERT_MAX = 3;
/** 기본 위치 — 설정 기본값(streamAlertAnchorX/Y)과 같다. */
export const ALERT_HOME = { x: 0.5, y: 0.08 };

export const ICON_CHEESE = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 15.5 12.6 4a1 1 0 0 1 1.4-.1l6.6 6.1c.3.2.4.5.4.8v6.7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5v-2Zm6 1.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm6.5-3a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z"/></svg>';
export const ICON_STAR = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"/></svg>';
const ICON_TROPHY = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 3h10v2h3v3a4 4 0 0 1-4 4h-.3A5 5 0 0 1 13 14.9V18h3v3H8v-3h3v-3.1A5 5 0 0 1 8.3 12H8a4 4 0 0 1-4-4V5h3V3Zm0 4H6v1a2 2 0 0 0 1 1.7V7Zm10 0v2.7A2 2 0 0 0 18 8V7h-1Z"/></svg>';

const KIND_LABEL = { chat: '채팅 후원', video: '영상 후원', mission: '미션 후원', party: '파티 후원' };

const svg = (d) => `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">${d}</svg>`;
/** 이벤트 아이콘 — 원본 SF Symbol(heart/flame/crown/play.rectangle/flag/star/gift/megaphone) 대응. */
export const EVENT_ICONS = {
  heart: svg('<path d="M12 20.3 4.6 13a4.8 4.8 0 0 1 6.8-6.8l.6.6.6-.6A4.8 4.8 0 0 1 19.4 13L12 20.3Z"/>'),
  flame: svg('<path d="M12.5 2.5c.6 3.4 5 5.6 5 10.6a5.5 5.5 0 0 1-11 0c0-2.4 1.1-4 2.4-5.2.3 1.7 1.1 2.8 2.2 3.4-.1-3.4-.2-6.2 1.4-8.8Z"/>'),
  crown: svg('<path d="M3 7.5 7.5 11 12 4.5 16.5 11 21 7.5 19.5 18h-15L3 7.5ZM4.5 19.5h15V21h-15z"/>'),
  play: svg('<path d="M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v13a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 18.5v-13Zm6 3.2v6.6a.6.6 0 0 0 .9.5l5.2-3.3a.6.6 0 0 0 0-1l-5.2-3.3a.6.6 0 0 0-.9.5Z"/>'),
  flag: svg('<path d="M5 3h1.8v1h11.7l-2.2 4.5 2.2 4.5H6.8V21H5V3Z"/>'),
  party: svg('<path d="M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm7.5 0a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2.5 19c0-3 2.9-5.5 6.5-5.5s6.5 2.5 6.5 5.5v1h-13v-1Zm14.5 1v-1c0-1.7-.6-3.2-1.6-4.4.4-.1.7-.1 1.1-.1 3 0 5.5 2 5.5 4.5v1h-5Z"/>'),
  star: svg('<path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"/>'),
  gift: svg('<path d="M4 11h7v10H5a1 1 0 0 1-1-1v-9Zm9 0h7v9a1 1 0 0 1-1 1h-6V11ZM3 7.5A1.5 1.5 0 0 1 4.5 6h3.2A2.8 2.8 0 0 1 12 2.6 2.8 2.8 0 0 1 16.3 6h3.2A1.5 1.5 0 0 1 21 7.5V9a.5.5 0 0 1-.5.5h-7.5V6h-2v3.5H3.5A.5.5 0 0 1 3 9V7.5Zm6.6-1.5H11v-.8A1.2 1.2 0 1 0 9.6 6Zm3.4 0h1.4A1.2 1.2 0 1 0 13 5.2V6Z"/>'),
  megaphone: svg('<path d="M18 4v13l-5.5-3H7.5l1.3 5.1a1 1 0 0 1-1 1.2H6.6a1 1 0 0 1-1-.8L4.2 14A3 3 0 0 1 2 11.2v-1.4a3 3 0 0 1 3-3h7.5L18 4Zm2 4.5a3 3 0 0 1 0 4v-4Z"/>'),
};

/** 치즈 금액 — `1,000치즈`. 잘못된 값이면 빈 문자열. */
export function formatCheese(amount) {
  if (typeof amount !== 'number' || !Number.isFinite(amount) || amount <= 0) return '';
  return `${amount.toLocaleString('ko-KR')}치즈`;
}

/** 강조 구간 — 원본 donationTierData: 1만 미만 1, 5만 미만 2, 그 이상 3. */
export function donationTier(amount) {
  if (amount >= 50000) return 3;
  if (amount >= 10000) return 2;
  return 1;
}

export function donationKindLabel(type) {
  return KIND_LABEL[String(type || '').toLowerCase()] || '후원';
}

/** 구독 기념 — 원본 subscriptionMilestone. */
export function subscriptionMilestone(months) {
  if (months === 3) return '3개월 달성';
  if (months === 6) return '6개월 달성';
  if (months > 0 && months % 12 === 0) return months === 12 ? '1년 달성' : `${months / 12}년 달성`;
  return '';
}

/** 선물 구독인가 — `giftType` 에 GIFT 가 들어 있으면. */
export function isGiftEvent(m) {
  return /GIFT/i.test(String(m.giftType || ''));
}

/**
 * 이벤트 표시 정보 — 원본 ChatEventCard(tone·symbol·title·value·detail) 규칙. 일반 줄이면 `null`. 순수 함수.
 * @returns {{tone:string, tier:number, icon:string, title:string, value:string, detail:string, extra:string}|null}
 */
export function eventMeta(m) {
  if (m.kind === 'donation') {
    const amount = m.donationAmount || 0;
    const tier = donationTier(amount);
    const type = String(m.donationType || 'chat').toLowerCase();
    const tierLabel = ['', '후원', '큰 후원', '대형 후원'][tier];
    const typed = type === 'video' || type === 'mission' || type === 'party';
    return {
      tone: type === 'video' ? 'video' : type === 'mission' ? 'mission' : 'donation',
      tier,
      icon: type === 'video' ? 'play' : type === 'mission' ? 'flag' : type === 'party' ? 'party' : ['', 'heart', 'flame', 'crown'][tier],
      title: typed ? donationKindLabel(type) : tierLabel,
      value: formatCheese(amount) || '후원 메시지',
      detail: typed && tier >= 3 ? tierLabel : '',
      extra: m.missionText ? `미션 · ${m.missionText}` : '',
    };
  }
  if (m.kind === 'subscription') {
    const months = m.subscriptionMonths > 0 ? m.subscriptionMonths : 0;
    if (isGiftEvent(m)) {
      const n = m.giftCount > 0 ? m.giftCount : 0;
      return {
        tone: 'gift', tier: n >= 10 ? 3 : n >= 5 ? 2 : 1, icon: 'gift', title: '구독 선물',
        value: n ? `${n}명에게` : '구독 선물', detail: m.subscriptionTier || '', extra: '',
      };
    }
    return {
      tone: 'subscription', tier: months >= 12 ? 3 : months >= 6 ? 2 : 1, icon: months >= 12 ? 'crown' : 'star', title: '구독',
      value: months ? `${months}개월` : '구독 알림',
      detail: [m.subscriptionTier, subscriptionMilestone(months)].filter(Boolean).join(' · '),
      extra: '',
    };
  }
  return null;
}

/** 후원·구독 카드 HTML — 모든 채팅 화면이 같은 모양을 쓴다(뷰어·멀티라이브·멀티채팅·통합·채팅 창). */
export function eventCardHtml(m, { time = '', badges = '', nick = '', content = '', tag = '' } = {}) {
  const e = eventMeta(m);
  if (!e) return '';
  const anon = m.anonymous || m.nickname === '익명';
  return `<div class="ev-card">
    <div class="ev-head">
      <span class="ev-icon">${EVENT_ICONS[e.icon] || ''}</span>
      <span class="ev-title">${esc(e.title)}</span>
      ${e.detail ? `<span class="ev-detail">${esc(e.detail)}</span>` : ''}
      <span class="ev-value">${esc(e.value)}</span>
    </div>
    <div class="ev-from">${tag}${badges}${nick}${anon ? '<span class="ev-anon">익명</span>' : ''}${time}</div>
    ${e.extra ? `<div class="ev-extra">${esc(e.extra)}</div>` : ''}
    ${content ? `<div class="ev-body">${content}</div>` : ''}
  </div>`;
}

/** 줄 클래스(`ev ev-<tone> ev-tier-N` + 호환용 donation/subscription). */
export function eventClasses(m) {
  const e = eventMeta(m);
  return e ? ['ev', `ev-${e.tone}`, `ev-tier-${e.tier}`, m.kind] : [];
}

export function streamAlertsEnabled() {
  try { return localStorage.getItem(ALERT_KEY) !== '0'; } catch { return true; }
}

export function setStreamAlertsEnabled(on) {
  try { localStorage.setItem(ALERT_KEY, on ? '1' : '0'); } catch { /* 이번 실행만 */ }
}

/** 옛 머리 표시 — 호환용(외부 코드가 쓰면). 새 카드는 eventCardHtml. */
export function eventHead(m) {
  const e = eventMeta(m);
  return e ? { classes: eventClasses(m), head: `<span class="ev-title">${esc(e.title)}</span><span class="ev-value">${esc(e.value)}</span>` } : null;
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
          ${avatar ? `<img loading="lazy" decoding="async" class="avatar" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '<span class="avatar"></span>'}
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

/**
 * 끌어 놓은 알림 위치 — 알림 묶음이 영상 틀 안에 다 들어가게 자른 비율(x 는 가운데, y 는 위 가장자리).
 * 설정이 받는 범위(x 0.05~0.95, y 0~0.95)도 지킨다. 순수 함수.
 */
export function clampAlertAnchor(x, y, boxW, boxH, frameW, frameH) {
  const half = frameW > 0 ? Math.min(0.5, boxW / 2 / frameW) : 0;
  const maxY = frameH > 0 ? Math.max(0, 1 - boxH / frameH) : 0.95;
  const round = (v) => Math.round(v * 1000) / 1000;
  return {
    x: round(Math.min(0.95, 1 - half, Math.max(0.05, half, x))),
    y: round(Math.min(0.95, maxY, Math.max(0, y))),
  };
}

/** 플레이어 위 후원·구독 알림. 알림을 끌어 위치를 바꾼다(모든 시청 화면이 같은 설정을 쓴다). */
export class StreamAlerts {
  constructor(stage) {
    this.el = document.createElement('div');
    this.el.className = 'stream-alerts';
    stage.append(this.el);
    this.drag = null;
    this.el.addEventListener('pointerdown', (e) => this.onDown(e));
    this.el.addEventListener('pointermove', (e) => this.onMove(e));
    this.el.addEventListener('pointerup', (e) => this.onUp(e));
    this.el.addEventListener('pointercancel', () => this.cancelDrag());
    this.el.addEventListener('lostpointercapture', () => this.cancelDrag());
    // 알림 위 클릭·더블클릭은 아래 영상(재생 토글·전체 화면·패인 크게 보기)으로 보내지 않는다.
    // 묶음 자체는 pointer-events 가 없어 여기 오는 건 카드에서 난 것뿐이다.
    for (const type of ['click', 'mousedown']) this.el.addEventListener(type, (e) => e.stopPropagation());
    this.el.addEventListener('dblclick', (e) => {
      e.stopPropagation();
      this.save(ALERT_HOME.x, ALERT_HOME.y, '알림을 기본 위치로 되돌렸습니다');
    });
    this.onKey = (e) => { if (e.key === 'Escape') this.endPreview(); };
  }

  /** 알림을 붙일 영상 틀을 바꾼다(멀티라이브: 채팅을 보여 주는 패인). 같은 틀이면 그대로. */
  attach(frame) {
    if (frame && this.el.parentElement !== frame) {
      this.cancelDrag();
      frame.append(this.el);
    }
  }

  push(m) {
    if (!streamAlertsEnabled()) return;
    const e = eventMeta(m);
    if (!e) return;
    const item = document.createElement('div');
    item.className = `stream-alert ev-${e.tone} ev-tier-${e.tier}`;
    item.title = '끌어서 옮기기 · 더블클릭하면 기본 위치';
    item.innerHTML = `
      <span class="stream-alert-icon">${EVENT_ICONS[e.icon] || ''}</span>
      <span class="stream-alert-text">
        <span class="stream-alert-kicker">${esc(e.title)}${e.detail ? ` · ${esc(e.detail)}` : ''}</span>
        <span class="stream-alert-title"><span class="stream-alert-nick">${esc(m.nickname || '익명')}</span><b>${esc(e.value)}</b></span>
        ${e.extra ? `<span class="stream-alert-msg">${esc(e.extra)}</span>` : ''}
        ${m.content ? `<span class="stream-alert-msg">${esc(m.content)}</span>` : ''}
      </span>`;
    this.show(item, ALERT_MS);
  }

  /** 위치 맞추기용 견본 알림 — 끌어 놓거나 Esc 를 누르면(또는 잠시 그대로 두면) 사라진다. */
  preview() {
    this.endPreview();
    const item = document.createElement('div');
    item.className = 'stream-alert ev-donation ev-tier-1 alert-ghost';
    item.innerHTML = `
      <span class="stream-alert-icon">${EVENT_ICONS.heart}</span>
      <span class="stream-alert-text">
        <span class="stream-alert-kicker">후원 알림 위치</span>
        <span class="stream-alert-title"><span class="stream-alert-nick">끌어서 옮기세요</span></span>
        <span class="stream-alert-msg">놓으면 저장 · 더블클릭은 기본 위치 · Esc 닫기</span>
      </span>`;
    this.ghost = item;
    document.addEventListener('keydown', this.onKey);
    this.show(item, 15000);
  }

  endPreview() {
    if (!this.ghost) return;
    this.ghost.remove();
    this.ghost = null;
    document.removeEventListener('keydown', this.onKey);
  }

  show(item, ms) {
    this.el.append(item);
    while (this.el.children.length > ALERT_MAX) {
      const old = this.el.firstElementChild;
      if (this.drag && this.drag.card === old) this.cancelDrag(); // 떼어 낸 카드는 놓기 이벤트를 못 받는다
      old.remove();
    }
    // 끄는 중이거나 마우스를 올려 둔 알림은 놓을 때까지 남겨 둔다.
    const leave = () => {
      if (!item.isConnected) return;
      if (this.drag || item.matches(':hover')) { setTimeout(leave, 800); return; }
      item.classList.add('leaving');
      setTimeout(() => { item.remove(); if (item === this.ghost) this.endPreview(); }, 400);
    };
    setTimeout(leave, ms);
  }

  /** 지금 묶음 위치(비율) — 화면에 그려진 자리에서 읽는다(설정값·창 크기와 무관하게 정확). */
  anchorNow() {
    const frame = this.el.parentElement.getBoundingClientRect();
    const box = this.el.getBoundingClientRect();
    return {
      frame, box,
      x: (box.left + box.width / 2 - frame.left) / frame.width,
      y: (box.top - frame.top) / frame.height,
    };
  }

  onDown(e) {
    const card = e.target.closest('.stream-alert');
    if (e.button !== 0 || !card) return;
    e.preventDefault();
    e.stopPropagation();
    const { frame, box, x, y } = this.anchorNow();
    if (!frame.width || !frame.height) return;
    this.drag = { id: e.pointerId, card, sx: e.clientX, sy: e.clientY, x, y, frame, box, moved: false };
    // 카드에 잡는다 — 묶음에 잡으면 click·dblclick 대상이 묶음이 되어 카드 판정이 어긋난다.
    card.setPointerCapture(e.pointerId);
    this.el.classList.add('dragging');
  }

  onMove(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.id) return;
    const dx = e.clientX - d.sx;
    const dy = e.clientY - d.sy;
    if (!d.moved && Math.hypot(dx, dy) < 4) return;
    d.moved = true;
    const p = clampAlertAnchor(d.x + dx / d.frame.width, d.y + dy / d.frame.height, d.box.width, d.box.height, d.frame.width, d.frame.height);
    d.at = p;
    this.el.style.setProperty('--alert-x', String(p.x));
    this.el.style.setProperty('--alert-y', String(p.y));
  }

  onUp(e) {
    const d = this.drag;
    if (!d || e.pointerId !== d.id) return;
    this.drag = null;
    this.el.classList.remove('dragging');
    if (e.target.hasPointerCapture?.(e.pointerId)) e.target.releasePointerCapture(e.pointerId);
    if (d.moved && d.at) {
      this.endPreview();
      this.save(d.at.x, d.at.y, '알림 위치를 저장했습니다');
    }
  }

  cancelDrag() {
    if (!this.drag) return;
    const moved = this.drag.moved;
    this.drag = null;
    this.el.classList.remove('dragging');
    if (moved) this.resetInline();
  }

  resetInline() {
    this.el.style.removeProperty('--alert-x');
    this.el.style.removeProperty('--alert-y');
  }

  /** 위치 저장 — 설정(player.streamAlertAnchorX/Y)에 남기고 모든 화면에 알린다. */
  async save(x, y, message) {
    this.el.style.setProperty('--alert-x', String(x));
    this.el.style.setProperty('--alert-y', String(y));
    try {
      const settings = await api.settingsUpdate('player', { streamAlertAnchorX: x, streamAlertAnchorY: y });
      window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain: 'player', settings } }));
      toast(message, 1500);
    } catch (er) {
      toast(er.message || String(er));
    }
    // 루트 CSS 변수(prefs-effects)가 새 값을 가지므로 임시 값은 걷는다(실패하면 원래 자리로).
    this.resetInline();
  }

  clear() {
    this.cancelDrag();
    this.endPreview();
    this.el.innerHTML = '';
  }
}
