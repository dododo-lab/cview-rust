// 채팅 아바타 UI 회귀 검사(모의 IPC, 헤드리스 Chromium) — chat-avatar.js.
// AV01 랜덤 아이콘은 사용자 ID 로 결정된다(같은 ID = 같은 아이콘, 페이지를 새로 열어도). 종류·색이 고르게 퍼진다.
// AV02 채팅 줄에 원형 아바타(크기 고정, 아이콘은 시트 한 장의 위치)가 붙고, 사진이 늦게 와도
//      줄·아바타 크기가 그대로다.
// AV03 사진을 못 불러오면 사진을 걷고 아이콘을 남기며, 그 사람은 다시 묻지 않는다.
// AV04 사진 찾기는 천천히(1.5초에 3명 이하), 가장 최근에 말한 사람부터, 같은 사람은 한 번만 묻는다.
// AV05 창이 가려진 동안은 묻지 않고, 다시 보이면 이어 묻는다.
// AV06 설정 — 'icon' 은 사진을 찾지 않고, 'off' 는 아바타를 그리지 않는다.
// AV07 오래된 줄을 지울 때 사진 <img> 의 src 를 비워 디코드 메모리를 놓아 준다. 익명(사용자 ID 없음)은 묻지 않는다.
// 저장소 루트에서: node tools/ui-regression/chat-avatar.mjs   (Playwright 가 필요하다)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

/** 채팅 패널 하나를 띄운 페이지. chat_avatars 호출은 window.__avCalls 에 남고, 사진은 photoFor(id) 규칙으로 준다. */
async function openChat(prefs = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1100, height: 900 } });
  const page = await ctx.newPage();
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(([origin, chatPrefs]) => {
    window.__avCalls = [];
    let hidden = false;
    Object.defineProperty(document, 'hidden', { get: () => hidden, configurable: true });
    window.__setHidden = (v) => { hidden = v; document.dispatchEvent(new Event('visibilitychange')); };
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_avatars') {
        window.__avCalls.push([...a.userIds]);
        // photo* → 사진, broken* → 없는 그림(404), 나머지 → 사진 없음
        return a.userIds.map((u) => (u.startsWith('photo') ? `${origin}__img/avatar/${u.length}.svg` : u.startsWith('broken') ? `${origin}__img/none/404.png` : null));
      }
      if (c === 'settings_get') {
        const s = await inv(c, a).catch(() => ({}));
        return { ...(s || {}), chat: { ...((s && s.chat) || {}), ...chatPrefs } };
      }
      try { return await inv(c, a); } catch { return null; }
    };
  }, [base, prefs]);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async (chatPrefs) => {
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    const box = document.createElement('div');
    box.style.cssText = 'position:fixed;right:0;top:0;width:380px;height:880px;z-index:9999;display:flex;flex-direction:column';
    box.innerHTML = document.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => true });
    await panel.connect('cc1', 'ch1').catch(() => {});
    panel.active = true;
    panel.chatChannelId = null;
    window.__panel = panel;
    window.__say = (users, extra = {}) => {
      const t = Date.now();
      panel.append(users.map((u, i) => ({ id: `${u}-${t}-${i}-${Math.random()}`, userId: u, nickname: `닉_${u}`, content: `안녕 ${i}`, kind: 'normal', role: 'viewer', badges: [], emojis: {}, timestamp: t - i * 120000, ...extra })));
      panel.flushLines();
    };
    void chatPrefs;
  }, prefs);
  return { ctx, page };
}

