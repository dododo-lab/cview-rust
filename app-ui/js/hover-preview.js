// 썸네일 호버 미리보기 — 재생 썸네일 위에 마우스를 잠시 올려 두면 그 자리에서 소리 없이 영상을 보여 준다.
//
// 카드 템플릿이 썸네일 자리에 `data-preview="종류:아이디"`(live·vod·clip)를 붙이고, 여기서 문서 전체의 포인터
// 이벤트를 위임으로 받는다. 규칙:
// - 지연: 포인터가 DELAY_MS 머물러야 시작한다 — 목록을 훑고 지나가는 동안은 아무것도 받지 않는다.
// - 하나만: 미리보기는 늘 하나다. 새 썸네일로 옮기면 앞의 것을 먼저 정리한다.
// - 음소거·인라인·컨트롤 없음. 첫 장이 나오면(playing) 썸네일 위로 페이드인 — 그 전에는 썸네일이 그대로다.
// - 끔: 설정 "썸네일 미리보기"를 껐을 때, 설정 모션 "절약", 데이터 절약, 터치 포인터.
//   시스템 "동작 줄이기"는 따르지 않는다 — Windows 는 "애니메이션 효과"를 끈 PC(원격·저사양에서 흔하다)가
//   모두 동작 줄이기로 잡혀 미리보기가 통째로 안 나왔다. 미리보기는 사용자가 직접 머물러야 나오고 앱에
//   끄는 설정이 따로 있다.
// - 정리: 포인터가 나가면, 누르면(재생 화면으로 넘어감), 스크롤·창 가림·화면 이동·카드가 다시 그려져 떨어지면,
//   그리고 MAX_PLAY_MS 가 지나면. 정리할 때 src 를 비우고 load() 해 디코더·연결을 바로 놓는다.
// - 주소는 Rust `preview_source` 가 고른다(라이브 표준 HLS, 다시보기·클립은 낮은 화질 MP4). 결과는 잠시
//   기억하고(실패도), 못 하는 카드는 다시 묻지 않는다.
import { api } from './api.js';
import { esc } from './dom.js';

const DELAY_MS = 550;
const MAX_PLAY_MS = { live: 30000, vod: 20000, clip: 30000 };
const FADE_MS = 200;
const TTL_MS = { live: 60000, vod: 600000, clip: 600000 };
const FAIL_TTL_MS = 300000;
// 목록 화면만 — 재생 중인 화면(다시보기·클립 독, 멀티라이브 추가 서랍)에선 하지 않는다.
const PAGES = new Set(['home', 'following', 'popular', 'categories', 'library', 'search', 'channel']);
const PREF_KEY = 'cview.thumbPreview';
const HLS_TYPE = 'application/vnd.apple.mpegurl';

/** 설정 — 기본 켬. */
export function thumbPreviewEnabled() {
  try { return localStorage.getItem(PREF_KEY) !== '0'; } catch { return true; }
}

export function setThumbPreviewEnabled(on) {
  try { localStorage.setItem(PREF_KEY, on ? '1' : '0'); } catch { /* 이번 실행만 */ }
  if (!on) controller.stop();
}

/** 카드 템플릿용 속성 문자열 — 아이디가 없으면 빈 문자열. */
export function previewAttr(kind, id) {
  return id === undefined || id === null || id === '' ? '' : ` data-preview="${esc(`${kind}:${id}`)}"`;
}

/** `live:abc` → { kind, id }. 순수 함수. */
export function parsePreview(value) {
  const s = String(value || '');
  const i = s.indexOf(':');
  if (i <= 0 || i === s.length - 1) return null;
  const kind = s.slice(0, i);
  return MAX_PLAY_MS[kind] ? { kind, id: s.slice(i + 1) } : null;
}

/** 지금 미리보기를 해도 되는가. */
export function previewAllowed() {
  if (!thumbPreviewEnabled() || document.documentElement.dataset.motion === 'reduced') return false;
  if (navigator.connection && navigator.connection.saveData) return false;
  return !document.hidden;
}

