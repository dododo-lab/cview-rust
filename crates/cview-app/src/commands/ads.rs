//! 치지직 광고·재생 조건 명령 — 치트키가 없는 시청자에게 치지직 웹과 같은 프리롤 광고를 보여 주고,
//! Windows 의 네이버 라이브 스트리밍 커넥터 상태를 알려 준다.
//!
//! 프리롤을 보일지는 치지직 웹 플레이어와 같은 순서로 정한다.
//! 1. 채널 `my-info` 의 `cheatKey` 가 참이면(치트키 구매) 광고 없음.
//! 2. 라이브 상세의 `skipPreRollAd`·`liveConnecting` 이면 광고 없음.
//! 3. 광고 노출 API(`seoraksan`)가 프리롤을 끄면 광고 없음 — 서버가 계정 혜택을 함께 본다.
//! 4. 네이버 GFP 일정·워터폴에서 재생할 광고 후보를 받는다. 재생과 추적은 화면(`ads.js`)이 VAST 로 맡는다.

use cview_networking::client_ads::{AdCandidate, AdContentKind};
use serde::Serialize;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::live_connector::{self, ConnectorStatus};
use crate::state::AppServices;

/// 치트키 구매 안내(네이버 게임 프로필의 치트키 탭).
pub const CHEAT_KEY_URL: &str = "https://game.naver.com/profile#cheat_key";

/// 프리롤 광고 계획.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdPrerollDto {
    /// 광고 자리마다 워터폴 후보(앞에서부터 재생 가능한 하나를 튼다).
    pub slots: Vec<Vec<AdCandidate>>,
    pub cheat_key_url: &'static str,
}

fn parse_kind(kind: &str) -> Result<AdContentKind, CommandError> {
    match kind {
        "live" => Ok(AdContentKind::Live),
        "vod" => Ok(AdContentKind::Vod),
        _ => Err(CommandError(format!("알 수 없는 광고 대상: {kind}"))),
    }
}

/// 재생 전 프리롤 — 보일 광고가 없으면 `None`(광고 서버 오류도 재생을 막지 않도록 `None`).
#[tauri::command]
pub async fn ad_preroll(
    services: State<'_, AppServices>,
    kind: String,
    id: String,
) -> CmdResult<Option<AdPrerollDto>> {
    let kind = parse_kind(&kind)?;
    let api = &services.api;
    // 셋 다 서로 기다릴 필요가 없다 — 노출 여부의 pgId 는 라이브면 채널 ID, 다시보기면 videoNo(= id).
    let (ctx, flags, viewer) = tokio::join!(
        api.ad_request_context(kind, &id),
        api.ad_display_flags(kind, &id),
        async {
            match kind {
                AdContentKind::Live => api.channel_viewer_info(&id).await.ok(),
                AdContentKind::Vod => None,
            }
        }
    );
    let ctx = match ctx {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::debug!("광고: 콘텐츠 정보를 받지 못함 ({e})");
            return Ok(None);
        }
    };
    if ctx.skip_pre_roll {
        return Ok(None);
    }
    // 다시보기는 채널을 상세에서 알게 되므로 치트키를 그 뒤에 본다.
    let viewer = match (viewer, kind) {
        (Some(v), _) => Some(v),
        (None, AdContentKind::Vod) if !ctx.channel_id.is_empty() => {
            api.channel_viewer_info(&ctx.channel_id).await.ok()
        }
        _ => None,
    };
    if viewer.is_some_and(|v| v.cheat_key) {
        return Ok(None);
    }
    match flags {
        Ok(flags) if flags.pre_roll => {}
        Ok(_) => return Ok(None),
        Err(e) => {
            tracing::debug!("광고: 노출 여부를 받지 못함 ({e})");
            return Ok(None);
        }
    }
    let slots = match api.preroll_ads(&ctx).await {
        Ok(slots) => slots,
        Err(e) => {
            tracing::debug!("광고: 일정을 받지 못함 ({e})");
            return Ok(None);
        }
    };
    if slots.is_empty() {
        return Ok(None);
    }
    Ok(Some(AdPrerollDto {
        slots,
        cheat_key_url: CHEAT_KEY_URL,
    }))
}

/// VAST 래퍼·구글 태그 본문.
#[tauri::command]
pub async fn ad_fetch_vast(services: State<'_, AppServices>, url: String) -> CmdResult<String> {
    Ok(services.api.ad_fetch_vast(&url).await?)
}

/// 광고 추적 URL 들을 보낸다(기다리지 않는다).
#[tauri::command]
pub fn ad_track(services: State<'_, AppServices>, urls: Vec<String>) {
    for url in urls.into_iter().take(32) {
        let api = services.api.clone();
        tauri::async_runtime::spawn(async move { api.ad_ping(&url).await });
    }
}

/// 네이버 라이브 스트리밍 커넥터 상태(Windows 1080p 조건).
#[tauri::command]
pub fn live_connector_status() -> ConnectorStatus {
    live_connector::status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(parse_kind("live"), Ok(AdContentKind::Live));
        assert_eq!(parse_kind("vod"), Ok(AdContentKind::Vod));
        assert!(parse_kind("clip").is_err());
    }
}