// ── AV01 결정성 ──
await check('AV01 identicon is deterministic per user and well spread', async () => {
  const run = async () => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.addInitScript(mockTauri, [base, 'dark']);
    await page.goto(base);
    const r = await page.evaluate(async () => {
      const m = await import('/js/chat-avatar.js');
      const ids = Array.from({ length: 3000 }, (_, i) => `a${(i * 2654435761 >>> 0).toString(16)}f${i}`);
      const picks = ids.map((id) => m.avatarFor(id));
      const species = {}; const palettes = {};
      for (const p of picks) { species[p.species] = (species[p.species] || 0) + 1; palettes[p.palette] = (palettes[p.palette] || 0) + 1; }
      const combos = new Set(picks.map((p) => `${p.species}/${p.palette}/${p.extra}`)).size;
      return {
        sample: ['0123456789abcdef0123456789abcdef', 'userA', 'userB', 'userA'].map((k) => [m.avatarSeed(k), JSON.stringify(m.avatarStyle(k))]),
        sheet: m.avatarSheetSvg(), sheetCells: (m.avatarSheetSvg().match(/<rect width="40"/g) || []).length,
        species, palettes, combos, nSpecies: m.AVATAR_SPECIES.length, nPalettes: m.AVATAR_PALETTES.length,
        // 끝 글자만 다른 ID 들이 같은 그림으로 몰리지 않는다
        neighbours: new Set(Array.from({ length: 10 }, (_, i) => JSON.stringify(m.avatarFor(`abcdef${i}`)))).size,
      };
    });
    await ctx.close();
    return r;
  };
  const a = await run();
  const b = await run();
  assert.deepEqual(a.sample, b.sample, '페이지를 새로 열면 아이콘이 바뀐다');
  assert.deepEqual(a.sample[1], a.sample[3], '같은 ID 인데 아이콘이 다르다');
  assert.notEqual(a.sample[1][1], a.sample[2][1], '다른 ID 가 같은 아이콘');
  assert.ok(!/var\(|https?:\/\/(?!www\.w3\.org)/.test(a.sheet), '아이콘 시트에 외부 주소·변수가 남음');
  assert.equal(a.sheetCells, a.nSpecies * a.nPalettes, '시트 칸 수');
  // 고정 값 — 해시가 바뀌면 모든 시청자의 아이콘이 바뀐다(의도한 변경이면 이 값을 고친다)
  assert.equal(a.sample[0][0], 1901886622, `해시 값이 바뀜: ${a.sample[0][0]}`);
  assert.equal(Object.keys(a.species).length, a.nSpecies, `안 쓰인 종류가 있다: ${JSON.stringify(a.species)}`);
  assert.equal(Object.keys(a.palettes).length, a.nPalettes, `안 쓰인 색이 있다: ${JSON.stringify(a.palettes)}`);
  const expect = 3000 / a.nSpecies;
  for (const [k, n] of Object.entries(a.species)) assert.ok(n > expect * 0.6 && n < expect * 1.4, `${k} 이 치우침: ${n}(기대 ${expect.toFixed(0)})`);
  assert.ok(a.combos > 300, `조합이 적다: ${a.combos}`);
  assert.ok(a.neighbours >= 7, `끝만 다른 ID 10개가 ${a.neighbours}가지 그림으로만 갈린다`);
});

