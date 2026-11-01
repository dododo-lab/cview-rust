// 팔로잉 "전체 채널" 정보 — 팔로우 날짜·라이브 알림·인증(팔로잉 목록 응답 personalData·verifiedMark),
// 팔로워 수(채널 정보)와 마지막 방송(종료된 채널의 live-detail closeDate)을 모아 보여 준다.
// - 순수 함수(날짜 해석·정렬·요약)는 위에, 화면에 보이는 행만 천천히 채우는 FollowExtras 는 아래.
// - 팔로워 수·마지막 방송은 채널마다 요청이 하나씩이라 보이는 행만, 동시에 2개씩, 앱 실행 중 10분 기억한다.
import { api } from './api.js';
import { esc } from './dom.js';
import { compactCount } from './cards.js';
import { verifiedBadge } from './verified-badge.js';

const DAY = 24 * 60 * 60 * 1000;

/** 팔로우 날짜 원문(`2024-02-19 23:28:11`, KST) → ms. 시간대가 없으면 KST 로 본다. 모르면 NaN. */
export function parseFollowDate(raw) {
  const s = String(raw || '').trim();
  if (!s) return NaN;
  const iso = s.replace(' ', 'T');
  return Date.parse(/([zZ]|[+-]\d\d:?\d\d)$/.test(iso) ? iso : `${iso}+09:00`);
}

/** "오늘" / "3일 전" / "5개월 전" / "2년 전". */
export function agoText(t, now = Date.now()) {
  if (!Number.isFinite(t)) return '';
  const days = Math.floor((now - t) / DAY);
  if (days < 1) return '오늘';
  if (days < 31) return `${days}일 전`;
  if (days < 365) return `${Math.floor(days / 30.44)}개월 전`;
  return `${Math.floor(days / 365.25)}년 전`;
}

/** 팔로우 기간 — "팔로우 12일째" / "팔로우 3개월째" / "팔로우 2년째". */
export function followAgeText(t, now = Date.now()) {
  if (!Number.isFinite(t)) return '';
  const days = Math.max(0, Math.floor((now - t) / DAY));
  if (days < 31) return `팔로우 ${days + 1}일째`;
  if (days < 365) return `팔로우 ${Math.floor(days / 30.44)}개월째`;
  return `팔로우 ${Math.floor(days / 365.25)}년째`;
}

export const FOLLOW_SORTS = [
  { id: 'live', label: '방송 중 먼저' },
  { id: 'name', label: '이름순' },
  { id: 'newest', label: '최근 팔로우' },
  { id: 'oldest', label: '오래된 팔로우' },
];

export const FOLLOW_FILTERS = [
  { id: 'all', label: '전체' },
  { id: 'live', label: '방송 중' },
  { id: 'offline', label: '오프라인' },
  { id: 'notify', label: '알림 켬' },
];

/** 필터·검색·정렬 — 순수 함수. 팔로우 날짜를 모르는 채널은 날짜순에서 맨 뒤. */
export function arrangeFollowed(channels, { sort = 'live', filter = 'all', query = '' } = {}) {
  const q = query.trim().toLowerCase();
  const list = channels.filter((c) => {
    if (filter === 'live' && !c.isLive) return false;
    if (filter === 'offline' && c.isLive) return false;
    if (filter === 'notify' && c.notification !== true) return false;
    return !q || String(c.channelName || '').toLowerCase().includes(q);
  });
  const name = (a, b) => String(a.channelName).localeCompare(String(b.channelName), 'ko');
  const date = (c) => parseFollowDate(c.followDate);
  list.sort((a, b) => {
    if (sort === 'name') return name(a, b);
    if (sort === 'newest' || sort === 'oldest') {
      const da = date(a), db = date(b);
      if (Number.isFinite(da) !== Number.isFinite(db)) return Number.isFinite(da) ? -1 : 1;
      if (Number.isFinite(da) && da !== db) return sort === 'newest' ? db - da : da - db;
      return name(a, b);
    }
    return (b.isLive - a.isLive) || ((b.viewerCount || 0) - (a.viewerCount || 0)) || name(a, b);
  });
  return list;
}

/** 요약 — 팔로우·방송 중·알림·인증 수, 가장 오래된 팔로우, 최근 30일 새 팔로우. 응답에 없는 값은 null. */
export function followedSummary(channels, now = Date.now()) {
  const dated = channels.map((c) => ({ c, t: parseFollowDate(c.followDate) })).filter((x) => Number.isFinite(x.t));
  const oldest = dated.reduce((a, x) => (!a || x.t < a.t ? x : a), null);
  const hasNotify = channels.some((c) => typeof c.notification === 'boolean');
  return {
    total: channels.length,
    live: channels.filter((c) => c.isLive).length,
    notify: hasNotify ? channels.filter((c) => c.notification === true).length : null,
    verified: channels.filter((c) => c.verified).length,
    oldest: oldest ? { name: oldest.c.channelName, t: oldest.t } : null,
    recent30: dated.length ? dated.filter((x) => now - x.t < 30 * DAY).length : null,
  };
}

