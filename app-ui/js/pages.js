// 사이드바 화면들 — 팔로잉 / 인기 / 검색 / 설정. 홈은 home.js.
import { api } from './api.js';
import { esc, toast } from './dom.js';
import { liveGrid, channelRow, emptyState, loadingState } from './cards.js';
import { THEMES, THEME_LABELS, currentTheme, setTheme } from './theme.js';
import { streamAlertsEnabled, setStreamAlertsEnabled } from './donations.js';

const loginAction = '<button class="btn primary" data-action="login">로그인</button>';

/** 공통: 목록 화면. `load()` 결과를 그린다. 늦게 끝난 이전 요청은 버린다. */
class ListPage {
  constructor(root) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.seq = 0;
    const refresh = root.querySelector('[data-action="refresh"]');
    if (refresh) refresh.addEventListener('click', () => this.show(true));
  }

  async show(refresh = false) {
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    try {
      const html = await this.render(refresh);
      if (seq === this.seq) this.body.innerHTML = html;
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = emptyState('불러오지 못했습니다', e.message);
    }
  }
}

export class PopularPage extends ListPage {
  async render(refresh) {
    const lives = await api.popularLives(50, refresh);
    return lives.length ? liveGrid(lives) : emptyState('라이브 채널이 없습니다');
  }
}

export class FollowingPage extends ListPage {
  constructor(root, { isLoggedIn }) {
    super(root);
    this.isLoggedIn = isLoggedIn;
  }

  async render(refresh) {
    if (!this.isLoggedIn()) {
      return emptyState('네이버 로그인이 필요합니다', '로그인하면 팔로우한 채널의 라이브를 볼 수 있습니다', loginAction);
    }
    const lives = await api.followingLives(refresh);
    return lives.length
      ? liveGrid(lives)
      : emptyState('라이브 채널이 없습니다', '팔로우한 채널 중 방송 중인 채널이 없습니다');
  }
}

export class SearchPage {
  constructor(root) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.input = root.querySelector('input[type=search]');
    this.seq = 0;
    root.querySelector('form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.run(this.input.value);
    });
    this.body.innerHTML = emptyState('검색어를 입력하세요', '채널 이름이나 방송 제목으로 찾을 수 있습니다');
  }

  show() {
    this.input.focus();
  }

  async run(keyword) {
    const q = keyword.trim();
    if (!q) return;
    const seq = ++this.seq;
    this.body.innerHTML = loadingState();
    try {
      const res = await api.search(q);
      if (seq !== this.seq) return;
      let html = '';
      if (res.lives.length) {
        html += `<h2 class="section-title">라이브</h2>${liveGrid(res.lives)}`;
      }
      if (res.channels.length) {
        html += `<h2 class="section-title">채널</h2><div class="channel-list">${res.channels.map(channelRow).join('')}</div>`;
      }
      this.body.innerHTML = html || emptyState('검색 결과가 없습니다', `"${esc(q)}"`);
    } catch (e) {
      if (seq === this.seq) this.body.innerHTML = emptyState('검색하지 못했습니다', e.message);
    }
  }
}

// ── 설정 ────────────────────────────────────────────────

const PLAYER_TOGGLES = [
  ['lowLatencyMode', '저지연 모드', '라이브 엣지에 가깝게 재생합니다'],
  ['forceHighestQuality', '항상 최고 화질', '1080p60 을 우선 유지합니다'],
  ['startMuted', '음소거로 시작', '방송을 열 때 소리를 끈 상태로 시작합니다'],
];
const GENERAL_TOGGLES = [
  ['autoRefreshEnabled', '목록 자동 새로고침', ''],
  ['notificationsEnabled', '알림', '팔로우 채널 방송 시작 알림'],
];

function toggleRow(domain, [key, label, detail], values) {
  const checked = values && values[key] ? 'checked' : '';
  return `
    <label class="setting">
      <span class="setting-label">${esc(label)}${detail ? `<small>${esc(detail)}</small>` : ''}</span>
      <span class="toggle"><input type="checkbox" data-domain="${domain}" data-key="${key}" ${checked}><span></span></span>
    </label>`;
}