function hostAllowed(host) {
  const page = host.closest('[data-page]');
  return Boolean(page && PAGES.has(page.dataset.page) && !page.hidden && host.offsetParent !== null);
}

// hls.js — HLS 를 직접 못 트는 WebView(Windows WebView2)에서만, 처음 필요할 때 한 번 불러온다.
let hlsLoading = null;
function loadHls() {
  if (window.Hls) return Promise.resolve(window.Hls);
  if (!hlsLoading) {
    hlsLoading = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = 'player/hls.min.js';
      s.onload = () => (window.Hls ? resolve(window.Hls) : reject(new Error('hls.js 없음')));
      s.onerror = () => { hlsLoading = null; reject(new Error('hls.js 불러오기 실패')); };
      document.head.append(s);
    });
  }
  return hlsLoading;
}

class HoverPreview {
  constructor() {
    this.host = null; // 지금 미리보기(또는 대기) 중인 썸네일 자리
    this.rest = null; // 끝난(시간 초과·실패·누름) 자리 — 포인터가 나갔다 들어오기 전엔 다시 시작하지 않는다
    this.seq = 0;
    this.delay = 0;
    this.cap = 0;
    this.watch = 0;
    this.video = null;
    this.hls = null;
    this.cache = new Map(); // "kind:id" → { at, value, promise }
  }

  install(root = document) {
    root.addEventListener('pointerover', (e) => this.onOver(e));
    root.addEventListener('pointerout', (e) => this.onOut(e));
    // 스크롤이 끝난 뒤 포인터가 같은 카드 위에 남아 있으면 움직임으로 다시 시작한다(pointerover 는 다시 안 온다).
    root.addEventListener('pointermove', (e) => { if (!this.host) this.onOver(e); }, { passive: true });
    // 누르면 곧 재생 화면으로 넘어간다 — 미리보기 연결을 먼저 놓는다.
    root.addEventListener('pointerdown', () => this.stop(), true);
    root.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') this.stop(); }, true);
    root.addEventListener('scroll', () => this.stop({ rest: false }), { capture: true, passive: true });
    window.addEventListener('blur', () => this.stop());
    document.addEventListener('visibilitychange', () => { if (document.hidden) this.stop(); });
  }

  onOver(e) {
    if (e.pointerType === 'touch') return;
    const host = e.target instanceof Element ? e.target.closest('[data-preview]') : null;
    if (!host || host === this.host || host === this.rest) return;
    this.stop({ rest: false });
    if (!previewAllowed() || !hostAllowed(host)) return;
    const target = parsePreview(host.dataset.preview);
    if (!target) return;
    this.host = host;
    const seq = ++this.seq;
    this.delay = setTimeout(() => this.start(host, target, seq), DELAY_MS);
  }

  onOut(e) {
    const to = e.relatedTarget;
    const inside = (el) => to instanceof Node && el.contains(to);
    if (this.rest && !inside(this.rest)) this.rest = null;
    if (!this.host || inside(this.host)) return;
    this.stop({ rest: false });
  }

  source(kind, id) {
    const key = `${kind}:${id}`;
    const hit = this.cache.get(key);
    const now = Date.now();
    if (hit) {
      if (hit.promise) return hit.promise;
      if (now - hit.at < (hit.value ? TTL_MS[kind] : FAIL_TTL_MS)) return Promise.resolve(hit.value);
    }
    const promise = api.previewSource(kind, id)
      .catch(() => null)
      .then((value) => {
        this.cache.set(key, { at: Date.now(), value: value || null });
        if (this.cache.size > 300) this.cache.delete(this.cache.keys().next().value);
        return value || null;
      });
    this.cache.set(key, { at: now, value: null, promise });
    return promise;
  }

  /** 재생 실패 — 이 카드는 한동안 다시 시도하지 않는다. */
  forget(kind, id) {
    this.cache.set(`${kind}:${id}`, { at: Date.now(), value: null });
  }

