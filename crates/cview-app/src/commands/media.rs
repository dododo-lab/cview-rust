//! VOD 재생·이어 보기·나중에 보기 — 원본 VODPlayerView / VODStreamResolver / VODPlaybackProgressStore.
//!
//! 재생은 WebView `<video>` 의 기본 HLS 로 한다(CDN CORS 를 피하려 hls.js 대신). 여기서는 재생 주소·
//! 화질 목록·이어 볼 위치를 돌려주고, 진행 위치를 저장한다.

use cview_player::vod::VodStreamResolver;
use serde::Serialize;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::state::AppServices;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodQualityDto {
    pub name: String,
    pub resolution: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodPlayDto {
    pub video_no: i64,
    pub title: String,
    pub stream_url: String,
    pub duration: f64,
    pub qualities: Vec<VodQualityDto>,
    pub channel_id: Option<String>,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub read_count: i64,
    pub publish_date: Option<String>,
    /// 이어 볼 위치(초). 거의 끝까지 봤으면 0.
    pub resume_at: f64,
    pub watch_later: bool,
}

/// 이어 보기 위치 — 끝 30초 안이나 앞 10초 안이면 처음부터.
pub fn resume_position(last: f64, duration: f64) -> f64 {
    if !last.is_finite() || last < 10.0 {
        return 0.0;
    }
    if duration > 0.0 && last > duration - 30.0 {
        return 0.0;
    }
    last
}

#[tauri::command]
pub async fn vod_play(services: State<'_, AppServices>, video_no: i64) -> CmdResult<VodPlayDto> {
    let resolver = VodStreamResolver::new(services.api.clone());
    let info = resolver
        .resolve_vod(video_no)
        .await
        .map_err(|e| CommandError(format!("다시보기를 재생할 수 없습니다: {e}")))?;
    let duration = info.duration.as_secs_f64();
    let (resume_at, watch_later) = match &services.data_store {
        Some(store) => (
            store
                .fetch_vod_playback(video_no)
                .ok()
                .flatten()
                .map(|p| resume_position(p.last_position_sec, duration))
                .unwrap_or(0.0),
            store.contains_vod_watch_later(video_no).unwrap_or(false),
        ),
        None => (0.0, false),
    };
    Ok(VodPlayDto {
        video_no,
        title: info.title,
        stream_url: info.stream_url.to_string(),
        duration,
        qualities: info
            .qualities
            .iter()
            .map(|q| VodQualityDto {
                name: q.name.clone(),
                resolution: q.resolution.clone(),
                url: q.url.to_string(),
            })
            .collect(),
        channel_id: info.channel_id,
        channel_name: info.channel_name,
        thumbnail_url: info.thumbnail_url_string,
        read_count: info.read_count,
        publish_date: info.publish_date.map(|d| d.to_rfc3339()),
        resume_at,
        watch_later,
    })
}

/// 재생 위치 저장(이어 보기).
#[tauri::command]
pub async fn vod_progress_save(
    services: State<'_, AppServices>,
    video_no: i64,
    position: f64,
    duration: f64,
    title: String,
    channel_name: String,
    thumbnail_url: Option<String>,
) -> CmdResult<()> {
    if let Some(store) = &services.data_store {
        store
            .record_vod_playback(
                video_no,
                position.max(0.0),
                duration.max(0.0),
                &title,
                &channel_name,
                thumbnail_url.as_deref(),
            )
            .map_err(|e| CommandError(format!("저장소 오류: {e}")))?;
    }
    Ok(())
}

/// 이어 보기에서 지우기.
#[tauri::command]
pub async fn vod_progress_remove(services: State<'_, AppServices>, video_no: i64) -> CmdResult<()> {
    if let Some(store) = &services.data_store {
        store
            .remove_vod_playback(video_no)
            .map_err(|e| CommandError(format!("저장소 오류: {e}")))?;
    }
    Ok(())
}

/// 이어 보기·나중에 보기 한 줄.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodShelfItemDto {
    pub video_no: i64,
    pub title: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub duration: f64,
    /// 이어 보기면 본 위치(초), 나중에 보기면 0.
    pub position: f64,
}

#[tauri::command]
pub async fn vod_continue_list(
    services: State<'_, AppServices>,
    limit: Option<usize>,
) -> CmdResult<Vec<VodShelfItemDto>> {
    let Some(store) = &services.data_store else {
        return Ok(Vec::new());
    };
    Ok(store
        .fetch_recent_vod_playback(limit.unwrap_or(20).clamp(1, 100))
        .map_err(|e| CommandError(format!("저장소 오류: {e}")))?
        .into_iter()
        .map(|p| VodShelfItemDto {
            video_no: p.video_no,
            title: p.video_title,
            channel_name: p.channel_name,
            thumbnail_url: p.thumbnail_url_string,
            duration: p.duration_sec,
            position: p.last_position_sec,
        })
        .collect())
}

#[tauri::command]
pub async fn vod_watch_later_set(
    services: State<'_, AppServices>,
    video_no: i64,
    on: bool,
    title: Option<String>,
    channel_name: Option<String>,
    thumbnail_url: Option<String>,
    duration: Option<f64>,
) -> CmdResult<()> {
    let Some(store) = &services.data_store else {
        return Err(CommandError("저장소를 열 수 없습니다".into()));
    };
    let result = if on {
        store.add_vod_watch_later(
            video_no,
            title.as_deref().unwrap_or(""),
            channel_name.as_deref().unwrap_or(""),
            thumbnail_url.as_deref(),
            duration.unwrap_or(0.0),
        )
    } else {
        store.remove_vod_watch_later(video_no)
    };
    result.map_err(|e| CommandError(format!("저장소 오류: {e}")))
}

#[tauri::command]
pub async fn vod_watch_later_list(
    services: State<'_, AppServices>,
) -> CmdResult<Vec<VodShelfItemDto>> {
    let Some(store) = &services.data_store else {
        return Ok(Vec::new());
    };
    Ok(store
        .fetch_vod_watch_later(100)
        .map_err(|e| CommandError(format!("저장소 오류: {e}")))?
        .into_iter()
        .map(|p| VodShelfItemDto {
            video_no: p.video_no,
            title: p.video_title,
            channel_name: p.channel_name,
            thumbnail_url: p.thumbnail_url_string,
            duration: p.duration_sec,
            position: 0.0,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_skips_start_and_end() {
        assert_eq!(resume_position(5.0, 600.0), 0.0);
        assert_eq!(resume_position(580.0, 600.0), 0.0);
        assert_eq!(resume_position(300.0, 600.0), 300.0);
        assert_eq!(resume_position(f64::NAN, 600.0), 0.0);
        assert_eq!(resume_position(300.0, 0.0), 300.0);
    }
}
