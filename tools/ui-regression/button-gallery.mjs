// 버튼 갤러리 — 앱의 단추 종류(기본·보조·위험·고스트·아이콘·둥근 아이콘·툴바·세그먼트/탭·토글·칩·글자 단추·메뉴 줄·닫기)를
// 실제 앱 CSS 로 한 화면에 모아 다크·라이트로 찍는다. 디자인 전후 비교용(검사가 아니라 사진). 모의 IPC 라 실제 서비스는 건드리지 않는다.
// 저장소 루트에서: CVIEW_UI_SHOTS=<폴더> node tools/ui-regression/button-gallery.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 WebKit. 갤러리 말고 주요 화면(홈·보관함·설정·멀티라이브·채널)도 함께 찍는다.)
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, go, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS || 'button-gallery-shots';
fs.mkdirSync(SHOTS, { recursive: true });

const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>';
const X = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
const GALLERY = `
<div class="bg-gallery" style="position:fixed;inset:0;z-index:9999;overflow:auto;padding:28px 36px;background:var(--deck);display:grid;gap:18px;align-content:start">
  <style>.bg-gallery h4{font-size:var(--fs-footnote);color:var(--text-tertiary);font-weight:600;margin:0 0 8px}
  .bg-gallery .row{display:flex;flex-wrap:wrap;align-items:center;gap:10px}
  .bg-gallery .pane{padding:14px;border-radius:var(--radius-lg);background:var(--surface-base);border:1px solid var(--border-subtle)}</style>
  <div><h4>글자 단추 — 보조 · 기본 · 고스트 · 위험 · 꺼짐 · 큰 · 아이콘 포함</h4><div class="row">
    <button class="btn">보조</button><button class="btn primary">기본 동작</button><button class="btn ghost">고스트</button>
    <button class="btn danger">삭제…</button><button class="btn" disabled>꺼짐</button><button class="btn primary" disabled>꺼짐</button>
    <button class="btn primary lg">큰 단추</button><button class="btn lg">큰 보조</button>
    <button class="btn" data-armed="1">한 번 더 누르면 삭제</button></div></div>
  <div class="pane"><h4>면 위 (카드·시트 안)</h4><div class="row">
    <button class="btn">보조</button><button class="btn primary">기본 동작</button><button class="btn ghost">고스트</button><button class="btn danger">초기화…</button>
    <button class="link-btn">글자 단추</button><button class="link-btn" data-armed="1">무장</button></div></div>
  <div><h4>아이콘 · 둥근 아이콘 · 툴바 · 닫기</h4><div class="row">
    <button class="icon-btn" aria-label="추가">${ICON}</button><button class="icon-btn round" aria-label="추가">${ICON}</button>
    <button class="icon-btn" aria-label="닫기">${X}</button><button class="notice-x" aria-label="닫기">${X}</button>
    <button class="ml-tb-icon" aria-label="툴바">${ICON}</button><button class="chat-hbtn" aria-label="채팅 머리">${ICON}</button>
    <button class="back-btn">‹ 뒤로</button><button class="load-more btn">더 보기</button></div></div>
  <div><h4>세그먼트 · 탭</h4><div class="row">
    <div class="segmented" role="tablist"><button class="seg active" role="tab" aria-selected="true">개요</button><button class="seg" role="tab">다시보기</button><button class="seg" role="tab">클립</button></div>
    <div class="segmented"><button class="seg">탭</button><button class="seg active">그리드</button><button class="seg">통합</button></div></div></div>
  <div><h4>칩 · 토글</h4><div class="row">
    <div class="chips"><button class="chip active">전체<span class="chip-count">12</span></button><button class="chip">게임<span class="chip-count">5</span></button><button class="chip" aria-pressed="true">음악</button><button class="chip">토크</button></div>
    <label class="toggle"><input type="checkbox" checked><span></span></label><label class="toggle"><input type="checkbox"><span></span></label>
    <label class="toggle"><input type="checkbox" disabled><span></span></label></div></div>
  <div><h4>메뉴</h4><div class="row" style="align-items:flex-start">
    <div class="ctx-menu" role="menu" style="position:static;display:block"><button class="ctx-item" role="menuitem">답장</button><button class="ctx-item" role="menuitem">복사</button><button class="ctx-item danger" role="menuitem">사용자 차단</button></div>
  </div></div>
</div>`;

const shot = (page, name) => page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
const browser = await engine.launch();
try {
  for (const theme of ['dark', 'light']) {
    const { ctx, page } = await openApp(browser, { width: 1280, height: 820, theme, welcomed: true });
    await page.waitForTimeout(600);
    await page.evaluate((html) => document.body.insertAdjacentHTML('beforeend', html), GALLERY);
    await page.waitForTimeout(300);
    await shot(page, `gallery-${theme}`);
    // 키보드 초점·호버 모습
    await page.hover('.bg-gallery .btn:not(.primary)');
    await page.evaluate(() => document.querySelector('.bg-gallery .btn.primary').focus({ focusVisible: true }));
    await page.keyboard.press('Shift+Tab'); await page.keyboard.press('Tab');
    await page.waitForTimeout(300);
    await shot(page, `gallery-${theme}-states`);
    await page.evaluate(() => document.querySelector('.bg-gallery').remove());
    for (const [route, params, wait] of [['home', {}, '.hero, .home'], ['library', {}, '.lib-tabs'], ['settings', {}, '.settings-search'], ['multilive', {}, '.ml-empty'], ['channel', { channelId: 'ch1' }, '.channel-tabs']]) {
      await go(page, route, params);
      await page.waitForSelector(wait, { timeout: 5000 }).catch(() => {});
      await page.waitForTimeout(900);
      await shot(page, `${route}-${theme}`);
    }
    await ctx.close();
  }
} finally { await browser.close(); server.close(); }
console.log(`saved to ${SHOTS}`);
