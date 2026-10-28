//! 보관함 — 이어 보기·나중에 볼 VOD 목록과 변경.

use cview_core::models::search::VodInfo;

use super::{LibrarySegment, VodBrowseViewModel, VodTab};
use crate::services::support::lock;
use crate::viewmodels::vod_library::{MutationReceipt, VodLoadState};

impl VodBrowseViewModel {
    pub async fn load_continue_watching(&self) {
        let generation = self.state.read().continue_mutation_generation;
        if self.deps.data_store.is_none() {
            let mut s = self.state.lock();
            s.continue_watching.clear();
            s.library_state = VodLoadState::Empty;
            return;
        }
        {
            let mut s = self.state.lock();
            if s.continue_watching.is_empty() {
                s.library_state = VodLoadState::Loading;
            }
        }
        let result = self.blocking(|st| st.fetch_recent_vod_playback(30)).await;
        let mut s = self.state.lock();
        if s.continue_mutation_generation != generation {
            return;
        }
        match result {
            None => {}
            Some(Ok(items)) => {
                s.library_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.continue_watching = items;
            }
            Some(Err(e)) => {
                tracing::error!("이어 보기 조회 실패: {e}");
                s.library_state = VodLoadState::Error("보관함을 불러오지 못했습니다".into());
            }
        }
    }

    /// 이어 보기에서 제거(시청 완료 처리).
    pub async fn remove_from_continue_watching(&self, video_no: i64) {
        if self.deps.data_store.is_none()
            || !self.state.lock().continue_removal_ids.insert(video_no)
        {
            return;
        }
        let result = self
            .blocking(move |st| st.remove_vod_playback(video_no))
            .await;
        let mut s = self.state.lock();
        s.continue_removal_ids.remove(&video_no);
        match result {
            Some(Ok(())) => {
                s.continue_mutation_generation = s.continue_mutation_generation.wrapping_add(1);
                if let Some(m) = &self.deps.progress_mirror {
                    lock(m).clear(video_no);
                }
                s.continue_watching.retain(|r| r.video_no != video_no);
                if s.continue_watching.is_empty() {
                    s.library_state = VodLoadState::Empty;
                }
            }
            _ => {
                s.library_mutation_error =
                    Some("이어 보기에서 제거하지 못했습니다. 다시 시도해 주세요.".into());
            }
        }
    }

    // MARK: 보관함 — 나중에 볼 VOD

    pub async fn load_watch_later(&self) {
        if self.mutations.has_pending() {
            return;
        }
        let generation = self.state.read().library_mutation_generation;
        if self.deps.data_store.is_none() {
            let mut s = self.state.lock();
            s.watch_later.clear();
            s.watch_later_state = VodLoadState::Empty;
            return;
        }
        {
            let mut s = self.state.lock();
            if s.watch_later.is_empty() {
                s.watch_later_state = VodLoadState::Loading;
            }
        }
        let result = self.blocking(|st| st.fetch_vod_watch_later(100)).await;
        let mut s = self.state.lock();
        if s.library_mutation_generation != generation || self.mutations.has_pending() {
            return;
        }
        match result {
            Some(Ok(items)) => {
                s.watch_later_ids = items.iter().map(|i| i.video_no).collect();
                s.watch_later_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.watch_later = items;
            }
            Some(Err(e)) => {
                tracing::error!("나중에 볼 조회 실패: {e}");
                s.watch_later_state = VodLoadState::Error("보관함을 불러오지 못했습니다".into());
            }
            None => {}
        }
    }

    /// 카드 토글 상태용 id 집합만 가볍게 로드.
    pub async fn load_watch_later_ids(&self) {
        if self.mutations.has_pending() || self.deps.data_store.is_none() {
            return;
        }
        let generation = self.state.read().library_mutation_generation;
        let ids = self
            .blocking(|st| st.fetch_vod_watch_later_ids())
            .await
            .and_then(Result::ok)
            .unwrap_or_default();
        let mut s = self.state.lock();
        if s.library_mutation_generation != generation || self.mutations.has_pending() {
            return;
        }
        s.watch_later_ids = ids.into_iter().collect();
    }

    pub fn is_in_watch_later(&self, video_no: i64) -> bool {
        self.state.read().watch_later_ids.contains(&video_no)
    }

    /// 나중에 볼 목록에 담기/빼기 토글.
    pub async fn toggle_watch_later(&self, vod: &VodInfo) {
        let Some(store) = self.deps.data_store.clone() else {
            return;
        };
        let no = vod.video_no;
        let (initially_saved, desired) = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let initially = s.watch_later_ids.contains(&no);
            if initially {
                s.watch_later_ids.remove(&no);
            } else {
                s.watch_later_ids.insert(no);
            }
            (initially, !initially)
        };
        let title = vod.video_title.clone();
        let channel_name = vod
            .channel
            .as_ref()
            .map(|c| c.channel_name.clone())
            .unwrap_or_default();
        let thumbnail = vod.video_image_url.as_ref().map(|u| u.to_string());
        let duration = vod.duration as f64;
        let write = async move {
            tokio::task::spawn_blocking(move || {
                if desired {
                    store.add_vod_watch_later(
                        no,
                        &title,
                        &channel_name,
                        thumbnail.as_deref(),
                        duration,
                    )
                } else {
                    store.remove_vod_watch_later(no)
                }
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
        };
        let receipt = self
            .mutations
            .apply(no, initially_saved, desired, write)
            .await;
        self.commit_watch_later_mutation(no, receipt).await;
    }

    /// 나중에 볼 목록에서 제거.
    pub async fn remove_from_watch_later(&self, video_no: i64) {
        let Some(store) = self.deps.data_store.clone() else {
            return;
        };
        let initially_saved = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let initially = s.watch_later_ids.contains(&video_no);
            s.watch_later_ids.remove(&video_no);
            initially
        };
        let write = async move {
            tokio::task::spawn_blocking(move || store.remove_vod_watch_later(video_no))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        };
        let receipt = self
            .mutations
            .apply(video_no, initially_saved, false, write)
            .await;
        self.commit_watch_later_mutation(video_no, receipt).await;
    }

    async fn commit_watch_later_mutation(&self, video_no: i64, receipt: MutationReceipt) {
        if !self.mutations.is_current(video_no, receipt.token) {
            return;
        }
        let reload = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let outcome = &receipt.outcome;
            if outcome.is_member {
                s.watch_later_ids.insert(video_no);
            } else {
                s.watch_later_ids.remove(&video_no);
            }
            if outcome.error.is_some() {
                s.library_mutation_error =
                    Some("나중에 볼 목록을 저장하지 못했습니다. 다시 시도해 주세요.".into());
            }
            if !outcome.is_member {
                s.watch_later.retain(|r| r.video_no != video_no);
                if s.watch_later.is_empty() {
                    s.watch_later_state = VodLoadState::Empty;
                }
            }
            s.library_segment == LibrarySegment::WatchLater
        };
        self.mutations.finish(video_no, receipt.token);
        if reload {
            self.load_watch_later().await;
        }
    }

    /// 미리보기의 "이 채널의 VOD 보기" — 채널 탭으로 전환 후 로드.
    pub fn show_channel_vods(&self, channel_id: &str) {
        self.state.lock().preview_vod = None;
        self.change_tab(VodTab::Channel);
        if tokio::runtime::Handle::try_current().is_ok() {
            drop(self.channel_browser.select(channel_id));
        }
    }
}
