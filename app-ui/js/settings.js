// 설정 — 원본 SettingsView / SettingsWorkspace: 그룹 탭(개요·시청·채팅·멀티·일반·연동·고급) + 설정 검색 (SET-01).
// 항목은 스키마로 정의하고 Rust 설정 도메인(settings_update)에 바로 저장한다.
// SET-02~06 플레이어·지연·되감기·스크린샷·비디오, SET-07 채팅, SET-08 일반, SET-09 외관,
// SET-10 성능, SET-11 네트워크(프리셋), SET-13 채널 알림, SET-15 동기화, SET-16 도메인, SET-17 백업·복원·초기화,
// MUL-16 멀티라이브 설정.
import { api, listen, Events } from './api.js';
import { ACTIONS, FIXED, currentBindings, bindingLabel, bindingFromEvent, saveBinding, conflictOf } from './shortcuts.js';
import { esc, toast } from './dom.js';
import { emptyState, errorState, loadingState } from './cards.js';
import { THEMES, THEME_LABELS, currentTheme, setTheme } from './theme.js';
import { streamAlertsEnabled, setStreamAlertsEnabled, ALERT_HOME } from './donations.js';
import { chatPrefs, updateChatPrefs, blockedLabel, loadChatPrefs } from './chat-prefs.js';
import { BACKGROUNDS, READING_WIDTHS } from './chat-extras.js';
import { kbd } from './platform.js';
import { checkForUpdate, lastUpdateResult, updateSummary, downloadTarget, openDownload, currentVersionText } from './update-check.js';

const opt = (pairs) => pairs.map(([value, label]) => ({ value, label }));

