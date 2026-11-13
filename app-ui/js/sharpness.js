// 선명도 — 예전 '선명한 화면'(채널별 언샤프)과 '업스케일'(전역 GPU 확대)을 합친 플레이어 설정 하나.
//
// 값: 0 끔 · 1 약 · 2 중 · 3 강 + 보조 스위치 '크게 볼 때만 적용'. 전역 플레이어 설정(Rust PlayerSettings
// `sharpnessLevel`·`sharpnessEnlargedOnly`)에 저장하고, 모든 라이브 화면(멀티라이브 그리드 패인·단독 보기·
// 예전 시청 화면)이 같은 값을 쓴다. 화면마다 다른 것은 '확대를 허용하는가'뿐이다 — 단독 보기 무대 패인과
// 시청 화면은 영상보다 크게 보이면 GPU 로 확대(업스케일)하고, 그리드 패인은 확대하지 않고 윤곽만 또렷하게 한다.
// 실제 처리(경로·패스)는 플레이어(player.html enhancePlan·setSharpness)가 정한다.
//
// 설정 이전(한 번): 저장본에 `sharpnessLevel` 이 없으면(새 판 첫 실행) 예전 값에서 정한다 — sharpnessFromLegacy.
//   선명한 화면(채널 다수결·처음 보는 채널 기본값) 또는 업스케일이 켜져 있었으면 '중', 업스케일만이었으면
//   '중 + 크게 볼 때만'(예전 업스케일과 화소가 같다), 아니면 '끔'.
// 구버전 거울: 사용자가 새 판에서 바꾸면 구버전이 읽는 값(`videoUpscaling`, localStorage `cview.picture.v1`
//   `__default.sharpen`)도 함께 써 둔다 — 구버전으로 되돌려도 비슷한 모습이고, 둘을 같이 켜는 일(이중 선명화)은 없다.
//   다시 새 판으로 올리면 스냅숏(`__default.sharpness`)으로 약·강까지 그대로 돌아온다(구버전에서 바꿨으면 그 선택을 따른다).
import { api } from './api.js';
import { toast } from './dom.js';
import { legacySharpenState, writeSharpnessMirror } from './picture.js';

export const SHARPNESS_LEVELS = [
  { value: 0, label: '끔', help: '원본 그대로 보여 줍니다.', option: '끔 — 원본 그대로' },
  { value: 1, label: '약', help: '윤곽을 살짝 또렷하게 합니다. 영상보다 크게 볼 때는 부드럽게 키우기만 합니다.', option: '약 — 살짝 또렷하게' },
  { value: 2, label: '중', help: "작게 볼 때는 치지직 '선명한 화면'과 같고, 영상보다 크게 볼 때는 업스케일로 또렷하게 키웁니다.", option: '중 — 치지직 선명한 화면 + 크게 볼 땐 업스케일' },
  { value: 3, label: '강', help: '더 또렷하게 합니다. 화면 잡티나 밝은 테두리가 눈에 띌 수 있어요.', option: '강 — 더 또렷하게(잡티가 늘 수 있음)' },
];
export const SHARPNESS_HELP = '선명한 화면과 업스케일을 하나로 합쳤어요. 영상보다 크게 볼 때(단독 보기·시청 화면)는 GPU 로 키우고, 그 밖에는 윤곽을 또렷하게 합니다. 모든 라이브 화면에 함께 적용됩니다.';
export const ENLARGED_ONLY_LABEL = '크게 볼 때만 적용';
export const ENLARGED_ONLY_HELP = '단독 보기·시청 화면에서 영상보다 크게 볼 때만 처리합니다. 여러 패인 보기와 작은 창은 원본 그대로 — GPU 를 아낍니다.';
const NOTICE_KEY = 'cview.sharpness.notice.v1';

const clampLevel = (v) => Math.max(0, Math.min(3, Math.round(Number(v) || 0)));
let state = { level: 0, enlargedOnly: false };
let ready = null;

/** 지금 선명도(단계·크게 볼 때만). */
export function sharpnessState() {
  return { ...state };
}

/** 처음 값을 읽었는가를 기다린다(부팅 때 initSharpness 가 시작). 시작 전이면 지금 저장값을 읽는다. */
export function sharpnessReady() {
  if (!ready) ready = adopt(null, { notice: false });
  return ready;
}

/** 이 화면의 플레이어에 보낼 setSharpness 인자 — stage = 확대를 허용하는 화면(단독 보기 무대·시청 화면). */
export function sharpnessArgs(stage) {
  return [state.level, { upscale: Boolean(stage), enlargedOnly: state.enlargedOnly }];
}

