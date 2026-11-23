// 채팅 메모리·붙이기 비용 회귀 검사(모의 IPC). 실제 채팅방에는 아무것도 보내지 않는다.
// M01 오래 붐비는 채팅(1만 2천 개) — 줄 수는 상한(최대 125%)을 넘지 않고, 화면 밖 상태(대기 줄·멘션·자동완성 닉네임·
//     본 이모티콘)도 상한 안이며, 문서 노드·이벤트 리스너가 늘지 않는다(Chromium 은 GC 뒤 JS 힙 증가도 본다).
// M02 붙이기 간격 — 메시지가 웹소켓 프레임마다 하나씩 와도(초당 40개) 초당 붙이기는 12번 이하로 모은다
//     (예전엔 메시지마다 붙여 초당 38번, 그때마다 바닥 맞추기가 레이아웃을 돌렸다 — WebKit 붙이기 비용 13%→7%).
//     한산할 때 온 메시지는 기다리지 않고 다음 프레임에 바로 붙는다. 붐빌 때(50ms 마다 10개)는 간격을 채우지 않고
//     한 프레임 분량씩 바로 붙인다 — 100ms 를 다 모으면 20줄 넘게 한꺼번에 붙어 느린 PC 에서 프레임이 끊겼다(perf P04).
// M03 패널을 버리면(멀티채팅 채널 닫기) 문서·창에 건 리스너와 이벤트 구독이 모두 풀린다 — 여닫기를 되풀이해도 늘지 않는다.
// M04 채팅 통계 참여자 표(stats.users)는 상한을 지킨다 — 한 번만 말한 사람부터 덜고, 많이 말한 사람·참여자 합계는 그대로.
// M05 컨텍스트 메뉴를 닫지 않고 거듭 열거나 열자마자 닫아도 문서·창 리스너가 남지 않는다.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/chat-memory.mjs
// CVIEW_UI_BROWSER=webkit 이면 WebKit 으로 검사한다(힙·리스너 수는 Chromium CDP 로만 잰다 — WebKit 은 DOM 기반 검사만).
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const webkit = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (webkit ? pw.webkit : pw.chromium).launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

async function open() {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => {
    // 모의 listen 이 구독을 기억하게 — 이벤트를 흘리고, 패널을 버린 뒤 구독이 풀렸는지 센다.
    const m = new Map();
    window.__TAURI__.event = { listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); m.get(n).add(f); return () => m.get(n).delete(f); } };
    window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
    window.__subs = (n) => (m.get(n) || new Set()).size;
    // 문서·창에 건 리스너 수(addEventListener/removeEventListener 짝) — WebKit 에는 CDP 가 없어 직접 센다.
    const live = new Map();
    for (const target of [Document.prototype, Window.prototype]) {
      const add = target.addEventListener;
      const remove = target.removeEventListener;
      target.addEventListener = function (type, fn, opts) {
        if (fn && (this === document || this === window)) {
          const k = `${this === document ? 'd' : 'w'}:${type}:${typeof opts === 'object' ? Boolean(opts?.capture) : Boolean(opts)}`;
          if (!live.has(k)) live.set(k, new Set());
          live.get(k).add(fn);
        }
        return add.call(this, type, fn, opts);
      };
      target.removeEventListener = function (type, fn, opts) {
        if (fn && (this === document || this === window)) {
          const k = `${this === document ? 'd' : 'w'}:${type}:${typeof opts === 'object' ? Boolean(opts?.capture) : Boolean(opts)}`;
          live.get(k)?.delete(fn);
        }
        return remove.call(this, type, fn, opts);
      };
    }
    window.__docListeners = () => [...live.values()].reduce((a, s) => a + s.size, 0);
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a = {}) => {
      if (c === 'chat_connect') return { canSend: true };
      if (c === 'chat_avatars') return (a.userIds || []).map(() => null);
      if (c === 'chat_emoticons' || c === 'chat_donation_ranking') return [];
      try { return await inv(c, a); } catch { return null; }
    };
  });
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async () => {
    const { ChatPanel } = await import('/js/chat.js');
    const { setMyNickname } = await import('/js/chat-prefs.js');
    setMyNickname('검사자');
    let n = 0;
    // 멘션·이모티콘·후원이 섞인 합성 메시지 — 사용자 3천 명, 닉네임은 메시지마다 새로(대형 방송처럼 고유 채팅자가 계속 는다 —
    // 예전엔 900개를 돌려 써서 통계 참여자 표(stats.users)가 끝없이 커지는 것을 잡지 못했다)
    window.__mk = () => {
      const k = ++n;
      const emo = k % 7 === 0 ? { [`e${k}`]: `https://ssl.pstatic.net/static/nng/glive/icon/e${k}.png` } : {};
      return { id: `syn-${k}`, kind: k % 97 === 0 ? 'donation' : 'chat', userId: `u${k % 3000}`, nickname: `시청자${k}`, content: `${k % 50 === 0 ? '@검사자 ' : ''}합성 메시지 ${k} ㅋㅋㅋ${k % 7 === 0 ? ` {:e${k}:}` : ''}`, emojis: emo, payAmount: k % 97 === 0 ? 1000 : undefined, role: 'common_user', badges: [], time: Date.now() };
    };
    window.__makePanel = async () => {
      const box = document.createElement('div');
      box.className = '__chat-box';
      box.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:860px;z-index:9999;display:flex;flex-direction:column';
      box.innerHTML = document.querySelector('#viewer .chat').outerHTML;
      document.body.appendChild(box);
      const root = box.firstElementChild;
      root.hidden = false;
      root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
      const panel = new ChatPanel(root, { isLoggedIn: () => true, session: 'mem' });
      await panel.connect('cc1', 'ch1').catch(() => {});
      panel.active = true;
      return panel;
    };
  });
  const cdp = webkit ? null : await ctx.newCDPSession(page);
  if (cdp) await cdp.send('Performance.enable');
  const metrics = async () => {
    if (!cdp) return {};
    await cdp.send('HeapProfiler.collectGarbage');
    await cdp.send('HeapProfiler.collectGarbage');
    return Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
  };
  return { ctx, page, errors, metrics };
}

