(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
try{ localStorage.removeItem('cview.picture.v1'); }catch{}
let lives=[]; for(let i=0;i<20&&lives.length<8;i++){ try{ lives=await I('popular_lives',{size:30,refresh:i>0}); }catch{} if(lives.length<8) await sleep(500); }
await I('multilive_stop_all'); for (const l of lives.slice(0,4)) await I('multilive_add',{channelId:l.channelId}); window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'multilive'}}));
for(let i=0;i<40;i++){ await sleep(500); const n=[...document.querySelectorAll('.ml-pane:not([hidden]) iframe')].filter(f=>{try{return f.contentDocument.getElementById('video').readyState>=2}catch{return false}}).length; if(n>=4) break; }
post('M ready'); 
window.__sharpAll=(on)=>{ for (const f of document.querySelectorAll('.ml-pane:not([hidden]) iframe')) { try{ f.contentWindow.setSharpen(on,1);}catch{} } return [...document.querySelectorAll('.ml-pane:not([hidden]) iframe')].map(f=>{try{return f.contentDocument.getElementById('video').style.filter}catch{return '-'}}).join('|'); };
post('Z done'); })();