/** 스키마 — [그룹, 섹션 제목, 항목들]. 항목 type: toggle | select | range | number | text. */
export const SCHEMA = [
  ['watch', '재생', [
    { d: 'player', k: 'quality', label: '기본 화질', type: 'select', options: opt([['1080p', '1080p (원본)'], ['720p', '720p'], ['480p', '480p'], ['360p', '360p'], ['auto', '자동']]) },
    { d: 'player', k: 'autoPlay', label: '자동 재생', help: '시청 화면을 열면 바로 재생합니다', type: 'toggle' },
    { d: 'player', k: 'startMuted', label: '음소거로 시작', help: '방송을 열 때 소리를 끈 상태로 시작합니다', type: 'toggle' },
    { d: 'player', k: 'forceHighestQuality', label: '항상 최고 화질', help: '1080p60 을 우선 유지합니다', type: 'toggle' },
    { d: 'player', k: 'volumeLevel', label: '기본 볼륨', type: 'range', min: 0, max: 1, step: 0.05, pct: true },
    { d: 'player', k: 'continuePlaybackInBackground', label: '백그라운드 재생', help: '끄면 창이 가려지거나 앱이 뒤로 가면 일시정지합니다', type: 'toggle' },
    { d: 'player', k: 'streamProxyMode', label: '스트림 프록시', help: '로컬 프록시는 CDN 응답을 보정하고 암호화 방송 키를 인증합니다', type: 'select', options: opt([['localProxy', '로컬 프록시 (권장)'], ['none', '직접 재생']]) },
    { d: 'player', k: 'hardwareDecoding', label: '하드웨어 디코딩', type: 'toggle' },
  ]],
  ['watch', '지연(저지연)', [
    { d: 'player', k: 'lowLatencyMode', label: '저지연 모드', help: '라이브 엣지에 가깝게 재생합니다', type: 'toggle' },
    { d: 'player', k: 'latencyPreset', label: '지연 프리셋', type: 'select', options: opt([['matchWeb', '웹과 같게'], ['webSync', '웹 동기'], ['standard', '표준'], ['ultraLow', '초저지연'], ['custom', '직접 조정']]) },
    { d: 'player', k: 'latencyTarget', label: '목표 지연(초)', type: 'range', min: 1, max: 15, step: 0.5, adv: true },
    { d: 'player', k: 'latencyMin', label: '최소 지연(초)', type: 'range', min: 0.5, max: 10, step: 0.5, adv: true },
    { d: 'player', k: 'latencyMax', label: '최대 지연(초)', type: 'range', min: 2, max: 30, step: 0.5, adv: true },
    { d: 'player', k: 'latencyMaxRate', label: '따라잡기 최대 배속', type: 'range', min: 1, max: 1.2, step: 0.01, adv: true },
    { d: 'player', k: 'latencyMinRate', label: '늦추기 최소 배속', type: 'range', min: 0.8, max: 1, step: 0.01, adv: true },
    { d: 'player', k: 'catchupRate', label: '따라잡기 배속', type: 'range', min: 1, max: 1.2, step: 0.01, adv: true },
    { d: 'player', k: 'latencyCatchUpThreshold', label: '따라잡기 시작 차이(초)', type: 'range', min: 0.1, max: 3, step: 0.05, adv: true },
    { d: 'player', k: 'latencySlowDownThreshold', label: '늦추기 시작 차이(초)', type: 'range', min: 0.1, max: 3, step: 0.05, adv: true },
    { d: 'player', k: 'latencyPidKp', label: 'PID Kp', type: 'number', step: 0.01, adv: true },
    { d: 'player', k: 'latencyPidKi', label: 'PID Ki', type: 'number', step: 0.01, adv: true },
    { d: 'player', k: 'latencyPidKd', label: 'PID Kd', type: 'number', step: 0.01, adv: true },
    { d: 'player', k: 'bufferDuration', label: '버퍼 길이(초)', type: 'range', min: 1, max: 10, step: 0.5, adv: true },
  ]],
  ['watch', '라이브 되감기 · 실험', [
    { d: 'player', k: 'liveRewindEnabled', label: '라이브 되감기', help: '방송을 뒤로 돌려 볼 수 있게 최근 구간을 기억합니다', type: 'toggle' },
    { d: 'player', k: 'liveRewindWindowMinutes', label: '되감기 범위(분)', type: 'range', min: 5, max: 180, step: 5 },
    { d: 'player', k: 'liveRewindDiskBudgetMB', label: '되감기 디스크 한도(MB)', type: 'range', min: 512, max: 20480, step: 512 },
    { d: 'player', k: 'experimentalServerTimeMachine', label: '서버 타임머신(실험)', type: 'toggle' },
    { d: 'player', k: 'experimentalMatchWebV2', label: '웹 지연 맞춤 v2(실험)', type: 'toggle' },
  ]],
  ['watch', '화면 · 비디오 · 스크린샷', [
    { d: 'player', k: 'sharpPixelScaling', label: '픽셀 그대로 확대', help: '확대할 때 픽셀을 부드럽게 하지 않습니다(선명한 화면은 영상 ⚙ 메뉴에서 채널마다)', type: 'toggle' },
    { d: 'player', k: 'autoPictureEnhancement', label: '자동 화질 보정', type: 'toggle' },
    { d: 'player', k: 'videoUpscaling', label: '업스케일', type: 'toggle' },
    { d: 'player', k: 'videoAdjustEnabled', label: '비디오 보정 사용', help: '시청 화면의 ⚙ 패널에서 밝기·대비 등을 조절합니다', type: 'toggle' },
    { d: 'player', k: 'screenshotFormat', label: '스크린샷 형식', type: 'select', options: opt([['png', 'PNG'], ['jpeg', 'JPEG']]) },
    { d: 'player', k: 'screenshotPath', label: '스크린샷 폴더', type: 'text' },
    { local: 'streamAlerts', label: '후원·구독 알림', help: '시청 화면 위에 새 후원과 구독을 잠깐 띄웁니다', type: 'toggle' },
    { d: 'player', k: 'streamAlertAnchorX', label: '알림 가로 위치', type: 'range', min: 0.1, max: 0.9, step: 0.05, pct: true },
    { d: 'player', k: 'streamAlertAnchorY', label: '알림 세로 위치', type: 'range', min: 0.02, max: 0.9, step: 0.02, pct: true },
    { d: 'player', k: 'streamAlertScale', label: '알림 크기', type: 'range', min: 0.6, max: 1.6, step: 0.1 },
    { type: 'action', action: 'reset-alert-pos', label: '알림 위치 초기화', help: '영상 위 알림은 끌어서 옮길 수 있어요(알림을 더블클릭해도 기본 위치로 돌아갑니다)', button: '기본 위치로' },
  ]],
  ['multi', '멀티라이브', [
    { d: 'multiLive', k: 'maxConcurrentSessions', label: '최대 동시 세션', help: '원본과 같이 4~20개 (많을수록 CPU·대역폭을 더 씁니다)', type: 'range', min: 4, max: 20, step: 1 },
    { d: 'multiLive', k: 'defaultLayoutMode', label: '기본 레이아웃', type: 'select', options: opt([['preset', '균등 분할'], ['focus', '포커스 모드']]) },
    { d: 'multiLive', k: 'defaultFocusDirection', label: '포커스 메인 위치', type: 'select', options: opt([['left', '왼쪽'], ['right', '오른쪽'], ['top', '위'], ['bottom', '아래']]) },
    { d: 'multiLive', k: 'rememberLastLayout', label: '마지막 레이아웃 기억', type: 'toggle' },
    { d: 'multiLive', k: 'multiAudioEnabled', label: '여러 패인 소리', help: '선택 패인 외에도 보조 볼륨으로 소리를 냅니다', type: 'toggle' },
    { d: 'multiLive', k: 'secondaryVolume', label: '보조 패인 볼륨', type: 'range', min: 0, max: 1, step: 0.05, pct: true },
    { d: 'multiLive', k: 'bandwidthCoordinationEnabled', label: '대역폭 분배', help: '선택 패인에 대역폭을 더 줍니다', type: 'toggle' },
    { d: 'multiLive', k: 'selectedSessionBWWeight', label: '선택 패인 대역폭 가중치', type: 'range', min: 1, max: 3, step: 0.1 },
    { d: 'multiLive', k: 'levelCappingEnabled', label: '패인 크기 화질 제한', type: 'toggle' },
    { d: 'multiLive', k: 'backgroundQualityReduction', label: '보조 패인 화질 낮추기', type: 'toggle' },
    { d: 'multiLive', k: 'autoReconnect', label: '자동 재연결', type: 'toggle' },
    { d: 'multiLive', k: 'autoReconnectMaxRetries', label: '재연결 최대 횟수', type: 'range', min: 1, max: 30, step: 1 },
    { d: 'multiLive', k: 'chatOverlayInGrid', label: '그리드 채팅 오버레이', type: 'toggle' },
    { d: 'multiLive', k: 'chatOverlayOpacity', label: '채팅 오버레이 투명도', type: 'range', min: 0.1, max: 1, step: 0.05, pct: true },
    { d: 'multiLive', k: 'chatOverlayFontSize', label: '채팅 오버레이 글자 크기', type: 'range', min: 9, max: 20, step: 1 },
  ]],
  ['general', '일반', [
    { d: 'general', k: 'autoRefreshEnabled', label: '목록 자동 새로고침', type: 'toggle' },
    { d: 'general', k: 'autoRefreshInterval', label: '새로고침 주기(초)', type: 'range', min: 30, max: 600, step: 30 },
    { d: 'general', k: 'restoreWindowOnLaunch', label: '창 위치·크기 기억', type: 'toggle' },
    { d: 'general', k: 'alwaysOnTop', label: '항상 위에', type: 'toggle' },
    { d: 'general', k: 'launchAtLogin', label: '로그인 시 자동 실행', type: 'toggle' },
    { d: 'general', k: 'showInMenuBar', label: '메뉴 막대에 표시', type: 'toggle' },
  ]],
  ['general', '알림', [
    { d: 'general', k: 'notificationsEnabled', label: '알림 사용', type: 'toggle' },
    { d: 'general', k: 'notifyOnLiveStart', label: '팔로우 채널 방송 시작', type: 'toggle' },
    { d: 'general', k: 'notifyOnCategoryChange', label: '카테고리 변경', type: 'toggle' },
    { d: 'general', k: 'notifyOnTitleChange', label: '방송 제목 변경', type: 'toggle' },
  ]],
  ['general', '외관', [
    { d: 'appearance', k: 'compactMode', label: '콤팩트 모드', help: '여백과 카드를 촘촘하게 줄입니다', type: 'toggle' },
    { d: 'appearance', k: 'motionQuality', label: '모션', type: 'select', options: opt([['reduced', '절약'], ['balanced', '균형'], ['rich', '풍부']]) },
    { d: 'appearance', k: 'transparencyMode', label: '투명도', type: 'select', options: opt([['system', '시스템'], ['full', '선명하게'], ['reduced', '불투명']]) },
    { d: 'appearance', k: 'frameRateMode', label: '화면 갱신', type: 'select', options: opt([['adaptive', '적응형 (최대 120Hz)'], ['fixed60', '60Hz 고정']]) },
  ]],
  ['link', '시청 기록 동기화', [
    { d: 'sync', k: 'explicitlyOptedIn', label: '동기화 동의', help: '다른 기기와 시청 기록을 맞추려면 동의가 필요합니다', type: 'toggle' },
    { d: 'sync', k: 'watchHistorySyncEnabled', label: '시청 기록 동기화', type: 'toggle' },
    { d: 'sync', k: 'endpoint', label: '동기화 서버', type: 'text' },
  ]],
  ['advanced', '네트워크', [
    { d: 'network', k: 'requestRateLimit', label: '초당 요청 한도', type: 'range', min: 1, max: 30, step: 1 },
    { d: 'network', k: 'maxConcurrentAPIRequests', label: '동시 API 요청', type: 'range', min: 1, max: 12, step: 1 },
    { d: 'network', k: 'maxConnectionsPerHost', label: '호스트당 연결', type: 'range', min: 2, max: 20, step: 1 },
    { d: 'network', k: 'connectionTimeout', label: '연결 시간 제한(초)', type: 'range', min: 5, max: 60, step: 1 },
    { d: 'network', k: 'streamConnectionTimeout', label: '스트림 연결 시간 제한(초)', type: 'range', min: 3, max: 40, step: 1 },
    { d: 'network', k: 'apiResourceTimeout', label: 'API 응답 시간 제한(초)', type: 'range', min: 5, max: 60, step: 1 },
    { d: 'network', k: 'retryCount', label: '재시도 횟수', type: 'range', min: 0, max: 10, step: 1 },
    { d: 'network', k: 'autoReconnect', label: '자동 재연결', type: 'toggle' },
    { d: 'network', k: 'maxReconnectAttempts', label: '최대 재연결', type: 'range', min: 1, max: 40, step: 1 },
    { d: 'network', k: 'reconnectBaseDelay', label: '재연결 기본 지연(초)', type: 'range', min: 0.5, max: 5, step: 0.5 },
    { d: 'network', k: 'cacheExpiry', label: '캐시 유효 시간(초)', type: 'range', min: 30, max: 1800, step: 30 },
    { d: 'network', k: 'forceStreamProxy', label: '스트림 프록시 강제', type: 'toggle' },
    { d: 'network', k: 'pauseRefreshWhenOffline', label: '오프라인이면 새로고침 멈춤', type: 'toggle' },
    { d: 'network', k: 'reduceBackgroundActivity', label: '백그라운드 활동 줄이기', type: 'toggle' },
  ]],
  ['advanced', '성능 · 진단', [
    { d: 'appearance', k: 'maxMemoryMB', label: '메모리 목표(MB)', type: 'range', min: 256, max: 4096, step: 128 },
    { d: 'appearance', k: 'debugMode', label: '디버그 로그', help: '시작 단계·API 요청·CPU/메모리·프레임·채팅·플레이어 지표를 로그 파일에 남깁니다(쿠키·토큰은 가림). 끄면 기록하지 않습니다', type: 'toggle' },
    { type: 'action', action: 'open-logs', label: '로그 폴더', help: 'debug.jsonl — 10MB 마다 새 파일, 5개까지', button: '로그 폴더 열기' },
    { type: 'action', action: 'clear-image-cache', label: '썸네일 캐시', help: '썸네일·아바타를 디스크에 두어 다시 열 때 바로 보입니다', button: '썸네일 캐시 비우기' },
  ]],
];

