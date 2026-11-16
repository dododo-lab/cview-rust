// 멀티라이브에서 볼 채널 고르기 — 라이브 단독 보기에는 멀티라이브 상한보다 많이 담을 수 있다. 그 채로 멀티라이브로
// 오면 상한까지만 함께 재생한다(Rust `multi_view_session_ids` — 보던 채널, 고른 채널, 최근에 본 순). 여기서 어떤 채널을
// 올릴지 고르거나 바꾼다. 고르지 않은 채널은 닫지 않고 라이브 머리줄(단독 보기 채널 고르기)에 그대로 남는다.
// 머리줄에는 따로 단추가 없다 — 멀티라이브 채널 추가 단추가 상한까지 차면 같은 자리에서 "채널 고르기"로 바뀐다
// (syncHeaderAddButton). 딱 상한만큼이라 고를 대상이 없으면 시트는 "빼고 채널 추가"(바꾸기)로 열린다.
import { esc, safeUrl, imageSrc, dismiss } from './dom.js';
import { trapFocus } from './a11y.js';
import { verifiedBadge } from './verified-badge.js';

/** 멀티라이브 보기에 올라간 세션 — 상한을 넘지 않았거나 라이브 단독 보기면 모두. */
export function visibleSessions(state) {
  return ((state && state.sessions) || []).filter((s) => s.inMultiView !== false);
}

/** 상한을 넘어 일부만 보이는가와 그 수 — 머리줄 고르기 단추·안내가 쓴다. */
export function viewLimit(state) {
  const total = ((state && state.sessions) || []).length;
  const shown = visibleSessions(state).length;
  const max = (state && (state.maxMultiSessions ?? state.maxSessions)) || shown;
  return { limited: !(state && state.watchId) && shown < total, shown, total, max };
}

const ICON_PICK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4.5" width="7" height="6" rx="1.4"/><rect x="13.5" y="4.5" width="7" height="6" rx="1.4"/><rect x="3.5" y="13.5" width="7" height="6" rx="1.4"/><path d="m14.2 16.6 2 2 3.6-4"/></svg>';

/**
 * 머리줄 "채널 추가" 단추가 "채널 고르기"로 바뀌는가 — 멀티라이브 보기(단독 보기가 아님)에서 상한까지 다 찼거나,
 * 라이브 단독 보기에 상한보다 많이 담아 와 일부만 보일 때. 추가 중이라 잠깐 막힌 것(세션 수 < 상한)은 아니다.
 */
export function headerAddMode(state) {
  const limit = viewLimit(state);
  const pick = !(state && state.watchId) && limit.total > 0 && (limit.limited || limit.total >= limit.max);
  return { ...limit, pick };
}

const PICK_INNER = `${ICON_PICK}<span class="ml-add-label"><span class="ml-add-long">채널 </span>고르기</span><span class="ml-add-full" aria-hidden="true"></span>`;

/**
 * 머리줄 채널 추가 단추를 지금 상태에 맞춘다 — 가득 차면 같은 자리·같은 단추가 "채널 고르기"(data-ml="pick")로 바뀌고,
 * 자리가 나면 "채널 추가"(data-ml="add", `addInner`)로 돌아온다. 같은 요소라 초점이 그대로 남는다.
 * 고르기일 때 true — 그때는 aria-disabled·가득 참 배지 같은 추가 단추 처리를 하지 않는다(multilive.js).
 * @param {HTMLButtonElement} button @param {object} state @param {{ addInner: string, drawerOpen: boolean }} opts
 */
