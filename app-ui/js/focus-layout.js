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

/** 서브 줄 끝 "채널 추가" 칸의 트랙 — 서브 패인보다 좁게. */
export const FOCUS_ADD_TRACK = 'minmax(72px, 0.4fr)';

/**
 * 패인 수·방향·비율 → CSS grid 템플릿과 패인별 위치(0 = 메인). 순수 함수.
 * withAdd 면 서브 줄 끝(가로 줄은 오른쪽, 세로 줄은 아래)에 채널 추가 칸을 하나 더 둔다.
 * @returns {{ columns: string, rows: string, areas: Array<{row: string, column: string}>, add: {row: string, column: string} | null }}
 */
export function focusGrid(count, direction, ratio, withAdd = false) {
  const r = clampFocusRatio(ratio);
  const k = Math.max(1, count - 1);
  const main = `${r.toFixed(3)}fr`;
  const side = `${(1 - r).toFixed(3)}fr`;
  const strip = withAdd ? `repeat(${k}, 1fr) ${FOCUS_ADD_TRACK}` : `repeat(${k}, 1fr)`;
  const areas = [];
  const vertical = direction === 'top' || direction === 'bottom';
  const mainFirst = direction === 'left' || direction === 'top' || !direction;
  const mainLine = mainFirst ? '1' : '2';
  const stripLine = mainFirst ? '2' : '1';
  if (vertical) {
    areas.push({ row: mainLine, column: '1 / -1' });
    for (let i = 0; i < k; i++) areas.push({ row: stripLine, column: String(i + 1) });
    const add = withAdd ? { row: stripLine, column: String(k + 1) } : null;
    return { columns: strip, rows: mainFirst ? `${main} ${side}` : `${side} ${main}`, areas, add };
  }
  areas.push({ row: '1 / -1', column: mainLine });
  for (let i = 0; i < k; i++) areas.push({ row: String(i + 1), column: stripLine });
  const add = withAdd ? { row: String(k + 1), column: stripLine } : null;
  return { columns: mainFirst ? `${main} ${side}` : `${side} ${main}`, rows: strip, areas, add };
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

/** 메인 아래 채널 정보 칸의 최소 높이 — 이보다 낮게 남으면 정보 칸 없이 메인이 칸을 다 쓴다(채널 카드 한 장 높이). */
export const FOCUS_INFO_MIN = 96;

/**
 * 메인 왼쪽·오른쪽 포커스 — 메인 칸은 무대 높이를 다 받는데 16:9 영상은 폭이 정하는 높이까지만 차서 위아래가 검은 띠였다.
 * 메인 영상을 폭에 맞는 16:9 높이로 줄이고 남는 높이를 아래 정보 칸으로 돌린다. 순수 함수.
 * 무대 열은 `r fr (1-r) fr` + 간격(gap) 하나라 메인 열 폭은 (폭 - 간격) × r 이다.
 * @returns {{ videoH: number, infoH: number } | null} 정보 칸이 FOCUS_INFO_MIN 보다 낮으면 null(예전 배치)
 */
export function focusInfoSplit(stageW, stageH, ratio, gap = 6) {
  const mainW = (stageW - gap) * clampFocusRatio(ratio);
  if (!(mainW > 0) || !(stageH > 0)) return null;
  const videoH = Math.ceil((mainW * 9) / 16);
  const infoH = Math.floor(stageH - videoH - gap);
  return infoH >= FOCUS_INFO_MIN ? { videoH, infoH } : null;
}
