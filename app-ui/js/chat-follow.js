// 채팅 바닥 따라가기 — "바닥을 따라갈지"를 매번 스크롤 위치로 다시 재지 않고, 사용자가 직접 올렸을 때만 끈다.
// 스크롤 이벤트는 위치를 바꾼 뒤 한 박자 늦게 오므로, 그사이 새 줄이 붙거나 이모티콘·배지 이미지가 늦게 로드돼
// 높이가 늘면 우리가 바닥으로 내린 스크롤조차 "바닥이 아님"으로 읽힌다. 붐비는 라이브 채팅에서는 이게 수시로 일어나
// 따라가기가 꺼지고 멈춰 있었다(앱이 가려졌다 돌아올 때 WebKit 이 예전 위치를 되돌리는 것도 같은 꼴).
// 그래서 휠·터치·키·포인터처럼 사용자 손이 닿은 스크롤만 따라가기를 끄고, 나머지는 바닥으로 되돌린다.
const BOTTOM_SLACK = 40;
const USER_GRACE_MS = 800;

const followers = new Set(); // WeakRef<Follower>
const byList = new WeakMap();
let pressed = null; // 스크롤바를 끌거나 글을 고르는 중인 목록

// 줄 등장(이벤트 카드·멘션·안내)과 6단계 후원 무지개 흐름은 한 번만 — CSS 애니메이션은 목록이 display:none 에서 다시
// 그려질 때(시청 화면을 떠났다 돌아옴·채팅 숨김·이 사람만 보기 해제·검색) 처음부터 다시 돈다. 300줄 목록의 카드가 한꺼번에
// 다시 튀어나오고 6단계 카드는 매 프레임 다시 칠하는 흐름을 10초 가까이 또 돌았다. 끝난 줄에 표시를 달아 다시 돌지 않게 한다(motion.css·app.css).
const LINE_DONE = { 'mo-pop-up': 'mo-in-done', 'mo-fade': 'mo-in-done', 'don-holo': 'mo-holo-done' };
export function markLineAnimationDone(e) {
  const cls = LINE_DONE[e.animationName];
  if (!cls || !e.target || !e.target.closest) return;
  const line = e.target.closest('.chat-line');
  if (line) line.classList.add(cls);
}

const atBottom = (list) => list.scrollHeight - list.scrollTop - list.clientHeight < BOTTOM_SLACK;

class Follower {
  constructor(list, enabled) {
    this.list = list;
    this.enabled = enabled || (() => true);
    this.stuck = true;
    this.userUntil = 0;
    this.touchedAt = 0; // 사용자 손이 마지막으로 닿은 때(followAppend 의 resumeMs)
    const touch = () => { this.touchedAt = performance.now(); this.userUntil = this.touchedAt + USER_GRACE_MS; };
    list.addEventListener('wheel', touch, { passive: true });
    list.addEventListener('touchstart', touch, { passive: true });
    list.addEventListener('keydown', touch);
    list.addEventListener('pointerdown', () => { pressed = this; touch(); });
    list.addEventListener('scroll', () => this.onScroll(), { passive: true });
    list.addEventListener('animationend', markLineAnimationDone);
    // 칸 크기가 바뀌거나(분할·보기 전환) 숨었다 다시 보이면 바닥에 있던 목록은 계속 바닥을 보이게 한다.
    if (typeof ResizeObserver === 'function') new ResizeObserver(() => { if (this.stuck && this.enabled()) this.snap(); }).observe(list);
  }

  get userActive() {
    return pressed === this || performance.now() < this.userUntil;
  }

  onScroll() {
    if (atBottom(this.list)) { this.stuck = true; return; }
    if (this.userActive && !document.hidden) { this.stuck = false; return; }
    // 사용자가 건드리지 않은 스크롤(늦게 늘어난 높이, 가려졌다 돌아올 때의 위치 복원) — 따라가던 중이면 바닥으로 되돌린다.
    if (this.stuck && this.enabled()) this.snap();
  }

  snap() {
    // 창이 숨어 있으면 내리지 않는다 — 아무도 보지 않는데 scrollHeight 를 읽으면 줄을 붙일 때마다 목록 전체
    // 레이아웃을 강제한다(가린 채 멀티라이브를 켜 두면 패인 채팅마다 계속). 다시 보이면 resyncAll 이 바닥으로 내린다.
    // 숨은 화면(채팅·시청·멀티라이브 페이지가 [hidden])의 목록도 같다 — 시작할 때 세 목록의 ResizeObserver·focus 가
    // 레이아웃을 강제했다. 화면이 다시 보이면 크기가 바뀌어 위 ResizeObserver 가 바닥으로 내린다.
    if (document.hidden) return;
    const l = this.list;
    if (l.closest('[hidden]')) return;
    if (l.scrollHeight - l.clientHeight - l.scrollTop > 1) l.scrollTop = l.scrollHeight;
  }