const GROUPS = [
  ['overview', '개요'], ['watch', '시청'], ['chat', '채팅'], ['multi', '멀티라이브'],
  ['general', '일반'], ['link', '연동'], ['advanced', '고급'],
];
const NETWORK_PRESETS = [['auto', '자동'], ['balanced', '밸런스'], ['stability', '안정 우선'], ['lowLatency', '저지연 우선'], ['performance', '고성능']];

/** 프리셋 줄 설명 — 자동이면 지금 고른 프리셋을 적는다. */
function netPresetHelp(settings) {
  if (settings.networkPreset === 'auto') {
    const now = NETWORK_PRESETS.find(([v]) => v === settings.networkAutoChoice)?.[1];
    return `자동 — 지금 ${now || '밸런스'}. 멀티라이브 2개 이상이면 고성능, 재생 오류가 나면 잠시 안정 우선으로 바꿉니다`;
  }
  if (settings.networkPreset === 'custom') return '직접 조정한 값 — 프리셋을 고르면 요청 한도·시간 제한·재연결 값을 함께 바꿉니다';
  return '요청 한도·시간 제한·재연결 값을 함께 바꿉니다';
}

const CHAT_TOGGLES = [
  ['showTimestamp', '시간 표시', '메시지 옆에 받은 시각을 보여 줍니다'],
  ['showBadge', '배지 표시', '구독·팬 배지를 보여 줍니다'],
  ['emoticonEnabled', '이모티콘 표시', '끄면 이모티콘을 :이름: 글자로 보여 줍니다'],
  ['highlightRoles', '역할 색', '스트리머·매니저 닉네임에 색을 입힙니다'],
  ['nicknameColors', '닉네임 색상', '시청자마다 다른 색으로 닉네임을 칠해 구분하기 쉽게 합니다(같은 이름은 늘 같은 색)'],
  ['highlightMentions', '멘션 강조', '나를 부른 메시지를 강조하고 모아 둡니다'],
  ['mentionNotify', '멘션 알림', '앱이 뒤에 있을 때 나를 부르면 시스템 알림을 띄웁니다'],
  ['autoScroll', '자동 스크롤', kbd('맨 아래를 보고 있을 때 새 메시지를 따라갑니다 (⌘J)')],
  ['showDonation', '후원·구독 표시', '끄면 채팅에서 후원·구독 카드를 숨깁니다'],
  ['showDonationsOnly', '후원만 보기', '후원·구독 메시지만 보여 줍니다'],
];

