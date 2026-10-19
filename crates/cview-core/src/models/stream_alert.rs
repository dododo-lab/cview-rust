//! `Sources/CViewCore/Models/StreamAlertItem.swift` 포팅 — 플레이어 오버레이 알림 아이템.
//!
//! `init?(from: ChatMessageItem)` 은 채팅 모델(별도 포팅)에 의존하므로, 그 분기 규칙만
//! [`StreamAlertItem::donation`] · [`StreamAlertItem::subscription`] 으로 옮겼다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 스트림 알림 유형.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamAlertType {
    /// 일반 채팅 후원
    Donation,
    /// 영상 후원
    VideoDonation,
    /// 미션 후원
    MissionDonation,
    /// 구독
    Subscription,
    /// 공지
    Notice,
    /// 시스템 메시지
    SystemMessage,
}

/// 플레이어 화면 위에 떠오르는 알림(후원, 구독, 시스템 등).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamAlertItem {
    pub id: String,
    pub alert_type: StreamAlertType,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    /// 후원 전용
    pub donation_amount: Option<i64>,
    /// `"CHAT"` / `"VIDEO"` / `"MISSION"`
    pub donation_type: Option<String>,
    /// 구독 전용
    pub subscription_months: Option<i64>,
}

impl StreamAlertItem {
    pub fn new(
        alert_type: StreamAlertType,
        nickname: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            alert_type,
            nickname: nickname.into(),
            content: content.into(),
            timestamp: Utc::now(),
            donation_amount: None,
            donation_type: None,
            subscription_months: None,
        }
    }

    /// 후원 메시지 → 알림. `donation_type` 이 없으면 `"CHAT"`; VIDEO/MISSION 은 전용 유형.
    pub fn donation(
        nickname: impl Into<String>,
        content: impl Into<String>,
        amount: Option<i64>,
        donation_type: Option<&str>,
    ) -> Self {
        let dtype = donation_type.unwrap_or("CHAT");
        let alert_type = match dtype {
            "VIDEO" => StreamAlertType::VideoDonation,
            "MISSION" => StreamAlertType::MissionDonation,
            _ => StreamAlertType::Donation,
        };
        Self {
            donation_amount: amount,
            donation_type: Some(dtype.to_string()),
            ..Self::new(alert_type, nickname, content)
        }
    }

    /// 구독 메시지 → 알림.
    pub fn subscription(
        nickname: impl Into<String>,
        content: impl Into<String>,
        months: Option<i64>,
    ) -> Self {
        Self {
            subscription_months: months,
            ..Self::new(StreamAlertType::Subscription, nickname, content)
        }
    }

    /// 공지 → 알림.
    pub fn notice(nickname: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(StreamAlertType::Notice, nickname, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn donation_kinds() {
        assert_eq!(
            StreamAlertItem::donation("n", "c", Some(1000), None).alert_type,
            StreamAlertType::Donation
        );
        let v = StreamAlertItem::donation("n", "c", Some(1000), Some("VIDEO"));
        assert_eq!(v.alert_type, StreamAlertType::VideoDonation);
        assert_eq!(v.donation_type.as_deref(), Some("VIDEO"));
        assert_eq!(
            StreamAlertItem::donation("n", "c", None, Some("MISSION")).alert_type,
            StreamAlertType::MissionDonation
        );
        assert_eq!(
            StreamAlertItem::subscription("n", "", Some(3)).subscription_months,
            Some(3)
        );
        assert_eq!(
            serde_json::to_string(&StreamAlertType::VideoDonation).unwrap(),
            "\"videoDonation\""
        );
        assert_ne!(
            StreamAlertItem::notice("a", "b").id,
            StreamAlertItem::notice("a", "b").id
        );
    }
}
