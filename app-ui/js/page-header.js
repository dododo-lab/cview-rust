// 화면 머리줄(제목 바) 꾸밈 — 메뉴 아이콘 칩 + 제목 위·설명 아래 2단.
// 각 화면은 예전처럼 `.page-header > h1.page-title (+ .page-subtitle)` 만 그리고, 여기서 한 번에 감싼다.
// 제목·설명 요소는 그대로 옮기기만 하므로 화면 코드의 querySelector('.page-title') 등은 그대로 동작한다.

/**
 * `header` 의 제목 앞에 아이콘 칩을 넣고 제목과 바로 뒤 설명을 한 덩어리로 묶는다.
 * `compact` 면 설명은 묶지 않고(멀티라이브 수 알약처럼 줄 안에 두는 것) 아이콘·제목만 한 줄로.
 */
export function decoratePageHeader(header, icon, { compact = false } = {}) {
  if (!header || header.querySelector(':scope > .ph-icon')) return;
  const title = header.querySelector(':scope > .page-title');
  if (!title) return;
  header.classList.add('ph');
  header.classList.toggle('ph-compact', compact);
  const chip = document.createElement('span');
  chip.className = 'ph-icon';
  chip.setAttribute('aria-hidden', 'true');
  chip.innerHTML = icon;
  const text = document.createElement('div');
  text.className = 'ph-text';
  title.before(chip, text);
  text.append(title);
  // 설명은 제목 뒤 형제 중 첫 .page-subtitle (사이에 전환 버튼이 있어도).
  const sub = compact ? null : header.querySelector(':scope > .page-subtitle');
  if (sub) text.append(sub);
}

/** 새로고침 아이콘 — 홈 머리줄과 같은 동그란 버튼에 쓴다. */
export const refreshIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>';
