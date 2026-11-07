// 채팅 폭주(초당 200개) CPU 프로파일 — 함수별 자기 시간 상위. 저장소 루트에서: node perf/tools/ui/chatprof.mjs "$PWD" 4  (XCSS 로 시험 CSS 주입)
import { createRequire } from 'node:module';
import path from 'node:path';
const repo = process.argv[2];
const RATE = Number(process.argv[3] || 1);
const { base, mockTauri, go } = await import(path.join(repo, 'tools/ui-regression/mock-app.mjs'));
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const browser = await chromium.launch();
const out = {};
const q = (a, p) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : NaN; };

// 이벤트 버스(모의 listen 은 아무것도 안 함) — mockTauri 뒤에 덮어쓴다
function bus() {
  const m = new Map();
  window.__TAURI__.event = {
    listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); const w = (e) => f(e); m.get(n).add(w); return () => m.get(n).delete(w); },
    emit: async (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); },
  };
  window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
}

async function open({ width = 1280, height = 900 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  if (RATE > 1) await cdp.send('Emulation.setCPUThrottlingRate', { rate: RATE });
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(bus);
  const reqs = [];
  page.on('request', (r) => reqs.push({ url: r.url(), type: r.resourceType() }));
  return { ctx, page, cdp, reqs };
}

// ── 화면 전환·노드·힙 ──
{
  const { ctx, page, cdp } = await open();
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 30000 });
  await page.waitForTimeout(1000);
  await cdp.send('Performance.enable');
  const routes = ['following', 'popular', 'categories', 'library', 'stats', 'settings', 'multichat', 'multilive', 'search', 'home'];
  out.nav = {};
  for (let pass = 0; pass < 0; pass++) for (const r of routes) {
    const m0 = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    const ms = await page.evaluate(async (route) => {
      const t0 = performance.now();
      window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route, params: {} } }));
      await new Promise((res) => requestAnimationFrame(() => setTimeout(res, 0)));
      return performance.now() - t0;
    }, r);
    await page.waitForTimeout(700);
    const m1 = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    const nodes = await page.evaluate(() => document.getElementsByTagName('*').length);
    (out.nav[r] ||= []).push({ ms: Math.round(ms), script: Math.round((m1.ScriptDuration - m0.ScriptDuration) * 1000), layout: Math.round((m1.LayoutDuration - m0.LayoutDuration) * 1000), nodes });
  }
  for (const r of []) { const a = out.nav[r]; out.nav[r] = { first: a[0], warmMs: q(a.slice(1).map((x) => x.ms), 0.5), nodes: a[2].nodes }; }
  // ── 채팅 폭주 처리량(본 시청 화면 채팅 패널) ──
  await page.evaluate((c) => { window.__CSS = c; }, process.env.XCSS || '');
  await page.evaluate(async () => {
    const inv = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async (c, a) => { try { return await inv(c, a); } catch { return null; } };
    const { ChatPanel } = await import('/js/chat.js');
    const src = document.querySelector('#viewer .chat') || document.querySelector('.chat');
    const box = document.createElement('div'); box.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:860px;z-index:9999;display:flex;flex-direction:column;background:#111';
    box.innerHTML = src.outerHTML; document.body.appendChild(box);
    const root = box.firstElementChild; root.hidden = false; root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    root.querySelectorAll('.chat-tools,.chat-tools-toggle').forEach((x) => x.remove());
    if (window.__CSS) { const st=document.createElement('style'); st.textContent=window.__CSS; document.head.appendChild(st); }
    window.__panel = new ChatPanel(root, { isLoggedIn: () => true });
    await window.__panel.connect('cc1', 'ch1').catch(() => {});
    window.__panel.active = true; window.__panel.chatChannelId = null;
  });
  out.chat = [];
  await cdp.send('Profiler.enable'); await cdp.send('Profiler.setSamplingInterval',{interval:200}); await cdp.send('Profiler.start');
  for (const rate of [200]) {
    const r = await page.evaluate(async (rate) => {
      let n = 0;
      const mk = () => ({ id: `syn-${++n}`, kind: 'chat', nickname: `테스트${n % 50}`, content: `합성 메시지 ${n} 가나다라마바사 아자차카타파하 ㅋㅋㅋㅋ {:d_1:}`, emojis: { d_1: '/__img/avatar/1.svg' }, role: 'common_user', badges: [], time: Date.now() });
      const d = []; let last = performance.now(); let run = true;
      const tick = (t) => { d.push(t - last); last = t; if (run) requestAnimationFrame(tick); }; requestAnimationFrame(tick);
      const per = Math.max(1, Math.round(rate / 20)); const until = performance.now() + 4000; let busy = 0;
      while (performance.now() < until) { const a = performance.now(); window.__emit('chat://event', { type: 'messages', messages: Array.from({ length: per }, mk) }); busy += performance.now() - a; await new Promise((r) => setTimeout(r, 50)); }
      run = false; await new Promise((r) => setTimeout(r, 200)); d.shift();
      const s = d.sort((a, b) => a - b); const qq = (p) => s[Math.floor(p * (s.length - 1))];
      return { rate, frames: s.length, p50: +qq(0.5).toFixed(1), p95: +qq(0.95).toFixed(1), max: +s[s.length - 1].toFixed(1), over20: +(100 * s.filter((x) => x > 20).length / s.length).toFixed(1), emitMs: +busy.toFixed(0), lines: window.__panel.list.children.length };
    }, rate);
    out.chat.push(r);
  }
  const { profile } = await cdp.send('Profiler.stop');
  const dt = profile.timeDeltas; const self = new Map(); const byId = new Map(profile.nodes.map(n=>[n.id,n]));
  profile.samples.forEach((id,i)=>{ const n=byId.get(id); const f=n.callFrame; const k=`${f.functionName||'(anon)'} ${f.url.split('/').pop()}:${f.lineNumber+1}`; self.set(k,(self.get(k)||0)+(dt[i]||0)); });
  out.top = [...self].sort((a,b)=>b[1]-a[1]).slice(0,25).map(([k,v])=>`${(v/1000).toFixed(0)}ms ${k}`);
  const tr=[]; 
  await ctx.close();
}
console.log(JSON.stringify(out, null, 1));
await browser.close();
process.exit(0);
