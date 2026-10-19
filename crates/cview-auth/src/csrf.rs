//! `Sources/CViewAuth/OAuthCSRF.swift` 포팅 — OAuth CSRF(state) 검증.
//!
//! 콜백으로 돌아온 state 는 "우리가 실제로 연 authURL 에 담아 보낸 state" 와 정확히 같아야 한다.
//! 둘 중 하나라도 없거나 비었거나 다르면 거부한다. authURL 은 호출자(웹뷰)의 안정적인 상태이므로
//! 서비스의 가변 state 대신 이것을 단일 기준으로 삼는다.

use url::Url;

/// 검증 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfResult {
    /// 통과 여부.
    pub ok: bool,
    /// 우리가 보낸 state (authURL 에서 추출). 토큰 교환에 재사용한다.
    pub sent_state: Option<String>,
}

/// URL 쿼리의 첫 `name` 값.
pub(crate) fn query_value(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// 우리가 연 authURL 의 `state` 와 콜백 `state` 를 비교한다.
pub fn validate(opened_url: Option<&Url>, callback_state: Option<&str>) -> CsrfResult {
    let sent_state = opened_url.and_then(|u| query_value(u, "state"));
    let ok = match (&sent_state, callback_state) {
        (Some(sent), Some(received)) => !sent.is_empty() && sent == received,
        _ => false,
    };
    CsrfResult { ok, sent_state }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAuthTests/OAuthCSRFTests.swift`
    use super::*;

    fn auth_url(state: Option<&str>) -> Url {
        let mut s =
            "https://chzzk.naver.com/account-interlock?clientId=abc&redirectUri=x".to_string();
        if let Some(state) = state {
            s.push_str(&format!("&state={state}"));
        }
        Url::parse(&s).unwrap()
    }

    #[test]
    fn matches() {
        let r = validate(Some(&auth_url(Some("ABC-123"))), Some("ABC-123"));
        assert!(r.ok);
        assert_eq!(r.sent_state.as_deref(), Some("ABC-123"));
    }

    #[test]
    fn mismatch_rejected() {
        let r = validate(Some(&auth_url(Some("ABC-123"))), Some("DIFFERENT"));
        assert!(!r.ok);
        assert_eq!(r.sent_state.as_deref(), Some("ABC-123"));
    }

    #[test]
    fn nil_callback_rejected() {
        assert!(!validate(Some(&auth_url(Some("ABC-123"))), None).ok);
    }

    #[test]
    fn nil_url_rejected() {
        let r = validate(None, Some("ABC-123"));
        assert!(!r.ok);
        assert_eq!(r.sent_state, None);
    }

    #[test]
    fn url_without_state_rejected() {
        let r = validate(Some(&auth_url(None)), Some("ABC-123"));
        assert!(!r.ok);
        assert_eq!(r.sent_state, None);
    }

    #[test]
    fn empty_state_rejected() {
        assert!(!validate(Some(&auth_url(Some(""))), Some("")).ok);
    }
}
