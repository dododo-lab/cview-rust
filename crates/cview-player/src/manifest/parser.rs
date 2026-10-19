//! `Sources/CViewPlayer/HLSManifestParser.swift` 포팅 — HLS M3U8 매니페스트 파서.
//!
//! 마스터/미디어 재생목록과 LL-HLS 확장(PART·PRELOAD-HINT·SERVER-CONTROL)을 지원한다.
//! 상대 URI 는 RFC 3986 참조 해석(`Url::join`)으로 푼다 — 토큰 경로의 `%2F` 를 이중 인코딩하지 않는다.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use cview_core::models::player::PlayerError;
use cview_core::utils::fast_date_parser;
use cview_core::AppError;
use url::Url;

// MARK: - 모델

/// 마스터 재생목록.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterPlaylist {
    /// 대역폭 내림차순.
    pub variants: Vec<Variant>,
    pub uri: Url,
}

/// 마스터 재생목록의 variant 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub bandwidth: i64,
    pub average_bandwidth: Option<i64>,
    pub resolution: String,
    pub codecs: Option<String>,
    pub frame_rate: Option<f64>,
    pub uri: Url,
    pub name: Option<String>,
}

impl Variant {
    /// `"\(bandwidth)-\(resolution)"`.
    pub fn id(&self) -> String {
        format!("{}-{}", self.bandwidth, self.resolution)
    }

    /// 표시용 화질 라벨.
    pub fn quality_label(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        for label in ["1080", "720", "480", "360"] {
            if self.resolution.contains(label) {
                return format!("{label}p");
            }
        }
        format!("{}kbps", self.bandwidth / 1000)
    }
}

/// VOD/EVENT 판별.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistType {
    Vod,
    Event,
}

impl PlaylistType {
    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "VOD" => Some(Self::Vod),
            "EVENT" => Some(Self::Event),
            _ => None,
        }
    }
}

/// 암호화 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMethod {
    Aes128,
    SampleAes,
}

impl EncryptionMethod {
    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "AES-128" => Some(Self::Aes128),
            "SAMPLE-AES" => Some(Self::SampleAes),
            _ => None,
        }
    }
}

/// AES-128 암호화 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionInfo {
    pub method: EncryptionMethod,
    pub uri: Url,
    pub iv: Option<[u8; 16]>,
}

/// `#EXT-X-BYTERANGE` 값. offset 생략은 `None` 으로 남긴다(해석은 [`super::byte_span`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub length: i64,
    pub offset: Option<i64>,
}

/// `#EXT-X-MAP` — fMP4 init 세그먼트 위치.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitSegment {
    pub uri: Url,
    pub byte_range: Option<ByteRange>,
}

/// LL-HLS 부분 세그먼트.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSegment {
    pub id: String,
    pub duration: f64,
    pub uri: Url,
    pub independent: bool,
    pub byte_range: Option<ByteRange>,
    pub gap: bool,
}

/// 완성된 미디어 세그먼트.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// 시퀀스 번호.
    pub id: i64,
    pub duration: f64,
    pub uri: Url,
    pub start_time: f64,
    pub program_date_time: Option<DateTime<Utc>>,
    pub discontinuity: bool,
    pub byte_range: Option<ByteRange>,
    pub parts: Vec<PartialSegment>,
    pub encryption_info: Option<EncryptionInfo>,
    pub discontinuity_sequence: i64,
    /// 이 세그먼트에 적용되는 `#EXT-X-MAP` (다음 MAP 까지 지속).
    pub init_segment: Option<InitSegment>,
    /// `#EXT-X-GAP` — 바로 다음 세그먼트 하나에만 적용.
    pub is_gap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerControl {
    pub can_skip_until: Option<f64>,
    pub can_block_reload: bool,
    pub hold_back: Option<f64>,
    pub part_hold_back: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreloadHint {
    pub hint_type: String,
    pub uri: Url,
    pub byte_range_start: Option<i64>,
    pub byte_range_length: Option<i64>,
}

/// 미디어 재생목록.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPlaylist {
    pub target_duration: f64,
    pub media_sequence: i64,
    pub part_target_duration: Option<f64>,
    pub segments: Vec<Segment>,
    pub partial_segments: Vec<PartialSegment>,
    pub server_control: Option<ServerControl>,
    pub preload_hint: Option<PreloadHint>,
    pub is_end_list: bool,
    pub is_low_latency: bool,
    pub version: Option<i64>,
    pub playlist_type: Option<PlaylistType>,
    pub discontinuity_sequence: i64,
}

