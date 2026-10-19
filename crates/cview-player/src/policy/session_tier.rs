//! `Sources/CViewPlayer/SessionTier.swift` 포팅 — 멀티라이브 세션 계층.

/// 멀티라이브 리소스 배분을 위한 세션 계층. 값이 작을수록 우선.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum SessionTier {
    /// 선택된 포그라운드 세션.
    Active = 0,
    /// 그리드에 보이지만 비선택.
    Visible = 1,
    /// 화면에 보이지 않음.
    Hidden = 2,
}

impl SessionTier {
    pub fn raw_value(self) -> i32 {
        self as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering() {
        assert!(SessionTier::Active < SessionTier::Visible);
        assert!(SessionTier::Visible < SessionTier::Hidden);
        assert_eq!(SessionTier::Hidden.raw_value(), 2);
    }
}
