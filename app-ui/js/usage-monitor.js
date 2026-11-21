// 앱 사용률 — Rust `usage` 모듈(상태·기록·실시간 표본)을 머리줄 칩·팝오버·설정 › 성능이 함께 쓰는 저장소와 그래프.
// - 실시간 표본(usage://sample)은 누군가 보고 있고(watchUsage) 창이 보일 때만 받는다. 숨으면 usage_watch(false) 로
//   Rust 가 이벤트를 보내지 않고(기록은 켜져 있으면 계속), 다시 보이면 그동안의 기록을 usage_history(since) 로 채운다.
// - 명령이 없거나 실패하면(일반 브라우저·옛 백엔드) initUsage 가 false — 칩·패널은 그리지 않는다. 타이머도 두지 않는다.
// - 값 글자는 aria-live 로 읽지 않는다(초마다 바뀐다). 그래프는 role=img 요약 + 키보드로 점을 옮기며 읽는 말풍선.
import { api, listen, Events, isTauri } from './api.js';

const store = {
  status: null, // UsageStatus — enabled·intervalMs·historyMinutes·cores·platform·gpu·latest
  appearance: null, // 외관 설정(usageMonitor·usageInHeader …)
  samples: [], // 시간순 표본
  latest: null, // 마지막 LiveUsage(종류별 내역 포함)
  subs: new Set(),
  watchers: new Set(),
  live: null, // Rust 에 마지막으로 보낸 실시간 받기(null = 모름 — 다음 맞추기에서 꼭 보낸다)
  liveSeq: 0,
  retry: 0,
  ready: null,
};

/** 지금 상태(읽기 전용으로 쓴다). */
export const usage = store;

/** 한 번만 — 상태·설정·기록을 읽고 이벤트를 단다. 쓸 수 없으면 false. */
export function initUsage() {
  if (store.ready) return store.ready;
  store.ready = (async () => {
    if (!isTauri || document.documentElement.dataset.window) return false;
    try {
      // 상태를 먼저 — 명령이 없는 환경(옛 백엔드·검사용 모의 IPC)에서는 요청 하나로 끝난다.
      const status = await api.usageStatus();
      const [settings, history] = await Promise.all([api.settingsGet(), api.usageHistory(null)]);
      store.status = status;
      store.appearance = (settings && settings.appearance) || {};
      const all = (history && history.samples) || [];
      // 기록 기간으로 자른 채 시작한다 — 첫 표본에서 한꺼번에 잘리며 그래프·표가 튀지 않게.
      const last = all[all.length - 1];
      store.samples = last ? all.filter((s) => s.t >= last.t - keepMs()) : all;
      store.latest = status.latest || null;
    } catch {
      return false;
    }
    listen(Events.usageSample, (live) => {
      if (!live || !live.sample) return;
      store.latest = live;
      push(live.sample);
      notify('sample');
    });
    listen(Events.appearanceChanged, (a) => onAppearance(a));
    document.addEventListener('visibilitychange', () => syncLive());
    // 창을 다시 불러오면 Rust 쪽 실시간 받기가 켜진 채 남아 있을 수 있다 — 부른 쪽이 watchUsage 를 마친 뒤 한 번 맞춘다.
    setTimeout(() => syncLive(), 0);
    return true;
  })();
  return store.ready;
}

function keepMs() {
  return Math.max(1, (store.status && store.status.historyMinutes) || 15) * 60000;
}

/** 채운 기록을 시각 순으로 끼워 넣는다(같은 시각은 하나) — 채우는 동안 들어온 실시간 표본이 있어도 버리지 않게. */
function merge(list) {
  if (!list || !list.length) return;
  const byT = new Map(store.samples.map((s) => [s.t, s]));
  for (const s of list) byT.set(s.t, s);
  const a = [...byT.values()].sort((x, y) => x.t - y.t);
  const oldest = a[a.length - 1].t - keepMs();
  store.samples = a.filter((s) => s.t >= oldest);
}

