//! 라이브 목록·검색·채널/라이브 상세 명령.

use cview_app_core::caching::following_store::to_live_channel_item;
use cview_app_core::viewmodels::all_broadcasts::LoadState;
use cview_core::models::search::SearchType;
use cview_core::models::LiveChannelItem;
use cview_core::utils::following_poll::FollowingPollReason;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::dto::{
    following_live_cards, live_cards, ChannelDto, LiveCardDto, LiveDetailDto, SearchResultsDto,
};
use crate::state::AppServices;

/// 인기 라이브 기본 개수.
pub const POPULAR_DEFAULT_SIZE: i64 = 50;
/// 검색 결과 개수(채널·라이브 각각) 상한.
pub const SEARCH_PAGE_SIZE: usize = 24;

/// 검색어 정리 — 앞뒤 공백 제거, 비면 `None`.
pub fn normalize_keyword(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty()).then(|| k.chars().take(100).collect())
}

/// 팔로잉 중 방송 중인 채널. 로그인이 필요하다.
///
/// app-core `FollowingStore` 폴 — 화면 진입은 60초 안의 결과를 재사용하고, 새로고침은 수동 폴이다.
#[tauri::command]
pub async fn following_lives(
    services: State<'_, AppServices>,
    refresh: Option<bool>,
) -> CmdResult<Vec<LiveCardDto>> {
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인이 필요합니다".into()));
    }
    let reason = if refresh.unwrap_or(false) {
        FollowingPollReason::Manual {
            invalidate_thumbnails: false,
        }
    } else {
        FollowingPollReason::ScreenEntry
    };
    match services.following.poll(reason).await {
        Some(result) => {
            let items: Vec<LiveChannelItem> =
                result.channels.iter().map(to_live_channel_item).collect();
            Ok(following_live_cards(&items))
        }
        None => {
            let error = services
                .following_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            Err(match error {
                Some(e) => e.into(),
                None => CommandError("팔로잉 목록을 불러오지 못했습니다".into()),
            })
        }
    }
}

/// 인기 라이브 (시청자 순) — app-core `AllBroadcastsViewModel` 의 첫 페이지.
#[tauri::command]
pub async fn popular_lives(
    services: State<'_, AppServices>,
    size: Option<i64>,
    refresh: Option<bool>,
) -> CmdResult<Vec<LiveCardDto>> {
    let size = size.unwrap_or(POPULAR_DEFAULT_SIZE).clamp(1, 50) as usize;
    let vm = &services.popular;
    if refresh.unwrap_or(false) {
        vm.reload().await;
    } else {
        vm.load_initial_if_needed().await;
    }
    // 첫 페이지(30개)보다 많이 원하면 다음 커서를 더 받는다.
    while vm.read().feed.len() < size && vm.read().can_load_more {
        let before = vm.read().feed.len();
        vm.load_more().await;
        if vm.read().feed.len() == before {
            break;
        }
    }
    let s = vm.read();
    if let LoadState::Error(message) = &s.state {
        return Err(CommandError(message.clone()));
    }
    let take = s.feed.len().min(size);
    Ok(live_cards(&s.feed[..take]))
}

/// 채널 + 라이브 통합 검색 — app-core `SearchViewModel`(15초 결과 캐시·최근 검색어 포함).
#[tauri::command]
pub async fn search(
    services: State<'_, AppServices>,
    keyword: String,
) -> CmdResult<SearchResultsDto> {
    let Some(keyword) = normalize_keyword(&keyword) else {
        return Ok(SearchResultsDto {
            keyword: String::new(),
            channels: Vec::new(),
            lives: Vec::new(),
        });
    };
    let vm = &services.search;
    vm.set_query(keyword.clone());
    vm.perform_search(false).await;
    let st = vm.read();
    // 한쪽만 실패하면 다른 쪽 결과는 보여준다. 둘 다 실패하면 오류.
    let channel_error = st.bucket_errors.get(&SearchType::Channel);
    let live_error = st.bucket_errors.get(&SearchType::Live);
    if let (Some(e), Some(_)) = (channel_error, live_error) {
        return Err(CommandError(e.clone()));
    }
    Ok(SearchResultsDto {
        keyword,
        channels: st
            .channel_results
            .iter()
            .take(SEARCH_PAGE_SIZE)
            .map(ChannelDto::from)
            .collect(),
        lives: live_cards(&st.live_results[..st.live_results.len().min(SEARCH_PAGE_SIZE)]),
    })
}

#[tauri::command]
pub async fn channel_info(
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<ChannelDto> {
    let info = services.api.channel_info(&channel_id).await?;
    Ok(ChannelDto::from(&info))
}

#[tauri::command]
pub async fn live_detail(
    services: State<'_, AppServices>,
    channel_id: String,
    refresh: Option<bool>,
) -> CmdResult<LiveDetailDto> {
    let info = services
        .api
        .live_detail(&channel_id, refresh.unwrap_or(false))
        .await?;
    Ok(LiveDetailDto::from_live_info(&info, &channel_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_normalization() {
        assert_eq!(normalize_keyword("  "), None);
        assert_eq!(normalize_keyword("  풍월량 "), Some("풍월량".into()));
        assert_eq!(
            normalize_keyword(&"가".repeat(300))
                .unwrap()
                .chars()
                .count(),
            100
        );
    }
}
