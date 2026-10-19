//! `Sources/CViewApp/Logic/PinnedCategoryDecoder.swift` 포팅.
//!
//! `category.pinnedCategories` JSON 원문 → 집합 디코딩을 원문 문자열로 메모한다. 키가 원문
//! 그 자체라 값이 바뀌면 메모는 자동으로 무효가 된다.

use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct PinnedCategoryDecoder {
    memo_raw: Option<String>,
    memo_value: HashSet<String>,
    decode_count: usize,
}

impl PinnedCategoryDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 실제로 JSON 디코딩을 수행한 횟수.
    pub fn decode_count(&self) -> usize {
        self.decode_count
    }

    pub fn decode(&mut self, raw: &str) -> HashSet<String> {
        if self.memo_raw.as_deref() == Some(raw) {
            return self.memo_value.clone();
        }
        self.decode_count += 1;
        // 손상된 값은 "핀 없음" 으로 읽는다.
        let decoded: HashSet<String> = serde_json::from_str::<Vec<String>>(raw)
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
        self.memo_raw = Some(raw.to_string());
        self.memo_value = decoded.clone();
        decoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memoizes_by_raw_string() {
        let mut d = PinnedCategoryDecoder::new();
        assert_eq!(d.decode(r#"["a","b"]"#).len(), 2);
        d.decode(r#"["a","b"]"#);
        assert_eq!(d.decode_count(), 1);
        assert!(d.decode("not json").is_empty());
        assert_eq!(d.decode_count(), 2);
        assert!(d.decode("[]").is_empty());
    }
}