/** 이름표 — '중', '중 · 크게 볼 때만'. */
export function sharpnessLabel(s = state) {
  const name = (SHARPNESS_LEVELS[s.level] || SHARPNESS_LEVELS[0]).label;
  return s.level && s.enlargedOnly ? `${name} · 크게 볼 때만` : name;
}

/**
 * 순수 함수 — 예전 설정에서 선명도를 정한다(검사가 직접 부른다).
 * @param {{ on?: number, off?: number, def?: boolean, upscale?: boolean, snapshot?: object|null }} legacy
 *   on/off = 선명한 화면을 켠·끈 채널 수(채널 값이 있는 것만), def = 처음 보는 채널 기본값(`__default.sharpen`),
 *   upscale = 예전 '업스케일'(Rust videoUpscaling), snapshot = 새 판이 써 둔 `__default.sharpness`.
 */
export function sharpnessFromLegacy({ on = 0, off = 0, def, upscale = false, snapshot = null } = {}) {
  const u = Boolean(upscale);
  // 새 판 → 구버전 → 새 판에서 구버전이 아무것도 바꾸지 않았으면(거울이 그대로·채널 값 없음) 스냅숏 그대로(약·강 포함).
  if (snapshot && typeof snapshot === 'object' && Number.isFinite(snapshot.level) && on === 0 && off === 0
      && snapshot.d === Boolean(def) && snapshot.u === u) {
    return { level: clampLevel(snapshot.level), enlargedOnly: Boolean(snapshot.enlargedOnly), from: 'snapshot', notice: false, mixed: false };
  }
  // 채널 다수결 — 동수면 처음 보는 채널 기본값(없으면 켠 채널이 있을 때 켬).
  const s = on > off ? true : (on < off ? false : (def === true || (def === undefined && on > 0)));
  const level = s || u ? 2 : 0;
  return { level, enlargedOnly: !s && u, from: 'legacy', notice: s || u, mixed: on > 0 && off > 0 };
}

/** 구버전 거울 값 — 둘을 같이 켜지 않는다(구버전에서 이중 선명화가 생기지 않게). */
export function legacyMirror(level, enlargedOnly) {
  const on = clampLevel(level) >= 1;
  return { d: on && !enlargedOnly, u: on && Boolean(enlargedOnly) };
}

function apply(level, enlargedOnly) {
  const next = { level: clampLevel(level), enlargedOnly: Boolean(enlargedOnly) };
  const changed = next.level !== state.level || next.enlargedOnly !== state.enlargedOnly;
  state = next;
  if (changed) window.dispatchEvent(new CustomEvent('cview:sharpness', { detail: sharpnessState() }));
  return changed;
}

/**
 * 저장된 플레이어 설정을 받아들인다 — 값이 있으면 그대로, 없으면(새 판 첫 실행·예전 백업 가져오기) 예전 값에서 이전.
 * @param {object|null} player settings_get().player (없으면 지금 읽는다)
 */
async function adopt(player, { notice = false } = {}) {
  if (!player) {
    try { player = (await api.settingsGet()).player || {}; } catch { player = {}; }
  }
  if (typeof player.sharpnessLevel === 'number') {
    apply(player.sharpnessLevel, player.sharpnessEnlargedOnly);
    // 새 판에서 바꾼 적이 있으면(스냅숏이 있으면) 구버전 거울을 저장값에 맞춰 둔다(초기화·가져오기·다른 창).
    const { snapshot } = legacySharpenState();
    if (snapshot && (snapshot.level !== state.level || snapshot.enlargedOnly !== state.enlargedOnly)) {
      writeSharpnessMirror({ level: state.level, enlargedOnly: state.enlargedOnly, ...legacyMirror(state.level, state.enlargedOnly) });
    }
    return sharpnessState();
  }
  const legacy = legacySharpenState();
  const r = sharpnessFromLegacy({ ...legacy, upscale: player.videoUpscaling });
  apply(r.level, r.enlargedOnly);
  // 한 번만 쓴다 — 다음부터는 Rust 에 값이 있다. 예전 키(채널 값·기본값·videoUpscaling)는 건드리지 않는다.
  try { await api.settingsUpdate('player', { sharpnessLevel: r.level, sharpnessEnlargedOnly: r.enlargedOnly }); } catch { /* 이번 실행은 메모리 값으로 */ }
  if (notice && r.notice) showMigrationNotice(r);
  return sharpnessState();
}

