//! Live/MultiLive 화면의 폭 기반 반응형 룰.
//!
//! 원본: `Sources/CViewCore/Logic/LiveResponsiveBreakpoints.swift`
//!
//! 디자인 문서의 860/1080/1320pt 분기점을 형식화한다. `chatMinWidth` 는 원본에서
//! `DesignTokens.Layout.workspaceChatDockMinWidth`(280) 를 읽는다 — 디자인 시스템은 UI 단계에서
//! 옮기므로 값을 [`WORKSPACE_CHAT_DOCK_MIN_WIDTH`] 로 고정해 둔다.

/// `DesignTokens.Layout.workspaceChatDockMinWidth`.
pub const WORKSPACE_CHAT_DOCK_MIN_WIDTH: f64 = 280.0;

/// 라이브 화면(단일/멀티)의 컨테이너 폭 분기점.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveBreakpoint {
    /// ~860pt 미만 — 스테이지 우선, 채팅은 overlay 권장.
    Compact,
    /// 860 ~ 1080pt — 사이드 채팅 280pt 권장.
    Regular,
    /// 1080 ~ 1320pt — 사이드 채팅 320pt.
    Wide,
    /// 1320pt 이상 — 사이드 채팅 360pt.
    Xwide,
}

impl LiveBreakpoint {
    pub const COMPACT_MAX: f64 = 860.0;
    pub const REGULAR_MAX: f64 = 1080.0;
    pub const WIDE_MAX: f64 = 1320.0;

    pub fn from_width(width: f64) -> Self {
        if width < Self::COMPACT_MAX {
            Self::Compact
        } else if width < Self::REGULAR_MAX {
            Self::Regular
        } else if width < Self::WIDE_MAX {
            Self::Wide
        } else {
            Self::Xwide
        }
    }

    /// 사이드 채팅 권장 기본 폭(저장값 없을 때).
    pub fn default_chat_width(self) -> f64 {
        match self {
            Self::Compact => 260.0,
            Self::Regular => 280.0,
            Self::Wide => 320.0,
            Self::Xwide => 360.0,
        }
    }

    /// 사이드 채팅이 컨테이너 폭에서 차지할 수 있는 최대 비율.
    pub fn chat_max_ratio(self) -> f64 {
        match self {
            Self::Compact => 0.30,
            Self::Regular => 0.40,
            Self::Wide => 0.45,
            Self::Xwide => 0.50,
        }
    }

    /// 사이드 채팅 최소 폭.
    pub fn chat_min_width(self) -> f64 {
        WORKSPACE_CHAT_DOCK_MIN_WIDTH
    }

    /// compact 진입 시 side 채팅을 overlay 로 폴백할지(스테이지 우선 strategy).
    pub fn prefers_stage_over_chat(self) -> bool {
        self == Self::Compact
    }

    /// 디버그/UI 표기용 라벨.
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Regular => "regular",
            Self::Wide => "wide",
            Self::Xwide => "xwide",
        }
    }

    /// 채팅 폭을 `[chat_min_width, chat_max_ratio * container]` 로 클램프.
    pub fn clamp_chat_width(self, requested: f64, container_width: f64) -> f64 {
        let upper = self
            .chat_min_width()
            .max(container_width * self.chat_max_ratio());
        requested.max(self.chat_min_width()).min(upper)
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/LiveBreakpointTests.swift`
    use super::LiveBreakpoint as B;

    #[test]
    fn boundaries() {
        for (w, e) in [
            (0.0, B::Compact),
            (600.0, B::Compact),
            (859.0, B::Compact),
            (860.0, B::Regular),
            (1000.0, B::Regular),
            (1079.0, B::Regular),
            (1080.0, B::Wide),
            (1200.0, B::Wide),
            (1319.0, B::Wide),
            (1320.0, B::Xwide),
            (2560.0, B::Xwide),
        ] {
            assert_eq!(B::from_width(w), e, "{w}");
        }
    }

    #[test]
    fn default_chat_width_values() {
        assert_eq!(B::Compact.default_chat_width(), 260.0);
        assert_eq!(B::Regular.default_chat_width(), 280.0);
        assert_eq!(B::Wide.default_chat_width(), 320.0);
        assert_eq!(B::Xwide.default_chat_width(), 360.0);
    }

    #[test]
    fn chat_max_ratio_ordering() {
        assert!(B::Compact.chat_max_ratio() < B::Regular.chat_max_ratio());
        assert!(B::Regular.chat_max_ratio() < B::Wide.chat_max_ratio());
        assert!(B::Wide.chat_max_ratio() < B::Xwide.chat_max_ratio());
    }

    #[test]
    fn clamp_chat_width() {
        let bp = B::Regular;
        assert_eq!(
            bp.clamp_chat_width(800.0, 1000.0),
            1000.0 * bp.chat_max_ratio()
        );
        assert_eq!(bp.clamp_chat_width(50.0, 1000.0), bp.chat_min_width());
        assert_eq!(bp.clamp_chat_width(300.0, 1000.0), 300.0);
    }

    #[test]
    fn prefers_stage_over_chat() {
        assert!(B::Compact.prefers_stage_over_chat());
        assert!(!B::Regular.prefers_stage_over_chat());
        assert!(!B::Wide.prefers_stage_over_chat());
        assert!(!B::Xwide.prefers_stage_over_chat());
    }
}
