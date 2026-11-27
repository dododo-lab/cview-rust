//! 시청 기록 조회·통계 집계·복구·정리·지문.
//!
//! 원본: `Sources/CViewPersistence/DataStore+WatchHistorySync.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{WatchHistory, WatchHistoryAggregationPolicy, WatchHistoryData};
use crate::revision::WatchInsightsRevision;
use crate::time::{adding, interval_since, now, Calendar, Date, TimeInterval};
use cview_core::models::LiveChannelItem;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// `currentWatchStreak` 가 한 번에 읽는 달력 창(일).
const WATCH_STREAK_WINDOW_DAYS: i64 = 14;

/// `aggregateInsights` 결과 묶음(Swift 튜플).
/// 순위 후보 수 — 상위 10(`channels`)만 담으면, 팔로우하지 않은 채널을 많이 보는 사용자의
/// '많이 보는 팔로잉'이 비거나 11위 아래 팔로잉 채널이 빠졌다.
pub const CHANNEL_POOL: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct AggregatedInsights {
    pub all: TimeInterval,
    pub w7: TimeInterval,
    pub w30: TimeInterval,
    pub streak: i64,
    pub avg: TimeInterval,
    /// 상위 10 채널 (이름, 초).
    pub channels: Vec<(String, TimeInterval)>,
    /// `channels` 와 같은 순서의 채널 id — 화면이 팔로우 여부로 거르거나 채널을 열 때 쓴다.
    pub channel_ids: Vec<String>,
    /// 같은 순서로 상위 [`CHANNEL_POOL`] 채널 (id, 이름, 초) — 화면이 팔로우한 채널만 골라도 10위 밖까지 닿게.
    pub channel_pool: Vec<(String, String, TimeInterval)>,
    /// 상위 8 카테고리 (표시 이름, 초).
    pub categories: Vec<(String, TimeInterval)>,
    pub hourly: BTreeMap<u32, TimeInterval>,
    /// 30일, 오래된 날부터. 빈 날은 0.
    pub daily: Vec<(Date, TimeInterval)>,
    /// 전체 레코드 수.
    pub session_count: usize,
    /// duration > 0 레코드 수.
    pub counted_session_count: usize,
    pub unique_channel_count: usize,
    pub active_session_count: usize,
    pub last_updated_at: Option<Date>,
}

/// 한 채널의 이 앱 시청 요약 — 채널 정보 탭 "내 시청 기록".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelWatchSummary {
    /// 전체 시청 시간(초).
    pub total_seconds: TimeInterval,
    /// 최근 30일 시청 시간(초).
    pub last30_seconds: TimeInterval,
    /// 시청 시간이 있는 기록 수.
    pub session_count: usize,
    pub first_watched_at: Option<Date>,
    pub last_watched_at: Option<Date>,
    /// 가장 오래 본 카테고리(표시 이름) — 분류 없는 기록만 있으면 `None`.
    pub top_category: Option<String>,
}

/// 인사이트 집계 입력 지문(`WatchHistoryFingerprint`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistoryFingerprint {
    pub row_count: i64,
    /// `endedAt == nil` 인 열린 행 수.
    pub open_count: i64,
    pub max_updated_at: Option<Date>,
    pub max_started_at: Option<Date>,
}

/// duration 내림차순 상위 n. Swift 는 동률 순서가 미정이라 여기서는 이름 오름차순으로 고정한다.
fn top_n(
    map: impl IntoIterator<Item = (String, TimeInterval)>,
    n: usize,
) -> Vec<(String, TimeInterval)> {
    let mut rows: Vec<(String, TimeInterval)> = map.into_iter().collect();
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    rows.truncate(n);
    rows
}

