// 채팅 닉네임 말풍선 — 닉네임(또는 아바타)을 누르면 그 사람 옆에 뜬다(CHT-25).
// 치지직이 주는 것: 프로필 카드(칭호·배지·이 채널 팔로우 날짜·구독 개월), 그 사람 채널(팔로워·방송 중·인증 마크),
// 내 팔로우 상태와 팔로우/언팔로우. 앱 쪽 기능: 이 사람 채팅만 보기·멘션·닉네임 복사·하이라이트·차단.
// - 위치는 누른 닉네임 아래(자리가 없으면 위), 꼬리가 닉네임을 가리킨다. 붐비는 채널은 줄이 1~2초 만에 밀려
//   지워지므로 말풍선은 연 자리에 머물고, 닉네임이 그 자리를 떠나면 꼬리만 감춘다(누르려던 버튼이 도망가지 않게).
// - 바깥 누르기·Esc·창 전환으로 닫는다. 열고 닫는 움직임은 motion.css 규칙(모션 줄이기면 바로)을 따른다.
import { api } from './api.js';
import { esc, safeUrl, imageSrc, toast, hideWithExit } from './dom.js';
import { verifiedBadge } from './verified-badge.js';
import { compactCount } from './cards.js';
import { chatPrefs } from './chat-prefs.js';

const ROLE_LABEL = { streamer: '스트리머', manager: '매니저', channelManager: '채널 관리자' };
// 같은 사람을 다시 누르면 바로 그리도록 잠깐 기억한다(팔로우 상태는 매번 새로 받는다).
const CARD_TTL_MS = 60_000;
const cardCache = new Map(); // `${chatChannelId}|${uid}` → { at, card }

let current = null; // { pop, close }

/** 열린 말풍선을 닫는다 — panel 을 주면 그 패널에서 연 것만(다른 채널로 옮길 때·패널을 닫을 때). */
export function closeUserPop(panel = null) {
  if (current && (!panel || current.panel === panel)) current.close();
}

/** "2024-02-19 23:28:11" → "2024.02.19" */
function shortDate(raw) {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(String(raw || ''));
  return m ? `${m[1]}.${m[2]}.${m[3]}` : '';
}

function copy(text, label) {
  if (!text) return;
  navigator.clipboard.writeText(text).then(() => toast(`${label}을 복사했습니다`, 1000), () => toast('복사하지 못했습니다'));
}

async function loadCard(uid, chatChannelId, fresh) {
  const key = `${chatChannelId || ''}|${uid}`;
  const hit = cardCache.get(key);
  if (!fresh && hit && Date.now() - hit.at < CARD_TTL_MS) return hit.card;
  const card = await api.chatUserCard(uid, chatChannelId || null);
  cardCache.set(key, { at: Date.now(), card });
  if (cardCache.size > 200) cardCache.delete(cardCache.keys().next().value);
  return card;
}

/**
 * @param {import('./chat.js').ChatPanel} panel
 * @param {HTMLElement} anchor 누른 닉네임·아바타
 * @param {HTMLElement} line 그 채팅 줄(.chat-line[data-uid])
 */
