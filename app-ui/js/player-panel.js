// 플레이어 고급 패널(⚙) — 원본 PlayerAdvancedSettingsView / MLEqualizerTab / MLVideoTab.
// PLY-04 재생 속도, PLY-05 고급 패널(재생·화면·이퀄라이저·도구 탭), PLY-06 이퀄라이저(프리셋·프리앰프·10밴드),
// PLY-07 비디오 보정(밝기·대비·채도·색조·감마)·선명한 화면·자동 보정·업스케일, PLY-08 화면 비율 강제,
// PLY-10 녹화 시작·중지(MediaRecorder 조각 → Rust 파일). 값은 플레이어 설정에 저장하고 새 재생마다 다시 건다.
import { api } from './api.js';
import { esc, toast } from './dom.js';
import { kbd } from './platform.js';

const RATES = [0.5, 0.75, 1, 1.25, 1.5, 2];
// macOS WKWebView 는 MSE 영상의 오디오를 Web Audio 그래프로 넘기지 않는다(소스 노드 출력이 0 — 실측).
// 그래프는 한 번 만들면 되돌릴 수 없어 켜는 순간 그 재생의 소리가 사라지므로 여기서는 막는다.
export const EQ_SUPPORTED = !/Mac OS X|Macintosh/.test(navigator.userAgent);
const ADJUST = [
  ['videoBrightness', '밝기', 0.5, 1.5, 0.01, 1],
  ['videoContrast', '대비', 0.5, 1.5, 0.01, 1],
  ['videoSaturation', '채도', 0, 2, 0.01, 1],
  ['videoHue', '색조', -180, 180, 1, 0],
  ['videoGamma', '감마', 0.5, 1.5, 0.01, 1],
];

/** 10밴드 주파수 표시 — 31 → "31", 1000 → "1k". */
export function freqLabel(hz) {
  return hz >= 1000 ? `${hz / 1000}k` : String(hz);
}

export class PlayerPanel {
  /** @param {import('./viewer.js').Viewer} viewer */
  constructor(viewer) {
    this.viewer = viewer;
    this.player = viewer.player;
    this.tab = 'play';
    this.settings = null;
    this.freqs = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
    this.presets = [];
    this.recording = null; // { id, bytes }
    this.rate = 1;

    const reload = viewer.root.querySelector('#reload');
    reload.insertAdjacentHTML('beforebegin', `
      <button class="icon-btn rec-btn" id="ctl-record" title="${kbd('녹화 시작 (⌥⌘R)')}" aria-label="녹화"><span class="rec-dot"></span></button>
      <button class="icon-btn" id="ctl-panel" title="플레이어 설정" aria-label="플레이어 설정"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/></svg></button>`);
    this.recButton = viewer.root.querySelector('#ctl-record');
    this.el = document.createElement('div');
    this.el.className = 'player-panel';
    this.el.hidden = true;
    viewer.root.querySelector('.player-stage').append(this.el);

    viewer.root.querySelector('#ctl-panel').addEventListener('click', (e) => { e.stopPropagation(); this.toggle(); });
    this.recButton.addEventListener('click', () => this.toggleRecording());
    this.el.addEventListener('click', (e) => this.onClick(e));
    this.el.addEventListener('input', (e) => this.onInput(e));
    this.el.addEventListener('change', (e) => this.onChange(e));
    document.addEventListener('mousedown', (e) => {
      if (!this.el.hidden && !this.el.contains(e.target) && !e.target.closest('#ctl-panel')) this.el.hidden = true;
    });
    document.addEventListener('keydown', (e) => {
      if (e.metaKey && e.altKey && e.key.toLowerCase() === 'r' && viewer.isPlaying) { e.preventDefault(); this.toggleRecording(); }
    });
    this.player.onMessage((type, data) => this.onPlayerMessage(type, data));
    api.equalizerPresets().then(([freqs, presets]) => { this.freqs = freqs; this.presets = presets; }).catch(() => {});
  }

  async loadSettings() {
    try { this.settings = (await api.settingsGet()).player; } catch { this.settings = {}; }
    return this.settings;
  }

