//! 녹화·이퀄라이저 명령 — 원본 HLSJSRecordingWriter / StreamRecordingService / EqualizerPreset (PLY-10, PLY-06).
//!
//! 녹화는 cview-player `StreamRecordingService` 가 재생 목록을 폴링해 조각을 받아 이어 쓴다.

use std::path::PathBuf;

use cview_core::models::equalizer::EqualizerPreset;
use serde::Serialize;
use tauri::State;

use super::content::{app_save_dir, sanitize_file_name};
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
    /// 쓰기 오류(디스크 가득 등)로 녹화가 멈췄으면 그 이유.
    pub error: Option<String>,
}

fn status(services: &AppServices) -> RecordingStatusDto {
    let r = &services.recorder;
    RecordingStatusDto {
        recording: r.is_recording(),
        path: r.output_path().map(|p| p.to_string_lossy().into_owned()),
        bytes: r.bytes_written(),
        started_at: r.recording_start_date().map(|d| d.timestamp_millis()),
        error: r.failure_reason(),
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
    app: tauri::AppHandle,
    window: tauri::Window,
    services: State<'_, AppServices>,
) -> CmdResult<RecordingStatusDto> {
    // 재생을 다른 창(플레이어 창)에 넘겨준 창에서 누르면 그 창의 방송을 녹화했다 — 지금 재생 대상 창에서만 받는다.
    if window.label() != crate::windows::player_target() {
        return Err(CommandError(
            "이 창의 방송은 지금 재생 중이 아닙니다 — 재생 중인 창에서 녹화해 주세요".into(),
        ));
    }
    let (url, name, encrypted) = services
        .playing_stream
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| CommandError("녹화할 방송이 없습니다".into()))?;
    // 인증 키가 필요한 암호화 방송은 녹화기가 키를 받지 못해(로그인 쿠키 없음) 모든 조각을 건너뛰고 빈 파일로 끝났다 —
    // '녹화 중'으로 보이다가 '녹화된 내용이 없습니다'만 남지 않게 시작 전에 알린다.
    if encrypted {
        return Err(CommandError(
            "이 방송은 암호화되어 있어 녹화할 수 없습니다".into(),
        ));
    }
    let dir = app_save_dir(&app, "recording");
    let path = unique_recording_path(&dir.join(recording_file_name(&name, chrono::Local::now())));
    services
        .recorder
        .start_recording(&url, &path)
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(status(&services))
}

