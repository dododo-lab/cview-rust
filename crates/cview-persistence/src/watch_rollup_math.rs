//! 시청 기록 롤업 집계의 순수 로직 — 스키마와 무관하다.
//!
//! 원본: `Sources/CViewPersistence/WatchRollupMath.swift`
//! (`WatchRecordSnapshot`, `RollupBucket`, `InsightsResult`, `WatchRollupMath`).
//!
//! 규칙
//! - 버킷 `seconds` 에는 저장된 몫(`duration` 의 유한·비음수 값)만 넣는다. 열린 행의 실시간 증분은
//!   `insights` 가 더한다.
//! - w7·w30 경계일 하나만 원시 행(`boundary_records`)으로 계산하고, 나머지 날은 버킷을 쓴다.
//! - 채널 이름은 가장 최근 시작한 기록의 이름. 동률은 이름 오름차순.

use crate::models::{WatchHistory, WatchHistoryAggregationPolicy};
use crate::time::{epoch_seconds, Calendar, Date, TimeInterval};
use cview_core::models::LiveChannelItem;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 롤업 입력 한 줄 — `WatchHistory` 행의 값 복사본.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchRecordSnapshot {
    pub channel_id: String,
    pub channel_name: String,
    pub category_name: Option<String>,
    pub started_at: Date,
    pub ended_at: Option<Date>,
    pub duration: TimeInterval,
    pub updated_at: Option<Date>,
}

impl WatchRecordSnapshot {
    pub fn new(
        channel_id: impl Into<String>,
        channel_name: impl Into<String>,
        category_name: Option<String>,
        started_at: Date,
        ended_at: Option<Date>,
        duration: TimeInterval,
        updated_at: Option<Date>,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            category_name,
            started_at,
            ended_at,
            duration,
            updated_at,
        }
    }

    /// 저장된 몫 — 유한하지 않거나 음수면 0.
    pub fn persisted_seconds(&self) -> TimeInterval {
        if self.duration.is_finite() {
            self.duration.max(0.0)
        } else {
            0.0
        }
    }

    pub fn effective_seconds(&self, now: Date) -> TimeInterval {
        WatchHistoryAggregationPolicy::effective_duration(
            self.started_at,
            self.ended_at,
            self.duration,
            self.updated_at,
            now,
        )
    }

    pub fn is_actively_updating(&self, now: Date) -> bool {
        WatchHistoryAggregationPolicy::is_actively_updating(
            self.ended_at,
            self.updated_at,
            self.started_at,
            now,
        )
    }
}

impl From<&WatchHistory> for WatchRecordSnapshot {
    fn from(r: &WatchHistory) -> Self {
        Self {
            channel_id: r.channel_id.clone(),
            channel_name: r.channel_name.clone(),
            category_name: r.category_name.clone(),
            started_at: r.started_at,
            ended_at: r.ended_at,
            duration: r.duration,
            updated_at: r.updated_at,
        }
    }
}

/// (일 시작 · 채널 · 카테고리) 한 칸의 저장된 몫 합계.
#[derive(Debug, Clone, PartialEq)]
pub struct RollupBucket {
    /// `"<dayStart epoch 초>|<channelId>|<표시 카테고리>"`.
    pub key: String,
    pub day_start: Date,
    pub channel_id: String,
    /// 표시 라벨(nil·'' → "분류 없음").
    pub category: String,
    /// 이 칸에서 가장 최근에 시작한 기록의 채널 이름.
    pub channel_name: String,
    pub name_started_at: Date,
    /// 저장된 몫 합(초).
    pub seconds: TimeInterval,
    /// 저장된 몫이 0 보다 큰 기록 수.
    pub sessions: i64,
    /// 0초 기록까지 센 전체 기록 수.
    pub raw_sessions: i64,
    /// 시작 시(hour)별 저장된 몫. 길이 24.
    pub hour_seconds: Vec<TimeInterval>,
    /// `updatedAt ?? endedAt ?? startedAt` 최댓값.
    pub updated_at: Date,
}

impl RollupBucket {
    pub fn make_key(day_start: &Date, channel_id: &str, category: &str) -> String {
        format!(
            "{}|{}|{}",
            epoch_seconds(day_start) as i64,
            channel_id,
            category
        )
    }
}

/// `aggregateInsights` 와 같은 필드 묶음.
#[derive(Debug, Clone, PartialEq)]
pub struct InsightsResult {
    pub all: TimeInterval,
    pub w7: TimeInterval,
    pub w30: TimeInterval,
    pub streak: i64,
    pub avg: TimeInterval,
    pub channels: Vec<(String, TimeInterval)>,
    pub categories: Vec<(String, TimeInterval)>,
    pub hourly: BTreeMap<u32, TimeInterval>,
    pub daily: Vec<(Date, TimeInterval)>,
    pub session_count: i64,
    pub counted_session_count: i64,
    pub unique_channel_count: usize,
    pub active_session_count: i64,
    pub last_updated_at: Option<Date>,
}

