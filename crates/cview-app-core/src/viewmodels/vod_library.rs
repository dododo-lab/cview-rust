//! VOD 보관함 저장 순서 큐와 채널별 VOD 브라우저.
//!
//! 원본:
//! - `Sources/CViewApp/ViewModels/VODLibraryMutationQueue.swift`
//! - `Sources/CViewApp/ViewModels/VODChannelBrowser.swift`
//! - `VODBrowseViewModel.LoadState`
//!
//! Swift `Task.isCancelled` 확인은 요청 토큰 비교로 대신한다(토큰이 바뀌면 결과를 버린다).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde::Serialize;

use cview_core::models::search::VodInfo;
use cview_networking::{ChzzkApiClient, PagedContent};

use super::clip::channel_resolver;
use super::support::{Observed, ObservedGuard};

/// `VODBrowseViewModel.LoadState`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "camelCase")]
pub enum VodLoadState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Empty,
    Error(String),
}

// MARK: - VODLibraryMutationQueue

/// 저장 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationOutcome {
    pub is_member: bool,
    pub error: Option<String>,
}

/// 영수증.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationReceipt {
    pub token: u64,
    pub outcome: MutationOutcome,
}

type OutcomeFuture = Shared<BoxFuture<'static, MutationOutcome>>;

struct Job {
    token: u64,
    task: OutcomeFuture,
}

#[derive(Default)]
struct QueueInner {
    sequence: u64,
    jobs: HashMap<i64, Job>,
}

/// 항목별 저장 순서를 유지한다. 이전 실패 뒤에도 실제로 저장된 상태를 다음 작업에 넘긴다.
#[derive(Default)]
pub struct VodLibraryMutationQueue {
    inner: Mutex<QueueInner>,
}

impl VodLibraryMutationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_pending(&self) -> bool {
        !self.inner.lock().unwrap().jobs.is_empty()
    }

    /// 앞선 같은 항목 작업이 끝난 뒤 `write` 를 실행한다. 작업은 호출자가 기다리지 않아도 돈다.
    pub async fn apply<W>(
        &self,
        video_no: i64,
        initially_saved: bool,
        desired: bool,
        write: W,
    ) -> MutationReceipt
    where
        W: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let (token, task) = {
            let mut inner = self.inner.lock().unwrap();
            inner.sequence = inner.sequence.wrapping_add(1);
            let token = inner.sequence;
            let previous = inner.jobs.get(&video_no).map(|j| j.task.clone());
            let handle = tokio::spawn(async move {
                let committed = match previous {
                    Some(p) => p.await.is_member,
                    None => initially_saved,
                };
                match write.await {
                    Ok(()) => MutationOutcome {
                        is_member: desired,
                        error: None,
                    },
                    Err(e) => MutationOutcome {
                        is_member: committed,
                        error: Some(e),
                    },
                }
            });
            let task: OutcomeFuture = async move {
                handle.await.unwrap_or(MutationOutcome {
                    is_member: initially_saved,
                    error: Some("cancelled".into()),
                })
            }
            .boxed()
            .shared();
            inner.jobs.insert(
                video_no,
                Job {
                    token,
                    task: task.clone(),
                },
            );
            (token, task)
        };
        MutationReceipt {
            token,
            outcome: task.await,
        }
    }

    pub fn is_current(&self, video_no: i64, token: u64) -> bool {
        self.inner
            .lock()
            .unwrap()
            .jobs
            .get(&video_no)
            .map(|j| j.token)
            == Some(token)
    }

    pub fn finish(&self, video_no: i64, token: u64) {
        let mut inner = self.inner.lock().unwrap();
        if inner.jobs.get(&video_no).map(|j| j.token) == Some(token) {
            inner.jobs.remove(&video_no);
        }
    }
}

// MARK: - VODChannelBrowser

/// 채널 VOD 브라우저 API 면.
#[async_trait]
pub trait VodChannelApi: Send + Sync {
    async fn vod_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<VodInfo>, String>;
    /// 채널명 검색 첫 결과 `(channelId, channelName)`.
    async fn first_channel(&self, keyword: &str) -> Result<Option<(String, String)>, String>;
}