impl MediaPlaylist {
    pub fn total_duration(&self) -> f64 {
        self.segments.iter().map(|s| s.duration).sum()
    }

    /// 마지막 세그먼트의 끝 시각.
    pub fn end_time(&self) -> f64 {
        self.segments
            .last()
            .map(|s| s.start_time + s.duration)
            .unwrap_or(0.0)
    }
}

// MARK: - 파서

/// HLS M3U8 파서 (상태 없음, 스레드 안전).
#[derive(Debug, Clone, Copy, Default)]
pub struct HlsManifestParser;

fn invalid_manifest() -> AppError {
    AppError::Player(PlayerError::InvalidManifest)
}

/// CR/LF/CRLF 모두 줄 구분자로 쓴다(Swift `whereSeparator: \.isNewline`).
fn split_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = content;
    loop {
        match rest.find(['\r', '\n']) {
            Some(i) => {
                lines.push(&rest[..i]);
                let skip = if rest[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[i + skip..];
            }
            None => {
                lines.push(rest);
                break;
            }
        }
    }
    lines
}

fn trim_ws(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}

fn parse_f64(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

fn parse_i64(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

impl HlsManifestParser {
    pub fn new() -> Self {
        Self
    }

    /// 마스터 재생목록 파싱.
    pub fn parse_master_playlist(
        &self,
        content: &str,
        base_url: &Url,
    ) -> Result<MasterPlaylist, AppError> {
        let lines = split_lines(content);
        if !lines.first().is_some_and(|l| l.starts_with("#EXTM3U")) {
            return Err(invalid_manifest());
        }
        let mut variants = Vec::new();
        let mut attrs: HashMap<String, String> = HashMap::new();
        for line in lines {
            let trimmed = trim_ws(line);
            if let Some(rest) = trimmed.strip_prefix("#EXT-X-STREAM-INF:") {
                attrs = parse_attributes(rest);
            } else if !trimmed.starts_with('#') && !trimmed.is_empty() && !attrs.is_empty() {
                let get = |k: &str| attrs.get(k).cloned();
                variants.push(Variant {
                    bandwidth: get("BANDWIDTH").and_then(|v| parse_i64(&v)).unwrap_or(0),
                    average_bandwidth: get("AVERAGE-BANDWIDTH").and_then(|v| parse_i64(&v)),
                    resolution: get("RESOLUTION").unwrap_or_default(),
                    codecs: get("CODECS"),
                    frame_rate: get("FRAME-RATE").and_then(|v| parse_f64(&v)),
                    uri: resolve_url(trimmed, base_url),
                    name: get("NAME"),
                });
                attrs.clear();
            }
        }
        if variants.is_empty() {
            return Err(invalid_manifest());
        }
        variants.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
        Ok(MasterPlaylist {
            variants,
            uri: base_url.clone(),
        })
    }

    /// 미디어 재생목록 파싱.
    pub fn parse_media_playlist(
        &self,
        content: &str,
        base_url: &Url,
    ) -> Result<MediaPlaylist, AppError> {
        let lines = split_lines(content);
        if !lines.first().is_some_and(|l| l.starts_with("#EXTM3U")) {
            return Err(invalid_manifest());
        }

        let mut target_duration = 0.0;
        let mut media_sequence = 0i64;
        let mut part_target_duration = None;
        let mut segments: Vec<Segment> = Vec::new();
        let mut partial_segments = Vec::new();
        let mut server_control = None;
        let mut preload_hint = None;
        let mut is_end_list = false;
        let mut is_low_latency = false;
        let mut version = None;
        let mut playlist_type = None;
        let mut discontinuity_sequence = 0i64;
        let mut current_encryption: Option<EncryptionInfo> = None;
        let mut continuity_index = 0i64;
        let mut current_init: Option<InitSegment> = None;

        let mut current_duration = 0.0;
        let mut current_pdt = None;
        let mut current_discontinuity = false;
        let mut current_gap = false;
        let mut current_byte_range = None;
        let mut current_parts: Vec<PartialSegment> = Vec::new();
        let mut current_time = 0.0;
        let mut segment_index = media_sequence;

        for line in lines {
            let t = trim_ws(line);
            if let Some(v) = t.strip_prefix("#EXT-X-TARGETDURATION:") {
                target_duration = parse_f64(v).unwrap_or(0.0);
            } else if let Some(v) = t.strip_prefix("#EXT-X-VERSION:") {
                version = parse_i64(v);
            } else if let Some(v) = t.strip_prefix("#EXT-X-PLAYLIST-TYPE:") {
                playlist_type = PlaylistType::from_raw(trim_ws(v));
            } else if let Some(v) = t.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
                discontinuity_sequence = parse_i64(v).unwrap_or(0);
                continuity_index = discontinuity_sequence;
            } else if let Some(v) = t.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                media_sequence = parse_i64(v).unwrap_or(0);
                segment_index = media_sequence;
            } else if let Some(v) = t.strip_prefix("#EXT-X-KEY:") {
                current_encryption = parse_key(v, current_encryption, base_url);
            } else if let Some(v) = t.strip_prefix("#EXT-X-MAP:") {
                if let Some(init) = parse_init_segment(v, base_url) {
                    current_init = Some(init);
                }
            } else if t == "#EXT-X-GAP" {
                current_gap = true;
            } else if let Some(v) = t.strip_prefix("#EXTINF:") {
                let d = v.split(',').next().unwrap_or("0");
                current_duration = parse_f64(d).unwrap_or(0.0);
            } else if let Some(v) = t.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:") {
                current_pdt = fast_date_parser::parse(v).or_else(|| parse_iso8601(v));
            } else if t == "#EXT-X-DISCONTINUITY" {
                current_discontinuity = true;
                continuity_index += 1;
            } else if let Some(v) = t.strip_prefix("#EXT-X-BYTERANGE:") {
                current_byte_range = parse_byte_range(v);
            } else if let Some(v) = t.strip_prefix("#EXT-X-PART-INF:") {
                let attrs = parse_attributes(v);
                part_target_duration = attrs.get("PART-TARGET").and_then(|v| parse_f64(v));
                is_low_latency = true;
            } else if let Some(v) = t.strip_prefix("#EXT-X-PART:") {
                let attrs = parse_attributes(v);
                if let Some(uri) = attrs.get("URI") {
                    let part = PartialSegment {
                        id: format!("{}-{}", segment_index, current_parts.len()),
                        duration: attrs
                            .get("DURATION")
                            .and_then(|v| parse_f64(v))
                            .unwrap_or(0.0),
                        uri: resolve_url(uri, base_url),
                        independent: attrs
                            .get("INDEPENDENT")
                            .is_some_and(|v| v.to_uppercase() == "YES"),
                        byte_range: attrs.get("BYTERANGE").and_then(|v| parse_byte_range(v)),
                        gap: attrs.get("GAP").is_some_and(|v| v.to_uppercase() == "YES"),
                    };
                    current_parts.push(part.clone());
                    partial_segments.push(part);
                }
                is_low_latency = true;
            } else if let Some(v) = t.strip_prefix("#EXT-X-SERVER-CONTROL:") {
                let a = parse_attributes(v);
                server_control = Some(ServerControl {
                    can_skip_until: a.get("CAN-SKIP-UNTIL").and_then(|v| parse_f64(v)),
                    can_block_reload: a
                        .get("CAN-BLOCK-RELOAD")
                        .is_some_and(|v| v.to_uppercase() == "YES"),
                    hold_back: a.get("HOLD-BACK").and_then(|v| parse_f64(v)),
                    part_hold_back: a.get("PART-HOLD-BACK").and_then(|v| parse_f64(v)),
                });
            } else if let Some(v) = t.strip_prefix("#EXT-X-PRELOAD-HINT:") {
                let a = parse_attributes(v);
                if let Some(uri) = a.get("URI") {
                    preload_hint = Some(PreloadHint {
                        hint_type: a.get("TYPE").cloned().unwrap_or_else(|| "PART".into()),
                        uri: resolve_url(uri, base_url),
                        byte_range_start: a.get("BYTERANGE-START").and_then(|v| parse_i64(v)),
                        byte_range_length: a.get("BYTERANGE-LENGTH").and_then(|v| parse_i64(v)),
                    });
                }
            } else if t == "#EXT-X-ENDLIST" {
                is_end_list = true;
            } else if !t.starts_with('#') && !t.is_empty() {
                segments.push(Segment {
                    id: segment_index,
                    duration: current_duration,
                    uri: resolve_url(t, base_url),
                    start_time: current_time,
                    program_date_time: current_pdt.take(),
                    discontinuity: current_discontinuity,
                    byte_range: current_byte_range.take(),
                    parts: std::mem::take(&mut current_parts),
                    encryption_info: effective_encryption(&current_encryption, segment_index),
                    discontinuity_sequence: continuity_index,
                    init_segment: current_init.clone(),
                    is_gap: current_gap,
                });
                current_time += current_duration;
                segment_index += 1;
                current_duration = 0.0;
                current_discontinuity = false;
                current_gap = false;
            }
        }

        Ok(MediaPlaylist {
            target_duration,
            media_sequence,
            part_target_duration,
            segments,
            partial_segments,
            server_control,
            preload_hint,
            is_end_list,
            is_low_latency,
            version,
            playlist_type,
            discontinuity_sequence,
        })
    }
}

