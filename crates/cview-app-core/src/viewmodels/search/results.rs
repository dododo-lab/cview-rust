//! 검색 상태 — 결과 캐시와 버킷별 결과 정리(중복 제거·소진 판정).

use std::collections::{HashMap, HashSet};

use cview_core::models::search::{ClipInfo, SearchType};

use super::{
    new_search_id, sort_lives, SearchState, Snapshot, CLIP_SOURCE_CHANNEL_COUNT, PAGE_SIZE,
    SNAPSHOT_MAX_AGE, SNAPSHOT_MAX_ROWS,
};

impl SearchState {
    pub fn save_snapshot(&mut self) {
        if self.rendered_query.is_empty()
            || self.is_any_bucket_searching()
            || self.loaded_buckets.is_empty()
        {
            return;
        }
        let counts = [
            self.channel_results.len(),
            self.live_results.len(),
            self.video_results.len(),
            self.clip_results.len(),
        ];
        let key = self.rendered_query.clone();
        if !counts.iter().all(|c| *c <= SNAPSHOT_MAX_ROWS) {
            self.result_cache.remove(&key);
            return;
        }
        let snapshot = Snapshot {
            channels: self.channel_results.clone(),
            lives: self.live_results.clone(),
            videos: self.video_results.clone(),
            clips: self.clip_results.clone(),
            offsets: [
                self.channel_offset,
                self.live_offset,
                self.video_offset,
                self.clip_page,
            ],
            has_more: [
                self.has_more_channels,
                self.has_more_lives,
                self.has_more_videos,
                self.has_more_clips,
            ],
            loaded: self.loaded_buckets.clone(),
            errors: self.bucket_errors.clone(),
            fetched_at: self.result_fetched_at,
            clip_source_channel_ids: self.clip_source_channel_ids.clone(),
            clip_source_keyword: self.clip_source_keyword.clone(),
            clip_next_page: self.clip_next_page.clone(),
            clip_exhausted: self.clip_exhausted.clone(),
        };
        self.result_cache.insert(snapshot, &key);
    }

    pub fn restore_snapshot(&mut self, key: &str) -> bool {
        let now = self.now();
        let Some(s) = self
            .result_cache
            .value(key)
            .filter(|s| now - s.fetched_at < SNAPSHOT_MAX_AGE)
        else {
            self.result_cache.remove(key);
            return false;
        };
        self.channel_results = s.channels;
        self.live_results = s.lives;
        self.video_results = s.videos;
        self.clip_results = s.clips;
        [
            self.channel_offset,
            self.live_offset,
            self.video_offset,
            self.clip_page,
        ] = s.offsets;
        [
            self.has_more_channels,
            self.has_more_lives,
            self.has_more_videos,
            self.has_more_clips,
        ] = s.has_more;
        self.loaded_buckets = s.loaded;
        self.bucket_errors = s.errors;
        self.result_fetched_at = s.fetched_at;
        self.rendered_query = key.to_string();
        self.clip_source_channel_ids = s.clip_source_channel_ids;
        self.clip_source_keyword = s.clip_source_keyword;
        self.clip_next_page = s.clip_next_page;
        self.clip_exhausted = s.clip_exhausted;
        self.sort_live_results();
        true
    }

    // MARK: 결과 정리

    pub(super) fn reset_clip_sources(&mut self) {
        self.clip_source_channel_ids.clear();
        self.clip_source_keyword = None;
        self.clip_next_page.clear();
        self.clip_exhausted.clear();
    }

    pub fn clear_results(&mut self) {
        self.active_search_id = new_search_id();
        self.channel_results.clear();
        self.live_results.clear();
        self.video_results.clear();
        self.clip_results.clear();
        self.channel_offset = 0;
        self.live_offset = 0;
        self.video_offset = 0;
        self.clip_page = 0;
        self.reset_clip_sources();
        self.has_more_channels = true;
        self.has_more_lives = true;
        self.has_more_videos = true;
        self.has_more_clips = true;
        self.error_message = None;
        self.bucket_errors.clear();
        self.is_search_pending = false;
        self.completed_search_query = None;
        self.is_searching_channels = false;
        self.is_searching_lives = false;
        self.is_searching_videos = false;
        self.is_searching_clips = false;
    }

    pub fn sort_live_results(&mut self) {
        sort_lives(&mut self.live_results, self.live_sort_option);
    }

    pub fn needs_clip_source_expansion(&self, keyword: &str) -> bool {
        if self.clip_source_keyword.as_deref() != Some(keyword) {
            return false;
        }
        self.clip_next_page.len()
            < CLIP_SOURCE_CHANNEL_COUNT.min(self.clip_source_channel_ids.len())
    }

    pub(super) fn clip_fetch_targets(&self, channel_limit: usize) -> Vec<(String, i64)> {
        let window: Vec<&String> = self
            .clip_source_channel_ids
            .iter()
            .take(channel_limit.max(1))
            .filter(|id| !self.clip_exhausted.contains(*id))
            .collect();
        let unstarted: Vec<&String> = window
            .iter()
            .copied()
            .filter(|id| !self.clip_next_page.contains_key(*id))
            .collect();
        let chosen = if unstarted.is_empty() {
            window
        } else {
            unstarted
        };
        chosen
            .into_iter()
            .map(|id| {
                (
                    id.clone(),
                    self.clip_next_page.get(id).copied().unwrap_or(0),
                )
            })
            .collect()
    }

    pub(super) fn apply_clip_pages(
        &mut self,
        fetched: &HashMap<String, Vec<ClipInfo>>,
        targets: &[(String, i64)],
        keyword: &str,
    ) {
        let mut clips = Vec::new();
        for (id, page) in targets {
            let rows = fetched.get(id).cloned().unwrap_or_default();
            self.clip_next_page.insert(id.clone(), page + 1);
            if (rows.len() as i64) < PAGE_SIZE {
                self.clip_exhausted.insert(id.clone());
            }
            clips.extend(rows);
        }
        let keyword_lower = keyword.to_lowercase();
        let filtered: Vec<ClipInfo> = clips
            .into_iter()
            .filter(|c| c.clip_title.to_lowercase().contains(&keyword_lower))
            .collect();
        if self.clip_page == 0 {
            self.clip_results = filtered;
        } else {
            let seen: HashSet<String> = self
                .clip_results
                .iter()
                .map(|c| c.clip_uid.clone())
                .collect();
            self.clip_results
                .extend(filtered.into_iter().filter(|c| !seen.contains(&c.clip_uid)));
        }
        if !targets.is_empty() {
            self.clip_page += 1;
        }
        self.has_more_clips = self
            .clip_source_channel_ids
            .iter()
            .any(|id| !self.clip_exhausted.contains(id));
        self.loaded_buckets.insert(SearchType::Clip);
    }
}
