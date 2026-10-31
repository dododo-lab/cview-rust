// 네이버 라이브 스트리밍 커넥터 — Windows 에서 치지직 1080p(P2P 전용 트랙)를 보려면 설치가 필요하다.
// 설치돼 있지 않으면 재생은 커넥터 없이 볼 수 있는 최고 화질(대개 720p)로 묶이고, 여기서 한 번 알린다.
import { api } from './api.js';
import { toast } from './dom.js';

const NOTICE = '1080p 원본 화질은 "네이버 라이브 스트리밍 커넥터"를 설치해야 볼 수 있어요. 지금은 720p까지 재생합니다.';
let noticed = false;

/** 지금 상태 — { required, installed, downloadUrl }. 앱 밖이면 null. */
export async function connectorStatus() {
  try { return await api.liveConnectorStatus(); } catch { return null; }
}

/** 커넥터 설치 페이지를 연다. */
export function openConnectorSetup(url) {
  api.openExternal(url).catch((e) => toast(e.message));
}

/**
 * 라이브를 열 때 부른다 — Windows 에서 커넥터가 없으면 이 실행 동안 한 번 설치 안내를 띄운다.
 * @param {number} [limitedHeight] 재생 시작 응답이 알려 준 상한(0 이면 이 방송은 제한이 없다)
 */
export async function noteConnectorLimit(limitedHeight) {
  if (noticed || limitedHeight === 0) return;
  const s = await connectorStatus();
  if (!s || !s.required || s.installed || noticed) return;
  noticed = true;
  const text = limitedHeight > 0 ? NOTICE.replace('720p', `${limitedHeight}p`) : NOTICE;
  toast(text, 9000, { label: '설치', run: () => openConnectorSetup(s.downloadUrl) });
}
