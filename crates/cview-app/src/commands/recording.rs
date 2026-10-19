//! 녹화·이퀄라이저 명령 — 원본 HLSJSRecordingWriter / StreamRecordingService / EqualizerPreset (PLY-10, PLY-06).
//!
//! 녹화는 cview-player `StreamRecordingService` 가 재생 목록을 폴링해 조각을 받아 이어 쓴다.

use std::path::PathBuf;

use cview_core::models::equalizer::EqualizerPreset;
use serde::Serialize;
use tauri::State;

use super::content::{default_save_dir, sanitize_file_name};
use super::{CmdResult, CommandError};
use crate::state::AppServices;

/// 녹화 상태.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusDto {
    pub recording: bool,
    pub path: Option<String>,
    pub bytes: i64,
    /// 시작 시각(유닉스 밀리초).
    pub started_at: Option<i64>,
}

fn status(services: &AppServices) -> RecordingStatusDto {
    let r = &services.recorder;
    RecordingStatusDto {
        recording: r.is_recording(),
        path: r.output_path().map(|p| p.to_string_lossy().into_owned()),
        bytes: r.bytes_written(),
        started_at: r.recording_start_date().map(|d| d.timestamp_millis()),
    }
}

/// 녹화 파일 이름 — `채널_20261019_1903.ts`.
pub fn recording_file_name(channel: &str, now: chrono::DateTime<chrono::Local>) -> String {
    format!(
        "{}_{}.ts",
        sanitize_file_name(channel),
        now.format("%Y%m%d_%H%M%S")
    )
}

/// 지금 단독 재생 중인 방송 녹화 시작 — 원본 스트림의 조각을 받아 이어 쓴다(MPEG-TS/fMP4 그대로).
/// WKWebView 에는 `video.captureStream` 이 없어 페이지 쪽 MediaRecorder 는 쓸 수 없다.
#[tauri::command]
pub async fn player_record_start(
    services: State<'_, AppServices>,
) -> CmdResult<RecordingStatusDto> {
    let (url, name) = services
        .playing_stream
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| CommandError("녹화할 방송이 없습니다".into()))?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = default_save_dir(&home, "recording");
    let path = dir.join(recording_file_name(&name, chrono::Local::now()));
    services
        .recorder
        .start_recording(&url, &path)
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(status(&services))
}

/// 내용 첫 바이트로 컨테이너를 판정한다 — fMP4(`ftyp`/`styp`/`moof`) 면 `mp4`, MPEG-TS(0x47) 면 `ts`.
pub fn sniff_extension(head: &[u8]) -> Option<&'static str> {
    if head.len() >= 8 && matches!(&head[4..8], b"ftyp" | b"styp" | b"moof" | b"moov") {
        return Some("mp4");
    }
    if head.first() == Some(&0x47) {
        return Some("ts");
    }
    None
}

#[tauri::command]
pub async fn player_record_stop(services: State<'_, AppServices>) -> CmdResult<RecordingStatusDto> {
    let before = status(&services);
    services.recorder.stop_recording();
    let mut path = before.path.clone();
    // 치지직 LL-HLS 는 fMP4 라 .ts 로 두면 재생기가 못 연다 — 내용을 보고 확장자를 맞춘다.
    if let Some(p) = before.path.as_deref().map(PathBuf::from) {
        let mut head = [0u8; 12];
        let n = std::fs::File::open(&p)
            .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
            .unwrap_or(0);
        if let Some(ext) = sniff_extension(&head[..n]) {
            if p.extension().and_then(|e| e.to_str()) != Some(ext) {
                let renamed = p.with_extension(ext);
                if std::fs::rename(&p, &renamed).is_ok() {
                    path = Some(renamed.to_string_lossy().into_owned());
                }
            }
        }
    }
    Ok(RecordingStatusDto {
        recording: false,
        path,
        ..before
    })
}

#[tauri::command]
pub async fn player_record_status(
    services: State<'_, AppServices>,
) -> CmdResult<RecordingStatusDto> {
    Ok(status(&services))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqualizerPresetDto {
    pub name: String,
    pub gains: Vec<f32>,
    pub pre_amp: f32,
}

/// 이퀄라이저 프리셋·주파수.
#[tauri::command]
pub async fn equalizer_presets() -> CmdResult<(Vec<f32>, Vec<EqualizerPresetDto>)> {
    Ok((
        EqualizerPreset::FREQUENCIES.to_vec(),
        EqualizerPreset::ALL
            .iter()
            .map(|p| EqualizerPresetDto {
                name: p.name.to_string(),
                gains: p.gains.to_vec(),
                pre_amp: p.pre_amp,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn sniffs_container() {
        assert_eq!(sniff_extension(b"\0\0\0\x14ftypisom"), Some("mp4"));
        assert_eq!(sniff_extension(&[0x47, 0, 0, 0]), Some("ts"));
        assert_eq!(sniff_extension(b"abc"), None);
    }

    #[test]
    fn recording_names_are_safe() {
        let t = chrono::Local
            .with_ymd_and_hms(2026, 10, 19, 19, 3, 5)
            .unwrap();
        assert_eq!(recording_file_name("a/b:c", t), "a_b_c_20261019_190305.ts");
    }
}
