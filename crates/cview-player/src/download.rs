//! VOD·클립 다운로더 — 원본 `VideoDownloader`(HLS 재생목록 파싱·AES-128 세그먼트 복호화·직접 다운로드·부분 파일).
//!
//! 단일 MP4 주소가 있으면 그대로 받아 쓰고, 없으면 HLS 미디어 재생목록의 세그먼트를 차례로 받아 한 파일로
//! 잇는다(`#EXT-X-MAP` init 은 바뀔 때만 한 번, AES-128 은 복호화). 고유한 임시 파일에 쓴 뒤
//! 기존 파일을 덮어쓰지 않고 완성본으로 옮긴다. 취소·실패·태스크 중단 시 임시 파일을 지운다.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use url::Url;

use crate::manifest::byte_span::HlsByteSpan;
use crate::manifest::parser::{
    ByteRange, EncryptionMethod, HlsManifestParser, InitSegment, Segment,
};
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
    #[error("서버의 바이트 범위 응답이 올바르지 않습니다")]
    InvalidByteRange,
    #[error("네트워크 오류: {0}")]
    Network(String),
}

/// 진행 콜백 — (0~1 진행률, 지금까지 쓴 바이트).
pub type ProgressFn = Arc<dyn Fn(f64, u64) + Send + Sync>;

/// 기존 부분 파일 이름(호환용). 실제 다운로드는 고유한 임시 경로를 쓴다.
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
pub fn range_header(range: &ByteRange, previous_end: Option<i64>) -> Option<(String, i64)> {
    let span = HlsByteSpan::validated(range.length, range.offset.or(previous_end));
    let range = span.byte_range()?;
    Some((span.range_header()?, range.offset? + range.length))
}

