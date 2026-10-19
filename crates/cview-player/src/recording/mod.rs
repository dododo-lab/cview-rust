//! `Sources/CViewPlayer/StreamRecordingService.swift` 포팅 — HLS 조각 순차 다운로드 기반 녹화.
//!
//! 엔진 독립적: 재생목록을 주기적으로 폴링해 새 조각을 받아 출력 파일(.ts/fMP4)에 이어 쓴다.
//! AES-128-CBC(PKCS7) 암호 조각을 풀고, 조각 단위로 지수 백오프 재시도한다.
//! 한 폴의 조각 처리(순서 보존·`#EXT-X-MAP`·BYTERANGE)는 [`segments`] 에 있다.
//!
//! Swift actor → `Arc<Inner>` + `Mutex<State>`(await 너머로 잠그지 않는다). 폴링 루프는 tokio 태스크이고
//! `stop_recording` 이 abort 한다(Swift `Task.isCancelled` 대신).

mod segments;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use chrono::{DateTime, Local, Utc};
use cview_core::models::player::PlayerError;
use cview_networking::headers::{CHZZK_REFERER, SAFARI_USER_AGENT};
use tokio::task::JoinHandle;
use url::Url;

use crate::manifest::parser::{EncryptionInfo, EncryptionMethod, Segment};
use crate::manifest::HlsManifestParser;

/// 폴링 간격.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// 마스터 → variant 를 따라가는 최대 단계(순환 목록 방어).
const MAX_PLAYLIST_HOPS: usize = 4;

/// 녹화 상태.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RecordingState {
    #[default]
    Idle,
    Recording,
    Stopping,
    Error(String),
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) state: RecordingState,
    file: Option<File>,
    output: Option<PathBuf>,
    task: Option<JoinHandle<()>>,
    /// 처리한(기록했거나 끝내 포기한) 조각 키 — [`StreamRecordingService::recording_key`].
    pub(crate) downloaded: HashSet<String>,
    /// 마지막으로 쓴 `#EXT-X-MAP` 키 — 처음이거나 바뀌면 다음 조각 앞에 init 을 쓴다.
    pub(crate) last_written_init_key: Option<String>,
    /// 불연속 조각 앞에 init 을 다시 쓴 그 조각의 키(재시도에서 또 쓰지 않게).
    pub(crate) init_rewritten_before: Option<String>,
    /// 불연속 조각을 init 없이 건너뛰었다 — MAP 이 있는 다음 조각 앞에 init 을 다시 쓴다.
    pub(crate) pending_init_restart: bool,
    /// 순서를 지키려고 폴을 멈추게 한 조각과 연속 실패 폴 수.
    pub(crate) blocked: Option<(String, u32)>,
    total_bytes: i64,
    start_date: Option<DateTime<Utc>>,
    key_cache: HashMap<Url, Vec<u8>>,
}

pub(crate) struct Inner {
    pub(crate) client: reqwest::Client,
    auto_poll: bool,
    /// 조각 재시도의 첫 대기(이후 2배씩, 최대 64초).
    pub(crate) retry_base_delay: Duration,
    state: Mutex<State>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let s = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = s.task.take() {
            task.abort();
        }
    }
}

/// HLS 조각 순차 다운로드 녹화 서비스.
#[derive(Clone)]
pub struct StreamRecordingService {
    pub(crate) inner: Arc<Inner>,
}

impl Default for StreamRecordingService {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamRecordingService {
    /// 기본 HTTP 클라이언트(요청 10초·전체 30초, Safari UA·치지직 Referer).
    pub fn new() -> Self {
        Self::with_options(Self::default_client(), true, Duration::from_secs(1))
    }