struct OpenShare<'a> {
    record: &'a WatchRecordSnapshot,
    day_start: Date,
    increment: TimeInterval,
    effective: TimeInterval,
}

fn hour_of(date: Date, calendar: &dyn Calendar) -> usize {
    calendar.hour(date).min(23) as usize
}

#[derive(Default)]
struct Dimensions {
    channel_seconds: HashMap<String, TimeInterval>,
    channel_name: HashMap<String, (String, Date)>,
    category_seconds: HashMap<String, TimeInterval>,
    hour_seconds: [TimeInterval; 24],
    daily_map: HashMap<Date, TimeInterval>,
    streak_days: HashSet<Date>,
}

fn sort_desc_then_name(rows: &mut [(String, TimeInterval)]) {
    rows.sort_by(|a, b| {
        if a.1 != b.1 {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.0.cmp(&b.0)
        }
    });
}

/// 롤업 수학(`WatchRollupMath`).
pub struct WatchRollupMath;

impl WatchRollupMath {
    /// 모든 기록을 버킷으로 접는다(키 오름차순).
    pub fn buckets(records: &[WatchRecordSnapshot], calendar: &dyn Calendar) -> Vec<RollupBucket> {
        let mut map: HashMap<String, RollupBucket> = HashMap::new();
        for record in records {
            let day_start = calendar.start_of_day(record.started_at);
            let category = LiveChannelItem::display_category_name(record.category_name.as_deref());
            let key = RollupBucket::make_key(&day_start, &record.channel_id, &category);
            let persisted = record.persisted_seconds();
            let hour = hour_of(record.started_at, calendar);
            let touched = record
                .updated_at
                .or(record.ended_at)
                .unwrap_or(record.started_at);
            let bucket = map.entry(key.clone()).or_insert_with(|| RollupBucket {
                key,
                day_start,
                channel_id: record.channel_id.clone(),
                category,
                channel_name: record.channel_name.clone(),
                name_started_at: record.started_at,
                seconds: 0.0,
                sessions: 0,
                raw_sessions: 0,
                hour_seconds: vec![0.0; 24],
                updated_at: touched,
            });
            if record.started_at > bucket.name_started_at {
                bucket.channel_name = record.channel_name.clone();
                bucket.name_started_at = record.started_at;
            }
            bucket.seconds += persisted;
            bucket.hour_seconds[hour] += persisted;
            bucket.raw_sessions += 1;
            if persisted > 0.0 {
                bucket.sessions += 1;
            }
            if touched > bucket.updated_at {
                bucket.updated_at = touched;
            }
        }
        let mut out: Vec<RollupBucket> = map.into_values().collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    }

