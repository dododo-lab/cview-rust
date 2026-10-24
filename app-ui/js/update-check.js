// 업데이트 확인 — 앱 메뉴 "업데이트 확인…" 시트와 설정 › 앱 줄이 같이 쓴다.
// 피드(자체 HTTPS 서버, CVIEW_UPDATE_URL 로 바꿀 수 있음)는 백엔드 `update_check` 가 고른다.
import { api } from './api.js';
import { esc } from './dom.js';

let last = null; // 마지막 확인 결과 — 설정 화면을 다시 그려도 남긴다
let appInfo = null;

/** 지금 앱 버전("0.1.20 (빌드 20)"). 앱 밖이면 null. */
export async function currentVersionText() {
  if (!appInfo) {
    try { appInfo = await api.appInfo(); } catch { return null; }
  }
  return appInfo.build && appInfo.build !== '0' ? `${appInfo.version} (빌드 ${appInfo.build})` : appInfo.version;
}

export function lastUpdateResult() { return last; }

/** 피드를 확인한다. 실패하면 던진다. */
export async function checkForUpdate() {
  last = await api.updateCheck();
  return last;
}

/** 결과 한 줄 — 설정 줄의 작은 글씨·시트 제목 아래에 쓴다. */
export function updateSummary(u) {
  if (!u.latest) return `현재 ${u.current} · 공개된 릴리스가 없습니다`;
  if (u.newer) return `새 버전 ${u.latest} 이 있습니다 (현재 ${u.current})`;
  return `최신 버전입니다 (${u.current})`;
}

/** 다운로드 단추를 열 주소 — 이 플랫폼 설치 파일, 없으면 릴리스 페이지. */
export function downloadTarget(u) {
  return u.downloadUrl || u.url || null;
}

export function openDownload(u) {
  const target = downloadTarget(u);
  return target ? api.openExternal(target) : Promise.resolve();
}

function sizeText(bytes) {
  if (!(bytes > 0)) return '';
  const mb = bytes / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)}MB` : `${Math.max(1, Math.round(bytes / 1024))}KB`;
}

function dateText(iso) {
  const d = iso ? new Date(iso) : null;
  return d && !Number.isNaN(d.getTime()) ? d.toLocaleDateString('ko-KR', { year: 'numeric', month: 'long', day: 'numeric' }) : '';
}

/** 시트 본문 — 버전 표와 릴리스 설명. */
export function updateDetailHtml(u, currentText = null) {
  const rows = [['현재 버전', currentText || u.current]];
  if (u.latest) rows.push(['최신 버전', u.latest]);
  const date = dateText(u.publishedAt);
  if (date) rows.push(['게시일', date]);
  if (u.newer && u.downloadName) rows.push(['설치 파일', `${u.downloadName}${u.downloadSize ? ` · ${sizeText(u.downloadSize)}` : ''}`]);
  const notes = u.newer && u.notes && u.notes.trim() ? `<pre class="update-notes">${esc(u.notes.trim())}</pre>` : '';
  return `<p class="update-summary${u.newer ? ' is-new' : ''}">${esc(updateSummary(u))}</p>
    <dl class="update-facts">${rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl>${notes}`;
}
