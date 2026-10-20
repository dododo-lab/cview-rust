(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
const SC='__SC__', CH=__CH__;
try{
  const nav=(r)=>window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:r}}));
  const v=document.getElementById('viewer');
  const closeViewer=async()=>{ if(!v.hidden){ document.getElementById('viewer-close').click(); await sleep(800);} };
  const lives=(await I('following_lives',{refresh:false})).sort((a,b)=>(b.concurrentUserCount||0)-(a.concurrentUserCount||0));
  const pick=(id)=>id && lives.find(l=>l.channelId===id) ? id : null;
  let used=[];
  if (SC!=='e') await I('multilive_stop_all');
  if (SC==='a'){ await closeViewer(); nav('home'); }
  if (SC==='b'){ await closeViewer(); nav('following'); }
  if (SC==='c'||SC==='f'){ const id=pick(CH[0])||lives[Math.min(4,lives.length-1)].channelId; used=[id]; window.dispatchEvent(new CustomEvent('cview:play',{detail:{channelId:id}})); }
  if (SC==='d'){ const id=pick(CH[0])||lives[0].channelId; used=[id]; window.dispatchEvent(new CustomEvent('cview:play',{detail:{channelId:id}})); }
  if (SC==='e'){ await closeViewer(); nav('multilive'); await sleep(500); await I('multilive_stop_all'); await sleep(1000);
    const ids=CH.length===4 && CH.every(pick)? CH : lives.slice(0,4).map(l=>l.channelId); used=ids;
    for (const id of ids) await I('multilive_add',{channelId:id}); await I('multilive_set_layout',{layout:'grid'}); }
  await sleep(12000);
  const vid=document.querySelector('#player-frame')?.contentDocument?.getElementById('video');
  post(`S sc=${SC} used=${JSON.stringify(used)} win=${innerWidth}x${innerHeight} vh=${vid&&!v.hidden?vid.videoHeight:'-'} chatLines=${document.querySelectorAll('.chat-list > *').length}`);
  post('Z done');
}catch(e){post('err '+e.stack)}})();
