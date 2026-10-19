//! `Sources/CViewCore/Models/LiveCatalogSummary.swift` · `LiveCatalogRowsSnapshot.swift` 포팅 —
//! 전수 수집(live catalog) 행·품질·요약 DTO.
//!
//! 계산(빌더)은 `Utilities/LiveCatalogSummaryBuilder.swift` 몫이라 여기 없다. 행 스냅샷의 지문
//! (`MenuSnapshotFingerprinting`)은 유틸 의존이라 포팅하지 않았다.
//! 날짜는 `chzzk_date` 형식(RFC3339 로 쓰고, 문자열·Swift 기준 시각 숫자 모두 읽음)으로 저장한다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::decode::{chzzk_date, chzzk_date_opt};
use super::live::LiveInfo;
use super::live_channel_item::LiveChannelItem;

/// 전수 수집의 경량 라이브 행. 수천 개가 저장되므로 키를 한 글자로 줄인다.
///
/// ⚠ serde 키 문자열은 저장 형식이다. 바꾸면 이전 파일을 읽지 못한다.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveCatalogRow {
    #[serde(rename = "l")]
    pub live_id: i64,
    /// 비어 있을 수 있다(응답에 channel 이 없을 때). 화면 행 id 는 그때 liveId 문자열이다.
    #[serde(rename = "c")]
    pub channel_id: String,
    #[serde(rename = "n")]
    pub channel_name: String,
    #[serde(rename = "i", default, skip_serializing_if = "Option::is_none")]
    pub channel_image_url: Option<String>,
    #[serde(rename = "t")]
    pub live_title: String,
    #[serde(rename = "v")]
    pub viewer_count: i64,
    /// 원시 카테고리 값(`LiveInfo.liveCategoryValue`).
    #[serde(rename = "k", default, skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    /// 카테고리 식별자(`LiveInfo.liveCategory`).
    #[serde(rename = "g", default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// "GAME" | "SPORTS" | "ETC" …
    #[serde(rename = "y", default, skip_serializing_if = "Option::is_none")]
    pub category_type: Option<String>,
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(
        rename = "o",
        default,
        with = "chzzk_date_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub open_date: Option<DateTime<Utc>>,
}

impl From<&LiveInfo> for LiveCatalogRow {
    /// 전수 수집 응답 한 건 → 행.
    fn from(info: &LiveInfo) -> Self {
        Self {
            live_id: info.live_id,
            channel_id: info
                .channel
                .as_ref()
                .map(|c| c.channel_id.clone())
                .unwrap_or_default(),
            channel_name: info
                .channel
                .as_ref()
                .map(|c| c.channel_name.clone())
                .unwrap_or_else(|| "Unknown".into()),
            channel_image_url: info
                .channel
                .as_ref()
                .and_then(|c| c.channel_image_url.as_ref())
                .map(|u| u.to_string()),
            live_title: info.live_title.clone(),
            viewer_count: info.concurrent_user_count,
            category_name: info.live_category_value.clone(),
            category_id: info.live_category.clone(),
            category_type: info.category_type.clone(),
            thumbnail_url: info.live_image_url.as_ref().map(|u| u.to_string()),
            open_date: info.open_date,
        }
    }
}

impl LiveCatalogRow {
    /// 화면 행 id — channelId, 비었으면 liveId 문자열. 중복 제거 키이기도 하다.
    pub fn item_id(&self) -> String {
        if self.channel_id.is_empty() {
            self.live_id.to_string()
        } else {
            self.channel_id.clone()
        }
    }

    /// 표시·집계 키(`LiveChannelItem::display_category_name`).
    pub fn category_key(&self) -> String {
        LiveChannelItem::display_category_name(self.category_name.as_deref())
    }

    /// 화면 행. 전수 수집 행은 모두 방송 중이다.
    pub fn item(&self) -> LiveChannelItem {
        LiveChannelItem {
            id: self.item_id(),
            channel_name: self.channel_name.clone(),
            channel_image_url: self.channel_image_url.clone(),
            live_title: self.live_title.clone(),
            viewer_count: self.viewer_count,
            category_name: self.category_name.clone(),
            category_type: self.category_type.clone(),
            thumbnail_url: self.thumbnail_url.clone(),
            channel_id: self.channel_id.clone(),
            is_live: true,
            open_date: self.open_date,
        }
    }
}

/// 수집 결과를 얼마나 믿을 수 있는지. Swift 합성 Codable 과 같은 `{"verified":{"exact":N}}` 형태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveCatalogQuality {
    /// 한 번의 순회가 총계 검증을 통과했다.
    Verified { exact: i64 },
    /// 체크포인트 이어받기 결과가 이어 붙이기 검증을 통과했다.
    Stitched { observed: i64 },
    /// 검증하지 못한 관측값 — 증가율 기준선에는 기록하지 않는다.
    Observed { count: i64, reason: String },
}

impl LiveCatalogQuality {
    /// 검증 통과본인지 — 완료본 승격과 기준선 기록의 조건.
    pub fn is_verified_or_stitched(&self) -> bool {
        !matches!(self, LiveCatalogQuality::Observed { .. })
    }

    /// 총계 검증값. `Verified` 에만 있다.
    pub fn exact_count(&self) -> Option<i64> {
        match self {
            LiveCatalogQuality::Verified { exact } => Some(*exact),
            _ => None,
        }
    }
}

/// 유형별 몫. 유형이 없는 행은 "ETC" 로 센다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeShare {
    pub r#type: String,
    pub live_count: i64,
    pub viewer_total: i64,
}

