// 지우기·초기화 확인 — 앱 전체가 같은 규칙으로 묻는다(P3-4). 자세한 표: docs/UI_CONFIRM_RULES.md
//
// 규칙(무엇을 잃는지와 앱 안에서 되돌릴 수 있는지로 고른다):
//   1. 확인 없음      — 다시 받아 오는 캐시, 화면에만 있는 것, 한 번 더 누르면 되돌아가는 토글, 손으로 바로 다시 넣는 한 항목.
//   2. 되돌리기 토스트 — 사용자가 만든 한 항목(북마크·프리셋). 바로 숨기고 5초 동안 '되돌리기'를 준 뒤 지운다. deferDelete
//   3. 두 번 누르기    — 되돌릴 수 없지만 가볍고 쓰다 보면 다시 쌓이는 목록 전체(최근 검색어·다운로드 기록)와
//                       다시 입력하면 되는 자격 증명. 단추가 '한 번 더 누르면 …'으로 바뀌고 3초 안에 다시 눌러야 한다. armConfirm
//   4. 확인 모달       — 앱 저장소(시청 기록 DB·설정 파일)를 통째로 바꾸고 앱 안에서 되돌릴 수 없는 것.
//                       대상·영향·복구 방법을 보이고, 지우기 전에 백업(내보내기)을 함께 묻는다. 단추 글자 끝에 '…'. confirmDialog
//   같은 데이터는 어디서 지우든 같은 방식으로 묻는다(최근 시청: 보관함·통계 둘 다 모달).
import { esc, dismiss, toast } from './dom.js';
import { trapFocus } from './a11y.js';

/** 두 번 누르기 — 다시 누를 수 있는 시간. */
export const ARM_MS = 3000;
/** 두 번 누르기 — 첫 누름 뒤 이 시간 안에 온 누름은 무시한다(더블클릭·연타가 그대로 확인이 되지 않게). */
export const ARM_GUARD_MS = 400;
/** 되돌리기 토스트 — 지우기 전에 기다리는 시간. */
export const UNDO_MS = 5000;

const armed = new WeakMap();

/** 화면 낭독기 알림 자리 — 두 번 누르기 상태를 바로 읽어 준다(단추 글자만 바뀌면 VoiceOver 가 읽지 않는다). */
function announce(text) {
  let live = document.getElementById('confirm-live');
  if (!live) {
    live = document.createElement('div');
    live.id = 'confirm-live';
    live.className = 'sr-only';
    live.setAttribute('aria-live', 'assertive');
    live.setAttribute('aria-atomic', 'true');
    document.body.append(live);
  }
  live.textContent = text;
}

/** 두 번 누르기 상태를 푼다(시간 끝·Esc·바깥 누름·초점 이탈·확인). 글자·이름을 되돌리고 듣던 이벤트를 뗀다. */
export function disarm(btn) {
  const st = armed.get(btn);
  if (!st) return;
  armed.delete(btn);
  clearTimeout(st.timer);
  document.removeEventListener('keydown', st.onKey, true);
  document.removeEventListener('pointerdown', st.onDown, true);
  btn.removeEventListener('focusout', st.onBlur);
  delete btn.dataset.armed;
  btn.textContent = st.label;
  if (st.aria == null) btn.removeAttribute('aria-label'); else btn.setAttribute('aria-label', st.aria);
  const live = document.getElementById('confirm-live');
  if (live) live.textContent = '';
}

/**
 * 두 번 누르기 — 클릭 처리기 맨 앞에서 부른다. 이번 누름이 확인이면 true(이제 지운다), 아니면 false(멈춘다).
 *   if (!armConfirm(btn, { what: '최근 검색어' })) return;
 * 글자만 있는 단추에 쓴다(풀 때 textContent 를 되돌린다). 첫 누름: 단추가 armedLabel 로 바뀌고 낭독기에 알린다. ms 안에 다시 누르면 확인.
 * 시간이 지나거나 Esc·단추 밖을 누르거나 초점이 떠나면 풀린다. 첫 누름 직후(ARM_GUARD_MS)의 연타·키 반복은 무시한다.
 * @param {HTMLElement} btn
 * @param {{ armedLabel?: string, what?: string, ms?: number }} [opts]
 */