#[async_trait]
impl VodChannelApi for ChzzkApiClient {
    async fn vod_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<VodInfo>, String> {
        ChzzkApiClient::vod_list(self, channel_id, page, size, true)
            .await
            .map_err(|e| e.to_string())
    }
    async fn first_channel(&self, keyword: &str) -> Result<Option<(String, String)>, String> {
        ChzzkApiClient::search_channels(self, keyword, 0, 1)
            .await
            .map(|r| {
                r.data
                    .into_iter()
                    .next()
                    .map(|c| (c.channel_id, c.channel_name))
            })
            .map_err(|e| e.to_string())
    }
}

/// 채널 VOD 페이지 크기.
pub const CHANNEL_VOD_PAGE_SIZE: i64 = 24;

/// 채널 VOD 브라우저 상태.
#[derive(Debug, Default)]
pub struct VodChannelBrowserState {
    channel_request_token: u64,
    pub channel_input: String,
    channel_vods: Vec<VodInfo>,
    channel_state: VodLoadState,
    resolved_channel_name: Option<String>,
    resolved_channel_id: Option<String>,
    channel_page: i64,
    channel_has_more: bool,
    is_loading_more_channel: bool,
}

impl VodChannelBrowserState {
    pub fn channel_vods(&self) -> &[VodInfo] {
        &self.channel_vods
    }
    pub fn channel_state(&self) -> &VodLoadState {
        &self.channel_state
    }
    pub fn resolved_channel_name(&self) -> Option<&str> {
        self.resolved_channel_name.as_deref()
    }
    pub fn resolved_channel_id(&self) -> Option<&str> {
        self.resolved_channel_id.as_deref()
    }
    pub fn channel_page(&self) -> i64 {
        self.channel_page
    }
    pub fn channel_has_more(&self) -> bool {
        self.channel_has_more
    }
    pub fn is_loading_more_channel(&self) -> bool {
        self.is_loading_more_channel
    }
}

/// 채널 VOD 브라우저 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodChannelBrowserSnapshot {
    pub channel_input: String,
    pub channel_vods: Vec<VodInfo>,
    pub channel_state: VodLoadState,
    pub resolved_channel_name: Option<String>,
    pub channel_has_more: bool,
    pub is_loading_more_channel: bool,
}

struct BrowserShared {
    obs: Observed<VodChannelBrowserState>,
    api: Arc<dyn VodChannelApi>,
}

/// 채널 검색과 VOD 목록이 한 요청 토큰을 공유해 늦은 검색이 새 선택을 덮지 못하게 한다.
#[derive(Clone)]
pub struct VodChannelBrowser {
    shared: Arc<BrowserShared>,
}

impl VodChannelBrowser {
    pub fn new(api: Arc<dyn VodChannelApi>) -> Self {
        Self {
            shared: Arc::new(BrowserShared {
                obs: Observed::new(VodChannelBrowserState::default()),
                api,
            }),
        }
    }

