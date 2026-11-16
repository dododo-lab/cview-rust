// 멀티라이브에서 볼 채널 고르기 — 라이브 단독 보기에는 멀티라이브 상한보다 많이 담을 수 있다. 그 채로 멀티라이브로
// 오면 상한까지만 함께 재생한다(Rust `multi_view_session_ids` — 보던 채널, 고른 채널, 최근에 본 순). 여기서 어떤 채널을
// 올릴지 고르거나 바꾼다. 고르지 않은 채널은 닫지 않고 라이브 머리줄(단독 보기 채널 고르기)에 그대로 남는다.
import { esc, safeUrl, imageSrc, dismiss } from './dom.js';
import { trapFocus } from './a11y.js';
import { verifiedBadge } from './verified-badge.js';

/** 멀티라이브 보기에 올라간 세션 — 상한을 넘지 않았거나 라이브 단독 보기면 모두. */
export function visibleSessions(state) {
  return ((state && state.sessions) || []).filter((s) => s.inMultiView !== false);
}

/** 상한을 넘어 일부만 보이는가와 그 수 — 고르기 단추·안내가 쓴다. */
export function viewLimit(state) {
  const total = ((state && state.sessions) || []).length;
  const shown = visibleSessions(state).length;
  const max = (state && (state.maxMultiSessions ?? state.maxSessions)) || shown;
  return { limited: !(state && state.watchId) && shown < total, shown, total, max };
}

const ICON_PICK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4.5" width="7" height="6" rx="1.4"/><rect x="13.5" y="4.5" width="7" height="6" rx="1.4"/><rect x="3.5" y="13.5" width="7" height="6" rx="1.4"/><path d="m14.2 16.6 2 2 3.6-4"/></svg>';

/** 머리줄 단추 — 상한을 넘었을 때만 보인다(multilive.js 가 hidden 을 맞춘다). */
export function viewPickerButtonHtml() {
  return `<button class="btn ml-pick" data-ml="pick" aria-haspopup="dialog" hidden>${ICON_PICK}<span class="ml-pick-label">볼 채널 고르기</span><span class="ml-pick-n"></span></button>`;
}

/** 단추의 수·이름을 지금 상태로. */
export function syncViewPickerButton(button, state) {
  const { limited, shown, total } = viewLimit(state);
  button.hidden = !limited;
  if (!limited) return;
  const n = `${shown}/${total}`;
  const nEl = button.querySelector('.ml-pick-n');
  if (nEl.textContent !== n) nEl.textContent = n;
  const label = `멀티라이브에서 볼 채널 고르기 — 라이브에 담은 ${total}개 중 ${shown}개 재생 중`;
  if (button.getAttribute('aria-label') !== label) { button.setAttribute('aria-label', label); button.title = label; }
}

/**
 * 고르기 시트를 연다. 고른 세션 ID(목록 순서)를 `onApply` 로 넘긴다 — 바뀐 게 없으면 부르지 않는다.
 * 상한만큼 골랐으면 고르지 않은 줄은 막히고(aria-disabled) 하나를 빼면 다시 고를 수 있다(빼고 넣어 바꾸기).
 * @param {{ state: object, onApply: (ids: string[]) => unknown }} opts
 */
export function openViewPicker({ state, onApply }) {
  document.querySelector('.sheet-scrim.ml-pick-scrim')?.remove();
  const sessions = (state.sessions || []).slice();
  const { max, total } = viewLimit(state);
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
      <p id="ml-pick-desc" class="ml-pick-desc">라이브에 담은 ${total}개 채널 중 최대 ${max}개를 함께 재생해요. 고르지 않은 채널은 닫히지 않고 라이브 머리줄에 남아요.</p>
      <fieldset class="ml-pick-list"><legend class="sr-only">채널 (최대 ${max}개)</legend>${rows}</fieldset>
      <p class="ml-pick-status" role="status" aria-live="polite"></p>
      <div class="sheet-actions"><button type="button" class="btn" data-pick-cancel>취소</button><button type="submit" class="btn primary" data-pick-apply>이대로 보기</button></div>
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
    apply.disabled = n === 0;
    status.classList.toggle('warn', Boolean(message));
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

  const opener = document.activeElement;
  const release = trapFocus(form, { restore: false });
  (boxes.find((b) => b.checked) || boxes[0] || apply).focus({ preventScroll: true });
  const close = () => {
    release();
    dismiss(scrim);
    if (opener?.isConnected) opener.focus({ preventScroll: true });
  };
  scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); } });
  scrim.addEventListener('click', (e) => { if (e.target === scrim || e.target.closest('[data-pick-cancel]')) close(); });
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const ids = picked();
    if (!ids.length) return;
    close();
    if (ids.length !== before.size || ids.some((id) => !before.has(id))) onApply(ids);
  });
  return scrim;
}