export function openUserPop(panel, anchor, line) {
  const uid = line.dataset.uid;
  const nick = line.dataset.nick || '익명';
  const role = line.dataset.role || '';
  if (!uid) return;
  // 같은 사람을 다시 누르면 닫기만 한다(토글).
  if (current && current.uid === uid && current.panel === panel) { current.close(); return; }
  current?.close();

  const pop = document.createElement('div');
  pop.className = 'user-pop';
  pop.setAttribute('role', 'dialog');
  pop.setAttribute('aria-label', `${nick} 정보`);
  pop.tabIndex = -1;
  pop.addEventListener('animationend', () => pop.classList.add('settled'), { once: true });
  document.body.append(pop);

  const badgesHtml = [...line.querySelectorAll('.badges img.badge')].map((b) => b.outerHTML).join('');
  const state = { card: null, loading: true, error: '', busy: false };

  const render = () => {
    const c = state.card || {};
    const prof = c.profile || null;
    const ch = c.channel || null;
    const loggedIn = c.loggedIn ?? panel.isLoggedIn();
    const isSelf = Boolean(c.isSelf);
    const prefs = chatPrefs();
    const highlighted = (prefs.highlightedUsers || []).includes(uid);
    const solo = panel.soloUid === uid;
    const name = (prof && prof.nickname) || nick;
    const img = safeUrl((prof && prof.profileImageUrl) || (ch && ch.channelImageUrl) || '');
    const initial = esc(Array.from(name)[0] || '•');
    const avatar = img
      ? `<img class="up-avatar" src="${esc(imageSrc(img, 'static'))}" alt="" referrerpolicy="no-referrer" decoding="async">`
      : `<span class="up-avatar up-avatar-text" aria-hidden="true">${initial}</span>`;
    const mark = verifiedBadge(prof ? prof.verifiedBadge : ch, 'sm');
    const titleText = (prof && prof.titleName) || ROLE_LABEL[role] || '';
    const titleColor = prof && /^#[0-9a-f]{3,8}$/i.test(prof.titleColor || '') ? ` style="--up-title:${prof.titleColor}"` : '';
    const title = titleText ? `<span class="up-title"${titleColor}>${esc(titleText)}</span>` : '';
    // 배지 — 카드 배지(설명 툴팁)가 오면 그것을, 아니면 채팅 줄의 배지 그림을 그대로.
    const cardBadges = prof && prof.badges && prof.badges.length
      ? prof.badges.slice(0, 10).map((b) => {
        const u = safeUrl(b.imageUrl);
        if (!u) return '';
        const tip = [b.title, b.description].filter(Boolean).join(' — ');
        return `<img class="up-badge${b.active ? ' on' : ''}" src="${esc(imageSrc(u, 'static'))}" alt="${esc(b.title || '')}" title="${esc(tip)}" referrerpolicy="no-referrer" decoding="async">`;
      }).join('')
      : badgesHtml;
    const facts = [];
    if (ch) facts.push(`팔로워 ${esc(compactCount(ch.followerCount || 0))}명`);
    if (ch && ch.openLive) facts.push('<span class="up-live">방송 중</span>');
    const since = prof && shortDate(prof.followDate);
    if (since) facts.push(`${since}부터 팔로우`);
    if (prof && prof.subscriptionMonths) facts.push(`<span class="up-sub">구독 ${esc(String(prof.subscriptionMonths))}개월</span>`);
    let factLine = facts.join('<i aria-hidden="true">·</i>');
    if (!factLine) factLine = state.loading ? '불러오는 중…' : (state.error || (ch ? '' : '채널을 만들지 않은 사용자'));

    // 팔로우 — 로그인했고, 내가 아니고, 채널이 있을 때만. 상태를 모르면(응답에 없음) 팔로우 버튼으로.
    let followBtn = '';
    if (ch && !isSelf) {
      if (!loggedIn) followBtn = '<button class="up-btn" data-up="login" title="로그인하면 팔로우할 수 있어요">팔로우</button>';
      else if (c.following) followBtn = `<button class="up-btn up-following" data-up="unfollow" title="눌러서 팔로우 끊기"${state.busy ? ' disabled' : ''}><span class="up-on">팔로잉</span><span class="up-off">언팔로우</span></button>`;
      else followBtn = `<button class="up-btn primary" data-up="follow"${state.busy ? ' disabled' : ''}>팔로우</button>`;
    }
    const canChat = !panel.input.disabled;
    const isHere = ch && panel.channelId === ch.channelId;
    pop.innerHTML = `
      <div class="up-head">
        ${avatar}
        <div class="up-who">
          <div class="up-name"><b>${esc(name)}</b>${mark}</div>
          <div class="up-sub-line">${title}<span class="up-badges">${cardBadges}</span></div>
        </div>
      </div>
      <div class="up-facts${state.loading && !facts.length ? ' loading' : ''}">${factLine}</div>
      <div class="up-main">
        ${ch ? `<button class="up-btn" data-up="channel">채널 보기</button>` : ''}
        ${ch && ch.openLive && !isHere ? '<button class="up-btn" data-up="watch">방송 보기</button>' : ''}
        ${followBtn}
      </div>
      <div class="up-list" role="menu">
        <button class="up-item${solo ? ' on' : ''}" role="menuitemcheckbox" aria-checked="${solo}" data-up="solo">${solo ? '모든 채팅 보기' : '이 사람 채팅만 보기'}</button>
        <button class="up-item" role="menuitem" data-up="mention"${canChat ? '' : ' disabled title="지금은 채팅을 보낼 수 없습니다"'}>@${esc(nick)} 멘션</button>
        <button class="up-item" role="menuitem" data-up="copy">닉네임 복사</button>
        <button class="up-item" role="menuitem" data-up="recent">이 화면의 최근 메시지</button>
        <button class="up-item" role="menuitemcheckbox" aria-checked="${highlighted}" data-up="highlight">${highlighted ? '하이라이트 해제' : '하이라이트'}</button>
        ${ch ? '<button class="up-item" role="menuitem" data-up="open">치지직에서 열기</button>' : ''}
        ${isSelf ? '' : '<button class="up-item danger" role="menuitem" data-up="block">채팅 숨기기(차단)</button>'}
      </div>`;
    place();
  };

  // 연 순간의 닉네임 자리 — 말풍선은 이 자리를 기준으로 놓는다.
  const a = anchor.getBoundingClientRect();
  const place = () => {
    const w = pop.offsetWidth;
    const h = pop.offsetHeight;
    const gap = 10;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const below = a.bottom + gap + h <= vh - 8 || a.top - gap - h < 8;
    const top = below ? Math.min(a.bottom + gap, vh - h - 8) : a.top - gap - h;
    const left = Math.max(8, Math.min(a.left - 14, vw - w - 8));
    const cx = a.left + Math.min(a.width, 60) / 2;
    pop.dataset.side = below ? 'bottom' : 'top';
    pop.style.setProperty('--tail', `${Math.round(Math.max(16, Math.min(w - 16, cx - left)))}px`);
    pop.style.left = `${Math.round(left)}px`;
    pop.style.top = `${Math.round(Math.max(8, top))}px`;
  };
  // 닉네임이 연 자리를 떠났나(흘러감·지워짐·목록 밖) — 떠나면 꼬리를 감춘다.
  const track = () => {
    const now = anchor.isConnected ? anchor.getBoundingClientRect() : null;
    const box = panel.list.getBoundingClientRect();
    const here = now && now.height > 0 && Math.abs(now.top - a.top) < 2 && Math.abs(now.left - a.left) < 2
      && now.bottom > box.top && now.top < box.bottom;
    pop.classList.toggle('detached', !here);
  };

  // 보이면 다음 프레임, 창이 숨어 rAF 가 멈춰도 타이머로(chat.js flushLines 와 같은 방식).
  let raf = 0;
  let timer = 0;
  const run = () => { cancelAnimationFrame(raf); clearTimeout(timer); raf = 0; timer = 0; track(); };
  const schedule = () => {
    if (raf || timer) return;
    raf = requestAnimationFrame(run);
    timer = setTimeout(run, 150);
  };
  const outside = (e) => { if (!pop.contains(e.target) && !anchor.contains(e.target)) close(); };
  const onKey = (e) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    close();
  };
  // 새 줄이 붙어 목록이 흐르면(scroll·줄 추가) 닉네임이 자리를 지키는지만 본다. 창 크기가 바뀌면 화면 안으로 다시 놓는다.
  const onResize = () => { place(); track(); };
  const mo = new MutationObserver(schedule);
  function close() {
    if (!current || current.pop !== pop) return;
    current = null;
    cancelAnimationFrame(raf);
    clearTimeout(timer);
    mo.disconnect();
    document.removeEventListener('mousedown', outside, true);
    document.removeEventListener('keydown', onKey, true);
    window.removeEventListener('resize', onResize);
    window.removeEventListener('blur', close);
    panel.list.removeEventListener('scroll', schedule);
    anchor.classList.remove('up-anchor');
    // 닫는 움직임은 hideWithExit 의 복제가 맡는다(모션 줄이기면 바로 사라짐).
    hideWithExit(pop);
    setTimeout(() => pop.remove(), 0);
  }
  current = { pop, uid, panel, close };
  anchor.classList.add('up-anchor');
  setTimeout(() => document.addEventListener('mousedown', outside, true), 0);
  document.addEventListener('keydown', onKey, true);
  window.addEventListener('resize', onResize);
  window.addEventListener('blur', close);
  panel.list.addEventListener('scroll', schedule, { passive: true });
  mo.observe(panel.list, { childList: true });

  const act = async (what) => {
    const c = state.card || {};
    const ch = c.channel;
    switch (what) {
      case 'channel': close(); window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: ch.channelId } })); break;
      case 'watch': close(); window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: ch.channelId } })); break;
      case 'open': close(); api.openExternal(`https://chzzk.naver.com/${ch.channelId}`).catch((e) => toast(e.message)); break;
      case 'login': close(); if (panel.onLogin) panel.onLogin(); else window.dispatchEvent(new Event('cview:login')); break;
      case 'follow':
      case 'unfollow': {
        const on = what === 'follow';
        state.busy = true;
        render();
        try {
          await api.channelFollow(ch.channelId, on);
          state.card = { ...c, following: on, channel: { ...ch, followerCount: Math.max(0, (ch.followerCount || 0) + (on ? 1 : -1)) } };
          cardCache.delete(`${panel.chatChannelId || ''}|${uid}`);
          toast(on ? `${ch.channelName} 채널을 팔로우했습니다` : `${ch.channelName} 채널 팔로우를 끊었습니다`, 1500);
        } catch (e) {
          toast(e.message || '팔로우를 바꾸지 못했습니다');
        }
        state.busy = false;
        if (current && current.pop === pop) render();
        break;
      }
      case 'solo': panel.setSolo(panel.soloUid === uid ? null : uid, nick); close(); break;
      case 'mention': close(); panel.mention(nick); break;
      case 'copy': copy(nick, '닉네임'); close(); break;
      case 'recent': close(); panel.showProfile(uid, nick); break;
      case 'highlight': panel.toggleHighlight(uid); render(); break;
      case 'block': close(); panel.block(uid, nick); break;
      default: break;
    }
  };
  pop.addEventListener('click', (e) => {
    const b = e.target.closest('[data-up]');
    if (b && !b.disabled) act(b.dataset.up);
  });

  render();
  pop.focus({ preventScroll: true });
  loadCard(uid, panel.chatChannelId, false)
    .then((card) => { state.card = card; })
    .catch((e) => { state.error = (e && e.message) || '정보를 불러오지 못했습니다'; })
    .finally(() => {
      state.loading = false;
      if (current && current.pop === pop) render();
    });
}