  /** 새 재생이 시작되면 저장된 보정·이퀄라이저·비율을 다시 건다. */
  async applySaved(cached = null) {
    const p = cached || await this.loadSettings();
    this.player.call('applyVideoAdjust', {
      enabled: Boolean(p.videoAdjustEnabled), brightness: p.videoBrightness, contrast: p.videoContrast,
      saturation: p.videoSaturation, hue: p.videoHue, gamma: p.videoGamma, sharpPixel: Boolean(p.sharpPixelScaling),
    });
    this.player.call('setPictureEnhancement', { auto: Boolean(p.autoPictureEnhancement), upscale: Boolean(p.videoUpscaling) });
    this.player.call('setForcedAspectRatio', p.aspectRatio === 'fill');
    if (p.equalizerPreset && EQ_SUPPORTED) this.player.call('setEqualizerEnabled', true, p.equalizerBands || [], p.equalizerPreAmp || 0);
    if (this.rate !== 1) this.player.call('setRate', this.rate);
  }

  async save(patch) {
    try {
      this.settings = (await api.settingsUpdate('player', patch)).player;
    } catch (e) { toast(e.message); }
  }

  toggle() {
    if (!this.el.hidden) { this.el.hidden = true; return; }
    this.loadSettings().then(() => { this.render(); this.el.hidden = false; });
  }

  render() {
    const p = this.settings || {};
    const tabs = [['play', '재생'], ['video', '화면'], ['eq', '이퀄라이저'], ['tools', '도구']];
    let body = '';
    if (this.tab === 'play') {
      body = `
        <div class="pp-row"><span>재생 속도</span><div class="segmented">${RATES.map((r) => `<button class="seg${r === this.rate ? ' active' : ''}" data-rate="${r}">${r}x</button>`).join('')}</div></div>
        <p class="pp-note">라이브는 1배속이 기본이에요. 바꾸면 지연이 늘거나 줄 수 있고, LIVE 를 누르면 1배속으로 돌아갑니다.</p>
        <label class="pp-row"><span>화면 꽉 채우기(비율 무시)</span><span class="toggle"><input type="checkbox" data-pp="aspect" ${p.aspectRatio === 'fill' ? 'checked' : ''}><span></span></span></label>`;
    } else if (this.tab === 'video') {
      body = `
        <label class="pp-row"><span>비디오 보정</span><span class="toggle"><input type="checkbox" data-pp="videoAdjustEnabled" ${p.videoAdjustEnabled ? 'checked' : ''}><span></span></span></label>
        ${ADJUST.map(([k, label, min, max, step, def]) => `
          <label class="pp-row slider"><span>${label}</span><input type="range" min="${min}" max="${max}" step="${step}" value="${esc(p[k] ?? def)}" data-adjust="${k}" ${p.videoAdjustEnabled ? '' : 'disabled'}><output>${esc(p[k] ?? def)}</output></label>`).join('')}
        <button class="link-btn" data-pp-reset="video">보정 초기화</button>
        <label class="pp-row"><span>선명한 화면</span><span class="toggle"><input type="checkbox" data-pp="sharpPixelScaling" ${p.sharpPixelScaling ? 'checked' : ''}><span></span></span></label>
        <label class="pp-row"><span>자동 화질 보정</span><span class="toggle"><input type="checkbox" data-pp="autoPictureEnhancement" ${p.autoPictureEnhancement ? 'checked' : ''}><span></span></span></label>
        <label class="pp-row"><span>업스케일</span><span class="toggle"><input type="checkbox" data-pp="videoUpscaling" ${p.videoUpscaling ? 'checked' : ''}><span></span></span></label>`;
    } else if (this.tab === 'eq') {
      const bands = p.equalizerBands && p.equalizerBands.length === 10 ? p.equalizerBands : new Array(10).fill(0);
      const on = Boolean(p.equalizerPreset) && EQ_SUPPORTED;
      body = `
        ${EQ_SUPPORTED ? '' : '<p class="pp-note warn">macOS 웹뷰는 라이브 영상의 오디오를 이퀄라이저로 넘기지 않아 지금은 쓸 수 없습니다. 켜면 소리가 끊기기 때문에 막아 두었어요.</p>'}
        <label class="pp-row${EQ_SUPPORTED ? '' : ' disabled'}"><span>이퀄라이저</span><span class="toggle"><input type="checkbox" data-pp="eq" ${on ? 'checked' : ''} ${EQ_SUPPORTED ? '' : 'disabled'}><span></span></span></label>
        <div class="pp-row"><span>프리셋</span><select class="select" data-eq-preset ${on ? '' : 'disabled'}>
          ${this.presets.map((pr) => `<option value="${esc(pr.name)}" ${pr.name === p.equalizerPreset ? 'selected' : ''}>${esc(pr.name)}</option>`).join('')}
          ${p.equalizerPreset && !this.presets.some((pr) => pr.name === p.equalizerPreset) ? `<option selected>${esc(p.equalizerPreset)}</option>` : ''}
        </select></div>
        <label class="pp-row slider"><span>프리앰프</span><input type="range" min="-12" max="12" step="0.5" value="${esc(p.equalizerPreAmp || 0)}" data-eq-preamp ${on ? '' : 'disabled'}><output>${esc(p.equalizerPreAmp || 0)}dB</output></label>
        <div class="eq-bands">${bands.map((g, i) => `
          <label class="eq-band"><output>${g > 0 ? '+' : ''}${g}</output><input type="range" min="-12" max="12" step="0.5" value="${g}" data-eq-band="${i}" ${on ? '' : 'disabled'} orient="vertical"><span>${freqLabel(this.freqs[i])}</span></label>`).join('')}</div>`;
    } else {
      body = `
        <div class="pp-tools">
          <button class="btn" data-tool="record">${this.recording ? '■ 녹화 중지' : '● 녹화 시작'}</button>
          <button class="btn" data-tool="shot">스크린샷</button>
          <button class="btn" data-tool="bookmark">북마크</button>
          <button class="btn" data-tool="alert-pos">후원 알림 위치 조정</button>
        </div>
        <p class="pp-note">녹화는 ~/Movies/CView, 스크린샷은 설정한 폴더에 저장됩니다.</p>`;
    }
    this.el.innerHTML = `
      <div class="pp-head"><div class="segmented">${tabs.map(([id, l]) => `<button class="seg${this.tab === id ? ' active' : ''}" data-pp-tab="${id}">${l}</button>`).join('')}</div></div>
      <div class="pp-body">${body}</div>`;
  }

