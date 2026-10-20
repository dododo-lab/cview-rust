(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
const E=window.__TAURI__.event; let n=0;
const mk=()=>({id:`syn-${++n}`,kind:'chat',nickname:`테스트${n%50}`,content:`합성 메시지 ${n} 가나다라마바사 아자차카타파하 ㅋㅋㅋㅋ`,emojis:{},role:'common_user',badges:[],time:Date.now()});
for (const rate of [__RATES__]){
  const deltas=[]; let last=performance.now(); let running=true;
  const tick=(t)=>{ deltas.push(t-last); last=t; if (running) requestAnimationFrame(tick); }; requestAnimationFrame(tick);
  const perTick=Math.max(1,Math.round(rate/20)); const until=performance.now()+5000;
  while (performance.now()<until){ await E.emit('chat://event',{type:'messages',messages:Array.from({length:perTick},mk)}); await sleep(50); }
  running=false; await sleep(100); deltas.shift(); const s=[...deltas].sort((a,b)=>a-b); const q=(p)=>s[Math.min(s.length-1,Math.floor(p*s.length))];
  post(`S chat rate=${rate}/s frames=${s.length} p50=${q(.5).toFixed(1)} p95=${q(.95).toFixed(1)} over20=${(100*s.filter(x=>x>20).length/s.length).toFixed(1)}% lines=${document.querySelectorAll('#viewer .chat-list .chat-line').length}`);
  await sleep(1000);
}
post('Z done'); })();