/// 하위 주소에 쿼리가 없고 같은 출처면 부모 재생목록의 쿼리(서명)를 물려준다. 순수 함수.
pub fn inherit_query(child: &Url, parent: &Url) -> Url {
    if child.query().is_some() || parent.query().is_none() || child.origin() != parent.origin() {
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
            .read_timeout(Duration::from_secs(30))
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

    /// AtomicBool cancellation also interrupts a server that stops sending bytes.
    async fn cancellable<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, reqwest::Error>>,
    ) -> Result<T, DownloadError> {
        tokio::pin!(future);
        loop {
            self.check_cancel()?;
            tokio::select! {
                result = &mut future => {
                    self.check_cancel()?;
                    return result.map_err(|e| DownloadError::Network(e.to_string()));
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
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
                .cancellable(self.client.get(url.clone()).send())
                .await?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(DownloadError::BadResponse(status));
            }
            let total = resp.content_length().unwrap_or(0);
            let mut written = 0u64;
            let mut stream = resp.bytes_stream();
            loop {
                let chunk = self
                    .cancellable(async { stream.next().await.transpose() })
                    .await?;
                let Some(chunk) = chunk else { break };
                self.check_cancel()?;
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
            let mut last_init: Option<InitSegment> = None;
            let spans = HlsByteSpan::resolve(&segments, |url| url.as_str().to_owned());
            let mut written = 0u64;
            for (i, seg) in segments.iter().enumerate() {
                self.check_cancel()?;
                if seg.is_gap {
                    continue;
                }
                if let Some(init) = &seg.init_segment {
                    if last_init.as_ref() != Some(init) {
                        let span = HlsByteSpan::of_init(init);
                        if span == HlsByteSpan::Invalid {
                            return Err(DownloadError::InvalidByteRange);
                        }
                        let data = self
                            .fetch(&init.uri, span.byte_range().as_ref(), None)
                            .await?
                            .0;
                        file.write_all(&data)
                            .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
                        written += data.len() as u64;
                        last_init = Some(init.clone());
                    }
                }
                if spans[i] == HlsByteSpan::Invalid {
                    return Err(DownloadError::InvalidByteRange);
                }
                let (mut data, _) = self
                    .fetch(&seg.uri, spans[i].byte_range().as_ref(), None)
                    .await?;
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

    /// RAII cleans up temporary files even if the download task is aborted.
    async fn finish(
        &self,
        output: &Path,
        body: impl AsyncFnOnce(&mut std::fs::File) -> Result<u64, DownloadError>,
    ) -> Result<u64, DownloadError> {
        self.check_cancel()?;
        let dir = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
        let mut file = tempfile::Builder::new()
            .prefix(".cview-download-")
            .suffix(".part")
            .tempfile_in(dir)
            .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
        let n = body(file.as_file_mut()).await?;
        self.check_cancel()?;
        file.flush()
            .and_then(|()| file.as_file().sync_all())
            .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
        file.persist_noclobber(output)
            .map_err(|e| DownloadError::WriteFailed(e.error.to_string()))?;
        Ok(n)
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
                let (h, end) =
                    range_header(r, previous_end).ok_or(DownloadError::InvalidByteRange)?;
                req = req.header("Range", h);
                next_end = Some(end);
            }
            match self.cancellable(req.send()).await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if !(200..300).contains(&status) {
                        last_err = DownloadError::BadResponse(status);
                        if status < 500 {
                            break;
                        }
                        continue;
                    }
                    let content_range = resp
                        .headers()
                        .get(reqwest::header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    match self.cancellable(resp.bytes()).await {
                        Ok(b) => {
                            let absolute = range.map(|r| ByteRange {
                                length: r.length,
                                offset: r.offset.or(previous_end),
                            });
                            let data = HlsByteSpan::recordable_body(
                                Some(&b),
                                status,
                                absolute,
                                content_range.as_deref(),
                            )
                            .ok_or(DownloadError::InvalidByteRange)?;
                            return Ok((data.to_vec(), next_end));
                        }
                        Err(DownloadError::Cancelled) => return Err(DownloadError::Cancelled),
                        Err(e) => last_err = e,
                    }
                }
                Err(DownloadError::Cancelled) => return Err(DownloadError::Cancelled),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    async fn media_segments(&self, playlist: &Url) -> Result<Vec<Segment>, DownloadError> {
        let parser = HlsManifestParser::new();
        let mut url = playlist.clone();
        for _ in 0..4 {
            let resp = self
                .cancellable(self.client.get(url.clone()).send())
                .await?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(DownloadError::BadResponse(status));
            }
            let final_url = resp.url().clone();
            let text = self.cancellable(resp.text()).await?;
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
        for value in [
            "http://cdn.example.com/a.ts",
            "https://cdn.example.com:8443/a.ts",
        ] {
            let other_origin = Url::parse(value).unwrap();
            assert_eq!(inherit_query(&other_origin, &parent), other_origin);
        }
    }

    #[test]
    fn byte_ranges_continue_from_previous_end() {
        let r = ByteRange {
            length: 100,
            offset: Some(10),
        };
        assert_eq!(range_header(&r, None), Some(("bytes=10-109".into(), 110)));
        let r2 = ByteRange {
            length: 50,
            offset: None,
        };
        assert_eq!(
            range_header(&r2, Some(110)),
            Some(("bytes=110-159".into(), 160))
        );
    }

    #[test]
    fn malformed_ranges_are_rejected_without_overflow() {
        for (length, offset) in [
            (0, Some(0)),
            (-1, Some(0)),
            (1, Some(-1)),
            (1, Some(i64::MAX)),
            (1, None),
        ] {
            assert_eq!(range_header(&ByteRange { length, offset }, None), None);
        }
    }

    fn downloader(cancel: Arc<AtomicBool>) -> VideoDownloader {
        VideoDownloader {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            cancel,
        }
    }

    #[tokio::test]
    async fn completed_download_never_overwrites_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("video.mp4");
        std::fs::write(&output, b"original").unwrap();
        let result = downloader(Arc::new(AtomicBool::new(false)))
            .finish(&output, async |file| {
                file.write_all(b"replacement").unwrap();
                Ok(11)
            })
            .await;
        assert!(matches!(result, Err(DownloadError::WriteFailed(_))));
        assert_eq!(std::fs::read(&output).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn cancellation_before_commit_removes_partial_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("video.mp4");
        let cancel = Arc::new(AtomicBool::new(false));
        let d = downloader(cancel.clone());
        let result = d
            .finish(&output, async |file| {
                file.write_all(b"video").unwrap();
                cancel.store(true, Ordering::Relaxed);
                Ok(5)
            })
            .await;
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn aborted_task_removes_partial_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("video.mp4");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            downloader(Arc::new(AtomicBool::new(false)))
                .finish(&output, async |file| {
                    file.write_all(b"partial").unwrap();
                    tx.send(()).unwrap();
                    std::future::pending::<Result<u64, DownloadError>>().await
                })
                .await
        });
        rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    struct TestServer {
        url: Url,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server(response: impl Fn(&str) -> String + Send + 'static) -> TestServer {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let byte = stream.read_u8().await.unwrap();
                    request.push(byte);
                }
                let body = response(std::str::from_utf8(&request).unwrap());
                stream.write_all(body.as_bytes()).await.unwrap();
            }
        });
        TestServer { url, task }
    }

    fn response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn hls_slices_full_responses_and_updates_init_ranges_at_same_url() {
        let s = server(|request| {
            if request.starts_with("GET /playlist") {
                response("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"bundle\",BYTERANGE=\"2@0\"\n#EXTINF:1,\nsegment\n#EXT-X-MAP:URI=\"bundle\",BYTERANGE=\"2@2\"\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n")
            } else if request.starts_with("GET /bundle") {
                response("ABCD")
            } else {
                response("X")
            }
        }).await;
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("video.mp4");
        let bytes = downloader(Arc::new(AtomicBool::new(false)))
            .download_hls(
                &s.url.join("playlist").unwrap(),
                &output,
                Arc::new(|_, _| {}),
            )
            .await
            .unwrap();
        assert_eq!(bytes, 6);
        assert_eq!(std::fs::read(output).unwrap(), b"ABXCDX");
    }

    #[tokio::test]
    async fn mismatched_partial_response_is_rejected() {
        let s = server(|_| "HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 0-1/20\r\nConnection: close\r\n\r\nAB".into()).await;
        let d = downloader(Arc::new(AtomicBool::new(false)));
        let result = d
            .fetch(
                &s.url,
                Some(&ByteRange {
                    length: 2,
                    offset: Some(4),
                }),
                None,
            )
            .await;
        assert_eq!(result, Err(DownloadError::InvalidByteRange));
    }

    #[tokio::test]
    async fn cancelled_download_interrupts_stalled_request() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let s = TestServer {
            url: url.clone(),
            task: tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                tx.send(()).unwrap();
                std::future::pending::<()>().await;
            }),
        };
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("video.mp4");
        let cancel = Arc::new(AtomicBool::new(false));
        let d = downloader(cancel.clone());
        let task =
            tokio::spawn(async move { d.download_file(&url, &output, Arc::new(|_, _| {})).await });
        rx.await.unwrap();
        cancel.store(true, Ordering::Relaxed);
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        drop(s);
    }
}
