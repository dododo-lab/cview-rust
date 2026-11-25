// 설정 — 원본 SettingsView / SettingsWorkspace: 그룹 탭(개요·시청·채팅·멀티·일반·연동·성능·고급·정보) + 설정 검색 (SET-01).
// 항목은 스키마로 정의하고 Rust 설정 도메인(settings_update)에 바로 저장한다.
// SET-02~06 플레이어·지연·되감기·스크린샷·비디오, SET-07 채팅, SET-08 일반, SET-09 외관,
// SET-10 성능, SET-11 네트워크(프리셋), SET-13 채널 알림, SET-15 동기화, SET-16 도메인, SET-17 백업·복원·초기화,
// MUL-16 멀티라이브 설정.
import { api, listen, Events } from './api.js';
import { ACTIONS, FIXED, currentBindings, bindingLabel, bindingFromEvent, saveBinding, conflictOf } from './shortcuts.js';
import { esc, toast } from './dom.js';
import { armConfirm, confirmDialog } from './confirm.js';
import { bindTabKeys } from './a11y.js';
import { emptyState, errorState, loadingState } from './cards.js';
import { THEMES, THEME_LABELS, currentTheme, setTheme, syncTheme } from './theme.js';
import { streamAlertsEnabled, setStreamAlertsEnabled, ALERT_HOME } from './donations.js';
import { chatPrefs, updateChatPrefs, blockedLabel, loadChatPrefs } from './chat-prefs.js';
import { BACKGROUNDS, READING_WIDTHS } from './chat-extras.js';
import { kbd, IS_WINDOWS, REVEAL_LABEL, TRAY_LABEL, TRAY_TO, saveDirPath, SECRET_STORE_LABEL } from './platform.js';
import { thumbPreviewEnabled, setThumbPreviewEnabled } from './hover-preview.js';
import { SHARPNESS_LEVELS, SHARPNESS_HELP, ENLARGED_ONLY_LABEL, ENLARGED_ONLY_HELP, sharpnessState, setSharpness } from './sharpness.js';
import { checkForUpdate, lastUpdateResult, updateSummary, downloadTarget, openDownload, currentAppInfo, canInstall, installUpdate, progressHtml, renderProgress } from './update-check.js';
import { aboutHtml, fillAbout, onAboutClick } from './settings-about.js';
import { usagePanelHtml, mountUsagePanel } from './settings-usage.js';
import { clearUsage, exportUsage } from './usage-monitor.js';

const opt = (pairs) => pairs.map(([value, label]) => ({ value, label }));
/** 상위 스위치가 꺼져 있으면 이 항목을 막는다 — disabled: offWhen('general', 'notificationsEnabled') (감사 UX-M10) */
const offWhen = (d, k) => (v) => !(v[d] || {})[k];

/** 외관 컨트롤을 마지막으로 바꾼 뒤 이만큼은 '조작 중'으로 본다(키를 연달아 누르는 간격보다 넉넉히). */
const APPEARANCE_SETTLE_MS = 700;

