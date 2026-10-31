// 치지직 프리롤 광고 — 치트키가 없는 시청자에게 라이브·다시보기 재생 전에 치지직 웹과 같은 광고를 튼다.
// 볼지 말지와 광고 후보는 Rust(`ad_preroll`)가 치지직·네이버 GFP 에서 정한다. 여기서는 표준 VAST 를 해석해
// (래퍼는 최대 5단) MP4 를 재생하고, VAST 추적(노출·사분위·완료·건너뛰기·클릭)과 GFP 추적을 보낸다.
// 광고를 받지 못하거나 재생에 실패하면 조용히 넘어가 본 재생을 막지 않는다.
import { api } from './api.js';
import { esc } from './dom.js';

const PLAN_TIMEOUT_MS = 5000;
const MAX_WRAPPERS = 5;
const VIEWABLE_SEC = 2;
const MEDIA_TYPES = ['video/mp4', 'video/webm'];

let audioHooks = { hold() {}, release() {} };

/** 광고 동안 보던 방송 소리를 끄고 되살리는 훅(app.js 가 소리 주인 규칙으로 건다). */
export function setAdAudioHooks(hooks) {
  audioHooks = hooks;
}

/** 광고 계획 요청 — 오래 걸리면 광고 없이 넘어간다. */
function fetchPlan(kind, id) {
  return Promise.race([
    api.adPreroll(kind, String(id)).catch(() => null),
    new Promise((resolve) => setTimeout(() => resolve(null), PLAN_TIMEOUT_MS)),
  ]);
}

function track(urls) {
  const list = (urls || []).filter(Boolean);
  if (list.length) api.adTrack(list).catch(() => {});
}

function withMacros(url, errorCode) {
  const cache = String(Math.floor(Math.random() * 1e8)).padStart(8, '0');
  return url
    .replace(/\[ERRORCODE\]/g, errorCode ? String(errorCode) : '')
    .replace(/\[CACHEBUSTING\]/g, cache)
    .replace(/\[TIMESTAMP\]/g, encodeURIComponent(new Date().toISOString()));
}

function texts(root, tag) {
  return [...root.getElementsByTagName(tag)].map((n) => n.textContent.trim()).filter(Boolean);
}

/** "00:00:15.000" 또는 "25%" → 초(퍼센트는 길이 필요). */
function parseOffset(value, duration) {
  if (!value) return null;
  const v = value.trim();
  if (v.endsWith('%')) return duration > 0 ? (parseFloat(v) / 100) * duration : null;
  const parts = v.split(':').map(Number);
  if (parts.length !== 3 || parts.some((n) => !Number.isFinite(n))) return null;
  return parts[0] * 3600 + parts[1] * 60 + parts[2];
}

