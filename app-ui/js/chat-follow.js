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

const atBottom = (list) => list.scrollHeight - list.scrollTop - list.clientHeight < BOTTOM_SLACK;

class Follower {
  constructor(list, enabled) {
    this.list = list;
    this.enabled = enabled || (() => true);
    this.stuck = true;
    this.userUntil = 0;
    const touch = () => { this.userUntil = performance.now() + USER_GRACE_MS; };
    list.addEventListener('wheel', touch, { passive: true });
    list.addEventListener('touchstart', touch, { passive: true });
    list.addEventListener('keydown', touch);
    list.addEventListener('pointerdown', () => { pressed = this; touch(); });
    list.addEventListener('scroll', () => this.onScroll(), { passive: true });
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
    const l = this.list;
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

/** 목록 끝에 붙이고 넘치면 앞에서 지운 뒤, 따라가던 중이면 바닥으로 내린다. */
export function followAppend(list, insert) {
  const f = chatFollower(list);
  const follow = f.stuck;
  insert();
  if (follow) f.snap();
  return follow;
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
  if (pressed) { pressed.userUntil = performance.now() + USER_GRACE_MS; pressed = null; }
};
window.addEventListener('pointerup', release, true);
window.addEventListener('pointercancel', release, true);
// 플레이어(iframe) 위에서 손을 떼면 pointerup 이 여기 오지 않는다 — 버튼이 떨어진 채 움직이면 놓은 것으로 본다.
window.addEventListener('pointermove', (e) => { if (pressed && e.buttons === 0) release(); }, true);
window.addEventListener('blur', release);
window.addEventListener('pageshow', resyncAll);
