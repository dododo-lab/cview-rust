//! 치지직 인증 마크(파트너·챌린저·공식) — 응답의 `verifiedMark` + `verifiedMarkType`.
//!
//! 치지직 웹은 `verifiedMarkType` 이 `PARTNER` / `CHALLENGER` / `OFFICIAL` 일 때만 마크를 그린다.
//! 예전 응답처럼 `verifiedMark: true` 인데 종류가 비어 있으면 챌린저 제도 전의 파트너 인증이므로
//! 파트너로 본다.

/// 화면에 보내는 마크 종류 — `"partner"` · `"challenger"` · `"official"`. 인증이 없으면 `None`.
pub fn verified_badge(verified_mark: bool, mark_type: Option<&str>) -> Option<&'static str> {
    match mark_type.map(str::trim) {
        Some(t) if t.eq_ignore_ascii_case("PARTNER") => Some("partner"),
        Some(t) if t.eq_ignore_ascii_case("CHALLENGER") => Some("challenger"),
        Some(t) if t.eq_ignore_ascii_case("OFFICIAL") => Some("official"),
        _ if verified_mark => Some("partner"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::verified_badge;

    #[test]
    fn maps_mark_types() {
        assert_eq!(verified_badge(true, Some("PARTNER")), Some("partner"));
        assert_eq!(verified_badge(true, Some("CHALLENGER")), Some("challenger"));
        assert_eq!(verified_badge(false, Some("OFFICIAL")), Some("official"));
        assert_eq!(verified_badge(true, None), Some("partner"));
        assert_eq!(verified_badge(true, Some("NEW_KIND")), Some("partner"));
        assert_eq!(verified_badge(false, None), None);
        assert_eq!(verified_badge(false, Some("")), None);
    }
}
