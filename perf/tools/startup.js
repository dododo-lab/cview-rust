(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
for (let i=0;i<100 && !performance.getEntriesByName('cview:interactive').length;i++) await sleep(100);
const m=performance.getEntriesByName('cview:interactive')[0]; const nav=performance.getEntriesByType('navigation')[0];
const launch=__LAUNCH__; const interactiveAbs=performance.timeOrigin+(m?m.startTime:NaN);
post(`S startup interactive_ms=${Math.round(interactiveAbs-launch)} page_load_to_interactive=${Math.round(m?m.startTime:NaN)} dcl=${nav?Math.round(nav.domContentLoadedEventEnd):'-'}`);
// 첫 라이브 프레임 — 재생 요청부터 영상 'playing' 까지
const I=window.__TAURI__.core.invoke; const lives=(await I('following_lives',{refresh:false})); await I('multilive_stop_all');
const f=document.getElementById('player-frame'); const t0=performance.now(); let tf=null;
const v=f.contentDocument.getElementById('video'); const on=()=>{ if (tf==null) tf=performance.now(); }; v.addEventListener('playing',on);
window.dispatchEvent(new CustomEvent('cview:play',{detail:{channelId:'__CH__'||lives[0].channelId}}));
for (let i=0;i<300 && tf==null;i++) await sleep(50);
post(`S first_live_frame_ms=${tf==null?'timeout':Math.round(tf-t0)}`); post('Z done'); })();
