//! NAVER 인증 쿠키 도메인 정책.
//!
//! 원본: `Sources/CViewCore/Utilities/NaverCookiePolicy.swift`

use super::media_url_policy::is_valid_dns_name;

/// 쿠키 도메인이 `naver.com` 또는 그 하위 도메인인가(앞의 `.` 하나 허용, DNS 레이블 검사).
pub fn matches_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    let host = lower.strip_prefix('.').unwrap_or(&lower);
    if !(host == "naver.com" || host.ends_with(".naver.com")) {
        return false;
    }
    is_valid_dns_name(host)
}

/// `NID_AUT`/`NID_SES` 이고 NAVER 도메인인가. 원본은 `HTTPCookie` 를 받는다.
pub fn is_authentication_cookie(name: &str, domain: &str) -> bool {
    (name == "NID_AUT" || name == "NID_SES") && matches_domain(domain)
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/NaverCookiePolicyTests.swift`
    use super::*;

    #[test]
    fn accepts_naver() {
        for d in [
            "naver.com",
            ".naver.com",
            "nid.naver.com",
            ".CHZZK.NAVER.COM",
        ] {
            assert!(matches_domain(d), "{d}");
        }
    }

    #[test]
    fn rejects_other_domains() {
        for d in [
            "notnaver.com",
            "naver.com.example.invalid",
            ".naver.com.evil",
            "..naver.com",
            "a..naver.com",
            "naver.com.",
            "evil/naver.com",
            "-x.naver.com",
            "",
            " naver.com",
        ] {
            assert!(!matches_domain(d), "{d:?}");
        }
    }

    #[test]
    fn auth_cookie_names() {
        assert!(is_authentication_cookie("NID_AUT", ".naver.com"));
        assert!(is_authentication_cookie("NID_SES", "nid.naver.com"));
        assert!(!is_authentication_cookie("NID_JKL", ".naver.com"));
        assert!(!is_authentication_cookie("NID_AUT", ".evil.com"));
    }
}
