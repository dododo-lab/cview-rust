// UI/UX 감사(2026-11-23) Phase 1 회귀 검사 — 실제 앱 CSS·모듈로 고친 결함이 다시 생기지 않는지 본다.
// 모의 IPC 라 실제 서비스는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/ui-audit.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
import { createRequire } from 'node:module';
import { server, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;

let pass = 0;
let fail = 0;
async function check(id, name, fn) {
  try {
    await fn();
    pass++;
    console.log(`PASS ${id} ${name}`);
  } catch (e) {
    fail++;
    console.log(`FAIL ${id} ${name}\n     ${String(e && e.message || e).split('\n').join('\n     ')}`);
  }
}
function expect(cond, msg) { if (!cond) throw new Error(msg); }

// 페이지 안에서 쓰는 명암비 계산(rgb() 문자열 두 개)
const CONTRAST = `(${String((a, b) => {
  const parse = (s) => s.match(/[\d.]+/g).slice(0, 3).map(Number);
  const lum = (c) => { const [r, g, bl] = c.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * bl; };
  const [x, y] = [lum(parse(a)), lum(parse(b))].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
})})`;

const browser = await engine.launch();
try {
  for (const theme of ['dark', 'light']) {
    const { ctx, page, errors } = await openApp(browser, { theme, welcomed: true });

    await check('U01', `${theme}: 채팅 사용자 말풍선의 .up-* 가 사용률(usage.css) 규칙에 덮이지 않는다`, async () => {
      const r = await page.evaluate(() => {
        const host = document.createElement('div');
        host.innerHTML = `
          <div class="user-pop" id="t-user"><div class="up-head"></div><span class="up-live">방송 중</span><span class="up-sub">구독 3개월</span></div>`;
        document.body.append(host);
        const cs = (sel) => getComputedStyle(document.querySelector(sel));
        const root = getComputedStyle(document.documentElement);
        const probe = document.createElement('span');
        document.body.append(probe);
        const colorOf = (v) => { probe.style.color = `var(${v})`; return getComputedStyle(probe).color; };
        const out = {
          userLive: cs('#t-user .up-live').color, userLiveDisplay: cs('#t-user .up-live').display, liveText: colorOf('--live-text'),
          userSub: cs('#t-user .up-sub').color, subscription: colorOf('--subscription'),
          userHeadAlign: cs('#t-user .up-head').alignItems, hasLiveText: root.getPropertyValue('--live-text').trim() !== '',
        };
        host.remove(); probe.remove();
        return out;
      });
      expect(r.hasLiveText, '--live-text 토큰이 없다');
      expect(r.userLive === r.liveText, `말풍선 '방송 중' 색 ${r.userLive} ≠ --live-text ${r.liveText}`);
      expect(r.userLiveDisplay !== 'inline-flex', '말풍선 .up-live 가 사용률 알약 모양(inline-flex)을 물려받았다');
      expect(r.userSub === r.subscription, `말풍선 구독 색 ${r.userSub} ≠ --subscription ${r.subscription}`);
      expect(r.userHeadAlign === 'center', `말풍선 머리 정렬 ${r.userHeadAlign}`);
    });

    await check('U02', `${theme}: 작은 빨강 글자·링크·시청 화면 안내막 글자 대비`, async () => {
      const r = await page.evaluate((contrastSrc) => {
        const contrast = eval(contrastSrc); // eslint-disable-line no-eval
        const bg = getComputedStyle(document.body).backgroundColor;
        const mk = (html) => { const d = document.createElement('div'); d.innerHTML = html; document.body.append(d); return d; };
        const d = mk('<a href="#x" id="t-a">링크</a><span class="live-dot-text" id="t-live">LIVE</span>');
        const a = getComputedStyle(document.getElementById('t-a')).color;
        const live = getComputedStyle(document.getElementById('t-live')).color;
        d.remove();
        const overlay = getComputedStyle(document.querySelector('#viewer .player-overlay')).color;
        return { link: contrast(a, bg), live: contrast(live, bg), overlay: contrast(overlay, 'rgb(0, 0, 0)'), bg };
      }, CONTRAST);
      expect(r.link >= 4.5, `링크 대비 ${r.link.toFixed(2)}:1 (바탕 ${r.bg})`);
      expect(r.live >= 4.5, `작은 LIVE 글자 대비 ${r.live.toFixed(2)}:1 (바탕 ${r.bg})`);
      expect(r.overlay >= 7, `검정 무대 위 안내막 글자 대비 ${r.overlay.toFixed(2)}:1`);
    });

    await check('U03', `${theme}: 사용률 지표 색이 바탕에서 3:1 이상(주황·청록·노랑)`, async () => {
      const r = await page.evaluate((contrastSrc) => {
        const contrast = eval(contrastSrc); // eslint-disable-line no-eval
        const probe = document.createElement('span');
        document.body.append(probe);
        const bg = getComputedStyle(document.body).backgroundColor;
        const out = {};
        for (const v of ['--u-gpu', '--u-mem', '--u-net']) { probe.style.color = `var(${v})`; out[v] = contrast(getComputedStyle(probe).color, bg); }
        probe.remove();
        return out;
      }, CONTRAST);
      for (const [k, v] of Object.entries(r)) expect(v >= 3, `${k} ${v.toFixed(2)}:1`);
    });

    if (theme === 'dark') {
      await check('U04', '같은 문구의 되돌리기 알림 두 개가 둘 다 남고, 첫 번째 되돌리기가 그 항목만 살린다', async () => {
        const r = await page.evaluate(async () => {
          const { deferDelete } = await import('/js/confirm.js');
          const log = [];
          const item = (n) => ({ message: '북마크를 지웠습니다', ms: 1200, hide: () => log.push(`hide${n}`), restore: () => log.push(`restore${n}`), commit: async () => { log.push(`commit${n}`); } });
          deferDelete(item(1));
          deferDelete(item(2));
          const actions = [...document.querySelectorAll('#toast-stack .toast .toast-action')];
          const count = actions.length;
          actions[0]?.click();
          await new Promise((res) => setTimeout(res, 1700));
          return { count, log };
        });
        expect(r.count === 2, `되돌리기 단추 ${r.count}개 — 같은 문구 중복 제거로 첫 알림이 사라졌다`);
        expect(r.log.includes('restore1') && !r.log.includes('commit1'), `첫 항목: ${r.log.join(',')}`);
        expect(r.log.includes('commit2') && !r.log.includes('restore2'), `둘째 항목: ${r.log.join(',')}`);
      });

      await check('U05', 'kbd(): 띄어 쓴 설명엔 + 를 붙이지 않고, 키가 붙거나 끝이면 붙인다', async () => {
        const r = await page.evaluate(async () => {
          const { kbd } = await import('/js/platform.js');
          return [kbd('(⇧ 60초)', true), kbd('⇧⌘F', true), kbd('⇧⌘', true), kbd('영상 전체 화면 (⇧⌘F)', true), kbd('⇧⌘F', false)];
        });
        expect(r[0] === '(Shift 60초)', `띄어 쓴 설명: ${r[0]} — "Shift+ 60초" 처럼 + 가 매달렸다`);
        expect(r[1] === 'Ctrl+Shift+F' && r[2] === 'Ctrl+Shift+' && r[3] === '영상 전체 화면 (Ctrl+Shift+F)' && r[4] === '⇧⌘F', `kbd: ${JSON.stringify(r)}`);
      });
    }

    await check('U06', `${theme}: 19 배지는 + 멀티라이브 단추와 겹치지 않고, 플레이어 패널은 무대 위로 넘치지 않는다`, async () => {
      const r = await page.evaluate(async () => {
        const { liveCard } = await import('/js/cards.js');
        const host = document.createElement('div');
        host.className = 'grid';
        host.style.cssText = 'position:fixed;left:0;top:0;width:320px;z-index:9999';
        host.innerHTML = liveCard({ channelId: 'x', channelName: '채널', liveTitle: '제목', isLive: true, adult: true, viewerCountText: '1', thumbnailUrl: '' });
        document.body.append(host);
        const add = host.querySelector('.card-ml-add').getBoundingClientRect();
        const adult = host.querySelector('.badge-adult').getBoundingClientRect();
        const overlap = !(adult.right <= add.left || add.right <= adult.left || adult.bottom <= add.top || add.bottom <= adult.top);
        host.remove();
        const stage = document.createElement('div');
        stage.className = 'player-stage';
        stage.style.cssText = 'position:fixed;left:0;top:0;width:640px;height:300px';
        stage.innerHTML = '<div class="player-panel" style="height:2000px"></div>';
        document.body.append(stage);
        const p = stage.querySelector('.player-panel').getBoundingClientRect();
        stage.remove();
        return { overlap, panelTop: p.top };
      });
      expect(!r.overlap, '19 배지와 + 단추가 겹친다');
      expect(r.panelTop >= 0, `플레이어 패널 위가 무대 밖(${r.panelTop}px)`);
    });

    await check('U07', `${theme}: 호버 미리보기 받는 동안 썸네일 아래에 2px 빛줄기(위쪽 검은 띠가 아니라)`, async () => {
      const r = await page.evaluate(() => {
        const host = document.createElement('div');
        host.innerHTML = '<article class="card" style="position:fixed;left:0;top:0;width:320px;z-index:9999"><div class="thumb preview-loading" style="position:relative;height:180px"></div></article>';
        document.body.append(host);
        const thumb = host.querySelector('.thumb');
        const after = getComputedStyle(thumb, '::after');
        const out = { height: after.height, top: after.top, bottom: after.bottom, opacity: after.opacity };
        host.remove();
        return out;
      });
      expect(r.height === '2px' && r.bottom === '0px' && r.opacity === '1', `::after ${JSON.stringify(r)}`);
    });

    if (errors.length) console.log(`  (페이지 오류 ${errors.length}건: ${errors.slice(0, 3).join(' | ')})`);
    await ctx.close();
  }
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${pass}/${pass + fail} passed`);
process.exit(fail ? 1 : 0);
