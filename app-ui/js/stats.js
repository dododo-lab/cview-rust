// 통계 — 원본 Statistics/StatisticsView(KPI 카드·채널/카테고리 순위·시간대·추이·기록 탭), CVChartStyle·CVRankingBar,
// AppState+StoreRecovery / LocalDataHygiene(데이터 관리 탭).
// DAT-04 통계 화면, DSN-05 차트·랭킹 바, DAT-06 저장소 상태·정리·백업.
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc, dismiss } from './dom.js';
import { emptyState, loadingState, errorState } from './cards.js';
import { relativeDate } from './channel.js';
import { syncTabs, bindTabKeys } from './a11y.js';
import { refreshVeiled } from './refresh-veil.js';

export const statsIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/></svg>';

/**
 * 되돌릴 수 없는 정리 — 첫 클릭에 바로 지우지 않고 대상·영향·복구 방법을 보인 뒤 한 번 더 확인한다.
 * 캐시 비우기(다시 받아 옴)는 바로 한다. 키는 data_cleanup 의 kind.
 */
export const DESTRUCTIVE_CLEANUPS = {
  oldHistory: {
    title: '90일 지난 시청 기록을 지울까요?',
    target: '90일보다 오래된 시청 기록',
    effect: '전체 시청 시간·채널 순위·시간대 통계에서 빠집니다. 최근 90일 기록과 즐겨찾기·북마크는 그대로 둡니다.',
  },
  recent: {
    title: '최근 시청 목록을 비울까요?',
    target: '보관함 › 최근 시청에 남은 채널 목록',
    effect: '채널마다 남은 마지막 시청 시각을 지웁니다. 즐겨찾기·메모·통계용 시청 기록은 그대로 둡니다.',
  },
};

/**
 * 정리 확인 시트 — 취소·Esc·바깥 클릭이면 null, 확인이면 { backup } (지우기 전에 백업할지).
 * 초점은 처음에 "취소"에 둔다(Enter 한 번으로 지워지지 않게).
 */
export function confirmCleanup(kind) {
  const info = DESTRUCTIVE_CLEANUPS[kind];
  return new Promise((resolve) => {
    const scrim = document.createElement('div');
    scrim.className = 'sheet-scrim cleanup-confirm';
    scrim.innerHTML = `<form class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="cleanup-title" aria-describedby="cleanup-desc">
      <h2 id="cleanup-title">${esc(info.title)}</h2>
      <dl class="confirm-facts" id="cleanup-desc">
        <dt>지울 대상</dt><dd>${esc(info.target)}</dd>
        <dt>영향</dt><dd>${esc(info.effect)}</dd>
        <dt>복구</dt><dd>지운 뒤에는 앱 안에서 되돌릴 수 없습니다. 지우기 전에 백업해 두면 앱을 끈 뒤 백업 폴더의 파일로 바꿔 복원할 수 있습니다.</dd>
      </dl>
      <label class="confirm-check"><span class="toggle"><input type="checkbox" data-backup checked><span></span></span>
        <span>지우기 전에 지금 상태를 백업 <small>백업은 최근 2개만 남깁니다 · 백업에 실패하면 지우지 않습니다</small></span></label>
      <div class="sheet-actions"><button type="button" class="btn" data-cancel>취소</button><button type="submit" class="btn danger">지우기</button></div>
    </form>`;
    document.body.append(scrim);
    const form = scrim.querySelector('form');
    let done = false;
    const finish = (value) => {
      if (done) return;
      done = true;
      dismiss(scrim);
      resolve(value);
    };
    scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-cancel]')) finish(null); });
    scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(null); } });
    form.addEventListener('submit', (e) => { e.preventDefault(); finish({ backup: form.querySelector('[data-backup]').checked }); });
    scrim.querySelector('[data-cancel]').focus();
  });
}

const TABS = [
  ['overview', '개요'],
  ['ranking', '채널 · 카테고리'],
  ['time', '시간대'],
  ['history', '시청 기록'],
  ['data', '데이터 관리'],
];

