// 라이브 정보란(ml-watch-info.js)·채널 화면(channel.js)의 손맛 모션 — 숫자 카운트업·롤링, 카드 기울기·빛 번짐,
// 배너 마우스 패럴랙스, 탭 내용 방향 전환, LIVE 맥동 멈춤. 모양(키프레임·지연)은 motion.css "라이브 정보·채널 화면".
//
// 성능 규칙(motion.css 머리말과 같다):
// - transform·opacity 만 움직인다. 크기·위치를 읽는 것은 마우스가 카드·배너에 들어올 때 한 번(pointerenter)뿐이고,
//   움직이는 동안에는 이벤트 좌표와 그때 잰 상자로 계산해 CSS 변수만 바꾼다(requestAnimationFrame 으로 한 프레임 한 번).
// - 숫자 카운트업은 0.7초 남짓 rAF 로 글자만 바꾼다. 창이 가려져 있으면(document.hidden) 바로 끝 값.
// - 무한 반복은 LIVE 맥동 하나 — 창이 가려지거나(:root.mo-hidden) 그 카드가 화면 밖이면(.mo-off) 멈춘다.
// - 움직임 줄이기(설정 "절약"·시스템)면 전부 건너뛴다 — CSS 도 같은 조건에서 애니메이션을 none 으로 둔다.
import { motionReduced } from './motion.js';

const EASE_OUT = (t) => 1 - (1 - t) ** 3;
const SPRING = 'cubic-bezier(.34, 1.4, .64, 1)';
const EMPH = 'cubic-bezier(.2, 0, 0, 1)';

// `52.3만`, `12,400`, `1.2천`, `830` — 앞뒤 글(단위)은 그대로 두고 가운데 숫자만 센다.
const NUM = /^(\D*?)(\d{1,3}(?:,\d{3})+|\d+(?:\.\d+)?)(\D*)$/;
function parse(text) {
  const m = NUM.exec(String(text || '').trim());
  if (!m) return null;
  const raw = m[2];
  return { pre: m[1], post: m[3], value: Number(raw.replace(/,/g, '')), decimals: (raw.split('.')[1] || '').length, comma: raw.includes(',') };
}
function format(p, v) {
  const s = p.comma ? Math.round(v).toLocaleString('en-US') : v.toFixed(p.decimals);
  return `${p.pre}${s}${p.post}`;
}

// 세는 중인 숫자 — el → { raf, final }. 몇 개 안 되고 1초 안에 비워진다.
const counting = new Map();
function stopCount(el, finish) {
  const c = counting.get(el);
  if (!c) return;
  cancelAnimationFrame(c.raf);
  counting.delete(el);
  if (finish && el.isConnected) el.textContent = c.final;
}
// 창이 가려지면 rAF 가 멈춘다 — 다시 보일 때 남은 숫자가 굴러가지 않게 바로 끝 값으로
document.addEventListener('visibilitychange', () => { for (const el of [...counting.keys()]) stopCount(el, true); });

/** el 의 지금 글(최종 값)까지 0 에서 세어 올라간다. 끝나면 원래 글 그대로. */
export function countUp(el, { duration = 720, delay = 0 } = {}) {
  stopCount(el, true);
  const final = el.textContent;
  const p = parse(final);
  if (!p || !(p.value > 0) || motionReduced() || document.hidden) return;
  el.textContent = format(p, 0);
  const start = performance.now() + delay;
  const step = (now) => {
    const c = counting.get(el);
    if (!c) return;
    if (!el.isConnected) { counting.delete(el); return; } // 다시 그려 지워졌다
    const t = Math.min(1, Math.max(0, (now - start) / duration));
    el.textContent = t >= 1 ? final : format(p, p.value * EASE_OUT(t));
    if (t < 1) c.raf = requestAnimationFrame(step);
    else counting.delete(el);
  };
  counting.set(el, { raf: requestAnimationFrame(step), final });
}

/** root 안의 [data-mo-count] 를 앞에서부터 조금씩 늦춰 센다. */
export function countUpIn(root, { stagger = 70, delay = 120 } = {}) {
  let i = 0;
  for (const el of root.querySelectorAll('[data-mo-count]')) countUp(el, { delay: delay + stagger * i++ });
}

/** 숫자 글을 바꾼다 — 커지면 아래에서, 줄면 위에서 굴러 들어온다(.mo-num 은 inline-block). */
export function rollTo(el, text) {
  if (!el || el.textContent === text) return;
  const before = parse(el.textContent);
  const after = parse(text);
  stopCount(el, false);
  el.textContent = text;
  if (motionReduced() || document.hidden || !before || !after || before.value === after.value) return;
  const dir = after.value > before.value ? 1 : -1;
  el.animate([
    { transform: `translateY(${dir * 0.55}em)`, opacity: 0 },
    { transform: 'none', opacity: 1 },
  ], { duration: 380, easing: SPRING, id: 'mo-roll' });
}

/** 탭 내용 바꾸기 — 오른쪽 탭으로 가면 오른쪽에서, 왼쪽이면 왼쪽에서 들어온다. */
export function tabSwap(el, dir) {
  if (!el || !dir || motionReduced()) return;
  el.animate([
    { transform: `translateX(${dir * 14}px)`, opacity: 0 },
    { transform: 'none', opacity: 1 },
  ], { duration: 260, easing: EMPH, id: 'mo-tab-swap' });
}

