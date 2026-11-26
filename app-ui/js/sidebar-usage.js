// 사이드바 아래 앱 사용률 — 계정 칸 위(새 버전 알림 줄 앞)에 단추 하나.
// 펼친 사이드바: 지표 4줄(CPU·GPU·메모리·네트워크 — 이름 · 작은 꺾은선 · 값). CPU 가 높으면 CPU 줄에
//   경고 아이콘·경고색 꺾은선·옅은 바탕(색만으로 알리지 않는다 — 아이콘과 낭독 글자 'CPU 높음').
// 접은 사이드바(아이콘 레일): 작은 CPU 꺾은선 + CPU % 한 칸. 레일 툴팁(sidebar.js data-tip)에 네 값.
// 누르면 설정 › 성능 탭으로 간다(자세한 그래프·프로세스별 내역·기록 내보내기는 거기에).
// 높이를 아낀다 — 사이드바에서 줄어드는 칸은 '방송 중' 목록뿐이라 이 칸이 클수록 그 목록이 먼저 사라진다(머리 줄 없이 4줄, 낮은 창은 usage.css 가 줄 수를 줄인다 — 2줄·1줄).
// 보이는 동안만 실시간 표본을 받는다(창이 숨으면 usage-monitor 가 끈다). 보조 창에는 달지 않는다.
// 갱신 비용(U17): 표본마다 보이는 쪽(펼침 4줄 / 접힘 1칸)만 글자·캔버스를 바꾼다. 꺾은선은 캔버스 — SVG path d 를 바꾸면
//   표본마다 레이아웃이 한 번 더 돌았다. 칸 크기는 값과 상태에 따라 바뀌지 않는다(U19 — 고정 폭 열·고정 폭 숫자).
import {
  usage, initUsage, watchUsage, subscribeUsage, inHeader, cores, METRICS, gpuState,
  fmtPct, fmtBytes, fmtRate, cpuLevel, sparkPath, niceMax,
} from './usage-monitor.js';
import { resolvedTheme } from './theme.js';

const SPARK_N = 40;
// 펼침 줄 꺾은선 — 사이드바 안쪽 폭(232 − 12×2)에서 칸 여백·이름·값 열(usage.css .su-row)을 뺀 고정 크기.
const ROW_W = 58;
const ROW_H = 14;
// 접힘 칸 꺾은선 — 레일 안쪽 폭(macOS 59 · Windows 51) 안에 들어가게.
const MINI_W = 36;
const MINI_H = 14;

let box = null;
let sidebar = null;
let installed = false;
let wasCollapsed = null;

const ICON_WARN = '<svg class="su-warn" viewBox="0 0 24 24" aria-hidden="true" fill="currentColor"><path d="M12 3.2 1.9 20.5h20.2L12 3.2Zm-1 6.3h2v5.4h-2V9.5Zm0 7.1h2v2h-2v-2Z"/></svg>';

/** 사이드바에 단다(한 번). 쓸 수 없으면(명령 없음·보조 창·사이드바 없음) 아무것도 하지 않는다. */
export async function installSidebarUsage() {
  if (installed) return;
  installed = true;
  sidebar = document.querySelector('.sidebar');
  if (!sidebar || !(await initUsage())) return;
  box = document.createElement('button');
  box.type = 'button';
  box.className = 'side-usage';
  box.hidden = true;
  box.dataset.level = 'idle';
  // 보이는 숫자는 aria-hidden — 완성된 문장형 aria-label 로만 읽게 해 WCAG 2.5.3 불일치를 막는다.
  box.innerHTML = `<span class="su-full" aria-hidden="true">
      ${METRICS.map((m) => `<span class="su-row" data-metric="${m.id}" style="--c: var(--u-${m.id})"><span class="su-k">${m.label}${m.id === 'cpu' ? ICON_WARN : ''}</span><canvas class="su-spark" width="${ROW_W}" height="${ROW_H}"></canvas><span class="su-v">—</span></span>`).join('')}
    </span>
    <span class="su-mini" aria-hidden="true"><canvas class="su-spark" width="${MINI_W}" height="${MINI_H}"></canvas><span class="su-mini-v">${ICON_WARN}<span class="su-v">—</span></span></span>`;
  box.addEventListener('click', () => {
    window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: { group: 'usage' } } }));
    window.dispatchEvent(new CustomEvent('cview:settings-group', { detail: { group: 'usage' } }));
  });
  sidebar.insertBefore(box, sidebar.querySelector('#update-pill') || sidebar.querySelector('#account'));
  subscribeUsage((kind) => {
    if (kind === 'config' || kind === 'status') syncVisible();
    render();
  });
  // 접기·펼치기 — 보이는 쪽이 바뀌면 그 쪽을 바로 그리고 툴팁 방식을 바꾼다.
  new MutationObserver(() => { if (collapsed() !== wasCollapsed) render(); }).observe(sidebar, { attributes: true, attributeFilter: ['class'] });
  syncVisible();
  render();
}

const collapsed = () => sidebar.classList.contains('collapsed');

function syncVisible() {
  if (!box) return;
  const show = inHeader();
  box.hidden = !show;
  watchUsage('sidebar', show);
}

/** 지표 값 — GPU 는 앱 단위로 못 재면 전체 GPU, 아무것도 못 재면 null. */
function valueOf(id, s) {
  if (!s) return null;
  if (id === 'cpu') return s.cpu / cores();
  if (id === 'gpu') { const g = gpuState(); return g === 'app' ? s.gpu : g === 'system' ? s.gpuSys : null; }
  if (id === 'mem') return s.mem;
  return s.netRx;
}

