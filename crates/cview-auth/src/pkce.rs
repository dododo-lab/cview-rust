//! `Sources/CViewAuth/PKCEChallenge.swift` 포팅 — RFC 7636 Proof Key for Code Exchange.
//!
//! 1. 앱이 고엔트로피 `code_verifier`(CSPRNG 32바이트 → base64url 43자)를 만든다.
//! 2. `code_challenge = BASE64URL_NO_PAD(SHA256(code_verifier))`.
//! 3. authorize 요청에 `codeChallenge` / `codeChallengeMethod=S256` 를 싣는다.
//! 4. 토큰 교환에 `codeVerifier` 를 싣는다.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 한 번의 OAuth 흐름 동안 사는 PKCE 쌍.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceChallenge {
    /// 토큰 교환 때 보내는 verifier (RFC 7636 §4.1 unreserved 문자만).
    pub code_verifier: String,
    /// authorize 요청의 challenge (S256).
    pub code_challenge: String,
    /// 항상 `"S256"` — `plain` 은 쓰지 않는다.
    pub code_challenge_method: String,
}

impl PkceChallenge {
    pub fn new(code_verifier: impl Into<String>) -> Self {
        let code_verifier = code_verifier.into();
        let code_challenge = Self::compute_s256_challenge(&code_verifier);
        Self {
            code_verifier,
            code_challenge,
            code_challenge_method: "S256".into(),
        }
    }

    /// CSPRNG 로 새 verifier 를 만든다.
    pub fn generate() -> Self {
        Self::new(Self::generate_verifier(32))
    }

    /// 최소 32바이트 CSPRNG → base64url-no-pad (32바이트면 43자).
    pub fn generate_verifier(byte_count: usize) -> String {
        let mut bytes = vec![0u8; byte_count.max(32)];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self::base64_url_encode_no_pad(&bytes)
    }

    /// `BASE64URL_NO_PAD(SHA256(verifier))` — RFC 7636 §4.2 S256.
    pub fn compute_s256_challenge(verifier: &str) -> String {
        let digest = Sha256::digest(verifier.as_bytes());
        Self::base64_url_encode_no_pad(&digest)
    }

    /// RFC 4648 §5 base64url, 패딩 없음.
    pub fn base64_url_encode_no_pad(data: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 Appendix B (원본은 DEBUG 자체 검사 `_rfc7636AppendixBSelfCheck`).
    #[test]
    fn rfc7636_appendix_b() {
        let p = PkceChallenge::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            p.code_challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(p.code_challenge_method, "S256");
    }

    #[test]
    fn generated_verifier_is_unreserved_and_unique() {
        let a = PkceChallenge::generate();
        let b = PkceChallenge::generate();
        assert_eq!(a.code_verifier.len(), 43);
        assert!(a
            .code_verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
        assert_ne!(a.code_verifier, b.code_verifier);
        assert_eq!(
            PkceChallenge::generate_verifier(8).len(),
            43,
            "최소 32바이트"
        );
        assert_eq!(PkceChallenge::generate_verifier(64).len(), 86);
    }

    #[test]
    fn base64url_alphabet() {
        assert_eq!(
            PkceChallenge::base64_url_encode_no_pad(&[0xfb, 0xff]),
            "-_8"
        );
    }
}
