//! 채널 헤더 — 주간 후원 랭킹·라이브 시청자 정보 폴링.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+DonationRanking.swift`,
//! `Sources/CViewApp/ViewModels/ChatViewModel+LiveInfo.swift`.
//! Swift 는 `ChannelHeaderHub` 의 공유 갱신 스트림을 구독했다 — 여기서는 [`ChannelHeaderSource`]
//! 를 주기적으로 조회하는 루프로 같은 효과를 낸다(즉시 1회 + `interval` 마다).

use std::sync::{Arc, Weak};
use std::time::Duration;

use cview_core::models::chat::DonationRankResponse;
use cview_core::models::LiveInfo;

use super::{ChannelHeaderSource, ChatShared, ChatState, ChatViewModel};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

/// 폴링 인자 — 같은 채널 재연결 시 폴링 복구용.
#[derive(Clone)]
pub struct HeaderPollArgs {
    pub channel_id: String,
    pub source: Arc<dyn ChannelHeaderSource>,
    pub rank_count: i64,
    pub interval: Duration,
}

impl std::fmt::Debug for HeaderPollArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderPollArgs")
            .field("channel_id", &self.channel_id)
            .field("rank_count", &self.rank_count)
            .field("interval", &self.interval)
            .finish()
    }
}

pub const DEFAULT_RANKING_INTERVAL: Duration = Duration::from_secs(300);
pub const DEFAULT_LIVE_INFO_INTERVAL: Duration = Duration::from_secs(60);

fn spawn_poll<T, F, Fut>(
    weak: Weak<ChatShared>,
    interval: Duration,
    fetch: F,
    apply: fn(&mut ChatState, T),
    slot_of: fn(&ChatState) -> &Option<TaskSlot>,
) -> TaskSlot
where
    T: Send + 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send,
{
    TaskSlot::spawn(move |id| async move {
        loop {
            let result = fetch().await;
            let Some(shared) = weak.upgrade() else { break };
            {
                let mut s = shared.obs.lock();
                if !slot_is(slot_of(&s), id) {
                    break;
                }
                match result {
                    Ok(v) => apply(&mut s, v),
                    Err(e) => tracing::debug!("채널 헤더 갱신 실패 (silent to UI): {e}"),
                }
            }
            drop(shared);
            tokio::time::sleep(interval).await;
        }
    })
}

impl ChatState {
    pub fn start_donation_ranking_polling(
        &mut self,
        channel_id: String,
        source: Arc<dyn ChannelHeaderSource>,
        rank_count: i64,
        interval: Duration,
    ) {
        if self.donation_ranking_task.is_some()
            && self
                .donation_ranking_poll_args
                .as_ref()
                .is_some_and(|a| a.channel_id == channel_id)
        {
            return;
        }
        self.donation_ranking_poll_args = Some(HeaderPollArgs {
            channel_id: channel_id.clone(),
            source: source.clone(),
            rank_count,
            interval,
        });
        cancel_slot(&mut self.donation_ranking_task);
        self.donation_ranking_task = Some(spawn_poll(
            self.this.clone(),
            interval,
            move || {
                let source = source.clone();
                let channel_id = channel_id.clone();
                async move {
                    source
                        .weekly_donation_ranking(&channel_id, rank_count)
                        .await
                        .map_err(|e| e.to_string())
                }
            },
            ChatState::apply_donation_ranking,
            |s| &s.donation_ranking_task,
        ));
    }

    pub fn apply_donation_ranking(&mut self, response: DonationRankResponse) {
        let mut sorted = response.rank_list;
        sorted.sort_by_key(|r| r.ranking);
        if self.weekly_donation_ranking != sorted {
            self.weekly_donation_ranking = sorted;
        }
        if self.my_weekly_donation_rank != response.donation_user_weekly_rank {
            self.my_weekly_donation_rank = response.donation_user_weekly_rank;
        }
    }

    pub fn start_live_info_polling(
        &mut self,
        channel_id: String,
        source: Arc<dyn ChannelHeaderSource>,
        interval: Duration,
    ) {
        if self.live_info_task.is_some()
            && self
                .live_info_poll_args
                .as_ref()
                .is_some_and(|a| a.channel_id == channel_id)
        {
            return;
        }
        self.live_info_poll_args = Some(HeaderPollArgs {
            channel_id: channel_id.clone(),
            source: source.clone(),
            rank_count: 10,
            interval,
        });
        cancel_slot(&mut self.live_info_task);
        self.live_info_task = Some(spawn_poll(
            self.this.clone(),
            interval,
            move || {
                let source = source.clone();
                let channel_id = channel_id.clone();
                async move {
                    source
                        .live_status(&channel_id)
                        .await
                        .map_err(|e| e.to_string())
                }
            },
            ChatState::apply_live_info,
            |s| &s.live_info_task,
        ));
    }

    pub fn apply_live_info(&mut self, status: LiveInfo) {
        self.live_viewer_count = status.concurrent_user_count;
        if status.accumulate_count > 0 {
            self.live_accumulate_count = status.accumulate_count;
        }
        if let Some(open) = status.open_date {
            self.live_open_date = Some(open);
        }
    }
}

impl ChatViewModel {
    /// 1회 조회 — 실패는 UI 에 알리지 않는다.
    pub async fn fetch_donation_ranking(
        &self,
        channel_id: &str,
        source: &dyn ChannelHeaderSource,
        rank_count: i64,
    ) {
        match source.weekly_donation_ranking(channel_id, rank_count).await {
            Ok(r) => self.lock().apply_donation_ranking(r),
            Err(e) => tracing::debug!("주간 후원 랭킹 fetch 실패 (silent to UI): {e}"),
        }
    }

    pub async fn fetch_live_info_snapshot(
        &self,
        channel_id: &str,
        source: &dyn ChannelHeaderSource,
    ) {
        match source.live_status(channel_id).await {
            Ok(s) => self.lock().apply_live_info(s),
            Err(e) => tracing::debug!("라이브 정보 fetch 실패 (silent to UI): {e}"),
        }
    }
}
