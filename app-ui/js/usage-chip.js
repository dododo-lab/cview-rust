// 머리줄 사용률 칩 + 자세히 팝오버 — 지금 보이는 화면의 머리줄 오른쪽 묶음 앞에 칩 하나를 옮겨 단다.
// 칩: CPU 꺾은선(최근 40표본) · CPU % · 메모리. CPU 가 높으면 경고 아이콘과 낭독 글자(색만으로 알리지 않는다).
// 팝오버(비모달 대화 상자): 지표 칸 4개(CPU·GPU·메모리·네트워크 — 고르면 아래 그래프가 그 지표), 최근 2분 그래프, 종류별 내역,
//   설정 › 성능으로 가는 단추. Esc·바깥 누르기·초점이 나가면 닫히고 초점은 칩으로 돌아온다.
// 칩이 보이는 동안만 실시간 표본을 받는다(창이 숨으면 usage-monitor 가 끈다). 보조 창에는 달지 않는다.
import {
  usage, initUsage, watchUsage, subscribeUsage, inHeader, measuring, cores, METRICS, metric, gpuState,
  fmtPct, fmtBytes, fmtRate, cpuLevel, sparkPath, niceMax, stats, GROUP_LABELS, UsageChart,
} from './usage-monitor.js';
import { esc } from './dom.js';

const SPARK_N = 40;
const SPARK_W = 44;
const SPARK_H = 18;
const POP_WINDOW_MS = 120000;

let chip = null;
let pop = null;
let chart = null;
let picked = 'cpu';
let installed = false;

const ICON_WARN = '<svg class="uchip-warn" viewBox="0 0 24 24" aria-hidden="true" fill="currentColor"><path d="M12 3.2 1.9 20.5h20.2L12 3.2Zm-1 6.3h2v5.4h-2V9.5Zm0 7.1h2v2h-2v-2Z"/></svg>';

/** 칩을 만든다(한 번). 쓸 수 없으면(명령 없음·보조 창) 아무것도 하지 않는다. */
export async function installUsageChip() {
  if (installed) return;
  installed = true;
  if (!(await initUsage())) return;
  chip = document.createElement('button');
  chip.type = 'button';
  chip.className = 'usage-chip';
  chip.hidden = true;
  chip.setAttribute('aria-haspopup', 'dialog');
  chip.setAttribute('aria-expanded', 'false');
  // 꺾은선은 캔버스 — SVG path 의 d 를 바꾸면 표본마다 레이아웃이 한 번 더 돌았다(글자 갱신과 따로). 캔버스는 칠하기만 한다.
  chip.innerHTML = `<canvas class="uchip-spark" width="${SPARK_W}" height="${SPARK_H}" aria-hidden="true"></canvas>
    ${ICON_WARN}<span class="uchip-cpu"><span class="uchip-k">CPU</span><span class="uchip-v">—</span></span><span class="uchip-mem"></span>`;
  // 키보드로 열면(click detail 0) 고른 칸에, 마우스로 열면 대화 상자 자체에 초점 — 마우스 사용자에게 초점 고리를 띄우지 않는다.
  chip.addEventListener('click', (e) => (pop && !pop.hidden ? closePop() : openPop({ keyboard: e.detail === 0 })));
  subscribeUsage((kind) => {
    if (kind === 'config' || kind === 'status') syncVisible();
    renderChip();
    if (pop && !pop.hidden) renderPop();
  });
  placeUsageChip();
  syncVisible();
  renderChip();
  watchPages();
}

/**
 * 화면이 바뀌면(페이지·시청 화면의 hidden) 다음 프레임에 칩을 옮긴다 — 이동 경로(뒤로 가기·단독 보기·시청 화면 닫기…)마다
 * 부르지 않아도 되게. 페이지 요소의 hidden 속성만 본다(하위 트리는 보지 않는다).
 */