// MARK: - 헬퍼

/// M3U8 속성 문자열 → 사전.
pub fn parse_attributes(input: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut rest = input;
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = trim_ws(&rest[..eq]).to_string();
        rest = &rest[eq + 1..];
        let value;
        if let Some(after) = rest.strip_prefix('"') {
            if let Some(end) = after.find('"') {
                value = after[..end].to_string();
                rest = &after[end + 1..];
            } else {
                value = after.to_string();
                rest = "";
            }
        } else if let Some(comma) = rest.find(',') {
            value = rest[..comma].to_string();
            rest = &rest[comma + 1..];
        } else {
            value = trim_ws(rest).to_string();
            rest = "";
        }
        result.insert(key, value);
        if let Some(r) = rest.strip_prefix(',') {
            rest = r;
        }
    }
    result
}

/// 상대 경로를 base 의 디렉터리 기준으로 푼다. 절대 http(s) URL 은 그대로.
pub fn resolve_url(path: &str, base: &Url) -> Url {
    let cleaned = path.trim_matches('"');
    if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
        return Url::parse(cleaned).unwrap_or_else(|_| base.clone());
    }
    base.join(cleaned).unwrap_or_else(|_| base.clone())
}

pub fn parse_byte_range(input: &str) -> Option<ByteRange> {
    let mut parts = input.split('@').filter(|p| !p.is_empty());
    let length = parse_i64(parts.next()?)?;
    let offset = parts.next().and_then(parse_i64);
    Some(ByteRange { length, offset })
}