// ── AV02 줄과 사진 ──
await check('AV02 lines get a fixed-size round avatar; a late photo shifts nothing', async () => {
  const { ctx, page } = await openChat();
  await page.evaluate(() => window.__say(['photoUser01', 'plainUser02']));
  const before = await page.evaluate(() => [...window.__panel.list.querySelectorAll('.chat-line.msg')].map((l) => {
    const av = l.querySelector('.chat-avatar');
    const r = av.getBoundingClientRect();
    const cs = getComputedStyle(av);
    return { h: l.getBoundingClientRect().height, w: r.width, ah: r.height, radius: cs.borderRadius, icon: getComputedStyle(av).backgroundImage.includes('blob:') && av.style.backgroundPosition !== '', photo: Boolean(av.querySelector('.av-photo')) };
  }));
  assert.equal(before.length, 2);
  for (const b of before) {
    assert.ok(b.icon, '아이콘이 없다');
    assert.equal(b.photo, false, '묻기도 전에 사진이 있다');
    assert.ok(b.w >= 30 && b.w === b.ah, `아바타 크기 ${b.w}×${b.ah}`);
    assert.equal(b.radius, '50%', `원형이 아니다: ${b.radius}`);
  }
  await page.waitForFunction(() => document.querySelector('.chat-avatar[data-av="photoUser01"] img.av-photo')?.complete, null, { timeout: 5000 });
  const after = await page.evaluate(() => [...window.__panel.list.querySelectorAll('.chat-line.msg')].map((l) => {
    const av = l.querySelector('.chat-avatar');
    const img = av.querySelector('.av-photo');
    return { h: l.getBoundingClientRect().height, w: av.getBoundingClientRect().width, photo: img ? { lazy: img.loading, decoding: img.decoding, w: img.getBoundingClientRect().width, nat: img.naturalWidth } : null };
  }));
  assert.deepEqual(after.map((x) => x.h), before.map((x) => x.h), '사진이 오면서 줄 높이가 바뀜');
  assert.equal(after[0].w, before[0].w);
  assert.ok(after[0].photo && after[0].photo.nat > 0, '사진이 붙지 않음');
  assert.equal(after[0].photo.lazy, 'lazy');
  assert.equal(after[0].photo.decoding, 'async');
  assert.equal(after[0].photo.w, before[0].w, '사진이 아바타 칸을 벗어남');
  assert.equal(after[1].photo, null, '사진 없는 사람에게 사진');
  // 같은 사람이 다시 말하면 묻지 않고 바로 사진
  const calls = await page.evaluate(() => window.__avCalls.flat().length);
  await page.evaluate(() => window.__say(['photoUser01']));
  const again = await page.evaluate(() => Boolean([...document.querySelectorAll('.chat-avatar[data-av="photoUser01"]')].pop().querySelector('.av-photo')));
  assert.ok(again, '아는 사진을 바로 쓰지 않음');
  await page.waitForTimeout(1700);
  assert.equal(await page.evaluate(() => window.__avCalls.flat().length), calls, '아는 사람을 다시 물음');
  await ctx.close();
});

// ── AV03 실패하면 아이콘 ──
await check('AV03 a broken photo falls back to the icon and is not retried', async () => {
  const { ctx, page } = await openChat();
  await page.evaluate(() => window.__say(['brokenUser1']));
  await page.waitForFunction(() => window.__avCalls.flat().includes('brokenUser1'), null, { timeout: 5000 });
  await page.waitForFunction(() => {
    const av = document.querySelector('.chat-avatar[data-av="brokenUser1"]');
    return av && !av.querySelector('.av-photo') && window.__avCalls.length > 0;
  }, null, { timeout: 5000 });
  await page.waitForTimeout(300);
  const r = await page.evaluate(() => {
    window.__say(['brokenUser1']);
    const avs = [...document.querySelectorAll('.chat-avatar[data-av="brokenUser1"]')];
    return { n: avs.length, photos: avs.filter((a) => a.querySelector('.av-photo')).length, icons: avs.filter((a) => (getComputedStyle(a).backgroundImage.includes('blob:') && a.style.backgroundPosition !== '')).length };
  });
  assert.equal(r.n, 2);
  assert.equal(r.photos, 0, '깨진 사진이 남았거나 다시 붙음');
  assert.equal(r.icons, 2, '아이콘이 없다');
  await page.waitForTimeout(1700);
  assert.equal(await page.evaluate(() => window.__avCalls.flat().filter((u) => u === 'brokenUser1').length), 1, '깨진 사진을 다시 물음');
  await ctx.close();
});

// ── AV04 속도 제한 ──
await check('AV04 lookups are paced (≤3 per 1.5s), newest first, once per user', async () => {
  const { ctx, page } = await openChat();
  const users = Array.from({ length: 40 }, (_, i) => `burstUser${String(i).padStart(2, '0')}`);
  await page.evaluate((u) => { window.__t0 = performance.now(); window.__say(u); window.__say(u); }, users);
  await page.waitForTimeout(3300);
  const calls = await page.evaluate(() => window.__avCalls);
  assert.ok(calls.length >= 1 && calls.length <= 2, `3.3초 동안 ${calls.length}번 물음`);
  assert.ok(calls.every((c) => c.length <= 3), `한 번에 ${calls.map((c) => c.length)}명`);
  assert.deepEqual(calls[0], ['burstUser39', 'burstUser38', 'burstUser37'], `최근 사람부터가 아님: ${calls[0]}`);
  const flat = calls.flat();
  assert.equal(new Set(flat).size, flat.length, '같은 사람을 두 번 물음');
  await ctx.close();
});