function watchPages() {
  const content = document.querySelector('.content');
  if (!content) return;
  let queued = false;
  const later = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; placeUsageChip(); });
  };
  const attrs = new MutationObserver(later);
  const observe = (node) => { if (node.nodeType === 1 && (node.matches('.page') || node.id === 'viewer')) attrs.observe(node, { attributes: true, attributeFilter: ['hidden'] }); };
  for (const node of content.children) observe(node);
  new MutationObserver((list) => {
    for (const m of list) for (const node of m.addedNodes) observe(node);
    later();
  }).observe(content, { childList: true });
}

/** 지금 보이는 화면 머리줄. 시청 화면(#viewer)·라이브 머리줄(.ml-header)·홈·일반 화면. */
function targetHeader() {
  const viewer = document.getElementById('viewer');
  if (viewer && !viewer.hidden) return viewer.querySelector('.viewer-header');
  for (const page of document.querySelectorAll('.content > .page')) {
    if (page.hidden) continue;
    const h = page.querySelector(':scope > .page-header, :scope > .home-header');
    if (h) return h;
  }
  return null;
}

/** 화면을 옮길 때(app.js navigate) 부른다 — 칩을 보이는 머리줄로 옮긴다. */
export function placeUsageChip() {
  if (!chip) return;
  const header = targetHeader();
  if (!header) { syncVisible(); return; }
  if (pop && !pop.hidden && chip.parentElement !== header) closePop({ focus: false });
  const toolbar = header.querySelector(':scope > .ml-toolbar');
  if (toolbar) {
    if (toolbar.firstElementChild !== chip) toolbar.prepend(chip);
  } else {
    // 오른쪽 묶음 앞 — 빈칸(.grow) 바로 뒤, 없으면 맨 끝(margin-inline-start:auto 가 오른쪽으로 민다).
    const grows = header.querySelectorAll(':scope > .grow');
    const after = grows[grows.length - 1];
    if (after) { if (after.nextElementSibling !== chip) after.after(chip); } else if (header.lastElementChild !== chip) header.append(chip);
  }
  // 빽빽한 머리줄(라이브 툴바·시청 화면)과 큰 인사말이 있는 홈은 작은 칩 — 꺾은선과 CPU 만(메모리는 팝오버에서).
  chip.classList.toggle('compact', Boolean(toolbar) || header.matches('.viewer-header, .home-header'));
  syncVisible();
}

function syncVisible() {
  if (!chip) return;
  const show = inHeader() && Boolean(chip.parentElement);
  chip.hidden = !show;
  if (!show && pop && !pop.hidden) closePop({ focus: false });
  watchUsage('chip', show);
}

function cpuNow() {
  const s = usage.latest && usage.latest.sample;
  return s ? s.cpu / cores() : null;
}

function renderChip() {
  if (!chip || chip.hidden) return;
  const s = usage.latest && usage.latest.sample;
  const cpu = cpuNow();
  const level = cpuLevel(cpu);
  chip.dataset.level = level;
  chip.querySelector('.uchip-v').textContent = fmtPct(cpu);
  chip.querySelector('.uchip-mem').textContent = s ? fmtBytes(s.mem) : '';
  const vals = usage.samples.slice(-SPARK_N).map((x) => x.cpu / cores());
  while (vals.length < SPARK_N) vals.unshift(null);
  const top = niceMax('pct', Math.max(0, ...vals.filter((v) => v != null)));
  drawSpark(chip.querySelector('.uchip-spark'), sparkPath(vals, SPARK_W, SPARK_H, top));
  const parts = [`CPU ${fmtPct(cpu)}`];
  if (s) parts.push(`메모리 ${fmtBytes(s.mem)}`);
  if (s && s.gpu != null) parts.push(`GPU ${fmtPct(s.gpu)}`);
  const warn = level === 'high' ? ' (CPU 높음)' : '';
  const label = `앱 사용률 — ${parts.join(', ')}${warn}. 자세히 보기`;
  // 이름은 초마다 바꾸지 않는다 — 초점이 칩에 있으면 화면 낭독기가 바뀐 이름을 다시 읽는다. 열 때 값은 팝오버가 보여 준다.
  if (document.activeElement !== chip || !chip.getAttribute('aria-label')) chip.setAttribute('aria-label', label);
  chip.title = `${parts.join(' · ')}${warn}`;
}