export function armConfirm(btn, { armedLabel = '한 번 더 누르면 지웁니다', what = '', ms = ARM_MS } = {}) {
  const st = armed.get(btn);
  if (st) {
    if (performance.now() - st.at < ARM_GUARD_MS) return false;
    disarm(btn);
    return true;
  }
  const next = {
    at: performance.now(),
    label: btn.textContent,
    aria: btn.getAttribute('aria-label'),
    timer: setTimeout(() => disarm(btn), ms),
    onKey: (e) => {
      if (e.key === 'Escape') {
        disarm(btn);
        if (e.target === btn) { e.preventDefault(); e.stopPropagation(); }
      } else if (e.target === btn && e.repeat && (e.key === 'Enter' || e.key === ' ')) {
        e.preventDefault(); // 누르고 있는 Enter 의 반복 입력이 확인이 되지 않게
      }
    },
    onDown: (e) => { if (!btn.contains(e.target)) disarm(btn); },
    onBlur: (e) => { if (!btn.contains(e.relatedTarget)) disarm(btn); },
  };
  armed.set(btn, next);
  btn.dataset.armed = '1';
  btn.textContent = armedLabel;
  if (next.aria != null) btn.setAttribute('aria-label', armedLabel);
  document.addEventListener('keydown', next.onKey, true);
  document.addEventListener('pointerdown', next.onDown, true);
  btn.addEventListener('focusout', next.onBlur);
  announce(`${what ? `${what} — ` : ''}${Math.round(ms / 1000)}초 안에 ${armedLabel}. Esc 로 취소합니다.`);
  return false;
}

let dialogSeq = 0;

/**
 * 확인 모달 — 취소·Esc·바깥 클릭이면 null, 확인이면 { checked } (확인 상자를 두었을 때 체크 여부).
 * 초점은 처음에 '취소'에 둔다(Enter 한 번으로 지워지지 않게). 닫히면 연 단추로 초점을 돌려준다.
 * @param {{ title: string, facts: [string, string][], check?: { label: string, hint?: string, checked?: boolean },
 *           confirmLabel?: string, className?: string }} opts  facts — [항목 이름, 설명] (지울 대상·영향·복구)
 */
export function confirmDialog({ title, facts = [], check = null, confirmLabel = '지우기', className = '' }) {
  const id = `confirm-${++dialogSeq}`;
  return new Promise((resolve) => {
    const scrim = document.createElement('div');
    scrim.className = `sheet-scrim confirm-sheet${className ? ` ${className}` : ''}`;
    scrim.innerHTML = `<form class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="${id}-title" aria-describedby="${id}-desc">
      <h2 id="${id}-title">${esc(title)}</h2>
      <dl class="confirm-facts" id="${id}-desc">${facts.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl>
      ${check ? `<label class="confirm-check"><span class="toggle"><input type="checkbox" data-confirm-check${check.checked === false ? '' : ' checked'}><span></span></span>
        <span>${esc(check.label)}${check.hint ? ` <small>${esc(check.hint)}</small>` : ''}</span></label>` : ''}
      <div class="sheet-actions"><button type="button" class="btn" data-cancel>취소</button><button type="submit" class="btn danger">${esc(confirmLabel)}</button></div>
    </form>`;
    document.body.append(scrim);
    const form = scrim.querySelector('form');
    const release = trapFocus(form); // 초점을 가두고, 닫으면 연 단추로 돌려준다
    let done = false;
    const finish = (value) => {
      if (done) return;
      done = true;
      release();
      dismiss(scrim);
      resolve(value);
    };
    scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-cancel]')) finish(null); });
    scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(null); } });
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      finish({ checked: Boolean(form.querySelector('[data-confirm-check]')?.checked) });
    });
    scrim.querySelector('[data-cancel]').focus();
  });
}

/**
 * 되돌리기 토스트 — 한 항목을 바로 숨기고(hide) '되돌리기'를 준 뒤, 시간이 지나면 지운다(commit).
 * 되돌리거나 지우기가 실패하면 restore 로 다시 보인다.
 * @param {{ message: string, hide: () => void, restore: () => void, commit: () => Promise<void>, ms?: number }} opts
 */
export function deferDelete({ message, hide, restore, commit, ms = UNDO_MS }) {
  hide();
  let undone = false;
  toast(message, ms, { label: '되돌리기', run: () => { undone = true; restore(); } });
  setTimeout(async () => {
    if (undone) return;
    try { await commit(); } catch (er) { restore(); toast(er.message || String(er)); }
  }, ms + 200);
}
