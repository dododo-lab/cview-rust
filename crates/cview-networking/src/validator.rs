//! `Sources/CViewNetworking/ResponseValidator.swift` 포팅 — API 응답 구조 검증과 방어적 디코딩.

use cview_core::error::ApiError;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// 구조 검증 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub warnings: Vec<String>,
}

/// 응답 데이터를 디코딩한다. 실패했을 때만 구조 검증으로 진단 경고를 남긴다(성공 경로 1회 파싱).
///
/// - 빈 데이터 → [`ApiError::MalformedResponse`]
/// - 디코딩 실패 → [`ApiError::DecodingFailed`] (상세 문구는 로그·진단용)
pub fn validate_and_decode<T: DeserializeOwned>(data: &[u8]) -> Result<T, ApiError> {
    if data.is_empty() {
        tracing::error!("ResponseValidator: 빈 응답 데이터");
        return Err(ApiError::MalformedResponse(
            "응답 데이터가 비어있습니다".into(),
        ));
    }
    match serde_json::from_slice::<T>(data) {
        Ok(v) => Ok(v),
        Err(e) => {
            let detail = describe_decoding_error(&e);
            tracing::error!("ResponseValidator: 디코딩 실패 — {detail}");
            for warning in validate_json_structure(data).warnings {
                tracing::warn!("ResponseValidator: {warning}");
            }
            let raw = String::from_utf8_lossy(&data[..data.len().min(500)]);
            tracing::debug!("ResponseValidator: raw(500자 제한) = {raw}");
            Err(ApiError::DecodingFailed(detail))
        }
    }
}

/// 치지직 표준 응답(`code`, `message`, `content`) 구조를 확인한다.
pub fn validate_json_structure(data: &[u8]) -> ValidationResult {
    let Ok(Value::Object(json)) = serde_json::from_slice::<Value>(data) else {
        return ValidationResult {
            is_valid: false,
            warnings: vec!["유효한 JSON 객체가 아닙니다".into()],
        };
    };
    let mut warnings = Vec::new();
    let code = json.get("code").and_then(Value::as_i64);
    match json.get("code") {
        None => warnings.push("'code' 필드 누락".to_string()),
        Some(_) => {
            if let Some(c) = code.filter(|c| *c != 200) {
                warnings.push(format!("API 응답 code={c} (비정상)"));
            }
        }
    }
    // Swift `json["content"] == nil` — NSNull 은 nil 이 아니므로 키 누락만 경고한다.
    if !json.contains_key("content") {
        warnings.push("'content' 필드가 null 또는 누락".to_string());
    }
    if let Some(message) = json.get("message").and_then(Value::as_str) {
        if !message.is_empty() && code != Some(200) {
            warnings.push(format!("API 메시지: {message}"));
        }
    }
    if json.is_empty() {
        return ValidationResult {
            is_valid: false,
            warnings: vec!["빈 JSON 객체".into()],
        };
    }
    ValidationResult {
        is_valid: true,
        warnings,
    }
}

/// serde 오류를 사람이 읽기 쉬운 문자열로 변환.
fn describe_decoding_error(error: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let kind = match error.classify() {
        Category::Data => "타입 불일치",
        Category::Syntax => "데이터 손상",
        Category::Eof => "데이터 손상(EOF)",
        Category::Io => "입출력 오류",
    };
    format!(
        "{kind} at line {} column {} — {error}",
        error.line(),
        error.column()
    )
}

#[cfg(test)]
mod tests {
    //! `ResponseValidatorTests.swift` 포팅.
    use super::*;
    use crate::response::ChzzkResponse;

    #[test]
    fn empty_data_is_malformed() {
        let r = validate_and_decode::<ChzzkResponse<i64>>(b"");
        assert!(matches!(r, Err(ApiError::MalformedResponse(_))));
    }

    #[test]
    fn invalid_json_is_decoding_failure() {
        let r = validate_and_decode::<ChzzkResponse<i64>>(b"not json");
        assert!(matches!(r, Err(ApiError::DecodingFailed(_))));
    }

    #[test]
    fn valid_response_decodes() {
        let r = validate_and_decode::<ChzzkResponse<i64>>(
            br#"{"code":200,"message":null,"content":3}"#,
        )
        .unwrap();
        assert_eq!(r.content, Some(3));
    }

    #[test]
    fn structure_warnings() {
        let ok = validate_json_structure(br#"{"code":200,"message":null,"content":{}}"#);
        assert!(ok.is_valid);
        assert!(ok.warnings.is_empty());

        let bad = validate_json_structure(r#"{"code":9004,"message":"앱 업데이트"}"#.as_bytes());
        assert!(bad.is_valid);
        assert!(bad.warnings.iter().any(|w| w.contains("code=9004")));
        assert!(bad.warnings.iter().any(|w| w.contains("content")));
        assert!(bad.warnings.iter().any(|w| w.contains("앱 업데이트")));

        let missing_code = validate_json_structure(br#"{"content":1}"#);
        assert!(missing_code
            .warnings
            .iter()
            .any(|w| w.contains("'code' 필드 누락")));

        assert!(!validate_json_structure(b"[]").is_valid);
        assert!(!validate_json_structure(b"{}").is_valid);
    }
}