/** 칩 꺾은선 — 선(1.6px)과 그 아래 옅은 면. 색은 캔버스의 CSS color(지표·경고 색). 화면 배율에 맞춰 선명하게. */
function drawSpark(canvas, d) {
  const g = canvas.getContext && canvas.getContext('2d');
  if (!g) return;
  const dpr = Math.max(1, Math.min(3, window.devicePixelRatio || 1));
  if (canvas.width !== Math.round(SPARK_W * dpr)) { canvas.width = Math.round(SPARK_W * dpr); canvas.height = Math.round(SPARK_H * dpr); }
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, SPARK_W, SPARK_H);
  if (!d || typeof Path2D !== 'function') return;
  const color = getComputedStyle(canvas).color;
  const first = d.match(/^M([\d.]+)/);
  if (first) {
    g.globalAlpha = 0.14;
    g.fillStyle = color;
    g.fill(new Path2D(`${d}L${SPARK_W} ${SPARK_H}L${first[1]} ${SPARK_H}Z`));
  }
  g.globalAlpha = 1;
  g.strokeStyle = color;
  g.lineWidth = 1.6;
  g.lineJoin = 'round';
  g.lineCap = 'round';
  g.stroke(new Path2D(d));
}

// MARK: - 팝오버

function buildPop() {
  pop = document.createElement('div');
  pop.className = 'usage-pop';
  pop.id = 'usage-pop';
  pop.hidden = true;
  pop.setAttribute('role', 'dialog');
  pop.tabIndex = -1;
  pop.setAttribute('aria-labelledby', 'usage-pop-title');
  pop.innerHTML = `
    <header class="up-head">
      <div class="up-titles"><h2 id="usage-pop-title">앱 사용률</h2><p class="up-sub"></p></div>
      <span class="up-live" aria-hidden="true"><i></i>실시간</span>
    </header>
    <div class="up-tiles" role="radiogroup" aria-label="그래프로 볼 지표">
      ${METRICS.map((m) => `<button type="button" class="up-tile" role="radio" data-metric="${m.id}" style="--c: var(--u-${m.id})" aria-checked="false" tabindex="-1">
        <span class="up-tile-label"><i aria-hidden="true"></i>${m.label}</span><strong class="up-tile-value">—</strong><span class="up-tile-sub"></span></button>`).join('')}
    </div>
    <div class="up-chart"></div>
    <div class="up-groups-head"><span>프로세스별</span><span aria-hidden="true">CPU · 메모리</span></div>
    <ul class="up-groups" aria-label="프로세스별 사용률"></ul>
    <footer class="up-foot"><span class="up-note"></span><button type="button" class="btn sm" data-up="settings">기록·내보내기</button></footer>`;
  document.body.append(pop);
  chart = new UsageChart(pop.querySelector('.up-chart'), { height: 92, axes: true, label: '앱 사용률 최근 2분' });
  pop.querySelector('.up-tiles').addEventListener('click', (e) => {
    const t = e.target.closest('.up-tile');
    if (t && t.getAttribute('aria-disabled') !== 'true') pick(t.dataset.metric, false);
  });
  pop.querySelector('.up-tiles').addEventListener('keydown', (e) => onTilesKey(e));
  pop.querySelector('[data-up="settings"]').addEventListener('click', () => {
    closePop({ focus: false });
    window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: { group: 'usage' } } }));
    window.dispatchEvent(new CustomEvent('cview:settings-group', { detail: { group: 'usage' } }));
  });
  pop.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closePop(); }
  });
  pop.addEventListener('focusout', (e) => {
    if (e.relatedTarget && !pop.contains(e.relatedTarget) && e.relatedTarget !== chip) closePop({ focus: false });
  });
}

