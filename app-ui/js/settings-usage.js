// 설정 › 성능 — 앱 사용률 실시간 그래프·통계·프로세스별 내역·표로 보기. 측정 설정과 내보내기·지우기는 settings.js SCHEMA('usage').
// 패널은 설정을 다시 그릴 때마다 새로 붙고(mountUsagePanel), 떨어져 나가거나 설정 화면이 숨으면 실시간 표본을 그만 받는다.
import {
  usage, initUsage, watchUsage, subscribeUsage, measuring, METRICS, metric, gpuState, stats, cores,
  fmtPct, fmtBytes, fmtRate, UsageChart,
} from './usage-monitor.js';
import { renderGroups } from './usage-chip.js';

const RANGES = [[60000, '1분'], [300000, '5분'], [900000, '15분'], [0, '전체']];
const view = { metric: 'cpu', range: 300000 };

/** 패널 HTML(검색 중엔 그리지 않는다 — 측정 설정 줄만 걸린다). */
export function usagePanelHtml() {
  const radios = (attr, items, on, label) => `<div class="segmented up-seg" role="radiogroup" aria-label="${label}">${items.map(([v, l]) => {
    const sel = String(v) === String(on);
    return `<button type="button" class="seg${sel ? ' active' : ''}" role="radio" aria-checked="${sel}" tabindex="${sel ? 0 : -1}" data-${attr}="${v}">${l}</button>`;
  }).join('')}</div>`;
  return `<section class="settings-group usage-panel" data-g="usage" data-usage-panel aria-labelledby="usage-panel-title">
    <h2 id="usage-panel-title">실시간 사용률 <span class="up-state" data-up-state></span></h2>
    <div class="usage-body">
      <div class="usage-controls">
        ${radios('up-metric', METRICS.map((m) => [m.id, m.label]), view.metric, '그래프 지표')}
        ${radios('up-range', RANGES, view.range, '그래프 기간')}
      </div>
      <dl class="usage-stats">
        <div><dt>지금</dt><dd data-st="now">—</dd></div>
        <div><dt>평균</dt><dd data-st="avg">—</dd></div>
        <div><dt>최대</dt><dd data-st="max">—</dd></div>
        <div><dt>최소</dt><dd data-st="min">—</dd></div>
      </dl>
      <div class="usage-chart-host"></div>
      <p class="usage-note" data-up-note></p>
      <div class="usage-split">
        <div class="usage-groups-box">
          <div class="up-groups-head"><span>프로세스별 지금</span><span aria-hidden="true">CPU · 메모리</span></div>
          <ul class="up-groups" aria-label="프로세스별 사용률"></ul>
        </div>
        <details class="usage-table">
          <summary>표로 보기 <small>1분 평균</small></summary>
          <div class="usage-table-scroll" tabindex="0" role="region" aria-label="1분 평균 사용률 표"><table>
            <thead><tr><th scope="col">시각</th><th scope="col">CPU</th><th scope="col">GPU</th><th scope="col">메모리</th><th scope="col">네트워크</th></tr></thead>
            <tbody></tbody></table></div>
        </details>
      </div>
    </div>
  </section>`;
}

function rangeMs() {
  if (view.range) return view.range;
  return Math.max(1, (usage.status && usage.status.historyMinutes) || 15) * 60000;
}

function noteText() {
  const st = usage.status || {};
  const g = st.gpu || {};
  const parts = [];
  if (!g.probed) parts.push('GPU 지원 여부를 확인하는 중입니다.');
  else if (!g.app && !g.system) parts.push('이 기기에서는 GPU 사용률을 읽을 수 없어 GPU는 "지원 안 됨"으로 보입니다.');
  else if (!g.app) parts.push('앱별 GPU 값을 읽을 수 없어 GPU 그래프는 전체 GPU 사용률입니다.');
  if (g.memoryScope === 'system') parts.push('GPU 메모리는 시스템 전체 사용량입니다(Apple 실리콘은 메모리를 CPU와 함께 씁니다).');
  if (g.memoryScope === 'app') parts.push('GPU 메모리는 앱 프로세스의 전용 + 공유 메모리입니다.');
  parts.push(`CPU는 전체 코어(${cores()}개) 대비, 네트워크는 앱이 직접 받은 양(영상·채팅·API·이미지)입니다.`);
  return parts.join(' ');
}

/** 1분 평균 줄(최근이 위). */
function minuteRows(samples) {
  const buckets = new Map();
  for (const s of samples) {
    const k = Math.floor(s.t / 60000) * 60000;
    let b = buckets.get(k);
    if (!b) { b = { t: k, n: 0, cpu: 0, gpu: 0, gn: 0, mem: 0, net: 0 }; buckets.set(k, b); }
    b.n += 1; b.cpu += s.cpu / cores(); b.mem += s.mem; b.net += s.netRx;
    const gv = s.gpu != null ? s.gpu : s.gpuSys;
    if (gv != null) { b.gpu += gv; b.gn += 1; }
  }
  return [...buckets.values()].sort((a, b) => b.t - a.t).map((b) => {
    const d = new Date(b.t);
    const time = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    return `<tr><th scope="row">${time}</th><td>${fmtPct(b.cpu / b.n)}</td><td>${b.gn ? fmtPct(b.gpu / b.gn) : '—'}</td><td>${fmtBytes(b.mem / b.n)}</td><td>${fmtRate(b.net / b.n)}</td></tr>`;
  }).join('');
}