    fn window_sum(
        cutoff: Date,
        buckets: &[RollupBucket],
        opens: &[OpenShare<'_>],
        boundary: &[(Date, TimeInterval)],
        calendar: &dyn Calendar,
    ) -> TimeInterval {
        let cut_day = calendar.start_of_day(cutoff);
        let mut sum = 0.0;
        for b in buckets.iter().filter(|b| b.day_start >= cutoff) {
            sum += b.seconds;
        }
        for o in opens.iter().filter(|o| o.day_start >= cutoff) {
            sum += o.increment;
        }
        if cut_day < cutoff {
            for (started, secs) in boundary {
                if *started >= cutoff && calendar.start_of_day(*started) == cut_day {
                    sum += secs;
                }
            }
        }
        sum
    }

    fn streak_count(days: &HashSet<Date>, today: Date, calendar: &dyn Calendar) -> i64 {
        let mut cursor = today;
        let mut streak = 0;
        if !days.contains(&cursor) {
            cursor = calendar.add_days(-1, cursor).unwrap_or(cursor);
        }
        while days.contains(&cursor) {
            streak += 1;
            match calendar.add_days(-1, cursor) {
                Some(prev) => cursor = prev,
                None => break,
            }
        }
        streak
    }

    fn accumulate(
        buckets: &[RollupBucket],
        opens: &[OpenShare<'_>],
        earliest30: Date,
        calendar: &dyn Calendar,
    ) -> Dimensions {
        let mut dim = Dimensions::default();
        for b in buckets {
            *dim.channel_seconds
                .entry(b.channel_id.clone())
                .or_insert(0.0) += b.seconds;
            let newer = match dim.channel_name.get(&b.channel_id) {
                Some((_, at)) => *at < b.name_started_at,
                None => true,
            };
            if newer {
                dim.channel_name.insert(
                    b.channel_id.clone(),
                    (b.channel_name.clone(), b.name_started_at),
                );
            }
            *dim.category_seconds
                .entry(b.category.clone())
                .or_insert(0.0) += b.seconds;
            for hour in 0..24 {
                dim.hour_seconds[hour] += b.hour_seconds[hour];
            }
            if b.day_start >= earliest30 {
                *dim.daily_map.entry(b.day_start).or_insert(0.0) += b.seconds;
            }
            if b.sessions > 0 {
                dim.streak_days.insert(b.day_start);
            }
        }
        for o in opens.iter().filter(|o| o.effective > 0.0) {
            let r = o.record;
            *dim.channel_seconds
                .entry(r.channel_id.clone())
                .or_insert(0.0) += o.increment;
            *dim.category_seconds
                .entry(LiveChannelItem::display_category_name(
                    r.category_name.as_deref(),
                ))
                .or_insert(0.0) += o.increment;
            dim.hour_seconds[hour_of(r.started_at, calendar)] += o.increment;
            if o.day_start >= earliest30 {
                *dim.daily_map.entry(o.day_start).or_insert(0.0) += o.increment;
            }
            dim.streak_days.insert(o.day_start);
        }
        dim
    }

    /// 버킷 → 인사이트.
    ///
    /// - `open_records`: `endedAt == nil` 인 기록 전부.
    /// - `boundary_records`: w7·w30 경계일에 시작한 기록(그 밖의 기록은 걸러진다).
    pub fn insights(
        buckets: &[RollupBucket],
        open_records: &[WatchRecordSnapshot],
        boundary_records: &[WatchRecordSnapshot],
        now: Date,
        calendar: &dyn Calendar,
    ) -> InsightsResult {
        let opens: Vec<OpenShare<'_>> = open_records
            .iter()
            .map(|r| {
                let effective = r.effective_seconds(now);
                OpenShare {
                    record: r,
                    day_start: calendar.start_of_day(r.started_at),
                    increment: (effective - r.persisted_seconds()).max(0.0),
                    effective,
                }
            })
            .collect();
        let boundary: Vec<(Date, TimeInterval)> = boundary_records
            .iter()
            .map(|r| (r.started_at, r.effective_seconds(now)))
            .collect();
        let today = calendar.start_of_day(now);
        let cutoff7 = calendar.add_days(-7, now).unwrap_or(now);
        let cutoff30 = calendar.add_days(-30, now).unwrap_or(now);
        let earliest30 = calendar.start_of_day(calendar.add_days(-29, now).unwrap_or(now));

        let mut total = 0.0;
        let mut raw = 0;
        let mut counted = 0;
        let mut last: Option<Date> = None;
        for b in buckets {
            total += b.seconds;
            raw += b.raw_sessions;
            counted += b.sessions;
            if last.is_none_or(|l| b.updated_at > l) {
                last = Some(b.updated_at);
            }
        }
        let mut active = 0;
        for o in &opens {
            total += o.increment;
            if o.record.persisted_seconds() == 0.0 && o.effective > 0.0 {
                counted += 1;
            }
            if o.record.is_actively_updating(now) {
                active += 1;
            }
        }

        let dim = Self::accumulate(buckets, &opens, earliest30, calendar);
        let positive: HashMap<&String, TimeInterval> = dim
            .channel_seconds
            .iter()
            .filter(|(_, s)| **s > 0.0)
            .map(|(k, v)| (k, *v))
            .collect();
        let mut channels: Vec<(String, TimeInterval)> = positive
            .iter()
            .map(|(id, s)| {
                (
                    dim.channel_name
                        .get(*id)
                        .map(|(n, _)| n.clone())
                        .unwrap_or_default(),
                    *s,
                )
            })
            .collect();
        sort_desc_then_name(&mut channels);
        channels.truncate(10);
        let mut categories: Vec<(String, TimeInterval)> = dim
            .category_seconds
            .iter()
            .filter(|(_, s)| **s > 0.0)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        sort_desc_then_name(&mut categories);
        categories.truncate(8);
        let hourly: BTreeMap<u32, TimeInterval> = (0..24)
            .filter(|h| dim.hour_seconds[*h] > 0.0)
            .map(|h| (h as u32, dim.hour_seconds[h]))
            .collect();
        let mut daily = Vec::with_capacity(30);
        for offset in 0..30 {
            if let Some(day) = calendar.add_days(-offset, today) {
                daily.push((day, dim.daily_map.get(&day).copied().unwrap_or(0.0)));
            }
        }
        daily.reverse();

        InsightsResult {
            all: total,
            w7: Self::window_sum(cutoff7, buckets, &opens, &boundary, calendar),
            w30: Self::window_sum(cutoff30, buckets, &opens, &boundary, calendar),
            streak: Self::streak_count(&dim.streak_days, today, calendar),
            avg: if counted > 0 {
                total / counted as f64
            } else {
                0.0
            },
            channels,
            categories,
            hourly,
            daily,
            session_count: raw,
            counted_session_count: counted,
            unique_channel_count: positive.len(),
            active_session_count: active,
            last_updated_at: last,
        }
    }
}