/** 스키마 — [그룹, 섹션 제목, 항목들]. 항목 type: toggle | select | range | number | text. */
export const SCHEMA = [
  ['watch', '재생', [
    { d: 'player', k: 'quality', label: '기본 화질', type: 'select', options: opt([['1080p', '1080p (원본)'], ['720p', '720p'], ['480p', '480p'], ['360p', '360p'], ['auto', '자동']]) },
    { d: 'player', k: 'autoPlay', label: '자동 재생', help: '시청 화면을 열면 바로 재생합니다', type: 'toggle' },
    { d: 'player', k: 'startMuted', label: '음소거로 시작', help: '방송을 열 때 소리를 끈 상태로 시작합니다', type: 'toggle' },
    { d: 'player', k: 'forceHighestQuality', label: '항상 최고 화질', help: '1080p60 을 우선 유지합니다', type: 'toggle' },
    { d: 'player', k: 'hevcEnabled', label: 'HEVC(H.265) 화질', help: '하드웨어로 디코딩할 수 있으면 1080p 를 HEVC 로 받습니다. 끄면 H.264 만 씁니다', keywords: 'h265 h.265 hevc h264 h.264 코덱 화질', type: 'toggle' },
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
    // 선명도 — 예전 '선명한 화면'(영상 ⚙, 채널별)과 '업스케일'을 합친 전역 값(sharpness.js). 저장은 setSharpness 로(구버전 거울까지).
    { d: 'player', k: 'sharpnessLevel', label: '선명도', help: SHARPNESS_HELP, type: 'select', options: opt(SHARPNESS_LEVELS.map((l) => [l.value, l.option])), get: () => sharpnessState().level },
    { d: 'player', k: 'sharpnessEnlargedOnly', label: ENLARGED_ONLY_LABEL, help: ENLARGED_ONLY_HELP, type: 'toggle', get: () => sharpnessState().enlargedOnly, disabled: () => !sharpnessState().level },
    { d: 'player', k: 'autoPictureEnhancement', label: '자동 밝기·색 보정', help: '어둡거나 뿌연 장면의 밝기·대비·채도를 천천히 맞춥니다(단독 보기·시청 화면)', type: 'toggle' },
    { d: 'player', k: 'sharpPixelScaling', label: '픽셀 그대로 확대', help: '확대할 때 픽셀을 부드럽게 하지 않습니다 — 선명도가 처리하는 동안에는 보이지 않습니다', type: 'toggle' },
    { d: 'player', k: 'videoAdjustEnabled', label: '색 보정 사용(시청 화면)', help: '시청 화면의 ⚙ 패널에서 밝기·대비 등을 조절합니다', type: 'toggle' },
    { d: 'player', k: 'screenshotFormat', label: '스크린샷 형식', type: 'select', options: opt([['png', 'PNG'], ['jpeg', 'JPEG']]) },
    { d: 'player', k: 'screenshotPath', label: '스크린샷 폴더', type: 'text' },
    { local: 'streamAlerts', label: '후원·구독 알림', help: '시청 화면 위에 새 후원과 구독을 잠깐 띄웁니다', type: 'toggle' },
    { d: 'player', k: 'streamAlertAnchorX', label: '알림 가로 위치', type: 'range', min: 0.1, max: 0.9, step: 0.05, pct: true },
    { d: 'player', k: 'streamAlertAnchorY', label: '알림 세로 위치', type: 'range', min: 0.02, max: 0.9, step: 0.02, pct: true },
    { d: 'player', k: 'streamAlertScale', label: '알림 크기', type: 'range', min: 0.6, max: 1.6, step: 0.1 },
    { type: 'action', action: 'reset-alert-pos', label: '알림 위치 초기화', help: '영상 위 알림은 끌어서 옮길 수 있어요(알림을 더블클릭해도 기본 위치로 돌아갑니다)', button: '기본 위치로' },
  ]],
  ['multi', '멀티라이브', [
    { d: 'multiLive', k: 'maxConcurrentSessions', label: '멀티라이브 최대 동시 세션', help: '멀티라이브에서 함께 재생하는 패인 수, 4~20개 (많을수록 CPU·대역폭을 더 씁니다). 라이브에서 더 많이 담아 왔다면 넘는 패인은 멈춘 채로 보이고 누르면 재생됩니다', type: 'range', min: 4, max: 20, step: 1 },
    { d: 'multiLive', k: 'maxLiveChannels', label: '라이브 단독 보기 최대 채널', help: '라이브 화면 머리줄에 담아 두고 다시 연결 없이 바꿔 볼 수 있는 채널 수, 4~20개 (보는 채널 밖은 로딩을 멈춰 가볍습니다)', type: 'range', min: 4, max: 20, step: 1 },
    { d: 'multiLive', k: 'defaultLayoutMode', label: '기본 레이아웃', type: 'select', options: opt([['preset', '균등 분할'], ['focus', '포커스 모드']]) },
    { d: 'multiLive', k: 'defaultFocusDirection', label: '포커스 메인 위치', type: 'select', options: opt([['left', '왼쪽'], ['right', '오른쪽'], ['top', '위'], ['bottom', '아래']]) },
    { d: 'multiLive', k: 'rememberLastLayout', label: '마지막 레이아웃 기억', type: 'toggle' },
    { d: 'multiLive', k: 'multiAudioEnabled', label: '여러 패인 소리', help: '선택 패인 외에도 보조 볼륨으로 소리를 냅니다', type: 'toggle' },
    { d: 'multiLive', k: 'secondaryVolume', label: '보조 패인 볼륨', type: 'range', min: 0, max: 1, step: 0.05, pct: true, disabled: offWhen('multiLive', 'multiAudioEnabled') },
    { d: 'multiLive', k: 'bandwidthCoordinationEnabled', label: '대역폭 분배', help: '선택 패인에 대역폭을 더 줍니다', type: 'toggle' },
    { d: 'multiLive', k: 'selectedSessionBWWeight', label: '선택 패인 대역폭 가중치', type: 'range', min: 1, max: 3, step: 0.1, disabled: offWhen('multiLive', 'bandwidthCoordinationEnabled') },
    { d: 'multiLive', k: 'levelCappingEnabled', label: '패인 크기 화질 제한', type: 'toggle' },
    { d: 'multiLive', k: 'backgroundQualityReduction', label: '보조 패인 화질 낮추기', type: 'toggle' },
    { d: 'multiLive', k: 'autoReconnect', label: '자동 재연결', type: 'toggle' },
    { d: 'multiLive', k: 'autoReconnectMaxRetries', label: '재연결 최대 횟수', type: 'range', min: 1, max: 30, step: 1, disabled: offWhen('multiLive', 'autoReconnect') },
    { d: 'multiLive', k: 'chatOverlayInGrid', label: '그리드 채팅 오버레이', type: 'toggle' },
    { d: 'multiLive', k: 'chatOverlayOpacity', label: '채팅 오버레이 투명도', type: 'range', min: 0.1, max: 1, step: 0.05, pct: true },
    { d: 'multiLive', k: 'chatOverlayFontSize', label: '채팅 오버레이 글자 크기', type: 'range', min: 9, max: 20, step: 1 },
  ]],
  ['general', '일반', [
    { d: 'general', k: 'autoRefreshEnabled', label: '목록 자동 새로고침', type: 'toggle' },
    { d: 'general', k: 'autoRefreshInterval', label: '새로고침 주기(초)', type: 'range', min: 30, max: 600, step: 30, disabled: offWhen('general', 'autoRefreshEnabled') },
    { d: 'general', k: 'restoreWindowOnLaunch', label: '창 위치·크기 기억', type: 'toggle' },
    { d: 'general', k: 'alwaysOnTop', label: '항상 위에', type: 'toggle' },
    // Windows 는 아직 자동 실행을 걸지 않는다(window_prefs.rs set_launch_at_login 이 macOS 만) — 켜도 아무 일이 없어 숨긴다.
    ...(IS_WINDOWS ? [] : [{ d: 'general', k: 'launchAtLogin', label: '로그인 시 자동 실행', type: 'toggle' }]),
    { d: 'general', k: 'showInMenuBar', label: `${TRAY_LABEL}에 표시`, help: `방송 중인 팔로잉·지금 시청 중인 방송을 아이콘 메뉴에서 봅니다. 창을 ${TRAY_TO} 숨겨 둔 동안에는 꺼 두어도 아이콘이 보입니다`, keywords: '트레이 시스템 트레이 메뉴바 아이콘', type: 'toggle' },
    // 숨기기(tray.rs) — 창만 숨겨 재생·채팅·알림은 계속된다. 끝내려면 아이콘 메뉴의 'CView 종료'.
    { d: 'general', k: 'closeToTray', label: `창을 닫으면 ${TRAY_TO} 숨기기`, help: `앱을 끝내지 않고 ${TRAY_LABEL} 아이콘으로 숨깁니다 — 재생·채팅·알림은 그대로 이어지고, 아이콘을 누르면 다시 열립니다. 끝내려면 아이콘을 오른쪽 클릭해 'CView 종료'`, keywords: '트레이 메뉴바 백그라운드 닫기 종료 숨기기', type: 'toggle' },
    { d: 'general', k: 'minimizeToTray', label: `최소화하면 ${TRAY_TO} 숨기기`, help: IS_WINDOWS ? '작업 표시줄 대신 알림 영역 아이콘으로 숨깁니다' : 'Dock 대신 메뉴 막대 아이콘으로 숨깁니다', keywords: '트레이 메뉴바 백그라운드 최소화 숨기기', type: 'toggle' },
    ...(IS_WINDOWS ? [] : [{ d: 'general', k: 'hideDockInTray', label: '숨겨 있는 동안 Dock 아이콘 숨기기', help: '메뉴 막대로 숨기면 Dock 에서도 빠집니다 — 창을 다시 열면 돌아옵니다', keywords: '독 dock 트레이 메뉴바', type: 'toggle', disabled: (v) => !((v.general || {}).closeToTray || (v.general || {}).minimizeToTray) }]),
  ]],
  ['general', '알림', [
    { d: 'general', k: 'notificationsEnabled', label: '알림 사용', type: 'toggle' },
    { d: 'general', k: 'notifyOnLiveStart', label: '팔로우 채널 방송 시작', type: 'toggle', disabled: offWhen('general', 'notificationsEnabled') },
    { d: 'general', k: 'notifyOnCategoryChange', label: '카테고리 변경', type: 'toggle', disabled: offWhen('general', 'notificationsEnabled') },
    { d: 'general', k: 'notifyOnTitleChange', label: '방송 제목 변경', type: 'toggle', disabled: offWhen('general', 'notificationsEnabled') },
  ]],
  ['general', '외관', [
    { d: 'appearance', k: 'uiScale', label: '화면 크기', help: '앱 전체의 글자·단추·카드 크기를 함께 키우거나 줄입니다(⌘= 확대 · ⌘- 축소 · ⌘0 실제 크기). 4K 모니터를 1배율로 쓸 때 키워 보세요', type: 'range', min: 0.8, max: 2, step: 0.05, pct: true },
    { d: 'appearance', k: 'compactMode', label: '콤팩트 모드', help: '여백과 카드를 촘촘하게 줄입니다', type: 'toggle' },
    { d: 'appearance', k: 'motionQuality', label: '모션', type: 'select', options: opt([['reduced', '절약'], ['balanced', '균형'], ['rich', '풍부']]) },
    { local: 'thumbPreview', label: '썸네일 미리보기', help: '라이브·다시보기·클립 썸네일 위에 마우스를 잠시 올려 두면 소리 없이 미리 봅니다(모션 "절약"에서는 쉽니다)', type: 'toggle' },
    { d: 'appearance', k: 'transparencyMode', label: '투명도', type: 'select', options: opt([['system', '시스템'], ['full', '선명하게'], ['reduced', '불투명']]) },
    { d: 'appearance', k: 'frameRateMode', label: '화면 갱신', type: 'select', options: opt([['adaptive', '적응형 (최대 120Hz)'], ['fixed60', '60Hz 고정']]) },
  ]],
  ['link', '시청 기록 동기화', [
    { d: 'sync', k: 'explicitlyOptedIn', label: '동기화 동의', help: '다른 기기와 시청 기록을 맞추려면 동의가 필요합니다', type: 'toggle' },
    // 동의 없이 켜면 Rust 가 꺼진 값으로 저장한다(settings_ui.rs) — 동의 전에는 막아 둔다.
    { d: 'sync', k: 'watchHistorySyncEnabled', label: '시청 기록 동기화', type: 'toggle', disabled: (v) => !(v.sync || {}).explicitlyOptedIn },
    { d: 'sync', k: 'endpoint', label: '동기화 서버', type: 'text' },
  ]],
  // 성능 — 앱 사용률 측정·기록(그래프 패널은 settings-usage.js, 값은 외관 도메인 usage*). 머리줄 칩은 usage-chip.js.
  ['usage', '측정 · 기록', [
    { d: 'appearance', k: 'usageMonitor', label: '사용률 측정', help: '앱과 웹 화면·GPU·네트워크 도우미 프로세스의 CPU·GPU·메모리·네트워크 수신을 재어 기록합니다. 끄면 측정을 멈춰 CPU를 쓰지 않습니다', keywords: '성능 사용률 모니터 cpu gpu 메모리 램 네트워크 자원', type: 'toggle' },
    { d: 'appearance', k: 'usageInHeader', label: '머리줄에 표시', help: '화면 위 머리줄에 실시간 사용률 칩을 보입니다 — 누르면 자세히 봅니다(측정이 켜져 있을 때)', keywords: '상단 메뉴 칩 사용률', type: 'toggle' },
    { d: 'appearance', k: 'usageIntervalMs', label: '측정 간격', help: '짧을수록 그래프가 촘촘하고, 길수록 측정 부담이 줄어듭니다', type: 'select', options: opt([[1000, '1초'], [2000, '2초'], [5000, '5초'], [10000, '10초']]) },
    { d: 'appearance', k: 'usageHistoryMinutes', label: '기록 기간', help: '이만큼 지난 기록은 지웁니다. 기록은 메모리에만 있고 앱을 끄면 사라집니다', type: 'select', options: opt([[5, '5분'], [15, '15분'], [30, '30분'], [60, '1시간'], [120, '2시간']]) },
    { type: 'action', action: 'usage-export-csv', label: '기록 내보내기', help: '다운로드 › CView 폴더에 저장합니다 — CSV는 표 계산 앱에서, JSON은 기기 정보·단위와 함께', keywords: '사용률 csv json 저장', button: 'CSV로 내보내기', button2: ['usage-export-json', 'JSON'] },
    { type: 'action', action: 'usage-clear', label: '기록 지우기', help: '지금까지 모은 사용률 기록을 지웁니다(측정은 계속됩니다)', keywords: '사용률 초기화', button: '기록 지우기' },
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
  ['general', '일반'], ['link', '연동'], ['usage', '성능'], ['advanced', '고급'], ['about', '정보'],
];
// 정보 › 업데이트 묶음의 토글 — 예전엔 일반 탭에 있었다(update-notify.js 가 읽는 general.autoCheckUpdates 그대로).
// keywords — 예전 이름('업데이트 자동 확인')으로 찾아도 나오게.
const AUTO_CHECK = { d: 'general', k: 'autoCheckUpdates', label: '자동으로 업데이트 확인', help: '실행할 때와 6시간마다 새 버전을 확인해 사이드바와 알림으로 알려 줍니다', keywords: '업데이트 자동 확인 · 자동 업데이트', type: 'toggle' };
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

const CHAT_AVATAR_MODES = [['photo', '프로필 사진 + 아이콘'], ['icon', '아이콘만(사진을 찾지 않음)'], ['off', '숨김']];
// 멀티채팅·멀티라이브 통합 목록의 줄 모양(chat.mergedChatStyle) — 멀티채팅 머리의 '촘촘|말풍선'과 같은 설정
const MERGED_CHAT_STYLES = [['compact', '촘촘 — 한 줄에 채널·닉네임·글'], ['bubble', '말풍선 — 아바타와 말풍선']];
// 후원 목록 표시(chat.donationList) — 기본은 채팅 머리의 후원 단추로 여는 목록(위쪽 띠 없음)
const DONATION_LIST_MODES = [['button', '후원 단추로 열기 — 위쪽 띠 없음'], ['strip', '위쪽 띠도 보이기(예전 방식)']];

// 이 기기에만 두는 설정(localStorage).
const LOCAL_GET = { streamAlerts: streamAlertsEnabled, thumbPreview: thumbPreviewEnabled };

function fmt(item, v) {
  if (item.pct) return `${Math.round(Number(v) * 100)}%`;
  return String(v);
}

/** 항목 하나 HTML. */
function itemHtml(item, values) {
  const v = item.local ? LOCAL_GET[item.local]() : (item.get ? item.get(values) : (values[item.d] || {})[item.k]);
  const off = item.disabled && item.disabled(values) ? ' disabled' : '';
  const attrs = item.local ? `data-local="${item.local}"` : `data-domain="${item.d}" data-key="${item.k}"`;
  // 설명 속 단축키(⌘= 등)도 Windows 표기로(감사 UX-M12)
  const label = `<span class="setting-label">${esc(item.label)}${item.help ? `<small>${esc(kbd(item.help))}</small>` : ''}</span>`;
  const search = esc(`${item.label} ${item.help || ''} ${item.keywords || ''} ${item.k || ''}`.toLowerCase());
  const wrap = (inner) => `<label class="setting${item.adv ? ' adv' : ''}" data-search="${search}">${label}${inner}</label>`;
  switch (item.type) {
    case 'action': {
      // button2: [동작, 이름] — 같은 줄의 두 번째 단추(예: CSV · JSON 내보내기).
      const second = item.button2 ? `<button class="btn" data-setting-action="${esc(item.button2[0])}">${esc(item.button2[1])}</button>` : '';
      const buttons = `<button class="btn" data-setting-action="${esc(item.action)}">${esc(item.button)}</button>${second}`;
      return `<div class="setting" data-search="${search}">${label}${second ? `<span class="shortcut-edit">${buttons}</span>` : buttons}</div>`;
    }
    case 'toggle':
      return wrap(`<span class="toggle"><input type="checkbox" ${attrs} ${v ? 'checked' : ''}${off}><span></span></span>`);
    case 'select':
      return wrap(`<select class="select" ${attrs}${off}>${item.options.map((o) => `<option value="${esc(o.value)}" ${String(o.value) === String(v) ? 'selected' : ''}>${esc(o.label)}</option>`).join('')}</select>`);
    case 'range':
      return wrap(`<span class="range-field"><input type="range" min="${item.min}" max="${item.max}" step="${item.step}" value="${esc(v)}" ${attrs} data-pct="${item.pct ? 1 : ''}"${off}><output>${esc(fmt(item, v))}</output></span>`);
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
    // 시스템 목소리 목록은 늦게 온다(첫 getVoices 가 빈 배열) — 오면 목소리 칸만 다시 채운다(감사 UX-M10)
    window.speechSynthesis?.addEventListener?.('voiceschanged', () => this.fillVoices());
    window.addEventListener('cview:sharpness', () => this.syncSharpnessControls());
    // 머리줄 사용률 팝오버의 '기록·내보내기' — 성능 탭으로 연다(검색은 닫는다).
    window.addEventListener('cview:settings-group', (e) => {
      const g = e.detail && e.detail.group;
      if (!GROUPS.some(([id]) => id === g)) return;
      this.group = g;
      this.query = '';
      if (this.settings && !this.root.hidden) this.render();
    });
    this.body.addEventListener('input', (e) => {
      if (e.target.matches('.settings-search')) this.onSearch(e.target);
      if (e.target.type === 'range') {
        const out = e.target.parentElement.querySelector('output');
        if (out) out.textContent = e.target.dataset.pct ? `${Math.round(Number(e.target.value) * 100)}%` : e.target.value;
      }
    });
    this.body.addEventListener('submit', (e) => this.onSubmit(e));
    this.body.addEventListener('click', (e) => this.onClick(e));
    // 외관이 다른 곳에서 바뀌면(⌘= ⌘- ⌘0·다른 창·가져오기·초기화) 저장된 값과 보이는 컨트롤을 맞춘다 —
    // 예전엔 화면 크기 슬라이더가 옛 값에 남아 있다가 → 키 한 번에 옛 값 기준으로 튀었다. 다시 그리지 않아 초점은 그대로.
    listen(Events.appearanceChanged, (appearance) => this.syncAppearance(appearance));
    // 사용자가 조작 중인 외관 컨트롤은 알림으로 덮지 않는다 — 저장 알림은 저장한 창에도 되돌아와서,
    // 슬라이더에서 → 를 빠르게 누르면 앞선 저장의 메아리가 값을 한 단계 되돌렸다. 드래그·저장 중·막 누른 동안은
    // 알림을 버리지 않고 키마다 마지막 것만 덮어 써 두었다가, 조작이 끝나면(놓기·초점 이동·저장 완료·정착 시간)
    // 그 값을 반영한다. Rust 는 알림을 저장 순서대로 보내므로 마지막 알림이 곧 실제(적용된) 값이다 — 값을 비교해
    // 메아리를 거르면 ⌘- 처럼 다른 곳의 변경이 우연히 보낸 값과 같을 때 버려져 화면과 어긋났다.
    this.appearanceBusy = { dragging: null, pending: new Map(), touched: new Map(), deferred: null, timer: 0 };
    this.body.addEventListener('pointerdown', (e) => {
      if (e.target.matches && e.target.matches('[data-domain="appearance"][data-key]')) this.appearanceBusy.dragging = e.target.dataset.key;
    });
    // 놓을 때 바로 반영하면 뒤따르는 change 가 미뤄 둔 값을 드래그 값 대신 저장한다 — change 다음 차례에 본다.
    const release = () => { if (this.appearanceBusy.dragging) { this.appearanceBusy.dragging = null; setTimeout(() => this.flushAppearance(), 0); } };
    window.addEventListener('pointerup', release);
    window.addEventListener('pointercancel', release);
    this.body.addEventListener('focusout', (e) => {
      if (e.target.matches && e.target.matches('[data-domain="appearance"][data-key]')) setTimeout(() => this.flushAppearance(), 0);
    });
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
      <div class="segmented page-tabs settings-tabs${this.query ? ' searching' : ''}" role="tablist" aria-label="설정 분류">${GROUPS.map(([id, l]) => {
        // 검색 중엔 모든 묶음에서 찾으므로 어느 탭도 켜진 것으로 보이지 않는다(누르면 검색을 닫고 그 묶음으로).
        const on = this.group === id; const lit = on && !this.query;
        return `<button class="seg${lit ? ' active' : ''}" data-group="${id}" role="tab" aria-selected="${lit}" tabindex="${on ? 0 : -1}">${l}</button>`;
      }).join('')}</div>
      <input type="search" class="settings-search" placeholder="설정 검색" value="${esc(this.query)}">
    </div>`;
    // 정보 탭은 한 단 — 머리말을 가운데 두고 카드를 그 아래로 쌓는다(검색 중엔 다른 탭 결과와 같이 두 단으로 흐른다).
    const about = this.group === 'about' && !this.query;
    // 성능 탭도 한 단 — 넓은 그래프 패널 아래에 측정 설정.
    const usageTab = this.group === 'usage' && !this.query;
    this.body.innerHTML = `${tabs}<div class="settings-groups${about ? ' about-tab' : ''}${usageTab ? ' usage-tab' : ''}">${this.groupHtml()}</div><div class="empty settings-search-empty" role="status" hidden></div>`;
    bindTabKeys(this.body.querySelector('.settings-tabs'));
    this.applySearch();
    this.refreshOAuth();
    if (this.body.querySelector('.update-row')) this.showUpdate();
    if (this.body.querySelector('[data-g="about"]')) fillAbout(this.body);
    const usagePanel = this.body.querySelector('[data-usage-panel]');
    if (usagePanel) mountUsagePanel(usagePanel);
    if (this.body.querySelector('[data-setting-action="clear-image-cache"]')) api.imageCacheStatus().then((st) => this.showImageCache(st)).catch(() => {});
  }

  /** 정보 › "업데이트" 줄 — 앱 이름·버전과 마지막 확인 결과, 새 버전이면 설치(안 되면 다운로드) 단추. */
  async showUpdate() {
    const row = this.body.querySelector('.update-row');
    if (!row) return;
    const u = lastUpdateResult();
    const status = row.querySelector('.update-status');
    // 확인하는 중에 다시 그렸으면(탭 이동·검색) 그 상태를 이어 보이고 단추로 또 시작하지 못하게 한다.
    // disabled 대신 aria-disabled — 누른 단추가 초점을 잃지 않는다(checkUpdateNow 가 막는다).
    const check = row.querySelector('[data-sys="update"]');
    if (this.updateChecking) status.textContent = '업데이트 서버를 확인하는 중…';
    else if (u) status.textContent = updateSummary(u);
    if (this.updateChecking) check.setAttribute('aria-disabled', 'true'); else check.removeAttribute('aria-disabled');
    row.querySelector('[data-sys="update-install"]').hidden = !canInstall(u);
    row.querySelector('[data-sys="update-download"]').hidden = !(u && u.newer && downloadTarget(u)) || canInstall(u);
    row.querySelector('[data-sys="update-page"]').hidden = !(u && u.newer && u.url);
    const info = await currentAppInfo();
    const v = row.isConnected && row.querySelector('.update-version');
    if (v) v.textContent = `CView ${info ? info.version : ''}`.trim();
  }

  /**
   * 정보 › [업데이트 확인] — 결과(최신·새 버전·실패)를 한 번만 알린다. 보이는 업데이트 줄이 있으면 그 상태 글(role=status)로,
   * 없으면(기다리는 동안 다른 탭·다른 화면으로 갔거나 검색에 걸러짐) 토스트로. 둘 다 하면 화면 낭독기가 두 번 읽었고,
   * 예전 줄(다시 그려 떨어져 나간 노드)에만 쓰면 아무도 듣지 못했다. 확인이 겹치지 않게 한 번에 하나만.
   */
  async checkUpdateNow() {
    if (this.updateChecking) return;
    this.updateChecking = true;
    this.showUpdate();
    let msg;
    let failed = false;
    try {
      msg = updateSummary(await checkForUpdate());
    } catch (er) {
      failed = true;
      msg = `확인하지 못했습니다 — ${er.message || er}`;
    } finally {
      this.updateChecking = false;
    }
    const live = this.body.querySelector('.update-row .update-status');
    const visible = Boolean(live) && !this.root.hidden && !live.closest('[hidden]');
    const shown = this.showUpdate();
    // 실패는 lastUpdateResult 에 남지 않아 showUpdate 가 옛 결과로 덮는다 — 그 뒤에 실패 글을 쓴다.
    if (failed && live) live.textContent = msg;
    if (!visible) toast(msg, 4000);
    await shown;
  }

  /** 설정 줄에서 업데이트 설치 — 상태 글씨 아래에 진행 막대. 실패하면 다운로드 단추도 보인다. */
  async installUpdate(b) {
    const row = b.closest('.update-row');
    const label = row.querySelector('.setting-label');
    row.querySelectorAll('[data-sys]').forEach((x) => { x.disabled = true; });
    label.querySelector('.update-progress')?.remove();
    label.insertAdjacentHTML('beforeend', progressHtml());
    const progress = label.querySelector('.update-progress');
    try {
      await installUpdate((st) => renderProgress(progress, st));
    } catch (er) {
      progress.remove();
      const status = row.querySelector('.update-status');
      if (status) status.textContent = `설치하지 못했습니다 — ${er.message || er}`;
      const u = lastUpdateResult();
      row.querySelector('[data-sys="update-download"]').hidden = !(u && downloadTarget(u));
      b.textContent = '다시 시도';
      row.querySelectorAll('[data-sys]').forEach((x) => { x.disabled = false; });
    }
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
      // 성능 탭 맨 위 — 실시간 그래프 패널(검색 중엔 설정 줄만).
      if (g === 'usage' && !this.query) sections.push(usagePanelHtml());
      const adv = items.some((i) => i.adv);
      sections.push(`<section class="settings-group" data-g="${g}"><h2>${esc(title)}${adv ? `<button class="link-btn adv-toggle" data-adv>${this.showAdvanced ? '고급 숨기기' : '고급 보기'}</button>` : ''}</h2>
        ${items.map((i) => itemHtml(i, this.settings)).join('')}</section>`);
      if (g === 'advanced' && title === '네트워크') {
        sections.push(`<section class="settings-group" data-g="advanced"><h2>네트워크 프리셋</h2><div class="setting" data-search="네트워크 프리셋 자동 밸런스 안정 저지연 고성능"><span class="setting-label">한 번에 맞추기<small>${esc(netPresetHelp(this.settings))}</small></span>
          <div class="segmented" role="radiogroup" aria-label="네트워크 프리셋">${NETWORK_PRESETS.map(([v, l]) => { const on = v === this.settings.networkPreset; return `<button class="seg ${on ? 'active' : ''}" data-net-preset="${v}" role="radio" aria-checked="${on}">${l}</button>`; }).join('')}</div></div></section>`);
      }
    }
    if (want('chat')) sections.push(this.chatHtml());
    if (want('general')) sections.push(this.shortcutsHtml());
    if (want('advanced')) sections.push(this.backupHtml());
    if (want('about')) {
      const u = lastUpdateResult();
      sections.push(aboutHtml({ autoCheckHtml: itemHtml(AUTO_CHECK, this.settings), updateText: u ? updateSummary(u) : '', searching: Boolean(this.query) }));
    }
    return sections.join('');
  }

  overviewHtml() {
    const auth = this.getAuth();
    const account = auth && auth.loggedIn
      ? `<div class="setting" data-search="계정 로그아웃"><span class="setting-label">${esc(auth.nickname || '로그인됨')}<small>네이버 계정 (${esc(auth.method || 'cookie')})</small></span><button class="btn" data-action="logout">로그아웃</button></div>`
      : '<div class="setting" data-search="계정 로그인"><span class="setting-label">로그인되지 않음<small>팔로잉 목록과 채팅 참여에 네이버 로그인이 필요합니다</small></span><button class="btn primary" data-action="login">로그인</button></div>';
    return `
      <section class="settings-group" data-g="overview"><h2>계정</h2>${account}</section>
      <section class="settings-group oauth-group" data-g="overview"><h2>치지직 OAuth 연동 <small class="muted">선택</small></h2>
        <div class="setting" data-search="oauth 치지직 연동 client id secret 개발자"><span class="setting-label">애플리케이션 자격 증명<small class="oauth-status">치지직 개발자 센터에 등록한 앱의 Client ID·Secret — ${SECRET_STORE_LABEL}에 저장합니다</small></span></div>
        <div class="oauth-form">
          <input type="text" name="clientId" placeholder="Client ID" autocomplete="off" spellcheck="false">
          <input type="password" name="clientSecret" placeholder="Client Secret" autocomplete="off">
          <button type="button" class="btn" data-oauth="save">저장</button>
          <button type="button" class="btn primary" data-oauth="connect">연결</button>
          <button type="button" class="link-btn" data-oauth="clear">지우기</button>
        </div>
      </section>
      <section class="settings-group" data-g="overview"><h2>화면</h2>
        <div class="setting" data-search="테마 다크 라이트 시스템">
          <span class="setting-label">테마<small>시스템은 ${IS_WINDOWS ? 'Windows' : 'macOS'} 화면 모드를 따릅니다</small></span>
          <div class="segmented" role="radiogroup" aria-label="테마">
            ${THEMES.map((t) => `<button class="seg ${t === currentTheme() ? 'active' : ''}" data-theme-choice="${t}" role="radio" aria-checked="${t === currentTheme()}">${THEME_LABELS[t]}</button>`).join('')}
          </div>
        </div>
      </section>
      <section class="settings-group" data-g="overview"><h2>앱</h2>
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
    const words = (p.blockedWords || []).map((w) => `<span class="word-chip">${esc(w)}<button data-remove-word="${esc(w)}" title="지우기" aria-label="${esc(`금지어 ${w} 지우기`)}">✕</button></span>`).join('');
    const users = (p.blockedUsers || []).map((u) => `<span class="word-chip">${esc(blockedLabel(u))}<button data-unblock="${esc(u)}" title="차단 해제">해제</button></span>`).join('');
    return `
      <section class="settings-group" data-g="chat"><h2>채팅 표시</h2>
        ${range('fontSize', '글자 크기', 11, 20, 1)}
        ${range('lineSpacing', '줄 간격', 0, 8, 1)}
        ${CHAT_TOGGLES.map(toggle).join('')}
        <label class="setting" data-search="채팅 아바타 프로필 사진 아이콘"><span class="setting-label">아바타<small>프로필 사진이 없는 시청자는 사람마다 다른 동물·캐릭터 아이콘으로 보여 줍니다</small></span>
          <select class="select" data-chat-str="chatAvatars">${CHAT_AVATAR_MODES.map(([v, l]) => `<option value="${v}" ${(p.chatAvatars || 'photo') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="멀티채팅 통합 목록 줄 모양 촘촘 말풍선 아바타"><span class="setting-label">통합 목록 줄 모양<small>멀티채팅·멀티라이브에서 여러 채널이 섞이는 통합 목록 — 말풍선은 한 화면에 보이는 메시지가 절반 이하로 줄어듭니다</small></span>
          <select class="select" data-chat-str="mergedChatStyle">${MERGED_CHAT_STYLES.map(([v, l]) => `<option value="${v}" ${(p.mergedChatStyle === 'bubble' ? 'bubble' : 'compact') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="후원 목록 표시 띠 칩 랭킹 단추"><span class="setting-label">후원 목록 표시<small>채팅 머리의 후원 단추를 누르면 후원·구독 목록이 열립니다 — 예전처럼 채팅 위쪽에 후원 칩 띠를 늘 둘 수도 있습니다</small></span>
          <select class="select" data-chat-str="donationList">${DONATION_LIST_MODES.map(([v, l]) => `<option value="${v}" ${(p.donationList === 'strip' ? 'strip' : 'button') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="최대 표시 줄 수"><span class="setting-label">최대 표시 줄 수<small>오래된 메시지부터 지웁니다</small></span>
          <select class="select" data-chat="maxVisibleMessages">${[150, 300, 500, 1000].map((n) => `<option value="${n}" ${Number(p.maxVisibleMessages) === n ? 'selected' : ''}>${n}줄</option>`).join('')}</select></label>
      </section>
      <section class="settings-group" data-g="chat"><h2>읽기 폭 · 배경</h2>
        <label class="setting" data-search="읽기 폭 채팅 너비"><span class="setting-label">읽기 폭<small>넓은 채팅 창에서 메시지 줄 길이를 제한합니다</small></span>
          <select class="select" data-chat-str="readingWidth">${READING_WIDTHS.map(([v, l]) => `<option value="${v}" ${(p.readingWidth || 'full') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="채팅 배경"><span class="setting-label">배경</span>
          <select class="select" data-chat-bg="mode">${BACKGROUNDS.map(([v, l]) => `<option value="${v}" ${(p.background || {}).mode === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="setting" data-search="배경 강도"><span class="setting-label">배경 강도</span>
          <span class="range-field"><input type="range" min="0" max="1" step="0.05" value="${esc((p.background || {}).strength ?? 0.5)}" data-chat-bg="strength"><output>${esc((p.background || {}).strength ?? 0.5)}</output></span></label>
      </section>
      <section class="settings-group" data-g="chat"><h2>후원 읽어 주기(TTS)</h2>
        ${toggle(['ttsEnabled', '후원·구독 읽어 주기', '새 후원과 구독을 소리 내어 읽습니다 (/tts 로도 켜고 끔)'])}
        ${range('ttsVolume', '음량', 0, 1, 0.05)}
        ${range('ttsRate', '속도', 100, 400, 10)}
        <label class="setting" data-search="tts 음성 목소리"><span class="setting-label">목소리</span>
          <select class="select" data-chat-str="ttsVoiceIdentifier"><option value="">기본</option>${(window.speechSynthesis ? speechSynthesis.getVoices() : []).filter((v) => v.lang.startsWith('ko')).map((v) => `<option value="${esc(v.voiceURI)}" ${p.ttsVoiceIdentifier === v.voiceURI ? 'selected' : ''}>${esc(v.name)}</option>`).join('')}</select></label>
      </section>
      <section class="settings-group" data-g="chat"><h2>필터 · 사용자 관리</h2>
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
    return `<section class="settings-group" data-g="general"><h2>키보드 단축키</h2>${rows}
      <div class="setting" data-search="단축키 키보드 목록"><span class="setting-label">전체 단축키 목록<small>${IS_WINDOWS ? kbd('창 오른쪽 위 메뉴 단추와 ⌘/ 로 볼 수 있습니다 · ⌘(Ctrl)가 들어간 단축키는 메뉴에도 반영됩니다') : kbd('메뉴 막대와 ⌘/ 로 볼 수 있습니다 · ⌘ 가 들어간 단축키는 메뉴 막대에도 반영됩니다')}</small></span><button class="btn" data-open-shortcuts>보기</button></div></section>`;
  }

  backupHtml() {
    return `<section class="settings-group" data-g="advanced"><h2>백업 · 초기화</h2>
      <div class="setting" data-search="설정 내보내기 백업"><span class="setting-label">설정 내보내기<small>모든 설정을 JSON 파일로 ${esc(saveDirPath('download'))} 에 저장합니다</small></span><button class="btn" data-export>내보내기</button></div>
      <div class="setting column" data-search="설정 가져오기 복원"><span class="setting-label">설정 가져오기<small>내보낸 JSON 내용을 붙여 넣으세요</small></span>
        <form class="word-form import-form" data-import><textarea rows="4" spellcheck="false" autocomplete="off" aria-label="가져올 설정 JSON" placeholder='{"player": …}'></textarea><button class="btn">가져오기</button></form></div>
      <div class="setting" data-search="초기화 기본값"><span class="setting-label">모든 설정 초기화<small>로그인·시청 기록은 그대로 둡니다</small></span><button class="btn danger" data-reset-all aria-haspopup="dialog">초기화…</button></div>
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

  /** 설정을 JSON 파일로 ~/Downloads/CView 에 저장하고 경로를 돌려준다. quiet 가 아니면 알림도 띄운다. */
  async exportSettings({ quiet = false } = {}) {
    const json = await api.settingsExport();
    const d = new Date();
    const path = await api.saveFile('settings', `CView-설정-${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}.json`, null, json);
    if (!quiet) toast(`내보냈습니다 — ${path}`, 4000, { label: REVEAL_LABEL, run: () => api.revealPath(path) });
    return path;
  }

  /**
   * 가져오기·전체 초기화 뒤 — 테마와 다른 화면(단축키·채팅·미리보기·플레이어)이 새 설정을 바로 따르게 알린다.
   * 예전엔 이 화면만 다시 그려 테마·채팅 설정이 앱을 다시 켤 때까지 옛 값으로 남았다.
   */
  broadcastAll() {
    const s = this.settings || {};
    syncTheme(s.appearance && s.appearance.theme);
    window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain: 'all', settings: s } }));
    loadChatPrefs().catch(() => {});
  }

  applySearch() {
    const q = this.query;
    for (const row of this.body.querySelectorAll('.setting')) {
      const adv = row.classList.contains('adv') && !this.showAdvanced && !q;
      const hit = !q || (row.dataset.search || '').includes(q);
      row.hidden = adv || !hit;
    }
    let shown = 0;
    for (const sec of this.body.querySelectorAll('.settings-group')) {
      sec.hidden = Boolean(q) && !sec.querySelector('.setting:not([hidden])');
      if (!sec.hidden) shown += 1;
    }
    // 0건 — 아래가 빈 채로 남지 않게 무엇을 찾았는지와 검색 지우기를 보인다
    const none = this.body.querySelector('.settings-search-empty');
    if (!none) return;
    none.hidden = !q || shown > 0;
    const typed = this.body.querySelector('.settings-search')?.value.trim() || q;
    none.innerHTML = none.hidden ? '' : `<strong>"${esc(typed)}"와 맞는 설정이 없습니다</strong><span>다른 낱말로 찾거나 위의 묶음에서 골라 보세요</span><button class="btn" data-settings-clear>검색 지우기</button>`;
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
    if (target.dataset.local === 'thumbPreview') {
      setThumbPreviewEnabled(target.checked);
      return;
    }
    const { domain, key } = target.dataset;
    if (!domain || !key) return;
    if (domain === 'player' && (key === 'sharpnessLevel' || key === 'sharpnessEnlargedOnly')) { await this.changeSharpness(key, target); return; }
    let value;
    if (target.type === 'checkbox') value = target.checked;
    else if (target.type === 'range' || target.type === 'number') value = Number(target.value);
    else if (target.tagName === 'SELECT' && /^\d+$/.test(target.value)) value = Number(target.value);
    else value = target.value;
    const busy = domain === 'appearance' && this.appearanceBusy;
    if (busy) {
      busy.pending.set(key, (busy.pending.get(key) || 0) + 1);
      busy.touched.set(key, Date.now());
    }
    try {
      this.settings = await api.settingsUpdate(domain, { [key]: value });
      // 저장된 값이 고른 값과 다르면(셸이 거절·보정) 화면을 저장된 값에 맞추고 성공이라 하지 않는다 — 예전엔 체크가 켜진 채 '저장했습니다'.
      const saved = (this.settings[domain] || {})[key];
      if (target.type === 'checkbox' && typeof saved === 'boolean' && saved !== value) {
        target.checked = saved;
        toast('이 설정은 지금 바꿀 수 없습니다', 2200);
      } else {
        // 외관은 응답보다 알림이 기준이다 — 미뤄 둔 마지막 알림이 있으면 정착할 때 그 값이 이 응답 값을 이긴다(flushAppearance).
        toast('설정을 저장했습니다', 1200);
      }
      this.syncDependentToggles();
      window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain, key, value, settings: this.settings } }));
    } catch (e) {
      toast(e.message);
      this.render();
    } finally {
      if (busy) {
        const n = (busy.pending.get(key) || 1) - 1;
        if (n > 0) busy.pending.set(key, n); else busy.pending.delete(key);
        this.flushAppearance();
      }
    }
  }

  fillVoices() {
    const sel = this.body.querySelector('select[data-chat-str="ttsVoiceIdentifier"]');
    if (!sel || !window.speechSynthesis) return;
    const cur = chatPrefs().ttsVoiceIdentifier || '';
    sel.innerHTML = `<option value="">기본</option>${speechSynthesis.getVoices().filter((v) => v.lang.startsWith('ko')).map((v) => `<option value="${esc(v.voiceURI)}" ${cur === v.voiceURI ? 'selected' : ''}>${esc(v.name)}</option>`).join('')}`;
  }

  /** 상위 스위치를 바꾸면 딸린 항목의 막힘을 저장된 값에 맞춘다(스키마의 disabled). 동기화 동의는 켜짐 값도 맞춘다 — 동의를 끄면 셸이 동기화도 끈다. */
  syncDependentToggles() {
    if (!this.settings) return;
    for (const [, , items] of SCHEMA) {
      for (const item of items) {
        if (!item.disabled || item.local || item.get) continue;
        const el = this.body.querySelector(`[data-domain="${item.d}"][data-key="${item.k}"]`);
        if (el) el.disabled = Boolean(item.disabled(this.settings));
      }
    }
    const sync = this.settings.sync || {};
    const el = this.body.querySelector('[data-domain="sync"][data-key="watchHistorySyncEnabled"]');
    if (el) el.checked = Boolean(sync.watchHistorySyncEnabled);
  }

  /** 이 외관 항목을 사용자가 지금 조작 중인지 — 드래그 중, 이 창의 저장이 진행 중, 또는 초점이 있고 막 바꾼 참. */
  appearanceKeyBusy(key, el) {
    const b = this.appearanceBusy;
    if (!b) return false;
    if (b.dragging === key || b.pending.get(key) > 0) return true;
    return el === document.activeElement && Date.now() - (b.touched.get(key) || 0) < APPEARANCE_SETTLE_MS;
  }

  /** 미뤄 둔 마지막 외관 알림을 조작이 끝난 컨트롤에 반영한다. 아직 조작 중이면 잠시 뒤 다시 본다. */
  flushAppearance() {
    const b = this.appearanceBusy;
    if (!b || !b.deferred) return;
    clearTimeout(b.timer);
    const pending = b.deferred;
    b.deferred = null;
    this.syncAppearance(pending);
  }

  /** 선명도 — setSharpness 로 저장한다(모든 라이브 화면에 바로 걸고 구버전 거울도 쓴다). */
  async changeSharpness(key, target) {
    const s = sharpnessState();
    const ok = key === 'sharpnessLevel'
      ? await setSharpness(Number(target.value), s.enlargedOnly)
      : await setSharpness(s.level, target.checked);
    if (ok) toast('설정을 저장했습니다', 1200);
    this.syncSharpnessControls();
  }

  /** 다른 곳(영상 ⚙·시청 화면)에서 바꾼 선명도를 이 화면의 칸에 맞춘다. */
  /** 외관 값(설정 저장 이벤트) → this.settings 와 화면의 외관 컨트롤(체크·선택·슬라이더와 % 표시). */
  syncAppearance(appearance) {
    if (!appearance || typeof appearance !== 'object' || !this.settings) return;
    const b = this.appearanceBusy;
    const apply = { ...appearance };
    let deferred = null;
    for (const el of this.body.querySelectorAll('[data-domain="appearance"][data-key]')) {
      const key = el.dataset.key;
      const v = appearance[key];
      if (v === undefined) continue;
      if (b && this.appearanceKeyBusy(key, el)) {
        // 조작 중 — 컨트롤도 this.settings 도 건드리지 않고 마지막 알림만 덮어 써 둔다(정착할 때 반영).
        delete apply[key];
        (deferred ||= {})[key] = v;
        continue;
      }
      if (b && b.deferred) delete b.deferred[key];
      if (el.type === 'checkbox') el.checked = Boolean(v);
      else el.value = String(v);
      const out = el.type === 'range' && el.parentElement.querySelector('output');
      if (out) out.textContent = el.dataset.pct ? `${Math.round(Number(v) * 100)}%` : String(v);
    }
    this.settings.appearance = { ...(this.settings.appearance || {}), ...apply };
    if (!b) return;
    if (deferred) b.deferred = { ...(b.deferred || {}), ...deferred };
    clearTimeout(b.timer);
    if (b.deferred && !Object.keys(b.deferred).length) b.deferred = null;
    if (b.deferred) b.timer = setTimeout(() => this.flushAppearance(), APPEARANCE_SETTLE_MS);
  }

  syncSharpnessControls() {
    const s = sharpnessState();
    const sel = this.body.querySelector('select[data-key="sharpnessLevel"]');
    if (sel && sel.value !== String(s.level)) sel.value = String(s.level);
    const chk = this.body.querySelector('input[data-key="sharpnessEnlargedOnly"]');
    if (chk) { chk.checked = s.enlargedOnly; chk.disabled = !s.level; }
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
        const text = form.querySelector('textarea, input').value;
        const skipped = await api.settingsImport(text);
        // 선명도 이전 전 백업(선명도 키 없음) — 가져온 예전 값(업스케일)으로 선명도를 다시 정하게 비운다.
        try {
          const imported = JSON.parse(text);
          if (imported && imported.player && typeof imported.player === 'object' && !('sharpnessLevel' in imported.player)) {
            await api.settingsUpdate('player', { sharpnessLevel: null });
          }
        } catch { /* 형식 오류는 위 가져오기가 이미 알린다 */ }
        this.settings = await api.settingsGet();
        toast(skipped.length ? `가져왔습니다 (건너뜀: ${skipped.join(', ')})` : '설정을 가져왔습니다', 2500);
        this.render();
        this.broadcastAll();
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
      // 예약 키(고정 키·메뉴·시스템)는 저장하지 않는다 — 그 키의 원래 동작이 말없이 죽는다(M-8).
      if (clash && clash.kind === 'reserved') {
        toast(`${bindingLabel(b)} 은(는) "${clash.label}"에 쓰는 고정 키라 쓸 수 없습니다 — 다른 키를 고르세요`, 4000);
        this.render();
        return;
      }
      if (clash) toast(`${clash.label} 와 겹칩니다 — 그 동작도 같은 키로 실행됩니다`, 4000);
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
    if (b.dataset.sys === 'update') { await this.checkUpdateNow(); return; }
    b.disabled = true;
    try {
      if (b.dataset.sys === 'update-install') {
        await this.installUpdate(b);
        return;
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
    if (onAboutClick(e)) return;
    const bind = e.target.closest('[data-bind]');
    if (bind) { this.captureBinding(bind); return; }
    const bindReset = e.target.closest('[data-bind-reset]');
    if (bindReset) { try { await saveBinding(bindReset.dataset.bindReset, null); this.render(); } catch (er) { toast(er.message); } return; }
    const oa = e.target.closest('[data-oauth]');
    if (oa) {
      e.preventDefault();
      // 키체인의 자격 증명 지우기 — 다시 입력하면 되지만 Secret 은 개발자 센터에서 다시 받아야 해 두 번 누르기로 묻는다
      if (oa.dataset.oauth === 'clear' && !armConfirm(oa, { what: 'OAuth 자격 증명' })) return;
      this.onOAuth(oa);
      return;
    }
    const sys = e.target.closest('[data-sys]');
    if (sys) { this.onSystem(sys); return; }
    const g = e.target.closest('[data-group]');
    if (g) {
      const keepFocus = g === document.activeElement; // ←/→ 로 고르면 다시 그린 뒤에도 탭에 초점을 둔다
      this.group = g.dataset.group; this.query = ''; this.render();
      if (keepFocus) this.body.querySelector('.settings-tabs [aria-selected="true"]')?.focus();
      return;
    }
    if (e.target.closest('[data-settings-clear]')) {
      this.query = '';
      this.render();
      this.body.querySelector('.settings-search')?.focus();
      return;
    }
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
    if (settingAction && /^usage-export-(csv|json)$/.test(settingAction.dataset.settingAction)) {
      const format = settingAction.dataset.settingAction.slice('usage-export-'.length);
      if (settingAction.getAttribute('aria-disabled') === 'true') return;
      settingAction.setAttribute('aria-disabled', 'true');
      try {
        const path = await exportUsage(format);
        toast(`내보냈습니다 — ${path}`, 4000, { label: REVEAL_LABEL, run: () => api.revealPath(path) });
      } catch (er) { toast(er.message || String(er)); } finally { settingAction.removeAttribute('aria-disabled'); }
      return;
    }
    if (settingAction && settingAction.dataset.settingAction === 'usage-clear') {
      if (!armConfirm(settingAction, { armedLabel: '한 번 더 누르면 지웁니다', what: '사용률 기록' })) return;
      try { await clearUsage(); toast('사용률 기록을 지웠습니다', 1500); } catch (er) { toast(er.message || String(er)); }
      return;
    }
    if (e.target.closest('[data-open-shortcuts]')) { window.dispatchEvent(new CustomEvent('cview:menu', { detail: 'shortcuts' })); return; }
    if (e.target.closest('[data-export]')) {
      try { await this.exportSettings(); } catch (er) { toast(er.message); }
      return;
    }
    if (e.target.closest('[data-reset-all]')) {
      // 모든 설정 영역을 기본값으로 — 범위가 넓고 앱 안에서 되돌릴 수 없어 확인 모달로 묻는다(confirm.js 규칙 4).
      // 복구 길(내보낸 파일 → 설정 가져오기)을 같이 보이고, 초기화 전에 내보내기를 기본으로 켠다.
      const btn = e.target.closest('[data-reset-all]');
      const ok = await confirmDialog({
        title: '모든 설정을 초기화할까요?',
        className: 'reset-confirm',
        confirmLabel: '초기화',
        facts: [
          ['바뀌는 것', '재생·화면·일반·채팅(금지어·차단 목록 포함)·키보드 단축키·네트워크·멀티라이브·멀티채팅·채널 알림·동기화 설정이 모두 기본값으로 돌아갑니다.'],
          ['그대로', '로그인, 시청 기록·통계, 즐겨찾기·북마크, 다운로드한 파일은 그대로 둡니다.'],
          ['복구', '초기화한 뒤에는 앱 안에서 되돌릴 수 없습니다. 내보내 둔 설정 파일이 있으면 \'설정 가져오기\'로 되돌릴 수 있습니다.'],
        ],
        check: { label: '초기화 전에 지금 설정을 내보내기', hint: `${saveDirPath('download')} 에 JSON 파일로 저장합니다 · 내보내기에 실패하면 초기화하지 않습니다` },
      });
      if (!ok) { if (btn.isConnected) btn.focus(); return; }
      let saved = null;
      if (ok.checked) {
        try { saved = await this.exportSettings({ quiet: true }); } catch (er) { toast(`내보내지 못해 초기화하지 않았습니다 — ${er.message}`, 5000); return; }
      }
      try {
        this.settings = await api.settingsReset('all');
        if (saved) toast(`내보낸 뒤 설정을 초기화했습니다 — ${saved}`, 5000, { label: REVEAL_LABEL, run: () => api.revealPath(saved) });
        else toast('설정을 초기화했습니다', 1800);
        this.render();
        this.broadcastAll();
      } catch (er) { toast(er.message); }
    }
  }
}
