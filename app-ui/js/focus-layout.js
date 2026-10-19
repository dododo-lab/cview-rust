// 멀티라이브 포커스 레이아웃 계산 — 메인 1개 + 나머지 줄. 원본 MultiLiveFocusDirection / focusRatio.

export const FOCUS_DIRECTIONS = [
  { id: 'left', label: '메인 왼쪽' },
  { id: 'right', label: '메인 오른쪽' },
  { id: 'top', label: '메인 위' },
  { id: 'bottom', label: '메인 아래' },
];
export const FOCUS_RATIO = Object.freeze({ min: 0.45, max: 0.85 });

export function clampFocusRatio(r) {
  if (!Number.isFinite(r)) return 0.7;
  return Math.min(FOCUS_RATIO.max, Math.max(FOCUS_RATIO.min, r));
}

/**
 * 패인 수·방향·비율 → CSS grid 템플릿과 패인별 위치(0 = 메인). 순수 함수.
 * @returns {{ columns: string, rows: string, areas: Array<{row: string, column: string}> }}
 */
export function focusGrid(count, direction, ratio) {
  const r = clampFocusRatio(ratio);
  const k = Math.max(1, count - 1);
  const main = `${r.toFixed(3)}fr`;
  const side = `${(1 - r).toFixed(3)}fr`;
  const strip = `repeat(${k}, 1fr)`;
  const areas = [];
  const vertical = direction === 'top' || direction === 'bottom';
  const mainFirst = direction === 'left' || direction === 'top' || !direction;
  const mainLine = mainFirst ? '1' : '2';
  const stripLine = mainFirst ? '2' : '1';
  if (vertical) {
    areas.push({ row: mainLine, column: '1 / -1' });
    for (let i = 0; i < k; i++) areas.push({ row: stripLine, column: String(i + 1) });
    return { columns: strip, rows: mainFirst ? `${main} ${side}` : `${side} ${main}`, areas };
  }
  areas.push({ row: '1 / -1', column: mainLine });
  for (let i = 0; i < k; i++) areas.push({ row: String(i + 1), column: stripLine });
  return { columns: mainFirst ? `${main} ${side}` : `${side} ${main}`, rows: strip, areas };
}

/** 드래그 위치(무대 안 비율 0~1) → 메인 비율. */
export function ratioFromPointer(direction, fx, fy) {
  switch (direction) {
    case 'right': return clampFocusRatio(1 - fx);
    case 'top': return clampFocusRatio(fy);
    case 'bottom': return clampFocusRatio(1 - fy);
    default: return clampFocusRatio(fx);
  }
}