  onClick(e) {
    const t = e.target.closest('[data-pp-tab]');
    if (t) { this.tab = t.dataset.ppTab; this.render(); return; }
    const r = e.target.closest('[data-rate]');
    if (r) {
      this.rate = Number(r.dataset.rate);
      this.player.call('setRate', this.rate);
      this.viewer.controls.flash(`${this.rate}배속`);
      this.render();
      return;
    }
    const reset = e.target.closest('[data-pp-reset]');
    if (reset) {
      const patch = { videoBrightness: 1, videoContrast: 1, videoSaturation: 1, videoHue: 0, videoGamma: 1 };
      this.save(patch).then(() => { this.applySaved(this.settings); this.render(); });
      return;
    }
    const tool = e.target.closest('[data-tool]');
    if (tool) {
      if (tool.dataset.tool === 'record') this.toggleRecording();
      if (tool.dataset.tool === 'shot') this.viewer.controls.screenshot();
      if (tool.dataset.tool === 'bookmark') this.viewer.controls.bookmark();
      if (tool.dataset.tool === 'alert-pos') { this.el.hidden = true; this.viewer.extras.startAlertPositioning(); }
    }
  }

  onInput(e) {
    const t = e.target;
    const out = t.parentElement.querySelector('output');
    if (t.dataset.adjust) {
      if (out) out.textContent = t.value;
      const p = { ...this.settings, [t.dataset.adjust]: Number(t.value) };
      this.player.call('applyVideoAdjust', { enabled: true, brightness: p.videoBrightness, contrast: p.videoContrast, saturation: p.videoSaturation, hue: p.videoHue, gamma: p.videoGamma });
    } else if (t.dataset.eqBand !== undefined) {
      const v = Number(t.value);
      if (out) out.textContent = `${v > 0 ? '+' : ''}${v}`;
      this.player.call('setEqualizerBand', Number(t.dataset.eqBand), v);
    } else if (t.dataset.eqPreamp !== undefined) {
      if (out) out.textContent = `${t.value}dB`;
      this.player.call('setEqualizerPreAmp', Number(t.value));
    }
  }

