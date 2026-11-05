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
 * 돌려주는 값: { count, countText, unit, metrics: [[이름, 값]], items: [{ thumb, shape, title, meta }], empty, hint }
 */
export function summarizeSection(tab, data, now = Date.now()) {
  const list = Array.isArray(data) ? data : [];
  const base = { count: list.length, countText: String(list.length), unit: '개', metrics: [], items: [], empty: '', hint: '' };
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
        empty: '클립이 없습니다', hint: '기간을 바꾸면 다른 클립을 볼 수 있어요',
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
        empty: '최근 다시보기가 없습니다', hint: '팔로우한 채널이 다시보기를 올리면 여기에 모입니다',
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
        hint: tab === 'continue' ? '다시보기를 보다 멈추면 여기에 남습니다' : '다시보기 화면의 "나중에 보기"로 담을 수 있어요',
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
        hint: tab === 'favorites' ? '채널 화면의 ☆ 버튼으로 추가할 수 있어요' : '라이브를 보면 채널이 여기에 남습니다',
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
        empty: '북마크가 없습니다', hint: '시청 중 북마크 버튼(또는 B 키)으로 지금 장면을 남길 수 있어요',
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
        empty: '받은 파일이 없습니다', hint: '다시보기·클립 화면의 받기 버튼으로 저장할 수 있어요',
      };
    }
    default: return base;
  }
}

const ic = (d) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
/** 탭마다 카드 머리 아이콘. */
export const SECTION_ICONS = {
  clips: ic('<rect x="6" y="3" width="12" height="18" rx="3"/><path d="m10.5 9.5 4 2.5-4 2.5z"/>'),
  vods: ic('<rect x="3" y="5" width="18" height="13" rx="3"/><path d="m10.5 9 4 2.5-4 2.5zM8 21h8"/>'),
  continue: ic('<circle cx="12" cy="12" r="8.5"/><path d="m10.5 9 4 3-4 3z"/>'),
  later: ic('<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>'),
  favorites: ic('<path d="m12 3.8 2.5 5.1 5.6.8-4 3.9.9 5.6-5-2.6-5 2.6.9-5.6-4-3.9 5.6-.8z"/>'),
  recent: ic('<path d="M3.5 12a8.5 8.5 0 1 0 2.5-6M3.5 4.5V8H7"/><path d="M12 8v4.2l2.8 1.8"/>'),
  bookmarks: ic('<path d="M7 3.5h10v17l-5-3.6-5 3.6z"/>'),
  downloads: ic('<path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14"/>'),
};
const CHEVRON = '<svg class="lib-ov-chev" viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';

