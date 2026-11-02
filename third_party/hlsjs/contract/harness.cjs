// CView hls.js 포크 계약 테스트 공용 도구.
// 실제 빌드 번들(기본: 앱 리소스, HLSJS_BUNDLE 로 바꿀 수 있음)을 Node 에서 불러, 가짜 XMLHttpRequest 로
// 재생목록을 실제 파서·로더·컨트롤러에 통과시킨다. 네트워크·디코드·MSE 는 없다.
'use strict';
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '../../..');
const bundlePath = process.env.HLSJS_BUNDLE || path.join(root, 'app-ui/player/hls.min.js');

let now = 1_000_000;
Object.defineProperty(globalThis, 'performance', { value: { now: () => now }, configurable: true });
globalThis.self = globalThis;
globalThis.location = { href: 'https://example.invalid/player.html' };

const xhrs = [];
class OfflineXHR {
  constructor() { this.readyState = 0; this.status = 0; this.headers = {}; this.responseHeaders = {}; xhrs.push(this); }
  open(method, url) { this.method = method; this.url = url; this.readyState = 1; }
  setRequestHeader(name, value) { this.headers[name] = value; }
  getResponseHeader(name) { return this.responseHeaders[name.toLowerCase()] ?? null; }
  getAllResponseHeaders() {
    return Object.entries(this.responseHeaders).map(([k, v]) => `${k}: ${v}`).join('\r\n');
  }
  send() {}
  abort() { this.aborted = true; }
  complete(status, data = '', headers = {}) {
    now += 40;
    this.responseHeaders = Object.fromEntries(Object.entries(headers).map(([k, v]) => [k.toLowerCase(), String(v)]));
    this.status = status;
    this.statusText = status === 200 ? 'OK' : 'Error';
    this.responseURL = this.url;
    this.responseText = data;
    this.response = data;
    this.readyState = 4;
    this.onreadystatechange?.({ currentTarget: this });
  }
}
globalThis.XMLHttpRequest = OfflineXHR;

const Hls = require(bundlePath);

const MASTER = '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080\nvariant.m3u8?auth=x\n';

/** 치지직형 라이브 미디어 재생목록: 선언 TARGETDURATION 과 실제 세그먼트 길이가 다르다. */
function livePlaylist({ targetDuration = 10, segment = 2, count = 15, sequence = 100, extra = '' } = {}) {
  let text = `#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:${targetDuration}\n#EXT-X-MEDIA-SEQUENCE:${sequence}\n${extra}`;
  for (let i = 0; i < count; i++) text += `#EXTINF:${segment.toFixed(3)},\nseg${sequence + i}.m4v\n`;
  return text;
}

/** 마스터를 싣고 첫 레벨 재생목록 요청을 띄운 Hls. */
function loadedHls(config = {}) {
  const hls = new Hls({ autoStartLoad: false, ...config });
  const errors = [];
  const loaded = [];
  hls.on(Hls.Events.ERROR, (_, data) => errors.push(data));
  hls.on(Hls.Events.LEVEL_LOADED, (_, data) => loaded.push(data));
  hls.loadSource('https://example.invalid/master.m3u8');
  xhrs.at(-1).complete(200, MASTER);
  return { hls, errors, loaded };
}

/** 레벨 0 재생목록을 요청하고 응답한다. 반환: LEVEL_LOADED 의 details. */
function loadLevel(f, body, { url = 'https://example.invalid/variant.m3u8?auth=x', deliveryDirectives = null } = {}) {
  const level = f.hls.levels[0];
  f.hls.trigger(Hls.Events.LEVEL_LOADING, { id: 0, level: 0, pathwayId: '', levelInfo: level, url, deliveryDirectives });
  xhrs.at(-1).complete(200, body);
  const details = f.loaded.at(-1)?.details;
  if (details) f.hls.streamController.getLevelDetails = () => details;
  return details;
}

module.exports = { Hls, xhrs, livePlaylist, loadedHls, loadLevel, bundlePath, advance: (ms) => { now += ms; } };
