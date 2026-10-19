//! `Sources/CViewPlayer/HLSByteSpan.swift` 포팅.
//!
//! `#EXT-X-BYTERANGE`·`#EXT-X-MAP BYTERANGE` 를 절대 바이트 구간으로 풀고 구간 응답을 검증하는 순수 헬퍼.
//! 녹화와 타임시프트 링이 같은 규칙을 쓴다.

use url::Url;

use super::parser::{ByteRange, InitSegment, Segment};

/// 조각(또는 init) 하나가 가리키는 바이트 구간.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlsByteSpan {
    /// BYTERANGE 없음 — 자원 전체.
    Whole,
    /// 절대 구간 — `offset` 은 늘 채워져 있다.
    Range(ByteRange),
    /// 풀 수 없음 또는 비정상 값.
    Invalid,
}

impl HlsByteSpan {
    /// 절대 구간이면 그 값.
    pub fn byte_range(&self) -> Option<ByteRange> {
        match self {
            Self::Range(r) => Some(*r),
            _ => None,
        }
    }

    /// HTTP `Range` 헤더 값.
    pub fn range_header(&self) -> Option<String> {
        let Self::Range(r) = self else { return None };
        let offset = r.offset?;
        Some(format!("bytes={}-{}", offset, offset + r.length - 1))
    }

    /// 길이 > 0, offset ≥ 0, 오버플로 없음일 때만 절대 구간.
    pub fn validated(length: i64, offset: Option<i64>) -> Self {
        match offset {
            Some(offset) if length > 0 && offset >= 0 && offset.checked_add(length).is_some() => {
                Self::Range(ByteRange {
                    length,
                    offset: Some(offset),
                })
            }
            _ => Self::Invalid,
        }
    }

    /// `#EXT-X-MAP` 의 구간 — offset 생략은 0 (RFC 8216 §4.3.2.5).
    pub fn of_init(init: &InitSegment) -> Self {
        match init.byte_range {
            None => Self::Whole,
            Some(r) => Self::validated(r.length, Some(r.offset.unwrap_or(0))),
        }
    }

    /// 목록 순서대로 조각 구간을 푼다. offset 생략은 같은 목록의 직전 조각이 같은 자원일 때 그 끝부터.
    pub fn resolve(segments: &[Segment], resource_key: impl Fn(&Url) -> String) -> Vec<Self> {
        if !segments.iter().any(|s| s.byte_range.is_some()) {
            return vec![Self::Whole; segments.len()];
        }
        let mut previous: Option<(String, i64)> = None;
        segments
            .iter()
            .map(|segment| {
                let Some(range) = segment.byte_range else {
                    previous = None;
                    return Self::Whole;
                };
                let key = resource_key(&segment.uri);
                let implied = previous
                    .as_ref()
                    .filter(|(k, _)| *k == key)
                    .map(|(_, end)| *end);
                let span = Self::validated(range.length, range.offset.or(implied));
                match span.byte_range() {
                    Some(ByteRange {
                        length,
                        offset: Some(offset),
                    }) => {
                        previous = Some((key, offset + length));
                        span
                    }
                    _ => {
                        previous = None;
                        Self::Invalid
                    }
                }
            })
            .collect()
    }

    /// 기록해도 되는 본문 — 아니면 `None`.
    pub fn recordable_body<'a>(
        data: Option<&'a [u8]>,
        status: u16,
        byte_range: Option<ByteRange>,
        content_range: Option<&str>,
    ) -> Option<&'a [u8]> {
        let data = data.filter(|d| !d.is_empty())?;
        let Some(range) = byte_range else {
            return (status == 200 || status == 206).then_some(data);
        };
        let Self::Range(valid) = Self::validated(range.length, Some(range.offset.unwrap_or(0)))
        else {
            return None;
        };
        let offset = valid.offset?;
        match status {
            206 => {
                if let Some(cr) = content_range {
                    if !Self::content_range_matches(cr, offset, valid.length) {
                        return None;
                    }
                }
                (data.len() as i64 == valid.length).then_some(data)
            }
            200 => {
                let len = data.len() as i64;
                if offset > len || valid.length > len - offset {
                    return None;
                }
                Some(&data[offset as usize..(offset + valid.length) as usize])
            }
            _ => None,
        }
    }

    /// `Content-Range: bytes a-b/total` 의 a·b 가 `[offset, offset+length-1]` 인지.
    pub fn content_range_matches(header: &str, offset: i64, length: i64) -> bool {
        let value = header.trim_matches([' ', '\t']);
        if !value.to_lowercase().starts_with("bytes ") {
            return false;
        }
        let Some(span) = value[6..].split('/').next().filter(|s| !s.is_empty()) else {
            return false;
        };
        let bounds: Vec<&str> = span.split('-').collect();
        if bounds.len() != 2 {
            return false;
        }
        let (Ok(first), Ok(last)) = (
            bounds[0].trim().parse::<i64>(),
            bounds[1].trim().parse::<i64>(),
        ) else {
            return false;
        };
        first == offset && last == offset + length - 1
    }

    /// 다시 받아도 같을 최종 실패 — 408·429 를 뺀 4xx.
    pub fn is_final_failure(status: u16) -> bool {
        (400..500).contains(&status) && status != 408 && status != 429
    }
}