/// [`DataStore::channel_watch_summary`] 의 순수 집계 — `rows` 는 한 채널의 기록.
pub fn summarize_channel(rows: &[WatchHistory], now: Date) -> ChannelWatchSummary {
    let cutoff = adding(&now, -30.0 * 86_400.0);
    let mut out = ChannelWatchSummary::default();
    let mut cats: HashMap<String, TimeInterval> = HashMap::new();
    for r in rows {
        let d = r.effective_duration(now);
        if d <= 0.0 {
            continue;
        }
        out.total_seconds += d;
        out.session_count += 1;
        if r.started_at >= cutoff {
            out.last30_seconds += d;
        }
        out.first_watched_at = Some(
            out.first_watched_at
                .map_or(r.started_at, |f| f.min(r.started_at)),
        );
        out.last_watched_at = Some(
            out.last_watched_at
                .map_or(r.started_at, |l| l.max(r.started_at)),
        );
        if let Some(c) = r
            .category_name
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            *cats
                .entry(LiveChannelItem::display_category_name(Some(c)))
                .or_insert(0.0) += d;
        }
    }
    out.top_category = top_n(cats, 1).into_iter().next().map(|(name, _)| name);
    out
}

impl DataStore {
    fn all_watch_rows(conn: &rusqlite::Connection) -> Result<Vec<WatchHistory>> {
        Ok(db::fetch::<WatchHistory>(conn, "ORDER BY id ASC", &[])?)
    }

    /// 최근 시청 기록(시작 시각 내림차순, 효과 duration).
    pub fn fetch_watch_history(&self, limit: usize) -> Result<Vec<WatchHistoryData>> {
        let now = now();
        self.read(|conn| {
            Ok(db::fetch::<WatchHistory>(
                conn,
                "ORDER BY startedAt DESC, id DESC LIMIT ?",
                &[Value::Integer(limit as i64)],
            )?
            .iter()
            .map(|r| {
                let mut d = r.to_data();
                d.duration = r.effective_duration(now);
                d
            })
            .collect())
        })
    }

    /// 총 시청 시간(초).
    pub fn total_watch_time(&self) -> Result<TimeInterval> {
        let now = now();
        self.read(|conn| {
            Ok(Self::all_watch_rows(conn)?
                .iter()
                .map(|r| r.effective_duration(now))
                .sum())
        })
    }

    /// 채널별 시청 시간 상위 limit.
    pub fn watch_time_by_channel(&self, limit: usize) -> Result<Vec<(String, TimeInterval)>> {
        let now = now();
        self.read(|conn| {
            let mut map: HashMap<String, (String, TimeInterval)> = HashMap::new();
            for r in Self::all_watch_rows(conn)? {
                let d = r.effective_duration(now);
                if d <= 0.0 {
                    continue;
                }
                let entry = map
                    .entry(r.channel_id.clone())
                    .or_insert((r.channel_name.clone(), 0.0));
                entry.1 += d;
            }
            Ok(top_n(map.into_values(), limit))
        })
    }

    /// 카테고리별 시청 시간 상위 limit(nil 은 "분류 없음").
    pub fn watch_time_by_category(&self, limit: usize) -> Result<Vec<(String, TimeInterval)>> {
        let now = now();
        self.read(|conn| {
            let mut bucket: HashMap<String, TimeInterval> = HashMap::new();
            for r in Self::all_watch_rows(conn)? {
                let d = r.effective_duration(now);
                if d <= 0.0 {
                    continue;
                }
                *bucket
                    .entry(LiveChannelItem::display_category_name(
                        r.category_name.as_deref(),
                    ))
                    .or_insert(0.0) += d;
            }
            Ok(top_n(bucket, limit))
        })
    }

    /// 한 채널의 시청 요약 — 시청 시간이 0인 기록은 세지 않는다.
    pub fn channel_watch_summary(
        &self,
        channel_id: &str,
        now: Date,
    ) -> Result<ChannelWatchSummary> {
        let rows = self.read(|conn| {
            Ok(db::fetch::<WatchHistory>(
                conn,
                "WHERE channelId = ? ORDER BY startedAt ASC, id ASC",
                &[Value::Text(channel_id.into())],
            )?)
        })?;
        Ok(summarize_channel(&rows, now))
    }

