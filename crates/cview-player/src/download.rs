//! VOD·클립 다운로더 — 원본 `VideoDownloader`(HLS 재생목록 파싱·AES-128 세그먼트 복호화·직접 다운로드·부분 파일).
//!
//! 단일 MP4 주소가 있으면 그대로 받아 쓰고, 없으면 HLS 미디어 재생목록의 세그먼트를 차례로 받아 한 파일로
//! 잇는다(`#EXT-X-MAP` init 은 바뀔 때만 한 번, AES-128 은 복호화). 받는 동안은 `<이름>.part` 에 쓰고
//! 끝나면 이름을 바꾼다. 취소·실패 시 부분 파일을 지운다.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use url::Url;

use crate::manifest::parser::{ByteRange, EncryptionMethod, HlsManifestParser, Segment};
use crate::recording::aes128_cbc_decrypt;

/// 다운로드 실패.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    #[error("취소했습니다")]
    Cancelled,
    #[error("재생 가능한 세그먼트를 찾지 못했습니다")]
    NoSegments,
    #[error("세그먼트 복호화에 실패했습니다")]
    DecryptFailed,
    #[error("파일 저장에 실패했습니다: {0}")]
    WriteFailed(String),
    #[error("서버 응답 오류 ({0})")]
    BadResponse(u16),
    #[error("네트워크 오류: {0}")]
    Network(String),
}

/// 진행 콜백 — (0~1 진행률, 지금까지 쓴 바이트).
pub type ProgressFn = Arc<dyn Fn(f64, u64) + Send + Sync>;

/// 받는 중 파일 이름(`a.mp4` → `a.mp4.part`).
pub fn part_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// IV 가 없으면 미디어 시퀀스 번호를 16바이트 빅엔디언으로 쓴다(HLS 규칙).
pub fn sequence_iv(sequence: i64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&(sequence as u64).to_be_bytes());
    iv
}

/// 바이트 범위를 `Range` 헤더 값으로. offset 이 없으면 같은 URI 의 직전 끝에서 잇는다.
pub fn range_header(range: &ByteRange, previous_end: Option<i64>) -> (String, i64) {
    let start = range.offset.or(previous_end).unwrap_or(0);
    let end = start + range.length.max(1) - 1;
    (format!("bytes={start}-{end}"), end + 1)
}

/// 하위 주소에 쿼리가 없고 같은 호스트면 부모 재생목록의 쿼리(서명)를 물려준다. 순수 함수.
pub fn inherit_query(child: &Url, parent: &Url) -> Url {
    if child.query().is_some() || parent.query().is_none() || child.host_str() != parent.host_str()
    {
        return child.clone();
    }
    let mut out = child.clone();
    out.set_query(parent.query());
    out
}

pub struct VideoDownloader {
    client: reqwest::Client,
    cancel: Arc<AtomicBool>,
}

impl VideoDownloader {
    pub fn new(cancel: Arc<AtomicBool>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self { client, cancel }
    }