  /** 다시 보일 때 — 지금 한 번, 그리기가 다시 시작된 다음 프레임에 한 번 더. */
  resync() {
    if (!this.stuck || !this.enabled() || !this.list.isConnected) return;
    this.snap();
    requestAnimationFrame(() => { if (this.stuck) this.snap(); });
  }
}

/** 목록의 따라가기 상태 — 처음 부를 때 만든다. enabled 가 거짓이면(자동 스크롤 끔) 스스로 바닥으로 내리지 않는다. */
export function chatFollower(list, enabled) {
  let f = byList.get(list);
  if (!f) {
    f = new Follower(list, enabled);
    byList.set(list, f);
    followers.add(new WeakRef(f));
  }
  return f;
}

/**
 * 목록 끝에 붙이고 넘치면 앞에서 지운 뒤, 따라가던 중이면 바닥으로 내린다.
 * resumeMs — 따라가기가 꺼져 있어도 사용자가 그만큼 손대지 않았으면 다시 바닥을 따라간다. '새 메시지' 버튼이 없는
 * 작은 칸(멀티라이브 그리드·포커스)은 살짝 올린 뒤 그대로 두면 멈춘 칸처럼 보였다.
 */
export function followAppend(list, insert, { resumeMs = 0, snaps = null } = {}) {
  const f = chatFollower(list);
  if (!f.stuck && resumeMs > 0 && pressed !== f && performance.now() - f.touchedAt > resumeMs) f.stuck = true;
  const follow = f.stuck;
  insert(follow);
  // snaps — 여러 목록에 차례로 붙일 때(멀티라이브 채팅 그리드) 바닥 맞추기를 모았다가 붙이기가 다 끝난 뒤 한 번에
  // 한다(snapAll). 목록마다 붙이고(쓰기) 곧바로 scrollHeight 를 읽으면 칸 수만큼 강제 레이아웃이 일어났다.
  if (follow) { if (snaps) snaps.push(f); else f.snap(); }
  return follow;
}

/** followAppend(…, { snaps }) 로 모아 둔 목록을 바닥으로 — 읽기·쓰기가 섞이지 않게 붙이기를 모두 끝낸 뒤에 부른다. */
export function snapAll(snaps) {
  for (const f of snaps) f.snap();
  snaps.length = 0;
}

/**
 * 줄 상한 — 넘친 줄을 앞에서 지운다. 붙일 때마다 몇 줄씩 지우면 남은 줄이 모두 위로 밀려 목록 전체를 다시
 * 배치한다(붐비는 채팅에서 붙일 때마다 수백 줄 레이아웃 — 4배 감속 초당 200개에서 프레임 25%가 20ms 를 넘었다).
 * 그래서 상한의 1/4(최소 20줄)을 더 넘칠 때까지 두었다가 상한까지 한 번에 지운다. 지웠으면 참.
 */
export function trimFront(list, max, { keepView = false } = {}) {
  const children = list.children;
  if (children.length <= max + Math.max(20, max >> 2)) return false;
  const range = document.createRange();
  range.setStartBefore(children[0]);
  const cut = children.length - max;
  range.setEndAfter(children[cut - 1]);
  // 위로 올려 읽는 중이면 지운 높이만큼 스크롤을 당겨 보던 줄을 제자리에 둔다 — WebKit(macOS)은 스크롤 앵커링이 없어
  // 앞줄 수십 줄이 한꺼번에 빠지면 보던 자리가 수천 px 튀거나 바닥에 닿아 따라가기가 다시 켜지고 '새 메시지' 수가 0 이
  // 됐다. 지우기는 상한의 1/4 마다 한 번이라 이 측정(레이아웃 한 번)은 드물다. Chromium 이 이미 앵커링했으면 두 번 당기지 않는다.
  const before = keepView ? list.scrollTop : 0;
  const removed = keepView ? children[cut].offsetTop - children[0].offsetTop : 0;
  // 지울 줄의 아바타 사진은 떼어 낸 뒤 src 를 비워 디코드한 그림 메모리를 바로 놓아 준다(chat-avatar.js).
  const photos = [];
  for (let i = 0; i < cut; i++) {
    const img = children[i].querySelector('img.av-photo');
    if (img) photos.push(img);
  }
  range.deleteContents();
  for (const img of photos) img.removeAttribute('src');
  if (keepView && removed > 0) {
    const want = Math.max(0, before - removed);
    if (list.scrollTop > want + 1) list.scrollTop = want;
  }
  return true;
}