/** 720p 에 가장 가까운 진행형 MP4(VPAID 제외). */
function pickMedia(linear) {
  const files = [...linear.getElementsByTagName('MediaFile')]
    .filter((m) => MEDIA_TYPES.includes((m.getAttribute('type') || '').toLowerCase()))
    .filter((m) => !m.getAttribute('apiFramework'))
    .filter((m) => (m.getAttribute('delivery') || 'progressive') === 'progressive')
    .filter((m) => /^https:\/\//.test(m.textContent.trim()));
  if (!files.length) return null;
  const score = (m) => Math.abs((Number(m.getAttribute('height')) || 720) - 720);
  files.sort((a, b) => score(a) - score(b));
  return files[0].textContent.trim();
}

/**
 * 후보 하나의 VAST 를 끝까지 풀어 재생 정보를 만든다. 래퍼 단계의 추적도 모은다.
 * 실패하면 { error, errors } 를 던진다 — errors 는 지금까지 모인 VAST Error URL.
 */
async function resolveVast(candidate) {
  const acc = { impressions: [], errors: [], tracking: {}, clickTracking: [] };
  let xml = candidate.vastXml || null;
  let url = candidate.vastUrl || null;
  for (let depth = 0; depth <= MAX_WRAPPERS; depth++) {
    if (!xml) {
      if (!url) throw { error: 303, errors: acc.errors };
      xml = await api.adFetchVast(url).catch(() => { throw { error: 301, errors: acc.errors }; });
    }
    const doc = new DOMParser().parseFromString(xml, 'text/xml');
    if (doc.getElementsByTagName('parsererror').length) throw { error: 100, errors: acc.errors };
    const vast = doc.documentElement;
    const ad = vast.getElementsByTagName('Ad')[0];
    if (!ad) {
      acc.errors.push(...texts(vast, 'Error'));
      throw { error: 303, errors: acc.errors };
    }
    acc.impressions.push(...texts(ad, 'Impression'));
    acc.errors.push(...texts(ad, 'Error'));
    acc.clickTracking.push(...texts(ad, 'ClickTracking'));
    const linear = ad.getElementsByTagName('Linear')[0];
    if (linear) {
      for (const t of linear.getElementsByTagName('Tracking')) {
        const event = t.getAttribute('event');
        const value = t.textContent.trim();
        if (!event || !value) continue;
        (acc.tracking[event] ||= []).push({ url: value, offset: t.getAttribute('offset') });
      }
    }
    const wrapper = ad.getElementsByTagName('Wrapper')[0];
    if (wrapper) {
      url = texts(wrapper, 'VASTAdTagURI')[0] || null;
      xml = null;
      continue;
    }
    if (!linear) throw { error: 201, errors: acc.errors };
    const media = pickMedia(linear);
    if (!media) throw { error: 403, errors: acc.errors };
    const duration = parseOffset(texts(linear, 'Duration')[0], 0) || 0;
    return {
      ...acc,
      media,
      duration,
      skipOffset: parseOffset(linear.getAttribute('skipoffset'), duration),
      clickThrough: texts(linear, 'ClickThrough')[0] || null,
    };
  }
  throw { error: 302, errors: acc.errors };
}

function formatRemain(sec) {
  const s = Math.max(0, Math.ceil(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** 광고 화면. */
class AdOverlay {
  /** host 가 있으면 그 영상 영역 안에만, 없으면(다시보기) 창 전체에 띄운다. */
  constructor(cheatKeyUrl, host = null) {
    this.root = document.createElement('div');
    this.root.className = host ? 'ad-overlay in-host' : 'ad-overlay';
    this.root.setAttribute('role', 'dialog');
    this.root.setAttribute('aria-label', '광고');
    this.root.innerHTML = `
      <div class="ad-stage">
        <video playsinline preload="auto"></video>
        <div class="ad-top">
          <span class="ad-badge">광고</span><span class="ad-remain"></span>
          <span class="ad-count" hidden></span>
        </div>
        <div class="ad-bottom">
          <button class="ad-btn ad-more" hidden>자세히 보기</button>
          <button class="ad-btn ad-unmute" hidden>소리 켜기</button>
          <a class="ad-cheat" href="${esc(cheatKeyUrl)}">치트키로 광고 없이 보기</a>
          <span class="grow"></span>
          <button class="ad-btn ad-skip" disabled>광고 건너뛰기</button>
        </div>
      </div>`;
    this.video = this.root.querySelector('video');
    this.remain = this.root.querySelector('.ad-remain');
    this.count = this.root.querySelector('.ad-count');
    this.skip = this.root.querySelector('.ad-skip');
    this.more = this.root.querySelector('.ad-more');
    this.unmute = this.root.querySelector('.ad-unmute');
    this.root.querySelector('.ad-cheat').addEventListener('click', (e) => {
      e.preventDefault();
      api.openExternal(cheatKeyUrl).catch(() => {});
    });
    (host || document.body).append(this.root);
    requestAnimationFrame(() => this.root.classList.add('shown'));
  }

  setCount(index, total) {
    this.count.hidden = total < 2;
    this.count.textContent = `${index + 1}/${total}`;
  }

  remove() {
    this.video.pause();
    this.video.removeAttribute('src');
    this.video.load();
    this.root.remove();
  }
}

/**
 * 광고 한 편 재생. 'done'(완료·건너뛰기), 'failed'(다음 후보로), 'cancelled' 중 하나로 끝난다.
 * @param {AdOverlay} ui
 */
function playOne(ui, candidate, vast, session) {
  const gfp = candidate.tracking || {};
  const video = ui.video;
  const fired = new Set();
  const fire = (event) => {
    if (fired.has(event)) return;
    fired.add(event);
    fireEach(event);
  };
  // 일시정지·재개는 여러 번 생길 수 있어 매번 보낸다.
  const fireEach = (event) => {
    track((vast.tracking[event] || []).filter((t) => !t.offset).map((t) => withMacros(t.url)));
  };
  const skipAt = vast.skipOffset ?? candidate.skipAfter ?? null;
  return new Promise((resolve) => {
    let started = false;
    let watched = 0;
    let lastTime = 0;
    let finished = false;
    const startTimer = setTimeout(() => fail(402), candidate.timeoutMs || 10000);
    const finish = (result) => {
      if (finished) return;
      finished = true;
      clearTimeout(startTimer);
      video.onplaying = video.ontimeupdate = video.onended = video.onerror = video.onvolumechange = null;
      video.onpause = null;
      ui.skip.onclick = ui.more.onclick = ui.unmute.onclick = null;
      session.cancel = null;
      resolve(result);
    };
    const fail = (code) => {
      if (finished) return;
      track(vast.errors.map((u) => withMacros(u, code)));
      track(started ? gfp.startErrors : gfp.loadErrors);
      finish('failed');
    };
    session.cancel = () => finish('cancelled');

    ui.skip.disabled = true;
    ui.skip.hidden = skipAt === null;
    ui.more.hidden = !vast.clickThrough;
    ui.unmute.hidden = true;
    video.muted = false;
    video.volume = 1;
    video.src = vast.media;
    track(gfp.ackImpressions);
    track(gfp.attached);

    video.onplaying = () => {
      if (started) { fireEach('resume'); return; }
      started = true;
      clearTimeout(startTimer);
      track(vast.impressions.map((u) => withMacros(u)));
      track(gfp.renderedImpressions);
      fire('creativeView');
      fire('start');
    };
    video.onpause = () => { if (started && !video.ended && !finished) fireEach('pause'); };
    video.ontimeupdate = () => {
      const t = video.currentTime;
      if (t > lastTime && t - lastTime < 1.5) watched += t - lastTime;
      lastTime = t;
      const d = video.duration || vast.duration || 0;
      ui.remain.textContent = d ? formatRemain(d - t) : '';
      if (watched >= VIEWABLE_SEC && !fired.has('__viewable')) {
        fired.add('__viewable');
        track(gfp.viewableImpressions);
        track(gfp.activeViewImpressions);
      }
      if (d > 0) {
        if (t >= d * 0.25) fire('firstQuartile');
        if (t >= d * 0.5) fire('midpoint');
        if (t >= d * 0.75) fire('thirdQuartile');
      }
      for (const p of vast.tracking.progress || []) {
        const at = parseOffset(p.offset, d);
        const key = `progress:${p.url}`;
        if (at !== null && t >= at && !fired.has(key)) { fired.add(key); track([withMacros(p.url)]); }
      }
      if (skipAt !== null) {
        const left = skipAt - t;
        ui.skip.disabled = left > 0;
        ui.skip.textContent = left > 0 ? `${Math.ceil(left)}초 후 건너뛰기` : '광고 건너뛰기';
      }
    };
    video.onvolumechange = () => {
      if (video.muted) { fire('mute'); fired.delete('unmute'); track(gfp.mute); } else { fire('unmute'); fired.delete('mute'); }
    };
    video.onended = () => {
      fire('complete');
      track(gfp.completions);
      finish('done');
    };
    video.onerror = () => fail(405);
    ui.skip.onclick = () => {
      if (ui.skip.disabled) return;
      fire('skip');
      track(gfp.close);
      finish('done');
    };
    ui.more.onclick = () => {
      track(vast.clickTracking.map((u) => withMacros(u)));
      track(gfp.clicks);
      video.pause();
      api.openExternal(vast.clickThrough).catch(() => {});
    };
    ui.unmute.onclick = () => { video.muted = false; ui.unmute.hidden = true; };

    video.play().catch(() => {
      // 자동 재생이 소리 때문에 막혔으면 소리 없이 틀고 켜기 단추를 보인다.
      video.muted = true;
      ui.unmute.hidden = false;
      video.play().catch(() => fail(400));
    });
  });
}

// 광고 소리를 듣는 동안 보던 방송 소리를 끈다 — 패인 광고가 겹쳐도 한 번만 끄고, 모두 끝나면 되살린다.
let audioHeld = 0;
function holdAudio(target) {
  if (audioHeld++ === 0) audioHooks.hold(target);
}
function releaseAudio(target) {
  if (--audioHeld === 0) audioHooks.release(target);
}

/** 영상 영역이 지금 화면에 보이는가(다른 메뉴로 가면 숨는다). */
function hostVisible(host) {
  return host.isConnected && host.getClientRects().length > 0 && !document.hidden;
}

// 창 전체 광고(다시보기)는 하나만, 영상 영역 광고는 영역마다 하나씩.
let active = null;
const hostSessions = new Map();

/**
 * 재생 전 프리롤. 광고가 끝나면(또는 없으면) true. 다른 광고 요청에 밀려나거나 화면을 떠나 그만두면 false.
 *
 * - host 없음(다시보기): 창 전체에 띄우고, 끝난 뒤 호출한 쪽이 재생을 시작한다.
 * - host 있음(라이브·멀티라이브): 이미 재생을 시작한 그 영상 영역 안에만 띄운다. 그동안 `player`(그 영역의
 *   플레이어 브리지)의 소리를 막아 두고, 광고가 끝나면 풀어 방송 소리가 이어진다.
 * @param {'live'|'vod'} kind
 * @param {string|number} id 라이브는 채널 ID, 다시보기는 videoNo
 * @param {{ host?: HTMLElement|null, player?: { exec(script: string): void }|null, target?: string }} [options]
 */
export async function playPreroll(kind, id, { host = null, player = null, target = 'pane' } = {}) {
  const previous = host ? hostSessions.get(host) : active;
  if (previous) previous.cancel();
  const session = { cancelled: false, cancel: null };
  const handle = {
    cancel() {
      session.cancelled = true;
      if (session.cancel) session.cancel();
    },
  };
  if (host) hostSessions.set(host, handle); else active = handle;
  // 영상 영역 광고는 광고 계획을 받는 동안에도 방송 소리를 막는다(받고 나서 막으면 잠깐 들린다).
  if (player) player.exec('setAdHold(true)');
  let ui = null;
  let watch = null;
  let held = false;
  try {
    const plan = await fetchPlan(kind, id);
    if (session.cancelled) return false;
    const slots = (plan && plan.slots) || [];
    if (!slots.length) return true;
    if (host && !host.isConnected) return false;
    ui = new AdOverlay(plan.cheatKeyUrl, host);
    holdAudio(host ? target : null);
    held = true;
    if (host) {
      // 영역이 사라지면 그만두고, 다른 메뉴로 가서 숨으면 멈췄다가 돌아오면 이어 튼다.
      let pausedHidden = false;
      watch = setInterval(() => {
        if (!host.isConnected) { handle.cancel(); return; }
        const visible = hostVisible(host);
        if (!visible && !ui.video.paused) { pausedHidden = true; ui.video.pause(); }
        else if (visible && pausedHidden) { pausedHidden = false; ui.video.play().catch(() => {}); }
      }, 500);
    }
    for (let i = 0; i < slots.length; i++) {
      ui.setCount(i, slots.length);
      for (const candidate of slots[i]) {
        if (session.cancelled) return false;
        let vast;
        try {
          vast = await resolveVast(candidate);
        } catch (e) {
          track(((e && e.errors) || []).map((u) => withMacros(u, e.error)));
          track(candidate.tracking && candidate.tracking.loadErrors);
          continue;
        }
        if (session.cancelled) return false;
        const result = await playOne(ui, candidate, vast, session);
        if (result === 'cancelled') return false;
        if (result === 'done') break;
      }
    }
    return !session.cancelled;
  } finally {
    if (watch) clearInterval(watch);
    if (ui) ui.remove();
    if (player) player.exec('setAdHold(false)');
    if (held) releaseAudio(host ? target : null);
    if (host) { if (hostSessions.get(host) === handle) hostSessions.delete(host); } else if (active === handle) active = null;
  }
}

/** 보고 있던 광고를 그만둔다 — host 를 주면 그 영상 영역의 광고, 없으면 창 전체 광고(다시보기 화면을 떠날 때). */
export function cancelPreroll(host = null) {
  const handle = host ? hostSessions.get(host) : active;
  if (handle) handle.cancel();
}
