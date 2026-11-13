// 플레이어 고급 패널(⚙) — 원본 PlayerAdvancedSettingsView / MLEqualizerTab / MLVideoTab.
// PLY-04 재생 속도, PLY-05 고급 패널(재생·화면·이퀄라이저·도구 탭), PLY-06 이퀄라이저(프리셋·프리앰프·10밴드),
// PLY-07 색 보정(밝기·대비·채도·색조·감마)·선명도(예전 선명한 화면 + 업스케일, sharpness.js)·자동 밝기·색 보정, PLY-08 화면 비율 강제,
// PLY-10 녹화 시작·중지(MediaRecorder 조각 → Rust 파일). 값은 플레이어 설정에 저장하고 새 재생마다 다시 건다.
import { api } from './api.js';
import { esc, toast, hideWithExit } from './dom.js';
import { kbd } from './platform.js';
import {
  SHARPNESS_LEVELS, SHARPNESS_HELP, ENLARGED_ONLY_LABEL, ENLARGED_ONLY_HELP,
  sharpnessState, sharpnessReady, sharpnessArgs, setSharpness, sharpnessNote,
} from './sharpness.js';
import { RATES, RATE_NOTE, EQ_SUPPORTED, EQ_FREQS, freqLabel, loadEqPresets, loadPlayerSettings, savePlayerSettings, applyPlayerTools, eqOnPatch } from './player-tools.js';

// 다른 모듈·검사가 예전처럼 여기서 가져갈 수 있게 다시 내보낸다.
export { EQ_SUPPORTED, freqLabel };
const ADJUST = [
  ['videoBrightness', '밝기', 0.5, 1.5, 0.01, 1],
  ['videoContrast', '대비', 0.5, 1.5, 0.01, 1],
  ['videoSaturation', '채도', 0, 2, 0.01, 1],
  ['videoHue', '색조', -180, 180, 1, 0],
  ['videoGamma', '감마', 0.5, 1.5, 0.01, 1],
];

