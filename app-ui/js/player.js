// hls.js 플레이어 브리지 — 원본 `HLSJSVideoView` 의 WebView 쪽 역할.
//
// - 플레이어 페이지(player/player.html, 원본 hlsjs-player.html)는 같은-출처 iframe 이다.
// - 네이티브 → JS: Rust 엔진이 `player://command` 로 보낸 스크립트(`loadSource(...)` 등,
//   cview-player `hlsjs::JsCommand::script`)를 iframe 전역에서 실행한다. 페이지가 `ready` 를
//   보내기 전 명령은 줄 세웠다가 순서대로 실행한다(Swift `pendingSource`).
// - JS → 네이티브: 페이지의 `postToSwift(type, data)` 가 부모의 `__cviewPlayerBridge` 를 부르고,
//   여기서 `{ type, data }` 를 그대로 `player_message` 로 넘긴다(메시지 이름 호환).
import { api, listen, Events } from './api.js';

// Rust 쪽이 소비하는 메시지만 IPC 로 넘긴다(진단 배치 등은 로컬에서 버린다).
const FORWARDED = new Set(['ready', 'playStarted', 'playbackState', 'metrics', 'fatalError', 'error', 'levels']);

export class PlayerBridge {
  constructor(iframe) {
    this.iframe = iframe;
    this.ready = false;
    this.queue = [];
    this.listeners = new Set();
    window.__cviewPlayerBridge = (message) => this.onPageMessage(message);
    iframe.addEventListener('load', () => {
      // 새 문서 — ready 를 다시 기다린다.
      this.ready = false;
    });
    listen(Events.playerCommand, (payload) => this.exec(payload && payload.script));
  }

  /** 페이지 메시지 구독 (type, data). */
  onMessage(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  onPageMessage(message) {
    if (!message || typeof message.type !== 'string') return;
    if (message.type === 'ready') {
      this.ready = true;
      this.flush();
    }
    for (const fn of this.listeners) {
      try { fn(message.type, message.data || {}); } catch (e) { console.error(e); }
    }
    if (FORWARDED.has(message.type)) {
      api.playerMessage(message).catch(() => {});
    }
  }

  exec(script) {
    if (typeof script !== 'string' || !script) return;
    if (!this.ready) {
      this.queue.push(script);
      return;
    }
    this.run(script);
  }

  flush() {
    const pending = this.queue.splice(0);
    for (const s of pending) this.run(s);
  }

  run(script) {
    const win = this.iframe.contentWindow;
    if (!win) return;
    try {
      win.eval(script);
    } catch (e) {
      console.warn('플레이어 스크립트 실패', script.slice(0, 60), e);
    }
  }

  /** 페이지 전역 함수 직접 호출 (음소거 등 UI 전용 제어). */
  call(name, ...args) {
    const win = this.iframe.contentWindow;
    if (this.ready && win && typeof win[name] === 'function') {
      try { return win[name](...args); } catch (e) { console.warn(e); }
    }
    return undefined;
  }

  isMuted() {
    const video = this.iframe.contentDocument && this.iframe.contentDocument.getElementById('video');
    return video ? video.muted : true;
  }
}