    /// 연속 시청 일수 — 오늘부터 거꾸로, 오늘 결석 1회 허용. 14일 달력 창을 최신→과거로 걷는다.
    pub fn current_watch_streak(&self, now: Date) -> Result<i64> {
        let calendar = self.calendar();
        let today = calendar.start_of_day(now);
        let Some(tomorrow) = calendar.add_days(1, today) else {
            return Ok(0);
        };
        self.read(|conn| {
            let mut window_end = tomorrow;
            let mut cursor = today;
            let mut streak = 0i64;
            let mut today_skip_available = true;
            loop {
                let Some(raw_start) = calendar.add_days(-WATCH_STREAK_WINDOW_DAYS, window_end)
                else {
                    return Ok(streak);
                };
                let window_start = calendar.start_of_day(raw_start);
                let rows = db::fetch::<WatchHistory>(
                    conn,
                    "WHERE startedAt >= ? AND startedAt < ?",
                    &[dv(&window_start), dv(&window_end)],
                )?;
                let watched: HashSet<Date> = rows
                    .iter()
                    .filter(|r| r.effective_duration(now) > 0.0)
                    .map(|r| calendar.start_of_day(r.started_at))
                    .collect();
                while cursor >= window_start {
                    if watched.contains(&cursor) {
                        streak += 1;
                        today_skip_available = false;
                    } else if today_skip_available && cursor == today {
                        today_skip_available = false;
                    } else {
                        return Ok(streak);
                    }
                    let Some(prev) = calendar.add_days(-1, cursor) else {
                        return Ok(streak);
                    };
                    cursor = prev;
                }
                window_end = window_start;
            }
        })
    }

    /// 최근 N일 시청 시간 합계(`now - N일` 이후 시작).
    pub fn total_watch_time_within_last_days(&self, days: i64) -> Result<TimeInterval> {
        let calendar = self.calendar();
        let Some(cutoff) = calendar.add_days(-days, now()) else {
            return Ok(0.0);
        };
        let now = now();
        self.read(|conn| {
            Ok(
                db::fetch::<WatchHistory>(conn, "WHERE startedAt >= ?", &[dv(&cutoff)])?
                    .iter()
                    .map(|r| r.effective_duration(now))
                    .sum(),
            )
        })
    }

    /// 인사이트 화면용 집계 일괄 묶음.
    pub fn aggregate_insights(&self, now: Date) -> Result<AggregatedInsights> {
        let calendar = self.calendar();
        let records = self.read(Self::all_watch_rows)?;
        Ok(aggregate(&records, now, calendar.as_ref()))
    }

    /// 통계 화면 기간 리포트 — [`crate::watch_report::build_report`].
    pub fn watch_report(
        &self,
        period: crate::watch_report::ReportPeriod,
        now: Date,
    ) -> Result<crate::watch_report::WatchReport> {
        let calendar = self.calendar();
        let records = self.read(Self::all_watch_rows)?;
        Ok(crate::watch_report::build_report(
            &records,
            period,
            now,
            calendar.as_ref(),
        ))
    }

    /// 앱 시작 시 남은 열린 레코드를 마지막 체크포인트 duration 까지만 보존해 닫는다.
    pub fn recover_interrupted_watch_records(&self, now: Date) -> Result<usize> {
        let n = self.write(|tx| {
            let records = db::fetch::<WatchHistory>(tx, "WHERE endedAt IS NULL", &[])?;
            if records.is_empty() {
                return Ok((0, false));
            }
            for mut record in records.iter().cloned() {
                let persisted = if record.duration.is_finite() {
                    record.duration.max(0.0)
                } else {
                    0.0
                };
                let recovered_end = now.min(adding(&record.started_at, persisted));
                let ended = record.started_at.max(recovered_end);
                record.ended_at = Some(ended);
                record.duration = interval_since(&ended, &record.started_at).max(0.0);
                record.updated_at = Some(now);
                record.synced_at = None;
                db::update(tx, &record)?;
            }
            Ok((records.len(), true))
        })?;
        if n > 0 {
            WatchInsightsRevision::bump();
        }
        Ok(n)
    }

    /// 오래된 시청 기록 정리(`now - days` 이전 시작). 지운 행이 있을 때만 리비전을 올린다.
    pub fn cleanup_old_watch_history(&self, older_than_days: i64) -> Result<()> {
        let n = now();
        let cutoff = self.calendar().add_days(-older_than_days, n).unwrap_or(n);
        let deleted = self.write(|tx| {
            let count = db::count::<WatchHistory>(tx, "WHERE startedAt < ?", &[dv(&cutoff)])?;
            if count == 0 {
                return Ok((false, false));
            }
            tx.execute(
                "DELETE FROM WatchHistory WHERE startedAt < ?",
                [crate::time::to_db(&cutoff)],
            )?;
            Ok((true, true))
        })?;
        if deleted {
            WatchInsightsRevision::bump();
        }
        Ok(())
    }

