//! VOD 검색(디바운스·더 보기)과 채널별 VOD.

use std::collections::HashSet;
use std::time::Duration;

use super::VodBrowseViewModel;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};
use crate::viewmodels::vod_library::VodLoadState;

impl VodBrowseViewModel {
    /// 검색어 변경 — 300ms 디바운스 후 자동 검색(2자 이상).
    pub fn update_search_query(&self, query: &str) {
        let mut s = self.state.lock();
        s.search_generation = s.search_generation.wrapping_add(1);
        s.search_query = query.to_string();
        cancel_slot(&mut s.search_task);
        let trimmed = query.trim().to_string();
        if trimmed.chars().count() < 2 {
            s.search_results.clear();
            s.search_state = VodLoadState::Idle;
            s.search_has_more = false;
            return;
        }
        let me = self.clone();
        s.search_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if !slot_is(&me.state.read().search_task, id) {
                return;
            }
            me.perform_search(trimmed).await;
        }));
    }

    pub(super) async fn perform_search(&self, keyword: String) {
        let generation = {
            let mut s = self.state.lock();
            if s.search_results.is_empty() {
                s.search_state = VodLoadState::Loading;
            }
            s.search_generation
        };
        let result = self
            .deps
            .search
            .search_videos(&keyword, 0, Self::SEARCH_PAGE_SIZE)
            .await;
        let mut s = self.state.lock();
        if s.search_generation != generation {
            return;
        }
        match result {
            Ok((items, has_next)) => {
                s.search_offset = items.len() as i64;
                s.search_has_more = has_next && !items.is_empty();
                s.search_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.search_results = items;
            }
            Err(e) => s.search_state = VodLoadState::Error(e),
        }
    }

    pub async fn load_more_search(&self) {
        let (keyword, offset, generation) = {
            let mut s = self.state.lock();
            if !s.search_has_more
                || s.is_loading_more_search
                || s.search_state != VodLoadState::Loaded
            {
                return;
            }
            let keyword = s.search_query.trim().to_string();
            if keyword.chars().count() < 2 {
                return;
            }
            s.is_loading_more_search = true;
            (keyword, s.search_offset, s.search_generation)
        };
        let result = self
            .deps
            .search
            .search_videos(&keyword, offset, Self::SEARCH_PAGE_SIZE)
            .await;
        let mut s = self.state.lock();
        s.is_loading_more_search = false;
        let Ok((items, has_next)) = result else {
            return;
        };
        if s.search_generation != generation {
            return;
        }
        let existing: HashSet<i64> = s.search_results.iter().map(|v| v.video_no).collect();
        s.search_offset += items.len() as i64;
        s.search_has_more = has_next && !items.is_empty();
        s.search_results.extend(
            items
                .into_iter()
                .filter(|v| !existing.contains(&v.video_no)),
        );
    }

    // MARK: 채널별

    pub async fn resolve_and_load_channel(&self) {
        self.channel_browser.resolve_and_load_channel().await;
    }

    pub async fn load_more_channel(&self) {
        self.channel_browser.load_more_channel().await;
    }
}
