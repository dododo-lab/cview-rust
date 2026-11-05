// 보관함 "개요" 탭 — 다른 탭 전부를 카드 하나씩으로 요약한다(개수 · 핵심 수치 · 최근 항목 몇 개).
// 카드를 누르면 그 탭으로 간다. 요약은 순수 함수(summarizeSection)로 계산하고, 카드 마크업은 overviewCard 가 만든다.
import { esc, safeUrl, imageSrc } from './dom.js';
import { compactCount, friendlyError } from './cards.js';
import { relativeDate, formatDuration } from './channel.js';
import { bytesText } from './stats.js';

/** 카드마다 보여 줄 최근 항목 수. */
export const OVERVIEW_ITEMS = 3;
/** 최근 시청 탭이 한 번에 받는 수 — 이만큼 차면 "60+" 로 적는다. */
export const RECENT_LIMIT = 60;

const DAY_MS = 86400000;
const ACTIVE_DL = ['queued', 'preparing', 'downloading', 'assembling'];

const toMs = (v) => {
  if (v == null || v === '') return NaN;
  const t = typeof v === 'number' ? v : Date.parse(v);
  return Number.isFinite(t) ? t : NaN;
};
const iso = (v) => { const t = toMs(v); return Number.isFinite(t) ? new Date(t).toISOString() : ''; };
const when = (v, now) => { const s = iso(v); return s ? relativeDate(s, now) : ''; };
const sum = (list, fn) => list.reduce((a, x) => a + (Number(fn(x)) || 0), 0);
const distinct = (list, fn) => new Set(list.map(fn).filter(Boolean)).size;
/** 초를 "3시간 20분"처럼 — 1분 미만은 "1분 미만". */
export function spanText(sec) {
  const s = Math.max(0, Math.round(Number(sec) || 0));
  if (s < 60) return s ? '1분 미만' : '0분';
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}시간${m ? ` ${m}분` : ''}` : `${m}분`;
}

/**
 * 탭 데이터 → 카드 요약. 순수 함수.
 * 돌려주는 값: { count, countText, unit, metrics: [[이름, 값]], items: [{ thumb, shape, title, meta }], empty }
 */
export function summarizeSection(tab, data, now = Date.now()) {
  const list = Array.isArray(data) ? data : [];
  const base = { count: list.length, countText: String(list.length), unit: '개', metrics: [], items: [], empty: '' };
  switch (tab) {
    case 'clips': {
      const top = list.reduce((m, c) => Math.max(m, Number(c.readCount) || 0), 0);
      return {
        ...base,
        metrics: list.length ? [['최고 조회', compactCount(top)], ['채널', `${distinct(list, (c) => c.channel?.channelId || c.channel?.channelName)}곳`]] : [],
        items: list.slice(0, OVERVIEW_ITEMS).map((c) => ({
          thumb: c.thumbnailImageUrl, shape: 'clip', title: c.clipTitle || '제목 없음',
          meta: `${c.channel?.channelName ? `${c.channel.channelName} · ` : ''}조회 ${compactCount(c.readCount || 0)}`,
        })),
        empty: '클립이 없습니다',
      };
    }
    case 'vods': {
      const sorted = [...list].sort((a, b) => (toMs(b.publishDate) || 0) - (toMs(a.publishDate) || 0));
      const fresh = list.filter((v) => now - toMs(v.publishDate) < DAY_MS).length;
      return {
        ...base,
        metrics: list.length ? [['채널', `${distinct(list, (v) => v.channel?.channelId || v.channel?.channelName)}곳`], ['오늘 올라옴', `${fresh}개`]] : [],
        items: sorted.slice(0, OVERVIEW_ITEMS).map((v) => ({
          thumb: v.thumbnailImageUrl || v.videoImageUrl, shape: 'wide', title: v.videoTitle || '제목 없음',
          meta: `${v.channel?.channelName ? `${v.channel.channelName} · ` : ''}${when(v.publishDate, now)}`,
        })),
        empty: '최근 다시보기가 없습니다',
      };
    }
    case 'continue':
    case 'later': {
      const left = sum(list, (x) => Math.max(0, (Number(x.duration) || 0) - (Number(x.position) || 0)));
      const total = sum(list, (x) => x.duration);
      return {
        ...base,
        metrics: list.length ? (tab === 'continue'
          ? [['남은 시간', spanText(left)], ['평균 진행', `${Math.round(list.reduce((a, x) => a + (x.duration > 0 ? Math.min(1, (x.position || 0) / x.duration) : 0), 0) / list.length * 100)}%`]]
          : [['전체 길이', spanText(total)], ['채널', `${distinct(list, (x) => x.channelName)}곳`]]) : [],
        items: list.slice(0, OVERVIEW_ITEMS).map((x) => ({
          thumb: x.thumbnailUrl, shape: 'wide', title: x.title || '제목 없음',
          meta: `${x.channelName ? `${x.channelName} · ` : ''}${x.position ? `${formatDuration(x.position)}까지 봄` : formatDuration(x.duration)}`,
          progress: x.duration > 0 && x.position ? Math.min(100, Math.round((x.position / x.duration) * 100)) : 0,
        })),
        empty: tab === 'continue' ? '이어 볼 다시보기가 없습니다' : '나중에 볼 다시보기가 없습니다',
      };
    }
    case 'favorites':
    case 'recent': {
      const week = list.filter((c) => now - toMs(c.lastWatched) < 7 * DAY_MS).length;
      const sorted = tab === 'favorites'
        ? [...list].sort((a, b) => (toMs(b.lastWatched) || 0) - (toMs(a.lastWatched) || 0))
        : list;
      const last = sorted.find((c) => Number.isFinite(toMs(c.lastWatched)));
      return {
        ...base,
        unit: '곳',
        countText: tab === 'recent' && list.length >= RECENT_LIMIT ? `${RECENT_LIMIT}+` : String(list.length),
        metrics: list.length ? (tab === 'favorites'
          ? [['7일 안에 봄', `${week}곳`], ['마지막 시청', last ? when(last.lastWatched, now) : '없음']]
          : [['7일 안에 봄', `${week}곳`], ['즐겨찾기', `${list.filter((c) => c.isFavorite).length}곳`]]) : [],
        items: sorted.slice(0, OVERVIEW_ITEMS).map((c) => ({
          thumb: c.channelImageUrl, shape: 'avatar', title: `${c.isFavorite && tab === 'recent' ? '★ ' : ''}${c.channelName || c.channelId || ''}`,
          meta: Number.isFinite(toMs(c.lastWatched)) ? `${when(c.lastWatched, now)} 시청` : '아직 안 봄',
        })),
        empty: tab === 'favorites' ? '즐겨찾기한 채널이 없습니다' : '최근 시청 기록이 없습니다',
      };
    }
    case 'bookmarks': {
      const sorted = [...list].sort((a, b) => (toMs(b.capturedAt) || 0) - (toMs(a.capturedAt) || 0));
      return {
        ...base,
        metrics: list.length ? [['채널', `${distinct(list, (b) => b.channelId || b.channelName)}곳`], ['마지막', when(sorted[0].capturedAt, now) || '—']] : [],
        items: sorted.slice(0, OVERVIEW_ITEMS).map((b) => ({
          thumb: b.thumbnailUrl, shape: 'wide', title: b.liveTitle || '제목 없음',
          meta: `${b.channelName || ''} · 방송 ${formatDuration(b.liveUptime)} 지점`,
        })),
        empty: '북마크가 없습니다',
      };
    }
    case 'downloads': {
      const d = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
      const history = Array.isArray(d.history) ? d.history : [];
      const jobs = Array.isArray(d.jobs) ? d.jobs : [];
      const active = jobs.filter((j) => ACTIVE_DL.includes(j.state));
      const done = history.filter((h) => h.completed && h.fileStatus !== 'missing');
      const failed = history.filter((h) => !h.completed).length;
      const metrics = [];
      if (active.length) metrics.push(['받는 중', `${active.length}개`]);
      if (done.length) metrics.push(['용량', bytesText(sum(done, (h) => h.fileSize))]);
      if (failed) metrics.push(['실패', `${failed}개`]);
      const rows = [
        ...active.map((j) => ({ shape: 'kind', kind: j.kind === 'clip' ? '클립' : 'VOD', title: j.title, meta: `받는 중 · ${Math.round((j.progress || 0) * 100)}%` })),
        ...history.map((h) => ({
          shape: 'kind', kind: String(h.id || '').startsWith('clip-') ? '클립' : 'VOD', title: h.title,
          meta: `${h.channelName ? `${h.channelName} · ` : ''}${h.completed ? (h.fileStatus === 'missing' ? '파일 없음' : bytesText(h.fileSize)) : '실패'}`,
        })),
      ];
      return {
        ...base,
        count: done.length + active.length,
        countText: String(done.length + active.length),
        metrics,
        items: rows.slice(0, OVERVIEW_ITEMS),
        empty: '받은 파일이 없습니다',
      };
    }
    default: return base;
  }
}

function itemRow(it) {
  const thumb = safeUrl(it.thumb);
  const kind = it.shape === 'clip' ? 'clip' : it.shape === 'avatar' ? 'avatar' : 'vod';
  const media = it.shape === 'kind'
    ? `<span class="lib-ov-kind">${esc(it.kind)}</span>`
    : `<span class="lib-ov-thumb ${it.shape}">${thumb ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(thumb, kind))}" alt="" referrerpolicy="no-referrer">` : ''}${it.progress ? `<i style="width:${it.progress}%"></i>` : ''}</span>`;
  return `<li class="lib-ov-item">${media}<span class="lib-ov-item-text"><span class="ellipsis">${esc(it.title || '')}</span><span class="ellipsis">${esc(it.meta || '')}</span></span></li>`;
}

