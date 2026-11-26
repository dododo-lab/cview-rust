// 스크린샷 형식 — 설정 › 화면 · 비디오 · 스크린샷의 '스크린샷 형식'(PNG/JPEG)을 찍을 때마다 읽는다.
// 예전엔 시청 화면·단독 보기 모두 'png' 로 고정해, JPEG 를 골라도 늘 PNG 로 저장됐다.
import { api } from './api.js';

/** 지금 설정의 형식 — 'jpeg' 아니면 'png'(읽지 못하면 기본 PNG). */
export async function screenshotFormat() {
  try {
    return (await api.settingsGet()).player?.screenshotFormat === 'jpeg' ? 'jpeg' : 'png';
  } catch {
    return 'png';
  }
}

/** 플레이어가 돌려준 결과의 형식(`format: 'image/jpeg'`)에 맞는 확장자. */
export function screenshotExtension(data) {
  return data && data.format === 'image/jpeg' ? 'jpg' : 'png';
}