/// 카테고리 한 줄.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    /// `display_category_name(원시값)` — 기준선 키와 같은 문자열.
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub live_count: i64,
    pub viewer_total: i64,
    /// 대표 방송.
    pub leader: LiveCatalogRow,
    /// 시청자순 상위 channelId(최대 3).
    pub top_channel_ids: Vec<String>,
    /// 1시간 기준선 대비 시청자 합 증가율(%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth_percent: Option<f64>,
}

impl CategorySummary {
    pub fn id(&self) -> &str {
        &self.key
    }
}

/// 전수 수집 한 번의 요약 (catalog-summary 파일 본문).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCatalogSummary {
    /// 저장 형식 버전(키 "v").
    #[serde(rename = "v")]
    pub version: i64,
    pub sweep_id: Uuid,
    #[serde(with = "chzzk_date")]
    pub completed_at: DateTime<Utc>,
    pub quality: LiveCatalogQuality,
    pub live_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_live_count: Option<i64>,
    pub viewer_total: i64,
    pub average_viewers: i64,
    pub median_viewers: i64,
    /// 시청자 구간별 행 수(4칸).
    pub viewer_buckets: Vec<i64>,
    pub type_shares: Vec<TypeShare>,
    /// 시청자순 상위 행(최대 40).
    pub top_lives: Vec<LiveCatalogRow>,
    pub categories: Vec<CategorySummary>,
}

impl LiveCatalogSummary {
    pub const CURRENT_VERSION: i64 = 1;
    /// 시청자 구간 경계(미만). 4칸: [0,100) · [100,1K) · [1K,1만) · [1만,∞).
    pub const VIEWER_BUCKET_UPPER_BOUNDS: [i64; 3] = [100, 1_000, 10_000];
    pub const VIEWER_BUCKET_LABELS: [&'static str; 4] = ["0~100", "100~1K", "1K~1만", "1만+"];

    /// 카테고리별 증가율 사전. 증가율이 없는 카테고리는 빠진다.
    pub fn growth_by_category(&self) -> std::collections::HashMap<String, f64> {
        self.categories
            .iter()
            .filter_map(|c| c.growth_percent.map(|g| (c.key.clone(), g)))
            .collect()
    }
}

/// 카테고리 합계 한 줄 — 기준선 기록과 요약이 같은 키·같은 값을 쓰도록 빌더가 함께 만든다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCatalogCategoryTotal {
    pub key: String,
    pub live_count: i64,
    pub viewer_total: i64,
}

/// 전수 수집 행 파일의 본문. 요약과 같은 `sweep_id` 쌍이다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCatalogRowsSnapshot {
    pub sweep_id: Uuid,
    #[serde(with = "chzzk_date")]
    pub completed_at: DateTime<Utc>,
    pub quality: LiveCatalogQuality,
    pub rows: Vec<LiveCatalogRow>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::channel::ChannelInfo;

    #[test]
    fn row_keys_and_mapping() {
        let mut info = LiveInfo::new(7, "t");
        info.concurrent_user_count = 12;
        let row = LiveCatalogRow::from(&info);
        assert_eq!(row.item_id(), "7");
        assert_eq!(row.channel_name, "Unknown");
        assert_eq!(row.category_key(), "분류 없음");
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"l": 7, "c": "", "n": "Unknown", "t": "t", "v": 12})
        );
        let back: LiveCatalogRow = serde_json::from_value(json).unwrap();
        assert_eq!(back, row);

        info.channel = Some(ChannelInfo::new("cid", "name"));
        let row = LiveCatalogRow::from(&info);
        assert_eq!(row.item_id(), "cid");
        assert!(row.item().is_live);
    }

    #[test]
    fn quality_encoding() {
        let q = LiveCatalogQuality::Verified { exact: 5 };
        assert_eq!(
            serde_json::to_string(&q).unwrap(),
            r#"{"verified":{"exact":5}}"#
        );
        let o: LiveCatalogQuality =
            serde_json::from_str(r#"{"observed":{"count":3,"reason":"limit"}}"#).unwrap();
        assert!(!o.is_verified_or_stitched());
        assert_eq!(o.exact_count(), None);
        assert_eq!(q.exact_count(), Some(5));
        assert!(LiveCatalogQuality::Stitched { observed: 1 }.is_verified_or_stitched());
    }

    #[test]
    fn summary_round_trip() {
        let row = LiveCatalogRow::from(&LiveInfo::new(1, "a"));
        let summary = LiveCatalogSummary {
            version: LiveCatalogSummary::CURRENT_VERSION,
            sweep_id: Uuid::new_v4(),
            completed_at: Utc::now(),
            quality: LiveCatalogQuality::Verified { exact: 1 },
            live_count: 1,
            exact_live_count: Some(1),
            viewer_total: 0,
            average_viewers: 0,
            median_viewers: 0,
            viewer_buckets: vec![1, 0, 0, 0],
            type_shares: vec![TypeShare {
                r#type: "ETC".into(),
                live_count: 1,
                viewer_total: 0,
            }],
            top_lives: vec![row.clone()],
            categories: vec![CategorySummary {
                key: "분류 없음".into(),
                category_id: None,
                r#type: None,
                live_count: 1,
                viewer_total: 0,
                leader: row,
                top_channel_ids: vec![],
                growth_percent: Some(12.5),
            }],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["v"], 1);
        assert!(json.get("sweepId").is_some());
        let back: LiveCatalogSummary = serde_json::from_value(json).unwrap();
        assert_eq!(back.categories, summary.categories);
        assert_eq!(back.growth_by_category().get("분류 없음"), Some(&12.5));
    }
}