function onTilesKey(e) {
  const tiles = [...pop.querySelectorAll('.up-tile')].filter((t) => t.getAttribute('aria-disabled') !== 'true');
  const i = tiles.indexOf(document.activeElement);
  if (i < 0) return;
  let j = -1;
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') j = (i + 1) % tiles.length;
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') j = (i - 1 + tiles.length) % tiles.length;
  else if (e.key === 'Home') j = 0;
  else if (e.key === 'End') j = tiles.length - 1;
  if (j < 0) return;
  e.preventDefault();
  pick(tiles[j].dataset.metric, true);
}

function pick(id, focus) {
  picked = id;
  renderPop();
  if (focus) pop.querySelector(`.up-tile[data-metric="${id}"]`)?.focus();
}

function onOutside(e) {
  if (!pop || pop.hidden) return;
  if (pop.contains(e.target) || chip.contains(e.target)) return;
  closePop({ focus: false });
}

function onViewport() { if (pop && !pop.hidden) placePop(); }

function openPop({ keyboard = true } = {}) {
  if (!chip || chip.hidden) return;
  if (!pop) buildPop();
  pop.hidden = false;
  chip.setAttribute('aria-expanded', 'true');
  chip.setAttribute('aria-controls', 'usage-pop');
  watchUsage('popover', true);
  if (picked === 'gpu' && gpuState() === 'none') picked = 'cpu';
  renderPop();
  placePop();
  document.addEventListener('pointerdown', onOutside, true);
  window.addEventListener('resize', onViewport);
  if (keyboard) pop.querySelector(`.up-tile[data-metric="${picked}"]`)?.focus({ preventScroll: true });
  else pop.focus({ preventScroll: true });
}

function closePop({ focus = true } = {}) {
  if (!pop || pop.hidden) return;
  pop.hidden = true;
  chip.setAttribute('aria-expanded', 'false');
  chart && chart.unhover();
  watchUsage('popover', false);
  document.removeEventListener('pointerdown', onOutside, true);
  window.removeEventListener('resize', onViewport);
  if (focus && chip.isConnected && !chip.hidden) chip.focus();
}

/** 칩 아래, 칩 오른쪽 끝에 맞춰 연다 — 창 밖으로 나가면 안으로 민다. 위 공간이 더 넓고 아래가 모자라면 위로. */
function placePop() {
  const r = chip.getBoundingClientRect();
  const m = 8;
  pop.style.maxWidth = `${Math.max(240, window.innerWidth - m * 2)}px`;
  const w = pop.offsetWidth;
  const h = pop.offsetHeight;
  const left = Math.min(Math.max(m, r.right - w), window.innerWidth - w - m);
  const below = window.innerHeight - r.bottom - m;
  const up = below < h && r.top - m > below;
  pop.style.left = `${Math.round(left)}px`;
  pop.style.top = up ? `${Math.round(Math.max(m, r.top - 6 - h))}px` : `${Math.round(r.bottom + 6)}px`;
  pop.style.maxHeight = `${Math.round(up ? r.top - 6 - m : below - 6)}px`;
  pop.style.transformOrigin = up ? 'bottom right' : 'top right';
  pop.dataset.side = up ? 'top' : 'bottom';
}

function tileValues(m, s) {
  if (!s) return ['—', '측정 중…'];
  if (m.id === 'cpu') return [fmtPct(s.cpu / cores()), `코어 1개 기준 ${fmtPct(s.cpu)} · 코어 ${cores()}개`];
  if (m.id === 'gpu') {
    const g = gpuState();
    if (g === 'none') return ['지원 안 됨', '이 기기에서는 GPU 값을 읽을 수 없어요'];
    if (g === 'probing') return ['—', '확인 중…'];
    if (g === 'system') return [fmtPct(s.gpuSys), '전체 GPU — 앱별 값은 지원 안 됨'];
    return [fmtPct(s.gpu), s.gpuSys != null ? `전체 GPU ${fmtPct(s.gpuSys)}` : '이 앱'];
  }
  if (m.id === 'mem') {
    // GPU 메모리 범위(시스템 전체·앱)는 설정 › 성능 설명에 — 칸에는 짧게.
    const gm = s.gpuMem != null ? ` · GPU ${fmtBytes(s.gpuMem)}` : '';
    return [fmtBytes(s.mem), `프로세스 ${s.procs}개${gm}`];
  }
  const st = stats(usage.samples.slice(-60), m);
  return [fmtRate(s.netRx), st.max != null ? `최근 1분 최대 ${fmtRate(st.max)}` : '앱이 받은 양'];
}

