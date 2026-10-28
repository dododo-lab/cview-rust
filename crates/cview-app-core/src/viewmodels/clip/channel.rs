//! 클립 브라우저 — 채널 탭과 채널 추천.

use cview_core::models::ChannelInfo;

use super::{
    channel_resolver, merging_channel_page, ClipBrowserViewModel, ClipTab, CHANNEL_FETCH_PAGE_SIZE,
    CHANNEL_SUGGESTION_DEBOUNCE, CHANNEL_SUGGESTION_SIZE, DISPLAY_PAGE_SIZE,
};

impl ClipBrowserViewModel {
    /// 입력 필드 submit — URL/ID 모두 처리.
    pub fn submit_channel_input(&self) {
        {
            let mut st = self.lock();
            match channel_resolver::resolve_channel_id(&st.channel_input) {
                Some(id) => st.resolved_channel_id = Some(id),
                None => {
                    st.channel_error =
                        Some("채널 ID 또는 채널 URL 형식이 올바르지 않습니다".into());
                    st.channel_clips.clear();
                    st.resolved_channel_id = None;
                    return;
                }
            }
        }
        self.load_channel_clips(true);
    }

    /// 채널 클립 한 페이지를 받는다 (백그라운드). 커서는 성공한 뒤에만 전진한다.
    pub fn load_channel_clips(&self, reset: bool) -> Option<tokio::task::JoinHandle<()>> {
        let (channel_id, token, page_to_fetch) = {
            let mut st = self.lock();
            let channel_id = st.resolved_channel_id.clone().filter(|c| !c.is_empty())?;
            if reset {
                st.channel_page = 0;
                st.channel_has_more = true;
                st.channel_clips.clear();
                st.channel_display_page = 0;
            }
            st.channel_request_token = st.channel_request_token.wrapping_add(1);
            let page = if reset { 0 } else { st.channel_page + 1 };
            (channel_id, st.channel_request_token, page)
        };
        let this = self.clone();
        Some(tokio::spawn(async move {
            this.run_channel_load(channel_id, token, page_to_fetch, reset)
                .await;
        }))
    }

    async fn run_channel_load(&self, channel_id: String, token: u64, page: i64, reset: bool) {
        let api = {
            let mut st = self.lock();
            st.channel_is_loading = true;
            st.channel_error = None;
            match st.api.clone() {
                Some(api) => api,
                None => {
                    if token == st.channel_request_token {
                        st.channel_error = Some("API 클라이언트가 초기화되지 않았습니다".into());
                        st.channel_is_loading = false;
                    }
                    return;
                }
            }
        };
        let result = api
            .clip_list(&channel_id, page, CHANNEL_FETCH_PAGE_SIZE)
            .await;
        let mut succeeded = false;
        let mut recent: Option<ChannelInfo> = None;
        {
            let mut st = self.lock();
            match result {
                Ok(result) => {
                    if token != st.channel_request_token
                        || st.resolved_channel_id.as_deref() != Some(channel_id.as_str())
                    {
                        return;
                    }
                    st.channel_page = page;
                    succeeded = true;
                    let existing = if reset {
                        Vec::new()
                    } else {
                        std::mem::take(&mut st.channel_clips)
                    };
                    st.channel_clips =
                        merging_channel_page(existing, &result.data, st.channel_sort_order);
                    st.channel_total_count = result.total_count;
                    st.channel_has_more = result.data.len() as i64 >= CHANNEL_FETCH_PAGE_SIZE;
                    if reset {
                        recent = result.data.first().and_then(|c| c.channel.clone());
                    }
                }
                Err(e) => {
                    if token != st.channel_request_token {
                        return;
                    }
                    st.channel_error = Some(format!("클립 로드 실패: {e}"));
                }
            }
        }
        if let Some(ch) = recent {
            self.add_recent_channel(ch);
        }
        let continue_fill = {
            let mut st = self.lock();
            if token != st.channel_request_token {
                return;
            }
            st.channel_is_loading = false;
            succeeded
                && st.channel_has_more
                && st.channel_clips.len() < (st.channel_display_page + 1) * DISPLAY_PAGE_SIZE
        };
        if continue_fill {
            self.load_more_channel_clips_if_needed();
        }
    }

    pub fn load_more_channel_clips_if_needed(&self) -> Option<tokio::task::JoinHandle<()>> {
        {
            let st = self.read();
            if !st.channel_has_more || st.channel_is_loading {
                return None;
            }
        }
        self.load_channel_clips(false)
    }

    /// 채널 표시 페이지 이동 — 데이터가 부족하면 즉시 이동 후 이어 받는다.
    pub fn go_to_channel_display_page(&self, page: usize) {
        let need_more = {
            let mut st = self.lock();
            if page >= st.channel_total_display_pages() {
                return;
            }
            let needed = (page + 1) * DISPLAY_PAGE_SIZE;
            st.channel_display_page = page;
            !(st.channel_clips.len() >= needed || !st.channel_has_more)
        };
        if need_more {
            self.load_more_channel_clips_if_needed();
        }
    }

    pub fn go_to_trending_display_page(&self, page: usize) {
        self.lock().go_to_trending_display_page(page);
    }

    /// 채널 ID 로 채널별 클립 탭을 연다.
    pub fn show_channel_clips(&self, channel_id: &str) {
        {
            let mut st = self.lock();
            st.channel_input = channel_id.to_string();
            st.resolved_channel_id = Some(channel_id.to_string());
            st.selected_tab = ClipTab::Channel;
        }
        self.load_channel_clips(true);
    }

    // MARK: Channel Suggestions

    /// 채널명 키워드를 디바운스 후 검색. URL/ID 면 후보를 지운다.
    pub async fn update_channel_suggestions(&self, keyword: &str) {
        let trimmed = keyword.trim().to_string();
        let token = {
            let mut st = self.lock();
            st.suggestion_token = st.suggestion_token.wrapping_add(1);
            let token = st.suggestion_token;
            let resolved = channel_resolver::resolve_channel_id(&trimmed).is_some();
            let short_ascii = trimmed.is_ascii() && trimmed.chars().count() < 2;
            if trimmed.is_empty() || resolved || short_ascii {
                st.channel_suggestions.clear();
                st.is_searching_channels = false;
                return;
            }
            token
        };
        tokio::time::sleep(CHANNEL_SUGGESTION_DEBOUNCE).await;
        let api = {
            let mut st = self.lock();
            if token != st.suggestion_token {
                return;
            }
            let Some(api) = st.api.clone() else { return };
            st.is_searching_channels = true;
            api
        };
        let result = api
            .search_channels(&trimmed, 0, CHANNEL_SUGGESTION_SIZE)
            .await;
        let mut st = self.lock();
        if token != st.suggestion_token {
            return;
        }
        st.channel_suggestions = result.unwrap_or_default();
        st.is_searching_channels = false;
    }

    /// 후보/팔로잉에서 고른 채널로 채널별 클립을 로드.
    pub fn select_channel_suggestion(&self, channel_id: &str, channel_name: &str) {
        {
            let mut st = self.lock();
            st.channel_input = channel_name.to_string();
            st.resolved_channel_id = Some(channel_id.to_string());
            st.channel_suggestions.clear();
        }
        self.load_channel_clips(true);
    }
}