    /// 모든 시청 기록 삭제. 지운 행이 있을 때만 리비전을 올린다.
    pub fn delete_all_watch_history(&self) -> Result<()> {
        let deleted = self.write(|tx| {
            let n = tx.execute("DELETE FROM WatchHistory", [])?;
            Ok((n > 0, n > 0))
        })?;
        if deleted {
            WatchInsightsRevision::bump();
        }
        Ok(())
    }

    /// 인사이트 집계 입력 지문 — 전 행을 읽지 않는다.
    pub fn watch_history_fingerprint(&self) -> Result<WatchHistoryFingerprint> {
        self.read(|conn| {
            let row_count = db::count::<WatchHistory>(conn, "", &[])?;
            let open_count = db::count::<WatchHistory>(conn, "WHERE endedAt IS NULL", &[])?;
            let max_updated_at: Option<i64> = conn.query_row(
                "SELECT max(updatedAt) FROM WatchHistory WHERE updatedAt IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            let max_started_at: Option<i64> =
                conn.query_row("SELECT max(startedAt) FROM WatchHistory", [], |r| r.get(0))?;
            Ok(WatchHistoryFingerprint {
                row_count,
                open_count,
                max_updated_at: max_updated_at.map(crate::time::from_db),
                max_started_at: max_started_at.map(crate::time::from_db),
            })
        })
    }
}

/// `start..end`(`hi` 까지)를 현지 정시 조각으로 나눠 `f(조각 시작, 초, 현지 시)` 를 부른다.
/// 기록 안에서 현지 오프셋이 같으면(거의 모든 기록) 달력에 묻지 않고 정수로 나눈다 — 전체 기록을 조각으로 나누어도
/// 인사이트 집계가 느려지지 않게. 오프셋이 바뀌는 기록(서머타임 전환)은 달력의 정시([`for_each_hour_piece`])로 나눈다.
///
/// [`for_each_hour_piece`]: crate::watch_report::for_each_hour_piece
fn hour_pieces(
    start: Date,
    end: Date,
    hi: Date,
    calendar: &dyn Calendar,
    f: &mut impl FnMut(Date, TimeInterval, u32),
) {
    let end = end.min(hi);
    if end <= start {
        return;
    }
    let off = calendar.offset_seconds(start);
    let (Some(s_ns), Some(e_ns)) = (start.timestamp_nanos_opt(), end.timestamp_nanos_opt()) else {
        crate::watch_report::for_each_hour_piece(start, end, start, end, calendar, |p, secs| {
            f(p, secs, calendar.hour(p))
        });
        return;
    };
    if calendar.offset_seconds(end) != off || e_ns - s_ns > 20 * 3_600_000_000_000 {
        crate::watch_report::for_each_hour_piece(start, end, start, end, calendar, |p, secs| {
            f(p, secs, calendar.hour(p))
        });
        return;
    }
    const HOUR: i64 = 3_600_000_000_000;
    let off_ns = i64::from(off) * 1_000_000_000;
    let mut cur = s_ns;
    let mut idx = (s_ns + off_ns).div_euclid(HOUR);
    while cur < e_ns {
        let next = ((idx + 1) * HOUR - off_ns).min(e_ns);
        let piece = chrono::DateTime::from_timestamp_nanos(cur);
        f(piece, (next - cur) as f64 / 1e9, idx.rem_euclid(24) as u32);
        cur = next;
        idx += 1;
    }
}

/// `aggregateInsights` 본체 — 원시 행 전량에서 한 번에 접는다.
pub(crate) fn aggregate(
    records: &[WatchHistory],
    now: Date,
    calendar: &dyn Calendar,
) -> AggregatedInsights {
    let durations: Vec<TimeInterval> = records.iter().map(|r| r.effective_duration(now)).collect();
    let total: TimeInterval = durations.iter().sum();
    let cutoff7 = calendar.add_days(-7, now).unwrap_or(now);
    let cutoff30 = calendar.add_days(-30, now).unwrap_or(now);
    let w7 = records
        .iter()
        .zip(&durations)
        .filter(|(r, _)| r.started_at >= cutoff7)
        .map(|(_, d)| d)
        .sum();
    let w30 = records
        .iter()
        .zip(&durations)
        .filter(|(r, _)| r.started_at >= cutoff30)
        .map(|(_, d)| d)
        .sum();
    let valid: Vec<TimeInterval> = durations.iter().copied().filter(|d| *d > 0.0).collect();
    let avg = if valid.is_empty() {
        0.0
    } else {
        valid.iter().sum::<f64>() / valid.len() as f64
    };

    let mut channel_map: HashMap<&str, (&str, TimeInterval)> = HashMap::new();
    let mut category_map: HashMap<String, TimeInterval> = HashMap::new();
    let mut hourly: BTreeMap<u32, TimeInterval> = BTreeMap::new();
    let mut daily_map: HashMap<Date, TimeInterval> = HashMap::new();
    let earliest30 = calendar.start_of_day(calendar.add_days(-29, now).unwrap_or(now));
    let mut streak_days: HashSet<Date> = HashSet::new();

    for (r, &duration) in records.iter().zip(&durations) {
        if duration <= 0.0 {
            continue;
        }
        let entry = channel_map
            .entry(r.channel_id.as_str())
            .or_insert((r.channel_name.as_str(), 0.0));
        entry.1 += duration;
        *category_map
            .entry(LiveChannelItem::display_category_name(
                r.category_name.as_deref(),
            ))
            .or_insert(0.0) += duration;
        // 일별 막대·'오늘 시청'·'주로 보는 시간'·'연속 시청'은 현지 정시 조각으로 나눠 그 시각·그날에 담는다 — 예전엔
        // 23시에 시작한 3시간을 모두 시작한 시각·날에 넣어, 자정 뒤 '오늘 시청'이 0, 밤에 본 다음 날 '연속 시청'이 끊겼고
        // 통계 화면(watch_report, 이미 조각으로 담음)과 값이 달랐다. (일별은 30일 칸 안의 조각만)
        let end = adding(&r.started_at, duration);
        let mut last_day: Option<(Date, u32)> = None; // (그날 0시, 앞 조각의 현지 시)
        let mut put = |piece: Date, secs: TimeInterval, hour: u32| {
            *hourly.entry(hour).or_insert(0.0) += secs;
            // 날은 조각이 그날 첫 조각일 때만 달력에 묻는다 — 조각은 정시에서 끊기므로 현지 시가 앞 조각보다 작아지면
            // (23시 → 0시) 다음 날이다(서머타임으로 같은 시가 두 번 와도 작아지지 않는다).
            let day = match last_day {
                Some((d, prev)) if hour >= prev => {
                    last_day = Some((d, hour));
                    d
                }
                _ => {
                    let d = calendar.start_of_day(piece);
                    streak_days.insert(d);
                    last_day = Some((d, hour));
                    d
                }
            };
            if piece >= earliest30 {
                *daily_map.entry(day).or_insert(0.0) += secs;
            }
        };
        hour_pieces(r.started_at, end, now.max(end), calendar, &mut put);
    }

    // 이름으로 정렬하던 top_n 과 같은 순서(초 내림차순, 같으면 이름)로 고르되 id 를 함께 남긴다.
    let mut ranked: Vec<(&str, &str, TimeInterval)> = channel_map
        .iter()
        .map(|(id, (n, d))| (*id, *n, *d))
        .collect();
    ranked.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
            .then_with(|| a.0.cmp(b.0))
    });
    ranked.truncate(CHANNEL_POOL);
    let channel_pool: Vec<(String, String, TimeInterval)> = ranked
        .iter()
        .map(|(id, n, d)| (id.to_string(), n.to_string(), *d))
        .collect();
    let channels: Vec<(String, TimeInterval)> = ranked
        .iter()
        .take(10)
        .map(|(_, n, d)| (n.to_string(), *d))
        .collect();
    let channel_ids: Vec<String> = ranked
        .iter()
        .take(10)
        .map(|(id, _, _)| id.to_string())
        .collect();
    let categories = top_n(category_map, 8);

    let today = calendar.start_of_day(now);
    let mut daily = Vec::with_capacity(30);
    for offset in 0..30 {
        if let Some(day) = calendar.add_days(-offset, today) {
            daily.push((day, daily_map.get(&day).copied().unwrap_or(0.0)));
        }
    }
    daily.reverse();

    let mut cursor = today;
    let mut streak = 0i64;
    if !streak_days.contains(&cursor) {
        cursor = calendar.add_days(-1, cursor).unwrap_or(cursor);
    }
    while streak_days.contains(&cursor) {
        streak += 1;
        match calendar.add_days(-1, cursor) {
            Some(prev) => cursor = prev,
            None => break,
        }
    }

    let active_session_count = records
        .iter()
        .filter(|r| {
            WatchHistoryAggregationPolicy::is_actively_updating(
                r.ended_at,
                r.updated_at,
                r.started_at,
                now,
            )
        })
        .count();
    let last_updated_at = records
        .iter()
        .map(|r| r.updated_at.or(r.ended_at).unwrap_or(r.started_at))
        .max();

    AggregatedInsights {
        all: total,
        w7,
        w30,
        streak,
        avg,
        channels,
        channel_ids,
        channel_pool,
        categories,
        hourly,
        daily,
        session_count: records.len(),
        counted_session_count: valid.len(),
        unique_channel_count: channel_map.len(),
        active_session_count,
        last_updated_at,
    }
}

