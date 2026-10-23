// 채팅 부가 기능 — 원본 ChatModerationService(명령)·ChatTTSService·ChatPanelEventRail·ChatStatsView·
// ChatDesignStyle·ChatBackgroundScope. 채팅 패널마다 설치한다.
// CHT-07 명령어, CHT-13 디자인 스타일 8종, CHT-15 배경, CHT-17 후원·구독 TTS, CHT-18 이벤트 레일,
// CHT-19 채팅 내보내기, CHT-24 채팅 통계.
import { api } from './api.js';
import { esc, toast } from './dom.js';
import { chatPrefs, onChatPrefs, updateChatPrefs } from './chat-prefs.js';
import { formatCheese, donationKindLabel, eventMeta, EVENT_ICONS } from './donations.js';

/** 원본 ChatDesignStyle — 앞 다섯은 고를 수 있는 스타일, 뒤 셋은 이전 스타일(저장값 호환). */
export const DESIGN_STYLES = [
  ['unified', 'Chat Lounge', '대화 카드와 아바타로 방송 옆 대화 공간을 구성합니다'],
  ['chzzk', '치지직 스타일', '배지·닉네임·본문을 간결한 줄 형태로 표시합니다'],
  ['hybrid', 'Lounge × CHZZK', '대화 카드와 간결한 채널 배지를 결합한 통합 디자인'],
  ['kakao', '카카오톡형 말풍선', '프로필 옆 말풍선, 내 말은 노란 말풍선'],
  ['imessage', 'iMessage형 말풍선', '회색·파란 꼬리 말풍선으로 메신저처럼'],
  ['calm', '차분하게 (이전)', '여백과 얇은 구분선으로 편하게 읽기'],
  ['compact', '촘촘하게 (이전)', '한 화면에 더 많은 메시지'],
  ['dashboard', '대시보드 (이전)', '시간·닉네임·본문을 열로 정렬'],
];
/** 읽기 폭 — 원본 ChatReadingWidthMenu. */
export const READING_WIDTHS = [['full', '전체', null], ['wide', '넓게', 720], ['normal', '보통', 560], ['narrow', '좁게', 420]];

/** 채팅 화면(패널·통합 목록)에 디자인 스타일·읽기 폭·표시 설정(글자 크기·줄 간격·시간·배지·역할 색)을 입힌다.
 *  글자 크기는 --chat-font 와 배율 --chat-scale(기본 14px = 1)로 — 디자인마다 닉네임·아바타·이모티콘 크기가 따라 커진다. */
export function applyChatSkin(el, p = chatPrefs()) {
  const style = DESIGN_STYLES.some(([v]) => v === p.designStyle) ? p.designStyle : 'unified';
  const font = Math.min(24, Math.max(10, Number(p.fontSize) || 14));
  el.classList.add('chat-skin');
  el.style.setProperty('--chat-font', `${font}px`);
  el.style.setProperty('--chat-scale', String(font / 14));
  el.style.setProperty('--chat-gap', `${Math.min(12, Math.max(0, Number(p.lineSpacing) || 0))}px`);
  el.classList.toggle('hide-time', !p.showTimestamp);
  el.classList.toggle('hide-badge', !p.showBadge);
  el.classList.toggle('no-role-color', !p.highlightRoles);
  el.classList.toggle('no-nick-colors', p.nicknameColors === false);
  for (const [v] of DESIGN_STYLES) el.classList.toggle(`style-${v}`, v === style);
  const w = READING_WIDTHS.find(([v]) => v === p.readingWidth);
  if (w && w[2]) { el.dataset.readWidth = w[0]; el.style.setProperty('--chat-read-width', `${w[2]}px`); }
  else { delete el.dataset.readWidth; el.style.removeProperty('--chat-read-width'); }
}