/**
 * 붙이기 간격 — 채팅 목록은 줄을 붙일 때마다 바닥 맞추기(snap)가 레이아웃을 한 번 돌린다. 그 비용은 붙인 줄 수보다
 * 붙인 횟수에 더 든다(WebKit, 300줄 목록: 1줄 3ms·10줄 6ms·40줄 14ms). 메시지가 웹소켓 프레임마다 따로 오면
 * 프레임마다 붙여 초당 수십 번 레이아웃을 돌렸다. 그래서 바로 앞 붙이기에서 FLUSH_GAP_MS 가 지나지 않았으면
 * 그때까지 모았다가 한 번에 붙인다 — 한산한 채팅은 예전처럼 다음 프레임에 바로 붙는다.
 */
export const FLUSH_GAP_MS = 100;

/**
 * 한 번에 붙일 줄 — 간격을 기다리는 동안 이만큼 모이면 더 기다리지 않고 다음 프레임에 붙인다. 붐빌 때 100ms 를
 * 다 채우면 20줄 넘게 한꺼번에 붙어 그 프레임이 길어졌다(4배 감속 초당 200개에서 20ms 넘은 프레임 4% → 12%).
 */
export const FLUSH_BATCH_LINES = 8;

/**
 * 묶어 붙이기 예약 — owner 의 flushScheduled·flushRaf·flushTimer·flushGap·lastFlushAt 을 쓴다.
 * 보이면 (간격을 지킨 뒤) 다음 프레임, 창이 숨어 rAF 가 멈춰도 타이머로 결국 붙인다(숨은 창은 hiddenMs 마다).
 */
export function scheduleChatFlush(owner, run, { hiddenMs = 1000, pending = 0 } = {}) {
  const full = pending >= FLUSH_BATCH_LINES;
  const frame = () => { owner.flushGap = 0; owner.flushRaf = requestAnimationFrame(run); };
  if (owner.flushScheduled) {
    // 간격을 기다리는 중에 한 번 붙일 만큼 모였으면 바로 다음 프레임으로 당긴다.
    if (full && owner.flushGap) { clearTimeout(owner.flushGap); frame(); }
    return;
  }
  owner.flushScheduled = true;
  const hidden = document.hidden;
  const wait = hidden || full ? 0 : Math.max(0, (owner.lastFlushAt || 0) + FLUSH_GAP_MS - performance.now());
  if (wait > 0) owner.flushGap = setTimeout(frame, wait);
  else frame();
  owner.flushTimer = setTimeout(run, (hidden ? hiddenMs : 250) + wait);
}

/** 붙이기를 시작할 때(또는 버릴 때) — 남은 예약을 지우고 간격을 잰다. */
export function flushStarted(owner, { ran = true } = {}) {
  cancelAnimationFrame(owner.flushRaf);
  clearTimeout(owner.flushTimer);
  clearTimeout(owner.flushGap);
  owner.flushGap = 0;
  owner.flushScheduled = false;
  if (ran) owner.lastFlushAt = performance.now();
}

function resyncAll() {
  for (const ref of followers) {
    const f = ref.deref();
    if (f) f.resync(); else followers.delete(ref);
  }
}

document.addEventListener('visibilitychange', () => { if (!document.hidden) resyncAll(); });
window.addEventListener('focus', resyncAll);
const release = () => {
  if (pressed) { pressed.touchedAt = performance.now(); pressed.userUntil = pressed.touchedAt + USER_GRACE_MS; pressed = null; }
};
window.addEventListener('pointerup', release, true);
window.addEventListener('pointercancel', release, true);
// 플레이어(iframe) 위에서 손을 떼면 pointerup 이 여기 오지 않는다 — 버튼이 떨어진 채 움직이면 놓은 것으로 본다.
window.addEventListener('pointermove', (e) => { if (pressed && e.buttons === 0) release(); }, true);
window.addEventListener('blur', release);
window.addEventListener('pageshow', resyncAll);
