(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});
const I=window.__TAURI__.core.invoke; let dom={}; try { dom=await import('/js/dom.js'); } catch {}
const mk=(url,kind)=> (dom.imageSrc ? dom.imageSrc(url,kind) : url);
let lives=[]; for (let i=0;i<20 && lives.length<62;i++){ try{ lives=await I('popular_lives',{size:100,refresh:i>0}); }catch{} if (lives.length<62) await new Promise(r=>setTimeout(r,500)); }
const pick=lives.slice(50,62);
const set=[...pick.filter(l=>l.thumbnailUrl).map(l=>[mk(l.thumbnailUrl,'live'),'live']), ...pick.filter(l=>l.channelImageUrl).map(l=>[mk(l.channelImageUrl,'avatar'),'avatar'])];
async function run(label){
  const t0=performance.now(); const res=await Promise.all(set.map(([src,k])=>{ const im=new Image(); im.src=src; return im.decode().then(()=>({k,ms:performance.now()-t0,w:im.naturalWidth}),()=>({k,ms:-1,w:0})); }));
  const ok=res.filter(r=>r.ms>=0); const s=ok.map(r=>r.ms).sort((a,b)=>a-b); const q=(p)=>Math.round(s[Math.min(s.length-1,Math.floor(p*s.length))]||0);
  const byk=(k)=>{const v=ok.filter(r=>r.k===k).map(r=>r.ms).sort((a,b)=>a-b); return v.length?Math.round(v[Math.floor(v.length/2)]):-1;};
  post(`S ${label} n=${set.length} ok=${ok.length} all=${q(0.999)}ms p50=${q(0.5)}ms live_p50=${byk('live')}ms avatar_p50=${byk('avatar')}ms widths=${[...new Set(ok.map(r=>r.k+':'+r.w))].slice(0,4).join(',')}`);
}
await run(location.hash||'load'); await run('revisit');
post('Z done'); })();
