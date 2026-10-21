(async function(){ if (location.search) return; const post=(t)=>fetch('http://127.0.0.1:47999/',{method:'POST',body:t});const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));const I=window.__TAURI__.core.invoke;
const vid=(f)=>{try{return f.contentDocument.getElementById('video')}catch{return null}};
const filt=(f)=>{const v=vid(f); return v?v.style.filter:'-';};
const sharp=(f)=>/url\(["']?#cview-unsharp/.test(filt(f));
const check=async(name,fn)=>{ try{ const r=await fn(); post(`R ${r.ok?'PASS':'FAIL'} ${name} — ${r.msg}`);}catch(e){ post(`R FAIL ${name} — ${e.message}`);} };
const CH=__CH__;
await check('다시 실행: 채널별 기억', async()=>{ const raw=localStorage.getItem('cview.picture.v1'); const p=JSON.parse(raw||'{}'); return {ok:p[CH[0]]?.sharpen===true&&p[CH[2]]?.sharpen===false&&p.__default?.sharpen===true, msg:raw}; });
await check('다시 실행: 패인에 다시 걸림', async()=>{ await I('multilive_stop_all'); for (const c of CH) await I('multilive_add',{channelId:c}); window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'multilive'}})); let panes=[]; for(let i=0;i<40;i++){ await sleep(500); panes=[...document.querySelectorAll('.ml-pane:not([hidden])')]; if(panes.length>=3&&panes.every(p=>{const v=vid(p.querySelector('iframe')); return v&&v.readyState>=2;})) break; } await sleep(1500); const st=await I('multilive_state'); const bySlot=Object.fromEntries(st.sessions.map(s=>[s.slot,s.channelId])); const r=panes.map(p=>[bySlot[Number(p.dataset.slot)], sharp(p.querySelector('iframe'))]); const want=(c)=>c!==CH[2]; await I('multilive_stop_all'); return {ok:r.length===3&&r.every(([c,s])=>s===want(c)), msg:r.map(([c,s])=>`${c.slice(0,6)}=${s}`).join(' ')}; });
post('Z done'); })();