// ── 카드 기울기·빛 번짐 ──────────────────────────────
// 썸네일(.thumb)만 기운다 — 카드는 호버로 이미 2px 떠오르고(.card:hover transform), 누르면 줄어든다.
const TILT = 6; // 최대 기울기(도)
function bindTilt(root, cardSel) {
  let box = null;
  let thumb = null;
  let frame = 0;
  let last = null;
  const reset = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    if (thumb) {
      thumb.classList.remove('mo-tilting');
      for (const k of ['--mo-rx', '--mo-ry', '--mo-gx', '--mo-gy']) thumb.style.removeProperty(k);
    }
    thumb = null;
    box = null;
  };
  const apply = () => {
    frame = 0;
    if (!thumb || !box || !last) return;
    const x = Math.min(1, Math.max(0, (last.clientX - box.left) / box.width));
    const y = Math.min(1, Math.max(0, (last.clientY - box.top) / box.height));
    thumb.style.setProperty('--mo-rx', `${((0.5 - y) * TILT).toFixed(2)}deg`);
    thumb.style.setProperty('--mo-ry', `${((x - 0.5) * TILT).toFixed(2)}deg`);
    // 빛 번짐은 배경을 다시 칠하지 않고 미리 그린 층을 옮긴다(translate, motion.css .mo-glare)
    thumb.style.setProperty('--mo-gx', x.toFixed(3));
    thumb.style.setProperty('--mo-gy', y.toFixed(3));
  };
  root.addEventListener('pointerover', (e) => {
    if (e.pointerType && e.pointerType !== 'mouse') return;
    const card = e.target.closest?.(cardSel);
    const t = card?.querySelector(':scope > .thumb');
    if (!t || t === thumb) return;
    reset();
    if (motionReduced()) return;
    thumb = t;
    box = t.getBoundingClientRect(); // 들어올 때 한 번 — 움직이는 동안에는 재지 않는다
    if (!t.querySelector(':scope > .mo-glare')) t.insertAdjacentHTML('beforeend', '<i class="mo-glare" aria-hidden="true"></i>');
    t.classList.add('mo-tilting');
    last = e;
    apply();
  });
  root.addEventListener('pointermove', (e) => {
    if (!thumb) return;
    last = e;
    if (!frame) frame = requestAnimationFrame(apply);
  }, { passive: true });
  root.addEventListener('pointerout', (e) => {
    if (!thumb) return;
    const card = thumb.closest(cardSel);
    if (card && !card.contains(e.relatedTarget)) reset();
  });
  // 스크롤하면 잰 상자가 틀어진다 — 스크롤 칸(.page 등)은 root 바깥이라 문서에서 받는다(가만히 있으면 바로 돌아간다)
  document.addEventListener('scroll', () => { if (thumb) reset(); }, { capture: true, passive: true });
}

// ── 배너 마우스 패럴랙스 — 흐린 배경이 마우스 반대쪽으로 조금 ──
function bindParallax(root, heroSel) {
  let hero = null;
  let box = null;
  let frame = 0;
  let last = null;
  const apply = () => {
    frame = 0;
    if (!hero || !box || !last) return;
    const x = (last.clientX - box.left) / box.width - 0.5;
    const y = (last.clientY - box.top) / box.height - 0.5;
    hero.style.setProperty('--mo-px', `${(-x * 18).toFixed(1)}px`);
    hero.style.setProperty('--mo-py', `${(-y * 12).toFixed(1)}px`);
  };
  root.addEventListener('pointerover', (e) => {
    if (e.pointerType && e.pointerType !== 'mouse') return;
    const h = e.target.closest?.(heroSel);
    if (!h || h === hero || motionReduced()) return;
    hero = h;
    box = h.getBoundingClientRect();
  });
  root.addEventListener('pointermove', (e) => {
    if (!hero) return;
    last = e;
    if (!frame) frame = requestAnimationFrame(apply);
  }, { passive: true });
  root.addEventListener('pointerout', (e) => {
    if (!hero || hero.contains(e.relatedTarget)) return;
    cancelAnimationFrame(frame);
    frame = 0;
    hero.style.removeProperty('--mo-px');
    hero.style.removeProperty('--mo-py');
    hero = null;
  });
}

/** 화면 루트에 한 번 — 카드 기울기·배너 패럴랙스를 위임으로 건다(다시 그려도 그대로). */
export function bindLiveMotion(root, { cards = '.card.media-card', hero = null } = {}) {
  bindTilt(root, cards);
  if (hero) bindParallax(root, hero);
}

// ── LIVE 맥동 멈춤 — 카드가 화면(스크롤 칸) 밖이면 .mo-off ──
const offscreen = typeof IntersectionObserver === 'function'
  ? new IntersectionObserver((entries) => { for (const e of entries) e.target.classList.toggle('mo-off', !e.isIntersecting); })
  : null;
/** 맥동이 있는 카드(머리)를 지켜본다 — 지워진 요소는 관찰자가 들고 있지 않게 다시 그릴 때 unwatch. */
export function watchPulse(el) { if (el && offscreen) offscreen.observe(el); }
export function unwatchPulse(el) { if (el && offscreen) offscreen.unobserve(el); }

/** 앱 시작 때 한 번 — 창이 가려지면 :root.mo-hidden(무한 반복 맥동을 멈춘다, motion.css). */
export function installLiveMotion() {
  const sync = () => document.documentElement.classList.toggle('mo-hidden', document.hidden);
  document.addEventListener('visibilitychange', sync);
  sync();
}
