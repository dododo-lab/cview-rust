(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
const MODE='__MODE__'; const URLS=__URLS__;
const dec=(src)=>{const t=performance.now(); const im=new Image(); im.src=src; return im.decode().then(()=>({w:im.naturalWidth,ms:performance.now()-t}),()=>({w:0,ms:performance.now()-t}));};
const check=async(name,fn)=>{ try{ const r=await fn(); post(`R ${r.ok?'PASS':'FAIL'} ${name} — ${r.msg}`);}catch(e){ post(`R FAIL ${name} — ${e.message}`);} };
if (MODE==='restart') {
 await check('다시 실행: 저장된 썸네일·아바타 바로', async()=>{ const r=await Promise.all(URLS.map(([k,u])=>dec(u))); const ok=r.filter(x=>x.w>0).length; const mx=Math.max(...r.map(x=>x.ms)); return {ok:ok===URLS.length&&mx<150, msg:`${ok}/${URLS.length} 최대 ${Math.round(mx)}ms`}; });
 await sleep(4000);
}
if (MODE==='offline') {
 await check('오프라인: 저장된 것은 보임', async()=>{ const r=await Promise.all(URLS.map(([k,u])=>dec(u))); const ok=r.filter(x=>x.w>0).length; return {ok:ok===URLS.length, msg:`${ok}/${URLS.length}`}; });
 await check('오프라인: 없는 것은 실패(빈칸)', async()=>{ const u=URLS.find(([k])=>k==='live')[1].replace('image_405','image_270'); const r=await dec(u); return {ok:r.w===0, msg:`w=${r.w}`}; });
}
if (MODE==='clear') {
 await check('썸네일 캐시 비우기', async()=>{ const a=await I('image_cache_status'); const b=await I('image_cache_clear'); return {ok:a.entries>0&&b.entries===0&&b.bytes===0, msg:`${a.entries}개 ${(a.bytes/1048576).toFixed(1)}MB → ${b.entries}개`}; });
}
post('Z done'); })();
