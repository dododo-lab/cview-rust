// 채팅 글자 대비 검사(감사 UX-M09) — 후원 카드 6단계의 금액 글자·아이콘, 후원 카드 위 닉네임 9색, 멘션 표시, 읽음 구분선을
// 실제 앱 CSS 로 그려 바탕과의 명암비를 잰다(다크·라이트). 모의 IPC 라 실제 서비스는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/chat-contrast.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit, CVIEW_CONTRAST_REPORT=1 이면 수치표만)
import { createRequire } from 'node:module';
import { server, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const REPORT = process.env.CVIEW_CONTRAST_REPORT === '1';

// 글자 4.5:1, 아이콘(그래픽) 3:1 — WCAG 2.1 AA
const TEXT = 4.5;
const GRAPHIC = 3;

let pass = 0;
let fail = 0;
function check(id, name, problems) {
  if (problems.length) { fail++; console.log(`FAIL ${id} ${name}\n     ${problems.join('\n     ')}`); } else { pass++; console.log(`PASS ${id} ${name}`); }
}

// 페이지 안: 색 문자열(rgb()/color(srgb …)) → [r,g,b,a](0~255), 반투명 겹침, 명암비
const HELPERS = () => {
  window.__c = {
    parse(s) {
      const n = (s.match(/[\d.]+/g) || []).map(Number);
      if (s.startsWith('color(')) return [n[0] * 255, n[1] * 255, n[2] * 255, n.length > 3 ? n[3] : 1];
      return [n[0], n[1], n[2], n.length > 3 ? n[3] : 1];
    },
    over(top, under) { const a = top[3]; return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a)).concat(1); },
    lum(c) { const [r, g, b] = c.slice(0, 3).map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; },
    ratio(a, b) { const [x, y] = [this.lum(a), this.lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); },
    /** 요소 뒤에 실제로 깔린 바탕색(반투명이면 조상과 겹친다) */
    bg(el) {
      const layers = [];
      for (let n = el; n; n = n.parentElement) {
        const c = this.parse(getComputedStyle(n).backgroundColor);
        if (c[3] > 0) { layers.push(c); if (c[3] >= 1) break; }
      }
      let out = [255, 255, 255, 1];
      if (layers.length && layers[layers.length - 1][3] >= 1) out = layers.pop();
      for (const l of layers.reverse()) out = this.over(l, out);
      return out;
    },
  };
};

const browser = await engine.launch();
try {
  for (const theme of ['dark', 'light']) {
    const { ctx, page } = await openApp(browser, { theme, welcomed: true });
    await page.evaluate(HELPERS);
    const r = await page.evaluate(async () => {
      const { eventCardHtml } = await import('/js/donations.js');
      const host = document.createElement('section');
      host.className = 'chat chat-skin';
      host.style.cssText = 'position:fixed;left:0;top:0;width:360px;height:900px;z-index:9999';
      const { donationLevel } = await import('/js/donations.js');
      const lines = [1, 2, 3, 4, 5, 6].map((lv) => `<div class="chat-line ev ev-donation donation ev-lv-${lv}" data-uid="u${lv}" data-level="${donationLevel([1000, 10000, 50000, 100000, 500000, 1000000][lv - 1])}">${eventCardHtml(
        { kind: 'donation', donationAmount: [1000, 10000, 50000, 100000, 500000, 1000000][lv - 1], donationType: 'CHAT', nickname: `닉${lv}`, content: '응원합니다' },
        { nick: `<span class="nick nc nc-${lv + 2}">닉네임${lv}</span>`, content: '응원합니다' },
      )}</div>`).join('');
      const nicks = Array.from({ length: 9 }, (_, i) => `<div class="chat-line ev ev-donation donation ev-lv-1"><div class="ev-card"><div class="ev-from"><span class="nick nc nc-${i}" data-nc="${i}">닉네임</span></div></div></div>`).join('');
      host.innerHTML = `<div class="chat-list">${lines}${nicks}<div class="chat-line msg mention"><span class="msg-mention">@나</span></div><div class="read-divider">여기까지 읽음</div></div>`;
      document.body.append(host);
      const C = window.__c;
      const fg = (el) => C.parse(getComputedStyle(el).color);
      const out = { value: [], icon: [], nick: [], mention: null, divider: null };
      for (const line of host.querySelectorAll('.chat-line.ev[data-uid]')) {
        const v = line.querySelector('.ev-value');
        const i = line.querySelector('.ev-icon');
        if (v && getComputedStyle(v).webkitTextFillColor !== 'rgba(0, 0, 0, 0)' && !getComputedStyle(v).backgroundClip.includes('text')) out.value.push(C.ratio(fg(v), C.bg(v)));
        else out.value.push(null); // 그라디언트 글자(6단계 다크) — 따로 보지 않는다
        if (i) {
          // 아이콘 바탕이 그라디언트면 그 색 띠의 끝색마다 재서 가장 낮은 값(흰 글리프가 가장 밝은 끝에서 가장 흐리다)
          const stops = (getComputedStyle(i).backgroundImage.match(/(rgba?\([^)]*\)|color\([^)]*\))/g) || []).map((s) => C.over(C.parse(s), C.bg(i.parentElement)));
          out.icon.push(stops.length ? Math.min(...stops.map((s) => C.ratio(fg(i), s))) : C.ratio(fg(i), C.bg(i)));
        }
      }
      for (const n of host.querySelectorAll('[data-nc]')) out.nick.push(C.ratio(fg(n), C.bg(n)));
      const m = host.querySelector('.msg-mention');
      if (m && getComputedStyle(m).display !== 'none') out.mention = C.ratio(fg(m), C.bg(m));
      const d = host.querySelector('.read-divider');
      out.divider = C.ratio(fg(d), C.bg(d));
      host.remove();
      return out;
    });
    const f = (x) => (x == null ? '—' : x.toFixed(2));
    if (REPORT) console.log(theme, JSON.stringify({ value: r.value.map(f), icon: r.icon.map(f), nick: r.nick.map(f), mention: f(r.mention), divider: f(r.divider) }));
    check('C01', `${theme}: 후원 카드 1~6단계 금액 글자 ${TEXT}:1 이상`, r.value.map((x, i) => (x != null && x < TEXT ? `${i + 1}단계 ${f(x)}:1` : null)).filter(Boolean));
    check('C02', `${theme}: 후원 카드 아이콘 ${GRAPHIC}:1 이상`, r.icon.map((x, i) => (x < GRAPHIC ? `${i + 1}단계 ${f(x)}:1` : null)).filter(Boolean));
    check('C03', `${theme}: 후원 카드 위 닉네임 9색 ${TEXT}:1 이상`, r.nick.map((x, i) => (x < TEXT ? `nc-${i} ${f(x)}:1` : null)).filter(Boolean));
    check('C04', `${theme}: 멘션 표시·읽음 구분선 ${TEXT}:1 이상`, [r.mention != null && r.mention < TEXT ? `멘션 ${f(r.mention)}:1` : null, r.divider < TEXT ? `읽음 구분선 ${f(r.divider)}:1` : null].filter(Boolean));
    await ctx.close();
  }
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${pass}/${pass + fail} passed`);
process.exit(fail ? 1 : 0);
