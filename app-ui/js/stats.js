// 통계 — 원본 Statistics/StatisticsView(KPI 카드·채널/카테고리 순위·시간대·추이·기록 탭), CVChartStyle·CVRankingBar,
// AppState+StoreRecovery / LocalDataHygiene(데이터 관리 탭).
// DAT-04 통계 화면, DSN-05 차트·랭킹 바, DAT-06 저장소 상태·정리·백업.
import { api } from './api.js';
import { esc, safeUrl, toast, imageSrc } from './dom.js';
import { emptyState, loadingState, errorState } from './cards.js';
import { relativeDate } from './channel.js';

export const statsIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/></svg>';

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

function kpi(label, value, sub = '') {
  return `<div class="kpi-card"><span class="kpi-label">${esc(label)}</span><strong class="kpi-value">${esc(value)}</strong>${sub ? `<span class="kpi-sub">${esc(sub)}</span>` : ''}</div>`;
}

function rankingBars(items, empty) {
  if (!items.length) return `<p class="muted">${esc(empty)}</p>`;
  const max = Math.max(...items.map((x) => x.seconds), 1);
  return `<ol class="rank-bars">${items.map((x, i) => `
    <li><span class="rank-no">${i + 1}</span><span class="rank-name ellipsis" title="${esc(x.name)}">${esc(x.name)}</span>
      <span class="rank-track"><span class="rank-fill" style="width:${((x.seconds / max) * 100).toFixed(1)}%"></span></span>
      <span class="rank-val">${esc(durationText(x.seconds))}</span></li>`).join('')}</ol>`;
}

function columnChart(values, labels, { highlight = -1, titles = labels } = {}) {
  const max = Math.max(...values, 1);
  return `<div class="col-chart">${values.map((v, i) => `
    <div class="col${i === highlight ? ' hi' : ''}" title="${esc(titles[i])} · ${esc(durationText(v))}">
      <span class="col-bar" style="height:${Math.max(v > 0 ? 3 : 0, (v / max) * 100).toFixed(1)}%"></span>
      <span class="col-label">${esc(labels[i])}</span></div>`).join('')}</div>`;
}

export class StatsPage {
  constructor(content, { onOpenChannel }) {
    this.onOpenChannel = onOpenChannel;
    this.acceptsParams = true;
    this.tab = 'overview';
    this.root = document.createElement('section');
    this.root.className = 'page';
    this.root.dataset.page = 'stats';
    this.root.hidden = true;
    this.root.innerHTML = `
      <header class="page-header" data-tauri-drag-region>
        <h1 class="page-title">통계</h1>
        <span class="page-subtitle">이 기기에서 본 방송 기록으로 계산합니다</span>
      </header>
      <div class="chips lib-tabs" role="tablist">${TABS.map(([id, label]) => `<button class="chip" data-tab="${id}" role="tab">${label}</button>`).join('')}</div>
      <div class="page-body stats-body"></div>`;
    content.append(this.root);
    this.body = this.root.querySelector('.page-body');
    this.seq = 0;
    this.root.addEventListener('click', (e) => this.onClick(e));
  }

  show(params = {}) {
    if (params.tab) this.tab = params.tab;
    for (const b of this.root.querySelectorAll('[data-tab]')) b.classList.toggle('active', b.dataset.tab === this.tab);
    this.load();
  }

  async load() {
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    try {
      if (this.tab === 'data') {
        const h = await api.dataHealth();
        if (seq === this.seq) this.renderData(h);
        return;
      }
      const d = await api.insightsOverview();
      if (seq !== this.seq) return;
      this.data = d;
      if (this.tab === 'overview') this.renderOverview(d);
      else if (this.tab === 'ranking') this.renderRanking(d);
      else if (this.tab === 'time') this.renderTime(d);
      else this.renderHistory(d);
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = errorState('통계를 불러오지 못했습니다', e);
    }
  }

  renderOverview(d) {
    if (!d.sessionCount) {
      this.body.innerHTML = emptyState('아직 시청 기록이 없습니다', '라이브를 보면 시청 시간이 여기에 쌓입니다');
      return;
    }
    const days = d.daily.map((x) => new Date(x.date));
    const labels = days.map((x, i) => (i % 5 === 0 || i === days.length - 1 ? `${x.getMonth() + 1}/${x.getDate()}` : ''));
    const full = days.map((x) => `${x.getMonth() + 1}/${x.getDate()}`);
    this.body.innerHTML = `
      <div class="kpi-grid">
        ${kpi('전체 시청', durationText(d.total), `${d.countedSessionCount}회 · 채널 ${d.uniqueChannelCount}개`)}
        ${kpi('최근 7일', durationText(d.last7))}
        ${kpi('최근 30일', durationText(d.last30))}
        ${kpi('연속 시청', `${d.streak}일`, d.streak ? '오늘 하루 쉬어도 이어집니다' : '')}
        ${kpi('회당 평균', durationText(d.average))}
        ${kpi('즐겨찾기 · 북마크', `${d.favoriteCount} · ${d.bookmarkCount}`)}
      </div>
      <section class="stats-card"><h3>최근 30일</h3>
        ${columnChart(d.daily.map((x) => x.seconds), labels, { highlight: d.daily.length - 1, titles: full })}</section>
      <div class="stats-split">
        <section class="stats-card"><h3>많이 본 채널</h3>${rankingBars(d.channels.slice(0, 5), '기록 없음')}</section>
        <section class="stats-card"><h3>많이 본 카테고리</h3>${rankingBars(d.categories.slice(0, 5), '기록 없음')}</section>
      </div>`;
  }

  renderRanking(d) {
    this.body.innerHTML = `<div class="stats-split">
      <section class="stats-card"><h3>채널 상위 10</h3>${rankingBars(d.channels, '기록 없음')}</section>
      <section class="stats-card"><h3>카테고리 상위 8</h3>${rankingBars(d.categories, '기록 없음')}</section></div>`;
  }

  renderTime(d) {
    const peak = d.hourly.indexOf(Math.max(...d.hourly));
    const labels = d.hourly.map((_, h) => (h % 3 === 0 ? `${h}시` : ''));
    const night = d.hourly.slice(0, 6).reduce((a, b) => a + b, 0);
    const total = d.hourly.reduce((a, b) => a + b, 0) || 1;
    this.body.innerHTML = `
      <div class="kpi-grid">
        ${kpi('가장 많이 보는 시간', d.hourly[peak] > 0 ? `${peak}시 ~ ${peak + 1}시` : '—')}
        ${kpi('심야 시청 비율', `${Math.round((night / total) * 100)}%`, '0시 ~ 6시')}
      </div>
      <section class="stats-card"><h3>시간대별 시청</h3>${columnChart(d.hourly, labels, { highlight: peak })}</section>`;
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
          <div class="cleanup-list">${cleanups.map(([k, l, s]) => `<div class="cleanup-row"><span><strong>${esc(l)}</strong><small>${esc(s)}</small></span><button class="btn" data-stats="cleanup" data-kind="${k}">정리</button></div>`).join('')}</div>
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
    const ch = e.target.closest('[data-open-channel-id]');
    if (ch) { this.onOpenChannel(ch.dataset.openChannelId); return; }
    const b = e.target.closest('[data-stats]');
    if (!b) return;
    try {
      if (b.dataset.stats === 'cleanup') {
        b.disabled = true;
        this.renderData(await api.dataCleanup(b.dataset.kind));
        toast('정리했습니다', 1500);
      } else if (b.dataset.stats === 'backup') {
        b.disabled = true;
        this.renderData(await api.dataBackupNow());
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
