//! 검색 버킷 받기 — 채널·라이브·다시보기·클립 요청과 더 보기.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use cview_core::models::search::{ClipListPage, SearchType};
use cview_core::models::{ChannelInfo, LiveInfo};

use super::{
    bucket_fields, channel_name_key, sort_channels, sort_lives, trimmed, SearchScope, SearchState,
    SearchViewModel, CHANNEL_LIVE_LIMIT, CLIP_SOURCE_CHANNEL_COUNT, PAGE_SIZE,
};

impl SearchViewModel {
    pub(super) async fn fetch_bucket(
        &self,
        ty: SearchType,
        keyword: &str,
        reset: bool,
        token: u64,
    ) {
        match ty {
            SearchType::Channel => self.fetch_channels(keyword, reset, token).await,
            SearchType::Live => self.fetch_lives(keyword, reset, token).await,
            SearchType::Video => self.fetch_videos(keyword, reset, token).await,
            SearchType::Clip => {
                self.fetch_clips(keyword, reset, token, CLIP_SOURCE_CHANNEL_COUNT)
                    .await
            }
        }
    }

    pub async fn load_more(&self) {
        let (t, token, ty) = {
            let st = self.read();
            let t = trimmed(&st.query);
            if t.is_empty() || st.rendered_query != t {
                return;
            }
            let ty = match st.selected_scope {
                SearchScope::Channel if st.has_more_channels && !st.is_searching_channels => {
                    Some(SearchType::Channel)
                }
                SearchScope::Live if st.has_more_lives && !st.is_searching_lives => {
                    Some(SearchType::Live)
                }
                SearchScope::Video if st.has_more_videos && !st.is_searching_videos => {
                    Some(SearchType::Video)
                }
                SearchScope::Clip if st.has_more_clips && !st.is_searching_clips => {
                    Some(SearchType::Clip)
                }
                _ => None,
            };
            (t, st.active_search_id, ty)
        };
        if let Some(ty) = ty {
            self.fetch_bucket(ty, &t, false, token).await;
        }
        let mut st = self.lock();
        if st.active_search_id == token {
            st.save_snapshot();
        }
    }

    async fn run_bucket<T, Fut>(
        &self,
        ty: SearchType,
        reset: bool,
        token: u64,
        fetch: impl FnOnce(i64) -> Fut,
        apply: impl FnOnce(&mut SearchState, Vec<T>),
    ) where
        Fut: Future<Output = Result<Vec<T>, String>>,
    {
        let offset = {
            let mut st = self.lock();
            let (offset, more, searching) = bucket_fields(&mut st, ty);
            if reset {
                *offset = 0;
                *more = true;
            }
            *searching = true;
            let o = *offset;
            st.bucket_errors.remove(&ty);
            o
        };
        let result = fetch(offset).await;
        let mut st = self.lock();
        if st.active_search_id != token {
            return;
        }
        match result {
            Ok(data) => {
                let count = data.len() as i64;
                apply(&mut st, data);
                let (offset, more, _) = bucket_fields(&mut st, ty);
                *offset += count;
                *more = count >= PAGE_SIZE;
                st.loaded_buckets.insert(ty);
            }
            Err(e) => {
                tracing::error!(
                    "{} 검색 실패: {e}",
                    SearchScope::from_search_type(ty).title()
                );
                st.bucket_errors.insert(ty, e);
            }
        }
        *bucket_fields(&mut st, ty).2 = false;
    }

    pub(super) async fn fetch_channels(&self, keyword: &str, reset: bool, token: u64) {
        let first_page = reset || self.read().channel_offset == 0;
        let api = self.shared.api.clone();
        let kw = keyword.to_string();
        self.run_bucket(
            SearchType::Channel,
            reset,
            token,
            |offset| async move {
                api.search_channels(&kw, offset, PAGE_SIZE)
                    .await
                    .map(|r| r.data)
            },
            |st, data| {
                if st.channel_offset == 0 {
                    let mut first = data;
                    sort_channels(&mut first, keyword);
                    st.channel_results = first;
                } else {
                    let seen: HashSet<String> = st
                        .channel_results
                        .iter()
                        .map(|c| c.channel_id.clone())
                        .collect();
                    let mut page: Vec<ChannelInfo> = data
                        .into_iter()
                        .filter(|c| !seen.contains(&c.channel_id))
                        .collect();
                    sort_channels(&mut page, keyword);
                    st.channel_results.extend(page);
                }
            },
        )
        .await;
        if first_page {
            self.fetch_channel_lives(token).await;
        }
    }

    /// 채널 검색 첫 쪽에서 방송 중인 채널의 라이브를 받아 라이브 결과에 합칠 몫으로 둔다.
    /// 한 채널이 실패해도 나머지는 남기고, 라이브 버킷 오류로 치지 않는다.
    async fn fetch_channel_lives(&self, token: u64) {
        let targets: Vec<ChannelInfo> = {
            let st = self.read();
            if st.active_search_id != token || st.bucket_errors.contains_key(&SearchType::Channel) {
                return;
            }
            st.channel_results
                .iter()
                .filter(|c| c.open_live)
                .take(CHANNEL_LIVE_LIMIT)
                .cloned()
                .collect()
        };
        let api = self.shared.api.clone();
        let results = futures_util::future::join_all(targets.into_iter().map(|channel| {
            let api = api.clone();
            async move {
                let r = api.channel_live(&channel.channel_id).await;
                (channel, r)
            }
        }))
        .await;
        let lives: Vec<LiveInfo> = results
            .into_iter()
            .filter_map(|(channel, r)| match r {
                Ok(Some(mut live)) => {
                    if live.channel.is_none() {
                        live.channel = Some(channel);
                    }
                    Some(live)
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!("검색 채널 라이브 받기 실패({}): {e}", channel.channel_id);
                    None
                }
            })
            .collect();
        let mut st = self.lock();
        if st.active_search_id == token {
            st.channel_lives = lives;
        }
    }