    pub fn lock(&self) -> ObservedGuard<'_, VodChannelBrowserState> {
        self.shared.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, VodChannelBrowserState> {
        self.shared.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    pub fn snapshot(&self) -> VodChannelBrowserSnapshot {
        let st = self.read();
        VodChannelBrowserSnapshot {
            channel_input: st.channel_input.clone(),
            channel_vods: st.channel_vods.clone(),
            channel_state: st.channel_state.clone(),
            resolved_channel_name: st.resolved_channel_name.clone(),
            channel_has_more: st.channel_has_more,
            is_loading_more_channel: st.is_loading_more_channel,
        }
    }

    /// 채널을 고르고 첫 페이지를 백그라운드로 받는다.
    pub fn select(&self, channel_id: &str) -> tokio::task::JoinHandle<()> {
        let token = {
            let mut st = self.lock();
            st.channel_request_token = st.channel_request_token.wrapping_add(1);
            st.channel_input = channel_id.to_string();
            st.resolved_channel_id = Some(channel_id.to_string());
            st.resolved_channel_name = None;
            st.channel_vods.clear();
            st.channel_state = VodLoadState::Loading;
            st.is_loading_more_channel = false;
            st.channel_request_token
        };
        let weak = Arc::downgrade(&self.shared);
        tokio::spawn(async move {
            if let Some(shared) = weak.upgrade() {
                VodChannelBrowser { shared }
                    .load_channel_vods(true, Some(token))
                    .await;
            }
        })
    }

    pub async fn refresh(&self) {
        let token = {
            let mut st = self.lock();
            if st.resolved_channel_id.is_none() {
                return;
            }
            st.channel_request_token = st.channel_request_token.wrapping_add(1);
            st.is_loading_more_channel = false;
            st.channel_request_token
        };
        self.load_channel_vods(true, Some(token)).await;
    }

    pub fn invalidate(&self) {
        let mut st = self.lock();
        st.channel_request_token = st.channel_request_token.wrapping_add(1);
        st.channel_input.clear();
        st.resolved_channel_id = None;
        st.resolved_channel_name = None;
        st.channel_vods.clear();
        st.channel_page = 0;
        st.channel_state = VodLoadState::Idle;
        st.channel_has_more = false;
        st.is_loading_more_channel = false;
    }

    /// 입력(채널명 또는 URL/ID)을 해석해 그 채널의 VOD 목록을 로드.
    pub async fn resolve_and_load_channel(&self) {
        let (raw, token) = {
            let mut st = self.lock();
            let raw = st.channel_input.trim().to_string();
            if raw.is_empty() {
                return;
            }
            st.channel_request_token = st.channel_request_token.wrapping_add(1);
            st.channel_state = VodLoadState::Loading;
            st.channel_vods.clear();
            st.resolved_channel_name = None;
            st.resolved_channel_id = None;
            st.channel_page = 0;
            st.is_loading_more_channel = false;
            (raw, st.channel_request_token)
        };
        let mut channel_id = channel_resolver::resolve_channel_id(&raw);
        let mut channel_name = None;
        if channel_id.is_none() {
            if let Ok(Some((id, name))) = self.shared.api.first_channel(&raw).await {
                channel_id = Some(id);
                channel_name = Some(name);
            }
        }
        {
            let mut st = self.lock();
            if st.channel_request_token != token {
                return;
            }
            match channel_id.filter(|c| !c.is_empty()) {
                Some(cid) => {
                    st.resolved_channel_id = Some(cid);
                    st.resolved_channel_name = channel_name;
                }
                None => {
                    st.channel_state = VodLoadState::Error("채널을 찾을 수 없습니다".into());
                    return;
                }
            }
        }
        self.load_channel_vods(true, Some(token)).await;
    }

    async fn load_channel_vods(&self, reset: bool, request_token: Option<u64>) {
        let (cid, token, page) = {
            let mut st = self.lock();
            let Some(cid) = st.resolved_channel_id.clone() else {
                return;
            };
            let token = request_token.unwrap_or(st.channel_request_token);
            if st.channel_request_token != token {
                return;
            }
            if reset {
                st.channel_page = 0;
            }
            (cid, token, st.channel_page)
        };
        let result = self
            .shared
            .api
            .vod_list(&cid, page, CHANNEL_VOD_PAGE_SIZE)
            .await;
        let mut st = self.lock();
        if st.channel_request_token != token {
            return;
        }
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                if reset {
                    // 이미 보여 주던 목록이 있으면 지우지 않는다.
                    st.channel_state = if st.channel_vods.is_empty() {
                        VodLoadState::Error(e)
                    } else {
                        VodLoadState::Loaded
                    };
                } else {
                    // 실패한 페이지를 다시 요청할 수 있게 커서를 되돌린다.
                    st.channel_page = (st.channel_page - 1).max(0);
                }
                return;
            }
        };
        let items = result.data;
        if reset {
            st.channel_vods = items.clone();
        } else {
            let existing: HashSet<i64> = st.channel_vods.iter().map(|v| v.video_no).collect();
            let fresh: Vec<VodInfo> = items
                .iter()
                .filter(|v| !existing.contains(&v.video_no))
                .cloned()
                .collect();
            st.channel_vods.extend(fresh);
        }
        st.channel_has_more = items.len() as i64 >= CHANNEL_VOD_PAGE_SIZE;
        st.channel_state = if st.channel_vods.is_empty() {
            VodLoadState::Empty
        } else {
            VodLoadState::Loaded
        };
        if st.resolved_channel_name.is_none() {
            if let Some(name) = items
                .first()
                .and_then(|v| v.channel.as_ref())
                .map(|c| c.channel_name.clone())
            {
                st.resolved_channel_name = Some(name);
            }
        }
    }

    pub async fn load_more_channel(&self) {
        let token = {
            let mut st = self.lock();
            if !st.channel_has_more
                || st.is_loading_more_channel
                || st.channel_state != VodLoadState::Loaded
            {
                return;
            }
            st.is_loading_more_channel = true;
            st.channel_page += 1;
            st.channel_request_token
        };
        self.load_channel_vods(false, Some(token)).await;
        let mut st = self.lock();
        if st.channel_request_token == token {
            st.is_loading_more_channel = false;
        }
    }
}