const SKELETON = `<ul class="lib-ov-items" aria-hidden="true">${'<li class="lib-ov-item"><span class="lib-ov-thumb wide sk"></span><span class="lib-ov-item-text"><span class="sk line"></span><span class="sk line short"></span></span></li>'.repeat(2)}</ul>`;

/**
 * 개요 카드 하나. state.status: 'loading' | 'ok' | 'error' | 'login'.
 * 카드 전체(빈 곳)를 누르거나 제목 단추를 누르면 data-ov-go 의 탭으로 간다. 안의 다른 단추(다시 시도·로그인)는 제 할 일을 한다.
 */
export function overviewCard(tab, label, state = { status: 'loading' }) {
  const { status } = state;
  const s = status === 'ok' ? state.summary : null;
  let count = '<span class="lib-ov-count muted" aria-hidden="true">—</span>';
  if (s) count = `<span class="lib-ov-count${s.count ? '' : ' zero'}"><strong>${esc(s.countText)}</strong>${esc(s.unit)}</span>`;
  let body;
  if (status === 'loading') body = SKELETON;
  else if (status === 'login') body = '<div class="lib-ov-note"><span>로그인하면 팔로우한 채널의 다시보기를 모아 보여 드립니다</span><button class="btn primary" data-action="login">로그인</button></div>';
  else if (status === 'error') body = `<div class="lib-ov-note error" role="alert"><span>${esc(friendlyError(state.error))}</span><button class="btn" data-ov-retry="${esc(tab)}">다시 시도</button></div>`;
  else if (!s.count && !s.items.length) body = `<div class="lib-ov-note"><span>${esc(s.empty)}</span></div>`;
  else {
    body = `${s.metrics.length ? `<dl class="lib-ov-metrics">${s.metrics.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl>` : ''}`
      + `<ul class="lib-ov-items">${s.items.map(itemRow).join('')}</ul>`;
  }
  return `
    <article class="lib-ov-card" data-ov-go="${esc(tab)}" data-ov-section="${esc(tab)}" data-status="${esc(status)}"${status === 'loading' ? ' aria-busy="true"' : ''}>
      <header class="lib-ov-head">
        <button class="lib-ov-title" data-ov-go="${esc(tab)}" aria-label="${esc(label)} 탭 열기"><span>${esc(label)}</span><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
        ${count}
      </header>
      ${body}
    </article>`;
}
