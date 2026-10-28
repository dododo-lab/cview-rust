//! 클립 브라우저 — 인기(Trending) 탭.

use cview_core::utils::refresh_schedule::{RefreshDemand, RefreshJobId};

use super::{
    persist_clips_batch, to_clip_info, ClipBrowserViewModel, ClipHubPageLifecycleEffects,
    CACHED_CLIP_FETCH_LIMIT,
};

impl ClipBrowserViewModel {
    /// 인기 클립 로드. `resets_page` 가 거짓이면(5분 자동 갱신) 보던 페이지를 지킨다.
    pub async fn load_trending_clips(&self, resets_page: bool) {
        let (api, token, filter, order, source) = {
            let mut st = self.lock();
            let Some(api) = st.api.clone() else {
                st.trending_error = Some("API 클라이언트가 초기화되지 않았습니다".into());
                return;
            };
            st.trending_request_token = st.trending_request_token.wrapping_add(1);
            st.trending_is_loading = true;
            st.trending_error = None;
            (
                api,
                st.trending_request_token,
                st.trending_filter,
                st.trending_order,
                st.trending_source(),
            )
        };
        let result = api
            .home_popular_clips(filter.api_value(), order.api_value())
            .await;
        match result {
            Ok(clips) => {
                let store = {
                    let mut st = self.lock();
                    if token != st.trending_request_token {
                        return;
                    }
                    if st.trending_clips != clips {
                        st.trending_clips = clips.clone();
                    }
                    let page = if resets_page {
                        0
                    } else {
                        st.trending_display_page
                            .min(st.trending_total_display_pages() - 1)
                    };
                    if st.trending_display_page != page {
                        st.trending_display_page = page;
                    }
                    st.data_store.clone()
                };
                if let Some(store) = store {
                    persist_clips_batch(store.as_ref(), &clips, &source);
                }
            }
            Err(e) => {
                let mut st = self.lock();
                if token != st.trending_request_token {
                    return;
                }
                st.trending_error = Some(format!("인기 클립 로드 실패: {e}"));
            }
        }
        let mut st = self.lock();
        if token == st.trending_request_token {
            st.trending_is_loading = false;
        }
    }

    /// 콜드 스타트 — fetch 전 DB 캐시에서 인기 클립 복원.
    pub fn hydrate_from_cache(&self) {
        let (store, source) = {
            let st = self.read();
            let Some(store) = st.data_store.clone() else {
                return;
            };
            (store, st.trending_source())
        };
        let Ok(cached) = store.fetch_cached_clips(&source, CACHED_CLIP_FETCH_LIMIT) else {
            return;
        };
        let mut st = self.lock();
        if !cached.is_empty() && st.trending_clips.is_empty() {
            st.trending_clips = cached.iter().map(to_clip_info).collect();
        }
    }

    /// 클립 탭 진입 — `.clips` 수요를 잡는다. 멱등.
    pub fn start_trending_auto_refresh(&self) {
        let coordinator = {
            let mut st = self.lock();
            if st.is_trending_auto_refresh_active {
                return;
            }
            st.is_trending_auto_refresh_active = true;
            st.refresh_coordinator.clone()
        };
        if let Some(c) = coordinator {
            c.retain_demand(RefreshDemand::Clips, self.shared.instance);
            c.note_job_completed(RefreshJobId::ClipTrending, true);
        }
    }

    pub fn stop_trending_auto_refresh(&self) {
        let coordinator = {
            let mut st = self.lock();
            if !st.is_trending_auto_refresh_active {
                return;
            }
            st.is_trending_auto_refresh_active = false;
            st.refresh_coordinator.clone()
        };
        if let Some(c) = coordinator {
            c.release_demand(RefreshDemand::Clips, self.shared.instance);
        }
    }

    /// 숨은 허브 탭에서 폴링과 재생이 남지 않도록 활성 상태를 동기화한다.
    pub fn sync_hub_page_activity(&self, is_active: bool) {
        let effects = ClipHubPageLifecycleEffects::resolve(is_active);
        if effects.maintains_auto_refresh {
            self.start_trending_auto_refresh();
        } else {
            self.stop_trending_auto_refresh();
        }
        if effects.dismisses_playback {
            self.lock().selected_clip = None;
        }
    }
}