#[cfg(test)]
mod tests {
    //! `PrecisionReviewR4RecordingHLSTests` · `PrecisionReviewR4TimeshiftByteRangeTests` 의 순수 부분 포팅.
    use super::*;
    use crate::manifest::parser::HlsManifestParser;

    fn range(length: i64, offset: i64) -> Option<ByteRange> {
        Some(ByteRange {
            length,
            offset: Some(offset),
        })
    }

    #[test]
    fn recordable_body() {
        let body: Vec<u8> = (0..1000).map(|i| (i % 251) as u8).collect();
        let w = range(200, 100);
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 200, w, None),
            Some(&body[100..300])
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body[100..300]), 206, w, None),
            Some(&body[100..300])
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body[100..250]), 206, w, None),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body[..250]), 200, w, None),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 404, w, None),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 200, None, None),
            Some(&body[..])
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 206, None, None),
            Some(&body[..])
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 404, None, None),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&[]), 200, None, None),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(&body), 200, range(i64::MAX, i64::MAX), None),
            None
        );
    }

    #[test]
    fn validated_and_header() {
        assert_eq!(
            HlsByteSpan::validated(200, Some(100))
                .range_header()
                .as_deref(),
            Some("bytes=100-299")
        );
        assert_eq!(HlsByteSpan::validated(0, Some(0)), HlsByteSpan::Invalid);
        assert_eq!(HlsByteSpan::validated(-1, Some(0)), HlsByteSpan::Invalid);
        assert_eq!(HlsByteSpan::validated(10, Some(-1)), HlsByteSpan::Invalid);
        assert_eq!(HlsByteSpan::validated(10, None), HlsByteSpan::Invalid);
        assert_eq!(
            HlsByteSpan::validated(i64::MAX, Some(1)),
            HlsByteSpan::Invalid
        );
        assert_eq!(HlsByteSpan::Invalid.range_header(), None);
        assert_eq!(HlsByteSpan::Whole.range_header(), None);
    }

    #[test]
    fn content_range_and_final_failure() {
        assert!(HlsByteSpan::content_range_matches(
            "bytes 300-499/1000",
            300,
            200
        ));
        assert!(HlsByteSpan::content_range_matches(
            "bytes 300-499/*",
            300,
            200
        ));
        assert!(!HlsByteSpan::content_range_matches(
            "bytes 0-199/1000",
            300,
            200
        ));
        assert!(!HlsByteSpan::content_range_matches(
            "bytes */1000",
            300,
            200
        ));
        assert!(!HlsByteSpan::content_range_matches("garbage", 300, 200));
        let w = range(3, 2);
        assert_eq!(
            HlsByteSpan::recordable_body(Some(b"abc"), 206, w, Some("bytes 0-2/10")),
            None
        );
        assert_eq!(
            HlsByteSpan::recordable_body(Some(b"abc"), 206, w, Some("bytes 2-4/10")),
            Some(&b"abc"[..])
        );
        assert!(HlsByteSpan::is_final_failure(404) && HlsByteSpan::is_final_failure(403));
        assert!(!HlsByteSpan::is_final_failure(408) && !HlsByteSpan::is_final_failure(429));
        assert!(!HlsByteSpan::is_final_failure(500) && !HlsByteSpan::is_final_failure(200));
    }

    #[test]
    fn resolve_spans() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\n#EXT-X-BYTERANGE:100\na.mp4\n#EXTINF:2,\n#EXT-X-BYTERANGE:50@10\na.mp4\n#EXTINF:2,\n#EXT-X-BYTERANGE:20\na.mp4\n#EXTINF:2,\n#EXT-X-BYTERANGE:20\nb.mp4\n#EXTINF:2,\nplain.ts\n";
        let pl = HlsManifestParser::new()
            .parse_media_playlist(
                text,
                &Url::parse("https://example.test/live/x.m3u8").unwrap(),
            )
            .unwrap();
        let spans = HlsByteSpan::resolve(&pl.segments, |u| u.to_string());
        assert_eq!(
            spans,
            vec![
                HlsByteSpan::Invalid,
                HlsByteSpan::Range(range(50, 10).unwrap()),
                HlsByteSpan::Range(range(20, 60).unwrap()),
                HlsByteSpan::Invalid,
                HlsByteSpan::Whole,
            ]
        );
    }

    #[test]
    fn init_span() {
        let u = Url::parse("https://a.test/init.mp4").unwrap();
        assert_eq!(
            HlsByteSpan::of_init(&InitSegment {
                uri: u.clone(),
                byte_range: None
            }),
            HlsByteSpan::Whole
        );
        assert_eq!(
            HlsByteSpan::of_init(&InitSegment {
                uri: u,
                byte_range: Some(ByteRange {
                    length: 720,
                    offset: None
                })
            }),
            HlsByteSpan::Range(range(720, 0).unwrap())
        );
    }
}