function push(s) {
  const a = store.samples;
  const last = a[a.length - 1];
  if (last && s.t <= last.t) return; // 채우기(backfill)와 이벤트가 겹친 표본
  a.push(s);
  const oldest = s.t - keepMs();
  let cut = 0;
  while (cut < a.length && a[cut].t < oldest) cut += 1;
  if (cut) a.splice(0, cut);
}

async function onAppearance(a) {
  if (!a || typeof a !== 'object') return;
  const prev = store.appearance || {};
  store.appearance = { ...prev, ...a };
  const changed = ['usageMonitor', 'usageIntervalMs', 'usageHistoryMinutes'].some((k) => a[k] !== undefined && a[k] !== prev[k]);
  if (changed) {
    try { store.status = await api.usageStatus(); } catch { /* 그대로 */ }
    const oldest = Date.now() - keepMs();
    store.samples = store.samples.filter((s) => s.t >= oldest);
  }
  notify('config');
  syncLive();
}

/** 화면 한 곳(key)이 실시간 표본을 원하는지. 모두 그만두거나 창이 숨으면 Rust 가 보내지 않는다. */
export function watchUsage(key, on) {
  if (on) store.watchers.add(key); else store.watchers.delete(key);
  syncLive();
}

function wanted() {
  return Boolean(store.status && store.status.enabled && store.watchers.size && document.visibilityState !== 'hidden');
}

async function syncLive() {
  if (!store.status) return;
  const want = wanted();
  if (want === store.live) return;
  store.live = want;
  const seq = ++store.liveSeq;
  clearTimeout(store.retry);
  // 숨어 있던 동안 쌓인 기록은 켜기 전 마지막 표본 뒤부터 채운다(그동안 이벤트는 오지 않았다).
  const last = store.samples[store.samples.length - 1];
  try {
    const st = await api.usageWatch(want);
    if (seq !== store.liveSeq) return;
    store.status = st;
    if (want) {
      const h = await api.usageHistory(last ? last.t : null);
      merge(h && h.samples);
      if (st.latest && (!store.latest || st.latest.sample.t >= store.latest.sample.t)) store.latest = st.latest;
    }
  } catch {
    // 보내지 못했다 — 모름으로 되돌리고 잠시 뒤 다시 맞춘다(안 그러면 원하는 상태가 바뀔 때까지 칩이 멈춰 있었다).
    if (seq === store.liveSeq) {
      store.live = null;
      store.retry = setTimeout(() => syncLive(), 5000);
    }
  }
  notify('status');
}

/** 바뀔 때마다 fn(kind) — kind: sample · status · config · clear. 해제 함수를 돌려준다. */
export function subscribeUsage(fn) {
  store.subs.add(fn);
  return () => store.subs.delete(fn);
}

function notify(kind) {
  for (const fn of [...store.subs]) {
    try { fn(kind); } catch (e) { console.error('사용률 화면 갱신 실패', e); }
  }
}

export async function clearUsage() {
  await api.usageClear();
  store.samples = [];
  store.latest = null;
  notify('clear');
}

export const exportUsage = (format) => api.usageExport(format);

// MARK: - 값·글자

export const cores = () => Math.max(1, (store.status && store.status.cores) || 1);
export const measuring = () => Boolean(store.status && store.status.enabled);
export const inHeader = () => measuring() && (store.appearance ? store.appearance.usageInHeader !== false : true);

const MB = 1048576;
const GB = 1073741824;

export function fmtPct(v) {
  if (v == null || !Number.isFinite(v)) return '—';
  return `${v < 10 ? (Math.round(v * 10) / 10).toFixed(1) : Math.round(v)}%`;
}

export function fmtBytes(b) {
  if (b == null || !Number.isFinite(b)) return '—';
  if (b < GB) return `${Math.round(b / MB)} MB`;
  return `${(b / GB).toFixed(b < 10 * GB ? 2 : 1)} GB`;
}