    fn check_cancel(&self) -> Result<(), DownloadError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(DownloadError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// 단일 파일(MP4)을 받는다. 크기를 알면 진행률을 낸다.
    pub async fn download_file(
        &self,
        url: &Url,
        output: &Path,
        progress: ProgressFn,
    ) -> Result<u64, DownloadError> {
        self.finish(output, async |file: &mut std::fs::File| {
            let resp = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(DownloadError::BadResponse(status));
            }
            let total = resp.content_length().unwrap_or(0);
            let mut written = 0u64;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                self.check_cancel()?;
                let chunk = chunk.map_err(|e| DownloadError::Network(e.to_string()))?;
                file.write_all(&chunk)
                    .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
                written += chunk.len() as u64;
                if total > 0 {
                    progress(written as f64 / total as f64, written);
                }
            }
            Ok(written)
        })
        .await
    }

    /// HLS 재생목록(마스터면 가장 높은 화질)을 받아 세그먼트를 이어 쓴다.
    pub async fn download_hls(
        &self,
        playlist: &Url,
        output: &Path,
        progress: ProgressFn,
    ) -> Result<u64, DownloadError> {
        let segments = self.media_segments(playlist).await?;
        if segments.is_empty() {
            return Err(DownloadError::NoSegments);
        }
        self.finish(output, async |file: &mut std::fs::File| {
            let n = segments.len();
            let mut keys: HashMap<Url, Vec<u8>> = HashMap::new();
            let mut last_init: Option<Url> = None;
            let mut range_ends: HashMap<Url, i64> = HashMap::new();
            let mut written = 0u64;
            for (i, seg) in segments.iter().enumerate() {
                self.check_cancel()?;
                if seg.is_gap {
                    continue;
                }
                if let Some(init) = &seg.init_segment {
                    if last_init.as_ref() != Some(&init.uri) {
                        let data = self
                            .fetch(&init.uri, init.byte_range.as_ref(), None)
                            .await?
                            .0;
                        file.write_all(&data)
                            .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
                        written += data.len() as u64;
                        last_init = Some(init.uri.clone());
                    }
                }
                let (mut data, end) = self
                    .fetch(
                        &seg.uri,
                        seg.byte_range.as_ref(),
                        range_ends.get(&seg.uri).copied(),
                    )
                    .await?;
                if let Some(end) = end {
                    range_ends.insert(seg.uri.clone(), end);
                }
                if let Some(enc) = &seg.encryption_info {
                    if enc.method != EncryptionMethod::Aes128 {
                        return Err(DownloadError::DecryptFailed);
                    }
                    let key = match keys.get(&enc.uri) {
                        Some(k) => k.clone(),
                        None => {
                            let k = self.fetch(&enc.uri, None, None).await?.0;
                            keys.insert(enc.uri.clone(), k.clone());
                            k
                        }
                    };
                    let iv = enc.iv.unwrap_or_else(|| sequence_iv(seg.id));
                    data = aes128_cbc_decrypt(&data, &key, &iv)
                        .map_err(|_| DownloadError::DecryptFailed)?;
                }
                file.write_all(&data)
                    .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
                written += data.len() as u64;
                progress((i + 1) as f64 / n as f64, written);
            }
            Ok(written)
        })
        .await
    }

    /// `.part` 에 쓰고, 성공하면 최종 이름으로 바꾼다. 실패·취소면 부분 파일을 지운다.
    async fn finish(
        &self,
        output: &Path,
        body: impl AsyncFnOnce(&mut std::fs::File) -> Result<u64, DownloadError>,
    ) -> Result<u64, DownloadError> {
        if let Some(dir) = output.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
        }
        let part = part_path(output);
        let mut file =
            std::fs::File::create(&part).map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
        let result = body(&mut file).await;
        let result = result.and_then(|n| {
            file.flush()
                .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
            drop(file);
            std::fs::rename(&part, output)
                .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
            Ok(n)
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result
    }

    async fn fetch(
        &self,
        url: &Url,
        range: Option<&ByteRange>,
        previous_end: Option<i64>,
    ) -> Result<(Vec<u8>, Option<i64>), DownloadError> {
        let mut last_err = DownloadError::Network(String::new());
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(600 * attempt)).await;
            }
            self.check_cancel()?;
            let mut req = self
                .client
                .get(url.clone())
                .timeout(Duration::from_secs(30));
            let mut next_end = None;
            if let Some(r) = range {
                let (h, end) = range_header(r, previous_end);
                req = req.header("Range", h);
                next_end = Some(end);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if !(200..300).contains(&status) {
                        last_err = DownloadError::BadResponse(status);
                        if status < 500 {
                            break;
                        }
                        continue;
                    }
                    match resp.bytes().await {
                        Ok(b) => return Ok((b.to_vec(), next_end)),
                        Err(e) => last_err = DownloadError::Network(e.to_string()),
                    }
                }
                Err(e) => last_err = DownloadError::Network(e.to_string()),
            }
        }
        Err(last_err)
    }

    async fn media_segments(&self, playlist: &Url) -> Result<Vec<Segment>, DownloadError> {
        let parser = HlsManifestParser::new();
        let mut url = playlist.clone();
        for _ in 0..4 {
            let resp = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(DownloadError::BadResponse(status));
            }
            let final_url = resp.url().clone();
            let text = resp
                .text()
                .await
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            if text.contains("#EXT-X-STREAM-INF") {
                let master = parser
                    .parse_master_playlist(&text, &final_url)
                    .map_err(|_| DownloadError::NoSegments)?;
                url = inherit_query(
                    &master
                        .variants
                        .first()
                        .ok_or(DownloadError::NoSegments)?
                        .uri,
                    &final_url,
                );
                continue;
            }
            let mut segments = parser
                .parse_media_playlist(&text, &final_url)
                .map(|m| m.segments)
                .map_err(|_| DownloadError::NoSegments)?;
            for seg in &mut segments {
                seg.uri = inherit_query(&seg.uri, &final_url);
                if let Some(init) = seg.init_segment.as_mut() {
                    init.uri = inherit_query(&init.uri, &final_url);
                }
            }
            return Ok(segments);
        }
        Err(DownloadError::NoSegments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_and_iv_rules() {
        assert_eq!(
            part_path(Path::new("/a/b.mp4")),
            PathBuf::from("/a/b.mp4.part")
        );
        let iv = sequence_iv(0x0102);
        assert_eq!(&iv[..14], &[0u8; 14]);
        assert_eq!(&iv[14..], &[1, 2]);
    }

    #[test]
    fn child_urls_inherit_signed_query() {
        let parent = Url::parse("https://cdn.example.com/hls/master.m3u8?_lsu_sa_=sig").unwrap();
        let child = Url::parse("https://cdn.example.com/hls/720.m3u8").unwrap();
        assert_eq!(
            inherit_query(&child, &parent).as_str(),
            "https://cdn.example.com/hls/720.m3u8?_lsu_sa_=sig"
        );
        let own = Url::parse("https://cdn.example.com/hls/a.ts?x=1").unwrap();
        assert_eq!(inherit_query(&own, &parent), own);
        let other = Url::parse("https://other.example.com/a.ts").unwrap();
        assert_eq!(inherit_query(&other, &parent), other);
    }

    #[test]
    fn byte_ranges_continue_from_previous_end() {
        let r = ByteRange {
            length: 100,
            offset: Some(10),
        };
        assert_eq!(range_header(&r, None), ("bytes=10-109".into(), 110));
        let r2 = ByteRange {
            length: 50,
            offset: None,
        };
        assert_eq!(range_header(&r2, Some(110)), ("bytes=110-159".into(), 160));
    }
}