// ── AV05 가려진 창 ──
await check('AV05 no lookups while the window is hidden; resumes when shown', async () => {
  const { ctx, page } = await openChat();
  await page.evaluate(() => { window.__setHidden(true); window.__say(['hiddenUser1', 'hiddenUser2']); });
  await page.waitForTimeout(2500);
  assert.equal(await page.evaluate(() => window.__avCalls.length), 0, '가려진 창에서 물음');
  await page.evaluate(() => window.__setHidden(false));
  await page.waitForFunction(() => window.__avCalls.length > 0, null, { timeout: 4000 });
  await ctx.close();
});

// ── AV06 설정 ──
await check('AV06 setting: icon-only never looks up photos; off renders no avatar', async () => {
  {
    const { ctx, page } = await openChat({ chatAvatars: 'icon' });
    await page.evaluate(() => window.__say(['photoUser01', 'photoUser02']));
    await page.waitForTimeout(2000);
    const r = await page.evaluate(() => ({ calls: window.__avCalls.length, icons: [...document.querySelectorAll('.chat-line.msg .chat-avatar')].filter((a) => (getComputedStyle(a).backgroundImage.includes('blob:') && a.style.backgroundPosition !== '')).length, photos: document.querySelectorAll('.av-photo').length }));
    assert.equal(r.calls, 0, '아이콘만인데 사진을 물음');
    assert.equal(r.photos, 0);
    assert.equal(r.icons, 2);
    await ctx.close();
  }
  {
    const { ctx, page } = await openChat({ chatAvatars: 'off' });
    await page.evaluate(() => window.__say(['photoUser01']));
    await page.waitForTimeout(1800);
    const r = await page.evaluate(() => ({ calls: window.__avCalls.length, avatars: window.__panel.list.querySelectorAll('.chat-avatar').length, skin: window.__panel.list.closest('.no-avatars') !== null || document.querySelector('.chat-skin.no-avatars') !== null }));
    assert.equal(r.calls, 0, '숨김인데 사진을 물음');
    assert.equal(r.avatars, 0, '숨김인데 아바타를 그림');
    assert.ok(r.skin, 'no-avatars 클래스가 없다');
    await ctx.close();
  }
});

// ── AV07 줄 지우기·익명 ──
await check('AV07 trimmed lines release photo src; anonymous lines are not looked up', async () => {
  const { ctx, page } = await openChat();
  await page.evaluate(() => window.__say(['photoUser01']));
  await page.waitForFunction(() => document.querySelector('.chat-avatar[data-av="photoUser01"] img.av-photo'), null, { timeout: 5000 });
  const r = await page.evaluate(async () => {
    const img = document.querySelector('.chat-avatar[data-av="photoUser01"] img.av-photo');
    const max = window.__panel.maxLines;
    // 익명 후원처럼 사용자 ID 가 없는 줄로 밀어낸다
    for (let i = 0; i < Math.ceil(max * 1.4 / 50); i++) {
      window.__panel.append(Array.from({ length: 50 }, (_, k) => ({ id: `anon-${i}-${k}`, userId: '', nickname: '익명', content: 'ㅋ', kind: 'normal', role: 'viewer', badges: [], emojis: {}, timestamp: Date.now() })));
      window.__panel.flushLines();
    }
    await new Promise((res) => setTimeout(res, 1800));
    const anonAv = window.__panel.list.querySelector('.chat-line.msg .chat-avatar');
    return { connected: img.isConnected, src: img.getAttribute('src'), anonData: anonAv?.hasAttribute('data-av'), anonIcon: Boolean(anonAv && getComputedStyle(anonAv).backgroundImage.includes('blob:') && anonAv.style.backgroundPosition), calls: window.__avCalls.flat() };
  });
  assert.equal(r.connected, false, '줄이 지워지지 않음');
  assert.equal(r.src, null, `지운 줄 사진의 src 가 남음: ${r.src}`);
  assert.equal(r.anonData, false, '익명 줄에 사용자 표시');
  assert.ok(r.anonIcon, '익명 줄에 아이콘이 없다');
  assert.ok(!r.calls.some((u) => !u || u === 'unknown'), `익명을 물음: ${r.calls}`);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 10).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
