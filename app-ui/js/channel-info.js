// 채널 상세 › 정보 탭 — 지금 방송(또는 마지막 방송)·채팅 설정·채널·소개·내 시청 기록·최근 다시보기·인기 클립·메모 카드 격자.
// 채널 상세(channel_detail)로 바로 그릴 수 있는 칸은 먼저 그리고, channel_overview 가 오면 `data-ci` 칸의 속만 바꾼다
// (카드 요소는 그대로 두어 등장 모션이 다시 돌지 않게). 순수 HTML 함수라 회귀 검사가 그대로 부른다.
import { esc, safeUrl, imageSrc } from './dom.js';
import { compactCount, uptimeText } from './cards.js';
import { VERIFIED_MARKS, badgeKind } from './verified-badge.js';

const CATEGORY_TYPES = { GAME: '게임', SPORTS: '스포츠', ETC: '기타', ENTERTAINMENT: '엔터' };

/** 정확한 수 — 1234567 → "1,234,567". */
export function exactCount(n) {
  const v = Number(n);
  return Number.isFinite(v) ? Math.max(0, Math.round(v)).toLocaleString('ko-KR') : '0';
}

/** 시청 시간 — 초 → "1분 미만" · "35분" · "12시간 5분" · "123시간". */
export function watchTimeText(sec) {
  const s = Math.max(0, Number(sec) || 0);
  if (s < 60) return '1분 미만';
  const min = Math.floor(s / 60);
  if (min < 60) return `${min}분`;
  const h = Math.floor(min / 60);
  return h >= 100 || !(min % 60) ? `${h.toLocaleString('ko-KR')}시간` : `${h}시간 ${min % 60}분`;
}

/** 날짜·시각 — "11월 13일 (목) 오후 12:12", 올해가 아니면 연도를 붙인다. 시각 없이도 쓴다. */
export function dateText(iso, { time = true, now = Date.now() } = {}) {
  const t = Date.parse(iso || '');
  if (!Number.isFinite(t)) return '';
  const d = new Date(t);
  const opts = { month: 'long', day: 'numeric', weekday: 'short' };
  if (d.getFullYear() !== new Date(now).getFullYear()) opts.year = 'numeric';
  if (time) Object.assign(opts, { hour: 'numeric', minute: '2-digit' });
  return d.toLocaleString('ko-KR', opts);
}