    pub fn default_client() -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::REFERER,
            reqwest::header::HeaderValue::from_static(CHZZK_REFERER),
        );
        reqwest::Client::builder()
            .user_agent(SAFARI_USER_AGENT)
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    }

    /// `auto_poll` 이 false 면 `start_recording` 이 폴링 루프를 띄우지 않는다 — 호출자가 `poll_once` 로 몬다.
    pub fn with_options(
        client: reqwest::Client,
        auto_poll: bool,
        retry_base_delay: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                auto_poll,
                retry_base_delay,
                state: Mutex::new(State::default()),
            }),
        }
    }

    pub(crate) fn st(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn current_state(&self) -> RecordingState {
        self.st().state.clone()
    }

    pub fn recording_start_date(&self) -> Option<DateTime<Utc>> {
        self.st().start_date
    }

    pub fn is_recording(&self) -> bool {
        self.st().state == RecordingState::Recording
    }

    pub fn bytes_written(&self) -> i64 {
        self.st().total_bytes
    }

    pub fn output_path(&self) -> Option<PathBuf> {
        self.st().output.clone()
    }

    /// 녹화 시작 — 출력 디렉터리를 만들고 파일을 새로 연다. 이미 녹화 중이면 오류.
    pub fn start_recording(&self, playlist_url: &Url, output: &Path) -> Result<(), PlayerError> {
        let mut s = self.st();
        if s.state != RecordingState::Idle {
            return Err(PlayerError::RecordingFailed("이미 녹화 중입니다".into()));
        }
        if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
        }
        let file = File::create(output).map_err(|_| {
            PlayerError::RecordingFailed(format!(
                "출력 파일을 생성할 수 없습니다: {}",
                output.display()
            ))
        })?;
        s.file = Some(file);
        s.output = Some(output.to_path_buf());
        s.downloaded.clear();
        s.last_written_init_key = None;
        s.init_rewritten_before = None;
        s.pending_init_restart = false;
        s.blocked = None;
        s.total_bytes = 0;
        s.start_date = Some(Utc::now());
        s.state = RecordingState::Recording;
        tracing::info!(
            "녹화 시작: {}",
            output.file_name().unwrap_or_default().to_string_lossy()
        );
        if self.inner.auto_poll {
            let weak = Arc::downgrade(&self.inner);
            let url = playlist_url.clone();
            s.task = Some(tokio::spawn(polling_loop(weak, url)));
        }
        Ok(())
    }

    /// 녹화 중지 — 폴링을 멈추고 파일을 닫는다.
    pub fn stop_recording(&self) {
        let mut s = self.st();
        if s.state != RecordingState::Recording {
            return;
        }
        s.state = RecordingState::Stopping;
        if let Some(task) = s.task.take() {
            task.abort();
        }
        if let Some(mut file) = s.file.take() {
            let _ = file.flush();
        }
        tracing::info!(
            "녹화 중지: {}개 세그먼트, {} bytes 저장됨",
            s.downloaded.len(),
            s.total_bytes
        );
        s.state = RecordingState::Idle;
    }

    /// 재생목록을 받아 조각 목록을 돌려준다. 마스터면 첫(최고 대역폭) variant 를 따라간다.
    /// 상대 URI 는 리다이렉트를 따라간 **최종 응답 URL** 기준으로 푼다.
    pub(crate) async fn fetch_parsed_segments(
        &self,
        playlist_url: &Url,
    ) -> Result<Vec<Segment>, PlayerError> {
        let parser = HlsManifestParser::new();
        let mut url = playlist_url.clone();
        for _ in 0..MAX_PLAYLIST_HOPS {
            let resp = self
                .inner
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(PlayerError::RecordingFailed(format!(
                    "재생목록 HTTP {status}"
                )));
            }
            let final_url = resp.url().clone();
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
            let Ok(content) = std::str::from_utf8(&bytes) else {
                return Ok(Vec::new());
            };
            if content.contains("#EXT-X-STREAM-INF") {
                let master = parser
                    .parse_master_playlist(content, &final_url)
                    .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
                match master.variants.first() {
                    Some(v) => {
                        url = v.uri.clone();
                        continue;
                    }
                    None => return Ok(Vec::new()),
                }
            }
            let media = parser
                .parse_media_playlist(content, &final_url)
                .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
            return Ok(media.segments);
        }
        Err(PlayerError::RecordingFailed(
            "재생목록 단계가 너무 깊습니다".into(),
        ))
    }

    /// AES-128-CBC 조각 복호화(키는 URL 별로 캐시).
    pub(crate) async fn decrypt_segment(
        &self,
        data: &[u8],
        encryption: &EncryptionInfo,
    ) -> Result<Vec<u8>, PlayerError> {
        if encryption.method != EncryptionMethod::Aes128 {
            return Err(PlayerError::RecordingFailed(format!(
                "지원하지 않는 암호화 방식: {:?}",
                encryption.method
            )));
        }
        let key = self.fetch_encryption_key(&encryption.uri).await?;
        if key.len() != 16 {
            return Err(PlayerError::RecordingFailed(format!(
                "잘못된 AES 키 크기: {} bytes",
                key.len()
            )));
        }
        let Some(iv) = encryption.iv else {
            return Err(PlayerError::RecordingFailed(
                "IV가 없거나 크기가 잘못됨".into(),
            ));
        };
        aes128_cbc_decrypt(data, &key, &iv)
    }

    async fn fetch_encryption_key(&self, url: &Url) -> Result<Vec<u8>, PlayerError> {
        if let Some(cached) = self.st().key_cache.get(url) {
            return Ok(cached.clone());
        }
        let resp = self
            .inner
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
        let data = resp
            .bytes()
            .await
            .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?
            .to_vec();
        self.st().key_cache.insert(url.clone(), data.clone());
        Ok(data)
    }

    /// 출력 파일에 이어 쓴다. 디스크 오류는 `RecordingFailed` 로 돌려준다.
    pub(crate) fn write_data(&self, data: &[u8]) -> Result<(), PlayerError> {
        let mut s = self.st();
        let Some(file) = s.file.as_mut() else {
            return Err(PlayerError::RecordingFailed(
                "출력 파일 핸들이 없습니다".into(),
            ));
        };
        file.write_all(data)
            .map_err(|e| PlayerError::RecordingFailed(e.to_string()))?;
        s.total_bytes += data.len() as i64;
        Ok(())
    }

    /// 기본 녹화 저장 경로: `<movies_dir>/CView/<채널명>_<타임스탬프>.ts`.
    /// `movies_dir` 가 없으면 임시 디렉터리를 쓴다(macOS `~/Movies` 는 호스트가 넘긴다).
    pub fn default_recording_path(
        movies_dir: Option<&Path>,
        channel_name: &str,
        now: DateTime<Local>,
    ) -> PathBuf {
        let base = movies_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let sanitized: String = channel_name
            .chars()
            .map(|c| {
                let keep = c.is_ascii_alphanumeric()
                    || c == '_'
                    || c == '-'
                    || ('가'..='힣').contains(&c)
                    || ('ㄱ'..='ㅎ').contains(&c)
                    || ('ㅏ'..='ㅣ').contains(&c);
                if keep {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let timestamp = now.format("%Y-%m-%dT%H%M%S");
        base.join("CView")
            .join(format!("{sanitized}_{timestamp}.ts"))
    }
}

async fn polling_loop(weak: Weak<Inner>, playlist_url: Url) {
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let service = StreamRecordingService { inner };
        if !service.is_recording() {
            return;
        }
        service.poll_once(&playlist_url).await;
        drop(service);
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// AES-128-CBC 복호화 + PKCS7 패딩 제거.
pub fn aes128_cbc_decrypt(data: &[u8], key: &[u8], iv: &[u8; 16]) -> Result<Vec<u8>, PlayerError> {
    let key: &[u8; 16] = key
        .try_into()
        .map_err(|_| PlayerError::RecordingFailed("잘못된 AES 키 크기".into()))?;
    cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|_| PlayerError::RecordingFailed("AES 복호화 실패".into()))
}