export class PlayerPanel {
  /** @param {import('./viewer.js').Viewer} viewer */
  constructor(viewer) {
    this.viewer = viewer;
    this.player = viewer.player;
    this.tab = 'play';
    this.settings = null;
    this.freqs = EQ_FREQS;
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
      if (!this.el.hidden && !this.el.contains(e.target) && !e.target.closest('#ctl-panel')) hideWithExit(this.el);
    });
    document.addEventListener('keydown', (e) => {
      // 숨긴 시청 화면(다른 메뉴로 옮겨 재생만 이어지는 중)에서는 녹화를 켜고 끄지 않는다 — 보이지 않는 곳에서 녹화가 시작됐다.
      if (e.metaKey && e.altKey && e.key.toLowerCase() === 'r' && viewer.isPlaying && !viewer.root.hidden) { e.preventDefault(); this.toggleRecording(); }
    });
    this.player.onMessage((type, data) => this.onPlayerMessage(type, data));
    this.sharpMsg = null;
    // 선명도는 전역 — 다른 화면(멀티라이브 ⚙·설정)에서 바꿔도 이 시청 화면에 바로 다시 건다.
    window.addEventListener('cview:sharpness', () => {
      this.applySharpness();
      this.syncSharpness();
    });
    loadEqPresets().then(({ freqs, presets }) => { this.freqs = freqs; this.presets = presets; });
  }

  async loadSettings() {
    this.settings = await loadPlayerSettings();
    return this.settings;
  }

  /**
   * 새 재생이 시작되면 저장된 보정·이퀄라이저·비율을 다시 건다.
   * 비율·자동 밝기·색 보정·픽셀 확대·이퀄라이저는 라이브 단독 보기 패인과 같은 함수(player-tools.js)로 건다.
   * 선명도(sharpness.js)는 확대(업스케일)를 허용해 건다 — 시청 화면은 늘 한 화면을 크게 본다.
   */
  async applySaved(cached = null) {
    const p = cached || await this.loadSettings();
    this.player.call('applyVideoAdjust', {
      enabled: Boolean(p.videoAdjustEnabled), brightness: p.videoBrightness, contrast: p.videoContrast,
      saturation: p.videoSaturation, hue: p.videoHue, gamma: p.videoGamma,
    });
    applyPlayerTools(this.player, p);
    await sharpnessReady();
    this.applySharpness();
    if (this.rate !== 1) this.player.call('setRate', this.rate);
  }

  /** 이 시청 화면 플레이어에 선명도를 건다(확대 허용). */
  applySharpness() {
    this.player.call('setSharpness', ...sharpnessArgs(true));
  }

  /** 선명도 묶음만 제자리에서 맞춘다 — 통째로 다시 그리면 키보드로 누른 단추의 초점이 사라진다. */
  syncSharpness() {
    if (this.el.hidden || this.tab !== 'video') return;
    const s = sharpnessState();
    for (const b of this.el.querySelectorAll('.pp-sharp [data-sharpness]')) {
      const on = Number(b.dataset.sharpness) === s.level;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
    const chk = this.el.querySelector('[data-pp="sharpnessEnlarged"]');
    if (chk) {
      chk.checked = s.enlargedOnly;
      chk.disabled = !s.level;
      chk.closest('.pp-row')?.classList.toggle('disabled', !s.level);
    }
    const note = this.el.querySelector('.pp-sharpness-note');
    if (note) note.textContent = sharpnessNote(this.sharpMsg);
  }

  /** '화면' 탭의 선명도 묶음 — 멀티라이브 ⚙ 와 같은 이름·같은 값. */
  sharpnessHtml() {
    const s = sharpnessState();
    return `
        <div class="pp-row"><span title="${esc(SHARPNESS_HELP)}">선명도</span><div class="segmented pp-sharp" role="group" aria-label="선명도">${SHARPNESS_LEVELS.map((l) => `<button class="seg${l.value === s.level ? ' active' : ''}" data-sharpness="${l.value}" aria-pressed="${l.value === s.level}" title="${esc(l.help)}">${l.label}</button>`).join('')}</div></div>
        <p class="pp-note pp-sharpness-note">${esc(sharpnessNote(this.sharpMsg))}</p>
        <label class="pp-row${s.level ? '' : ' disabled'}" title="${esc(ENLARGED_ONLY_HELP)}"><span>${ENLARGED_ONLY_LABEL}</span><span class="toggle"><input type="checkbox" data-pp="sharpnessEnlarged" ${s.enlargedOnly ? 'checked' : ''} ${s.level ? '' : 'disabled'}><span></span></span></label>
        <p class="pp-note">선명한 화면과 업스케일을 합쳤어요 — 모든 라이브 화면에 함께 적용됩니다</p>`;
  }

  async save(patch) {
    try {
      this.settings = await savePlayerSettings(patch);
    } catch (e) { toast(e.message); }
  }

  toggle() {
    if (!this.el.hidden) { hideWithExit(this.el); return; }
    this.loadSettings().then(() => { this.render(); this.el.hidden = false; });
  }

  render() {
    const p = this.settings || {};
    const tabs = [['play', '재생'], ['video', '화면'], ['eq', '이퀄라이저'], ['tools', '도구']];
    let body = '';
    if (this.tab === 'play') {
      body = `
        <div class="pp-row"><span>재생 속도</span><div class="segmented">${RATES.map((r) => `<button class="seg${r === this.rate ? ' active' : ''}" data-rate="${r}">${r}x</button>`).join('')}</div></div>
        <p class="pp-note">${RATE_NOTE}</p>
        <label class="pp-row"><span>화면 꽉 채우기(비율 무시)</span><span class="toggle"><input type="checkbox" data-pp="aspect" ${p.aspectRatio === 'fill' ? 'checked' : ''}><span></span></span></label>`;
    } else if (this.tab === 'video') {
      body = `
        ${this.sharpnessHtml()}
        <label class="pp-row"><span>자동 밝기·색 보정</span><span class="toggle"><input type="checkbox" data-pp="autoPictureEnhancement" ${p.autoPictureEnhancement ? 'checked' : ''}><span></span></span></label>
        <label class="pp-row" title="확대할 때 픽셀을 부드럽게 하지 않습니다 — 선명도가 처리하는 동안에는 보이지 않습니다"><span>픽셀 그대로 확대</span><span class="toggle"><input type="checkbox" data-pp="sharpPixelScaling" ${p.sharpPixelScaling ? 'checked' : ''}><span></span></span></label>
        <label class="pp-row"><span>색 보정</span><span class="toggle"><input type="checkbox" data-pp="videoAdjustEnabled" ${p.videoAdjustEnabled ? 'checked' : ''}><span></span></span></label>
        ${ADJUST.map(([k, label, min, max, step, def]) => `
          <label class="pp-row slider"><span>${label}</span><input type="range" min="${min}" max="${max}" step="${step}" value="${esc(p[k] ?? def)}" data-adjust="${k}" ${p.videoAdjustEnabled ? '' : 'disabled'}><output>${esc(p[k] ?? def)}</output></label>`).join('')}
        <button class="link-btn" data-pp-reset="video">보정 초기화</button>`;
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
    const sh = e.target.closest('[data-sharpness]');
    if (sh) {
      const level = Number(sh.dataset.sharpness);
      if (level !== sharpnessState().level) setSharpness(level);
      return;
    }
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
        const patch = eqOnPatch(this.settings, this.presets);
        await this.save(patch);
        this.player.call('setEqualizerEnabled', true, patch.equalizerBands, this.settings.equalizerPreAmp || 0);
      } else {
        await this.save({ equalizerPreset: null });
        this.player.call('setEqualizerEnabled', false);
      }
      this.render();
      return;
    }
    if (key === 'sharpnessEnlarged') {
      const s = sharpnessState();
      if (s.level) setSharpness(s.level, t.checked);
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
    if (type === 'sharpness') {
      this.sharpMsg = data;
      const note = this.el.querySelector('.pp-sharpness-note');
      if (note) note.textContent = sharpnessNote(data);
      return;
    }
    if (type === 'equalizer' && data && data.ok === false) toast('이퀄라이저를 켤 수 없습니다 — 이 방송은 오디오 처리를 쓸 수 없습니다');
  }

  /** 시청 화면을 닫을 때 — 녹화 표시·상태 확인 타이머를 바로 거둔다(예전엔 2초 확인을 기다려 빨간 점이 남았다). */
  resetRecording() {
    clearInterval(this.recTimer);
    if (!this.recording) return;
    this.recording = null;
    this.setRecUi(false);
  }

  setRecUi(on) {
    this.recButton.classList.toggle('recording', on);
    this.recButton.title = kbd(on ? '녹화 중지 (⌥⌘R)' : '녹화 시작 (⌥⌘R)');
    if (!this.el.hidden && this.tab === 'tools') this.render();
  }
}
