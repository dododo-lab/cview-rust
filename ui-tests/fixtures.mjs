// Synthetic data for layout checks only; no live accounts or streams.
export function setup({settings,theme='dark',loggedIn=true,empty=false,richLibrary=false}) {
 if (window.top !== window) return;
 localStorage.clear();localStorage.setItem('cview.theme',theme);localStorage.setItem('cview.welcomed.v1','1');
 settings.appearance.theme=theme;
 const lives=Array.from({length:12},(_,i)=>({channelId:`ch-${i}`,liveId:i+1,chatChannelId:`chat-${i}`,channelName:['서늘한 오후','CView 스튜디오','긴 채널 이름과 함께하는 아주 특별한 오늘의 이야기','밤의 산책'][i%4],liveTitle:['오늘도 편안하게 함께해요 · 음악과 이야기','새로운 세계로 모험을 떠나는 날! 첫 플레이부터 엔딩까지','아주 긴 방송 제목이 여러 줄로 표시될 때 카드와 컨트롤이 안정적으로 정렬되는지 확인합니다'][i%3],thumbnailUrl:`${location.origin}/fixture/${i}.svg`,channelImageUrl:`${location.origin}/fixture/${i+20}.svg`,isLive:true,adult:false,viewerCount:14532-i*1100,viewerCountText:(14532-i*1100).toLocaleString('ko-KR'),categoryName:['음악','게임','소통','야외'][i%4],openDate:new Date(Date.now()-((i+1)*900000)).toISOString(),verified:i%3===0}));
 const list=empty?[]:lives;
 const channels=list.map(x=>({...x,openLive:true,verified:true,followerCountText:'12.3만',followerCount:123000}));
 const clips=list.slice(0,6).map((l,i)=>({clipUID:`clip-${i}`,clipTitle:l.liveTitle,thumbnailImageUrl:l.thumbnailUrl,readCount:1000+i*100,duration:42,publishDate:new Date().toISOString(),optionalProperty:{ownerChannel:{channelId:l.channelId,channelName:l.channelName,channelImageUrl:l.channelImageUrl}},channel:{channelId:l.channelId,channelName:l.channelName}}));
 const insights={total:124200,last7:32400,last30:108000,streak:5,average:2400,sessionCount:42,countedSessionCount:40,uniqueChannelCount:12,favoriteCount:4,bookmarkCount:3,channels:lives.slice(0,5).map((l,i)=>({name:l.channelName,seconds:9000-i*1300})),categories:[{name:'음악',seconds:15000},{name:'게임',seconds:12000},{name:'소통',seconds:7200}],hourly:Array.from({length:24},(_,i)=>i>14?(i-14)*300:0),daily:Array.from({length:30},(_,i)=>({date:new Date(Date.now()-(29-i)*86400000).toISOString(),seconds:i%6*700})),recent:lives.slice(0,5).map(l=>({channelId:l.channelId,channelName:l.channelName,thumbnailUrl:l.channelImageUrl,categoryName:l.categoryName,startedAt:new Date().toISOString(),seconds:1800,active:false}))};
 const vods=lives.slice(0,4).map((l,i)=>({videoNo:i+1,videoTitle:l.liveTitle,title:l.liveTitle,thumbnailImageUrl:l.thumbnailUrl,thumbnailUrl:l.thumbnailUrl,channel:{channelName:l.channelName},channelName:l.channelName,channelId:l.channelId,duration:7200,position:(i+1)*900,readCount:12345,publishDate:new Date().toISOString()}));
 const downloads={folder:'/Downloads/CView',jobs:['downloading','queued','cancelled'].map((state,i)=>({id:`job-${i}`,kind:'vod',target:String(i+1),title:lives[i].liveTitle,channelName:lives[i].channelName,state,progress:.42,bytes:123456789})),history:['available','missing','failed'].map((status,i)=>({id:`vod-${i+1}`,title:lives[i].liveTitle,channelName:lives[i].channelName,completed:status!=='failed',finishedAt:new Date().toISOString(),fileStatus:status,filePath:`/Downloads/CView/video-${i}.mp4`,fileSize:123456789,failureReason:status==='failed'?'네트워크 연결을 확인해 주세요':null}))};
 const events=new Map();window.__design={settings,lives,events,calls:[],emit(name,payload){for(const fn of events.get(name)||[])fn({payload});}};
 const invoke=async(name,args={})=>{
  window.__design.calls.push(name);
  switch(name){
   case 'settings_get': return structuredClone(settings);
   case 'settings_update':settings[args.domain]={...settings[args.domain],...args.patch};return structuredClone(settings);
   case 'appearance_set_theme':settings.appearance.theme=args.theme;return;
   case 'auth_status':return {loggedIn,nickname:'디자인 검토',state:loggedIn?'loggedIn':'loggedOut'};
   case 'popular_lives':case 'following_lives':return list;
   case 'following_channels_all':return channels;
   case 'channel_info':return channels[0]||lives[0];
   case 'channel_detail':return {channel:{...lives[0],channelDescription:'작은 이야기들이 모이는 편안한 공간입니다. 오늘의 방송, 다시보기와 클립을 만나보세요.',followerCountText:'12.3만'},live:lives[0],memo:'',isFavorite:false};
   case 'live_detail':return lives[0];
   case 'search':return {lives:list,channels};
   case 'channel_vods':case 'search_videos':case 'channel_clips':return {items:name==='channel_clips'?clips:[],hasMore:false,totalCount:6};
   case 'popular_clips':return clips;
   case 'following_vods':case 'vod_continue_list':case 'vod_watch_later_list':return richLibrary?vods:[];
   case 'bookmarks_list':return richLibrary?lives.slice(0,3).map((l,i)=>({...l,id:`bookmark-${i}`,liveUptime:1800,capturedAt:new Date().toISOString()})):[];
   case 'insights_overview':return insights;
   case 'recent_channels':case 'favorites_list':return channels.slice(0,4);
   case 'downloads_list':case 'download_list':return richLibrary?downloads:{jobs:[],history:[],folder:'/Downloads/CView'};
   case 'data_health':return {available:true,storeBytes:234567,backups:[{createdAt:new Date().toISOString(),reason:'manual',bytes:234567}],tables:[{label:'시청 기록',rows:42},{label:'설정',rows:11}],backupDir:'/data/Backups',backupCount:2,storePath:'/data/CView.store',settingsSnapshot:true,freeBytes:3456789012,usage:{storeBytes:234567}};
   case 'oauth_status':return {hasCredentials:false,connected:false,redirectUri:"http://localhost:52735/callback"};
   case 'app_info':return {version:'0.1.0',platform:'linux',arch:'x86_64'};
   case 'multilive_state':return {sessions:[],maxSessions:8,canAdd:true,layout:'preset',focusRatio:.7,focusDirection:'left',multiAudio:false,selectedId:null};
   case 'deeplink_take_pending':return [];
   case 'vod_play':return {videoNo:1,title:lives[0].liveTitle,channelId:lives[0].channelId,channelName:lives[0].channelName,streamUrl:location.origin+'/fixture/video.mp4',duration:3600,resumeAt:0,qualities:[{name:'1080p',resolution:'1920×1080',url:location.origin+'/fixture/video.mp4'}]};
   case 'clip_play':return {detail:clips[0]};
   case 'player_start':return {live:lives[0],audioOnly:false};
   case 'player_record_status':return {recording:false};
   case 'notify_permission':return 'granted';
   case 'equalizer_presets':return [[],[]];
   default:return [];
  }
 };
 const listen=async(name,fn)=>{if(!events.has(name))events.set(name,new Set());events.get(name).add(fn);return()=>events.get(name).delete(fn);};
 window.__TAURI__={core:{invoke},event:{listen},webviewWindow:{getCurrentWebviewWindow:()=>({listen})}};
}