/** 바이트/초 → 비트 속도(영상 화질과 같은 단위). */
export function fmtRate(bps) {
  if (bps == null || !Number.isFinite(bps)) return '—';
  const bits = bps * 8;
  if (bits < 1e6) return `${Math.round(bits / 1e3)} kbps`;
  return `${(bits / 1e6).toFixed(bits < 1e8 ? 1 : 0)} Mbps`;
}

/** 지표 — 그래프·칸·표가 같은 정의를 쓴다. value(s) 가 null 이면 '없음'. */
export const METRICS = [
  { id: 'cpu', label: 'CPU', value: (s) => s.cpu / cores(), fmt: fmtPct, kind: 'pct' },
  { id: 'gpu', label: 'GPU', value: (s) => (s.gpu == null ? null : s.gpu), fmt: fmtPct, kind: 'pct' },
  { id: 'mem', label: '메모리', value: (s) => s.mem, fmt: fmtBytes, kind: 'bytes' },
  { id: 'net', label: '네트워크', value: (s) => s.netRx, fmt: fmtRate, kind: 'rate' },
];
export const metric = (id) => METRICS.find((m) => m.id === id) || METRICS[0];

/** GPU 를 앱 단위로 잴 수 있나 / 아무것도 못 재나. */
export function gpuState() {
  const g = (store.status && store.status.gpu) || {};
  if (!g.probed) return 'probing';
  if (g.app) return 'app';
  if (g.system) return 'system';
  return 'none';
}

export const GROUP_LABELS = { app: '앱 본체', web: '웹 화면', gpu: 'GPU 처리', network: '네트워크', other: '기타 도우미' };

/** CPU 수준 — 높음이면 칩이 경고 모양(아이콘 + 낭독 글자). 색만으로 알리지 않는다. */
export function cpuLevel(pct) {
  if (pct == null) return 'idle';
  if (pct >= 80) return 'high';
  if (pct >= 50) return 'busy';
  return 'ok';
}

/** 그래프 세로축 위 끝 — 읽기 좋은 둥근 값. */
export function niceMax(kind, max) {
  const m = Number.isFinite(max) ? max : 0;
  const pick = (steps) => steps.find((s) => s >= m) || steps[steps.length - 1];
  if (kind === 'pct') return pick([5, 10, 25, 50, 75, 100]);
  if (kind === 'bytes') {
    const steps = [256, 512, 1024, 1536, 2048, 3072, 4096, 6144, 8192, 12288, 16384, 32768, 65536].map((x) => x * MB);
    return m > steps[steps.length - 1] ? Math.ceil(m / (8 * GB)) * 8 * GB : pick(steps);
  }
  // 속도(바이트/초) — 1·2·5 Mbps 단계
  const mbps = [0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000].map((x) => (x * 1e6) / 8);
  return pick(mbps);
}

/** 통계 — 지금·평균·최대·최소(값 없는 표본은 건너뛴다). */
export function stats(samples, m) {
  let n = 0; let sum = 0; let max = -Infinity; let min = Infinity; let now = null;
  for (const s of samples) {
    const v = m.value(s);
    if (v == null || !Number.isFinite(v)) continue;
    n += 1; sum += v; if (v > max) max = v; if (v < min) min = v; now = v;
  }
  return n ? { now, avg: sum / n, max, min, n } : { now: null, avg: null, max: null, min: null, n: 0 };
}

/** 작은 꺾은선(칩·칸) — 점 목록 → path d. 세로는 0..max, 값 없는 표본은 끊는다. */
export function sparkPath(values, w, h, max) {
  const n = values.length;
  if (!n) return '';
  const top = max > 0 ? max : 1;
  const step = n > 1 ? w / (n - 1) : 0;
  let d = ''; let pen = false;
  for (let i = 0; i < n; i += 1) {
    const v = values[i];
    if (v == null || !Number.isFinite(v)) { pen = false; continue; }
    const x = n > 1 ? i * step : w;
    const y = h - 1 - Math.min(1, Math.max(0, v / top)) * (h - 2);
    d += `${pen ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`;
    pen = true;
  }
  return d;
}