function showMigrationNotice(r) {
  try {
    if (localStorage.getItem(NOTICE_KEY)) return;
    localStorage.setItem(NOTICE_KEY, String(Date.now()));
  } catch { return; }
  const pre = r.mixed ? '채널마다 다르던 선명한 화면 설정을 하나로 합쳤어요. ' : '';
  toast(`${pre}선명한 화면과 업스케일이 '선명도' 하나로 합쳐졌어요 — 지금 '${sharpnessLabel()}'. 영상 ⚙ 에서 바꿀 수 있어요`, 6000);
}

/**
 * 부팅 때 한 번(app.js) — 받은 설정으로 선명도를 정하고 필요하면 이전한다.
 * 다른 창·설정 화면에서 바꾼 값은 창 포커스·'cview:settings-changed' 때 다시 읽는다.
 */
export function initSharpness(player, { notice = true } = {}) {
  ready = adopt(player || null, { notice });
  return ready;
}

/** 사용자가 바꿨을 때 — 모든 라이브 화면에 바로 다시 걸고(이벤트), 저장하고, 구버전 거울도 써 둔다. */
export async function setSharpness(level, enlargedOnly = state.enlargedOnly) {
  const prev = sharpnessState();
  const next = { level: clampLevel(level), enlargedOnly: Boolean(enlargedOnly) };
  const mirror = legacyMirror(next.level, next.enlargedOnly);
  apply(next.level, next.enlargedOnly);
  writeSharpnessMirror({ ...next, ...mirror });
  try {
    await api.settingsUpdate('player', { sharpnessLevel: next.level, sharpnessEnlargedOnly: next.enlargedOnly, videoUpscaling: mirror.u });
    return true;
  } catch (e) {
    apply(prev.level, prev.enlargedOnly);
    writeSharpnessMirror({ ...prev, ...legacyMirror(prev.level, prev.enlargedOnly) });
    toast(`선명도를 저장하지 못했어요${e && e.message ? ` — ${e.message}` : ''}`);
    return false;
  }
}

// 다른 창(보조 창)·설정 화면에서 바꾼 값 — 창이 다시 앞으로 올 때 저장값을 읽어 맞춘다(storage 이벤트에 기대지 않는다).
let lastFocusRead = 0;
window.addEventListener('focus', () => {
  const now = Date.now();
  if (now - lastFocusRead < 1500) return;
  lastFocusRead = now;
  api.settingsGet().then((s) => adopt((s && s.player) || {}, { notice: false })).catch(() => {});
});
window.addEventListener('cview:settings-changed', (e) => {
  const d = e.detail || {};
  if ((d.domain === 'player' || d.domain === 'all') && d.settings && d.settings.player) adopt(d.settings.player, { notice: false });
});

// ── 상태 줄 — 플레이어의 'sharpness' 알림 { level, path, reason, videoHeight, outHeight } ──

/** 메뉴 상태 줄 문구(지금 무엇을 하고 있는지·왜 쉬는지). */
export function sharpnessNote(msg, s = state) {
  if (!s.level) return '꺼짐 — 원본 그대로';
  if (!msg) return '영상을 기다리는 중';
  const vh = msg.videoHeight, oh = msg.outHeight;
  switch (msg.path) {
    case 'upscale': return `지금: 업스케일 ${vh}p → ${oh}p${s.level === 1 ? ' (부드럽게)' : ''}`;
    case 'sharpen': return msg.reason === 'grid'
      ? '지금: 윤곽 선명화 — 여러 패인 보기에서는 업스케일하지 않아요'
      : '지금: 윤곽 선명화 — 영상보다 1.15배 이상 크게 보면 업스케일해요';
    case 'sharpen-css': return '지금: 윤곽 선명화(간단 모드 — GPU 를 쓸 수 없음)';
    default: break;
  }
  switch (msg.reason) {
    case 'not-enlarged': return '쉬는 중 — 영상보다 1.15배 이상 크게 볼 때만 처리해요';
    case 'grid-enlarged-only': return "쉬는 중 — '크게 볼 때만'이라 여러 패인 보기에서는 원본 그대로예요";
    case 'gpu-unavailable': return '쉬는 중 — 이 환경에서는 GPU 처리를 쓸 수 없어요';
    case 'hidden': return '쉬는 중 — 화면에 보이지 않아요';
    case 'off': return '꺼짐 — 원본 그대로';
    default: return '영상을 기다리는 중';
  }
}

/** ⚙ 단추 title 에 붙일 짧은 상태. */
export function sharpnessShort(msg, s = state) {
  if (!s.level) return '끔';
  const st = msg && msg.path === 'upscale' ? '지금 업스케일' : (msg && (msg.path === 'sharpen' || msg.path === 'sharpen-css') ? '지금 윤곽 선명화' : '쉬는 중');
  return `${sharpnessLabel(s)} · ${st}`;
}