const ICON_BELL = '<svg class="fx-bell" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="라이브 알림 켬"><path d="M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15z"/><path d="M10 20.5a2 2 0 0 0 4 0"/></svg>';

/** 이름 옆 표시 — 인증 마크(파트너·챌린저)·알림 종. */
export function followBadges(c) {
  return `${verifiedBadge(c)}${c.notification === true ? `<span title="라이브 알림을 켠 채널">${ICON_BELL}</span>` : ''}`;
}

/** 요약 레일 HTML. */
export function followedInsights(channels, now = Date.now()) {
  if (!channels.length) return '';
  const s = followedSummary(channels, now);
  const item = (label, value, title = '') => `<div class="insight"${title ? ` title="${esc(title)}"` : ''}><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`;
  return `<div class="insight-rail">
    ${item('팔로우', `${s.total}개`)}
    ${item('방송 중', `${s.live}개`)}
    ${s.notify != null ? item('알림 켬', `${s.notify}개`) : ''}
    ${s.verified ? item('인증 채널', `${s.verified}개`) : ''}
    ${s.recent30 != null ? item('최근 30일 새 팔로우', `${s.recent30}개`) : ''}
    ${s.oldest ? item('가장 오래된 팔로우', `${s.oldest.name} · ${followAgeText(s.oldest.t, now).replace('팔로우 ', '')}`, new Date(s.oldest.t).toLocaleDateString('ko-KR')) : ''}
  </div>`;
}

// ── 보이는 행만 채우는 팔로워 수·마지막 방송 ──────────────
const TTL = 10 * 60 * 1000;
const CONCURRENCY = 2;
const extras = new Map(); // channelId → { at, followersText, lastClose, lastTitle, lastCategory }

export class FollowExtras {
  constructor() {
    this.queue = [];
    this.running = 0;
    this.pending = new Set();
    this.observer = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        this.observer.unobserve(e.target);
        this.want(e.target.dataset.fid, e.target.dataset.live === '1');
      }
    }, { rootMargin: '200px 0px' });
  }

  /** 새로 그린 목록의 행을 지켜본다 — 이미 아는 값은 바로 채운다. */
  attach(list) {
    this.observer.disconnect();
    this.queue = [];
    if (!list) return;
    for (const row of list.querySelectorAll('[data-fid]')) {
      const known = extras.get(row.dataset.fid);
      if (known && Date.now() - known.at < TTL) fill(row, known);
      else this.observer.observe(row);
    }
  }

  want(id, live) {
    if (!id || this.pending.has(id)) return;
    this.queue.push({ id, live });
    this.pump();
  }

  pump() {
    while (this.running < CONCURRENCY && this.queue.length) {
      const job = this.queue.shift();
      this.pending.add(job.id);
      this.running += 1;
      load(job).then((x) => {
        extras.set(job.id, x);
        for (const row of document.querySelectorAll(`[data-fid="${CSS.escape(job.id)}"]`)) fill(row, x);
      }).finally(() => {
        this.pending.delete(job.id);
        this.running -= 1;
        this.pump();
      });
    }
  }
}

async function load({ id, live }) {
  const x = { at: Date.now(), followersText: '', lastClose: '', lastTitle: '', lastCategory: '' };
  const info = await api.channelInfo(id).catch(() => null);
  if (info && info.followerCount > 0) x.followersText = info.followerCountText || compactCount(info.followerCount);
  if (!live) {
    const d = await api.liveDetail(id, false).catch(() => null);
    if (d && !d.isLive) {
      x.lastClose = d.closeDate || '';
      x.lastTitle = d.liveTitle || '';
      x.lastCategory = d.categoryName || '';
    }
  }
  return x;
}

function fill(row, x) {
  const f = row.querySelector('.fx-followers');
  if (f) f.textContent = x.followersText ? `팔로워 ${x.followersText}` : '';
  const last = row.querySelector('.fx-last');
  if (!last) return;
  const t = Date.parse(x.lastClose || '');
  const when = Number.isFinite(t) ? `마지막 방송 ${agoText(t)}` : '';
  const parts = [when, x.lastCategory, x.lastTitle].filter(Boolean);
  last.textContent = parts.join(' · ');
  last.title = Number.isFinite(t) ? `마지막 방송 ${new Date(t).toLocaleString('ko-KR')}` : '';
}
