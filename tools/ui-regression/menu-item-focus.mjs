// 메뉴 줄 키보드 초점 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·채팅 전송·차단 없음).
// 줄 메뉴·우클릭 메뉴(.ctx-item, chat.js showMenu 하나를 함께 쓴다)는 키보드 초점에 바탕을 --primary 로 채웠는데,
// 라이트의 #00d38a 는 흰 메뉴 바탕과 1.97:1 이었다. 다크의 위험 항목 채움(--danger-fill #c42b1c)도 메뉴 바탕과 2.6:1.
// 계정 메뉴(.ap-item)는 초점에 바탕이 --surface-elevated 로만 바뀌어 1.1:1 — 거의 보이지 않았다.
// 두 메뉴 모두 초점과 호버가 같은 모양이라, 마우스가 다른 줄에 있으면 어느 줄이 키보드 초점인지 알 수 없었다.
// 대상: 채팅 줄 메뉴(Shift+F10 로 연다) 첫 항목·'사용자 차단'(위험) · 계정 메뉴(계정 단추에서 Enter) 첫 항목·'로그아웃'(위험).
// MF01 초점 대비 — 초점만 한 줄과 쉬는 줄(초점·호버 없음)을 픽셀로 비교해, 줄 둘레 네 변 모두 3:1 이상 바뀐 띠가
//      1.5px(레티나 3픽셀) 이상 이어진다(WCAG 2.4.13 의 '바뀐 정도' 방식). 다크·라이트, 일반·위험.
// MF02 글자 대비 — 초점·호버·초점+호버에서 글자색과 그 뒤 바탕 4.5:1 이상, 계정 메뉴 아이콘은 3:1 이상.
// MF03 호버와 구별 — 호버만 한 줄과 초점만 한 줄이 둘레 네 변에서 2:1 이상 다른 띠(1.5px 이상)를 가진다.
//      마우스가 다른 줄에 있어도 키보드 초점 줄을 가려낼 수 있어야 한다(같은 모양이면 실패).
// MF04 위험 항목 — 초점·호버·초점+호버에서 바탕이 위험 채움(--danger-fill)이고, 글자가 흰색 쪽(--on-danger)이다.
// MF05 모션 줄이기(시스템 설정·앱 설정 두 경로) — 초점을 옮기고 두 프레임 안에 끝 모양이고 도는 전환·애니메이션이 없다
//      (모션 줄이기는 전환을 0.01ms 로 줄일 뿐이라 바로 그 프레임엔 시작 값이 읽힌다).
// 고대비(forced-colors) 테두리는 button-press-focus.mjs(BF06) 가 본다 — 엔진이 box-shadow 를 지우니 이 검사의 띠는 그리지 않는다.
// 저장소 루트에서: node tools/ui-regression/menu-item-focus.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG 를 남긴다.
//   CVIEW_MF_REPORT=1 이면 잰 대비를 모두 찍는다.)
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const REPORT = process.env.CVIEW_MF_REPORT ? [] : null;
const W = 1280, H = 820, DPR = 2;
const fails = { MF01: [], MF02: [], MF03: [], MF04: [], MF05: [] };

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 계산용 빈 페이지

// 메뉴 — items: 화살표가 오가는 항목 · normal/danger: 대상 항목(첫 줄·마지막 줄) ·
// far: 일반 대상을 잴 때 초점을 옮겨 둘 줄 — 대상과 붙어 있으면 그 줄의 고리가 대상 둘레 띠에 섞여 들어오므로 떨어진 줄
// (위험 대상을 잴 때는 일반 대상(첫 줄)으로 옮긴다)
const MENUS = [
  { key: 'ctx', name: '줄 메뉴', items: '.ctx-menu .ctx-item', normal: '.ctx-menu .ctx-item[data-ctx="copy-text"]', far: '.ctx-menu .ctx-item[data-ctx="highlight"]', danger: '.ctx-menu .ctx-item.danger', menu: '.ctx-menu', text: null, icon: null },
  { key: 'ap', name: '계정 메뉴', items: '.account-pop :is(.ap-item, .seg)', normal: '.account-pop .ap-item[data-ap-route="following"]', far: '.account-pop .ap-item[data-ap-menu="about"]', danger: '.account-pop .ap-item.danger', menu: '.account-pop', text: '.ap-label', icon: '.ap-icon' },
];

