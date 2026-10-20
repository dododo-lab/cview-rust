(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
const ROUTE='__R__';
let box;
if (ROUTE==='chat'){ box=document.querySelector('#viewer .chat-list'); }
else { window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:ROUTE}})); await sleep(5000); box=document.querySelector('.page:not([hidden])'); }
if (!box){ post('S scroll none'); post('Z done'); return; }
box.scrollTop = ROUTE==='chat' ? box.scrollHeight : 0; await sleep(500);
const deltas=[]; let last=performance.now(); const end=last+4000; let dir=ROUTE==='chat'?-1:1;
await new Promise(res=>{ const step=(t)=>{ deltas.push(t-last); last=t; box.scrollTop+=dir*45; if (box.scrollTop<=0||box.scrollTop+box.clientHeight>=box.scrollHeight-2) dir=-dir; if (t<end) requestAnimationFrame(step); else res(); }; requestAnimationFrame(step); });
deltas.shift(); const s=[...deltas].sort((a,b)=>a-b); const q=(p)=>s[Math.min(s.length-1,Math.floor(p*s.length))];
post(`S scroll ${ROUTE} frames=${s.length} p50=${q(.5).toFixed(1)} p95=${q(.95).toFixed(1)} max=${s[s.length-1].toFixed(1)} over20=${(100*s.filter(x=>x>20).length/s.length).toFixed(1)}% height=${box.scrollHeight} items=${box.querySelectorAll('.card,.chat-line,.category-card,.channel-row').length}`);
post('Z done'); })();