#[cfg(test)]
mod channel_summary_tests {
    use super::*;

    fn row(category: Option<&str>, days_ago: f64, secs: f64, now: Date) -> WatchHistory {
        let started = adding(&now, -days_ago * 86_400.0);
        let mut r = WatchHistory::new("c1", "채널", None, category.map(String::from), started);
        r.ended_at = Some(adding(&started, secs));
        r.duration = secs;
        r.updated_at = r.ended_at;
        r
    }

    #[test]
    fn summarize_counts_time_sessions_dates_and_top_category() {
        let now = crate::time::now();
        let rows = vec![
            row(Some("마인크래프트"), 40.0, 3600.0, now),
            row(Some("저스트 채팅"), 3.0, 1200.0, now),
            row(Some("마인크래프트"), 1.0, 600.0, now),
            row(None, 0.5, 0.0, now), // 시청 시간 0 — 세지 않는다
        ];
        let s = summarize_channel(&rows, now);
        assert_eq!(s.session_count, 3);
        assert!((s.total_seconds - 5400.0).abs() < 1e-6);
        assert!((s.last30_seconds - 1800.0).abs() < 1e-6);
        assert_eq!(s.first_watched_at, Some(rows[0].started_at));
        assert_eq!(s.last_watched_at, Some(rows[2].started_at));
        assert_eq!(s.top_category.as_deref(), Some("마인크래프트"));
    }