async function open(theme, M, { reduced = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: theme, deviceScaleFactor: DPR, reducedMotion: reduced === 'system' ? 'reduce' : 'no-preference' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(() => {
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_connect') return { canSend: true };
      if (c === 'chat_disconnect' || c === 'settings_update') return null;
      if (c === 'chat_avatars') return (a.userIds || []).map(() => null);
      if (c === 'chat_donation_ranking') return [];
      return inv(c, a);
    };
    try { localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
  });
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await go(page, 'home');
  await page.waitForTimeout(600);
  if (reduced === 'app') await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
  if (M.key === 'ctx') {
    // 시청 화면의 패널은 이미 부속을 달았으므로 index.html 원본 마크업에서 채팅 패널을 하나 새로 만든다.
    await page.evaluate(async () => {
      const { ChatPanel } = await import('/js/chat.js');
      const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
      await loadChatPrefs();
      setMyNickname('도도');
      const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
      const box = document.createElement('div');
      box.id = 'mfbox';
      box.style.cssText = 'position:fixed;left:420px;top:40px;width:360px;height:600px;z-index:50;display:flex;flex-direction:column';
      box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
      document.body.appendChild(box);
      const root = box.querySelector('.chat');
      root.hidden = false;
      root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
      const panel = new ChatPanel(root, { isLoggedIn: () => true, onHighlight: () => {} });
      await panel.connect('cc1', 'ch1');
      panel.onEvent({ type: 'connected' });
      let k = 0;
      const t = Date.now();
      const msg = (o) => ({ id: `m${++k}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, timestamp: t + k * 61000, ...o });
      panel.append([
        { userId: 'u1', nickname: '사과', content: '안녕하세요 반가워요' },
        { userId: 'u2', nickname: '바나나', content: '오늘 방송 최고' },
        { userId: 'u3', nickname: '체리', content: '사과 좋아하세요?' },
      ].map(msg));
      panel.flushLines();
      window.__panel = panel;
    });
    await page.waitForTimeout(300);
    await page.locator('#mfbox .chat-list').focus();
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Shift+F10');
  } else {
    await page.locator('.account-trigger').focus();
    await page.keyboard.press('Enter');
  }
  await page.waitForSelector(`${M.menu}`, { state: 'visible' });
  await page.waitForTimeout(350);   // 메뉴 등장 애니메이션(mo-pop)이 끝나게
  return { ctx, page, errors };
}

// 화살표로 sel 항목에 키보드 초점을 둔다(두 메뉴 모두 ↓ 가 끝에서 처음으로 돈다)
async function keyTo(page, M, sel) {
  for (let i = 0; i < 16; i += 1) {
    if (await page.evaluate((s) => document.activeElement?.matches(s), sel)) return;
    await page.keyboard.press('ArrowDown');
  }
  throw new Error(`화살표로 ${sel} 에 닿지 못했다`);
}
const away = (page) => page.mouse.move(W - 4, H - 4);
async function hoverOn(page, sel) {
  const b = await page.locator(sel).boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
}

// 상태 만들기 — rest: 다른 줄에 초점·마우스 치움 · focus: 대상에 초점·마우스 치움 · hover: 다른 줄에 초점·대상에 마우스 · both: 둘 다 대상
async function setState(page, M, target, other, state) {
  await keyTo(page, M, state === 'focus' || state === 'both' ? target : other);
  if (state === 'hover' || state === 'both') await hoverOn(page, target);
  else await away(page);
  await page.waitForTimeout(420);   // 바탕·글자 전환(--dur-2 .18s)이 끝나게
}

// 대상 둘레를 찍는다 — 정수 CSS 픽셀로 맞춘 6px 여백 상자
async function shot(page, sel, name) {
  const b = await page.locator(sel).boundingBox();
  const clip = { x: Math.floor(b.x) - 6, y: Math.floor(b.y) - 6, width: Math.ceil(b.x + b.width) - Math.floor(b.x) + 12, height: Math.ceil(b.y + b.height) - Math.floor(b.y) + 12 };
  const buf = await page.screenshot({ clip });
  if (SHOTS && name) fs.writeFileSync(path.join(SHOTS, `${name}.png`), buf);
  const r = await page.locator(sel).evaluate((el) => parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0);
  return { buf, clip, box: b, radius: r };
}

// 두 화면의 둘레 띠를 변마다 깊이별로 비교한다 — 깊이 k 는 테두리 상자 가장자리에서 장치 픽셀(음수는 바깥).
// 변마다 모서리 둥근 곳(반경 + 2px)을 빼고 그 줄의 가운뎃값 색을 쓴다. 돌려주는 값: { 위: [{k, c}], … } (c = 대비)
function sideProfiles(a, b) {
  return calc.evaluate(async ({ a, b, clip, box, radius, dpr }) => {
    const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
    const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    const load = async (b64) => { const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode(); const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const x = c.getContext('2d'); x.drawImage(img, 0, 0); return x.getImageData(0, 0, img.width, img.height); };
    const A = await load(a); const B = await load(b);
    const px = (D, X, Y) => { const i = (Y * D.width + X) * 4; return [D.data[i], D.data[i + 1], D.data[i + 2]]; };
    const median = (list) => { const s = [...list].sort((p, q) => lum(p) - lum(q)); return s[Math.floor(s.length / 2)]; };
    const L = (box.x - clip.x) * dpr, T = (box.y - clip.y) * dpr, R = (box.x + box.width - clip.x) * dpr, Bm = (box.y + box.height - clip.y) * dpr;
    const skip = (radius + 2) * dpr;
    const out = {};
    const sides = {
      위: (k) => ({ line: Math.floor(T) + k, from: Math.ceil(L + skip), to: Math.floor(R - skip), horiz: true }),
      아래: (k) => ({ line: Math.ceil(Bm) - 1 - k, from: Math.ceil(L + skip), to: Math.floor(R - skip), horiz: true }),
      왼쪽: (k) => ({ line: Math.floor(L) + k, from: Math.ceil(T + skip), to: Math.floor(Bm - skip), horiz: false }),
      오른쪽: (k) => ({ line: Math.ceil(R) - 1 - k, from: Math.ceil(T + skip), to: Math.floor(Bm - skip), horiz: false }),
    };
    for (const [side, at] of Object.entries(sides)) {
      out[side] = [];
      for (let k = -8; k <= 14; k += 1) {
        const { line, from, to, horiz } = at(k);
        const pa = [], pb = [];
        for (let t = from; t <= to; t += 1) {
          const [X, Y] = horiz ? [t, line] : [line, t];
          if (X < 0 || Y < 0 || X >= A.width || Y >= A.height) continue;
          pa.push(px(A, X, Y)); pb.push(px(B, X, Y));
        }
        if (!pa.length) continue;
        const ma = median(pa), mb = median(pb);
        out[side].push({ k, c: ratio(ma, mb), a: ma, b: mb });
      }
    }
    return out;
  }, { a: a.buf.toString('base64'), b: b.buf.toString('base64'), clip: a.clip, box: a.box, radius: a.radius, dpr: DPR });
}
// 변마다 c ≥ min 인 깊이가 연달아 몇 장치 픽셀 이어지는지, 그 띠의 가장 낮은 대비
function bands(prof, min) {
  const out = {};
  for (const [side, rows] of Object.entries(prof)) {
    let run = 0, best = 0, low = Infinity, bestLow = 0, top = 0;
    for (const r of rows) {
      top = Math.max(top, r.c);
      if (r.c >= min) { run += 1; low = Math.min(low, r.c); if (run > best) { best = run; bestLow = low; } } else { run = 0; low = Infinity; }
    }
    out[side] = { run: best, low: bestLow, top };
  }
  return out;
}
const fmt = (o) => Object.entries(o).map(([s, v]) => `${s} ${v.top.toFixed(2)}(${v.run}px)`).join(' · ');

// 글자·아이콘·바탕 — 항목 바탕을 메뉴 바탕 위에 겹쳐 실제로 보이는 색을 낸다
const colors = (page, M, sel) => page.evaluate(({ sel, menu, text, icon }) => {
  const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
  const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
  const probe = document.createElement('i');
  document.body.append(probe);
  const rgba = (c) => { probe.style.color = ''; probe.style.color = c; const m = getComputedStyle(probe).color.match(/[\d.]+/g).map(Number); return [m[0], m[1], m[2], m.length > 3 ? m[3] : 1]; };
  const over = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));
  const el = document.querySelector(sel);
  const m = el.closest(menu);
  const back = over(rgba(getComputedStyle(el).backgroundColor), rgba(getComputedStyle(m).backgroundColor));
  const t = rgba(getComputedStyle(text ? el.querySelector(text) : el).color);
  const ic = icon ? rgba(getComputedStyle(el.querySelector(icon)).color) : null;
  const root = getComputedStyle(document.documentElement);
  const tok = (n) => rgba(root.getPropertyValue(n).trim());
  const out = { text: ratio(over(t, back), back), icon: ic ? ratio(over(ic, back), back) : null, back, danger: tok('--danger-fill').slice(0, 3), onDanger: tok('--on-danger').slice(0, 3), fv: el.matches(':focus-visible') };
  probe.remove();
  return out;
}, { sel, menu: M.menu, text: M.text, icon: M.icon });
const same = (p, q) => p.every((v, i) => Math.abs(v - q[i]) <= 2);

// ── MF01~MF04 ────────────────────────────────────────────────────────────────
for (const theme of ['dark', 'light']) {
  for (const M of MENUS) {
    for (const tone of ['normal', 'danger']) {
      const tag = `${M.name} ${tone === 'danger' ? '위험' : '일반'} ${theme}`;
      const target = M[tone];
      const other = tone === 'danger' ? M.normal : M.far;
      let ctx, page, errors;
      try {
        ({ ctx, page, errors } = await open(theme, M));
        await page.locator(target).first().evaluate((el) => el.setAttribute('data-mf', ''));
        const T = '[data-mf]';
        const otherSel = await page.evaluate(([o]) => { const el = document.querySelector(o); el.setAttribute('data-mf-other', ''); return '[data-mf-other]'; }, [other]);
        const S = {};
        const C = {};
        for (const state of ['rest', 'focus', 'hover', 'both']) {
          await setState(page, M, T, otherSel, state);
          S[state] = await shot(page, T, `${M.key}-${tone}-${theme}-${state}`);
          C[state] = await colors(page, M, T);
        }
        if (!C.focus.fv) throw new Error('키보드 초점이 서지 않았다(:focus-visible 아님)');
        // MF01 — 초점 vs 쉼
        const f = bands(await sideProfiles(S.focus, S.rest), 3);
        REPORT?.push(`MF01 ${tag}: ${fmt(f)}`);
        const weak = Object.entries(f).filter(([, v]) => v.run < 3);
        if (weak.length) fails.MF01.push(`${tag}: 3:1 띠가 모자란 변 ${weak.map(([s, v]) => `${s} 최고 ${v.top.toFixed(2)}:1(${v.run}px)`).join(', ')}`);
        // MF02 — 글자 대비
        for (const state of ['focus', 'hover', 'both']) {
          REPORT?.push(`MF02 ${tag} ${state}: 글자 ${C[state].text.toFixed(2)}${C[state].icon ? ` 아이콘 ${C[state].icon.toFixed(2)}` : ''}`);
          if (C[state].text < 4.5) fails.MF02.push(`${tag} ${state}: 글자 ${C[state].text.toFixed(2)}:1 (4.5:1 미만)`);
          if (C[state].icon !== null && C[state].icon < 3) fails.MF02.push(`${tag} ${state}: 아이콘 ${C[state].icon.toFixed(2)}:1 (3:1 미만)`);
        }
        // MF03 — 호버와 구별
        const h = bands(await sideProfiles(S.focus, S.hover), 2);
        REPORT?.push(`MF03 ${tag}: ${fmt(h)}`);
        const flat = Object.entries(h).filter(([, v]) => v.run < 3);
        if (flat.length) fails.MF03.push(`${tag}: 호버와 2:1 넘게 다른 띠가 없는 변 ${flat.map(([s, v]) => `${s} 최고 ${v.top.toFixed(2)}:1`).join(', ')}`);
        // MF04 — 위험 항목 채움
        if (tone === 'danger') {
          for (const state of ['focus', 'hover', 'both']) {
            if (!same(C[state].back, C[state].danger)) fails.MF04.push(`${tag} ${state}: 바탕 rgb(${C[state].back.map(Math.round)}) — 위험 채움(--danger-fill)이 아님`);
          }
        }
        if (errors.length) fails.MF01.push(`${tag}: 페이지 오류 ${errors[0]}`);
      } catch (e) {
        fails.MF01.push(`${tag}: ${e.message.split('\n')[0]}`);
      }
      await ctx?.close();
    }
  }
}

// ── MF05 모션 줄이기 ──────────────────────────────────────────────────────────
// 화살표를 누르고 두 프레임 뒤와 한참 뒤의 계산 스타일이 같고, 도는 전환·애니메이션이 없어야 한다.
// (보통 모션이면 바탕·고리 전환(.18s)이 아직 도는 중이라 다르다.)
const snap = (page, frames = 0) => page.evaluate(async (frames) => {
  for (let i = 0; i < frames; i += 1) await new Promise(requestAnimationFrame);
  const el = document.activeElement;
  const cs = getComputedStyle(el);
  const runs = el.getAnimations().filter((a) => a.playState === 'running' && (a.effect?.getComputedTiming().duration || 0) > 1).map((a) => a.transitionProperty || a.animationName);
  return { fv: el.matches(':focus-visible'), look: [cs.backgroundColor, cs.color, cs.boxShadow, cs.outlineStyle, cs.outlineColor, cs.outlineWidth].join(' | '), runs };
}, frames);
for (const reduced of ['system', 'app']) {
  for (const M of MENUS) {
    const tag = `${M.name} ${reduced === 'system' ? '시스템' : '앱 설정'}`;
    let ctx;
    try {
      let page;
      ({ ctx, page } = await open('light', M, { reduced }));
      await away(page);
      for (const sel of [M.normal, M.danger]) {
        // 대상 바로 앞 항목까지 간 뒤, 한 번 눌러 대상으로 옮기고 두 프레임 뒤를 잰다
        const prev = await page.evaluate(([items, s]) => { const list = [...document.querySelectorAll(items)]; const i = list.indexOf(document.querySelector(s)); list.forEach((x) => x.removeAttribute('data-mf-prev')); list[(i - 1 + list.length) % list.length].setAttribute('data-mf-prev', ''); return '[data-mf-prev]'; }, [M.items, sel]);
        await keyTo(page, M, prev);
        await page.waitForTimeout(300);
        await page.keyboard.press('ArrowDown');
        const now = await snap(page, 2);
        await page.waitForTimeout(400);
        const later = await snap(page);
        if (!now.fv) fails.MF05.push(`${tag} ${sel}: 키보드 초점이 서지 않았다`);
        if (now.runs.length) fails.MF05.push(`${tag} ${sel}: 도는 전환·애니메이션 ${now.runs.join(', ')}`);
        if (now.look !== later.look) fails.MF05.push(`${tag} ${sel}: 두 프레임 뒤 ${now.look} → 나중 ${later.look}`);
      }
    } catch (e) {
      fails.MF05.push(`${tag}: ${e.message.split('\n')[0]}`);
    }
    await ctx?.close();
  }
}

await browser.close();
server.close();
const titles = {
  MF01: '초점 띠가 쉼과 3:1 이상(네 변)', MF02: '글자 4.5:1·아이콘 3:1', MF03: '호버와 구별되는 초점(네 변)',
  MF04: '위험 항목 채움 유지', MF05: '모션 줄이기 — 초점이 바로 끝 모양(두 프레임 안)',
};
if (REPORT) console.log(REPORT.join('\n'));
let failed = 0;
for (const [id, list] of Object.entries(fails)) {
  console.log(`${list.length ? '✗' : '✓'} ${id} ${titles[id]}`);
  for (const m of list) console.log(`    ${m}`);
  if (list.length) failed += 1;
}
console.log(`\n${Object.keys(fails).length - failed}/${Object.keys(fails).length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