/// 같은 이름(`.ts`·`.mp4` 어느 쪽이든)이 있으면 ` (2)` · ` (3)` 을 붙인다 — 녹화 이름은 초 단위라, 같은 초에 멈췄다 다시 시작하면
/// 새 녹화가 앞 파일을 처음부터 덮어썼다(`File::create`). 순수 함수에 가깝다(파일이 있는지만 본다).
pub fn unique_recording_path(p: &std::path::Path) -> PathBuf {
    let taken = |c: &std::path::Path| {
        c.exists() || c.with_extension("mp4").exists() || c.with_extension("ts").exists()
    };
    if !taken(p) {
        return p.to_path_buf();
    }
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("recording")
        .to_string();
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("ts")
        .to_string();
    (2..1000)
        .map(|i| p.with_file_name(format!("{stem} ({i}).{ext}")))
        .find(|c| !taken(c))
        .unwrap_or_else(|| p.to_path_buf())
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

/// 녹화를 끝내고 내용에 맞게 확장자를 바로잡는다 — 중지 단추뿐 아니라 시청 화면 닫기·다른 방송 열기·
/// 앱 종료(재생 정지)도 이 길로 끝낸다. 예전엔 중지 단추만 확장자를 맞춰, 그 밖의 길로 끝난 치지직
/// LL-HLS(fMP4) 녹화는 `.ts` 로 남아 재생기가 열지 못했다. 녹화 중이 아니면 아무것도 하지 않는다.
/// 쓰기 오류로 멈춘 녹화도 여기서 마무리한다(오류를 거두고, 거기까지 쓴 파일의 확장자를 맞춘다).
pub(crate) fn finish_recording(services: &AppServices) -> RecordingStatusDto {
    let before = status(services);
    if !before.recording && before.error.is_none() {
        return RecordingStatusDto {
            path: None,
            ..before
        };
    }
    services.recorder.stop_recording();
    let path = before
        .path
        .as_deref()
        .and_then(|p| finalize_recording_file(&PathBuf::from(p)))
        .map(|p| p.to_string_lossy().into_owned());
    RecordingStatusDto {
        recording: false,
        path,
        ..before
    }
}

/// 끝난 녹화 파일 마무리 — 아무것도 쓰지 못한(0바이트) 파일은 지우고 `None`, 아니면 확장자를 맞춘 경로.
/// 예전엔 화면은 '녹화된 내용이 없습니다'라고 하면서 빈 `.ts` 파일이 녹화 폴더에 쌓였다.
fn finalize_recording_file(p: &std::path::Path) -> Option<PathBuf> {
    if std::fs::metadata(p).is_ok_and(|m| m.len() == 0) {
        let _ = std::fs::remove_file(p);
        return None;
    }
    Some(fix_recording_extension(p))
}

/// 치지직 LL-HLS 는 fMP4 라 .ts 로 두면 재생기가 못 연다 — 내용을 보고 확장자를 맞춘다. 바꾼(또는 그대로인) 경로.
pub fn fix_recording_extension(p: &std::path::Path) -> PathBuf {
    let mut head = [0u8; 12];
    let n = std::fs::File::open(p)
        .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
        .unwrap_or(0);
    if let Some(ext) = sniff_extension(&head[..n]) {
        if p.extension().and_then(|e| e.to_str()) != Some(ext) {
            let renamed = p.with_extension(ext);
            // 그 이름이 이미 있으면 덮지 않는다(rename 은 말없이 바꿔치기한다) — 앞 녹화가 사라졌다.
            if !renamed.exists() && std::fs::rename(p, &renamed).is_ok() {
                return renamed;
            }
        }
    }
    p.to_path_buf()
}

#[tauri::command]
pub async fn player_record_stop(services: State<'_, AppServices>) -> CmdResult<RecordingStatusDto> {
    Ok(finish_recording(&services))
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

    /// 같은 초에 다시 시작해도 앞 녹화(.ts 든 확장자를 바꾼 .mp4 든)를 덮지 않는다.
    #[test]
    fn a_same_second_restart_gets_a_new_file_name() {
        let dir = std::env::temp_dir().join(format!("cview-rec-name-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("채널_20261128_120000.ts");
        assert_eq!(super::unique_recording_path(&p), p);
        std::fs::write(p.with_extension("mp4"), b"x").unwrap();
        let second = super::unique_recording_path(&p);
        assert_eq!(second, dir.join("채널_20261128_120000 (2).ts"));
        std::fs::write(&second, b"y").unwrap();
        assert_eq!(
            super::unique_recording_path(&p),
            dir.join("채널_20261128_120000 (3).ts")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    use chrono::TimeZone;

    #[test]
    fn empty_recording_file_is_removed_and_reports_no_path() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("빈 녹화.ts");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(finalize_recording_file(&empty), None);
        assert!(!empty.exists(), "빈 녹화 파일이 남았다");
        let full = dir.path().join("녹화.ts");
        std::fs::write(&full, [0x47u8; 376]).unwrap();
        assert_eq!(finalize_recording_file(&full), Some(full.clone()));
        assert!(full.exists());
    }

    #[test]
    fn fmp4_recording_gets_mp4_extension_and_ts_stays() {
        let dir = std::env::temp_dir().join(format!("cview-rec-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fmp4 = dir.join("a.ts");
        std::fs::write(&fmp4, b"\0\0\0\x18ftypiso6rest").unwrap();
        let fixed = fix_recording_extension(&fmp4);
        assert_eq!(fixed, dir.join("a.mp4"));
        assert!(fixed.exists() && !fmp4.exists());
        let ts = dir.join("b.ts");
        std::fs::write(&ts, [0x47u8, 0, 0, 0]).unwrap();
        assert_eq!(fix_recording_extension(&ts), ts);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
