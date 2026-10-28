//! 채팅 모델이 쓰는 작은 보조 함수.
//!
//! Swift 의 `UUID().uuidString` 과 `URL(string:)` 대체물. cview-core 에 `uuid`/`url`
//! 의존성을 추가하지 않기 위해 여기서 최소 구현한다.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_u64() -> u64 {
    // RandomState 는 프로세스마다 무작위 키를 갖는다. 카운터·시각을 섞어 호출마다 다른 값.
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    h.write_u64(nanos);
    h.finish()
}

/// `UUID().uuidString` 과 같은 형식(대문자 하이픈 구분 v4)의 무작위 식별자.
pub fn new_uuid_string() -> String {
    let hi = random_u64();
    let lo = random_u64();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_be_bytes());
    bytes[8..].copy_from_slice(&lo.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// `URL(string:)` 이 nil 을 돌려줄 문자열을 거른다(근사).
///
/// Foundation 은 빈 문자열·공백·제어문자가 섞인 문자열을 URL 로 받지 않는다.
/// 완전한 RFC 3986 검증이 아니라, 채팅 페이로드에서 실제로 문제가 된 경우
/// (빈 값, `"<null>"` 같은 공백 없는 가짜 값은 통과 — Swift 도 통과시킨다)를 맞춘다.
pub fn is_plausible_url(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_has_canonical_shape_and_is_unique() {
        let a = new_uuid_string();
        let b = new_uuid_string();
        assert_eq!(a.len(), 36);
        assert_eq!(a.chars().filter(|c| *c == '-').count(), 4);
        assert_eq!(&a[14..15], "4");
        assert_ne!(a, b);
    }

    #[test]
    fn url_plausibility() {
        assert!(is_plausible_url("https://example.com/a.png"));
        assert!(!is_plausible_url(""));
        assert!(!is_plausible_url("has space"));
    }
}
