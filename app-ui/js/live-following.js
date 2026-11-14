// 홈 맨 위 "방송 중인 팔로잉" — 우선순위 타일 격자 + 미리보기 카드.
//
// 예전 모양은 동그란 아바타만 한 줄로 늘어선 띠였다. 개수가 많으면 오른쪽에서 잘려 가로 스크롤로만 보였고(스크롤 막대도 없었다),
// 누가 무엇을(방송 제목·시청자 수·썸네일) 하는지는 알 수 없었다. 이제:
// - 타일 — 아바타(라이브 고리) + 이름·인증·즐겨찾기 + 방송 제목 + 시청자·카테고리·경과. 썸네일은 받지 않는다(아바타만).
// - 격자 — 창 폭에 맞춰 칸 수가 바뀌고, 접힌 상태는 두 줄(좁으면 세·네 줄)만 보인다. 넘치면 마지막 칸이 "+N 더 보기"가 되어
//   아바타 셋을 겹쳐 보여 준다. 펼치면 모두, 마지막 칸은 "접기". 접힌 줄의 그림은 받지 않는다(display: none 이라 lazy 가 미룬다).
// - 정렬 — 추천(즐겨찾기 → 최근 본 채널 → 시청자 많은 순) · 시청자 · 방금 시작. 고른 값은 이 기기에 기억한다.
//   즐겨찾기도 최근 시청 기록도 없으면 추천 = 예전 순서(시청자 많은 순)라 습관이 깨지지 않는다. 사이드바 홈 목록도 즐겨찾기 먼저다.
// - 미리보기 카드 — 타일에 마우스를 잠시 올리거나 키보드로 초점을 옮기면 그 아래 썸네일·전체 제목·시청하기·멀티라이브·채널 정보가 뜬다.
//   썸네일은 이때 한 장만 받는다. 썸네일에 마우스를 두면 기존 호버 미리보기(hover-preview.js)가 영상을 보여 준다(19세 방송 제외).
//   카드 위로 마우스를 옮겨도 닫히지 않고, Esc 로 닫는다(WCAG 1.4.13). 화면 낭독기에는 숨긴다 — 같은 정보가 타일 이름에 있다.
// - 키보드 — 목록 전체가 Tab 한 번(지금 칸) + 그 칸의 + 단추. ←→ 옆 칸, ↑↓ 윗·아랫줄, Home/End 처음·끝, Enter 시청,
//   Shift+F10·메뉴 키는 우클릭 메뉴(재생·멀티라이브·채널 정보·채팅만·즐겨찾기·링크). "+N 더 보기"에서 Enter 면 펼치고 처음 드러난 칸으로.
// - 갱신 — 같은 채널의 타일은 그대로 두고 바뀐 글자만 고친다(그림을 다시 받지 않는다 → 깜박임 없음). 홈을 다시 그려도
//   이 구역 요소는 옮겨 붙인다(초점·펼침·정렬 유지). 마우스가 올라가 있거나 키보드로 다루는 초점이 안에 있으면 자리 바꿈은
//   손을 뗄 때까지 미룬다(마우스로 누른 단추에 남은 초점은 세지 않는다 — 마지막 입력 방식으로 가른다).
// 클릭·Enter 는 홈 본문의 카드 위임(cards.js bindCardOpen)이, + 는 멀티라이브 위임(data-ml-add), 우클릭은 카드 메뉴(app.js)가 받는다.
import { esc, safeUrl, imageSrc, icons } from './dom.js';
import { uptimeText, compactCount } from './cards.js';
import { previewAttr } from './hover-preview.js';
import { verifiedBadge, badgeKind, VERIFIED_MARKS } from './verified-badge.js';