/** 렌더된 패널에 그래프를 붙이고 실시간 표본을 받는다. 패널이 문서에서 빠지거나 숨으면 스스로 그만둔다. */
export async function mountUsagePanel(panel) {
  if (!panel || panel.dataset.mounted) return;
  panel.dataset.mounted = '1';
  const ok = await initUsage();
  if (!panel.isConnected) return;
  if (!ok) {
    panel.querySelector('.usage-body').innerHTML = '<p class="usage-note">이 환경에서는 사용률을 잴 수 없습니다.</p>';
    return;
  }
  const chart = new UsageChart(panel.querySelector('.usage-chart-host'), { height: 180, axes: true, label: '앱 사용률' });
  const groups = panel.querySelector('.up-groups');
  const table = panel.querySelector('.usage-table');
  let tableKey = '';
  const key = `settings-${Math.random().toString(36).slice(2)}`;

  const shown = () => panel.isConnected && !panel.closest('[hidden]');
  const render = () => {
    const m = view.metric === 'gpu' && gpuState() === 'system'
      ? { ...metric('gpu'), label: 'GPU(전체)', value: (x) => x.gpuSys }
      : metric(view.metric);
    const win = rangeMs();
    const end = usage.samples.length ? usage.samples[usage.samples.length - 1].t : Date.now();
    const rows = usage.samples.filter((s) => s.t >= end - win);
    const st = stats(rows, m);
    const unsupported = view.metric === 'gpu' && gpuState() === 'none';
    for (const k of ['now', 'avg', 'max', 'min']) panel.querySelector(`[data-st="${k}"]`).textContent = unsupported ? '지원 안 됨' : m.fmt(st[k]);
    const on = measuring();
    const state = panel.querySelector('[data-up-state]');
    state.textContent = on ? `실시간 · ${Math.round(((usage.status && usage.status.intervalMs) || 1000) / 1000)}초마다` : '측정 꺼짐';
    state.classList.toggle('off', !on);
    chart.set(usage.samples, m, {
      windowMs: win,
      intervalMs: (usage.status && usage.status.intervalMs) || 1000,
      emptyText: unsupported ? '이 기기에서는 GPU 사용률을 읽을 수 없습니다' : on ? '첫 표본을 기다리는 중…' : '측정이 꺼져 있습니다 — 아래 "사용률 측정"을 켜세요',
    });
    renderGroups(groups, usage.latest && usage.latest.groups);
    panel.querySelector('[data-up-note]').textContent = noteText();
    if (table.open) {
      const last = usage.samples[usage.samples.length - 1];
      const k = `${usage.samples.length ? Math.floor(last.t / 60000) : 0}:${usage.samples.length && usage.samples[0].t}`;
      if (k !== tableKey) { tableKey = k; table.querySelector('tbody').innerHTML = minuteRows(usage.samples) || '<tr><td colspan="5">기록이 없습니다</td></tr>'; }
    }
  };
  table.addEventListener('toggle', () => { tableKey = ''; render(); });

  const unsub = subscribeUsage(() => {
    if (!shown()) { stop(); return; }
    render();
  });
  let stopped = false;
  function stop() {
    if (stopped) return;
    stopped = true;
    unsub();
    watchUsage(key, false);
    chart.destroy();
    delete panel.dataset.mounted;
  }

  const pickRadio = (attr, value, focus) => {
    if (attr === 'up-metric') view.metric = value; else view.range = Number(value);
    for (const b of panel.querySelectorAll(`[data-${attr}]`)) {
      const sel = b.dataset[attr === 'up-metric' ? 'upMetric' : 'upRange'] === String(value);
      b.classList.toggle('active', sel);
      b.setAttribute('aria-checked', String(sel));
      b.tabIndex = sel ? 0 : -1;
      if (sel && focus) b.focus();
    }
    render();
  };
  panel.addEventListener('click', (e) => {
    const b = e.target.closest('[data-up-metric], [data-up-range]');
    if (!b) return;
    if (b.dataset.upMetric) pickRadio('up-metric', b.dataset.upMetric, false);
    else pickRadio('up-range', b.dataset.upRange, false);
  });
  panel.addEventListener('keydown', (e) => {
    const b = e.target.closest('[data-up-metric], [data-up-range]');
    if (!b) return;
    const attr = b.dataset.upMetric ? 'up-metric' : 'up-range';
    const all = [...panel.querySelectorAll(`[data-${attr}]`)];
    const i = all.indexOf(b);
    let j = -1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') j = (i + 1) % all.length;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') j = (i - 1 + all.length) % all.length;
    else if (e.key === 'Home') j = 0;
    else if (e.key === 'End') j = all.length - 1;
    if (j < 0) return;
    e.preventDefault();
    const n = all[j];
    pickRadio(attr, attr === 'up-metric' ? n.dataset.upMetric : n.dataset.upRange, true);
  });

  watchUsage(key, true);
  render();
  // 설정 화면이 숨어 있는 동안 표본이 오지 않으면(측정 꺼짐) 구독이 남는다 — 화면을 옮길 때 한 번 더 본다.
  const pageObs = new MutationObserver(() => { if (!shown()) { pageObs.disconnect(); if (panel.dataset.mounted) stop(); } });
  const page = panel.closest('.page');
  if (page) pageObs.observe(page, { attributes: true, attributeFilter: ['hidden'] });
  const bodyObs = new MutationObserver(() => { if (!panel.isConnected) { bodyObs.disconnect(); pageObs.disconnect(); if (panel.dataset.mounted) stop(); } });
  if (panel.parentElement && panel.parentElement.parentElement) bodyObs.observe(panel.parentElement.parentElement, { childList: true });
}