fn parse_init_segment(attr: &str, base: &Url) -> Option<InitSegment> {
    let attrs = parse_attributes(attr);
    let uri = attrs.get("URI")?;
    Some(InitSegment {
        uri: resolve_url(uri, base),
        byte_range: attrs.get("BYTERANGE").and_then(|v| parse_byte_range(v)),
    })
}

/// `#EXT-X-KEY` — METHOD=NONE 이면 해제, 모르는 METHOD/URI 없음이면 직전 키 유지.
fn parse_key(attr: &str, current: Option<EncryptionInfo>, base: &Url) -> Option<EncryptionInfo> {
    let attrs = parse_attributes(attr);
    let method = attrs.get("METHOD").map(String::as_str).unwrap_or("NONE");
    if method == "NONE" {
        return None;
    }
    let (Some(method), Some(uri)) = (EncryptionMethod::from_raw(method), attrs.get("URI")) else {
        return current;
    };
    Some(EncryptionInfo {
        method,
        uri: resolve_url(uri, base),
        iv: attrs.get("IV").and_then(|v| parse_hex_iv(v)),
    })
}

fn effective_encryption(current: &Option<EncryptionInfo>, sequence: i64) -> Option<EncryptionInfo> {
    let enc = current.as_ref()?;
    if enc.iv.is_some() {
        return Some(enc.clone());
    }
    Some(EncryptionInfo {
        iv: Some(sequence_number_to_iv(sequence)),
        ..enc.clone()
    })
}