export const LF_SORTS = Object.freeze([
  { id: 'mine', label: '추천', tip: '즐겨찾기·최근 본 채널 먼저, 나머지는 시청자 많은 순' },
  { id: 'viewers', label: '시청자', tip: '시청자 많은 순' },
  { id: 'fresh', label: '방금 시작', tip: '방송을 막 시작한 순' },
]);
const SORT_KEY = 'cview.home.liveSort';
const SORT_IDS = new Set(LF_SORTS.map((s) => s.id));
/** 정렬 단추를 보이는 최소 개수 — 셋 이하를 줄 세울 일은 없다. */
export const LF_SORT_MIN = 4;
/** 미리보기 카드가 뜨기까지 — 마우스(목록을 훑고 지나갈 땐 뜨지 않게) · 키보드 초점. */
const PEEK_HOVER_MS = 420;
const PEEK_FOCUS_MS = 260;
/** 카드가 열린 채 옆 타일로 옮길 때 — 카드로 비스듬히 내려가는 길에 다른 타일을 스쳐도 바로 바뀌지 않게. */
const PEEK_SWITCH_MS = 90;
const PEEK_HIDE_MS = 180;
/** 타일 최소 폭·칸 사이 — CSS(.lf-grid `repeat(auto-fill, minmax(min(100%, 212px), 1fr))`, gap 8px)와 같아야 한다. */
const TILE_MIN = 212;
const TILE_GAP = 8;
/** 구역 안에서 누르면 키보드로 다루는 중인 키 — Esc·Shift·PageDown 처럼 칸을 옮기지 않는 키는 세지 않는다. */
const NAV_KEYS = new Set(['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'Tab', 'Enter', ' ', 'ContextMenu', 'F10']);
/**
 * 카드보다 위에 떠서 제 Esc 를 받는 것들 — app.js escapeTaken 의 ESC_POPUPS·ESC_LAYERS 와 계정·사용자 말풍선, 메뉴·대화상자 역할 전반.
 * 숨었거나(자신·조상의 hidden·inert, 숨은 화면 안) 닫히는 중인(.exit-ghost·.leaving 조상) 것은 escTakenAbove 가 거른다.
 */
const ESC_ABOVE = '.ctx-menu, .design-menu, .account-pop, .user-pop, .notify-pop, .sheet-scrim, .palette-scrim, '
  + '.quality-menu, .player-panel, .emoji-picker, .chat-tray, .dp-panel, .chat-autocomplete, .ml-pane-menu, .ml-more-menu, '
  + '[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"], [aria-modal="true"]';

/**
 * 제 안에 초점이 있을 때만 Esc 를 받는 팝업 — 보이기만 해서는 카드의 Esc 를 넘기지 않는다. 마우스로 연 계정 메뉴·알림 설정 말풍선은
 * 닫을 Esc 처리가 없어, 넘기면 카드도 메뉴도 Esc 로 닫히지 않았다.
 */
const ESC_FOCUS_ONLY = '.account-pop, .notify-pop';
/** 숨었거나 닫히는 중인 팝업의 조상 표시 — hideWithExit 의 .exit-ghost 사본·dismiss 의 .leaving 시트는 바깥 상자에만 붙는다. */
const ESC_GONE = '[hidden], [inert], .exit-ghost, .leaving';

/** 이번 Esc 를 카드 대신 위에 뜬 다른 팝업이 받아야 하나 — 초점이 그 안에 있거나, 보이는 것이 하나라도 있다. */
function escTakenAbove(target, own) {
  if (target instanceof Element && !own.contains(target) && target.closest(ESC_ABOVE)) return true;
  for (const el of document.querySelectorAll(ESC_ABOVE)) {
    if (own.contains(el) || el.matches(ESC_FOCUS_ONLY) || el.closest(ESC_GONE)) continue;
    if (el.getClientRects().length) return true; // 그려져 있다(display: none 인 조상 아래가 아니다)
  }
  return false;
}

/** 격자 폭 → 칸 수(CSS auto-fill 과 같은 셈). 순수 함수. */
export function gridColumns(width, gap = TILE_GAP, min = TILE_MIN) {
  return Math.max(1, Math.floor((width + gap) / (min + gap)));
}

/** 기억한 정렬 — 없거나 모르는 값이면 추천. */
export function loadLiveSort() {
  try {
    const v = localStorage.getItem(SORT_KEY);
    return SORT_IDS.has(v) ? v : 'mine';
  } catch { return 'mine'; }
}
function saveLiveSort(v) {
  try { localStorage.setItem(SORT_KEY, v); } catch { /* 이번 실행만 */ }
}

const opened = (l) => {
  const t = Date.parse(l.openDate || '');
  return Number.isFinite(t) ? t : -Infinity;
};
// 시청자 수가 같으면 채널 아이디로 — 갱신마다 같은 수의 두 채널이 자리를 바꾸지 않게.
const byViewers = (a, b) => ((b.viewerCount || 0) - (a.viewerCount || 0)) || String(a.channelId).localeCompare(String(b.channelId));
const toSet = (v) => (v instanceof Set ? v : new Set(Array.isArray(v) ? v.map((x) => (x && typeof x === 'object' ? x.channelId : x)) : []));

/**
 * 정렬 — 순수 함수. 같은 채널이 두 번 오면 앞의 것만 쓴다.
 * - mine(추천): 즐겨찾기(시청자순) → 최근 본 채널(최근에 본 순) → 나머지(시청자순)
 * - viewers: 시청자 많은 순 · fresh: 방송 시작이 늦은(막 시작한) 순, 같으면 시청자순
 * @param {Array} lives LiveCardDto 목록
 * @param {{ sort?: string, favorites?: Set<string>|string[], recent?: Array<{channelId:string}> }} [opts]
 */
export function arrangeLiveFollowing(lives, { sort = 'mine', favorites = new Set(), recent = [] } = {}) {
  const seen = new Set();
  const list = (lives || []).filter((l) => l && l.channelId && !seen.has(l.channelId) && seen.add(l.channelId));
  if (sort === 'viewers') return list.sort(byViewers);
  if (sort === 'fresh') return list.sort((a, b) => (opened(b) - opened(a)) || byViewers(a, b));
  const fav = toSet(favorites);
  const recentAt = new Map();
  (recent || []).forEach((r, i) => { if (r && r.channelId && !recentAt.has(r.channelId)) recentAt.set(r.channelId, i); });
  const tier = (l) => (fav.has(l.channelId) ? 0 : recentAt.has(l.channelId) ? 1 : 2);
  return list.sort((a, b) => (tier(a) - tier(b))
    || (tier(a) === 1 ? recentAt.get(a.channelId) - recentAt.get(b.channelId) : 0)
    || byViewers(a, b));
}

/** 접힌 상태의 줄 수 — 칸이 적은 좁은 창은 줄을 늘려 너무 적게 보이지 않게. */
export function collapsedRows(cols) {
  return cols <= 1 ? 4 : cols === 2 ? 3 : 2;
}

/**
 * 접힌 상태에서 보일 타일 수 — 순수 함수. 넘치면 마지막 칸을 "+N 더 보기"에 내준다.
 * @returns {{ shown: number, hidden: number, slots: number }}
 */
export function collapseLimit(total, cols) {
  const c = Math.max(1, Math.floor(cols) || 1);
  const slots = c * collapsedRows(c);
  if (total <= slots) return { shown: total, hidden: 0, slots };
  return { shown: slots - 1, hidden: total - (slots - 1), slots };
}

/** 타일 주 동작의 이름 — 화면에 보이는 글(이름)로 시작해 음성 제어로도 부를 수 있게. 순수 함수. */
export function liveFollowLabel(l, { favorite = false } = {}) {
  const parts = [l.channelName || '채널'];
  const kind = badgeKind(l);
  if (kind) parts.push(VERIFIED_MARKS[kind].label);
  if (favorite) parts.push('즐겨찾기');
  parts.push(l.liveTitle || '제목 없음');
  parts.push(`시청자 ${l.viewerCountText || compactCount(l.viewerCount || 0)}명`);
  if (l.categoryName) parts.push(l.categoryName);
  const up = uptimeText(l.openDate);
  if (up) parts.push(up === '방금 시작' ? up : `${up}째 방송`);
  if (l.adult) parts.push('19세 이상');
  return parts.join(', ');
}

const plusIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M12 6v12M6 12h12"/></svg>';
const starIcon = '<svg class="lf-fav" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 2.8l2.83 5.74 6.33.92-4.58 4.47 1.08 6.3L12 17.25l-5.66 2.98 1.08-6.3L2.84 9.46l6.33-.92z"/></svg>';
const chevronUp = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 15l6-6 6 6"/></svg>';

function avatarHtml(url) {
  const src = safeUrl(url);
  return src
    ? `<img class="avatar" src="${esc(imageSrc(src, 'avatar'))}" alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer">`
    : '<span class="avatar"></span>';
}

/** 한 칸의 뼈대 — 글자·그림은 patch 가 채운다. */
function itemShell() {
  const li = document.createElement('li');
  li.className = 'lf-item';
  li.innerHTML = `<button type="button" class="lf-open" tabindex="-1"><span class="lf-ava"></span><span class="lf-text">`
    + `<span class="lf-name"><span class="lf-name-text"></span><span class="lf-marks"><span class="lf-vb"></span>${starIcon.replace('<svg ', '<svg hidden ')}<span class="lf-adult" hidden>19</span></span></span>`
    + '<span class="lf-title"></span>'
    + '<span class="lf-meta"><span class="lf-viewers"><i class="dot" aria-hidden="true"></i><span class="lf-vn"></span></span><span class="lf-cat"></span><span class="lf-up"></span></span>'
    + `</span></button><button type="button" class="lf-add" data-ml-add tabindex="-1" title="멀티라이브에 추가">${plusIcon}</button>`;
  return li;
}

/** 세로로 스크롤되는 가장 가까운 조상(홈은 .page) — 없으면 문서. */
function scrollParentOf(el) {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const o = getComputedStyle(p).overflowY;
    if ((o === 'auto' || o === 'scroll') && p.scrollHeight > p.clientHeight) return p;
  }
  return document.scrollingElement;
}

// 값이 바뀐 곳만 쓴다 — 같은 글을 다시 쓰면 글자 노드가 새로 생겨 화면 낭독기가 다시 읽거나 선택이 풀린다.
function setText(el, v) { if (el.textContent !== v) el.textContent = v; }
function setAttr(el, name, v) { if (el.getAttribute(name) !== v) el.setAttribute(name, v); }

export class LiveFollowingStrip {
  constructor() {
    this.lives = [];
    this.order = []; // 지금 화면 순서(채널 아이디)
    this.items = new Map(); // 채널 아이디 → li
    this.byId = new Map();
    this.favorites = new Set();
    this.recent = [];
    this.sort = loadLiveSort();
    this.expanded = false;
    this.cols = 4;
    this.activeKey = null; // 목록에서 Tab 이 들어올 칸(로빙 초점) — 채널 아이디 또는 '__more'
    this.reorderPending = false;
    this.peekKey = null;
    this.peekDismissed = null; // Esc 로 닫은 칸 — 다른 칸으로 옮기기 전엔 다시 띄우지 않는다
    this.timers = { show: 0, hide: 0 };
    this.pendingFrom = null; // 열기 예약이 어디서 왔나 — 'hover' | 'focus'
    // 마지막 입력 방식 — 'keyboard' | 'pointer'. 초점이 구역 안이어도 마우스로 누른 단추에 남은 초점이면 바쁜 게 아니다(busy).
    // :focus-visible 은 마우스로 누른 단추에서도 아무 키(Esc·Shift·PageDown)나 누르면 켜지고, WebKit 은 화살표 뒤 스크립트 초점에선
    // 꺼져 있어 쓸 수 없었다.
    this.input = null;
    this.hoverKey = null; // 실제 포인터가 마지막으로 들어간 칸
    this.ptrX = NaN; // 마지막 pointermove 자리 — 합성 경계 이벤트 가려내기(isSynthetic)
    this.ptrY = NaN;
    this.ptrMoved = false;
    this.selfFocus = false; // 이 구역이 스크립트로 옮기는 초점 — 입력 방식을 바꾸지 않는다
    this.navKey = false; // 마지막 키가 탐색 키였나(문서 전체) — 들어온 초점의 :focus-visible 을 믿을지
    this.stripMenu = null; // 칸에서 연 카드 메뉴(.ctx-menu) — 떠 있는 동안은 바쁘다(watchStripMenu)
    this.menuWait = false;
    this.uid = `lf-${Math.random().toString(36).slice(2, 8)}`;
    this.build();
  }

  build() {
    const el = document.createElement('section');
    el.className = 'home-section lf';
    el.dataset.sort = this.sort; // 좁은 타일의 셋째 줄 — 방금 시작 정렬이면 경과를 남긴다(app.css)
    el.setAttribute('aria-labelledby', `${this.uid}-title`);
    el.innerHTML = `
      <div class="section-head lf-head">
        <div class="section-titles"><h2 class="section-title" id="${this.uid}-title">방송 중인 팔로잉 <span class="count"></span></h2></div>
        <div class="grow"></div>
        <div class="segmented lf-sort" role="group" aria-label="방송 중인 팔로잉 정렬" data-no-glider>${LF_SORTS.map((s) => `<button type="button" class="seg${s.id === this.sort ? ' active' : ''}" data-lf-sort="${s.id}" title="${esc(s.tip)}">${esc(s.label)}</button>`).join('')}</div>
        <button type="button" class="link-btn lf-all" data-goto="following">전체 보기</button>
      </div>
      <div class="lf-ruler" aria-hidden="true"></div>
      <ul class="lf-grid" id="${this.uid}-grid" role="list" aria-labelledby="${this.uid}-title"></ul>
      <div class="lf-empty" hidden><span class="lf-empty-ico" aria-hidden="true">${icons.following}</span><span class="lf-empty-text"><strong>지금 방송 중인 팔로잉 채널이 없어요</strong><span>방송을 시작하면 여기에 바로 보여요</span></span></div>
      <div class="lf-peek" aria-hidden="true" hidden></div>`;
    this.el = el;
    this.count = el.querySelector('.count');
    this.sortBox = el.querySelector('.lf-sort');
    this.grid = el.querySelector('.lf-grid');
    this.ruler = el.querySelector('.lf-ruler');
    this.empty = el.querySelector('.lf-empty');
    this.peek = el.querySelector('.lf-peek');
    this.moreItem = document.createElement('li');
    this.moreItem.className = 'lf-item lf-more-item';
    this.moreItem.innerHTML = `<button type="button" class="lf-more" tabindex="-1" aria-controls="${this.uid}-grid" aria-expanded="false"><span class="lf-stack" aria-hidden="true"></span><span class="lf-more-text"><span class="lf-more-count"></span><span class="lf-more-label"></span></span></button>`;
    this.moreBtn = this.moreItem.firstElementChild;

    el.addEventListener('click', (e) => this.onClick(e));
    el.addEventListener('keydown', (e) => this.onKey(e));
    el.addEventListener('focusin', (e) => this.onFocusIn(e));
    el.addEventListener('focusout', (e) => this.onFocusOut(e));
    el.addEventListener('pointerover', (e) => this.onPointerOver(e));
    el.addEventListener('pointerout', (e) => this.onPointerOut(e));
    el.addEventListener('pointermove', (e) => this.onPointerMove(e), { passive: true });
    // 누름·실제 움직임·키는 문서 전체에서 본다 — 구역 밖을 누른 것도 마우스로 돌아간 것이고, 포인터가 구역 밖에 있다가 스크롤로
    // 구역이 그 아래로 온 것도 합성 이벤트로 가려야 한다. 리스너들은 값 몇 개만 적는다.
    const onDocPointer = (e) => {
      if (e.type === 'pointerdown') { this.setInput('pointer'); this.navKey = false; return; }
      this.ptrMoved = e.clientX !== this.ptrX || e.clientY !== this.ptrY;
      this.ptrX = e.clientX;
      this.ptrY = e.clientY;
      // 칸이 가만있는 포인터 아래에서 밀려나며(사이드바 펼침·방송 종료) 온 pointerout 은 합성이라 걸렀다 — 그 뒤 실제로 움직인 곳이
      // 칸·카드가 아니면 그때 떠난 것으로 본다(Chromium, 예전엔 카드가 남아 히어로를 가렸다). 구역 밖(사이드바)도 여기서 본다.
      if (this.ptrMoved && this.hoverKey && e.pointerType !== 'touch' && !this.isStripTarget(e.target)) this.leaveTile();
    };
    document.addEventListener('pointerdown', onDocPointer, { capture: true, passive: true });
    document.addEventListener('pointermove', onDocPointer, { capture: true, passive: true });
    // 마지막 키 — 다른 곳(카드 메뉴·팔레트·시트)이 Esc 로 닫히며 초점을 돌려줄 때 Chromium 은 그 초점도 :focus-visible 로 친다.
    // 탐색 키 다음에 들어온 초점만 키보드로 센다(밖에서 Tab 으로 들어오는 것은 Tab 이 탐색 키라 그대로 센다).
    document.addEventListener('keydown', (e) => {
      this.navKey = NAV_KEYS.has(e.key) && !e.metaKey && !e.ctrlKey && !e.altKey;
    }, { capture: true, passive: true });
    // 칸에서 연 카드 메뉴(우클릭·Shift+F10)가 떠 있는 동안은 바쁘다 — watchStripMenu
    el.addEventListener('contextmenu', (e) => { if (e.target.closest && e.target.closest('.lf-item')) this.watchStripMenu(); });
    // 엔진에 따라 pointerleave 를 보내는 순간 :hover 가 아직 남아 있다 — 한 틱 뒤에 본다.
    el.addEventListener('pointerleave', () => setTimeout(() => this.settle(), 0));
    // 칸 수는 격자 폭으로 정한다 — 창·사이드바 폭이 바뀌면 접힌 줄 수에 맞춰 다시 숨긴다(다시 그리지 않는다).
    // 폭은 ResizeObserver 가 레이아웃 뒤에 넘겨주는 값만 쓴다(직접 재면 강제 동기 레이아웃 — 시작 중 29ms 였다).
    // 붙인 직후·숨었다 보일 때도 알림이 오고, 여기서 바꾼 숨김은 같은 프레임의 페인트 전에 다시 배치돼 깜박이지 않는다.
    // 홈 스켈레톤의 같은 격자도 본다(watch) — 붙이기 전에 칸 수를 알아 두면 붙일 때 숨김을 바꿀 일이 없어, 같은 프레임의
    // 다른 관찰자(멀티라이브 머리줄 등)가 크기를 읽다 강제 레이아웃을 일으키지 않는다(perf.mjs P01).
    // 격자 대신 높이 0 의 자(.lf-ruler — 격자와 같은 폭)를 본다: 여기서 칸을 숨기면 격자 높이가 같은 프레임에 바뀌어
    // "ResizeObserver loop completed with undelivered notifications" 오류가 났다(WebKit, 창 폭을 줄일 때). 자는 폭만 따른다.
    this.ro = new ResizeObserver((entries) => {
      let w = 0;
      for (const e of entries) {
        if (e.target !== this.ruler && !e.target.isConnected) { this.ro.unobserve(e.target); continue; }
        if (e.contentRect.width) w = e.contentRect.width;
      }
      if (!w) return; // 숨음·떨어짐 — 칸 수는 그대로 둔다
      // 칸 사이 — 계산 스타일을 읽으면 앞 콜백(세그먼트 판)이 쓴 스타일 때문에 레이아웃을 당겼다. CSS 와 같은 값(촘촘하게 6px).
      const gap = document.documentElement.classList.contains('compact') ? 6 : TILE_GAP;
      const cols = gridColumns(w, gap);
      if (cols === this.cols) return;
      this.cols = cols;
      this.applyLimit();
    });
    this.ro.observe(this.ruler);
    // 미리보기 카드가 열린 동안만 듣는다 — 스크롤·창 크기·바깥 누름·창 가림이면 닫는다.
    // 스크롤로 닫을 때는 키보드 초점의 열기 예약만 남긴다 — 화살표로 화면 밖 칸에 가면 focusNav 의 scrollIntoView 가 스크롤을 일으켜,
    // 그 칸의 카드 예약(focusin)까지 지웠다. 마우스 예약(옆 칸 90ms)은 지운다 — 남기면 휠로 화면 밖에 밀려난 칸의 카드가 다시 떴다.
    this.onAway = (e) => {
      if (e.type === 'pointerdown' && (this.peek.contains(e.target) || this.grid.contains(e.target))) return;
      this.hidePeek({ keepPending: e.type === 'scroll' && this.pendingFrom === 'focus' });
    };
    // Esc — 초점이 어디 있든(마우스로 연 카드는 초점이 body 인 일이 흔하다: WKWebView 는 클릭으로 단추에 초점을 주지 않는다)
    // 카드가 열린 동안 문서에서 먼저 받는다(stats.js 풍선과 같은 방식). 위에 뜬 메뉴·시트·팔레트가 있으면 그것부터 닫히게
    // 이번 Esc 는 넘긴다. 받으면 preventDefault(app.js escapeTaken 이 시청 화면 등을 함께 닫지 않게) + stopPropagation
    // (구역의 키 처리와 두 번 다루지 않게).
    // 홈이 숨었으면(단축키·메뉴로 다른 화면 — onHidden 이 닫지만 혹시 남았으면) 가로채지 않고 정리만 한다.
    this.onEscDoc = (e) => {
      if (e.key !== 'Escape' || !this.peekKey || e.defaultPrevented) return;
      if (this.peek.hidden || !this.peek.getClientRects().length) { this.hidePeek(); return; }
      if (escTakenAbove(e.target, this.el)) return;
      e.preventDefault();
      e.stopPropagation();
      this.peekDismissed = this.peekKey;
      this.hidePeek();
    };
  }

  /** 같은 폭의 다른 격자(홈 스켈레톤의 자리)로 칸 수를 미리 잰다. 붙일 때(mount)·뗄 때(detach) 놓는다. */
  watch(el) {
    this.unwatch();
    if (!el || el === this.ruler) return;
    this.watched = el;
    this.ro.observe(el);
  }

  unwatch() {
    if (this.watched) this.ro.unobserve(this.watched);
    this.watched = null;
  }

  /**
   * 홈에 이 구역 자리가 없어질 때(로그아웃·팔로잉 실패·스켈레톤) — 옛 부모에서 떼어 낸다. 떼지 않으면 문서에서 빠진 옛 홈 트리
   * (히어로·격자·그림)를 이 요소의 부모 사슬로 붙잡아 두었다(HomePage 가 이 객체를 계속 들고 있다). 카드·문서 리스너·예약도 정리한다.
   */
  detach({ forget = false } = {}) {
    this.hidePeek();
    this.unwatch();
    if (forget) this.restore = null; // 자리가 아예 없어졌다 — 한참 뒤 다시 그릴 때 옛 칸으로 초점을 끌어가지 않게
    if (this.el.parentNode) this.el.remove();
  }

  // ── 데이터 ───────────────────────────────────────

  /**
   * 새 목록으로 고친다(같은 채널 칸은 그대로). favorites·recent 를 빼면 앞의 값을 쓴다.
   * @param {Array} lives 방송 중인 팔로잉(LiveCardDto)
   * @param {{ favorites?: Set<string>|Array, recent?: Array }} [extra]
   */
  update(lives, { favorites, recent } = {}) {
    this.lives = Array.isArray(lives) ? lives : [];
    if (favorites !== undefined) this.favorites = toSet(favorites);
    if (recent !== undefined) this.recent = Array.isArray(recent) ? recent : [];
    this.render();
  }

  /**
   * 마우스가 올라가 있거나 키보드로 다루는 초점이 안에 있다 — 이때는 칸 자리를 바꾸지 않는다.
   * 초점은 마지막 입력이 키보드일 때만 센다(this.input) — 정렬·더 보기·접기를 마우스로 누르면 초점이 그 단추에 남아, 마우스가 떠난
   * 뒤에도 늘 바쁜 것으로 보여 2분 갱신의 새 방송·즐겨찾기가 '+N 더 보기' 뒤 끝에 숨었다. 숨은 홈은 바쁘지 않다.
   */
  busy() {
    if (!this.el.isConnected || this.el.closest('[hidden]')) return false;
    if (this.stripMenu && this.stripMenu.isConnected) return true; // 칸에서 연 카드 메뉴 — 닫히면 settle
    if (this.el.matches(':hover')) return true;
    const a = document.activeElement;
    return Boolean(a && this.el.contains(a) && this.input === 'keyboard');
  }

  /** 홈이 숨었다(app.js navigate — 단축키·메뉴로 다른 화면) — 포인터를 움직이지 않으면 숨은 홈에 카드와 문서 리스너가 남았다. */
  onHidden() {
    this.hidePeek();
    this.hoverKey = null;
  }

  /** 마지막 입력 방식 — 구역에도 적어 둔다(data-input). WebKit 은 마우스로 누른 뒤 화살표로 옮긴 스크립트 초점을 :focus-visible 로 치지 않아
   *  초점 테두리가 없었다 — app.css 가 키보드일 때 :focus 에도 테두리를 그린다. */
  setInput(mode) {
    if (this.input === mode) return;
    this.input = mode;
    this.el.dataset.input = mode;
  }

  /**
   * 칸에서 연 카드 메뉴(app.js installCardMenu → chat.js showMenu, body 에 붙는다)가 떠 있는 동안은 바쁘다. 초점·포인터가 메뉴로 옮겨 가
   * 구역을 떠난 것처럼 보여, 미뤄 둔 자리 바꿈이 열린 메뉴 아래에서 일어났다 — 연 칸이 '+N 더 보기' 뒤로 숨고, Esc 로 닫으면 숨은 칸으로
   * 초점을 돌려주지 못해 body 로 떨어졌다. 메뉴는 즐겨찾기 목록을 받은 뒤 붙으니 잠시 기다리고, 닫히면 미룬 자리 바꿈을 한다.
   */
  watchStripMenu() {
    this.menuWait = true;
    clearTimeout(this.menuWaitTimer);
    this.menuWaitTimer = setTimeout(() => { this.menuWait = false; this.checkStripMenu(); }, 2000); // 끝내 뜨지 않으면
    if (this.menuObs) return;
    this.menuObs = new MutationObserver((recs) => {
      for (const r of recs) {
        for (const n of r.addedNodes) {
          if (this.menuWait && n.nodeType === 1 && n.classList.contains('ctx-menu')) {
            this.stripMenu = n;
            this.menuWait = false;
            clearTimeout(this.menuWaitTimer);
          }
        }
      }
      this.checkStripMenu();
    });
    this.menuObs.observe(document.body, { childList: true });
  }

  checkStripMenu() {
    if (this.stripMenu && !this.stripMenu.isConnected) this.stripMenu = null;
    if (this.stripMenu || this.menuWait || !this.menuObs) return;
    this.menuObs.disconnect();
    this.menuObs = null;
    this.settle(); // 메뉴가 닫혔다 — Esc·Tab 으로 닫았으면 초점은 이미 칸에 돌아왔다(chat.js close(true))
  }

  render({ force = false } = {}) {
    const list = arrangeLiveFollowing(this.lives, { sort: this.sort, favorites: this.favorites, recent: this.recent });
    const want = list.map((l) => l.channelId);
    this.byId = new Map(list.map((l) => [l.channelId, l]));
    let order = want;
    if (!force && this.order.length && this.busy()) {
      // 손 아래에서 칸이 옮겨 가지 않게 — 있던 칸은 제자리, 새 칸은 끝에. 손을 떼면(settle) 제 순서로.
      const keep = this.order.filter((k) => this.byId.has(k));
      const kept = new Set(keep);
      order = [...keep, ...want.filter((k) => !kept.has(k))];
    }
    this.reorderPending = order.join('\n') !== want.join('\n');

    const focused = this.grid.contains(document.activeElement) ? document.activeElement : null;
    const a0 = document.activeElement;
    const kbdIn = this.input === 'keyboard' && a0 && this.el.contains(a0) && !this.peek.contains(a0);
    const focusedLi = focused && focused.closest('.lf-item');
    const focusedIndex = focusedLi ? [...this.grid.children].indexOf(focusedLi) : -1;

    for (const k of order) {
      let li = this.items.get(k);
      if (!li) { li = itemShell(); li.dataset.key = k; this.items.set(k, li); }
      this.patch(li, this.byId.get(k));
    }
    for (const [k, li] of this.items) {
      if (this.byId.has(k)) continue;
      li.remove();
      this.items.delete(k);
    }
    // 순서 맞추기 — 제자리인 칸은 건드리지 않는다(옮기면 그림이 다시 그려지고 초점이 풀린다).
    let ref = this.grid.firstElementChild;
    for (const k of order) {
      const li = this.items.get(k);
      if (li === ref) ref = ref.nextElementSibling;
      else this.grid.insertBefore(li, ref);
    }
    if (this.grid.lastElementChild !== this.moreItem) this.grid.append(this.moreItem);
    this.order = order;

    const total = order.length;
    this.count.textContent = total ? String(total) : '';
    this.count.hidden = !total;
    this.sortBox.hidden = total < LF_SORT_MIN;
    this.grid.hidden = !total;
    this.empty.hidden = Boolean(total);
    this.el.classList.toggle('is-empty', !total);
    this.applyLimit();

    // 초점이 있던 칸이 사라졌으면(방송 종료) 같은 자리의 칸으로 — 문서 처음으로 떨어지지 않게.
    if (focused && !focused.isConnected) {
      const navs = this.navs();
      const next = navs[Math.min(Math.max(focusedIndex, 0), navs.length - 1)];
      if (next) this.focusNav(next, { scroll: false });
    }
    // 키보드 초점이 숨은 정렬 단추(셋 이하)·격자(0개)와 함께 사라졌다 — 첫 칸, 칸이 없으면 '전체 보기'로(body 로 떨어지지 않게).
    // 마우스로 남은 초점은 그대로 둔다 — 다시 잡으면 바쁜 것으로 보여 자리 바꿈을 미뤘다.
    const now = document.activeElement;
    if (kbdIn && (!now || now === document.body || now.closest('[hidden]'))) {
      const first = this.navs()[0];
      if (first) this.focusNav(first, { scroll: false });
      else this.focusQuiet(this.el.querySelector('.lf-all'));
    }
    if (this.peekKey) {
      if (this.byId.has(this.peekKey) && !this.items.get(this.peekKey).hidden) this.fillPeek(this.peekKey, { keepMedia: true });
      else this.hidePeek();
    }
  }

  /** 한 칸을 고친다 — 바뀐 값만 쓴다. */
  patch(li, l) {
    const fav = this.favorites.has(l.channelId);
    setAttr(li, 'data-channel-id', l.channelId);
    setAttr(li, 'data-channel-name', l.channelName || '');
    setAttr(li, 'data-channel-image', safeUrl(l.channelImageUrl));
    li.classList.toggle('is-fav', fav);
    const open = li.firstElementChild;
    setAttr(open, 'aria-label', liveFollowLabel(l, { favorite: fav }));
    const add = open.nextElementSibling;
    setAttr(add, 'aria-label', `${l.channelName || '채널'} 멀티라이브에 추가`);
    const ava = open.firstElementChild;
    const avatar = safeUrl(l.channelImageUrl);
    if (ava.dataset.src !== avatar) { ava.dataset.src = avatar; ava.innerHTML = avatarHtml(avatar); }
    const text = ava.nextElementSibling;
    const [name, title, meta] = text.children;
    setText(name.firstElementChild, l.channelName || '채널');
    // 표시(인증·즐겨찾기 별·19)는 칸마다 하나씩 두고 켜고 끈다 — 별을 붙일 때 인증 마크 그림을 다시 만들지 않게.
    const markBox = name.lastElementChild;
    const [vb, star, adult] = markBox.children;
    const kind = badgeKind(l) || '';
    if (vb.dataset.kind !== kind) { vb.dataset.kind = kind; vb.innerHTML = verifiedBadge(l, 'sm'); }
    vb.hidden = !kind;
    if (star.hasAttribute('hidden') === fav) star.toggleAttribute('hidden', !fav); // SVG 요소엔 .hidden 속성이 없다
    adult.hidden = !l.adult;
    markBox.hidden = !(kind || fav || l.adult);
    setText(title, l.liveTitle || '제목 없음');
    const [viewers, cat, up] = meta.children;
    setText(viewers.lastElementChild, l.viewerCountText || compactCount(l.viewerCount || 0));
    setText(cat, l.categoryName || '');
    cat.hidden = !l.categoryName;
    const upText = uptimeText(l.openDate);
    setText(up, upText);
    up.hidden = !upText;
  }

  // ── 접기·펼치기 ─────────────────────────────────

  applyLimit() {
    const total = this.order.length;
    const lim = collapseLimit(total, this.cols);
    const shown = this.expanded ? total : lim.shown;
    // 초점 있던 칸이 숨겨지면 초점이 body 로 사라졌다 — 아래에서 옮긴다. 타일은 창·사이드바 폭이 줄거나 접힐 때,
    // '+N 더 보기'·'접기'는 넘침이 없어질 때(창이 넓어짐·방송 종료) 숨는다. 숨기기 전의 화면 순서 자리를 재 둔다.
    const a = document.activeElement;
    const focusLi = a && a !== document.body && this.grid.contains(a) ? a.closest('.lf-item') : null;
    const focusIdx = focusLi ? this.navs().indexOf(focusLi.firstElementChild) : -1;
    this.order.forEach((k, i) => {
      const li = this.items.get(k);
      const hide = i >= shown;
      if (li.hidden !== hide) li.hidden = hide;
    });
    const overflow = total > lim.slots;
    const more = this.moreBtn;
    this.moreItem.hidden = !overflow;
    if (overflow) {
      const count = more.querySelector('.lf-more-count');
      const label = more.querySelector('.lf-more-label');
      const stack = more.querySelector('.lf-stack');
      if (this.expanded) {
        setText(count, '접기');
        setText(label, `${total}개 모두 보는 중`);
        setAttr(more, 'aria-label', '방송 중인 팔로잉 접기');
        setAttr(more, 'aria-expanded', 'true');
        if (stack.dataset.keys !== 'up') { stack.dataset.keys = 'up'; stack.innerHTML = `<span class="lf-collapse-ico">${chevronUp}</span>`; }
      } else {
        setText(count, `+${lim.hidden}`);
        setText(label, '더 보기');
        setAttr(more, 'aria-label', `방송 중인 팔로잉 ${lim.hidden}개 더 보기`);
        setAttr(more, 'aria-expanded', 'false');
        const next = this.order.slice(lim.shown, lim.shown + 3);
        const keys = next.join('\n');
        if (stack.dataset.keys !== keys) {
          stack.dataset.keys = keys;
          stack.innerHTML = next.map((k) => avatarHtml(this.byId.get(k).channelImageUrl)).join('');
        }
      }
    }
    if (this.peekKey && this.items.get(this.peekKey)?.hidden) this.hidePeek();
    if (focusLi && focusLi.hidden) {
      // 접힌 뒤쪽으로 숨은 타일 → 그 칸들을 대신하는 '+N 더 보기'. 사라진 더 보기·접기(또는 더 보기도 없을 때) → 같은 자리의 칸
      // (더 보기 자리엔 처음 드러난 칸이 온다).
      let target = focusLi !== this.moreItem && !this.moreItem.hidden ? this.moreBtn : null;
      if (!target) {
        const navs = this.navs();
        target = navs[Math.min(Math.max(focusIdx, 0), navs.length - 1)] || null;
      }
      if (target) { this.focusNav(target, { scroll: false }); return; }
    }
    this.syncRoving();
  }

  setExpanded(on, { focusFirst = false } = {}) {
    const before = collapseLimit(this.order.length, this.cols).shown;
    // 접기 — 펼친 목록 끝의 '접기' 단추가 접히며 위로 올라가 화면 밖으로 사라지고 보던 자리도 바뀌었다.
    // 누르기 전 단추의 화면 위치를 재 두었다가, 접은 뒤 스크롤을 그만큼 되돌려 단추를 제자리에 둔다(누른 뒤라 강제 레이아웃은 괜찮다).
    const anchor = !on && !this.moreItem.hidden && this.moreBtn.isConnected ? this.moreBtn.getBoundingClientRect().top : null;
    this.expanded = on;
    this.hidePeek();
    this.applyLimit(); // 숨겨진 칸에 있던 초점은 여기서 '접기'(→ '+N 더 보기')로 옮긴다
    if (on && focusFirst) {
      const li = this.items.get(this.order[before]);
      if (li) this.focusNav(li.firstElementChild);
    }
    if (anchor !== null && !this.moreItem.hidden) {
      const delta = this.moreBtn.getBoundingClientRect().top - anchor;
      const sc = scrollParentOf(this.el);
      if (sc && Math.abs(delta) > 0.5) sc.scrollBy({ top: delta, behavior: 'instant' });
      const r = this.moreBtn.getBoundingClientRect();
      if (r.top < 0 || r.bottom > window.innerHeight) this.moreBtn.scrollIntoView({ block: 'nearest' }); // 위로 더 갈 수 없을 때
    }
  }

  // ── 로빙 초점 ───────────────────────────────────

  /** 화살표로 오가는 칸(보이는 타일의 주 단추 + 더 보기) — 화면 순서. */
  navs() {
    const out = [];
    for (const li of this.grid.children) {
      if (li.hidden) continue;
      out.push(li.firstElementChild);
    }
    return out;
  }

  keyOf(nav) {
    return nav === this.moreBtn ? '__more' : nav.closest('.lf-item').dataset.key;
  }

  /** Tab 이 들어올 칸을 하나만 남긴다 — 그 칸의 + 도 Tab 으로 닿는다. */
  syncRoving() {
    const navs = this.navs();
    if (!navs.length) return;
    let active = navs.find((n) => this.keyOf(n) === this.activeKey);
    if (!active) { active = navs[0]; this.activeKey = this.keyOf(active); }
    for (const li of this.grid.children) {
      const on = li.firstElementChild === active;
      const t = on ? 0 : -1;
      if (li.firstElementChild.tabIndex !== t) li.firstElementChild.tabIndex = t;
      const add = li.querySelector('.lf-add');
      if (add && add.tabIndex !== t) add.tabIndex = t;
    }
  }

  focusNav(nav, { scroll = true } = {}) {
    this.activeKey = this.keyOf(nav);
    this.syncRoving();
    this.focusQuiet(nav);
    if (scroll) nav.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  /** 스크립트로 옮기는 초점 — focusin 이 그 초점의 :focus-visible 로 입력 방식을 바꾸지 않게(입력은 사용자가 정한다). */
  focusQuiet(el) {
    this.selfFocus = true;
    try { el.focus({ preventScroll: true }); } finally { this.selfFocus = false; }
  }

  // ── 홈 다시 그리기 ──────────────────────────────

  /** 홈 본문을 갈아 끼우기 직전 — 초점 자리를 기억하고 미리보기를 닫는다. */
  beforeSwap() {
    this.hidePeek();
    const a = document.activeElement;
    this.restore = null;
    if (!a || !this.el.contains(a)) return;
    const li = a.closest('.lf-item');
    this.restore = {
      key: li ? li.dataset.key || '__more' : null,
      idx: li ? this.navs().indexOf(li.firstElementChild) : -1, // 화면 순서 자리 — 그 칸이 사라졌을 때 같은 자리로
      sel: a.matches('.lf-add') ? '.lf-add' : a.dataset.lfSort ? `[data-lf-sort="${a.dataset.lfSort}"]` : a.matches('.lf-all') ? '.lf-all' : null,
    };
  }

  /** 새 홈 본문의 자리(slot — 아직 문서 밖 템플릿 안)를 이 구역으로 바꾼다. 칸 수는 문서에 붙은 뒤 ResizeObserver 가 고친다(페인트 전). */
  mount(slot) {
    this.unwatch();
    slot.replaceWith(this.el);
  }

  /** 문서에 붙은 뒤 — beforeSwap 이 기억한 초점 자리로 되돌린다. 그 사이 사용자가 다른 곳으로 초점을 옮겼으면 그대로 둔다. */
  restoreFocus() {
    const r = this.restore;
    this.restore = null;
    if (!r) return;
    const a = document.activeElement;
    if (a && a !== document.body && a !== document.documentElement) return;
    let target = null;
    if (r.key) {
      const li = r.key === '__more' ? this.moreItem : this.items.get(r.key);
      if (li && !li.hidden) target = li.querySelector(r.sel || '.lf-open, .lf-more');
      // 다시 받은 목록에서 '+N 더 보기' 뒤로 밀렸다 → 더 보기. 사라졌다(방송 종료·더 보기 없음) → 같은 자리의 칸.
      else if (li && li !== this.moreItem && !this.moreItem.hidden) target = this.moreBtn;
      else {
        const navs = this.navs();
        target = navs[Math.min(Math.max(r.idx, 0), navs.length - 1)] || null;
      }
    } else if (r.sel) {
      target = this.el.querySelector(r.sel);
      if (target && target.closest('[hidden]')) target = this.navs()[0] || null; // 정렬 단추가 숨었다(셋 이하)
    }
    if (target) this.focusQuiet(target);
  }

  /** 손을 뗐다 — 미뤄 둔 자리 바꿈을 한다. */
  settle() {
    if (!this.reorderPending || this.busy()) return;
    this.render({ force: true });
  }

  // ── 이벤트 ─────────────────────────────────────

  onClick(e) {
    const sortBtn = e.target.closest('[data-lf-sort]');
    if (sortBtn) {
      const v = sortBtn.dataset.lfSort;
      if (v === this.sort) return;
      this.sort = v;
      this.el.dataset.sort = v;
      saveLiveSort(v);
      for (const b of this.sortBox.querySelectorAll('.seg')) b.classList.toggle('active', b === sortBtn);
      this.hidePeek();
      this.render({ force: true });
      return;
    }
    if (e.target.closest('.lf-more')) {
      // 키보드(Enter·Space)로 펼치면 처음 드러난 칸으로 초점을 옮긴다 — 마우스는 그 자리에 둔다.
      this.setExpanded(!this.expanded, { focusFirst: e.detail === 0 });
      return;
    }
    // 타일·카드 열기·+ 는 홈 본문 위임이 맡는다 — 여기서는 미리보기만 닫는다. 위임(본문 click)이 누른 요소를 찾을 수 있게
    // 이 클릭이 다 퍼진 뒤에 비운다(바로 비우면 카드 안 단추가 문서에서 떨어져 위임이 무시했다).
    if (e.target.closest('.lf-item, .lf-peek')) setTimeout(() => this.hidePeek(), 0);
  }

  onKey(e) {
    // Esc 는 카드가 열린 동안 문서에서 먼저 받는다(onEscDoc) — 여기서는 다루지 않는다.
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (NAV_KEYS.has(e.key)) this.setInput('keyboard');
    const li = e.target.closest && e.target.closest('.lf-item');
    if (li && li !== this.moreItem && (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey))) {
      // 키보드 우클릭 — 카드 메뉴(app.js)를 칸 왼쪽 아래에 연다(엔진마다 좌표가 0,0 이거나 없다).
      e.preventDefault();
      const r = li.getBoundingClientRect();
      li.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: Math.round(r.left + 16), clientY: Math.round(r.bottom - 6) }));
      return;
    }
    const nav = e.target.closest && e.target.closest('.lf-open, .lf-more');
    if (!nav || !this.grid.contains(nav)) return;
    const navs = this.navs();
    const i = navs.indexOf(nav);
    if (i < 0) return;
    const cols = this.cols;
    let j;
    switch (e.key) {
      case 'ArrowRight': j = i + 1; break;
      case 'ArrowLeft': j = i - 1; break;
      case 'ArrowDown':
        j = i + cols;
        // 아랫줄이 짧으면 그 줄 끝으로
        if (j >= navs.length && Math.floor(i / cols) < Math.floor((navs.length - 1) / cols)) j = navs.length - 1;
        break;
      case 'ArrowUp': j = i - cols; break;
      case 'Home': j = 0; break;
      case 'End': j = navs.length - 1; break;
      default: return;
    }
    e.preventDefault();
    if (j < 0 || j >= navs.length || j === i) return;
    this.focusNav(navs[j]);
  }

  onFocusIn(e) {
    const t = e.target;
    // 카드 단추를 마우스로 누르면 그 단추가 초점을 받는다 — 여기서 카드를 닫으면 단추가 문서에서 빠져 click 이 사라졌다.
    if (this.peek.contains(t)) return;
    const li = t.closest('.lf-item');
    if (li && this.grid.contains(li)) {
      const nav = li.firstElementChild;
      if (this.activeKey !== this.keyOf(nav)) { this.activeKey = this.keyOf(nav); this.syncRoving(); }
    }
    // 키보드로 들어온 초점 — 밖에서 Tab 으로 들어오면 그 keydown 은 구역 밖이라 onKey 가 못 본다. 탐색 키 바로 다음이고 :focus-visible 이면
    // 키보드다. 이 구역이 스크립트로 옮긴 초점(focusQuiet), 다른 곳이 Esc 로 닫히며 돌려준 초점(navKey 아님)은 입력 방식을 바꾸지 않는다.
    let kbd = this.input === 'keyboard';
    if (!kbd && !this.selfFocus && this.navKey) {
      try { kbd = t.matches(':focus-visible'); } catch { kbd = true; } // :focus-visible 이 없는 엔진은 예전처럼
      if (kbd) this.setInput('keyboard');
    }
    if (!t.matches('.lf-open')) {
      if (t.matches('.lf-more') || !li) this.hidePeek();
      return;
    }
    if (!kbd) return; // 마우스로 누른 초점은 호버가 맡는다
    this.schedulePeek(li.dataset.key, PEEK_FOCUS_MS, 'focus');
  }

  onFocusOut(e) {
    const to = e.relatedTarget;
    if (to && this.el.contains(to)) return;
    if (!this.el.matches(':hover')) this.hidePeek();
    // 키보드가 목록을 떠났다 — 미뤄 둔 자리 바꿈(마우스가 없다면)
    queueMicrotask(() => this.settle());
  }

  /**
   * 손이 움직이지 않았는데 온 경계 이벤트 — 스크롤(화살표 초점 이동의 scrollIntoView·휠)로 포인터 아래 칸이 바뀌면 엔진이 같은 자리에서
   * pointerover/out 을 보낸다(Chromium 은 바로, WebKit 은 약 100ms 뒤). 그대로 받으면 키보드 초점 칸의 열기 예약을 지워, 포인터
   * 아래 칸의 카드가 뜨거나 아무것도 뜨지 않았다. 실제 움직임은 경계 이벤트가 pointermove 보다 먼저 오므로 자리가 다르다.
   */
  isSynthetic(e) {
    // 아직 pointermove 를 하나도 못 봤으면 합성이다 — 앱이 뜨기 전부터 커서가 창 위에 있으면 Chromium 은 pointermove 없이 경계 이벤트만
    // 보낸다. 처음 실제로 움직이면 그 pointermove 가 칸에 들어온 것으로 처리한다(onPointerMove).
    if (Number.isNaN(this.ptrX)) return true;
    return e.clientX === this.ptrX && e.clientY === this.ptrY;
  }

  /** 포인터가 이 구역의 타일(더 보기 칸 빼고)이나 카드 위인가. */
  isStripTarget(t) {
    if (!(t instanceof Element)) return false;
    if (this.peek.contains(t)) return true;
    const li = t.closest('.lf-item');
    return Boolean(li && li !== this.moreItem && this.grid.contains(li));
  }

  onPointerOver(e) {
    if (e.pointerType === 'touch' || this.isSynthetic(e)) return;
    if (this.peek.contains(e.target)) { clearTimeout(this.timers.hide); clearTimeout(this.timers.show); return; }
    const li = e.target.closest('.lf-item');
    if (!li || li === this.moreItem || !this.grid.contains(li)) return;
    this.hoverTile(li);
  }

  /** 합성 이벤트를 걸렀으니, 스크롤로 아래에 온 칸에서 실제로 움직이기 시작하면 그 칸에 들어온 것으로 본다. */
  onPointerMove(e) {
    if (e.pointerType === 'touch' || !this.ptrMoved) return;
    const li = e.target.closest && e.target.closest('.lf-item');
    const key = li && li !== this.moreItem && this.grid.contains(li) ? li.dataset.key : null;
    if (key && key !== this.hoverKey) this.hoverTile(li); // 칸·카드 밖으로의 실제 움직임은 문서 pointermove(leaveTile)가 맡는다
  }

  hoverTile(li) {
    clearTimeout(this.timers.hide);
    const key = li.dataset.key;
    this.hoverKey = key;
    if (key === this.peekKey) { clearTimeout(this.timers.show); return; }
    this.schedulePeek(key, this.peekKey ? PEEK_SWITCH_MS : PEEK_HOVER_MS, 'hover');
  }

  onPointerOut(e) {
    if (e.pointerType === 'touch' || this.isSynthetic(e)) return;
    const to = e.relatedTarget;
    const from = e.target.closest('.lf-item, .lf-peek');
    if (!from || (to instanceof Node && from.contains(to))) return;
    if (to instanceof Node && this.peek.contains(to)) return;
    const toLi = to instanceof Element ? to.closest('.lf-item') : null;
    if (toLi && toLi !== this.moreItem && this.grid.contains(toLi)) return; // 옆 칸으로 — pointerover 가 맡는다
    this.leaveTile();
  }

  /** 마우스가 칸·카드를 떠났다 — 마우스 열기 예약을 지우고 잠시 뒤 카드를 닫는다. 키보드 초점의 카드·예약은 남긴다. */
  leaveTile() {
    this.hoverKey = null;
    if (this.pendingFrom !== 'focus') clearTimeout(this.timers.show);
    const focusKey = document.activeElement && document.activeElement.closest && document.activeElement.closest('.lf-item')?.dataset.key;
    if (this.peekKey && focusKey === this.peekKey) return;
    clearTimeout(this.timers.hide);
    // 닫을 때도 그 사이 잡힌 키보드 초점의 열기 예약은 남긴다(포인터가 격자를 떠나는 길에 화살표를 누른 경우)
    this.timers.hide = setTimeout(() => this.hidePeek({ keepPending: this.pendingFrom === 'focus' }), PEEK_HIDE_MS);
  }

  // ── 미리보기 카드 ───────────────────────────────

  /** @param {'hover'|'focus'} from 마우스 머묾인지 키보드 초점인지 — 스크롤 때 남길 예약을 가른다(onAway). */
  schedulePeek(key, delay, from) {
    clearTimeout(this.timers.show);
    this.pendingFrom = null;
    if (key !== this.peekDismissed) this.peekDismissed = null;
    if (this.peekDismissed === key) return;
    this.pendingFrom = from;
    this.timers.show = setTimeout(() => { this.pendingFrom = null; this.showPeek(key, from); }, delay);
  }

  showPeek(key, from) {
    const li = this.items.get(key);
    if (!li || li.hidden || !li.isConnected || !this.byId.has(key)) return;
    // 마우스 예약은 그 칸이 아직 포인터 아래일 때만 — 기다리는 동안 스크롤로 칸이 밀려났으면 뜨지 않는다. :hover 는 WebKit 에서 스크롤이
    // 멈추고 약 100ms 뒤(가짜 mousemove)에야 바뀌어, 그 사이 시간이 되면 밀려난 칸의 카드가 떴다 — 마지막 포인터 자리를 지금 배치에 맞춰 본다.
    if (from === 'hover') {
      const at = Number.isFinite(this.ptrX) ? document.elementFromPoint(this.ptrX, this.ptrY) : null;
      if (!at || !li.contains(at)) return;
    }
    clearTimeout(this.timers.hide);
    const fresh = this.peek.hidden;
    this.fillPeek(key);
    this.peek.hidden = false;
    this.place(li);
    if (fresh) {
      this.peek.classList.remove('on');
      void this.peek.offsetWidth; // 처음 열 때만 떠오른다(옆 칸으로 옮길 땐 자리만 바꾼다)
      document.addEventListener('pointerdown', this.onAway, true);
      document.addEventListener('keydown', this.onEscDoc, true);
      document.addEventListener('scroll', this.onAway, { capture: true, passive: true });
      window.addEventListener('resize', this.onAway);
      window.addEventListener('blur', this.onAway);
    }
    this.peek.classList.add('on');
    for (const x of this.grid.querySelectorAll('.lf-item.peeking')) x.classList.remove('peeking');
    li.classList.add('peeking');
  }

  /** @param {{ keepPending?: boolean }} [opts] keepPending — 다른 칸의 열기 예약은 그대로 둔다(키보드 초점 이동이 일으킨 스크롤로 닫을 때). */
  hidePeek({ keepPending = false } = {}) {
    if (!keepPending) { clearTimeout(this.timers.show); this.pendingFrom = null; }
    clearTimeout(this.timers.hide);
    if (!this.peekKey && this.peek.hidden) return;
    this.peekKey = null;
    this.peek.hidden = true;
    this.peek.classList.remove('on');
    this.peek.innerHTML = ''; // 영상 미리보기(hover-preview)는 자리가 문서에서 빠지면 스스로 정리한다
    // 닫힌 카드는 채널 카드가 아니다 — 카드 위임·우클릭 메뉴가 찾지 않게 채널 표지를 뗀다
    delete this.peek.dataset.channelId;
    delete this.peek.dataset.channelName;
    delete this.peek.dataset.channelImage;
    for (const x of this.grid.querySelectorAll('.lf-item.peeking')) x.classList.remove('peeking');
    document.removeEventListener('pointerdown', this.onAway, true);
    document.removeEventListener('keydown', this.onEscDoc, true);
    document.removeEventListener('scroll', this.onAway, { capture: true });
    window.removeEventListener('resize', this.onAway);
    window.removeEventListener('blur', this.onAway);
  }

  /** 카드 내용 — 갱신 때(keepMedia)는 썸네일 칸을 그대로 두어 영상 미리보기가 끊기지 않게 글자만 고친다. */
  fillPeek(key, { keepMedia = false } = {}) {
    const l = this.byId.get(key);
    if (!l) return;
    const p = this.peek;
    p.dataset.channelId = l.channelId;
    p.dataset.channelName = l.channelName || '';
    p.dataset.channelImage = safeUrl(l.channelImageUrl);
    const thumb = safeUrl(l.thumbnailUrl);
    const up = uptimeText(l.openDate);
    const badges = `<span class="badge-live">LIVE</span>${l.adult ? '<span class="badge-adult">19</span>' : ''}<span class="badge-viewers"><i class="dot"></i>${esc(l.viewerCountText || compactCount(l.viewerCount || 0))}명</span>${up ? `<span class="badge-uptime">${esc(up)}</span>` : ''}`;
    const body = `<p class="lf-peek-title">${esc(l.liveTitle || '제목 없음')}</p>`
      + `<p class="lf-peek-meta">${avatarHtml(l.channelImageUrl).replace('class="avatar"', 'class="avatar xs"')}<span class="lf-peek-ch ellipsis">${esc(l.channelName || '')}</span>${verifiedBadge(l, 'sm')}${l.categoryName ? `<span class="lf-peek-cat ellipsis">${esc(l.categoryName)}</span>` : ''}</p>`
      + '<div class="lf-peek-actions">'
      + `<button type="button" class="btn primary lf-peek-play" tabindex="-1">${icons.play}시청하기</button>`
      + `<button type="button" class="btn lf-peek-ml" data-ml-add tabindex="-1">${plusIcon}멀티라이브</button>`
      + '<button type="button" class="btn ghost lf-peek-info" data-open-channel tabindex="-1">채널 정보</button>'
      + '</div>';
    const media = p.querySelector('.lf-peek-media');
    if (keepMedia && media && this.peekKey === key && media.dataset.thumb === thumb) {
      for (const b of media.querySelectorAll('.badge-live, .badge-adult, .badge-viewers, .badge-uptime')) b.remove();
      media.insertAdjacentHTML('beforeend', badges);
      p.querySelector('.lf-peek-body').innerHTML = body;
    } else {
      p.innerHTML = `<div class="lf-peek-media" data-thumb="${esc(thumb)}"${l.adult ? '' : previewAttr('live', l.channelId)}>`
        + `${thumb ? `<img src="${esc(imageSrc(thumb, 'live'))}" alt="" decoding="async" referrerpolicy="no-referrer">` : `<span class="lf-peek-noimg">${avatarHtml(l.channelImageUrl)}</span>`}`
        + `${badges}</div><div class="lf-peek-body">${body}</div>`;
    }
    this.peekKey = key;
  }

  /** 칸 아래(자리가 없으면 위)에, 구역 밖으로 나가지 않게. */
  place(li) {
    const p = this.peek;
    const sec = this.el.getBoundingClientRect();
    const r = li.getBoundingClientRect();
    const w = p.offsetWidth;
    const h = p.offsetHeight;
    const left = Math.max(0, Math.min(r.left - sec.left + r.width / 2 - w / 2, sec.width - w));
    const gap = 8;
    const below = r.bottom + gap + h <= window.innerHeight || r.top - gap - h < 0;
    p.style.left = `${Math.round(left)}px`;
    p.style.top = `${Math.round(below ? r.bottom - sec.top + gap : r.top - sec.top - gap - h)}px`;
    p.dataset.side = below ? 'below' : 'above';
  }
}

/** 홈 스켈레톤의 이 구역 — 접힌 두 줄 높이를 미리 잡아 데이터가 와도 아래 히어로가 덜 밀린다. */
export function liveFollowingSkeleton(tiles = 8) {
  return `<section class="home-section lf lf-skeleton" aria-hidden="true"><div class="sk lf-sk-title"></div><div class="lf-grid">${'<div class="sk lf-sk-tile"></div>'.repeat(tiles)}</div></section>`;
}
