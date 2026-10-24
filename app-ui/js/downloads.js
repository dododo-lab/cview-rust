// 다운로드 — 원본 VideoDownloadButton / VideoDownloadViews / DownloadsLibraryView.
// VOD-06 다시보기·클립 받기(진행률·취소·다시 시도), VOD-07 보관함 "다운로드" 탭(진행 중/완료, 끝난 항목 지우기, 폴더 열기).
import { api, listen, Events } from './api.js';
import { esc, toast, keepFocus } from './dom.js';
import { emptyState } from './cards.js';
import { relativeDate } from './channel.js';
import { bytesText } from './stats.js';

const STATE_LABEL = {
  queued: '대기 중', preparing: '주소 확인 중', downloading: '받는 중', assembling: '합치는 중',
  completed: '완료', failed: '실패', cancelled: '취소함',
};

let latest = null;
const subscribers = new Set();
listen(Events.downloadUpdate, (d) => {
  const before = new Map((latest?.jobs || []).map((j) => [j.id, j.state]));
  latest = d;
  for (const j of d.jobs) {
    const prev = before.get(j.id);
    if (prev && prev !== j.state && j.state === 'completed') {
      toast(`받기 완료 — ${j.title}`, 6000, { label: 'Finder에서 보기', run: () => api.revealPath(j.filePath).catch(() => {}) });
    } else if (prev && prev !== j.state && j.state === 'failed') {
      toast(`받기 실패 — ${j.title}: ${j.error || ''}`, 6000);
    }
  }
  for (const fn of subscribers) fn(d);
}).catch(() => {});

/** 다운로드 시작(버튼에서). */
export async function startDownload(kind, target, title, channelName) {
  try {
    latest = await api.downloadStart(kind, String(target), title || null, channelName || null);
    for (const fn of subscribers) fn(latest);
    toast('받기를 시작했습니다 — 보관함 › 다운로드에서 볼 수 있습니다', 3500, {
      label: '열기', run: () => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'library', params: { tab: 'downloads' } } })),
    });
  } catch (e) {
    toast(e.message);
  }
}

function jobRow(j) {
  const pct = Math.round((j.progress || 0) * 100);
  const active = ['queued', 'preparing', 'downloading', 'assembling'].includes(j.state);
  return `<div class="dl-row" data-dl-id="${esc(j.id)}">
    <span class="dl-kind">${j.kind === 'clip' ? '클립' : 'VOD'}</span>
    <span class="dl-text"><strong class="ellipsis">${esc(j.title)}</strong>
      <span class="ellipsis">${j.channelName ? `${esc(j.channelName)} · ` : ''}${esc(STATE_LABEL[j.state] || j.state)}${active && j.state === 'downloading' ? ` · ${pct}% · ${esc(bytesText(j.bytes))}` : ''}${j.error ? ` · ${esc(j.error)}` : ''}</span>
      ${active ? `<span class="dl-track"><span class="dl-fill${j.state === 'preparing' || j.state === 'queued' ? ' indeterminate' : ''}" style="--p:${pct / 100}"></span></span>` : ''}</span>
    ${active ? '<button class="btn ghost" data-dl="cancel">취소</button>' : ''}
    ${j.state === 'failed' || j.state === 'cancelled' ? `<button class="btn" data-dl="retry" data-kind="${esc(j.kind)}" data-target="${esc(j.target)}" data-title="${esc(j.title)}">다시 시도</button>` : ''}
    ${j.state === 'completed' && j.filePath ? `<button class="btn" data-dl="reveal" data-path="${esc(j.filePath)}">Finder에서 보기</button>` : ''}
  </div>`;
}

function historyRow(h) {
  const missing = h.fileStatus === 'missing';
  const [kind, ...rest] = h.id.split('-');
  return `<div class="dl-row${missing ? ' missing' : ''}" data-dl-id="${esc(h.id)}">
    <span class="dl-kind">${kind === 'clip' ? '클립' : 'VOD'}</span>
    <span class="dl-text"><strong class="ellipsis">${esc(h.title)}</strong>
      <span class="ellipsis">${h.channelName ? `${esc(h.channelName)} · ` : ''}${esc(relativeDate(h.finishedAt))}${h.completed ? (h.fileSize ? ` · ${esc(bytesText(h.fileSize))}` : '') : ` · 실패${h.failureReason ? `: ${esc(h.failureReason)}` : ''}`}${missing ? ' · 파일이 없습니다' : ''}</span></span>
    ${h.completed && !missing && h.filePath ? `<button class="btn" data-dl="reveal" data-path="${esc(h.filePath)}">Finder에서 보기</button><button class="btn ghost" data-dl="open" data-path="${esc(h.filePath)}">열기</button>` : ''}
    ${!h.completed || missing ? `<button class="btn" data-dl="retry" data-kind="${esc(kind)}" data-target="${esc(rest.join('-'))}" data-title="${esc(h.title)}">다시 받기</button>` : ''}
    <button class="icon-btn" data-dl="remove" title="목록에서 지우기(파일은 남김)">✕</button>
  </div>`;
}

/** 보관함 "다운로드" 탭 — 컨테이너를 채우고 갱신을 구독한다. 구독 해제 함수를 돌려준다. */
export async function mountDownloads(container) {
  const render = (d) => {
    // 완료·실패는 기록에도 남으므로 작업 목록에서는 진행 중·취소만 보인다.
    const active = d.jobs.filter((j) => j.state !== 'completed' && j.state !== 'failed');
    const histShown = d.history;
    // 진행 이벤트마다 다시 그린다 — 취소·Finder에서 보기에 둔 키보드 초점을 지킨다
    keepFocus(container, () => { container.innerHTML = `
      <div class="dl-head"><span class="muted">저장 폴더 · ${esc(d.folder)}</span><div class="grow"></div>
        <button class="btn ghost" data-dl="folder" data-path="${esc(d.folder)}">폴더 열기</button>
        ${histShown.length || active.some((j) => j.state === 'cancelled') ? '<button class="btn ghost" data-dl="clear">끝난 항목 지우기</button>' : ''}</div>
      ${active.length ? `<h3 class="section-title">진행 중</h3><div class="dl-list">${active.map(jobRow).join('')}</div>` : ''}
      <h3 class="section-title">받은 파일</h3>
      ${histShown.length ? `<div class="dl-list">${histShown.map(historyRow).join('')}</div>` : emptyState('받은 파일이 없습니다', '다시보기·클립 화면의 "받기" 버튼으로 저장할 수 있어요')}`; });
  };
  const onClick = async (e) => {
    const b = e.target.closest('[data-dl]');
    if (!b) return;
    const id = b.closest('[data-dl-id]')?.dataset.dlId;
    try {
      switch (b.dataset.dl) {
        case 'cancel': render(await api.downloadCancel(id)); break;
        case 'retry': await startDownload(b.dataset.kind, b.dataset.target, b.dataset.title); break;
        case 'reveal': await api.revealPath(b.dataset.path); break;
        case 'open': await api.openPath(b.dataset.path); break;
        case 'folder': await api.revealPath(b.dataset.path); break;
        case 'remove': render(await api.downloadClear([id])); break;
        case 'clear': render(await api.downloadClear(null)); break;
        default: break;
      }
    } catch (er) { toast(er.message); }
  };
  // 목록을 받은 뒤에 구독한다 — 받기가 실패하면 구독만 남아 해제 함수 없이 새던 문제
  render(latest = await api.downloadList());
  container.addEventListener('click', onClick);
  subscribers.add(render);
  return () => { subscribers.delete(render); container.removeEventListener('click', onClick); };
}