    pub(super) async fn fetch_lives(&self, keyword: &str, reset: bool, token: u64) {
        let api = self.shared.api.clone();
        let kw = keyword.to_string();
        self.run_bucket(
            SearchType::Live,
            reset,
            token,
            |offset| async move {
                api.search_lives(&kw, offset, PAGE_SIZE)
                    .await
                    .map(|r| r.data)
            },
            |st, data| {
                if st.live_offset == 0 {
                    st.live_results = data;
                    st.sort_live_results();
                } else {
                    let seen: HashSet<i64> = st.live_results.iter().map(|l| l.live_id).collect();
                    let mut page: Vec<LiveInfo> = data
                        .into_iter()
                        .filter(|l| !seen.contains(&l.live_id))
                        .collect();
                    sort_lives(&mut page, st.live_sort_option);
                    st.live_results.extend(page);
                }
            },
        )
        .await;
    }

    pub(super) async fn fetch_videos(&self, keyword: &str, reset: bool, token: u64) {
        let api = self.shared.api.clone();
        let kw = keyword.to_string();
        self.run_bucket(
            SearchType::Video,
            reset,
            token,
            |offset| async move {
                api.search_videos(&kw, offset, PAGE_SIZE)
                    .await
                    .map(|r| r.data)
            },
            |st, data| {
                if st.video_offset == 0 {
                    st.video_results = data;
                } else {
                    st.video_results.extend(data);
                }
            },
        )
        .await;
    }

    pub(super) async fn fetch_clips(
        &self,
        keyword: &str,
        reset: bool,
        token: u64,
        channel_limit: usize,
    ) {
        {
            let mut st = self.lock();
            if reset {
                st.clip_page = 0;
                st.has_more_clips = true;
                st.clip_next_page.clear();
                st.clip_exhausted.clear();
            }
            st.is_searching_clips = true;
            st.bucket_errors.remove(&SearchType::Clip);
        }
        let result = self.fetch_clips_inner(keyword, token, channel_limit).await;
        let mut st = self.lock();
        if st.active_search_id != token {
            return;
        }
        if let Err(e) = result {
            tracing::error!("클립 검색 실패: {e}");
            st.bucket_errors.insert(SearchType::Clip, e);
        }
        st.is_searching_clips = false;
    }

    async fn fetch_clips_inner(
        &self,
        keyword: &str,
        token: u64,
        channel_limit: usize,
    ) -> Result<(), String> {
        // 클립 소스 채널 해석 — 같은 키워드면 재사용.
        let needs_sources = {
            let st = self.read();
            st.clip_source_keyword.as_deref() != Some(keyword)
                || st.clip_source_channel_ids.is_empty()
        };
        if needs_sources {
            let result = self
                .shared
                .api
                .search_channels(keyword, 0, PAGE_SIZE)
                .await?;
            let mut st = self.lock();
            if st.active_search_id != token {
                return Ok(());
            }
            // 관련도 순(이름이 같은 채널이 맨 앞) — 첫 물결은 맨 앞 채널만 받으므로 검색한 채널이 먼저 온다.
            let mut sources = result.data;
            sort_channels(&mut sources, keyword);
            sources.truncate(CLIP_SOURCE_CHANNEL_COUNT);
            let key = channel_name_key(keyword);
            st.clip_full_channel_ids = sources
                .iter()
                .filter(|c| channel_name_key(&c.channel_name) == key)
                .map(|c| c.channel_id.clone())
                .collect();
            st.clip_source_channel_ids = sources.into_iter().map(|c| c.channel_id).collect();
            st.clip_source_keyword = Some(keyword.to_string());
            st.clip_next_page.clear();
            st.clip_cursor.clear();
            st.clip_exhausted.clear();
        }
        let targets = self.read().clip_fetch_targets(channel_limit);
        let mut fetched: HashMap<String, ClipListPage> = HashMap::new();
        if !targets.is_empty() {
            let api = self.shared.api.clone();
            let futures = targets.iter().map(|(id, _, cursor)| {
                let api = api.clone();
                let id = id.clone();
                let cursor = cursor.clone();
                async move {
                    api.clip_list(&id, cursor.as_deref(), PAGE_SIZE)
                        .await
                        .map(|p| (id, p))
                }
            });
            for r in futures_util::future::join_all(futures).await {
                let (id, data) = r?;
                fetched.insert(id, data);
            }
            if self.read().active_search_id != token {
                return Ok(());
            }
        }
        self.lock().apply_clip_pages(&fetched, &targets, keyword);
        Ok(())
    }

    pub(super) async fn expand_clip_sources_if_needed(
        &self,
        scope: SearchScope,
        keyword: &str,
        token: u64,
    ) {
        {
            let st = self.read();
            if scope != SearchScope::Clip
                || st.is_searching_clips
                || !st.needs_clip_source_expansion(keyword)
            {
                return;
            }
        }
        self.fetch_clips(keyword, false, token, CLIP_SOURCE_CHANNEL_COUNT)
            .await;
        let mut st = self.lock();
        if st.active_search_id == token {
            st.save_snapshot();
        }
    }
}