const SVG_NS = 'http://www.w3.org/2000/svg';
const el = (tag, cls, ns) => { const e = ns ? document.createElementNS(SVG_NS, tag) : document.createElement(tag); if (cls) e.setAttribute('class', cls); return e; };

function clock(t) {
  const d = new Date(t);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}:${String(d.getSeconds()).padStart(2, '0')}`;
}

function ago(ms) {
  const s = Math.round(ms / 1000);
  if (s < 90) return `${s}초 전`;
  const m = Math.round(s / 60);
  return m < 90 ? `${m}분 전` : `${Math.round(m / 60)}시간 전`;
}

/**
 * 한 지표 꺾은선 + 옅은 면 그래프. 세로축 0 과 위 끝, 가로축 시작·지금 글자, 가는 격자 3줄.
 * 포인터를 올리면 세로선이 가장 가까운 표본에 붙고 말풍선(값 굵게 · 시각)이 뜬다. 초점이 있으면 ←/→/Home/End 로 같은 말풍선.
 * 데이터가 바뀌어도(초마다) 가리키던 시각에 머문다.
 */
export class UsageChart {
  constructor(host, { height = 120, axes = true, label = '사용률 그래프' } = {}) {
    this.host = host;
    this.height = height;
    this.axes = axes;
    this.label = label;
    this.samples = [];
    this.metric = METRICS[0];
    this.windowMs = 120000;
    this.intervalMs = 1000;
    this.hoverT = null; // 가리키는 표본 시각(포인터·키보드)
    this.root = el('div', `uc${axes ? ' uc-axes' : ''}`);
    this.root.style.setProperty('--uc-h', `${height}px`);
    this.svg = el('svg', 'uc-svg', true);
    this.svg.setAttribute('role', 'img');
    this.svg.setAttribute('focusable', 'false');
    this.grid = el('g', 'uc-grid', true);
    this.area = el('path', 'uc-area', true);
    this.line = el('path', 'uc-line', true);
    this.cross = el('line', 'uc-cross', true);
    this.dot = el('circle', 'uc-dot', true);
    this.dot.setAttribute('r', '4');
    this.svg.append(this.grid, this.area, this.line, this.cross, this.dot);
    this.plot = el('div', 'uc-plot');
    this.plot.tabIndex = 0;
    this.plot.append(this.svg);
    this.tip = el('div', 'uc-tip');
    this.tip.hidden = true;
    this.tipValue = el('strong');
    this.tipTime = el('span');
    this.tip.append(this.tipValue, this.tipTime);
    this.plot.append(this.tip);
    this.yTop = el('span', 'uc-y-top');
    this.yZero = el('span', 'uc-y-zero');
    this.xStart = el('span', 'uc-x-start');
    this.xEnd = el('span', 'uc-x-end');
    this.empty = el('div', 'uc-empty');
    this.empty.hidden = true;
    if (axes) {
      const y = el('div', 'uc-y'); y.setAttribute('aria-hidden', 'true'); y.append(this.yTop, this.yZero);
      const x = el('div', 'uc-x'); x.setAttribute('aria-hidden', 'true'); x.append(this.xStart, this.xEnd);
      this.root.append(y, this.plot, x);
    } else {
      this.root.append(this.plot);
    }
    this.plot.append(this.empty);
    host.append(this.root);
    this.width = 0;
    this.ro = new ResizeObserver(() => { const w = Math.round(this.plot.clientWidth); if (w && w !== this.width) { this.width = w; this.draw(); } });
    this.ro.observe(this.plot);
    this.plot.addEventListener('pointermove', (e) => this.pointAt(e.clientX));
    this.plot.addEventListener('pointerleave', () => { if (document.activeElement !== this.plot) this.unhover(); });
    this.plot.addEventListener('blur', () => this.unhover());
    this.plot.addEventListener('keydown', (e) => this.onKey(e));
  }

  /** 표본·지표·기간을 바꾸고 다시 그린다. */
  set(samples, metricDef, { windowMs = this.windowMs, intervalMs = this.intervalMs, emptyText = '아직 기록이 없습니다' } = {}) {
    this.samples = samples;
    this.metric = metricDef;
    this.windowMs = windowMs;
    this.intervalMs = intervalMs;
    this.emptyText = emptyText;
    this.root.style.setProperty('--uc-color', `var(--u-${metricDef.id})`);
    this.draw();
  }

  destroy() { this.ro.disconnect(); this.root.remove(); }

  visible() {
    const end = this.samples.length ? this.samples[this.samples.length - 1].t : Date.now();
    const start = end - this.windowMs;
    let i = this.samples.length;
    while (i > 0 && this.samples[i - 1].t >= start) i -= 1;
    return { start, end, rows: this.samples.slice(i) };
  }

  draw() {
    const w = this.width || Math.round(this.plot.clientWidth);
    if (!w) return;
    const h = this.height;
    const m = this.metric;
    const { start, end, rows } = this.visible();
    this.rows = rows;
    this.span = { start, end };
    this.svg.setAttribute('viewBox', `0 0 ${w} ${h}`);
    this.svg.setAttribute('width', String(w));
    this.svg.setAttribute('height', String(h));
    const st = stats(rows, m);
    const top = niceMax(m.kind, st.max == null ? 0 : st.max);
    this.top = top;
    const pad = 3; // 2px 선이 위·아래 끝에서 잘리지 않게
    const x = (t) => ((t - start) / Math.max(1, end - start)) * w;
    const y = (v) => h - pad - Math.min(1, Math.max(0, v / top)) * (h - pad * 2);
    this.x = x; this.y = y;
    // 격자 — 0·절반·위 끝(가는 실선, 물러난 색)
    if (this.gridKey !== `${w}x${h}`) {
      this.gridKey = `${w}x${h}`;
      this.grid.replaceChildren(...[0, 0.5, 1].map((f) => {
        const l = el('line', '', true);
        const yy = Math.round(h - pad - f * (h - pad * 2)) + 0.5;
        l.setAttribute('x1', '0'); l.setAttribute('x2', String(w)); l.setAttribute('y1', String(yy)); l.setAttribute('y2', String(yy));
        return l;
      }));
    }
    // 한 픽셀 열에 표본이 여럿이면 가장 큰 값만(짧은 치솟음을 잃지 않게). 간격의 3배보다 벌어지면 선을 끊는다.
    const gap = Math.max(3 * this.intervalMs, 2500);
    let line = ''; let area = ''; let pen = false; let runStart = 0; let lastX = -1; let lastY = 0; let prevT = null;
    let colX = -1; let colY = Infinity;
    const flush = () => {
      if (colX < 0) return;
      line += `${pen ? 'L' : 'M'}${colX.toFixed(1)} ${colY.toFixed(1)}`;
      if (!pen) { area += `M${colX.toFixed(1)} ${h}L${colX.toFixed(1)} ${colY.toFixed(1)}`; runStart = colX; } else area += `L${colX.toFixed(1)} ${colY.toFixed(1)}`;
      pen = true; lastX = colX; lastY = colY; colX = -1; colY = Infinity;
    };
    const close = () => { if (pen) area += `L${lastX.toFixed(1)} ${h}L${runStart.toFixed(1)} ${h}Z`; pen = false; };
    for (const s of rows) {
      const v = m.value(s);
      if (prevT != null && s.t - prevT > gap) { flush(); close(); }
      prevT = s.t;
      if (v == null || !Number.isFinite(v)) { flush(); close(); continue; }
      const px = Math.round(x(s.t));
      const py = y(v);
      if (px !== colX) { flush(); colX = px; colY = py; } else colY = Math.min(colY, py);
    }
    flush(); close();
    this.line.setAttribute('d', line);
    this.area.setAttribute('d', area);
    const last = rows.length ? rows[rows.length - 1] : null;
    const lastV = last ? m.value(last) : null;
    const showDot = last && lastV != null && Number.isFinite(lastV);
    this.dot.style.display = showDot ? '' : 'none';
    if (showDot) { this.dot.setAttribute('cx', x(last.t).toFixed(1)); this.dot.setAttribute('cy', y(lastV).toFixed(1)); }
    const hasData = st.n > 0;
    this.empty.hidden = hasData;
    this.empty.textContent = this.emptyText;
    if (this.axes) {
      this.yTop.textContent = m.fmt(top);
      this.yZero.textContent = m.kind === 'pct' ? '0%' : '0';
      this.xStart.textContent = ago(end - start);
      this.xEnd.textContent = '지금';
    }
    // 낭독 요약 — 값이 바뀌어도 알리지 않는다(role=img, live 아님).
    const summary = hasData
      ? `${this.label}: ${m.label} 최근 ${ago(end - start).replace(' 전', '')} — 지금 ${m.fmt(st.now)}, 평균 ${m.fmt(st.avg)}, 최대 ${m.fmt(st.max)}`
      : `${this.label}: ${this.emptyText}`;
    this.plot.setAttribute('aria-label', `${summary}. 화살표 키로 시점별 값을 볼 수 있습니다`);
    this.svg.setAttribute('aria-label', summary);
    if (this.hoverT != null) this.showAt(this.nearest(this.hoverT));
  }

  nearest(t) {
    const rows = this.rows || [];
    if (!rows.length) return -1;
    let lo = 0; let hi = rows.length - 1;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (rows[mid].t < t) lo = mid + 1; else hi = mid; }
    if (lo > 0 && Math.abs(rows[lo - 1].t - t) < Math.abs(rows[lo].t - t)) lo -= 1;
    return lo;
  }

  pointAt(clientX) {
    if (!this.rows || !this.rows.length || !this.span) return;
    const r = this.plot.getBoundingClientRect();
    const t = this.span.start + ((clientX - r.left) / Math.max(1, r.width)) * (this.span.end - this.span.start);
    const i = this.nearest(t);
    this.hoverT = i >= 0 ? this.rows[i].t : null;
    this.showAt(i);
  }

  showAt(i) {
    const s = i >= 0 && this.rows ? this.rows[i] : null;
    if (!s) { this.unhover(); return; }
    const v = this.metric.value(s);
    const cx = this.x(s.t);
    this.cross.setAttribute('x1', cx.toFixed(1)); this.cross.setAttribute('x2', cx.toFixed(1));
    this.cross.setAttribute('y1', '0'); this.cross.setAttribute('y2', String(this.height));
    this.cross.style.display = 'block';
    this.tipValue.textContent = v == null ? '값 없음' : this.metric.fmt(v);
    this.tipTime.textContent = clock(s.t);
    this.tip.hidden = false;
    const w = this.width || this.plot.clientWidth;
    const tw = this.tip.offsetWidth;
    const left = Math.min(Math.max(cx - tw / 2, 0), Math.max(0, w - tw));
    this.tip.style.left = `${Math.round(left)}px`;
    this.root.classList.add('hovering');
  }

  unhover() {
    this.hoverT = null;
    this.tip.hidden = true;
    this.cross.style.display = 'none';
    this.root.classList.remove('hovering');
  }

  onKey(e) {
    const rows = this.rows || [];
    if (!rows.length) return;
    let i = this.hoverT == null ? rows.length - 1 : this.nearest(this.hoverT);
    const step = Math.max(1, Math.round(rows.length / 60));
    if (e.key === 'ArrowLeft') i = Math.max(0, i - (e.shiftKey ? step * 10 : step));
    else if (e.key === 'ArrowRight') i = Math.min(rows.length - 1, i + (e.shiftKey ? step * 10 : step));
    else if (e.key === 'Home') i = 0;
    else if (e.key === 'End') i = rows.length - 1;
    else if (e.key === 'Escape' && this.hoverT != null) { e.stopPropagation(); this.unhover(); return; }
    else return;
    e.preventDefault();
    this.hoverT = rows[i].t;
    this.showAt(i);
  }
}
