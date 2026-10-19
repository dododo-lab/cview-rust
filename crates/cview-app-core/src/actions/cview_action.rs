//! `Sources/CViewApp/Actions/CViewAction.swift` 포팅.
//!
//! Command Palette / DeepLink / Widget / AppIntent 가 공유하는 단일 액션 모델.

use serde::{Deserialize, Serialize};

use crate::navigation::hub_mode::FollowingHubMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveOpenMode {
    /// 단일 시청.
    Watch,
    /// 멀티라이브 추가.
    Multi,
    /// 채팅만.
    ChatOnly,
}

impl LiveOpenMode {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Multi => "multi",
            Self::ChatOnly => "chatOnly",
        }
    }
}

/// 모든 진입점이 만들어 [`super::registry::CViewActionRegistry::dispatch`] 에 넘기는 액션.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CViewAction {
    OpenLive {
        channel_id: String,
        mode: LiveOpenMode,
    },
    OpenChannel {
        channel_id: String,
    },
    AddToMultiLive {
        channel_id: String,
    },
    AddToMultiChat {
        channel_id: String,
    },
    SwitchLiveMode(FollowingHubMode),
    OpenSearch {
        query: Option<String>,
    },
    OpenClip {
        clip_uid: String,
    },
    OpenHome,
}

impl CViewAction {
    /// 로그·중복 제거용 안정 ID.
    pub fn id(&self) -> String {
        match self {
            Self::OpenLive { channel_id, mode } => {
                format!("openLive:{channel_id}:{}", mode.raw_value())
            }
            Self::OpenChannel { channel_id } => format!("openChannel:{channel_id}"),
            Self::AddToMultiLive { channel_id } => format!("addToMultiLive:{channel_id}"),
            Self::AddToMultiChat { channel_id } => format!("addToMultiChat:{channel_id}"),
            Self::SwitchLiveMode(m) => format!("switchLiveMode:{}", m.raw_value()),
            Self::OpenSearch { query } => format!("openSearch:{}", query.as_deref().unwrap_or("")),
            Self::OpenClip { clip_uid } => format!("openClip:{clip_uid}"),
            Self::OpenHome => "openHome".into(),
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::OpenLive { mode, .. } => match mode {
                LiveOpenMode::Watch => "라이브 시청",
                LiveOpenMode::Multi => "멀티라이브 추가",
                LiveOpenMode::ChatOnly => "채팅만 열기",
            },
            Self::OpenChannel { .. } => "채널 상세",
            Self::AddToMultiLive { .. } => "멀티라이브에 추가",
            Self::AddToMultiChat { .. } => "멀티채팅에 추가",
            Self::SwitchLiveMode(m) => match m {
                FollowingHubMode::Explore => "탐색 모드",
                FollowingHubMode::Watch => "시청 모드",
                FollowingHubMode::Multi => "멀티 모드",
                FollowingHubMode::Search => "검색 모드",
                FollowingHubMode::Popular => "인기 탭",
                FollowingHubMode::Category => "카테고리 탭",
            },
            Self::OpenSearch { .. } => "검색 열기",
            Self::OpenClip { .. } => "클립 재생",
            Self::OpenHome => "홈으로",
        }
    }

    /// 입력값을 검증해 정상화된 액션을 돌려주거나, 실패 시 None.
    pub fn validated(&self) -> Option<Self> {
        fn non_empty(s: &str) -> Option<String> {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        match self {
            Self::OpenLive { channel_id, mode } => non_empty(channel_id).map(|id| Self::OpenLive {
                channel_id: id,
                mode: *mode,
            }),
            Self::OpenChannel { channel_id } => {
                non_empty(channel_id).map(|id| Self::OpenChannel { channel_id: id })
            }
            Self::AddToMultiLive { channel_id } => {
                non_empty(channel_id).map(|id| Self::AddToMultiLive { channel_id: id })
            }
            Self::AddToMultiChat { channel_id } => {
                non_empty(channel_id).map(|id| Self::AddToMultiChat { channel_id: id })
            }
            Self::OpenClip { clip_uid } => {
                non_empty(clip_uid).map(|uid| Self::OpenClip { clip_uid: uid })
            }
            Self::OpenSearch { query } => Some(Self::OpenSearch {
                query: query.as_deref().and_then(non_empty),
            }),
            Self::SwitchLiveMode(_) | Self::OpenHome => Some(self.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_trims_and_rejects_blank() {
        assert_eq!(
            CViewAction::OpenLive {
                channel_id: "  ".into(),
                mode: LiveOpenMode::Watch
            }
            .validated(),
            None
        );
        assert_eq!(
            CViewAction::OpenChannel {
                channel_id: " a ".into()
            }
            .validated(),
            Some(CViewAction::OpenChannel {
                channel_id: "a".into()
            })
        );
        assert_eq!(
            CViewAction::OpenSearch {
                query: Some("   ".into())
            }
            .validated(),
            Some(CViewAction::OpenSearch { query: None })
        );
    }

    #[test]
    fn ids_are_stable() {
        assert_eq!(
            CViewAction::OpenLive {
                channel_id: "x".into(),
                mode: LiveOpenMode::ChatOnly
            }
            .id(),
            "openLive:x:chatOnly"
        );
        assert_eq!(
            CViewAction::SwitchLiveMode(FollowingHubMode::Multi).id(),
            "switchLiveMode:멀티"
        );
    }
}