/** 상대 날짜 — "방금" · "3시간 전" · "어제" · "5일 전" · "2개월 전". */
export function agoText(iso, now = Date.now()) {
  const t = Date.parse(iso || '');
  if (!Number.isFinite(t)) return '';
  const min = Math.max(0, Math.floor((now - t) / 60000));
  if (min < 1) return '방금';
  if (min < 60) return `${min}분 전`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h}시간 전`;
  const d = Math.floor(h / 24);
  if (d === 1) return '어제';
  if (d < 30) return `${d}일 전`;
  const mo = Math.floor(d / 30);
  return mo < 12 ? `${mo}개월 전` : `${Math.floor(mo / 12)}년 전`;
}

/** 방송 길이 — 시작·끝 RFC3339 → "5시간 1분". 모르면 ''. */
export function spanText(open, close) {
  const a = Date.parse(open || '');
  const b = Date.parse(close || '');
  if (!Number.isFinite(a) || !Number.isFinite(b) || b <= a) return '';
  return watchTimeText((b - a) / 1000);
}

/** 채팅 설정 → 사람이 읽을 줄들 [[이름, 값]]. 정보가 없으면 빈 배열. */
export function chatPolicyRows(live) {
  if (!live) return [];
  const rows = [];
  if (live.chatActive === false) return [['채팅', '꺼 둠']];
  const group = String(live.chatAvailableGroup || '').toUpperCase();
  const min = Number(live.minFollowerMinute) || 0;
  const who = {
    ALL: '누구나',
    FOLLOWER: min > 0 ? `팔로우 ${min >= 60 && !(min % 60) ? `${min / 60}시간` : `${min}분`} 지난 팔로워` : '팔로워',
    SUBSCRIBER: '구독자만',
    MANAGER: '운영자만',
  }[group];
  if (who) rows.push(['채팅 가능', who]);
  const cond = String(live.chatAvailableCondition || '').toUpperCase();
  if (cond === 'REAL_NAME') rows.push(['조건', '본인 인증한 사람']);
  else if (cond && cond !== 'NONE') rows.push(['조건', cond]);
  return rows;
}

const URL_RE = /\bhttps?:\/\/[^\s<>"'()]+[^\s<>"'().,!?;:]/gi;
/** 채널 소개 — 글자는 그대로(줄바꿈 유지), http(s) 주소만 바깥 브라우저로 여는 링크로. */
export function linkify(text) {
  const s = String(text || '');
  let out = '';
  let at = 0;
  for (const m of s.matchAll(URL_RE)) {
    out += esc(s.slice(at, m.index));
    const url = safeUrl(m[0]);
    out += url ? `<a class="ci-link" href="${esc(url)}" data-ext title="브라우저에서 열기">${esc(m[0])}</a>` : esc(m[0]);
    at = m.index + m[0].length;
  }
  return out + esc(s.slice(at));
}

const row = (label, value, cls = '') => `<div class="ci-row${cls ? ` ${cls}` : ''}"><dt>${esc(label)}</dt><dd>${value}</dd></div>`;
const skeleton = (n = 3) => `<div class="ci-skel" role="status" aria-label="불러오는 중">${Array.from({ length: n }, (_, i) => `<span class="sk line${i === n - 1 ? ' short' : ''}"></span>`).join('')}</div>`;
const note = (text) => `<p class="ci-note">${esc(text)}</p>`;
const failed = (text) => `<div class="ci-failed" role="alert"><span>${esc(text)}</span><button type="button" class="btn sm" data-ch="overview-retry">다시 시도</button></div>`;
const card = (id, title, body, { wide = false, action = '' } = {}) => `
  <section class="ci-card${wide ? ' ci-wide' : ''}" data-ci="${id}" aria-labelledby="ci-h-${id}">
    <header class="ci-head"><h2 id="ci-h-${id}">${esc(title)}</h2>${action}</header>
    <div class="ci-body">${body}</div>
  </section>`;

function tagChips(live) {
  const cat = live.categoryName ? `<span class="ci-chip cat">${live.categoryType && CATEGORY_TYPES[live.categoryType] ? `${esc(CATEGORY_TYPES[live.categoryType])} · ` : ''}${esc(live.categoryName)}</span>` : '';
  const tags = (live.tags || []).filter(Boolean).slice(0, 12).map((t) => `<span class="ci-chip">#${esc(t)}</span>`).join('');
  return cat || tags ? `<div class="ci-chips">${cat}${tags}</div>` : '';
}

function flags(live) {
  const out = [];
  if (live.adult) out.push('<span class="ci-flag adult">19세</span>');
  if (live.krOnlyViewing) out.push('<span class="ci-flag">국내 전용</span>');
  if (live.timeMachineActive) out.push('<span class="ci-flag">타임머신 가능</span>');
  if (live.clipActive === false) out.push('<span class="ci-flag">클립 막힘</span>');
  return out.join('');
}

/** 지금 방송 카드 속. */
export function liveBody(live, now = Date.now()) {
  const thumb = safeUrl(live.thumbnailUrl);
  const up = uptimeText(live.openDate, now);
  const qualities = (live.qualities || []).map((q) => `<span class="ci-chip q">${esc(q)}</span>`).join('');
  return `
    <div class="ci-live">
      ${thumb ? `<div class="ci-live-thumb"><img src="${esc(imageSrc(thumb, 'live'))}" alt="" loading="lazy" referrerpolicy="no-referrer"><span class="badge-live">LIVE</span></div>` : ''}
      <div class="ci-live-main">
        <p class="ci-title">${esc(live.liveTitle || '제목 없음')}</p>
        ${tagChips(live)}
        <dl class="ci-stats">
          ${row('지금 시청자', `${esc(exactCount(live.viewerCount))}명`)}
          ${live.accumulateCount ? row('누적 시청자', `${esc(exactCount(live.accumulateCount))}명`) : ''}
          ${live.openDate ? row('방송 시작', `${esc(dateText(live.openDate, { now }))}${up ? ` <small>· ${esc(up)}째</small>` : ''}`) : ''}
        </dl>
        ${qualities ? `<div class="ci-quality"><span class="ci-label">화질</span><div class="ci-chips">${qualities}</div></div>` : ''}
        ${flags(live) ? `<div class="ci-flags">${flags(live)}</div>` : ''}
      </div>
    </div>`;
}