export function syncHeaderAddButton(button, state, { addInner, drawerOpen }) {
  const m = headerAddMode(state);
  const mode = m.pick ? 'pick' : 'add';
  if (button.dataset.ml !== mode) {
    button.dataset.ml = mode;
    // 고르기는 추가 단추(브랜드색 키·열림 ×·가득 참 점선)가 아니라 머리줄의 다른 유리 단추와 같은 모양이다.
    button.classList.toggle('primary', !m.pick);
    button.classList.toggle('ch-add', !m.pick);
    button.classList.toggle('ml-add-pick', m.pick);
    button.innerHTML = m.pick ? PICK_INNER : addInner;
    button.removeAttribute('aria-disabled');
    if (m.pick) {
      button.setAttribute('aria-haspopup', 'dialog');
      button.removeAttribute('aria-expanded');
    } else {
      button.removeAttribute('aria-haspopup');
      button.setAttribute('aria-expanded', String(Boolean(drawerOpen)));
    }
  }
  if (!m.pick) return false;
  const n = m.limited ? `${m.shown}/${m.total}` : `${m.total}/${m.max}`;
  const badge = button.querySelector('.ml-add-full');
  if (badge.textContent !== n) badge.textContent = n;
  const label = m.limited
    ? `채널 고르기 — 라이브에 담은 ${m.total}개 중 ${m.shown}개 재생 중`
    : `채널 고르기 — 가득 참 ${m.total}/${m.max}, 채널을 빼고 다른 채널로 바꿀 수 있어요`;
  if (button.getAttribute('aria-label') !== label) {
    button.setAttribute('aria-label', label);
    button.title = m.limited ? `라이브에 담은 ${m.total}개 중 ${m.shown}개 재생 중 — 볼 채널 고르기` : `가득 찼어요(최대 ${m.max}개) — 뺄 채널을 골라 다른 채널로 바꾸기`;
  }
  return true;
}

/**
 * 고르기 시트를 연다. 두 가지 경우가 있다.
 * - 일부만 보임(라이브에 상한보다 많이 담아 옴): 고른 세션 ID(목록 순서)를 `onApply` 로 넘긴다 — 바뀐 게 없으면 부르지 않는다.
 *   상한만큼 골랐으면 고르지 않은 줄은 막히고(aria-disabled) 하나를 빼면 다시 고를 수 있다(빼고 넣어 바꾸기).
 * - 딱 상한만큼(가득 참): 숨은 채널이 없어 고를 대상이 없다. 그래서 "바꾸기"로 연다 — 뺄 채널의 체크를 끄면
 *   "빼고 채널 추가"가 켜지고, 누르면 뺀 세션 ID 를 `onSwap` 으로 넘긴다(그 채널을 닫고 채널 추가 사이드바를 연다).
 * `opener` — 닫으면 초점을 돌려줄 단추. WebKit(macOS WKWebView)은 마우스로 누른 단추에 초점을 주지 않아
 * activeElement 로는 찾을 수 없다(예전엔 마우스로 열고 Esc 로 닫으면 초점이 body 로 사라졌다).
 * @param {{ state: object, onApply: (ids: string[]) => unknown, onSwap?: (removed: string[]) => unknown, opener?: HTMLElement | null }} opts
 */
