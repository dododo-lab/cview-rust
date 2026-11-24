// 치지직 웹 공식 플레이어 지연 기준값 — Playwright WebKit(헤드리스)으로 라이브 페이지를 열고
// 초마다 벽시계 지연(Date.now() − hls.playingDate)·hls.latency·targetLatency·전방 버퍼·배속을 JSONL 로 남긴다.
// 사용: node web-ref.mjs <channelId> <초> <출력.jsonl> [proxy]
import { webkit } from 'playwright';
import fs from 'node:fs';

const [channelId, secs = '600', out = 'web.jsonl', proxy = 'http://192.168.64.1:18888'] = process.argv.slice(2);
const browser = await webkit.launch({ headless: true, proxy: { server: proxy } });
const UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36';
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, locale: 'ko-KR', userAgent: UA });
// 치지직 웹이 Chrome 처럼 hls.js(MSE) 경로로 재생하게 — 기본 HLS 재생 가능 여부를 숨긴다.
await ctx.addInitScript(() => {
  const orig = HTMLMediaElement.prototype.canPlayType;
  HTMLMediaElement.prototype.canPlayType = function (t) { return /mpegurl/i.test(String(t)) ? '' : orig.call(this, t); };
  Object.defineProperty(navigator, 'vendor', { get: () => 'Google Inc.' });
});
const page = await ctx.newPage();
await page.goto(`https://chzzk.naver.com/live/${channelId}`, { waitUntil: 'domcontentloaded', timeout: 60000 });

// 비디오 요소의 React 파이버를 거슬러 올라가며 hls.js 인스턴스(playingDate·latency·targetLatency 를 가진 객체)를 찾는다.
const finder = () => {
  if (window.__webHls && window.__webHls.media) return true;
  const v = document.querySelector('video');
  if (!v) return false;
  const seen = new WeakSet();
  const isHls = (o) => { try { return o && typeof o === 'object' && 'playingDate' in o && 'targetLatency' in o && 'latency' in o && o.config; } catch (e) { return false; } };
  const scan = (root, depth) => {
    const q = [[root, 0]];
    let n = 0;
    while (q.length && n < 200000) {
      const [o, d] = q.shift(); n++;
      if (!o || typeof o !== 'object' || seen.has(o)) continue;
      seen.add(o);
      if (o instanceof Node || o instanceof Window) continue;
      if (isHls(o)) return o;
      if (d >= depth) continue;
      let keys = [];
      try { keys = Object.keys(o); } catch (e) {}
      for (const k of keys) {
        if (k === 'return' || k === 'child' || k === 'sibling' || k === '_owner' || k === 'alternate') continue;
        let val; try { val = o[k]; } catch (e) { continue; }
        if (val && typeof val === 'object') q.push([val, d + 1]);
      }
    }
    return null;
  };
  // 재생 중인 video 는 플레이어 라이브러리가 만든 요소라 파이버가 없다 — 파이버가 있는 조상까지 올라간다.
  let vid = [...document.querySelectorAll('video')].find((x) => x.readyState > 0) || v;
  let el = vid;
  while (el && !Object.keys(el).some((k) => k.startsWith('__reactFiber$'))) el = el.parentElement;
  while (el) {
    const fk = Object.keys(el).find((k) => k.startsWith('__reactFiber$'));
    let f = fk ? el[fk] : null;
    let hops = 0;
    while (f && hops < 60) {
      for (const src of [f.memoizedProps, f.memoizedState, f.stateNode && !(f.stateNode instanceof Node) ? f.stateNode : null, f.ref && f.ref.current]) {
        const h = src && scan(src, 9);
        if (h) { window.__webHls = h; return true; }
      }
      f = f.return; hops++;
    }
    el = null;
  }
  return false;
};

const deadline = Date.now() + 90000;
let found = false;
while (Date.now() < deadline) {
  await page.evaluate(() => { const v = document.querySelector('video'); if (v) { v.muted = true; v.play().catch(() => {}); } });
  found = await page.evaluate(finder).catch(() => false);
  if (found) break;
  await page.waitForTimeout(2000);
}
console.log('hls found:', found);
const fd = fs.openSync(out, 'w');
const end = Date.now() + Number(secs) * 1000;
while (Date.now() < end) {
  const s = await page.evaluate(() => {
    let h = window.__webHls;
    const v = h && h.media;
    if (!h || !v) { return { t: Date.now(), lost: 1 }; }
    const pd = h.playingDate;
    let ahead = 0;
    for (let i = 0; i < v.buffered.length; i++) if (v.buffered.start(i) <= v.currentTime + 0.1 && v.buffered.end(i) > v.currentTime) ahead = v.buffered.end(i) - v.currentTime;
    return { t: Date.now(), hidden: document.hidden ? 1 : 0, wall: pd ? (Date.now() - pd.getTime()) / 1000 : null,
      latency: h.latency, target: h.targetLatency, ahead, rate: v.playbackRate, ll: h.lowLatencyMode ? 1 : 0, paused: v.paused ? 1 : 0 };
  }).catch((e) => ({ t: Date.now(), err: String(e).slice(0, 80) }));
  if (s.lost) await page.evaluate(finder).catch(() => false);
  fs.writeSync(fd, JSON.stringify(s) + '\n');
  await page.waitForTimeout(1000);
}
fs.closeSync(fd);
await browser.close();