  async onChange(e) {
    const t = e.target;
    if (t.dataset.adjust) { await this.save({ [t.dataset.adjust]: Number(t.value) }); return; }
    if (t.dataset.eqBand !== undefined) {
      const bands = [...this.el.querySelectorAll('[data-eq-band]')].map((x) => Number(x.value));
      await this.save({ equalizerBands: bands, equalizerPreset: '사용자 지정' });
      return;
    }
    if (t.dataset.eqPreamp !== undefined) { await this.save({ equalizerPreAmp: Number(t.value) }); return; }
    if (t.dataset.eqPreset !== undefined) {
      const pr = this.presets.find((x) => x.name === t.value);
      if (!pr) return;
      await this.save({ equalizerPreset: pr.name, equalizerBands: pr.gains, equalizerPreAmp: pr.preAmp });
      if (EQ_SUPPORTED) this.player.call('setEqualizerEnabled', true, pr.gains, pr.preAmp);
      this.render();
      return;
    }
    const key = t.dataset.pp;
    if (!key) return;
    if (key === 'eq') {
      if (t.checked) {
        const pr = this.presets[0] || { name: '평탄', gains: new Array(10).fill(0), preAmp: 0 };
        const bands = this.settings.equalizerBands && this.settings.equalizerBands.length === 10 ? this.settings.equalizerBands : pr.gains;
        await this.save({ equalizerPreset: this.settings.equalizerPreset || pr.name, equalizerBands: bands });
        this.player.call('setEqualizerEnabled', true, bands, this.settings.equalizerPreAmp || 0);
      } else {
        await this.save({ equalizerPreset: null });
        this.player.call('setEqualizerEnabled', false);
      }
      this.render();
      return;
    }
    if (key === 'aspect') {
      await this.save({ aspectRatio: t.checked ? 'fill' : null });
      this.player.call('setForcedAspectRatio', t.checked);
      return;
    }
    await this.save({ [key]: t.checked });
    await this.applySaved(this.settings);
    if (key === 'videoAdjustEnabled') this.render();
  }

  // ── 녹화 (PLY-10) — 원본 스트림 조각을 Rust 가 받아 쓴다(WKWebView 에는 captureStream 이 없다) ──

  async toggleRecording() {
    if (!this.viewer.isPlaying) return;
    try {
      if (this.recording) {
        const st = await api.playerRecordStop();
        this.recording = null;
        this.setRecUi(false);
        clearInterval(this.recTimer);
        if (st.path && st.bytes > 0) {
          const mb = (st.bytes / 1048576).toFixed(1);
          toast(`녹화를 저장했습니다 (${mb}MB) — ${st.path}`, 6000, { label: 'Finder에서 보기', run: () => api.revealPath(st.path) });
        } else {
          toast('녹화된 내용이 없습니다');
        }
        return;
      }
      const st = await api.playerRecordStart();
      this.recording = st;
      this.setRecUi(true);
      this.viewer.controls.flash('녹화 시작');
      clearInterval(this.recTimer);
      this.recTimer = setInterval(async () => {
        try {
          const s = await api.playerRecordStatus();
          if (!s.recording) { clearInterval(this.recTimer); this.recording = null; this.setRecUi(false); return; }
          const sec = Math.round((Date.now() - (s.startedAt || Date.now())) / 1000);
          this.recButton.title = `녹화 중 ${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, '0')} · ${(s.bytes / 1048576).toFixed(1)}MB — 눌러서 중지`;
        } catch { /* 무시 */ }
      }, 2000);
    } catch (e) {
      toast(e.message);
    }
  }

  async onPlayerMessage(type, data) {
    if (type === 'playStarted') { this.rate = 1; this.applySaved(); return; }
    if (type === 'equalizer' && data && data.ok === false) toast('이퀄라이저를 켤 수 없습니다 — 이 방송은 오디오 처리를 쓸 수 없습니다');
  }

  setRecUi(on) {
    this.recButton.classList.toggle('recording', on);
    this.recButton.title = kbd(on ? '녹화 중지 (⌥⌘R)' : '녹화 시작 (⌥⌘R)');
    if (!this.el.hidden && this.tab === 'tools') this.render();
  }
}