/** 초 → "3시간 12분" / "12분" / "40초". 순수 함수. */
export function durationText(sec) {
  const s = Math.max(0, Math.round(Number(sec) || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h) return m ? `${h}시간 ${m}분` : `${h}시간`;
  if (m) return `${m}분`;
  return `${s}초`;
}

/** 바이트 → "12.3 MB". 순수 함수. */
export function bytesText(n) {
  const v = Number(n) || 0;
  if (v < 1024) return `${v} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let x = v / 1024;
  let i = 0;
  while (x >= 1024 && i < units.length - 1) { x /= 1024; i += 1; }
  return `${x.toFixed(x < 10 ? 1 : 0)} ${units[i]}`;
}

function kpi(label, value, sub = '', { tone = '', title = '' } = {}) {
  return `<div class="kpi-card"${title ? ` title="${esc(title)}"` : ''}><span class="kpi-label">${esc(label)}</span><strong class="kpi-value">${esc(value)}</strong>${sub ? `<span class="kpi-sub${tone ? ` ${tone}` : ''}">${esc(sub)}</span>` : ''}</div>`;
}

// ── 기간 리포트(insights_report) 화면 ─────────────────────

export const PERIODS = [['today', '오늘'], ['7d', '7일'], ['30d', '30일'], ['90d', '90일'], ['all', '전체']];
const PREV_LABEL = { today: '어제 이 시각', '7d': '직전 7일', '30d': '직전 30일', '90d': '직전 90일' };
const PERIOD_KEY = 'cview.stats.period';
const REPORT_TABS = new Set(['overview', 'ranking', 'time']);
export const WEEKDAYS = ['월', '화', '수', '목', '금', '토', '일'];
const LENGTH_LABELS = ['10분 미만', '10~30분', '30분~1시간', '1~2시간', '2~4시간', '4시간 이상'];
/** 추이 쌓기·카테고리 색 — 앞 4개는 카테고리 순위와 같은 순서, 마지막은 기타. */
const SERIES_CLASS = (i, n) => (i < n ? `s${i}` : 's-rest');

function loadPeriod() {
  try {
    const v = localStorage.getItem(PERIOD_KEY);
    if (PERIODS.some(([k]) => k === v)) return v;
  } catch { /* 저장소를 못 쓰면 기본값 */ }
  return '7d';
}

/** 0~1 → "23%"(1% 미만은 따로). 순수 함수. */
export function shareText(part, total) {
  if (!(total > 0) || !(part > 0)) return '0%';
  const p = (part / total) * 100;
  return p < 1 ? '1% 미만' : `${Math.round(p)}%`;
}

const STEPS = [60, 300, 600, 900, 1800, 3600, 7200, 10800, 14400, 21600, 43200, 86400, 172800, 345600, 864000, 1728000, 4320000];
/** 세로축 눈금 — 칸이 4개 이하가 되는 가장 작은 보기 좋은 간격. 순수 함수. */
export function axisScale(max) {
  if (!(max > 0)) return { step: 1800, top: 3600 };
  const step = STEPS.find((x) => max / x <= 4) || Math.ceil(max / 4 / 86400) * 86400;
  return { step, top: Math.ceil(max / step - 1e-9) * step };
}

/** 축 눈금 글자 — "30분" · "2시간" · "1.5시간". 순수 함수. */
export function axisText(sec) {
  if (sec >= 3600) {
    const h = sec / 3600;
    return `${Number.isInteger(h) ? h : h.toFixed(1)}시간`;
  }
  return `${Math.round(sec / 60)}분`;
}

/** 직전 기간 대비 — { text, tone }. 화살표와 글자로 늘고 준 것을 함께 보인다(색만으로 구분하지 않는다). */
export function deltaText(total, prev, period) {
  const label = PREV_LABEL[period];
  if (prev == null || !label) return null;
  if (!(prev > 0)) return total > 0 ? { text: `${label} 기록 없음`, tone: '' } : null;
  const pct = Math.round(((total - prev) / prev) * 100);
  if (pct === 0) return { text: `${label}과 비슷`, tone: '' };
  return pct > 0 ? { text: `▲ ${pct}% · ${label} 대비`, tone: 'up' } : { text: `▼ ${-pct}% · ${label} 대비`, tone: 'down' };
}

const md = (d) => `${d.getMonth() + 1}/${d.getDate()}`;
const mdLong = (d) => `${d.getMonth() + 1}월 ${d.getDate()}일`;

/** 추이 칸 글자 — { label(축, 빈 칸 가능), title(풍선) }. `unit`: hour · day · week · month. 순수 함수. */
export function bucketLabels(trend, unit, now = new Date()) {
  const n = trend.length;
  const stride = unit === 'hour' ? 3 : Math.max(1, Math.ceil(n / 8));
  return trend.map((b, i) => {
    const d = new Date(b.start);
    const show = unit === 'hour' ? d.getHours() % 3 === 0 : (n - 1 - i) % stride === 0;
    if (unit === 'hour') return { label: show ? `${d.getHours()}시` : '', title: `${d.getHours()}시 ~ ${d.getHours() + 1}시` };
    if (unit === 'day') {
      const today = d.toDateString() === now.toDateString();
      return { label: show ? (today ? '오늘' : md(d)) : '', title: `${mdLong(d)} (${WEEKDAYS[(d.getDay() + 6) % 7]})${today ? ' · 오늘' : ''}` };
    }
    if (unit === 'week') {
      const end = new Date(d);
      end.setDate(end.getDate() + 6);
      return { label: show ? md(d) : '', title: `${mdLong(d)} ~ ${mdLong(end)} 주` };
    }
    const y = d.getFullYear();
    const m = d.getMonth() + 1;
    return { label: show ? (m === 1 || i === 0 ? `${String(y).slice(2)}년 ${m}월` : `${m}월`) : '', title: `${y}년 ${m}월` };
  });
}

/**
 * 세로 막대(쌓기) 차트 — cols: [{ label, total, parts }]. 높이는 가장 큰 칸(또는 평균선)에 맞춘 보기 좋은 눈금.
 * 값은 풍선(마우스·키보드)으로 읽는다. 그래프 자체는 role=img 가 아니라 화살표로 칸을 옮기는 묶음이다.
 */
function barChart(id, cols, { seriesCount = 0, avg = 0, avgLabel = '', aria = '', active = -1, tall = true } = {}) {
  const max = Math.max(0, avg, ...cols.map((c) => c.total));
  const { step, top } = axisScale(max);
  const pct = (v) => `${Math.min(100, (v / top) * 100).toFixed(2)}%`;
  const ticks = [];
  for (let v = step; v <= top + 1e-6; v += step) ticks.push(v);
  return `<div class="chart bar-chart${tall ? '' : ' short'}" data-chart="${id}" tabindex="0" role="group" aria-roledescription="막대 그래프" aria-label="${esc(aria)}" aria-describedby="stats-chart-help"${active >= 0 ? ` data-active="${active}"` : ''}>
    <div class="chart-y" aria-hidden="true"><span style="bottom:0">0</span>${ticks.map((v) => `<span style="bottom:${pct(v)}">${esc(axisText(v))}</span>`).join('')}</div>
    <div class="chart-plot">
      ${ticks.map((v) => `<span class="chart-grid" style="bottom:${pct(v)}"></span>`).join('')}
      ${avg > 0 ? `<span class="chart-avg" style="bottom:${pct(avg)}"><em>${esc(avgLabel)}</em></span>` : ''}
      <div class="chart-cols">${cols.map((c, i) => `<div class="chart-col" data-i="${i}">${c.total > 0 ? `<span class="chart-stack" style="height:${pct(c.total)}">${c.parts.map((p, k) => (p > 0 ? `<i class="${seriesCount ? SERIES_CLASS(k, seriesCount) : 'mono'}" style="flex-grow:${p.toFixed(1)}"></i>` : '')).join('')}</span>` : ''}</div>`).join('')}</div>
    </div>
    <div class="chart-x" aria-hidden="true">${cols.map((c) => `<span>${esc(c.label)}</span>`).join('')}</div>
  </div>`;
}

function legend(names, seriesCount, extra = '') {
  return `<ul class="chart-legend">${names.map((n, i) => `<li><i class="sw ${SERIES_CLASS(i, seriesCount)}"></i>${esc(n)}</li>`).join('')}${extra}</ul>`;
}

/** 요일 × 시간대 히트맵 — 진하기 5단계(0 은 빈 칸), 값은 풍선으로. */
function heatmap(id, grid) {
  const max = Math.max(0, ...grid.flat());
  const level = (v) => (v <= 0 || max <= 0 ? 0 : Math.min(4, Math.ceil((v / max) * 4)));
  const hours = Array.from({ length: 24 }, (_, h) => `<span>${h % 3 === 0 ? h : ''}</span>`).join('');
  return `<div class="chart heatmap" data-chart="${id}" tabindex="0" role="group" aria-roledescription="히트맵" aria-label="요일과 시간대별 시청 시간" aria-describedby="stats-chart-help">
    <div class="hm-grid">${grid.map((row, d) => `<span class="hm-day" aria-hidden="true">${WEEKDAYS[d]}</span>${row.map((v, h) => `<span class="hm-cell l${level(v)}" data-i="${d * 24 + h}"></span>`).join('')}`).join('')}
      <span></span>${hours.replace(/<span>/g, '<span class="hm-hour" aria-hidden="true">')}</div>
    <div class="hm-scale" aria-hidden="true">적음 ${[0, 1, 2, 3, 4].map((l) => `<span class="hm-cell l${l}"></span>`).join('')} 많음</div>
  </div>`;
}

/** 도넛 — rows: [{ name, seconds, cls }]. 가운데에 큰 글자, 옆에 범례(이름·비중·시간)를 둔다. */
function donut(id, rows, total, center, sub) {
  const R = 42;
  const C = 2 * Math.PI * R;
  let acc = 0;
  const arcs = rows.map((r, i) => {
    const len = total > 0 ? (r.seconds / total) * C : 0;
    const gap = rows.length > 1 && len > 3 ? 1.5 : 0;
    const arc = `<circle class="donut-arc ${r.cls}" data-i="${i}" r="${R}" cx="50" cy="50" stroke-dasharray="${(len - gap).toFixed(2)} ${(C - len + gap).toFixed(2)}" stroke-dashoffset="${(-acc).toFixed(2)}"/>`;
    acc += len;
    return arc;
  }).join('');
  return `<div class="chart donut-wrap" data-chart="${id}" tabindex="0" role="group" aria-roledescription="원 그래프" aria-label="카테고리 비중" aria-describedby="stats-chart-help">
    <div class="donut"><svg viewBox="0 0 100 100" aria-hidden="true"><circle class="donut-bg" r="${R}" cx="50" cy="50"/><g transform="rotate(-90 50 50)">${arcs}</g></svg>
      <div class="donut-center"><strong>${esc(center)}</strong><span>${esc(sub)}</span></div></div>
    <ul class="donut-legend">${rows.map((r, i) => `<li data-i="${i}"><i class="sw ${r.cls}"></i><span class="ellipsis">${esc(r.name)}</span><b>${esc(shareText(r.seconds, total))}</b><small>${esc(durationText(r.seconds))}</small></li>`).join('')}</ul>
  </div>`;
}

/** 순위 막대 — 비중(전체 대비)과 시간·횟수. 채널이면 이름을 눌러 채널로 간다. */
function shareBars(rows, total, { channel = false, limit = rows.length, swatches = 0 } = {}) {
  const max = Math.max(...rows.map((x) => x.seconds), 1);
  return `<ol class="share-bars">${rows.slice(0, limit).map((x, i) => {
    const name = channel
      ? `<button class="link-btn rank-name ellipsis" data-open-channel-id="${esc(x.channelId)}" title="${esc(x.name)}">${esc(x.name)}</button>`
      : `<span class="rank-name ellipsis" title="${esc(x.name)}">${i < swatches ? `<i class="sw ${SERIES_CLASS(i, swatches)}"></i>` : ''}${esc(x.name)}</span>`;
    return `<li><span class="rank-no">${i + 1}</span>${name}
      <span class="rank-track" aria-hidden="true"><span class="rank-fill" style="width:${((x.seconds / max) * 100).toFixed(1)}%"></span></span>
      <span class="rank-share">${esc(shareText(x.seconds, total))}</span><span class="rank-val">${esc(durationText(x.seconds))}</span></li>`;
  }).join('')}</ol>`;
}

export class StatsPage {
  constructor(content, { onOpenChannel }) {
    this.onOpenChannel = onOpenChannel;
    this.acceptsParams = true;
    this.tab = 'overview';
    this.period = loadPeriod();
    this.charts = {};
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'stats';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">통계</h1>
        <span class="page-subtitle">이 기기에서 본 방송 기록으로 계산합니다 · 기록은 이 기기에만 저장됩니다</span>
      </header>
      <div class="stats-toolbar">
        <div class="chips lib-tabs" role="tablist" aria-label="통계">${TABS.map(([id, label]) => `<button class="chip" data-tab="${id}" role="tab">${label}</button>`).join('')}</div>
        <div class="segmented stats-period" role="group" aria-label="기간">${PERIODS.map(([id, label]) => `<button class="seg" data-period="${id}" aria-pressed="false">${label}</button>`).join('')}</div>
      </div>
      <div class="page-body stats-body"></div>
      <p id="stats-chart-help" class="sr-only">화살표 키로 칸을 옮기면 값을 읽어 줍니다</p>
      <div class="sr-only" aria-live="polite" data-stats-live></div>
      <div class="chart-tip" aria-hidden="true" hidden></div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.tabs = this.root.querySelector('.lib-tabs');
    this.periodBar = this.root.querySelector('.stats-period');
    this.tip = this.root.querySelector('.chart-tip');
    this.live = this.root.querySelector('[data-stats-live]');
    this.seq = 0;
    bindTabKeys(this.tabs);
    this.root.addEventListener('click', (e) => this.onClick(e));
    this.bindCharts();
  }

  show(params = {}) {
    if (params.tab) this.tab = params.tab;
    if (params.period && PERIODS.some(([k]) => k === params.period)) this.period = params.period;
    syncTabs(this.tabs, this.tab, this.body, 'stats');
    this.syncPeriod();
    // 직접 새로고침(덮개 아래)이면 보던 내용을 흐리게 둔 채 바꾼다.
    return this.load({ keep: refreshVeiled(this.root) });
  }

  syncPeriod() {
    this.periodBar.hidden = !REPORT_TABS.has(this.tab);
    for (const b of this.periodBar.querySelectorAll('[data-period]')) {
      const on = b.dataset.period === this.period;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
  }

  async load({ keep = false } = {}) {
    const seq = ++this.seq;
    this.hideTip();
    if (!keep) this.body.innerHTML = loadingState();
    try {
      if (this.tab === 'data') {
        const h = await api.dataHealth();
        if (seq === this.seq) this.renderData(h);
        return;
      }
      if (this.tab === 'history') {
        const d = await api.insightsOverview();
        if (seq === this.seq) this.renderHistory(d);
        return;
      }
      const r = await api.insightsReport(this.period);
      if (seq !== this.seq) return;
      this.report = r;
      this.charts = {};
      if (!(r.total > 0)) this.renderEmpty(r);
      else if (this.tab === 'overview') this.renderOverview(r);
      else if (this.tab === 'ranking') this.renderRanking(r);
      else this.renderTime(r);
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('통계를 불러오지 못했습니다', e);
    }
  }

  periodName() {
    return this.period === 'all' ? '전체 기간' : this.period === 'today' ? '오늘' : `최근 ${this.period.replace('d', '일')}`;
  }

  renderEmpty(r) {
    if (!r.firstWatchedAt) {
      this.body.innerHTML = emptyState('아직 시청 기록이 없습니다', '라이브를 보면 시청 시간이 여기에 쌓입니다 · 기록은 이 기기에만 저장됩니다');
      return;
    }
    this.body.innerHTML = emptyState(`${this.periodName()}에는 시청 기록이 없습니다`, `처음 기록: ${new Date(r.firstWatchedAt).toLocaleDateString()}`,
      '<button class="btn" data-period="all">전체 기간 보기</button>');
  }

  /** 추이 차트(쌓기) + 범례. */
  trendCard(r) {
    const labels = bucketLabels(r.trend, r.unit);
    const elapsed = r.trend.filter((b) => new Date(b.start) <= new Date(r.rangeEnd)).length || 1;
    const avg = r.unit === 'hour' ? 0 : r.total / elapsed;
    const unitName = { hour: '시간', day: '하루', week: '주', month: '달' }[r.unit];
    const names = [...r.series, ...(r.trend.some((b) => b.parts[r.series.length] > 0) ? ['기타'] : [])];
    const active = r.trend.length - 1 - [...r.trend].reverse().findIndex((b) => b.seconds > 0);
    this.charts.trend = {
      count: r.trend.length,
      describe: (i) => {
        const b = r.trend[i];
        const rows = b.parts.map((p, k) => [SERIES_CLASS(k, r.series.length), k < r.series.length ? r.series[k] : '기타', p]).filter((x) => x[2] > 0);
        return { title: labels[i].title, total: b.seconds, rows };
      },
    };
    const range = r.unit === 'hour' ? '오늘 0시부터' : `${mdLong(new Date(r.rangeStart))} ~ ${mdLong(new Date(r.rangeEnd))}`;
    return `<section class="stats-card"><div class="card-head"><h3>시청 추이</h3><span class="stats-meta">${esc(range)} · ${esc(unitName)} 단위</span></div>
      ${barChart('trend', r.trend.map((b, i) => ({ label: labels[i].label, total: b.seconds, parts: b.parts })),
    { seriesCount: r.series.length, avg, avgLabel: `${unitName} 평균 ${durationText(avg)}`, aria: `${this.periodName()} 시청 추이`, active })}
      ${legend(names, r.series.length, avg > 0 ? '<li><i class="sw avg"></i>평균</li>' : '')}</section>`;
  }

  categoryDonut(r) {
    const n = r.series.length;
    const top = r.categories.slice(0, n).map((c, i) => ({ name: c.name, seconds: c.seconds, cls: SERIES_CLASS(i, n) }));
    const rest = r.total - top.reduce((a, c) => a + c.seconds, 0);
    const restCount = r.categoryCount - top.length;
    if (rest > 0.5 && restCount > 0) top.push({ name: `기타 ${restCount}개`, seconds: rest, cls: 's-rest' });
    this.charts.cats = {
      count: top.length,
      describe: (i) => ({ title: top[i].name, total: top[i].seconds, note: `전체의 ${shareText(top[i].seconds, r.total)}` }),
    };
    return donut('cats', top, r.total, `${r.categoryCount}`, '카테고리');
  }

  renderOverview(r) {
    const delta = deltaText(r.total, r.previousTotal, r.period);
    const days = r.period === 'today' ? '오늘' : `시청한 날 ${r.activeDays}/${r.days}일`;
    const longest = r.longest;
    this.body.innerHTML = `
      <div class="kpi-grid">
        ${kpi('시청 시간', durationText(r.total), delta ? delta.text : '', { tone: delta?.tone })}
        ${kpi('하루 평균', durationText(r.dailyAverage), days)}
        ${kpi('시청 횟수', `${r.sessionCount.toLocaleString()}회`, `회당 평균 ${durationText(r.averageSession)}`)}
        ${kpi('본 채널', `${r.channelCount.toLocaleString()}개`, `카테고리 ${r.categoryCount}개`)}
        ${kpi('연속 시청', `${r.currentStreak}일`, `최장 ${r.bestStreak}일`, { title: '오늘 아직 안 봤어도 어제까지 이어진 날을 셉니다' })}
        ${longest ? kpi('가장 길게 본 방송', durationText(longest.seconds), `${longest.channelName} · ${relativeDate(longest.startedAt)}`, { title: `${longest.channelName} · ${longest.category}` }) : ''}
      </div>
      ${this.trendCard(r)}
      <div class="stats-split">
        <section class="stats-card"><div class="card-head"><h3>많이 본 채널</h3><button class="link-btn" data-tab="ranking">전체 순위</button></div>
          ${shareBars(r.channels, r.total, { channel: true, limit: 5 })}</section>
        <section class="stats-card"><div class="card-head"><h3>카테고리 비중</h3></div>${this.categoryDonut(r)}</section>
      </div>`;
  }

  renderRanking(r) {
    const others = r.otherChannels.count ? `<p class="stats-meta rank-others">그 밖의 채널 ${r.otherChannels.count}개 · ${esc(durationText(r.otherChannels.seconds))} (${esc(shareText(r.otherChannels.seconds, r.total))})</p>` : '';
    const otherCats = r.otherCategories.count ? `<p class="stats-meta rank-others">그 밖의 카테고리 ${r.otherCategories.count}개 · ${esc(durationText(r.otherCategories.seconds))}</p>` : '';
    this.body.innerHTML = `
      <section class="stats-card"><div class="card-head"><h3>채널 순위</h3><span class="stats-meta">${esc(this.periodName())} · ${r.channelCount}개 채널</span></div>
        <div class="rank-table" role="table" aria-label="채널 순위">
          <div class="rt-row rt-head" role="row"><span role="columnheader">순위</span><span role="columnheader">채널</span><span role="columnheader" class="rt-bar">비중</span><span role="columnheader">시청 시간</span><span role="columnheader" class="rt-opt">횟수</span><span role="columnheader" class="rt-opt">마지막 시청</span></div>
          ${r.channels.map((c, i) => `<div class="rt-row" role="row"><span role="cell" class="rank-no">${i + 1}</span>
            <span role="cell" class="rt-name"><button class="link-btn ellipsis" data-open-channel-id="${esc(c.channelId)}" title="${esc(c.name)}">${esc(c.name)}</button></span>
            <span role="cell" class="rt-bar"><span class="rank-track" aria-hidden="true"><span class="rank-fill" style="width:${((c.seconds / r.channels[0].seconds) * 100).toFixed(1)}%"></span></span><span class="rank-share">${esc(shareText(c.seconds, r.total))}</span></span>
            <span role="cell" class="rank-val">${esc(durationText(c.seconds))}</span>
            <span role="cell" class="rt-opt rank-val">${c.sessions}회</span>
            <span role="cell" class="rt-opt rank-val">${esc(relativeDate(c.lastWatchedAt))}</span></div>`).join('')}
        </div>${others}</section>
      <div class="stats-split">
        <section class="stats-card"><div class="card-head"><h3>카테고리 비중</h3></div>${this.categoryDonut(r)}</section>
        <section class="stats-card"><div class="card-head"><h3>카테고리 순위</h3></div>${shareBars(r.categories, r.total, { swatches: r.series.length })}${otherCats}</section>
      </div>`;
  }

  renderTime(r) {
    const hourly = Array.from({ length: 24 }, (_, h) => r.heatmap.reduce((a, row) => a + row[h], 0));
    const weekly = r.heatmap.map((row) => row.reduce((a, b) => a + b, 0));
    const peakH = hourly.indexOf(Math.max(...hourly));
    const peakD = weekly.indexOf(Math.max(...weekly));
    const night = hourly.slice(0, 6).reduce((a, b) => a + b, 0);
    const weekend = weekly[5] + weekly[6];
    let cell = [0, 0];
    r.heatmap.forEach((row, d) => row.forEach((v, h) => { if (v > r.heatmap[cell[0]][cell[1]]) cell = [d, h]; }));
    this.charts.heat = {
      count: 168,
      rowStep: 24,
      describe: (i) => {
        const d = Math.floor(i / 24);
        const h = i % 24;
        return { title: `${WEEKDAYS[d]}요일 ${h}시 ~ ${h + 1}시`, total: r.heatmap[d][h], note: `기간 전체의 ${shareText(r.heatmap[d][h], r.total)}` };
      },
    };
    this.charts.hours = { count: 24, describe: (i) => ({ title: `${i}시 ~ ${i + 1}시`, total: hourly[i], note: `전체의 ${shareText(hourly[i], r.total)}` }) };
    this.charts.days = { count: 7, describe: (i) => ({ title: `${WEEKDAYS[i]}요일`, total: weekly[i], note: `전체의 ${shareText(weekly[i], r.total)}` }) };
    const lenMax = Math.max(...r.lengths.map((l) => l.count), 1);
    this.body.innerHTML = `
      <div class="kpi-grid">
        ${kpi('주로 보는 시간대', `${peakH}시 ~ ${peakH + 1}시`, `전체의 ${shareText(hourly[peakH], r.total)}`)}
        ${kpi('주로 보는 요일', `${WEEKDAYS[peakD]}요일`, `전체의 ${shareText(weekly[peakD], r.total)}`)}
        ${kpi('가장 많이 본 때', `${WEEKDAYS[cell[0]]} ${cell[1]}시`, durationText(r.heatmap[cell[0]][cell[1]]))}
        ${kpi('심야 시청', shareText(night, r.total), '0시 ~ 6시')}
        ${kpi('주말 시청', shareText(weekend, r.total), '토 · 일')}
      </div>
      <section class="stats-card"><div class="card-head"><h3>요일 · 시간대</h3><span class="stats-meta">${esc(this.periodName())} · 실제로 본 시간에 나눠 담습니다</span></div>${heatmap('heat', r.heatmap)}</section>
      <div class="stats-split">
        <section class="stats-card"><div class="card-head"><h3>시간대별</h3></div>
          ${barChart('hours', hourly.map((v, h) => ({ label: h % 3 === 0 ? `${h}시` : '', total: v, parts: [v] })), { aria: '시간대별 시청 시간', active: peakH, tall: false })}</section>
        <section class="stats-card"><div class="card-head"><h3>요일별</h3></div>
          ${barChart('days', weekly.map((v, d) => ({ label: WEEKDAYS[d], total: v, parts: [v] })), { aria: '요일별 시청 시간', active: peakD, tall: false })}</section>
      </div>
      <section class="stats-card"><div class="card-head"><h3>한 번에 보는 길이</h3><span class="stats-meta">${r.sessionCount}회 · 회당 평균 ${esc(durationText(r.averageSession))}</span></div>
        <ol class="share-bars len-bars">${r.lengths.map((l, i) => `<li><span class="rank-name">${LENGTH_LABELS[i]}</span>
          <span class="rank-track" aria-hidden="true"><span class="rank-fill" style="width:${((l.count / lenMax) * 100).toFixed(1)}%"></span></span>
          <span class="rank-share">${l.count}회</span><span class="rank-val">${esc(shareText(l.count, r.sessionCount))}</span></li>`).join('')}</ol></section>`;
  }

  // ── 차트 풍선·키보드 ─────────────────────

  bindCharts() {
    const at = (e) => {
      const chart = e.target.closest?.('[data-chart]');
      const cell = e.target.closest?.('[data-i]');
      return chart && cell && chart.contains(cell) ? [chart, Number(cell.dataset.i)] : [chart, -1];
    };
    this.body.addEventListener('pointermove', (e) => {
      const [chart, i] = at(e);
      if (chart && i >= 0) this.activate(chart, i, false);
      else if (!chart || chart !== this.tipChart || document.activeElement !== chart) this.hideTip();
    });
    this.body.addEventListener('pointerleave', () => { if (!this.tipChart || document.activeElement !== this.tipChart) this.hideTip(); });
    this.body.addEventListener('focusin', (e) => {
      const chart = e.target.matches?.('[data-chart]') ? e.target : null;
      if (!chart) return;
      const meta = this.charts[chart.dataset.chart];
      if (!meta) return;
      const i = Number(chart.dataset.active ?? meta.count - 1);
      this.activate(chart, Math.max(0, Math.min(meta.count - 1, i)), true);
    });
    this.body.addEventListener('focusout', (e) => { if (e.target === this.tipChart) this.hideTip(); });
    this.body.addEventListener('keydown', (e) => {
      const chart = e.target.matches?.('[data-chart]') ? e.target : null;
      const meta = chart && this.charts[chart.dataset.chart];
      if (!meta) return;
      if (e.key === 'Escape') { if (!this.tip.hidden) { e.preventDefault(); e.stopPropagation(); this.hideTip(); } return; }
      const cur = Number(chart.dataset.active ?? 0);
      const row = meta.rowStep || 1;
      const next = {
        ArrowRight: cur + 1, ArrowLeft: cur - 1, ArrowDown: cur + row, ArrowUp: cur - row, Home: 0, End: meta.count - 1,
      }[e.key];
      if (next === undefined || e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      this.activate(chart, Math.max(0, Math.min(meta.count - 1, next)), true);
    });
    // 창 크기가 바뀌면 칸 자리가 달라진다 — 옛 자리에 풍선을 남기지 않는다.
    window.addEventListener('resize', () => this.hideTip(), { passive: true });
    // 마우스로 띄운 풍선도 초점 위치와 상관없이 Esc 로 닫는다(WCAG 1.4.13). 풍선이 떠 있을 때만 가로챈다.
    document.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || this.tip.hidden || this.root.hidden) return;
      if (e.target.closest?.('[data-chart]')) return; // 초점 있는 차트는 위 처리기가 맡는다
      e.preventDefault();
      e.stopPropagation();
      this.hideTip();
    }, true);
    // 스크롤하면 풍선이 엉뚱한 곳에 떠 있지 않게 — 키보드 초점이 있는 차트(초점 이동으로 자동 스크롤된 경우 포함)는
    // 다음 프레임에 자리를 다시 잡고, 마우스로 연 풍선은 닫는다.
    this.root.addEventListener('scroll', () => {
      const chart = this.tipChart;
      if (!chart || document.activeElement !== chart) { this.hideTip(); return; }
      cancelAnimationFrame(this.tipFrame);
      this.tipFrame = requestAnimationFrame(() => {
        if (this.tipChart === chart && document.activeElement === chart) this.activate(chart, Number(chart.dataset.active ?? 0), false);
      });
    }, { capture: true, passive: true });
  }

  activate(chart, i, announce) {
    const meta = this.charts[chart.dataset.chart];
    if (!meta) return;
    const d = meta.describe(i);
    chart.dataset.active = String(i);
    for (const el of chart.querySelectorAll('.is-active')) el.classList.remove('is-active');
    const cells = chart.querySelectorAll(`[data-i="${i}"]`);
    cells.forEach((el) => el.classList.add('is-active'));
    const rows = (d.rows || []).map(([cls, name, v]) => `<li><i class="sw ${cls}"></i><span>${esc(name)}</span><b>${esc(durationText(v))}</b></li>`).join('');
    this.tip.innerHTML = `<strong>${esc(d.title)}</strong><span class="tip-total">${d.total > 0 ? esc(durationText(d.total)) : '기록 없음'}</span>${d.note && d.total > 0 ? `<small>${esc(d.note)}</small>` : ''}${rows ? `<ul>${rows}</ul>` : ''}`;
    this.tip.hidden = false;
    this.tipChart = chart;
    const anchor = (cells[0]?.querySelector('.chart-stack') || cells[0] || chart).getBoundingClientRect();
    const tw = this.tip.offsetWidth;
    const th = this.tip.offsetHeight;
    const x = Math.max(8, Math.min(window.innerWidth - tw - 8, anchor.left + anchor.width / 2 - tw / 2));
    const above = anchor.top - th - 10;
    const y = above >= 8 ? above : Math.min(window.innerHeight - th - 8, anchor.bottom + 10);
    // 페이지 전환 애니메이션(transform)이 걸린 조상이 fixed 의 기준이 될 수 있어, 풍선의 실제 원점을 재서 뺀다.
    this.tip.style.transform = 'none';
    const origin = this.tip.getBoundingClientRect();
    this.tip.style.transform = `translate(${Math.round(x - origin.left)}px, ${Math.round(y - origin.top)}px)`;
    if (announce) {
      const parts = (d.rows || []).map(([, n, v]) => `${n} ${durationText(v)}`).join(', ');
      this.live.textContent = `${d.title}, ${d.total > 0 ? durationText(d.total) : '기록 없음'}${parts ? `, ${parts}` : ''}${d.note && d.total > 0 ? `, ${d.note}` : ''}`;
    }
  }

  hideTip() {
    if (this.tip) this.tip.hidden = true;
    if (this.tipChart) for (const el of this.tipChart.querySelectorAll('.is-active')) el.classList.remove('is-active');
    this.tipChart = null;
  }

  renderHistory(d) {
    if (!d.recent.length) {
      this.body.innerHTML = emptyState('시청 기록이 없습니다');
      return;
    }
    this.body.innerHTML = `<div class="history-list">${d.recent.map((r) => {
      const thumb = safeUrl(r.thumbnailUrl);
      return `<button class="history-row" data-open-channel-id="${esc(r.channelId)}">
        <span class="bookmark-thumb">${thumb ? `<img src="${esc(imageSrc(thumb, 'static'))}" alt="" referrerpolicy="no-referrer">` : ''}</span>
        <span class="bookmark-text"><strong class="ellipsis">${esc(r.channelName || r.channelId)}${r.active ? ' <span class="live-dot-text">시청 중</span>' : ''}</strong>
        <span class="ellipsis">${r.categoryName ? `${esc(r.categoryName)} · ` : ''}${esc(relativeDate(r.startedAt))} · ${esc(durationText(r.seconds))}</span></span></button>`;
    }).join('')}</div>`;
  }

  renderData(h) {
    if (!h.available) {
      this.body.innerHTML = emptyState('데이터 저장소를 열지 못했습니다', '앱을 다시 시작해도 계속되면 백업 폴더에서 이전 상태를 확인하세요',
        `<button class="btn" data-stats="reveal" data-path="${esc(h.backupDir)}">백업 폴더 열기</button>`);
      return;
    }
    const cleanups = [
      ['liveCache', '라이브 목록 캐시 비우기', '다음 새로고침 때 다시 받아 옵니다'],
      ['imageIndex', '이미지 캐시 색인 비우기', '썸네일을 다시 받아 옵니다'],
      ['oldHistory', '90일 지난 시청 기록 지우기', '통계에서 빠집니다'],
      ['recent', '최근 시청 목록 비우기', '즐겨찾기는 그대로 둡니다'],
      ['legacy', '이전 버전 잔여 데이터 정리', '쓰지 않는 옛 캐시 행'],
    ];
    this.body.innerHTML = `
      <div class="kpi-grid">
        ${kpi('저장소 크기', bytesText(h.storeBytes))}
        ${kpi('남은 디스크', h.freeBytes != null ? bytesText(h.freeBytes) : '—')}
        ${kpi('백업', `${h.backups.length}개`, h.backups[0] ? `${relativeDate(h.backups[0].createdAt)} · ${h.backups[0].reason}` : '아직 없음')}
      </div>
      <div class="stats-split">
        <section class="stats-card"><h3>저장된 항목</h3>
          <table class="data-table">${h.tables.map((t) => `<tr><td>${esc(t.label)}</td><td class="num">${t.rows.toLocaleString()}</td></tr>`).join('')}</table></section>
        <section class="stats-card"><h3>정리</h3>
          <div class="cleanup-list">${cleanups.map(([k, l, s]) => `<div class="cleanup-row"><span><strong>${esc(l)}</strong><small>${esc(s)}</small></span>${DESTRUCTIVE_CLEANUPS[k]
            ? `<button class="btn danger" data-stats="cleanup" data-kind="${k}" aria-haspopup="dialog">지우기…</button>`
            : `<button class="btn" data-stats="cleanup" data-kind="${k}">정리</button>`}</div>`).join('')}</div>
          <h3 class="sub-h">백업</h3>
          <div class="cleanup-row"><span><strong>지금 백업</strong><small>최근 2개만 남깁니다 · 복원은 앱을 끈 뒤 백업 폴더의 파일로 바꿉니다</small></span><button class="btn primary" data-stats="backup">백업</button></div>
          ${h.backups.map((b) => `<div class="cleanup-row"><span><strong>${esc(new Date(b.createdAt).toLocaleString())}</strong><small>${esc(b.reason)} · ${esc(bytesText(b.bytes))}</small></span></div>`).join('')}
          <button class="link-btn" data-stats="reveal" data-path="${esc(h.backupDir)}">백업 폴더 열기</button>
        </section>
      </div>
      <p class="muted small">${esc(h.storePath)}</p>`;
  }

  async onClick(e) {
    const tab = e.target.closest('[data-tab]');
    if (tab) { this.show({ tab: tab.dataset.tab }); return; }
    const period = e.target.closest('[data-period]');
    if (period) {
      this.period = period.dataset.period;
      try { localStorage.setItem(PERIOD_KEY, this.period); } catch { /* 기억 못 해도 화면은 바꾼다 */ }
      this.syncPeriod();
      this.load();
      return;
    }
    const ch = e.target.closest('[data-open-channel-id]');
    if (ch) { this.onOpenChannel(ch.dataset.openChannelId); return; }
    const b = e.target.closest('[data-stats]');
    if (!b) return;
    // 기다리는 동안 다른 탭으로 갔으면 결과는 알림으로만 — 데이터 화면을 그 탭 위에 그리지 않는다
    const seq = this.seq;
    const paint = (h) => { if (seq === this.seq && this.tab === 'data') this.renderData(h); };
    try {
      if (b.dataset.stats === 'cleanup') {
        const kind = b.dataset.kind;
        if (DESTRUCTIVE_CLEANUPS[kind]) {
          const ok = await confirmCleanup(kind);
          if (!ok) { b.focus(); return; } // 취소 — 아무것도 바꾸지 않는다
          b.disabled = true;
          if (ok.backup) {
            try { paint(await api.dataBackupNow()); } catch (er) {
              b.disabled = false;
              toast(`백업하지 못해 지우지 않았습니다 — ${er.message}`, 5000);
              return;
            }
          }
          paint(await api.dataCleanup(kind));
          toast(ok.backup ? '백업한 뒤 지웠습니다' : '지웠습니다', 1800);
          return;
        }
        b.disabled = true;
        paint(await api.dataCleanup(kind));
        toast('정리했습니다', 1500);
      } else if (b.dataset.stats === 'backup') {
        b.disabled = true;
        paint(await api.dataBackupNow());
        toast('백업했습니다', 1500);
      } else if (b.dataset.stats === 'reveal') {
        await api.revealPath(b.dataset.path);
      }
    } catch (er) {
      b.disabled = false;
      toast(er.message);
    }
  }
}