/** 채팅 머리글 "디자인" 메뉴 — 스타일·읽기 폭 고르기. */
export function showDesignMenu(anchor) {
  document.querySelector('.design-menu')?.remove();
  const p = chatPrefs();
  const cur = DESIGN_STYLES.some(([v]) => v === p.designStyle) ? p.designStyle : 'unified';
  const menu = document.createElement('div');
  menu.className = 'design-menu';
  menu.setAttribute('role', 'menu');
  menu.innerHTML = `<div class="design-menu-head">채팅 디자인</div>
    ${DESIGN_STYLES.map(([v, l, d], i) => `${i === 5 ? '<div class="design-menu-sep"></div><div class="design-menu-head">이전 스타일</div>' : ''}<button class="design-item${v === cur ? ' on' : ''}" data-style="${v}" role="menuitemradio" aria-checked="${v === cur}"><span class="design-swatch sw-${v}"></span><span class="design-text"><b>${esc(l)}</b><small>${esc(d)}</small></span><span class="design-check" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg></span></button>`).join('')}
    <div class="design-menu-sep"></div><div class="design-menu-head">읽기 폭</div>
    <div class="segmented design-width">${READING_WIDTHS.map(([v, l]) => `<button class="seg${(p.readingWidth || 'full') === v ? ' active' : ''}" data-width="${v}">${l}</button>`).join('')}</div>`;
  document.body.append(menu);
  const r = anchor.getBoundingClientRect();
  menu.style.top = `${Math.min(window.innerHeight - menu.offsetHeight - 8, r.bottom + 6)}px`;
  menu.style.left = `${Math.max(8, Math.min(window.innerWidth - menu.offsetWidth - 8, r.right - menu.offsetWidth))}px`;
  menu.addEventListener('click', (e) => {
    const st = e.target.closest('[data-style]');
    if (st) { updateChatPrefs({ designStyle: st.dataset.style }); menu.remove(); return; }
    const wd = e.target.closest('[data-width]');
    if (wd) { updateChatPrefs({ readingWidth: wd.dataset.width }); menu.remove(); }
  });
  setTimeout(() => document.addEventListener('pointerdown', function off(ev) {
    if (!menu.contains(ev.target)) { menu.remove(); document.removeEventListener('pointerdown', off); }
  }), 0);
}
export const BACKGROUNDS = [['neutral', '기본'], ['tint', '브랜드 색'], ['dim', '어둡게'], ['clear', '투명']];

const COMMANDS = [
  ['/clear', '채팅 지우기'], ['/export', '채팅 내보내기(텍스트 파일)'], ['/filter 단어', '금지어 추가'],
  ['/block 닉네임', '사용자 차단(로컬)'], ['/unblock 닉네임', '차단 해제'], ['/mute 닉네임 [초]', '잠시 숨기기(로컬, 기본 300초)'],
  ['/tts', '후원 읽어 주기 켜기·끄기'], ['/stats', '채팅 통계'], ['/help', '명령어 목록'],
];
const SERVER_ONLY = new Set(['/ban', '/unban', '/slow', '/notice', '/host', '/unmute']);

/** 슬래시 명령 해석 — [명령, 인자들] 또는 null. 순수 함수. */
export function parseCommand(text) {
  const t = String(text || '').trim();
  if (!t.startsWith('/') || t.length < 2) return null;
  const [cmd, ...args] = t.split(/\s+/);
  return [cmd.toLowerCase(), args];
}

/** 후원·구독 읽을 문장. */
export function ttsText(m) {
  if (m.kind === 'subscription') return `${m.nickname || '익명'}님이 ${m.subscriptionMonths ? `${m.subscriptionMonths}개월 ` : ''}구독했습니다. ${m.content || ''}`;
  return `${m.nickname || '익명'}님이 ${formatCheese(m.donationAmount)} ${donationKindLabel(m.donationType)}했습니다. ${m.content || ''}`;
}