/** 마지막 방송 카드 속 — `last` 는 overview.lastLive(없으면 null). */
export function lastLiveBody(last, now = Date.now()) {
  if (!last) return note('최근 방송 기록이 없습니다');
  const span = spanText(last.openDate, last.closeDate);
  return `
    <p class="ci-title">${esc(last.liveTitle || '제목 없음')}</p>
    ${tagChips(last)}
    <dl class="ci-stats">
      ${last.closeDate ? row('방송 끝', `${esc(dateText(last.closeDate, { now }))} <small>· ${esc(agoText(last.closeDate, now))}</small>`) : ''}
      ${last.openDate ? row('방송 시작', esc(dateText(last.openDate, { now }))) : ''}
      ${span ? row('방송 길이', esc(span)) : ''}
      ${last.accumulateCount ? row('누적 시청자', `${esc(exactCount(last.accumulateCount))}명`) : ''}
    </dl>
    ${last.adult ? '<div class="ci-flags"><span class="ci-flag adult">19세</span></div>' : ''}`;
}

/** 채널 카드 속 — 팔로워·인증·구독·(로그인 시) 팔로우 시작일. `follow` 는 overview 가 오기 전이면 undefined. */
export function channelBody(c, follow, now = Date.now()) {
  const kind = badgeKind(c);
  const mark = kind ? esc(VERIFIED_MARKS[kind].label) : '인증 없음';
  let fol = '';
  if (follow === undefined) fol = row('내 팔로우', '<span class="sk line short" aria-label="불러오는 중"></span>');
  else if (follow) {
    fol = follow.following
      ? row('내 팔로우', follow.followDate ? `${esc(dateText(follow.followDate, { time: false, now }))}부터 <small>· ${esc(agoText(follow.followDate, now))}</small>` : '팔로우 중')
      : row('내 팔로우', '팔로우하지 않음');
  }
  return `
    <dl class="ci-stats">
      ${row('팔로워', `${esc(exactCount(c.followerCount))}명 <small>· ${esc(compactCount(c.followerCount || 0))}</small>`)}
      ${row('인증', mark)}
      ${row('구독', c.subscriptionAvailability ? '구독할 수 있음' : '구독 상품 없음')}
      ${fol}
    </dl>`;
}

/** 내 시청 기록 카드 속 — `watch` 는 overview.watch(저장소가 없으면 null). */
export function watchBody(watch, now = Date.now()) {
  if (!watch) return note('시청 기록을 읽을 수 없습니다');
  if (!watch.sessionCount) return note('이 앱에서 아직 이 채널을 본 기록이 없습니다');
  return `
    <div class="ci-kpis">
      <div class="ci-kpi"><strong>${esc(watchTimeText(watch.totalSeconds))}</strong><span>전체 시청</span></div>
      <div class="ci-kpi"><strong>${esc(watchTimeText(watch.last30Seconds))}</strong><span>최근 30일</span></div>
      <div class="ci-kpi"><strong>${esc(exactCount(watch.sessionCount))}회</strong><span>시청 횟수</span></div>
    </div>
    <dl class="ci-stats">
      ${watch.lastWatchedAt ? row('마지막 시청', `${esc(agoText(watch.lastWatchedAt, now))} <small>· ${esc(dateText(watch.lastWatchedAt, { now }))}</small>`) : ''}
      ${watch.firstWatchedAt ? row('처음 본 날', esc(dateText(watch.firstWatchedAt, { time: false, now }))) : ''}
      ${watch.topCategory ? row('자주 본 카테고리', esc(watch.topCategory)) : ''}
    </dl>`;
}

/** 최근 다시보기 카드 속. `vods` 가 null 이면 못 받은 것. */
export function vodsBody(vods, now = Date.now()) {
  if (!vods) return failed('다시보기를 불러오지 못했습니다');
  if (!vods.length) return note('다시보기가 없습니다');
  return `<div class="ci-media">${vods.map((v) => {
    const thumb = safeUrl(v.thumbnailImageUrl || v.videoImageUrl);
    return `
      <article class="card ci-mini" tabindex="0" data-vod-no="${esc(v.videoNo)}" title="${esc(v.videoTitle)}">
        <div class="thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'vod'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}<span class="badge-duration">${esc(durationLabel(v.duration))}</span></div>
        <div class="ci-mini-title">${esc(v.videoTitle || '제목 없음')}</div>
        <div class="ci-mini-meta">조회 ${esc(compactCount(v.readCount || 0))} · ${esc(agoText(v.publishDate, now))}</div>
      </article>`;
  }).join('')}</div>`;
}

