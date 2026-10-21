(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
const nav=(r,p)=>window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:r,params:p||{}}}));
const check=async(name,fn)=>{ try{ const r=await fn(); post(`R ${r.ok?'PASS':'FAIL'} ${name} — ${r.msg}`);}catch(e){ post(`R FAIL ${name} — ${e.message}`);} };
const dec=(src)=>{const im=new Image(); im.src=src; return im.decode().then(()=>im.naturalWidth,()=>0);};
try{
 for (const [r,p] of [['home'],['popular'],['categories'],['search',{query:'게임'}]]) {
  await check(`화면 ${r} 썸네일`, async()=>{ nav(r,p); let imgs=[]; for(let i=0;i<40;i++){ await sleep(500); imgs=[...document.querySelectorAll('.page:not([hidden]) img')].filter(x=>x.getAttribute('src')); if(imgs.length>=4) break; }
    const srcs=imgs.slice(0,12).map(x=>x.getAttribute('src')); const viaCache=srcs.filter(s=>s.startsWith('cvimg:')).length; const w=await Promise.all(srcs.map(dec)); const ok=w.filter(x=>x>0).length;
    return {ok:srcs.length>=4&&ok===srcs.length&&viaCache===srcs.length, msg:`이미지 ${srcs.length}개, cvimg ${viaCache}, 디코드 ${ok}`}; });
 }
 const lives=await I('popular_lives',{size:60,refresh:false});
 const live=(u)=>window.__cvimgSrc?window.__cvimgSrc(u):u;
 const dom=await import('/js/dom.js'); const L=lives.slice(20,26);
 for (const l of L){ post('U live '+dom.imageSrc(l.thumbnailUrl,'live')); post('U avatar '+dom.imageSrc(l.channelImageUrl,'avatar')); }
 await Promise.all(L.flatMap(l=>[dec(dom.imageSrc(l.thumbnailUrl,'live')),dec(dom.imageSrc(l.channelImageUrl,'avatar'))]));
 await check('라이브 재생+채팅(회귀)', async()=>{ window.dispatchEvent(new CustomEvent('cview:play',{detail:{channelId:lives[0].channelId}})); let v; for(let i=0;i<24;i++){ await sleep(500); try{v=document.getElementById('player-frame').contentDocument.getElementById('video');}catch{} if(v&&v.readyState>=2) break; } const lines=document.querySelectorAll('#viewer .chat-list .chat-line').length; return {ok:!!v&&v.readyState>=2, msg:`rs=${v&&v.readyState} h=${v&&v.videoHeight} 채팅=${lines}줄 아바타=${(document.querySelector('#viewer img')||{}).src?.slice(0,8)}`}; });
 await check('멀티라이브 2패인(회귀)', async()=>{ document.getElementById('viewer-close').click(); await sleep(800); await I('multilive_stop_all'); for (const l of lives.slice(1,3)) await I('multilive_add',{channelId:l.channelId}); nav('multilive'); let ok=0; for(let i=0;i<30;i++){ await sleep(500); ok=[...document.querySelectorAll('.ml-pane:not([hidden]) iframe')].map(f=>{try{return f.contentDocument.getElementById('video')}catch{return null}}).filter(v=>v&&v.readyState>=2).length; if(ok>=2) break; } await I('multilive_stop_all'); return {ok:ok>=2, msg:`재생 패인 ${ok}`}; });
 await check('캐시 상태', async()=>{ await sleep(1500); const s=await I('image_cache_status'); return {ok:s.entries>0&&s.capBytes===268435456, msg:`${s.entries}개 ${(s.bytes/1048576).toFixed(1)}MB 상한 ${s.capBytes/1048576}MB`}; });
 post('Z done');
}catch(e){post('err '+e.stack)}})();
