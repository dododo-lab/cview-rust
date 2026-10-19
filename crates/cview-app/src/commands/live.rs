//! 라이브 목록·검색·채널/라이브 상세 명령.

use tauri::State;

use super::{CmdResult, CommandError};
use crate::dto::{
    following_live_cards, live_cards, ChannelDto, LiveCardDto, LiveDetailDto, SearchResultsDto,
};
use crate::state::AppServices;

/// 인기 라이브 기본 개수.
pub const POPULAR_DEFAULT_SIZE: i64 = 50;
/// 팔로잉 라이브 상세 조회 동시 수.
pub const FOLLOWING_DETAIL_CONCURRENCY: usize = 6;
/// 검색 결과 개수(채널·라이브 각각).
pub const SEARCH_PAGE_SIZE: i64 = 24;

/// 검색어 정리 — 앞뒤 공백 제거, 비면 `None`.
pub fn normalize_keyword(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty()).then(|| k.chars().take(100).collect())
}

/// 팔로잉 중 방송 중인 채널. 로그인이 필요하다.
#[tauri::command]
pub async fn following_lives(
    services: State<'_, AppServices>,
    refresh: Option<bool>,
) -> CmdResult<Vec<LiveCardDto>> {
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인이 필요합니다".into()));
    }
    let items = services
        .api
        .fetch_following_channels(refresh.unwrap_or(false), FOLLOWING_DETAIL_CONCURRENCY)
        .await?;
    Ok(following_live_cards(&items))
}

/// 인기 라이브 (시청자 순).
#[tauri::command]
pub async fn popular_lives(
    services: State<'_, AppServices>,
    size: Option<i64>,
    refresh: Option<bool>,
) -> CmdResult<Vec<LiveCardDto>> {
    let size = size.unwrap_or(POPULAR_DEFAULT_SIZE).clamp(1, 50);
    let page = services
        .api
        .top_lives(size, None, None, refresh.unwrap_or(false))
        .await?;
    Ok(live_cards(&page.data))
}

/// 채널 + 라이브 통합 검색.
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
    let (channels, lives) = tokio::join!(
        services.api.search_channels(&keyword, 0, SEARCH_PAGE_SIZE),
        services.api.search_lives(&keyword, 0, SEARCH_PAGE_SIZE),
    );
    // 한쪽만 실패하면 다른 쪽 결과는 보여준다. 둘 다 실패하면 오류.
    if let (Err(e), Err(_)) = (&channels, &lives) {
        return Err(e.clone().into());
    }
    Ok(SearchResultsDto {
        keyword,
        channels: channels
            .map(|r| r.data.iter().map(ChannelDto::from).collect())
            .unwrap_or_default(),
        lives: lives.map(|r| live_cards(&r.data)).unwrap_or_default(),
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
