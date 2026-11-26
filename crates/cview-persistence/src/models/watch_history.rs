//! 시청 기록 영속 모델과 집계 정책.
//!
//! 원본: `Sources/CViewPersistence/Models/WatchHistory.swift`
//! (`WatchHistory`, `WatchHistoryAggregationPolicy`, `WatchHistorySyncRow`, `WatchHistoryData`).

use crate::db::db_model;
use crate::time::{epoch_seconds, interval_since, swift_double_description, Date, TimeInterval};
use cview_core::utils::watch_duration::formatted_watch_duration;
use serde::{Deserialize, Serialize};

db_model! {
    /// 시청 기록 행.
    pub struct WatchHistory in "WatchHistory" {
        pub channel_id: String => "channelId",
        pub channel_name: String => "channelName",
        pub thumbnail_url: Option<String> => "thumbnailURL",
        pub category_name: Option<String> => "categoryName",
        pub started_at: Date => "startedAt",
        pub ended_at: Option<Date> => "endedAt",
        pub duration: f64 => "duration",
        /// 마지막 변경 시각(서버 LWW 판정). nil 이면 startedAt 으로 폴백.
        pub updated_at: Option<Date> => "updatedAt",
        /// 마지막으로 서버에 push 성공한 시각. nil 이면 큐에 남아 있다.
        pub synced_at: Option<Date> => "syncedAt",
    }
}

impl WatchHistory {
    /// Swift `init` — `updatedAt = .now`, `syncedAt = nil`.
    pub fn new(
        channel_id: impl Into<String>,
        channel_name: impl Into<String>,
        thumbnail_url: Option<String>,
        category_name: Option<String>,
        started_at: Date,
    ) -> Self {
        Self {
            id: 0,
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            thumbnail_url,
            category_name,
            started_at,
            ended_at: None,
            duration: 0.0,
            updated_at: Some(crate::time::now()),
            synced_at: None,
        }
    }

    /// 효과적 updated 시각 — nil 이면 startedAt.
    pub fn effective_updated_at(&self) -> Date {
        self.updated_at.unwrap_or(self.started_at)
    }

    /// 지금 시각 기준 집계 시간(`WatchHistoryAggregationPolicy.effectiveDuration`).
    pub fn effective_duration(&self, now: Date) -> TimeInterval {
        WatchHistoryAggregationPolicy::effective_duration(
            self.started_at,
            self.ended_at,
            self.duration,
            self.updated_at,
            now,
        )
    }

    pub(crate) fn to_data(&self) -> WatchHistoryData {
        WatchHistoryData {
            channel_id: self.channel_id.clone(),
            channel_name: self.channel_name.clone(),
            thumbnail_url: self.thumbnail_url.clone(),
            category_name: self.category_name.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            duration: self.duration,
        }
    }
}

/// 진행 중인 시청 세션을 집계할 때 쓰는 단일 정책.
pub struct WatchHistoryAggregationPolicy;

impl WatchHistoryAggregationPolicy {
    /// 진행 중으로 볼 마지막 갱신 뒤 시간 — 체크포인트 간격(60초)보다 길어야 한다. 예전 45초는 간격보다 짧아,
    /// 매분 45~60초 사이에 진행 중 시청 시간이 마지막 체크포인트 값으로 뒤로 갔다(첫 1분은 0초 취급).
    pub const ACTIVE_GRACE_INTERVAL: TimeInterval = 75.0;
    /// 재생 중 시청 기록 체크포인트 간격(초).
    pub const CHECKPOINT_INTERVAL: TimeInterval = 30.0;
    /// 체크포인트 허용 오차(초).
    pub const CHECKPOINT_TOLERANCE: TimeInterval = 2.0;

    /// 다음 체크포인트까지 기다린다(future 를 버리면 취소).
    pub async fn sleep_until_next_checkpoint() {
        tokio::time::sleep(std::time::Duration::from_secs_f64(
            Self::CHECKPOINT_INTERVAL,
        ))
        .await;
    }

    pub fn is_actively_updating(
        ended_at: Option<Date>,
        updated_at: Option<Date>,
        started_at: Date,
        now: Date,
    ) -> bool {
        if ended_at.is_some() {
            return false;
        }
        let age = interval_since(&now, &updated_at.unwrap_or(started_at));
        (-Self::ACTIVE_GRACE_INTERVAL..=Self::ACTIVE_GRACE_INTERVAL).contains(&age)
    }