function fmt(item, v) {
  if (item.pct) return `${Math.round(Number(v) * 100)}%`;
  return String(v);
}

/** 항목 하나 HTML. */
function itemHtml(item, values) {
  const v = item.local === 'streamAlerts' ? streamAlertsEnabled() : (values[item.d] || {})[item.k];
  const attrs = item.local ? `data-local="${item.local}"` : `data-domain="${item.d}" data-key="${item.k}"`;
  const label = `<span class="setting-label">${esc(item.label)}${item.help ? `<small>${esc(item.help)}</small>` : ''}</span>`;
  const search = esc(`${item.label} ${item.help || ''} ${item.k || ''}`.toLowerCase());
  const wrap = (inner) => `<label class="setting${item.adv ? ' adv' : ''}" data-search="${search}">${label}${inner}</label>`;
  switch (item.type) {
    case 'action':
      return `<div class="setting" data-search="${search}">${label}<button class="btn" data-setting-action="${esc(item.action)}">${esc(item.button)}</button></div>`;
    case 'toggle':
      return wrap(`<span class="toggle"><input type="checkbox" ${attrs} ${v ? 'checked' : ''}><span></span></span>`);
    case 'select':
      return wrap(`<select class="select" ${attrs}>${item.options.map((o) => `<option value="${esc(o.value)}" ${String(o.value) === String(v) ? 'selected' : ''}>${esc(o.label)}</option>`).join('')}</select>`);
    case 'range':
      return wrap(`<span class="range-field"><input type="range" min="${item.min}" max="${item.max}" step="${item.step}" value="${esc(v)}" ${attrs} data-pct="${item.pct ? 1 : ''}"><output>${esc(fmt(item, v))}</output></span>`);
    case 'number':
      return wrap(`<input class="num-input" type="number" step="${item.step || 1}" value="${esc(v)}" ${attrs}>`);
    default:
      return wrap(`<input class="text-input" type="text" value="${esc(v ?? '')}" ${attrs}>`);
  }
}