/// "0x..." 16진수 IV → 16바이트(앞 0 패딩).
pub fn parse_hex_iv(hex: &str) -> Option<[u8; 16]> {
    let mut s = trim_ws(hex);
    if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        s = r;
    }
    if s.len() > 32 || !s.is_ascii() {
        return None;
    }
    let padded = format!("{s:0>32}");
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// 시퀀스 번호를 big-endian 16바이트 IV 로(하위 8바이트).
pub fn sequence_number_to_iv(seq: i64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[8..].copy_from_slice(&(seq as u64).to_be_bytes());
    out
}

fn parse_iso8601(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(trim_ws(s))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/HLSManifestParserTests.swift` 포팅.
    use super::*;

    fn p() -> HlsManifestParser {
        HlsManifestParser::new()
    }
    fn master_base() -> Url {
        Url::parse("https://cdn.example.com/live/master.m3u8").unwrap()
    }
    fn media_base() -> Url {
        Url::parse("https://cdn.example.com/live/playlist.m3u8").unwrap()
    }
    fn master(c: &str) -> MasterPlaylist {
        p().parse_master_playlist(c, &master_base()).unwrap()
    }
    fn media(c: &str) -> MediaPlaylist {
        p().parse_media_playlist(c, &media_base()).unwrap()
    }
    fn variant(bw: i64, res: &str, name: Option<&str>) -> Variant {
        Variant {
            bandwidth: bw,
            average_bandwidth: None,
            resolution: res.into(),
            codecs: None,
            frame_rate: None,
            uri: Url::parse("https://example.com/v.m3u8").unwrap(),
            name: name.map(Into::into),
        }
    }
    fn last_component(u: &Url) -> String {
        u.path_segments()
            .and_then(|mut s| s.next_back().map(str::to_string))
            .unwrap_or_default()
    }

    #[test]
    fn master_single_and_sorted() {
        let pl = master(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p/stream.m3u8",
        );
        assert_eq!(pl.variants.len(), 1);
        assert_eq!(pl.variants[0].bandwidth, 3_000_000);
        assert_eq!(pl.variants[0].resolution, "1280x720");
        assert!(pl.variants[0].uri.as_str().contains("720p/stream.m3u8"));
        assert_eq!(pl.uri, master_base());

        let pl = master(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=854x480\n480p/stream.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080p/stream.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p/stream.m3u8",
        );
        let bws: Vec<_> = pl.variants.iter().map(|v| v.bandwidth).collect();
        assert_eq!(bws, vec![5_000_000, 3_000_000, 1_500_000]);
    }

    #[test]
    fn master_full_attributes() {
        let pl = master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,AVERAGE-BANDWIDTH=4500000,RESOLUTION=1920x1080,CODECS=\"avc1.64001f,mp4a.40.2\",FRAME-RATE=30.0,NAME=\"1080p\"\n1080p/playlist.m3u8");
        let v = &pl.variants[0];
        assert_eq!(v.codecs.as_deref(), Some("avc1.64001f,mp4a.40.2"));
        assert_eq!(v.frame_rate, Some(30.0));
        assert_eq!(v.name.as_deref(), Some("1080p"));
        assert_eq!(v.average_bandwidth, Some(4_500_000));
    }

    #[test]
    fn master_absolute_uri_and_errors() {
        let pl = master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhttps://other-cdn.example.com/1080p/playlist.m3u8");
        assert_eq!(
            pl.variants[0].uri.as_str(),
            "https://other-cdn.example.com/1080p/playlist.m3u8"
        );
        assert!(p()
            .parse_master_playlist("Not a valid manifest", &master_base())
            .is_err());
        assert!(p()
            .parse_master_playlist("#EXTM3U\n", &master_base())
            .is_err());
        // 빈 줄/주석은 크래시하지 않는다.
        let pl = master("#EXTM3U\n## comment\n\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n\n720p/stream.m3u8");
        assert_eq!(pl.variants.len(), 1);
    }

    #[test]
    fn master_crlf() {
        let pl = master("#EXTM3U\r\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\r\n720p/stream.m3u8\r\n");
        assert_eq!(pl.variants.len(), 1);
        assert!(pl.variants[0].uri.as_str().ends_with("720p/stream.m3u8"));
    }

    #[test]
    fn quality_labels() {
        assert_eq!(
            variant(5_000_000, "1920x1080", Some("Full HD")).quality_label(),
            "Full HD"
        );
        assert_eq!(
            variant(5_000_000, "1920x1080", None).quality_label(),
            "1080p"
        );
        assert_eq!(variant(3_000_000, "1280x720", None).quality_label(), "720p");
        assert_eq!(variant(1_500_000, "854x480", None).quality_label(), "480p");
        assert_eq!(variant(800_000, "640x360", None).quality_label(), "360p");
        assert_eq!(
            variant(2_500_000, "960x540", None).quality_label(),
            "2500kbps"
        );
        assert_eq!(
            variant(5_000_000, "1920x1080", None).id(),
            "5000000-1920x1080"
        );
        assert_eq!(
            variant(1000, "720x480", None),
            variant(1000, "720x480", None)
        );
        assert_ne!(
            variant(1000, "720x480", None),
            variant(2000, "1280x720", None)
        );
    }

    #[test]
    fn media_basics() {
        let pl = media(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:5.0,\nseg0.ts",
        );
        assert_eq!(pl.target_duration, 6.0);
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\nseg100.ts\n#EXTINF:4.0,\nseg101.ts\n#EXTINF:4.0,\nseg102.ts");
        assert_eq!(pl.media_sequence, 100);
        let ids: Vec<_> = pl.segments.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![100, 101, 102]);
    }

    #[test]
    fn media_times() {
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:5\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\nseg0.ts\n#EXTINF:3.5,\nseg1.ts\n#EXTINF:5.0,\nseg2.ts");
        let starts: Vec<_> = pl.segments.iter().map(|s| s.start_time).collect();
        assert_eq!(starts, vec![0.0, 4.0, 7.5]);
        assert!((pl.total_duration() - 12.5).abs() < 1e-3);
        assert!((pl.end_time() - 12.5).abs() < 1e-3);
        let empty = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0");
        assert!(empty.segments.is_empty());
        assert_eq!(empty.total_duration(), 0.0);
        assert_eq!(empty.end_time(), 0.0);
    }

    #[test]
    fn media_endlist_discontinuity_invalid() {
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-ENDLIST");
        assert!(pl.is_end_list);
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts");
        assert!(!pl.is_end_list);
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4.0,\nseg1.ts");
        assert!(!pl.segments[0].discontinuity);
        assert!(pl.segments[1].discontinuity);
        assert!(p()
            .parse_media_playlist("This is not M3U8", &media_base())
            .is_err());
    }

    #[test]
    fn llhls() {
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg0.ts");
        assert!(pl.is_low_latency);
        assert_eq!(pl.part_target_duration, Some(0.5));

        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg100.ts\n#EXT-X-PART:DURATION=0.5,URI=\"part101-0.ts\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.5,URI=\"part101-1.ts\"\n#EXT-X-PART:DURATION=0.5,URI=\"part101-2.ts\",INDEPENDENT=YES");
        let ind: Vec<_> = pl.partial_segments.iter().map(|p| p.independent).collect();
        assert_eq!(ind, vec![true, false, true]);

        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=1.5,CAN-SKIP-UNTIL=24.0,HOLD-BACK=6.0\n#EXTINF:4.0,\nseg0.ts");
        let sc = pl.server_control.unwrap();
        assert!(sc.can_block_reload);
        assert_eq!(sc.part_hold_back, Some(1.5));
        assert_eq!(sc.can_skip_until, Some(24.0));
        assert_eq!(sc.hold_back, Some(6.0));

        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"next-part.ts\"");
        let hint = pl.preload_hint.unwrap();
        assert_eq!(hint.hint_type, "PART");
        assert_eq!(last_component(&hint.uri), "next-part.ts");

        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg100.ts\n#EXT-X-PART:DURATION=0.5,URI=\"part101-0.ts\"");
        assert!(pl.is_low_latency);

        let pl =
            media("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXTINF:6.0,\nseg1.ts");
        assert!(!pl.is_low_latency);
        assert!(pl.part_target_duration.is_none());
        assert!(pl.server_control.is_none());
        assert!(pl.preload_hint.is_none());
    }

    #[test]
    fn pdt_and_byte_range() {
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PROGRAM-DATE-TIME:2024-01-15T10:00:00.000Z\n#EXTINF:4.0,\nseg0.ts");
        assert!(pl.segments[0].program_date_time.is_some());
        let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PROGRAM-DATE-TIME:2024-01-15T10:00:00+09:00\n#EXTINF:4.0,\nseg0.ts");
        assert!(pl.segments[0].program_date_time.is_some());
        let pl = media(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-BYTERANGE:1000@0\n#EXTINF:4.0,\nseg0.ts",
        );
        assert_eq!(
            pl.segments[0].byte_range,
            Some(ByteRange {
                length: 1000,
                offset: Some(0)
            })
        );
        let pl =
            media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-BYTERANGE:2048\n#EXTINF:4.0,\nseg0.ts");
        assert_eq!(
            pl.segments[0].byte_range,
            Some(ByteRange {
                length: 2048,
                offset: None
            })
        );
        assert_eq!(parse_byte_range("@"), None);
        assert_eq!(parse_byte_range(""), None);
    }

    const TOKEN_BASE: &str = "https://livecloud.pstatic.net/chzzk/lip2_kr/cflexnmss2u0001/warrpu9abc/720p/hdntl=exp=1790086400~acl=*%2Fwarrpu9abc%2F*~data=hdntl~hmac=0f0f/chunklist.m3u8";

    fn token_media(c: &str) -> MediaPlaylist {
        p().parse_media_playlist(c, &Url::parse(TOKEN_BASE).unwrap())
            .unwrap()
    }

    #[test]
    fn map_without_byte_range_keeps_token_path() {
        let pl = token_media("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-MAP:URI=\"720p_init.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:00.000Z\n#EXTINF:2.000,\n720p_0_0_100.m4v\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:02.000Z\n#EXTINF:2.000,\n720p_0_0_101.m4v");
        assert_eq!(pl.segments.len(), 2);
        for s in &pl.segments {
            let init = s.init_segment.as_ref().unwrap();
            assert!(init.byte_range.is_none());
            assert!(init.uri.as_str().ends_with(
                "/720p/hdntl=exp=1790086400~acl=*%2Fwarrpu9abc%2F*~data=hdntl~hmac=0f0f/720p_init.m4v"
            ));
            assert!(!s.is_gap);
        }
    }

    #[test]
    fn map_with_byte_range_and_changes() {
        let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"main.mp4\",BYTERANGE=\"720@0\"\n#EXTINF:4.0,\n#EXT-X-BYTERANGE:50000@720\nmain.mp4");
        let init = pl.segments[0].init_segment.as_ref().unwrap();
        assert_eq!(
            init.byte_range,
            Some(ByteRange {
                length: 720,
                offset: Some(0)
            })
        );
        assert_eq!(last_component(&init.uri), "main.mp4");
        assert_eq!(
            pl.segments[0].byte_range,
            Some(ByteRange {
                length: 50000,
                offset: Some(720)
            })
        );

        let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-MAP:URI=\"a_init.m4v\"\n#EXTINF:2.0,\na_10.m4v\n#EXTINF:2.0,\na_11.m4v\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"b_init.m4v\"\n#EXTINF:2.0,\nb_12.m4v\n#EXTINF:2.0,\nb_13.m4v");
        let names: Vec<_> = pl
            .segments
            .iter()
            .map(|s| last_component(&s.init_segment.as_ref().unwrap().uri))
            .collect();
        assert_eq!(
            names,
            ["a_init.m4v", "a_init.m4v", "b_init.m4v", "b_init.m4v"]
        );
        assert!(pl.segments[2].discontinuity);
    }

    #[test]
    fn missing_or_invalid_map_and_gap() {
        let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts");
        assert!(pl.segments[0].init_segment.is_none());
        let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"good_init.mp4\"\n#EXTINF:2.0,\ns0.m4v\n#EXT-X-MAP:BYTERANGE=\"100@0\"\n#EXTINF:2.0,\ns1.m4v");
        let init = pl.segments[1].init_segment.as_ref().unwrap();
        assert_eq!(last_component(&init.uri), "good_init.mp4");
        assert!(init.byte_range.is_none());

        let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MAP:URI=\"init.m4v\"\n#EXTINF:2.0,\ns0.m4v\n#EXT-X-GAP\n#EXTINF:2.0,\ns1.m4v\n#EXTINF:2.0,\ns2.m4v");
        let gaps: Vec<_> = pl.segments.iter().map(|s| s.is_gap).collect();
        assert_eq!(gaps, vec![false, true, false]);
        assert!(pl
            .segments
            .iter()
            .all(|s| last_component(&s.init_segment.as_ref().unwrap().uri) == "init.m4v"));
    }

    #[test]
    fn llhls_yields_only_complete_segments() {
        let pl = token_media("#EXTM3U\n#EXT-X-VERSION:9\n#EXT-X-TARGETDURATION:10\n#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=1.5,CAN-SKIP-UNTIL=12.0\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXT-X-MEDIA-SEQUENCE:200\n#EXT-X-DISCONTINUITY-SEQUENCE:3\n#EXT-X-MAP:URI=\"1080p_init.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:00.000Z\n#EXTINF:2.000,\n1080p_0_0_200.m4v\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.0.m4v\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.1.m4v\"\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.2.m4v\"\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.3.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:02.000Z\n#EXTINF:2.000,\n1080p_0_0_201.m4v\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_202.0.m4v\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_202.1.m4v\"\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"1080p_0_0_202.2.m4v\"\n#EXT-X-RENDITION-REPORT:URI=\"../720p/chunklist.m3u8\",LAST-MSN=202,LAST-PART=1");
        assert!(pl.is_low_latency);
        let ids: Vec<_> = pl.segments.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![200, 201]);
        assert_eq!(
            pl.segments.iter().map(|s| s.duration).collect::<Vec<_>>(),
            vec![2.0, 2.0]
        );
        assert_eq!(pl.segments[1].parts.len(), 4);
        assert_eq!(pl.partial_segments.len(), 6);
        assert_eq!(
            last_component(&pl.preload_hint.unwrap().uri),
            "1080p_0_0_202.2.m4v"
        );
        assert!(pl.segments.iter().all(|s| s.discontinuity_sequence == 3));
    }

    #[test]
    fn key_handling() {
        let segs = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4.0,\ns7.ts\n#EXT-X-KEY:METHOD=FUTURE-CIPHER,URI=\"other.bin\"\n#EXTINF:4.0,\ns8.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key2.bin\",IV=0x000000000000000000000000000000FF\n#EXTINF:4.0,\ns9.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:4.0,\ns10.ts").segments;
        let mut derived = [0u8; 16];
        derived[15] = 7;
        assert_eq!(segs[0].encryption_info.as_ref().unwrap().iv, Some(derived));
        assert_eq!(
            last_component(&segs[0].encryption_info.as_ref().unwrap().uri),
            "key.bin"
        );
        assert_eq!(
            last_component(&segs[1].encryption_info.as_ref().unwrap().uri),
            "key.bin"
        );
        assert_eq!(segs[1].encryption_info.as_ref().unwrap().iv.unwrap()[15], 8);
        assert_eq!(
            segs[2].encryption_info.as_ref().unwrap().iv.unwrap()[15],
            0xFF
        );
        assert!(segs[3].encryption_info.is_none());
    }
}