    /// 체크포인트·종료 한 번에 더하는 시간의 상한(초). 체크포인트 간격(60초)을 몇 번 놓친 정도는 그대로 세고,
    /// 그보다 긴 틈(노트북을 덮은 잠자기, 멀티라이브 일시정지·메뉴 이탈로 체크포인트를 건너뛴 시간)은 세지 않는다 —
    /// 예전엔 시작 시각부터 잰 벽시계 시간을 써서, 8시간 잠들었다 깨면 그 채널에 8시간이 쌓였다.
    pub const MAX_CHECKPOINT_GAP: TimeInterval = 300.0;

    /// 기록 시간을 `at` 까지 늘린다 — 마지막 갱신(없으면 시작) 뒤 지난 시간을 [`Self::MAX_CHECKPOINT_GAP`] 까지만 더한다.
    pub fn advanced_duration(
        persisted: TimeInterval,
        started_at: Date,
        updated_at: Option<Date>,
        at: Date,
    ) -> TimeInterval {
        let persisted = if persisted.is_finite() {
            persisted.max(0.0)
        } else {
            0.0
        };
        // 마지막 갱신이 대상 시각보다 뒤(시계가 되돌아감·과거 시각으로 연 기록)면 시작 시각부터 잰다.
        let last = updated_at
            .filter(|u| interval_since(&at, u) >= 0.0)
            .unwrap_or(started_at);
        let gap = interval_since(&at, &last).clamp(0.0, Self::MAX_CHECKPOINT_GAP);
        // 시작부터 잰 길이를 넘지는 않는다(시계가 앞뒤로 흔들려도).
        let wall = interval_since(&at, &started_at).max(0.0);
        (persisted + gap).min(wall.max(persisted))
    }

    pub fn effective_duration(
        started_at: Date,
        ended_at: Option<Date>,
        persisted_duration: TimeInterval,
        updated_at: Option<Date>,
        now: Date,
    ) -> TimeInterval {
        let persisted = if persisted_duration.is_finite() {
            persisted_duration.max(0.0)
        } else {
            0.0
        };
        if !Self::is_actively_updating(ended_at, updated_at, started_at, now) {
            return persisted;
        }
        Self::advanced_duration(persisted, started_at, updated_at, now)
    }
}

/// 서버 push 용 sync row(`WatchHistorySyncRow`). 날짜는 Swift `JSONEncoder` 기본(2001 기준 초)이
/// 아니라 호출자가 정하는 형식으로 직렬화되므로 여기서는 ISO 8601 을 쓴다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistorySyncRow {
    /// `"<channelId>:<startedAtEpochMillis>"`
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    #[serde(rename = "thumbnailURL")]
    pub thumbnail_url: Option<String>,
    pub category_name: Option<String>,
    #[serde(with = "crate::time::iso8601")]
    pub started_at: Date,
    #[serde(with = "crate::time::iso8601::option")]
    pub ended_at: Option<Date>,
    pub duration: TimeInterval,
    #[serde(with = "crate::time::iso8601")]
    pub updated_at: Date,
}

impl WatchHistorySyncRow {
    /// `"channelId:ms"` — ms 는 `Int(epoch * 1000)`(0 쪽으로 버림).
    pub fn make_id(channel_id: &str, started_at: &Date) -> String {
        let ms = (epoch_seconds(started_at) * 1000.0) as i64;
        format!("{channel_id}:{ms}")
    }
}

/// 시청 기록 DTO(`WatchHistoryData`).
#[derive(Debug, Clone, PartialEq)]
pub struct WatchHistoryData {
    pub channel_id: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub category_name: Option<String>,
    pub started_at: Date,
    pub ended_at: Option<Date>,
    pub duration: TimeInterval,
}

impl WatchHistoryData {
    /// `"<channelId>-<startedAt.timeIntervalSince1970>"` (Swift Double 표기, 예: `ch1-1700000000.0`).
    pub fn id(&self) -> String {
        format!(
            "{}-{}",
            self.channel_id,
            swift_double_description(epoch_seconds(&self.started_at))
        )
    }

    /// 시청 시간 표시 — `TimeInterval.formattedWatchDuration` 에 위임.
    pub fn formatted_duration(&self) -> String {
        formatted_watch_duration(self.duration)
    }

    /// `MM/dd HH:mm`(현지 시각).
    pub fn formatted_date(&self) -> String {
        self.started_at
            .with_timezone(&chrono::Local)
            .format("%m/%d %H:%M")
            .to_string()
    }
}