const FMT = { cpu: fmtPct, gpu: fmtPct, mem: fmtBytes, net: fmtRate };
const KIND = { cpu: 'pct', gpu: 'pct', mem: 'bytes', net: 'rate' };

/** 같으면 쓰지 않는다 — 초마다 같은 글자·속성을 다시 넣으면 문서 관찰자를 깨우고 레이아웃을 더럽힌다. */
function setText(el, text) {
  if (el && el.textContent !== text) el.textContent = text;
}
function setAttr(el, name, value) {
  if (value == null) { if (el.hasAttribute(name)) el.removeAttribute(name); } else if (el.getAttribute(name) !== value) el.setAttribute(name, value);
}

function render() {
  if (!box || box.hidden) return;
  const s = usage.latest && usage.latest.sample;
  const cpu = valueOf('cpu', s);
  const level = cpuLevel(cpu);
  if (box.dataset.level !== level) box.dataset.level = level;
  const folded = collapsed();
  const recent = usage.samples.slice(-SPARK_N);
  if (folded) {
    setText(box.querySelector('.su-mini .su-v'), fmtPct(cpu));
    drawRow(box.querySelector('.su-mini .su-spark'), recent, 'cpu', MINI_W, MINI_H, level);
  } else {
    for (const row of box.querySelectorAll('.su-row')) {
      const id = row.dataset.metric;
      setText(row.querySelector('.su-v'), FMT[id](valueOf(id, s)));
      drawRow(row.querySelector('.su-spark'), recent, id, ROW_W, ROW_H, level);
    }
  }
  // 이름·툴팁 — 네 값 모두.
  const g = gpuState();
  const gpuText = g === 'none' ? 'GPU 지원 안 됨' : `GPU${g === 'system' ? '(전체)' : ''} ${fmtPct(valueOf('gpu', s))}`;
  const parts = [`CPU ${fmtPct(cpu)}`, gpuText, `메모리 ${fmtBytes(valueOf('mem', s))}`, `네트워크 ${fmtRate(valueOf('net', s))}`];
  const warn = level === 'high' ? ' (CPU 높음)' : '';
  const label = `앱 사용률 — ${parts.join(', ')}${warn}. 설정 › 성능에서 자세히 보기`;
  // 이름은 초점이 있는 동안 바꾸지 않는다 — 화면 낭독기가 바뀐 이름을 초마다 다시 읽는다.
  if (document.activeElement !== box || !box.getAttribute('aria-label')) setAttr(box, 'aria-label', label);
  const tip = `${parts.join(' · ')}${warn} — 눌러서 설정 › 성능`;
  // 접힌 레일은 사이드바 툴팁(data-tip), 펼치면 기본 툴팁(title) — 둘이 함께 뜨지 않게.
  setAttr(box, 'data-tip', tip);
  setAttr(box, 'title', folded ? null : tip);
  wasCollapsed = folded;
}

// 꺾은선 색 — 지표 색 토큰(--u-*)·경고색은 테마로만 바뀐다. 초마다 getComputedStyle 로 읽으면 방금 바꾼 글자 때문에
// 스타일 계산을 강제했다. 테마가 같으면 지난 값을 쓴다.
let colors = { key: '', map: null };
function colorOf(id, level) {
  const key = resolvedTheme();
  if (colors.key !== key || !colors.map) {
    const cs = getComputedStyle(box);
    const map = { warn: cs.getPropertyValue('--warning').trim() };
    for (const m of METRICS) map[m.id] = cs.getPropertyValue(`--u-${m.id}`).trim();
    colors = { key, map };
  }
  return id === 'cpu' && level === 'high' ? colors.map.warn : colors.map[id];
}

/** 한 지표 꺾은선 — 선(1.5px)과 그 아래 옅은 면. 세로는 0..둥근 위 끝(niceMax). 화면 배율에 맞춰 선명하게. */
function drawRow(canvas, recent, id, w, h, level) {
  const g = canvas && canvas.getContext && canvas.getContext('2d');
  if (!g) return;
  const dpr = Math.max(1, Math.min(3, window.devicePixelRatio || 1));
  if (canvas.width !== Math.round(w * dpr)) { canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr); }
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  if (typeof Path2D !== 'function') return;
  let vals = recent.map((x) => valueOf(id, x));
  while (vals.length < SPARK_N) vals.unshift(null);
  const seen = vals.filter((v) => v != null);
  let top = niceMax(KIND[id], Math.max(0, ...seen));
  // 메모리는 0 부터 그리면 칸이 거의 꽉 찬 덩어리로만 보인다 — 최근 범위(최소의 90% ~ 최대)로 늘려 늘고 주는 것을 보인다.
  if (id === 'mem' && seen.length) {
    const floor = Math.min(...seen) * 0.9;
    top = Math.max(1, (Math.max(...seen) - floor) * 1.15);
    vals = vals.map((v) => (v == null ? null : v - floor));
  }
  const d = sparkPath(vals, w, h, top);
  if (!d) return;
  const color = colorOf(id, level) || '#888';
  const first = d.match(/^M([\d.]+)/);
  if (first) {
    g.globalAlpha = 0.16;
    g.fillStyle = color;
    g.fill(new Path2D(`${d}L${w} ${h}L${first[1]} ${h}Z`));
  }
  g.globalAlpha = 1;
  g.strokeStyle = color;
  g.lineWidth = 1.5;
  g.lineJoin = 'round';
  g.lineCap = 'round';
  g.stroke(new Path2D(d));
}
