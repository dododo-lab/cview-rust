//! `Sources/CViewCore/Models/ChatSystemNoticeKind.swift` 포팅.

use serde::{Deserialize, Serialize};

/// 시스템 행의 의미. **만드는 쪽이 선언한다.**
///
/// 서버가 준 임의 문구(`.systemMessage`)는 생산 지점이 의미를 모르므로 `Info` 로 들어오고,
/// 뷰가 `Info` 일 때만 본문 매칭을 폴백으로 돌린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatSystemNoticeKind {
    /// 의미를 모르거나 단순 정보. 뷰가 본문 매칭 폴백을 돌린다.
    #[default]
    Info,
    /// 사용자가 놓치면 곤란한 제약 — 추방·제한·차단.
    Warning,
    /// 실패. 사용자의 행동이 반영되지 않았다.
    Error,
    /// 해제·완료처럼 제약이 풀린 경우.
    Positive,
}

impl ChatSystemNoticeKind {
    /// Swift `CaseIterable.allCases`.
    pub const ALL: [ChatSystemNoticeKind; 4] = [Self::Info, Self::Warning, Self::Error, Self::Positive];

    /// Swift raw value.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Positive => "positive",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_swift_raw_values() {
        for kind in ChatSystemNoticeKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.raw_value()));
            let back: ChatSystemNoticeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }
}
