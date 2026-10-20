(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
await I('multilive_stop_all'); const v=document.getElementById('viewer'); if(!v.hidden){ document.getElementById('viewer-close').click(); await sleep(800);}
const ids=__CH3__; await I('settings_update',{domain:'multiChat',patch:{savedSessions:ids.map(id=>({channelId:id,channelName:id})),selectedChannelId:ids[0],isGridLayout:true}}).catch(e=>post('S settings err '+e));
window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'multichat'}})); await sleep(12000);
post(`S g panes=${document.querySelectorAll('.mc-pane').length} lines=${document.querySelectorAll('.mc-pane .chat-line').length}`); post('Z done'); })();