// 세션 이벤트로 흘린다 — 실제 앱의 멀티라이브·멀티채팅 패널과 같은 경로(chat://session-event).
const pump = (page, total, per, gapMs) => page.evaluate(async ([total, per, gapMs]) => {
  for (let sent = 0; sent < total; sent += per) {
    window.__emit('chat://session-event', { session: 'mem', event: { type: 'messages', messages: Array.from({ length: per }, window.__mk) } });
    await new Promise((r) => setTimeout(r, gapMs));
  }
  await new Promise((r) => setTimeout(r, 400));
}, [total, per, gapMs]);

await check('M01 long busy chat: lines, side state, nodes and listeners stay bounded', async () => {
  const { ctx, page, errors, metrics } = await open();
  await page.evaluate(async () => { window.__panel = await window.__makePanel(); });
  await pump(page, 2000, 20, 25);
  const warm = await metrics();
  const warmDom = await page.evaluate(() => ({ nodes: document.getElementsByTagName('*').length, listeners: window.__docListeners() }));
  await pump(page, 10000, 20, 25);
  const end = await metrics();
  const s = await page.evaluate(() => {
    const p = window.__panel;
    return {
      lines: p.list.children.length, max: p.maxLines, pending: (p.pendingLines || []).length, mentions: p.mentions.length,
      nicks: p.recentNicks.size, emoji: p.seenEmoji.size, nodes: document.getElementsByTagName('*').length, listeners: window.__docListeners(),
    };
  });
  assert.ok(s.lines <= s.max * 1.25 + 2, `줄 ${s.lines}(상한 ${s.max})`);
  assert.ok(s.pending <= s.max, `대기 줄 ${s.pending}`);
  assert.ok(s.mentions <= 200 && s.nicks <= 400 && s.emoji <= 2000, `멘션 ${s.mentions}·닉네임 ${s.nicks}·이모티콘 ${s.emoji}`);
  // 상한 근처에서 trimFront 가 125%까지 채웠다 지우므로 노드 수는 그 폭만큼 흔들린다.
  assert.ok(s.nodes <= warmDom.nodes * 1.3, `문서 요소 ${warmDom.nodes} → ${s.nodes}`);
  assert.equal(s.listeners, warmDom.listeners, `문서·창 리스너 ${warmDom.listeners} → ${s.listeners}`);
  // 통계 참여자 — 표는 상한으로 덜어 내도 '참여자' 수는 덜어 낸 사람까지 센다(고유 닉네임 1만 2천 명).
  const who = await page.evaluate(() => {
    const p = window.__panel;
    // 복제해 온 채팅 DOM 에는 원래 패널이 단 단추도 들어 있다 — 이 패널이 단 것(마지막)을 누른다.
    [...p.root.querySelectorAll('.chat-hbtn[title="채팅 통계"]')].at(-1).click();
    const cell = [...p.tray.querySelectorAll('.stat-grid > div')].find((d) => d.textContent.includes('참여자'));
    const n = Number(cell?.querySelector('b')?.textContent);
    p.tray.hidden = true;
    return n;
  });
  assert.equal(who, 12000, `통계 참여자 ${who}`);
  if (warm.JSHeapUsedSize) {
    const grow = (end.JSHeapUsedSize - warm.JSHeapUsedSize) / 1048576;
    assert.ok(grow < 2, `메시지 1만 개 뒤 JS 힙 +${grow.toFixed(2)}MB`);
    assert.ok(end.JSEventListeners <= warm.JSEventListeners * 1.3 + 20, `JS 리스너 ${warm.JSEventListeners} → ${end.JSEventListeners}`);
  }
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('M04 chat stats participant table is capped (one-time talkers dropped first, top talkers kept, total still counted)', async () => {
  const { ctx, page, errors } = await open();
  const r = await page.evaluate(async () => {
    const { trimStatsUsers } = await import('/js/chat-extras.js');
    const stats = { users: new Map(), usersDropped: 0 };
    for (let i = 0; i < 50; i++) stats.users.set(`단골${i}`, 100 + i);
    for (let i = 0; i < 4100; i++) stats.users.set(`손님${i}`, 1);
    trimStatsUsers(stats);
    const afterOnce = { size: stats.users.size, dropped: stats.usersDropped, regulars: [...stats.users.keys()].filter((n) => n.startsWith('단골')).length, newest: stats.users.has('손님4099') };
    // 모두 여러 번 말한 사람뿐이어도 상한을 지킨다(오래된 순).
    const all = { users: new Map(), usersDropped: 0 };
    for (let i = 0; i < 5000; i++) all.users.set(`n${i}`, 2);
    trimStatsUsers(all);
    return { afterOnce, allSize: all.users.size, allDropped: all.usersDropped, allNewest: all.users.has('n4999') };
  });
  assert.equal(r.afterOnce.size, 3000, JSON.stringify(r));
  assert.equal(r.afterOnce.size + r.afterOnce.dropped, 4150);
  assert.equal(r.afterOnce.regulars, 50, '여러 번 말한 사람은 남는다');
  assert.ok(r.afterOnce.newest, '최근 사람은 남는다');
  assert.equal(r.allSize, 3000);
  assert.equal(r.allDropped, 2000);
  assert.ok(r.allNewest);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('M02 per-frame messages are coalesced into at most ~10 appends per second; a lone message appends next frame', async () => {
  const { ctx, page, errors } = await open();
  const r = await page.evaluate(async () => {
    const panel = await window.__makePanel();
    // 상한까지 채워 둔다(실제 붐비는 채팅처럼)
    for (let i = 0; i < 35; i++) { panel.append(Array.from({ length: 10 }, window.__mk)); panel.flushLines(); }
    await new Promise((r) => setTimeout(r, 300));
    let flushes = 0;
    const orig = panel.flushLines.bind(panel);
    panel.flushLines = () => { flushes++; orig(); };
    const t0 = performance.now();
    let sent = 0;
    while (performance.now() - t0 < 3000) {
      const due = Math.floor((performance.now() - t0) / 25) + 1; // 초당 40개, 하나씩
      while (sent < due) { window.__emit('chat://session-event', { session: 'mem', event: { type: 'messages', messages: [window.__mk()] } }); sent++; }
      await new Promise((r) => setTimeout(r, 10));
    }
    await new Promise((r) => setTimeout(r, 400));
    const rate = flushes / ((performance.now() - t0) / 1000);
    const lastId = panel.list.lastElementChild?.dataset.id;
    // 붐빌 때 — 50ms 마다 10개. 한 번에 붙이는 줄은 한 프레임 분량(약 10줄)에 머문다.
    const batches = [];
    panel.flushLines = () => { batches.push((panel.pendingLines || []).length); orig(); };
    for (let i = 0; i < 30; i++) {
      window.__emit('chat://session-event', { session: 'mem', event: { type: 'messages', messages: Array.from({ length: 10 }, window.__mk) } });
      await new Promise((r) => setTimeout(r, 50));
    }
    await new Promise((r) => setTimeout(r, 400));
    panel.flushLines = orig;
    const burst = batches.filter((n) => n > 0);
    // 한산할 때 — 조용히 있다가 온 한 줄은 바로 다음 프레임에 붙는다.
    await new Promise((r) => setTimeout(r, 300));
    const m = window.__mk();
    const a = performance.now();
    window.__emit('chat://session-event', { session: 'mem', event: { type: 'messages', messages: [m] } });
    while (panel.list.lastElementChild?.dataset.id !== m.id && performance.now() - a < 1000) await new Promise((r) => requestAnimationFrame(r));
    return { sent, rate, lastId, expect: `syn-${350 + sent}`, burst, lone: performance.now() - a, shown: panel.list.lastElementChild?.dataset.id === m.id };
  });
  assert.equal(r.lastId, r.expect, '모은 메시지가 모두 순서대로 붙는다');
  assert.ok(r.rate <= 12, `초당 붙이기 ${r.rate.toFixed(1)}번(메시지 ${r.sent}개)`);
  const avg = r.burst.reduce((a, b) => a + b, 0) / r.burst.length;
  assert.ok(r.burst.reduce((a, b) => a + b, 0) === 300 && avg <= 12 && Math.max(...r.burst) <= 20, `붐빌 때 한 번에 붙인 줄 ${r.burst}`);
  assert.ok(r.shown && r.lone < 80, `한산할 때 한 줄이 ${r.lone.toFixed(0)}ms 뒤에 붙음`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('M03 destroying a panel releases document listeners and event subscriptions', async () => {
  const { ctx, page, errors, metrics } = await open();
  // 살아 있는 ChatPanel 인스턴스 수(Chromium CDP) — 버린 패널이 GC 되는지 본다.
  const livePanels = async () => {
    if (webkit) return null;
    await metrics(); // GC
    const cdp = await ctx.newCDPSession(page);
    const proto = await cdp.send('Runtime.evaluate', { expression: "import('/js/chat.js').then((m) => m.ChatPanel.prototype)", awaitPromise: true });
    const q = await cdp.send('Runtime.queryObjects', { prototypeObjectId: proto.result.objectId });
    const n = (await cdp.send('Runtime.callFunctionOn', { objectId: q.objects.objectId, functionDeclaration: 'function () { return this.length; }', returnByValue: true })).result.value;
    await cdp.detach();
    return n;
  };
  const before = await page.evaluate(() => ({ listeners: window.__docListeners(), subs: window.__subs('chat://session-event') }));
  const panels0 = await livePanels();
  const m0 = await metrics();
  for (let i = 0; i < 6; i++) {
    await page.evaluate(async () => {
      const panel = await window.__makePanel();
      for (let k = 0; k < 5; k++) panel.append(Array.from({ length: 20 }, window.__mk));
      panel.flushLines();
      await panel.destroy();
      document.querySelector('.__chat-box').remove();
    });
  }
  await page.waitForTimeout(300);
  const after = await page.evaluate(() => ({ listeners: window.__docListeners(), subs: window.__subs('chat://session-event') }));
  const m1 = await metrics();
  const panels1 = await livePanels();
  if (panels0 != null) assert.equal(panels1, panels0, `버린 패널이 GC 되지 않음: 살아 있는 ChatPanel ${panels0} → ${panels1}`);
  assert.equal(after.subs, before.subs, `채팅 이벤트 구독 ${before.subs} → ${after.subs}`);
  // 채팅 모듈이 처음 불릴 때 한 번 거는 전역 리스너(줄 메뉴·따라가기)는 몇 개 생길 수 있다 — 패널마다 늘면 안 된다.
  assert.ok(after.listeners - before.listeners <= 6, `문서·창 리스너 ${before.listeners} → ${after.listeners}(패널 6번 여닫기)`);
  if (m0.JSEventListeners) assert.ok(m1.JSEventListeners - m0.JSEventListeners <= 30, `JS 리스너 ${m0.JSEventListeners} → ${m1.JSEventListeners}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('M05 context menus opened back to back leave no document/window listeners behind', async () => {
  const { ctx, page, errors } = await open();
  const r = await page.evaluate(async () => {
    const { showMenu } = await import('/js/chat.js');
    const before = window.__docListeners();
    // 우클릭을 거듭하듯 앞 메뉴를 닫지 않고 새 메뉴를 연다 — 예전엔 앞 메뉴의 mousedown·blur 리스너가 남았다.
    for (let i = 0; i < 20; i++) {
      showMenu(100, 100, [['a', '하나'], ['b', '둘']], () => {});
      if (i % 2) await new Promise((r) => setTimeout(r, 0));
    }
    await new Promise((r) => setTimeout(r, 20));
    const open = document.querySelectorAll('.ctx-menu').length;
    document.querySelector('.ctx-menu .ctx-item')?.click();
    await new Promise((r) => setTimeout(r, 20));
    // 열자마자 같은 틱에 닫혀도(늦게 붙는 mousedown) 남지 않는다.
    showMenu(100, 100, [['a', '하나']], () => {});
    document.querySelector('.ctx-menu .ctx-item')?.click();
    await new Promise((r) => setTimeout(r, 20));
    return { before, after: window.__docListeners(), open, left: document.querySelectorAll('.ctx-menu').length };
  });
  assert.equal(r.open, 1, `열린 메뉴 ${r.open}`);
  assert.equal(r.left, 0);
  assert.equal(r.after, r.before, `문서·창 리스너 ${r.before} → ${r.after}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
console.log(`${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