export class SettingsPage {
  constructor(root, { getAuth, onLogin, onLogout }) {
    this.root = root;
    this.body = root.querySelector('.page-body');
    this.getAuth = getAuth;
    this.onLogin = onLogin;
    this.onLogout = onLogout;
    this.settings = null;
    this.body.addEventListener('change', (e) => this.onChange(e.target));
    this.body.addEventListener('click', (e) => {
      const action = e.target.closest('[data-action]');
      if (!action) return;
      if (action.dataset.action === 'login') this.onLogin();
      if (action.dataset.action === 'logout') this.onLogout();
    });
    this.body.addEventListener('click', async (e) => {
      const choice = e.target.closest('[data-theme-choice]');
      if (!choice) return;
      await setTheme(choice.dataset.themeChoice);
      this.render();
    });
  }

  async show() {
    try {
      this.settings = await api.settingsGet();
    } catch (e) {
      this.body.innerHTML = emptyState('설정을 불러오지 못했습니다', e.message);
      return;
    }
    this.render();
  }

  render() {
    if (!this.settings) return;
    const auth = this.getAuth();
    const player = this.settings.player || {};
    const general = this.settings.general || {};
    const account = auth && auth.loggedIn
      ? `<div class="setting"><span class="setting-label">${esc(auth.nickname || '로그인됨')}<small>네이버 계정 (${esc(auth.method || 'cookie')})</small></span><button class="btn" data-action="logout">로그아웃</button></div>`
      : '<div class="setting"><span class="setting-label">로그인되지 않음<small>팔로잉 목록과 채팅 참여에 네이버 로그인이 필요합니다</small></span><button class="btn primary" data-action="login">로그인</button></div>';
    this.body.innerHTML = `
      <section class="settings-group"><h3>계정</h3>${account}</section>
      <section class="settings-group"><h3>화면</h3>
        <div class="setting">
          <span class="setting-label">테마<small>시스템은 macOS 화면 모드를 따릅니다</small></span>
          <div class="segmented" role="radiogroup" aria-label="테마">
            ${THEMES.map((t) => `<button class="seg ${t === currentTheme() ? 'active' : ''}" data-theme-choice="${t}" role="radio" aria-checked="${t === currentTheme()}">${THEME_LABELS[t]}</button>`).join('')}
          </div>
        </div>
        <label class="setting">
          <span class="setting-label">후원·구독 알림<small>시청 화면 위에 새 후원과 구독을 잠깐 띄웁니다</small></span>
          <span class="toggle"><input type="checkbox" data-local="streamAlerts" ${streamAlertsEnabled() ? 'checked' : ''}><span></span></span>
        </label>
      </section>
      <section class="settings-group"><h3>플레이어</h3>
        ${PLAYER_TOGGLES.map((t) => toggleRow('player', t, player)).join('')}
        <label class="setting">
          <span class="setting-label">스트림 프록시<small>로컬 프록시는 CDN 응답을 보정하고 암호화 방송 키를 인증합니다</small></span>
          <select class="select" data-domain="player" data-key="streamProxyMode">
            <option value="localProxy" ${player.streamProxyMode !== 'none' ? 'selected' : ''}>로컬 프록시 (권장)</option>
            <option value="none" ${player.streamProxyMode === 'none' ? 'selected' : ''}>직접 재생</option>
          </select>
        </label>
      </section>
      <section class="settings-group"><h3>일반</h3>
        ${GENERAL_TOGGLES.map((t) => toggleRow('general', t, general)).join('')}
      </section>`;
  }

  async onChange(target) {
    if (target.dataset.local === 'streamAlerts') {
      setStreamAlertsEnabled(target.checked);
      return;
    }
    const { domain, key } = target.dataset;
    if (!domain || !key) return;
    const value = target.type === 'checkbox' ? target.checked : target.value;
    try {
      this.settings = await api.settingsUpdate(domain, { [key]: value });
      toast('설정을 저장했습니다', 1400);
    } catch (e) {
      toast(e.message);
      this.render();
    }
  }
}