function renderPop() {
  if (!pop) return;
  const s = usage.latest && usage.latest.sample;
  const st = usage.status || {};
  const helpers = Math.max(0, ((s && s.procs) || 1) - 1);
  const every = Math.round((st.intervalMs || 1000) / 1000);
  pop.querySelector('.up-sub').textContent = measuring()
    ? `CView와 도우미 ${helpers}개 · ${every}초마다`
    : '측정이 꺼져 있습니다';
  pop.querySelector('.up-live').hidden = !measuring();
  const gpuNone = gpuState() === 'none';
  if (picked === 'gpu' && gpuNone) picked = 'cpu';
  for (const t of pop.querySelectorAll('.up-tile')) {
    const m = metric(t.dataset.metric);
    const [value, sub] = tileValues(m, s);
    const on = m.id === picked;
    t.setAttribute('aria-checked', String(on));
    t.tabIndex = on ? 0 : -1;
    t.classList.toggle('unsupported', m.id === 'gpu' && gpuNone);
    if (m.id === 'gpu' && gpuNone) t.setAttribute('aria-disabled', 'true'); else t.removeAttribute('aria-disabled');
    t.querySelector('.up-tile-value').textContent = value;
    const subEl = t.querySelector('.up-tile-sub');
    subEl.textContent = sub;
    subEl.title = sub; // 좁아서 잘리면 가리켜 전체 글
  }
  // GPU 를 앱 단위로 못 재면(시스템 전체만) 그래프는 전체 GPU 로.
  const m = picked === 'gpu' && gpuState() === 'system'
    ? { ...metric('gpu'), label: 'GPU(전체)', value: (x) => x.gpuSys }
    : metric(picked);
  chart.set(usage.samples, m, { windowMs: POP_WINDOW_MS, intervalMs: st.intervalMs || 1000, emptyText: measuring() ? '첫 표본을 기다리는 중…' : '측정이 꺼져 있습니다' });
  renderGroups(pop.querySelector('.up-groups'), usage.latest && usage.latest.groups);
  pop.querySelector('.up-note').textContent = measuring() ? `최근 ${st.historyMinutes || 15}분 기록 중` : '설정 › 성능에서 켤 수 있습니다';
}

/** 종류별 내역 — 이름 · CPU(전체 대비) · 메모리, 가는 막대는 앱 전체 CPU 중 몫. 설정 패널도 쓴다. */
export function renderGroups(list, groups) {
  if (!list) return;
  const rows = (groups || []).filter((g) => g.count > 0);
  const total = rows.reduce((a, g) => a + g.cpu, 0);
  const html = rows.map((g) => {
    const share = total > 0 ? Math.round((g.cpu / total) * 100) : 0;
    const name = GROUP_LABELS[g.kind] || g.kind;
    return `<li data-kind="${esc(g.kind)}"><span class="ug-name">${name}${g.count > 1 ? `<small>${g.count}개</small>` : ''}</span>
      <span class="ug-bar" aria-hidden="true"><i style="width:${share}%"></i></span>
      <span class="ug-cpu">${fmtPct(g.cpu / cores())}</span><span class="ug-mem">${fmtBytes(g.mem)}</span></li>`;
  }).join('') || '<li class="ug-empty">아직 값이 없습니다</li>';
  if (list.dataset.html !== html) { list.innerHTML = html; list.dataset.html = html; }
}