function itemRow(it) {
  const thumb = safeUrl(it.thumb);
  const kind = it.shape === 'clip' ? 'clip' : it.shape === 'avatar' ? 'avatar' : 'vod';
  const media = it.shape === 'kind'
    ? `<span class="lib-ov-kind">${esc(it.kind)}</span>`
    : `<span class="lib-ov-thumb ${it.shape}">${thumb ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(thumb, kind))}" alt="" referrerpolicy="no-referrer">` : ''}${it.progress ? `<i style="width:${it.progress}%"></i>` : ''}</span>`;
  return `<li class="lib-ov-item">${media}<span class="lib-ov-item-text"><span class="lib-ov-item-title ellipsis" title="${esc(it.title || '')}">${esc(it.title || '')}</span><span class="lib-ov-item-meta ellipsis">${esc(it.meta || '')}</span></span></li>`;
}

const SKELETON = `<div class="lib-ov-metrics sk-row" aria-hidden="true"><span class="sk"></span><span class="sk"></span></div>`
  + `<ul class="lib-ov-items" aria-hidden="true">${'<li class="lib-ov-item"><span class="lib-ov-thumb wide sk"></span><span class="lib-ov-item-text"><span class="sk line"></span><span class="sk line short"></span></span></li>'.repeat(OVERVIEW_ITEMS)}</ul>`;

/**
 * 개요 카드 하나. state.status: 'loading' | 'ok' | 'error' | 'login'.
 * 모양은 통계 카드(stats-card)와 같은 틀 — 머리(아이콘 · 제목 › · 개수), 핵심 수치 두 칸(kpi), 최근 항목 세 줄.
 * 항목 자리는 늘 세 줄만큼 잡아 같은 줄 카드 높이가 맞고, 비었을 때는 그 자리 가운데에 안내를 둔다.
 * 카드 전체(빈 곳)를 누르거나 제목 단추를 누르면 data-ov-go 의 탭으로 간다. 안의 다른 단추(다시 시도·로그인)는 제 할 일을 한다.
 */
export function overviewCard(tab, label, state = { status: 'loading' }) {
  const { status } = state;
  const s = status === 'ok' ? state.summary : null;
  let count = '<span class="lib-ov-count muted" aria-hidden="true">—</span>';
  if (s) count = `<span class="lib-ov-count${s.count ? '' : ' zero'}"><strong>${esc(s.countText)}</strong>${esc(s.unit)}</span>`;
  const icon = SECTION_ICONS[tab] || '';
  const note = (cls, title, sub, action = '') => `<div class="lib-ov-note${cls}"${cls === ' error' ? ' role="alert"' : ''}><span class="lib-ov-note-icon">${icon}</span><span class="lib-ov-note-text"><strong>${esc(title)}</strong>${sub ? `<small>${esc(sub)}</small>` : ''}</span>${action}</div>`;
  let body;
  if (status === 'loading') body = SKELETON;
  else if (status === 'login') body = note('', '로그인이 필요합니다', '로그인하면 팔로우한 채널의 다시보기를 모아 보여 드립니다', '<button class="btn primary" data-action="login">로그인</button>');
  else if (status === 'error') body = note(' error', friendlyError(state.error), '', `<button class="btn" data-ov-retry="${esc(tab)}">다시 시도</button>`);
  else if (!s.count && !s.items.length) body = note(' empty', s.empty, s.hint);
  else {
    body = `${s.metrics.length ? `<dl class="lib-ov-metrics">${s.metrics.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl>` : ''}`
      + `<ul class="lib-ov-items">${s.items.map(itemRow).join('')}</ul>`;
  }
  return `
    <article class="lib-ov-card" data-ov-go="${esc(tab)}" data-ov-section="${esc(tab)}" data-status="${esc(status)}"${status === 'loading' ? ' aria-busy="true"' : ''}>
      <header class="lib-ov-head">
        <span class="lib-ov-icon">${icon}</span>
        <button class="lib-ov-title" data-ov-go="${esc(tab)}" aria-label="${esc(label)} 탭 열기"><span>${esc(label)}</span>${CHEVRON}</button>
        ${count}
      </header>
      ${body}
    </article>`;
}

/** 개요 묶음 — 탭 순서 그대로 넷씩 나눠 홈·검색처럼 구역 제목 아래 카드 격자로 둔다. */
export const OVERVIEW_GROUPS = [
  ['media', '영상', '클립 · 다시보기 · 보다 만 영상', ['clips', 'vods', 'continue', 'later']],
  ['saved', '채널과 저장한 것', '즐겨찾기 · 최근 시청 · 북마크 · 다운로드', ['favorites', 'recent', 'bookmarks', 'downloads']],
];

/** 개요 화면 틀(모든 카드를 로딩 상태로). labels: Map(탭 → 이름). */
export function overviewLayout(labels) {
  return `<div class="lib-ov">${OVERVIEW_GROUPS.map(([id, title, sub, tabs]) => `
    <section class="lib-ov-group" data-ov-group="${esc(id)}" aria-label="${esc(title)}">
      <div class="section-head"><div class="section-titles"><h2 class="section-title">${esc(title)}</h2><span class="section-sub">${esc(sub)}</span></div></div>
      <div class="lib-ov-grid">${tabs.map((t) => overviewCard(t, labels.get(t) || t)).join('')}</div>
    </section>`).join('')}</div>`;
}