export class SettingsPage {
  constructor(root, { getAuth, onLogin, onLogout }) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.getAuth = getAuth;
    this.onLogin = onLogin;
    this.onLogout = onLogout;
    this.settings = null;
    this.group = 'overview';
    this.query = '';
    this.showAdvanced = false;
    this.body.addEventListener('change', (e) => this.onChange(e.target));
    this.body.addEventListener('input', (e) => {
      if (e.target.matches('.settings-search')) this.onSearch(e.target);
      if (e.target.type === 'range') {
        const out = e.target.parentElement.querySelector('output');
        if (out) out.textContent = e.target.dataset.pct ? `${Math.round(Number(e.target.value) * 100)}%` : e.target.value;
      }
    });
    this.body.addEventListener('submit', (e) => this.onSubmit(e));
    this.body.addEventListener('click', (e) => this.onClick(e));
    // 자동 프리셋이 값을 바꾸면 네트워크 값만 다시 읽어 프리셋 줄이 보일 때 다시 그린다.
    listen(Events.networkPresetChanged, async () => {
      if (!this.settings) return;
      try { this.settings = await api.settingsGet(); } catch { return; }
      if (this.body.querySelector('[data-net-preset]')) this.render();
    });
  }

  async show() {
    if (!this.settings) this.body.innerHTML = loadingState();
    try {
      await loadChatPrefs();
      this.settings = await api.settingsGet();
    } catch (e) {
      this.body.innerHTML = errorState('설정을 불러오지 못했습니다', e);
      return;
    }
    this.render();
  }

  render() {
    if (!this.settings) return;
    const tabs = `<div class="settings-top">
      <div class="chips settings-tabs">${GROUPS.map(([id, l]) => `<button class="chip${this.group === id ? ' active' : ''}" data-group="${id}">${l}</button>`).join('')}</div>
      <input type="search" class="settings-search" placeholder="설정 검색" value="${esc(this.query)}">
    </div>`;
    this.body.innerHTML = `${tabs}<div class="settings-groups">${this.groupHtml()}</div>`;
    this.applySearch();
    this.refreshOAuth();
    if (this.body.querySelector('.update-row')) this.showUpdate();
    if (this.body.querySelector('[data-setting-action="clear-image-cache"]')) api.imageCacheStatus().then((st) => this.showImageCache(st)).catch(() => {});
  }

  /** "업데이트 확인" 줄 — 현재 버전과 마지막 확인 결과, 새 버전이면 다운로드 단추. */
  async showUpdate() {
    const row = this.body.querySelector('.update-row');
    if (!row) return;
    const u = lastUpdateResult();
    const status = row.querySelector('.update-status');
    if (u) status.textContent = updateSummary(u);
    row.querySelector('[data-sys="update-download"]').hidden = !(u && u.newer && downloadTarget(u));
    row.querySelector('[data-sys="update-page"]').hidden = !(u && u.newer && u.url);
    const version = await currentVersionText();
    const v = row.isConnected && row.querySelector('.update-version');
    if (v) v.textContent = `현재 버전 ${version || '-'}`;
  }

  /** "썸네일 캐시" 줄에 지금 크기를 적는다. */
  showImageCache(st) {
    const row = this.body.querySelector('[data-setting-action="clear-image-cache"]')?.closest('.setting');
    const small = row && row.querySelector('.setting-label small');
    if (!small || !st) return;
    const mb = (b) => (b / 1048576).toFixed(b < 10485760 ? 1 : 0);
    small.textContent = `지금 ${mb(st.bytes)}MB · ${st.entries.toLocaleString()}개 (상한 ${mb(st.capBytes)}MB, 넘으면 오래 안 쓴 것부터 지움)`;
  }

  /** 검색 중이면 모든 그룹, 아니면 선택 그룹. */
  groupHtml() {
    const sections = [];
    const want = (g) => this.query || g === this.group;
    if (want('overview')) sections.push(this.overviewHtml());
    for (const [g, title, items] of SCHEMA) {
      if (!want(g)) continue;
      const adv = items.some((i) => i.adv);
      sections.push(`<section class="settings-group" data-g="${g}"><h3>${esc(title)}${adv ? `<button class="link-btn adv-toggle" data-adv>${this.showAdvanced ? '고급 숨기기' : '고급 보기'}</button>` : ''}</h3>
        ${items.map((i) => itemHtml(i, this.settings)).join('')}</section>`);
      if (g === 'advanced' && title === '네트워크') {
        sections.push(`<section class="settings-group" data-g="advanced"><h3>네트워크 프리셋</h3><div class="setting" data-search="네트워크 프리셋 자동 밸런스 안정 저지연 고성능"><span class="setting-label">한 번에 맞추기<small>${esc(netPresetHelp(this.settings))}</small></span>
          <div class="segmented" role="radiogroup" aria-label="네트워크 프리셋">${NETWORK_PRESETS.map(([v, l]) => { const on = v === this.settings.networkPreset; return `<button class="seg ${on ? 'active' : ''}" data-net-preset="${v}" role="radio" aria-checked="${on}">${l}</button>`; }).join('')}</div></div></section>`);
      }
    }
    if (want('chat')) sections.push(this.chatHtml());
    if (want('general')) sections.push(this.shortcutsHtml());
    if (want('advanced')) sections.push(this.backupHtml());
    return sections.join('');
  }

  overviewHtml() {
    const auth = this.getAuth();
    const account = auth && auth.loggedIn
      ? `<div class="setting" data-search="계정 로그아웃"><span class="setting-label">${esc(auth.nickname || '로그인됨')}<small>네이버 계정 (${esc(auth.method || 'cookie')})</small></span><button class="btn" data-action="logout">로그아웃</button></div>`
      : '<div class="setting" data-search="계정 로그인"><span class="setting-label">로그인되지 않음<small>팔로잉 목록과 채팅 참여에 네이버 로그인이 필요합니다</small></span><button class="btn primary" data-action="login">로그인</button></div>';
    return `
      <section class="settings-group" data-g="overview"><h3>계정</h3>${account}</section>
      <section class="settings-group oauth-group" data-g="overview"><h3>치지직 OAuth 연동 <small class="muted">선택</small></h3>
        <div class="setting" data-search="oauth 치지직 연동 client id secret 개발자"><span class="setting-label">애플리케이션 자격 증명<small class="oauth-status">치지직 개발자 센터에 등록한 앱의 Client ID·Secret — 키체인에 저장합니다</small></span></div>
        <div class="oauth-form">
          <input type="text" name="clientId" placeholder="Client ID" autocomplete="off" spellcheck="false">
          <input type="password" name="clientSecret" placeholder="Client Secret" autocomplete="off">
          <button type="button" class="btn" data-oauth="save">저장</button>
          <button type="button" class="btn primary" data-oauth="connect">연결</button>
          <button type="button" class="link-btn" data-oauth="clear">지우기</button>
        </div>
      </section>
      <section class="settings-group" data-g="overview"><h3>화면</h3>
        <div class="setting" data-search="테마 다크 라이트 시스템">
          <span class="setting-label">테마<small>시스템은 macOS 화면 모드를 따릅니다</small></span>
          <div class="segmented" role="radiogroup" aria-label="테마">
            ${THEMES.map((t) => `<button class="seg ${t === currentTheme() ? 'active' : ''}" data-theme-choice="${t}" role="radio" aria-checked="${t === currentTheme()}">${THEME_LABELS[t]}</button>`).join('')}
          </div>
        </div>
      </section>
      <section class="settings-group" data-g="overview"><h3>앱</h3>
        <div class="setting update-row" data-search="업데이트 확인 버전 다운로드 릴리스"><span class="setting-label">업데이트 확인<small class="update-version">현재 버전 확인 중…</small><small class="update-status">업데이트 서버의 최신 릴리스와 비교합니다</small></span><span class="shortcut-edit"><button class="link-btn" data-sys="update-page" hidden>릴리스 노트</button><button class="btn primary" data-sys="update-download" hidden>다운로드</button><button class="btn" data-sys="update">지금 확인</button></span></div>
        <div class="setting" data-search="알림 권한 시스템 알림"><span class="setting-label">시스템 알림 권한<small class="notify-status">방송 시작·멘션 알림에 필요합니다</small></span><span class="shortcut-edit"><button class="btn" data-sys="notify">권한 확인</button><button class="link-btn" data-sys="notify-settings">시스템 설정 열기</button></span></div>
        <div class="setting" data-search="딥 링크 cview url"><span class="setting-label">링크로 열기<small>cview://live?channelId=… · cview://search?q=… · cview://multilive</small></span></div>
      </section>`;
  }

  chatHtml() {
    const p = chatPrefs();
    const toggle = ([key, label, help]) => `
      <label class="setting" data-search="${esc(`${label} ${help}`.toLowerCase())}"><span class="setting-label">${esc(label)}<small>${esc(help)}</small></span>
        <span class="toggle"><input type="checkbox" data-chat="${key}" ${p[key] ? 'checked' : ''}><span></span></span></label>`;
    const range = (key, label, min, max, step) => `
      <label class="setting" data-search="${esc(label)}"><span class="setting-label">${esc(label)}</span>
        <span class="range-field"><input type="range" min="${min}" max="${max}" step="${step}" value="${esc(p[key])}" data-chat="${key}"><output>${esc(p[key])}</output></span></label>`;
    const words = (p.blockedWords || []).map((w) => `<span class="word-chip">${esc(w)}<button data-remove-word="${esc(w)}" title="지우기">✕</button></span>`).join('');
    const users = (p.blockedUsers || []).map((u) => `<span class="word-chip">${esc(blockedLabel(u))}<button data-unblock="${esc(u)}" title="차단 해제">해제</button></span>`).join('');
    return `
      <section class="settings-group" data-g="chat"><h3>채팅 표시</h3>
        ${range('fontSize', '글자 크기', 11, 20, 1)}
        ${range('lineSpacing', '줄 간격', 0, 8, 1)}
        ${CHAT_TOGGLES.map(toggle).join('')}
        <label class="setting" data-search="최대 표시 줄 수"><span class="setting-label">최대 표시 줄 수<small>오래된 메시지부터 지웁니다</small></span>
          <select class="select" data-chat="maxVisibleMessages">${[150, 300, 500, 1000].map((n) => `<option value="${n}" ${Number(p.maxVisibleMessages) === n ? 'selected' : ''}>${n}줄</option>`).join('')}</select></label>
      </section>
      <section class="settings-group" data-g="chat"><h3>읽기 폭 · 배경</h3>
        <label class="setting" data-search="읽기 폭 채팅 너비"><span class="setting-label">읽기 폭<small>넓은 채팅 창에서 메시지 줄 길이를 제한합니다</small></span>
          <select class="select" data-chat-str="readingWidth">${READING_WIDTHS.map(([v, l]) => `<option value="${v}" ${(p.readingWidth || 'full') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="채팅 배경"><span class="setting-label">배경</span>
          <select class="select" data-chat-bg="mode">${BACKGROUNDS.map(([v, l]) => `<option value="${v}" ${(p.background || {}).mode === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="배경 강도"><span class="setting-label">배경 강도</span>
          <span class="range-field"><input type="range" min="0" max="1" step="0.05" value="${esc((p.background || {}).strength ?? 0.5)}" data-chat-bg="strength"><output>${esc((p.background || {}).strength ?? 0.5)}</output></span></label>
      </section>
      <section class="settings-group" data-g="chat"><h3>후원 읽어 주기(TTS)</h3>
        ${toggle(['ttsEnabled', '후원·구독 읽어 주기', '새 후원과 구독을 소리 내어 읽습니다 (/tts 로도 켜고 끔)'])}
        ${range('ttsVolume', '음량', 0, 1, 0.05)}
        ${range('ttsRate', '속도', 100, 400, 10)}
        <label class="setting" data-search="tts 음성 목소리"><span class="setting-label">목소리</span>
          <select class="select" data-chat-str="ttsVoiceIdentifier"><option value="">기본</option>${(window.speechSynthesis ? speechSynthesis.getVoices() : []).filter((v) => v.lang.startsWith('ko')).map((v) => `<option value="${esc(v.voiceURI)}" ${p.ttsVoiceIdentifier === v.voiceURI ? 'selected' : ''}>${esc(v.name)}</option>`).join('')}</select></label>
      </section>
      <section class="settings-group" data-g="chat"><h3>필터 · 사용자 관리</h3>
        ${toggle(['chatFilterEnabled', '금지어 필터', '아래 단어가 들어간 메시지를 숨깁니다'])}
        <div class="setting column" data-search="금지어"><div class="word-list">${words || '<span class="muted">금지어가 없습니다</span>'}</div>
          <form class="word-form" data-word-form><input type="text" maxlength="40" placeholder="숨길 단어 추가"><button class="btn">추가</button></form></div>
        <div class="setting column" data-search="차단 사용자"><span class="setting-label">차단한 사용자<small>채팅의 닉네임을 오른쪽 클릭해 차단할 수 있습니다</small></span>
          <div class="word-list">${users || '<span class="muted">차단한 사용자가 없습니다</span>'}</div></div>
      </section>`;
  }

  shortcutsHtml() {
    const b = currentBindings();
    const rows = ACTIONS.map(([id, label, fixed]) => `
      <div class="setting" data-search="${esc(`단축키 키보드 ${label}`)}"><span class="setting-label">${esc(label)}${FIXED.has(id) ? '<small>메뉴가 정한 고정 키</small>' : ''}</span>
        ${FIXED.has(id) ? `<kbd class="kbd">${esc(fixed)}</kbd>` : `<span class="shortcut-edit"><kbd class="kbd">${esc(bindingLabel(b[id]))}</kbd><button class="btn" data-bind="${id}">바꾸기</button><button class="link-btn" data-bind-reset="${id}">기본값</button></span>`}
      </div>`).join('');
    return `<section class="settings-group" data-g="general"><h3>키보드 단축키</h3>${rows}
      <div class="setting" data-search="단축키 키보드 목록"><span class="setting-label">전체 단축키 목록<small>${kbd('메뉴 막대와 ⌘/ 로 볼 수 있습니다 · ⌘ 가 들어간 단축키는 메뉴 막대에도 반영됩니다')}</small></span><button class="btn" data-open-shortcuts>보기</button></div></section>`;
  }

  backupHtml() {
    return `<section class="settings-group" data-g="advanced"><h3>백업 · 초기화</h3>
      <div class="setting" data-search="설정 내보내기 백업"><span class="setting-label">설정 내보내기<small>모든 설정을 JSON 파일로 ~/Downloads/CView 에 저장합니다</small></span><button class="btn" data-export>내보내기</button></div>
      <div class="setting column" data-search="설정 가져오기 복원"><span class="setting-label">설정 가져오기<small>내보낸 JSON 내용을 붙여 넣으세요</small></span>
        <form class="word-form" data-import><input type="text" placeholder='{"player": …}'><button class="btn">가져오기</button></form></div>
      <div class="setting" data-search="초기화 기본값"><span class="setting-label">모든 설정 초기화<small>로그인·시청 기록은 그대로 둡니다</small></span><button class="btn danger" data-reset-all>초기화</button></div>
    </section>`;
  }

  /**
   * 검색어 — 비어 있다가 생기거나 그 반대면 그룹 구성이 바뀐다(검색 중엔 모든 탭의 설정을 그린다).
   * applySearch 만 부르면 지금 탭 안에서만 찾았다. 다시 그린 뒤 입력란 초점·커서를 되돌린다.
   */
  onSearch(input) {
    const was = Boolean(this.query);
    this.query = input.value.trim().toLowerCase();
    if (was === Boolean(this.query)) { this.applySearch(); return; }
    const caret = input.selectionStart;
    this.render();
    const again = this.body.querySelector('.settings-search');
    again.value = input.value;
    again.focus();
    again.setSelectionRange(caret, caret);
  }

  applySearch() {
    const q = this.query;
    for (const row of this.body.querySelectorAll('.setting')) {
      const adv = row.classList.contains('adv') && !this.showAdvanced && !q;
      const hit = !q || (row.dataset.search || '').includes(q);
      row.hidden = adv || !hit;
    }
    for (const sec of this.body.querySelectorAll('.settings-group')) {
      sec.hidden = Boolean(q) && !sec.querySelector('.setting:not([hidden])');
    }
  }

  async onChange(target) {
    if (target.matches('.settings-search')) return;
    if (target.dataset.chatStr) { await updateChatPrefs({ [target.dataset.chatStr]: target.value || null }); return; }
    if (target.dataset.chatBg) {
      const bg = { mode: 'neutral', strength: 0.5, ...(chatPrefs().background || {}) };
      bg[target.dataset.chatBg] = target.dataset.chatBg === 'strength' ? Number(target.value) : target.value;
      await updateChatPrefs({ background: bg });
      return;
    }
    if (target.dataset.chat) {
      const value = target.type === 'checkbox' ? target.checked : Number(target.value);
      await updateChatPrefs({ [target.dataset.chat]: value });
      return;
    }
    if (target.dataset.local === 'streamAlerts') {
      setStreamAlertsEnabled(target.checked);
      return;
    }
    const { domain, key } = target.dataset;
    if (!domain || !key) return;
    let value;
    if (target.type === 'checkbox') value = target.checked;
    else if (target.type === 'range' || target.type === 'number') value = Number(target.value);
    else if (target.tagName === 'SELECT' && /^\d+$/.test(target.value)) value = Number(target.value);
    else value = target.value;
    try {
      this.settings = await api.settingsUpdate(domain, { [key]: value });
      toast('설정을 저장했습니다', 1200);
      window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain, key, value, settings: this.settings } }));
    } catch (e) {
      toast(e.message);
      this.render();
    }
  }

  async onSubmit(e) {
    const form = e.target;
    if (form.matches('[data-word-form]')) {
      e.preventDefault();
      const word = form.querySelector('input').value.trim();
      if (!word) return;
      await updateChatPrefs({ blockedWords: [...new Set([...(chatPrefs().blockedWords || []), word])] });
      this.render();
    } else if (form.matches('[data-import]')) {
      e.preventDefault();
      try {
        const skipped = await api.settingsImport(form.querySelector('input').value);
        this.settings = await api.settingsGet();
        toast(skipped.length ? `가져왔습니다 (건너뜀: ${skipped.join(', ')})` : '설정을 가져왔습니다', 2500);
        this.render();
      } catch (er) { toast(er.message); }
    }
  }

  /** 다음 키 조합을 받아 바인딩으로 저장한다. Esc 취소. */
  captureBinding(btn) {
    // 받는 중에 다시 누르면 취소 — 한 번에 하나만 받는다.
    if (this.cancelCapture) { this.cancelCapture(); return; }
    const action = btn.dataset.bind;
    btn.textContent = '키를 누르세요…';
    btn.classList.add('primary');
    const onKey = async (e) => {
      // 화면을 떠났으면 그만 받는다 — 다음 키(검색란 입력 등)를 삼켜 바인딩으로 저장하던 문제.
      if (this.root.hidden || !btn.isConnected) { done(); return; }
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') { done(); this.render(); return; }
      const b = bindingFromEvent(e);
      if (!b) return;
      done();
      const clash = conflictOf(action, b);
      if (clash) toast(`${ACTIONS.find(([a]) => a === clash)?.[1] || clash} 와 겹칩니다 — 그 동작도 같은 키로 실행됩니다`, 4000);
      try { await saveBinding(action, b); } catch (er) { toast(er.message); }
      this.render();
    };
    // 다른 곳을 누르면 취소(이 버튼을 다시 누르는 것은 위의 토글이 처리한다).
    const onDown = (e) => { if (!btn.contains(e.target)) { done(); this.render(); } };
    const done = () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('pointerdown', onDown, true);
      this.cancelCapture = null;
    };
    this.cancelCapture = () => { done(); this.render(); };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('pointerdown', onDown, true);
  }

  async refreshOAuth() {
    const el = this.body.querySelector('.oauth-status');
    if (!el) return;
    try {
      const st = await api.oauthStatus();
      el.textContent = st.connected
        ? `연결됨 · ${st.clientIdHint || ''}`
        : st.hasCredentials ? `자격 증명 저장됨 (${st.clientIdHint || ''}) · 아직 연결 안 됨 · 리디렉트 ${st.redirectUri}` : `미설정 · 리디렉트 URI 를 ${st.redirectUri} 로 등록하세요`;
    } catch { /* 앱 밖 */ }
  }

  async onOAuth(b) {
    const form = this.body.querySelector('.oauth-form');
    b.disabled = true;
    try {
      if (b.dataset.oauth === 'save') {
        const secret = form.querySelector('[name=clientSecret]');
        await api.oauthSetCredentials(form.querySelector('[name=clientId]').value, secret.value);
        secret.value = '';
        toast('자격 증명을 저장했습니다', 1500);
      } else if (b.dataset.oauth === 'clear') {
        await api.oauthClearCredentials();
        toast('자격 증명을 지웠습니다', 1500);
      } else {
        toast('브라우저에서 치지직 연동을 허용해 주세요 (5분 안)', 5000);
        await api.oauthConnect();
        toast('치지직 OAuth 연결을 마쳤습니다', 2000);
      }
    } catch (er) { toast(er.message); }
    b.disabled = false;
    this.refreshOAuth();
  }

  async onSystem(b) {
    b.disabled = true;
    try {
      if (b.dataset.sys === 'update') {
        const status = this.body.querySelector('.update-status');
        if (status) status.textContent = '업데이트 서버를 확인하는 중…';
        try {
          await checkForUpdate();
        } catch (er) {
          if (status) status.textContent = `확인하지 못했습니다 — ${er.message || er}`;
          throw er;
        }
        this.showUpdate();
      } else if (b.dataset.sys === 'update-download') {
        const u = lastUpdateResult();
        if (u) await openDownload(u);
      } else if (b.dataset.sys === 'update-page') {
        const u = lastUpdateResult();
        if (u && u.url) await api.openExternal(u.url);
      } else if (b.dataset.sys === 'notify-settings') {
        await api.openNotificationSettings();
      } else if (b.dataset.sys === 'notify') {
        let st = await api.notifyPermission(false);
        if (st.state !== 'granted') st = await api.notifyPermission(true);
        const label = { granted: '허용됨', denied: '거부됨 — 시스템 설정 › 알림에서 CView 를 켜 주세요', prompt: '아직 묻지 않음', 'prompt-with-rationale': '아직 묻지 않음' };
        this.body.querySelector('.notify-status').textContent = label[st.state] || st.state;
        if (st.state === 'granted') api.notifySend('CView', '알림이 이렇게 표시됩니다').catch(() => {});
      }
    } catch (er) { toast(er.message); }
    b.disabled = false;
  }

  async onClick(e) {
    const bind = e.target.closest('[data-bind]');
    if (bind) { this.captureBinding(bind); return; }
    const bindReset = e.target.closest('[data-bind-reset]');
    if (bindReset) { try { await saveBinding(bindReset.dataset.bindReset, null); this.render(); } catch (er) { toast(er.message); } return; }
    const oa = e.target.closest('[data-oauth]');
    if (oa) { e.preventDefault(); this.onOAuth(oa); return; }
    const sys = e.target.closest('[data-sys]');
    if (sys) { this.onSystem(sys); return; }
    const g = e.target.closest('[data-group]');
    if (g) { this.group = g.dataset.group; this.query = ''; this.render(); return; }
    if (e.target.closest('[data-adv]')) { e.preventDefault(); this.showAdvanced = !this.showAdvanced; this.render(); return; }
    const action = e.target.closest('[data-action]');
    if (action) {
      if (action.dataset.action === 'login') this.onLogin();
      if (action.dataset.action === 'logout') this.onLogout();
      return;
    }
    const choice = e.target.closest('[data-theme-choice]');
    if (choice) { await setTheme(choice.dataset.themeChoice); this.render(); return; }
    const w = e.target.closest('[data-remove-word]');
    if (w) { await updateChatPrefs({ blockedWords: (chatPrefs().blockedWords || []).filter((x) => x !== w.dataset.removeWord) }); this.render(); return; }
    const u = e.target.closest('[data-unblock]');
    if (u) {
      const p = chatPrefs();
      const names = { ...(p.blockedUserNames || {}) };
      delete names[u.dataset.unblock];
      await updateChatPrefs({ blockedUsers: (p.blockedUsers || []).filter((x) => x !== u.dataset.unblock), blockedUserNames: names });
      this.render();
      return;
    }
    const preset = e.target.closest('[data-net-preset]');
    if (preset) {
      try { this.settings = await api.networkPreset(preset.dataset.netPreset); toast(`네트워크: ${preset.textContent}`, 1500); this.render(); } catch (er) { toast(er.message); }
      return;
    }
    const settingAction = e.target.closest('[data-setting-action]');
    if (settingAction && settingAction.dataset.settingAction === 'clear-image-cache') {
      api.imageCacheClear().then((st) => { this.showImageCache(st); toast('썸네일 캐시를 비웠습니다'); }).catch((er) => toast(er.message || String(er)));
      return;
    }
    if (settingAction && settingAction.dataset.settingAction === 'reset-alert-pos') {
      try {
        this.settings = await api.settingsUpdate('player', { streamAlertAnchorX: ALERT_HOME.x, streamAlertAnchorY: ALERT_HOME.y });
        window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain: 'player', settings: this.settings } }));
        toast('알림을 기본 위치로 되돌렸습니다', 1500);
        this.render();
      } catch (er) { toast(er.message); }
      return;
    }
    if (settingAction && settingAction.dataset.settingAction === 'open-logs') {
      api.perfLogStatus().then((s) => api.revealPath(s.file && s.enabled ? s.file : s.dir)).catch((er) => toast(`로그 폴더를 열 수 없습니다 — 디버그 로그를 한 번 켜 주세요 (${er.message || er})`));
      return;
    }
    if (e.target.closest('[data-open-shortcuts]')) { window.dispatchEvent(new CustomEvent('cview:menu', { detail: 'shortcuts' })); return; }
    if (e.target.closest('[data-export]')) {
      try {
        const json = await api.settingsExport();
        const d = new Date();
        const path = await api.saveFile('settings', `CView-설정-${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}.json`, null, json);
        toast(`내보냈습니다 — ${path}`, 4000, { label: 'Finder에서 보기', run: () => api.revealPath(path) });
      } catch (er) { toast(er.message); }
      return;
    }
    if (e.target.closest('[data-reset-all]')) {
      const btn = e.target.closest('[data-reset-all]');
      if (btn.dataset.armed !== '1') { btn.dataset.armed = '1'; btn.textContent = '한 번 더 누르면 초기화'; setTimeout(() => { btn.dataset.armed = ''; btn.textContent = '초기화'; }, 3000); return; }
      try { this.settings = await api.settingsReset('all'); toast('설정을 초기화했습니다', 1800); this.render(); } catch (er) { toast(er.message); }
    }
  }
}