function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}`;
}

export function installChatExtras(panel) {
  const root = panel.root;
  const stats = { times: [], users: new Map(), donations: 0, cheese: 0, subs: 0, since: Date.now() };
  const events = [];
  const muted = new Map(); // nick → 해제 시각

  // ── 이벤트 레일 (CHT-18) ──
  const rail = document.createElement('div');
  rail.className = 'chat-events';
  rail.hidden = true;
  const anchor = root.querySelector('.donation-rank') || root.querySelector('.chat-header');
  anchor.after(rail);
  const renderRail = () => {
    rail.hidden = events.length === 0;
    rail.innerHTML = events.slice(0, 12).map((m) => {
      const e = eventMeta(m);
      if (!e) return '';
      return `<span class="event-chip ev-${e.tone} ev-tier-${e.tier} ev-lv-${e.level}" title="${esc(`${e.title} · ${m.content || ''}`)}"><span class="event-chip-icon">${EVENT_ICONS[e.icon] || ''}</span><b>${esc(m.nickname || '익명')}</b><span class="event-chip-value">${esc(e.value)}</span></span>`;
    }).join('');
  };

  // ── 통계 버튼 (CHT-24) ──
  const statsBtn = document.createElement('button');
  statsBtn.className = 'icon-btn chat-hbtn';
  statsBtn.title = '채팅 통계';
  statsBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 20V10M12 20V4M19 20v-7"/></svg>';
  panel.header.querySelector('[data-chat="clear"]').before(statsBtn);
  const exportBtn = document.createElement('button');
  exportBtn.className = 'icon-btn chat-hbtn';
  exportBtn.title = '채팅 내보내기';
  exportBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>';
  statsBtn.after(exportBtn);
  const designBtn = document.createElement('button');
  designBtn.className = 'icon-btn chat-hbtn';
  designBtn.title = '채팅 디자인·읽기 폭';
  designBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a9 9 0 1 0 0 18c1 0 1.5-.8 1.5-1.6 0-1.1-.9-1.4-.9-2.4 0-.9.7-1.5 1.6-1.5H17a4 4 0 0 0 4-4c0-4.7-4-8.5-9-8.5Z"/><circle cx="7.5" cy="11" r="1" fill="currentColor"/><circle cx="10.5" cy="7" r="1" fill="currentColor"/><circle cx="15" cy="7.5" r="1" fill="currentColor"/></svg>';
  statsBtn.before(designBtn);
  designBtn.addEventListener('click', (e) => { e.stopPropagation(); showDesignMenu(designBtn); });
  statsBtn.addEventListener('click', () => showStats());
  exportBtn.addEventListener('click', () => exportChat());

  const showStats = () => {
    const now = Date.now();
    const minute = stats.times.filter((t) => now - t < 60000).length;
    const top = [...stats.users.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8);
    const mins = Math.max(1, (now - stats.since) / 60000);
    panel.tray.innerHTML = `
      <div class="tray-head"><strong>채팅 통계</strong><div class="grow"></div><span class="muted">${Math.round(mins)}분 동안</span></div>
      <div class="stat-grid">
        <div><b>${minute}</b><span>최근 1분 메시지</span></div>
        <div><b>${Math.round(stats.times.length / mins)}</b><span>분당 평균</span></div>
        <div><b>${stats.users.size}</b><span>참여자</span></div>
        <div><b>${stats.donations}</b><span>후원 · ${esc(formatCheese(stats.cheese) || '0치즈')}</span></div>
        <div><b>${stats.subs}</b><span>구독</span></div>
      </div>
      <div class="tray-head"><strong>가장 많이 말한 사람</strong></div>
      ${top.map(([n, c], i) => `<div class="tray-item"><b>${i + 1}. ${esc(n)}</b> <span class="muted">${c}개</span></div>`).join('') || '<div class="tray-empty">아직 없습니다</div>'}`;
    panel.tray.hidden = false;
  };

  const exportChat = async () => {
    const lines = [...panel.list.querySelectorAll('.chat-line')].map((l) => {
      const time = l.querySelector('.chat-time')?.textContent || '';
      const nick = l.dataset.nick || '';
      const amount = l.querySelector('.amount')?.textContent || '';
      const text = l.querySelector('.text')?.textContent || l.textContent;
      return `${time ? `[${time}] ` : ''}${nick ? `${nick}: ` : ''}${amount ? `(${amount}) ` : ''}${text}`.trim();
    });
    if (!lines.length) { toast('내보낼 채팅이 없습니다'); return; }
    try {
      const path = await api.saveFile('chat', `채팅_${panel.channelId || 'cview'}_${stamp()}.txt`, null, lines.join('\n'));
      toast(`채팅 ${lines.length}줄을 저장했습니다`, 5000, { label: 'Finder에서 보기', run: () => api.revealPath(path) });
    } catch (e) { toast(e.message); }
  };

  // ── TTS (CHT-17) ──
  const speak = (m) => {
    const p = chatPrefs();
    if (!p.ttsEnabled || !window.speechSynthesis) return;
    const u = new SpeechSynthesisUtterance(ttsText(m).slice(0, 200));
    u.lang = 'ko-KR';
    u.volume = Math.min(1, Math.max(0, Number(p.ttsVolume ?? 0.8)));
    u.rate = Math.min(2, Math.max(0.5, Number(p.ttsRate ?? 200) / 200));
    const voice = speechSynthesis.getVoices().find((v) => v.voiceURI === p.ttsVoiceIdentifier);
    if (voice) u.voice = voice;
    speechSynthesis.speak(u);
  };

  panel.appendHooks.push((messages) => {
    const now = Date.now();
    for (const m of messages) {
      if (m.kind === 'system') continue;
      stats.times.push(now);
      if (m.nickname) stats.users.set(m.nickname, (stats.users.get(m.nickname) || 0) + 1);
      if (m.kind === 'donation') { stats.donations += 1; stats.cheese += m.donationAmount || 0; }
      if (m.kind === 'subscription') stats.subs += 1;
      if (m.kind === 'donation' || m.kind === 'subscription') {
        events.unshift(m);
        if (panel.active && root.offsetParent) speak(m);
      }
    }
    if (stats.times.length > 5000) stats.times.splice(0, stats.times.length - 5000);
    if (events.length > 30) events.length = 30;
    renderRail();
    // 로컬 음소거(/mute) — 시간이 지나면 다시 보인다.
    if (muted.size) {
      for (const [nick, until] of muted) {
        if (now > until) { muted.delete(nick); continue; }
        for (const el of panel.list.querySelectorAll(`.chat-line[data-nick="${CSS.escape(nick)}"]`)) el.remove();
      }
    }
  });

  // 연결이 바뀌면 통계·레일 초기화
  const origConnect = panel.connect.bind(panel);
  panel.connect = async (...args) => {
    stats.times = []; stats.users.clear(); stats.donations = 0; stats.cheese = 0; stats.subs = 0; stats.since = Date.now();
    events.length = 0;
    renderRail();
    return origConnect(...args);
  };

  // ── 명령어 (CHT-07) ──
  panel.sendHooks.push((text) => {
    const parsed = parseCommand(text);
    if (!parsed) return false;
    const [cmd, args] = parsed;
    const p = chatPrefs();
    const nickToUid = (nick) => panel.list.querySelector(`.chat-line[data-nick="${CSS.escape(nick)}"]`)?.dataset.uid;
    switch (cmd) {
      case '/clear': panel.clear(true); return true;
      case '/export': exportChat(); return true;
      case '/stats': showStats(); return true;
      case '/tts': updateChatPrefs({ ttsEnabled: !p.ttsEnabled }); toast(!p.ttsEnabled ? '후원 읽어 주기를 켰습니다' : '후원 읽어 주기를 껐습니다', 1500); return true;
      case '/filter':
        if (!args.length) { toast('사용법: /filter 단어'); return true; }
        updateChatPrefs({ blockedWords: [...new Set([...(p.blockedWords || []), args.join(' ')])], chatFilterEnabled: true });
        toast(`금지어 "${args.join(' ')}" 를 추가했습니다`, 1500);
        return true;
      case '/block': {
        const nick = (args[0] || '').replace(/^@/, '');
        const uid = nick && nickToUid(nick);
        if (!uid) { toast('이 채팅에서 그 닉네임을 찾지 못했습니다'); return true; }
        panel.block(uid, nick);
        return true;
      }
      case '/unblock': {
        const nick = (args[0] || '').replace(/^@/, '');
        const names = p.blockedUserNames || {};
        const uid = Object.keys(names).find((k) => names[k] === nick) || nick;
        const next = { ...names };
        delete next[uid];
        updateChatPrefs({ blockedUsers: (p.blockedUsers || []).filter((x) => x !== uid), blockedUserNames: next });
        toast(`${nick} 님 차단을 풀었습니다`, 1500);
        return true;
      }
      case '/mute': {
        const nick = (args[0] || '').replace(/^@/, '');
        if (!nick) { toast('사용법: /mute 닉네임 [초]'); return true; }
        const sec = Math.min(86400, Math.max(10, Number(args[1]) || 300));
        muted.set(nick, Date.now() + sec * 1000);
        for (const el of panel.list.querySelectorAll(`.chat-line[data-nick="${CSS.escape(nick)}"]`)) el.remove();
        toast(`${nick} 님 메시지를 ${sec}초 동안 숨깁니다`, 2000);
        return true;
      }
      case '/help':
        panel.system(`명령어: ${COMMANDS.map(([c, d]) => `${c} — ${d}`).join(' · ')}`);
        return true;
      default:
        if (SERVER_ONLY.has(cmd)) { toast('방송 관리 명령은 치지직 웹에서 해 주세요 (앱은 아직 보내지 않습니다)'); return true; }
        return false;
    }
  });

  // ── 디자인 스타일 (CHT-13) · 배경 (CHT-15) ──
  const applyLook = () => {
    const p = chatPrefs();
    applyChatSkin(root, p);
    const bg = p.background && typeof p.background === 'object' ? p.background : {};
    const mode = BACKGROUNDS.some(([v]) => v === bg.mode) ? bg.mode : 'neutral';
    for (const [v] of BACKGROUNDS) root.classList.toggle(`bg-${v}`, v === mode);
    root.style.setProperty('--chat-bg-strength', String(Math.min(1, Math.max(0, Number(bg.strength ?? 0.5)))));
  };
  onChatPrefs(applyLook);
  applyLook();
}