  async start(host, { kind, id }, seq) {
    host.classList.add('preview-loading');
    const src = await this.source(kind, id);
    if (seq !== this.seq) return;
    if (!src || !src.url || !host.isConnected || !previewAllowed()) { this.stop(); return; }

    const v = document.createElement('video');
    v.className = 'thumb-preview';
    v.muted = true;
    v.defaultMuted = true;
    v.playsInline = true;
    v.autoplay = true;
    v.loop = kind === 'clip';
    v.preload = 'auto';
    v.disablePictureInPicture = true;
    v.setAttribute('muted', '');
    v.setAttribute('playsinline', '');
    v.setAttribute('aria-hidden', 'true');
    v.tabIndex = -1;
    v.addEventListener('playing', () => {
      if (seq !== this.seq) return;
      host.classList.remove('preview-loading');
      host.classList.add('preview-on');
    }, { once: true });
    v.addEventListener('error', () => {
      if (seq !== this.seq) return;
      this.forget(kind, id);
      this.stop();
    });
    if (src.startAt > 0) {
      v.addEventListener('loadedmetadata', () => {
        try { if (Number.isFinite(v.duration) && src.startAt < v.duration) v.currentTime = src.startAt; } catch { /* 탐색 불가 — 처음부터 */ }
      }, { once: true });
    }
    // 배지(z-index 2)보다 아래, 썸네일 그림 바로 위.
    const img = host.querySelector(':scope > img');
    if (img) img.after(v); else host.prepend(v);
    this.video = v;

    if (src.format === 'hls' && !v.canPlayType(HLS_TYPE)) {
      try {
        const Hls = await loadHls();
        if (seq !== this.seq) return;
        if (!Hls.isSupported()) throw new Error('MSE 없음');
        const hls = new Hls({ startLevel: 0, capLevelToPlayerSize: true, maxBufferLength: 8, backBufferLength: 0, lowLatencyMode: false });
        this.hls = hls;
        hls.on(Hls.Events.ERROR, (_, data) => {
          if (data && data.fatal && seq === this.seq) { this.forget(kind, id); this.stop(); }
        });
        hls.loadSource(src.url);
        hls.attachMedia(v);
      } catch {
        if (seq === this.seq) { this.forget(kind, id); this.stop(); }
        return;
      }
    } else {
      v.src = src.url;
    }
    v.play().catch(() => {});

    this.cap = setTimeout(() => this.stop(), MAX_PLAY_MS[kind]);
    // 목록 자동 새로고침·화면 이동으로 카드가 떨어지거나 가려지면 정리한다.
    this.watch = setInterval(() => {
      if (!host.isConnected || !hostAllowed(host)) this.stop();
    }, 1000);
  }

  /** 정리. `rest` 면 이 자리는 포인터가 나갈 때까지 다시 시작하지 않는다. */
  stop({ rest = true } = {}) {
    clearTimeout(this.delay);
    clearTimeout(this.cap);
    clearInterval(this.watch);
    this.seq += 1;
    const host = this.host;
    const v = this.video;
    const hls = this.hls;
    this.host = null;
    if (host) this.rest = rest ? host : null;
    this.video = null;
    this.hls = null;
    if (host) host.classList.remove('preview-loading', 'preview-on');
    if (!v) return;
    try { v.pause(); } catch { /* 이미 떨어짐 */ }
    // 페이드아웃이 끝난 뒤 연결·디코더를 놓는다(먼저 비우면 검은 화면이 번쩍인다).
    setTimeout(() => {
      if (hls) { try { hls.destroy(); } catch { /* 무시 */ } }
      v.removeAttribute('src');
      try { v.load(); } catch { /* 무시 */ }
      v.remove();
    }, FADE_MS);
  }
}

const controller = new HoverPreview();

export function installHoverPreview(root = document) {
  controller.install(root);
  // 설정에서 모션을 "절약"으로 바꾸면 하던 미리보기도 멈춘다.
  window.addEventListener('cview:settings-changed', () => { if (!previewAllowed()) controller.stop(); });
}

export function stopHoverPreview() {
  controller.stop();
}