    /// 자정을 넘긴 시청은 두 날에 나눠 담는다 — 23시에 시작한 3시간이 어제 1시간·오늘 2시간.
    #[test]
    fn daily_splits_a_session_across_midnight() {
        use chrono::TimeZone;
        let tz = chrono_tz::Asia::Seoul;
        let now = tz
            .with_ymd_and_hms(2026, 3, 10, 9, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc);
        let started = tz
            .with_ymd_and_hms(2026, 3, 9, 23, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut r = WatchHistory::new("c1", "채널", None, None, started);
        r.ended_at = Some(adding(&started, 3.0 * 3600.0));
        r.duration = 3.0 * 3600.0;
        r.updated_at = r.ended_at;
        let a = aggregate(&[r], now, &tz);
        let n = a.daily.len();
        assert!(
            (a.daily[n - 1].1 - 7200.0).abs() < 1e-6,
            "오늘 {:?}",
            a.daily[n - 1]
        );
        assert!(
            (a.daily[n - 2].1 - 3600.0).abs() < 1e-6,
            "어제 {:?}",
            a.daily[n - 2]
        );
        assert!((a.all - 3.0 * 3600.0).abs() < 1e-6);
    }

    /// 정수로 나누는 빠른 길은 달력 정시로 나눈 조각과 같다(30분 오프셋·서머타임 날 포함).
    #[test]
    fn hour_pieces_match_the_calendar_pieces() {
        use chrono::TimeZone;
        fn check(cal: &dyn Calendar, start: Date, secs: f64) {
            let end = adding(&start, secs);
            let mut fast = Vec::new();
            hour_pieces(start, end, end, cal, &mut |p, s, h| fast.push((p, s, h)));
            let mut slow = Vec::new();
            crate::watch_report::for_each_hour_piece(start, end, start, end, cal, |p, s| {
                slow.push((p, s, cal.hour(p)))
            });
            assert_eq!(fast.len(), slow.len(), "{start}");
            for (a, b) in fast.iter().zip(&slow) {
                assert_eq!((a.0, a.2), (b.0, b.2), "{start}");
                assert!((a.1 - b.1).abs() < 1e-6);
            }
        }
        let seoul = chrono_tz::Asia::Seoul;
        let kolkata = chrono_tz::Asia::Kolkata;
        let ny = chrono_tz::America::New_York;
        let utc = |y, mo, d, h, mi| chrono::Utc.with_ymd_and_hms(y, mo, d, h, mi, 7).unwrap();
        for start in [
            utc(2026, 3, 9, 14, 50),
            utc(2026, 3, 9, 23, 59),
            utc(2026, 12, 31, 14, 0),
        ] {
            check(&seoul, start, 3.0 * 3600.0 + 125.5);
            check(&kolkata, start, 5.0 * 3600.0);
        }
        // 뉴욕 서머타임 시작(2026-03-08 07:00Z)·끝(2026-11-01 06:00Z)을 걸친 기록
        check(&ny, utc(2026, 3, 8, 5, 30), 4.0 * 3600.0);
        check(&ny, utc(2026, 11, 1, 4, 30), 4.0 * 3600.0);
        // 하루를 넘는 기록
        check(&seoul, utc(2026, 5, 1, 3, 0), 30.0 * 3600.0);
    }

    /// '주로 보는 시간'·'연속 시청'도 정시 조각으로 — 월 23시~화 1시만 본 사용자는 수요일에 연속 2일, 23시·0시 각 1시간.
    #[test]
    fn hourly_and_streak_split_a_session_across_midnight() {
        use chrono::TimeZone;
        let tz = chrono_tz::Asia::Seoul;
        let at = |d, h, m| {
            tz.with_ymd_and_hms(2026, 3, d, h, m, 0)
                .unwrap()
                .with_timezone(&chrono::Utc)
        };
        let row = |started: Date, secs: f64| {
            let mut r = WatchHistory::new("c1", "채널", None, None, started);
            r.ended_at = Some(adding(&started, secs));
            r.duration = secs;
            r.updated_at = r.ended_at;
            r
        };
        // 2026-03-09 은 월요일
        let a = aggregate(&[row(at(9, 23, 0), 7200.0)], at(11, 9, 0), &tz);
        assert_eq!(a.streak, 2, "월·화 연속");
        assert!((a.hourly[&23] - 3600.0).abs() < 1e-6, "{:?}", a.hourly);
        assert!((a.hourly[&0] - 3600.0).abs() < 1e-6, "{:?}", a.hourly);
        // 19:50 부터 3시간 — 19시엔 10분뿐
        let b = aggregate(&[row(at(10, 19, 50), 3.0 * 3600.0)], at(11, 9, 0), &tz);
        assert!((b.hourly[&19] - 600.0).abs() < 1e-6, "{:?}", b.hourly);
        assert!((b.hourly[&20] - 3600.0).abs() < 1e-6);
        assert!((b.hourly[&22] - 3000.0).abs() < 1e-6);
        let total: f64 = b.hourly.values().sum();
        assert!((total - 3.0 * 3600.0).abs() < 1e-6);
    }

    #[test]
    fn summarize_empty_is_default() {
        assert_eq!(
            summarize_channel(&[], crate::time::now()),
            ChannelWatchSummary::default()
        );
    }
}
