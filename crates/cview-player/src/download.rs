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

use crate::manifest::byte_span::HlsByteSpan;
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

    /// 본문 다음 조각 — 기다리는 동안에도 취소를 보고, 오래 멈추면 실패로 끝낸다. 예전엔 조각이 올 때만 취소를 봐서,
    /// 연결이 본문 도중 멈추면(네트워크 전환) 취소가 듣지 않고 '받는 중'으로 남았고 같은 영상을 다시 받을 수도 없었다.
    async fn next_chunk<S, T>(&self, stream: &mut S) -> Result<Option<T>, DownloadError>
    where
        S: futures_util::Stream<Item = T> + Unpin,
    {
        let mut idle = Duration::ZERO;
        loop {
            self.check_cancel()?;
            match tokio::time::timeout(Self::CANCEL_POLL, stream.next()).await {
                Ok(item) => return Ok(item),
                Err(_) => {
                    idle += Self::CANCEL_POLL;
                    if idle >= Self::STALL_LIMIT {
                        return Err(DownloadError::Network(
                            "응답이 멈췄습니다(네트워크를 확인해 주세요)".into(),
                        ));
                    }
                }
            }
        }
    }

    /// 응답 머리를 기다리는 동안에도 취소·멈춤을 본다 — 예전엔 `send()` 에 시간 제한이 없어, 요청 직후 네트워크가 바뀌면
    /// 본문을 받기 전이라 취소가 듣지 않고 '받는 중'(다시 받으면 '이미 받는 중')으로 남았다.
    async fn until_ready<F: std::future::Future>(
        &self,
        fut: F,
    ) -> Result<F::Output, DownloadError> {
        tokio::pin!(fut);
        let mut idle = Duration::ZERO;
        loop {
            self.check_cancel()?;
            match tokio::time::timeout(Self::CANCEL_POLL, &mut fut).await {
                Ok(v) => return Ok(v),
                Err(_) => {
                    idle += Self::CANCEL_POLL;
                    if idle >= Self::STALL_LIMIT {
                        return Err(DownloadError::Network(
                            "응답이 멈췄습니다(네트워크를 확인해 주세요)".into(),
                        ));
                    }
                }
            }
        }
    }

    const CANCEL_POLL: Duration = Duration::from_millis(250);
    const STALL_LIMIT: Duration = Duration::from_secs(30);

    /// 단일 파일(MP4)을 받는다. 크기를 알면 진행률을 낸다.
    pub async fn download_file(
        &self,
        url: &Url,
        output: &Path,
        progress: ProgressFn,
    ) -> Result<u64, DownloadError> {
        self.finish(output, async |file: &mut std::fs::File| {
            let resp = self
                .until_ready(self.client.get(url.clone()).send())
                .await?
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(DownloadError::BadResponse(status));
            }
            let total = resp.content_length().unwrap_or(0);
            let mut written = 0u64;
            let mut stream = Box::pin(resp.bytes_stream());
            while let Some(chunk) = self.next_chunk(&mut stream).await? {
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
            let spans = HlsByteSpan::resolve(&segments, |u| u.as_str().to_string());
            let mut keys: HashMap<Url, Vec<u8>> = HashMap::new();
            let mut last_init: Option<(Url, Option<ByteRange>)> = None;
            let mut written = 0u64;
            for (i, (seg, span)) in segments.iter().zip(spans).enumerate() {
                self.check_cancel()?;
                if seg.is_gap {
                    continue;
                }
                if let Some(init) = &seg.init_segment {
                    let init_span = HlsByteSpan::of_init(init);
                    if init_span == HlsByteSpan::Invalid {
                        return Err(DownloadError::BadResponse(416));
                    }
                    let init_key = (init.uri.clone(), init_span.byte_range());
                    if last_init.as_ref() != Some(&init_key) {
                        let data = self.fetch(&init.uri, init_span).await?;
                        file.write_all(&data)
                            .map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
                        written += data.len() as u64;
                        last_init = Some(init_key);
                    }
                }
                if span == HlsByteSpan::Invalid {
                    return Err(DownloadError::BadResponse(416));
                }
                let mut data = self.fetch(&seg.uri, span).await?;
                if let Some(enc) = &seg.encryption_info {
                    if enc.method != EncryptionMethod::Aes128 {
                        return Err(DownloadError::DecryptFailed);
                    }
                    let key = match keys.get(&enc.uri) {
                        Some(k) => k.clone(),
                        None => {
                            let k = self.fetch(&enc.uri, HlsByteSpan::Whole).await?;
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

    async fn fetch(&self, url: &Url, span: HlsByteSpan) -> Result<Vec<u8>, DownloadError> {
        if span == HlsByteSpan::Invalid {
            return Err(DownloadError::BadResponse(416));
        }
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
            if let Some(h) = span.range_header() {
                req = req.header("Range", h);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if !(200..300).contains(&status) {
                        last_err = DownloadError::BadResponse(status);
                        if HlsByteSpan::is_final_failure(status) {
                            break;
                        }
                        continue;
                    }
                    let content_range = resp
                        .headers()
                        .get(reqwest::header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    match resp.bytes().await {
                        Ok(b) => {
                            if let Some(slice) = HlsByteSpan::recordable_body(
                                Some(&b),
                                status,
                                span.byte_range(),
                                content_range.as_deref(),
                            ) {
                                return Ok(slice.to_vec());
                            }
                            last_err = DownloadError::BadResponse(status);
                            if status == 206 || status == 200 {
                                break;
                            }
                        }
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
            // 재생목록 요청에도 시간 제한 — 멈추면 취소도 듣지 않았다.
            let resp = self
                .client
                .get(url.clone())
                .timeout(Duration::from_secs(30))
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

    /// 본문 1000바이트만 보내고 연결을 붙잡은 채 멈추는 서버.
    /// 연결만 받고 응답 머리를 보내지 않는 서버.
    async fn serve_silent() -> Url {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = stream.read(&mut buf).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        Url::parse(&format!("http://{addr}/v.mp4")).unwrap()
    }

    /// 응답 머리를 받기 전에 멈춘 요청도 취소가 바로 듣는다.
    #[tokio::test]
    async fn cancel_works_before_the_response_head_arrives() {
        let url = serve_silent().await;
        let dir = std::env::temp_dir().join(format!("cview-dl-head-{}", std::process::id()));
        let out = dir.join("v.mp4");
        let cancel = Arc::new(AtomicBool::new(false));
        let dl = VideoDownloader::new(cancel.clone());
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c.store(true, Ordering::Relaxed);
        });
        let r = tokio::time::timeout(
            Duration::from_secs(5),
            dl.download_file(&url, &out, Arc::new(|_, _| {})),
        )
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(r, Ok(Err(DownloadError::Cancelled))),
            "취소 5초 뒤에도 끝나지 않았다: {r:?}"
        );
    }

    async fn serve_stalling_mp4() -> Url {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = stream.read(&mut buf).await;
                    let mut resp = b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n".to_vec();
                    resp.extend_from_slice(&[1u8; 1000]);
                    let _ = stream.write_all(&resp).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        Url::parse(&format!("http://{addr}/v.mp4")).unwrap()
    }

    /// 본문 도중 멈춘 연결에서도 취소가 바로 듣는다 — 예전엔 다음 조각이 올 때까지 '받는 중'으로 남았다.
    #[tokio::test]
    async fn cancel_works_while_the_body_stalls() {
        let url = serve_stalling_mp4().await;
        let dir = std::env::temp_dir().join(format!("cview-dl-stall-{}", std::process::id()));
        let out = dir.join("v.mp4");
        let cancel = Arc::new(AtomicBool::new(false));
        let dl = VideoDownloader::new(cancel.clone());
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c.store(true, Ordering::Relaxed);
        });
        let r = tokio::time::timeout(
            Duration::from_secs(5),
            dl.download_file(&url, &out, Arc::new(|_, _| {})),
        )
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(r, Ok(Err(DownloadError::Cancelled))),
            "취소 5초 뒤에도 끝나지 않았다: {r:?}"
        );
    }

    async fn serve_byte_range_hls(mode: &'static str) -> (Url, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let playlist_url = Url::parse(&format!("http://{addr}/index.m3u8")).unwrap();
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    if first_line.contains("/index.m3u8") {
                        let body = "#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n\
                                    #EXT-X-BYTERANGE:2@0\n#EXTINF:2.0,\nmedia.mp4\n\
                                    #EXT-X-BYTERANGE:2\n#EXTINF:2.0,\nmedia.mp4\n\
                                    #EXT-X-ENDLIST\n";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }
                    let full = b"ABCD";
                    match mode {
                        "200-full" => {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                full.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(full).await;
                        }
                        "206-partial" => {
                            let range_line = req
                                .lines()
                                .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                                .unwrap_or("");
                            let (s, e) = if range_line.contains("0-1") {
                                (0usize, 1usize)
                            } else {
                                (2usize, 3usize)
                            };
                            let slice = &full[s..=e];
                            let resp = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {s}-{e}/4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                slice.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(slice).await;
                        }
                        _ => {
                            // 잘못된 206: 요청은 2바이트인데 전체 4바이트를 내려보낸다.
                            let resp = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                full.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(full).await;
                        }
                    }
                });
            }
        });
        (playlist_url, handle)
    }

    #[tokio::test]
    async fn hls_byte_range_handles_206_and_200_identically_and_rejects_bad_206() {
        let dir = std::env::temp_dir().join(format!("cview-dl-range-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // 1) 정상 206 부분 응답 → 정확히 "ABCD" 4바이트
        let (url_206, h1) = serve_byte_range_hls("206-partial").await;
        let out_206 = dir.join("ok_206.mp4");
        let dl = VideoDownloader::new(Arc::new(AtomicBool::new(false)));
        let n_206 = dl
            .download_hls(&url_206, &out_206, Arc::new(|_, _| {}))
            .await
            .unwrap();
        h1.abort();
        assert_eq!(n_206, 4);
        assert_eq!(std::fs::read(&out_206).unwrap(), b"ABCD");

        // 2) Range 를 무시하고 200 으로 전체 "ABCD" 를 돌려주는 서버 → 요청 구간만 잘라 같은 "ABCD" 4바이트
        let (url_200, h2) = serve_byte_range_hls("200-full").await;
        let out_200 = dir.join("ok_200.mp4");
        let n_200 = dl
            .download_hls(&url_200, &out_200, Arc::new(|_, _| {}))
            .await
            .unwrap();
        h2.abort();
        assert_eq!(n_200, 4);
        assert_eq!(std::fs::read(&out_200).unwrap(), b"ABCD");

        // 3) 요청 구간 길이와 다른 206 응답 → 완료 처리하지 않고 중단하며 부분 파일도 정리한다
        let (url_bad, h3) = serve_byte_range_hls("206-wrong-len").await;
        let out_bad = dir.join("bad_206.mp4");
        let err = dl
            .download_hls(&url_bad, &out_bad, Arc::new(|_, _| {}))
            .await;
        h3.abort();
        assert_eq!(err, Err(DownloadError::BadResponse(206)));
        assert!(!out_bad.exists());
        assert!(!part_path(&out_bad).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
