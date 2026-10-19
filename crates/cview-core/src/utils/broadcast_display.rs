//! 극단 데이터(빈 제목·빈 카테고리·오프라인)를 화면이 뭐라고 부를지의 단일 출처.
//!
//! 원본: `Sources/CViewCore/Utilities/BroadcastDisplayPolicy.swift`
//! (`LiveChannelItem.uncategorizedDisplayName` / `displayCategoryName` 포함)
//!
//! ⚠ 문자열 값을 바꾸지 말 것 — `"분류 없음"` 은 통계 스냅샷에 그대로 저장된다.
//! ⚠ trim 을 넣지 말 것 — 카테고리 버킷 키 생성과 같은 커밋에서만 함께 바꾼다.

/// 카테고리가 없을 때의 표시명. 영속 키로도 쓰이므로 값 고정.
pub const UNCATEGORIZED: &str = "분류 없음";

/// 제목이 없을 때의 표시명.
pub const UNTITLED_BROADCAST: &str = "제목 없음";

/// 오프라인 표기.
pub const OFFLINE: &str = "오프라인";

/// 오프라인 표기 기호(SF Symbols 이름).
pub const OFFLINE_SYMBOL: &str = "moon.zzz";

/// 오프라인 카드의 썸네일 감쇠. ⚠ 썸네일 이미지 레이어에만 건다(라벨·컨트롤 금지).
pub const OFFLINE_THUMBNAIL_OPACITY: f64 = 0.72;

/// 빈/없는 카테고리를 표시용 라벨로.
pub fn category(raw: Option<&str>) -> &str {
    match raw {
        Some(s) if !s.is_empty() => s,
        _ => UNCATEGORIZED,
    }
}

/// 빈/없는 제목을 "제목 없음" 으로.
pub fn title(raw: Option<&str>) -> &str {
    title_or(raw, UNTITLED_BROADCAST)
}

/// 제목이 없을 때 `fallback`(보통 채널명)으로 대신하는 변형 — 히어로·큰 카드용.
pub fn title_or<'a>(raw: Option<&'a str>, fallback: &'a str) -> &'a str {
    match raw {
        Some(s) if !s.is_empty() => s,
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallbacks() {
        assert_eq!(category(None), "분류 없음");
        assert_eq!(category(Some("")), "분류 없음");
        assert_eq!(category(Some(" ")), " ", "trim 하지 않는다");
        assert_eq!(category(Some("게임")), "게임");
        assert_eq!(title(None), "제목 없음");
        assert_eq!(title(Some("")), "제목 없음");
        assert_eq!(title(Some("방송")), "방송");
        assert_eq!(title_or(Some(""), "채널"), "채널");
        assert_eq!(title_or(Some("방송"), "채널"), "방송");
    }
}