/** 인기 클립 카드 속. `clips` 가 null 이면 못 받은 것. */
export function clipsBody(clips) {
  if (!clips) return failed('클립을 불러오지 못했습니다');
  if (!clips.length) return note('클립이 없습니다');
  return `<div class="ci-clips">${clips.map((c) => {
    const thumb = safeUrl(c.thumbnailImageUrl);
    const uid = c.clipUID || c.clipUid;
    return `
      <article class="card ci-mini clip" tabindex="0" data-clip-uid="${esc(uid)}" title="${esc(c.clipTitle)}">
        <div class="thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'clip'))}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ''}<span class="badge-duration">조회 ${esc(compactCount(c.readCount || 0))}</span></div>
        <div class="ci-mini-title">${esc(c.clipTitle || '제목 없음')}</div>
      </article>`;
  }).join('')}</div>`;
}

function durationLabel(sec) {
  const s = Math.max(0, Math.floor(Number(sec) || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${r}` : `${m}:${r}`;
}

/**
 * 정보 탭 전체. `ov`: overview 응답, 아직이면 undefined, 통째로 실패면 null.
 * 카드 순서는 넓은 창(3열)에서 [방송 2칸][채널] / [소개][시청 기록][채팅 또는 메모] / [다시보기 2칸][클립] 이 되도록 맞췄다.
 */
export function infoTabHtml(d, ov, now = Date.now()) {
  const c = d.channel;
  const live = d.live;
  const loading = ov === undefined;
  const bad = ov === null;
  const pend = (fn) => (loading ? skeleton() : bad ? failed('불러오지 못했습니다') : fn());
  const goto = (tab, label) => `<button type="button" class="btn ghost sm" data-ch-goto="${tab}">${esc(label)}</button>`;
  const vodTotal = ov && Number.isFinite(ov.vodTotal) && ov.vodTotal > 0 ? ` ${exactCount(ov.vodTotal)}개` : '';
  const desc = c.channelDescription && c.channelDescription.trim();
  const memoCard = card('memo', '내 메모', `<p class="ci-text${d.memo ? '' : ' muted'}">${d.memo ? esc(d.memo) : '메모가 없습니다'}</p>`, { action: `<button type="button" class="btn ghost sm" data-ch="memo">${d.memo ? '고치기' : '쓰기'}</button>` });
  const cards = [
    live
      ? card('live', '지금 방송', liveBody(live, now), { wide: true })
      : card('last', '마지막 방송', pend(() => lastLiveBody(ov.lastLive, now)), { wide: true }),
    card('channel', '채널', channelBody(c, loading ? undefined : (ov && ov.follow) || null, now)),
    card('about', '채널 소개', desc ? `<div class="ci-text ci-about">${linkify(desc)}</div>` : note('소개가 없습니다')),
    card('watch', '내 시청 기록', pend(() => watchBody(ov.watch, now))),
    live && chatPolicyRows(live).length ? card('chat', '채팅 설정', `<dl class="ci-stats">${chatPolicyRows(live).map(([k, v]) => row(k, esc(v))).join('')}</dl>`) : memoCard,
    card('vods', '최근 다시보기', pend(() => vodsBody(ov.recentVods, now)), { wide: true, action: goto('vod', `모두 보기${vodTotal}`) }),
    card('clips', '인기 클립', pend(() => clipsBody(ov.popularClips)), { action: goto('clip', '모두 보기') }),
  ];
  if (live && chatPolicyRows(live).length) cards.push(memoCard);
  return `<div class="ci-grid">${cards.join('')}</div>`;
}

/** overview 가 온 뒤 — 카드 요소는 두고 속만 바꾼다(등장 모션이 다시 돌지 않게). */
export function fillOverview(root, d, ov, now = Date.now()) {
  const html = document.createElement('div');
  html.innerHTML = infoTabHtml(d, ov, now);
  for (const fresh of html.querySelectorAll('[data-ci]')) {
    const cur = root.querySelector(`[data-ci="${fresh.dataset.ci}"]`);
    if (cur && cur.innerHTML !== fresh.innerHTML) cur.innerHTML = fresh.innerHTML;
  }
}