export function openViewPicker({ state, onApply, onSwap = null, opener = null }) {
  document.querySelector('.sheet-scrim.ml-pick-scrim')?.remove();
  const sessions = (state.sessions || []).slice();
  const { max, total, limited } = viewLimit(state);
  const swap = !limited && Boolean(onSwap);
  const before = new Set(visibleSessions(state).map((s) => s.id));
  const scrim = document.createElement('div');
  scrim.className = 'sheet-scrim ml-pick-scrim';
  const rows = sessions.map((s) => {
    const name = s.channelName || s.channelId;
    const avatar = safeUrl(s.profileImageUrl);
    const img = avatar ? `<img loading="lazy" decoding="async" src="${esc(imageSrc(avatar, 'avatar'))}" alt="" referrerpolicy="no-referrer">` : '';
    const watching = s.id === state.selectedId ? '<span class="ml-pick-tag">채팅·소리</span>' : '';
    return `<label class="ml-pick-row" data-session-id="${esc(s.id)}">
        <input type="checkbox" value="${esc(s.id)}" ${before.has(s.id) ? 'checked' : ''}>
        <span class="ml-pick-avatar" aria-hidden="true">${img}</span>
        <span class="ml-pick-text"><span class="ml-pick-name"><span class="ellipsis">${esc(name)}</span>${verifiedBadge(s.verifiedBadge)}</span><span class="ml-pick-title ellipsis">${esc(s.liveTitle || '')}</span></span>
        ${watching}
      </label>`;
  }).join('');
  scrim.innerHTML = `<form class="sheet ml-pick-sheet" role="dialog" aria-modal="true" aria-labelledby="ml-pick-title" aria-describedby="ml-pick-desc" tabindex="-1">
      <h2 id="ml-pick-title">멀티라이브에서 볼 채널</h2>
      <p id="ml-pick-desc" class="ml-pick-desc">${swap
    ? `멀티라이브는 최대 ${max}개까지 함께 재생해요. 다른 채널을 보려면 뺄 채널의 체크를 끄고 새 채널을 추가하세요. 뺀 채널은 닫혀요.`
    : `라이브에 담은 ${total}개 채널 중 최대 ${max}개를 함께 재생해요. 고르지 않은 채널은 닫히지 않고 라이브 머리줄에 남아요.`}</p>
      <fieldset class="ml-pick-list"><legend class="sr-only">채널 (최대 ${max}개)</legend>${rows}</fieldset>
      <p class="ml-pick-status" role="status" aria-live="polite"></p>
      <div class="sheet-actions"><button type="button" class="btn" data-pick-cancel>취소</button><button type="submit" class="btn primary" data-pick-apply>${swap ? '빼고 채널 추가' : '이대로 보기'}</button></div>
    </form>`;
  document.body.append(scrim);
  const form = scrim.querySelector('form');
  const status = scrim.querySelector('.ml-pick-status');
  const apply = scrim.querySelector('[data-pick-apply]');
  const boxes = [...scrim.querySelectorAll('input[type="checkbox"]')];
  const picked = () => boxes.filter((b) => b.checked).map((b) => b.value);
  const sync = (message = '') => {
    const n = picked().length;
    const full = n >= max;
    for (const b of boxes) {
      const blocked = full && !b.checked;
      if (blocked) b.setAttribute('aria-disabled', 'true'); else b.removeAttribute('aria-disabled');
      b.closest('.ml-pick-row').classList.toggle('blocked', blocked);
      b.closest('.ml-pick-row').classList.toggle('checked', b.checked);
    }
    status.classList.toggle('warn', Boolean(message));
    if (swap) {
      const out = boxes.length - n;
      apply.disabled = out === 0;
      status.textContent = message || (out === 0 ? `${n} / ${max}개 — 다른 채널로 바꾸려면 뺄 채널의 체크를 꺼 주세요` : `${out}개를 빼고 새 채널을 추가해요`);
      return;
    }
    apply.disabled = n === 0;
    status.textContent = message || (n === 0 ? '볼 채널을 하나 이상 골라 주세요' : `${n} / ${max}개 선택${full ? ' — 바꾸려면 하나를 먼저 빼 주세요' : ''}`);
  };
  sync();
  // 가득 찼을 때 고르지 않은 줄을 누르면 체크하지 않고 이유를 알린다(키보드 Space 도 click 으로 온다).
  form.addEventListener('click', (e) => {
    const box = e.target.closest('input[type="checkbox"]');
    if (box && box.checked && picked().length > max) {
      e.preventDefault();
      sync(`최대 ${max}개까지예요 — 바꾸려면 다른 채널을 먼저 빼 주세요`);
    }
  });
  form.addEventListener('change', () => sync());

  const returnTo = opener || document.activeElement;
  const release = trapFocus(form, { restore: false });
  (boxes.find((b) => b.checked) || boxes[0] || apply).focus({ preventScroll: true });
  const close = () => {
    release();
    dismiss(scrim);
    if (returnTo?.isConnected) returnTo.focus({ preventScroll: true });
  };
  scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); } });
  scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-pick-cancel]')) close(); });
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const ids = picked();
    if (swap) {
      const out = boxes.filter((b) => !b.checked).map((b) => b.value);
      if (!out.length) return;
      close();
      onSwap(out);
      return;
    }
    if (!ids.length) return;
    close();
    if (ids.length !== before.size || ids.some((id) => !before.has(id))) onApply(ids);
  });
  return scrim;
}

