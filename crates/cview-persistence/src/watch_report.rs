//! 통계 화면의 기간 리포트 — 시청 기록을 기간(오늘·7일·30일·90일·전체)으로 잘라 한 번에 집계한다.
//!
//! `aggregate_insights` 는 기록 하나를 시작 시각 한 칸에 몰아 담지만, 여기서는 기록을 현지 정시 경계로
//! 잘라 실제로 본 시간대·요일·날에 나눠 담는다(밤 11시에 시작한 3시간 시청은 23·0·1시, 이틀에 걸친다).
//! 기간 밖으로 걸친 부분은 버린다. 이전 기간 비교는 같은 길이만큼 앞선 구간을 지금과 같은 경과 시각까지
//! 견준다(오늘 오후 3시면 어제 0시~오후 3시).
//!
//! 규칙
//! - 기록의 끝은 `시작 + 효과 duration`(진행 중이면 지금까지). 0초 기록은 세지 않는다.
//! - 시청 횟수·채널별 횟수는 기간 안에 1초라도 걸친 기록 수, 회당 평균은 기간 안 시간 ÷ 횟수.
//! - 길이 분포는 기간에 걸친 기록의 **전체** 길이로 나눈다(잘린 조각이 아니라 한 번 본 길이).
//! - 연속 시청은 기간과 무관하게 전 기록의 시작 날로 센다(개요 KPI 와 같은 정의, 오늘 결석 1회 허용).

use crate::models::WatchHistory;
use crate::time::{adding, interval_since, Calendar, Date, TimeInterval};
use cview_core::models::LiveChannelItem;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

/// 리포트 기간.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportPeriod {
    Today,
    Days7,
    Days30,
    Days90,
    All,
}

impl ReportPeriod {
    /// 화면 키(`today` · `7d` · `30d` · `90d` · `all`).
    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "today" => Self::Today,
            "7d" => Self::Days7,
            "30d" => Self::Days30,
            "90d" => Self::Days90,
            "all" => Self::All,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::Days90 => "90d",
            Self::All => "all",
        }
    }

    /// 오늘을 포함한 달력 일수. 전체는 None.
    fn days(self) -> Option<i64> {
        match self {
            Self::Today => Some(1),
            Self::Days7 => Some(7),
            Self::Days30 => Some(30),
            Self::Days90 => Some(90),
            Self::All => None,
        }
    }
}

/// 추이 차트 한 칸의 크기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BucketUnit {
    Hour,
    Day,
    Week,
    Month,
}

/// 추이 차트에서 따로 쌓아 보이는 카테고리 수(나머지는 "기타" 한 칸).
pub const TREND_SERIES: usize = 4;
/// 채널 순위 길이.
pub const TOP_CHANNELS: usize = 15;
/// 카테고리 순위 길이.
pub const TOP_CATEGORIES: usize = 10;
/// 길이 분포 경계(초) — 10분·30분·1시간·2시간·4시간, 마지막 칸은 그 이상.
pub const LENGTH_EDGES: [TimeInterval; 5] = [600.0, 1800.0, 3600.0, 7200.0, 14400.0];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelShare {
    pub channel_id: String,
    /// 가장 최근에 시작한 기록의 이름.
    pub name: String,
    pub seconds: TimeInterval,
    pub sessions: usize,
    pub last_watched_at: Date,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryShare {
    pub name: String,
    pub seconds: TimeInterval,
    pub sessions: usize,
}

/// 순위 밖 나머지.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherShare {
    pub count: usize,
    pub seconds: TimeInterval,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendBucket {
    pub start: Date,
    pub seconds: TimeInterval,
    /// `series` 순서의 카테고리별 몫, 마지막 하나는 나머지.
    pub parts: Vec<TimeInterval>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LengthBucket {
    /// 이 칸의 상한(초, 미만). 마지막 칸은 None.
    pub max_seconds: Option<TimeInterval>,
    pub count: usize,
    pub seconds: TimeInterval,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongestSession {
    pub channel_id: String,
    pub channel_name: String,
    pub category: String,
    pub started_at: Date,
    pub seconds: TimeInterval,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchReport {
    pub period: &'static str,
    pub range_start: Date,
    pub range_end: Date,
    /// 기간이 덮는 달력 일수(오늘 포함).
    pub days: i64,
    pub unit: BucketUnit,
    pub total: TimeInterval,
    /// 앞선 같은 길이 구간의 합. 전체 기간은 None.
    pub previous_total: Option<TimeInterval>,
    pub session_count: usize,
    pub active_days: usize,
    pub average_session: TimeInterval,
    pub daily_average: TimeInterval,
    pub longest: Option<LongestSession>,
    pub channel_count: usize,
    pub category_count: usize,
    pub current_streak: i64,
    pub best_streak: i64,
    pub first_watched_at: Option<Date>,
    pub channels: Vec<ChannelShare>,
    pub other_channels: OtherShare,
    pub categories: Vec<CategoryShare>,
    pub other_categories: OtherShare,
    /// [요일(월 0)][시(0…23)] 초.
    pub heatmap: Vec<Vec<TimeInterval>>,
    pub series: Vec<String>,
    pub trend: Vec<TrendBucket>,
    pub lengths: Vec<LengthBucket>,
}

/// 기록 한 줄의 효과 구간 `[start, end)`. 0초면 None.
fn span(r: &WatchHistory, now: Date) -> Option<(Date, Date, TimeInterval)> {
    let secs = r.effective_duration(now);
    (secs > 0.0).then(|| (r.started_at, adding(&r.started_at, secs), secs))
}

/// `[start, end)` 를 `[lo, hi)` 로 자르고 현지 정시마다 끊어 `(조각 시작, 초)` 로 넘긴다.
fn for_each_hour_piece(
    start: Date,
    end: Date,
    lo: Date,
    hi: Date,
    calendar: &dyn Calendar,
    mut f: impl FnMut(Date, TimeInterval),
) {
    let mut cursor = start.max(lo);
    let end = end.min(hi);
    while cursor < end {
        let next = calendar.next_hour(cursor).min(end);
        if next <= cursor {
            break;
        }
        f(cursor, interval_since(&next, &cursor));
        cursor = next;
    }
}

/// 같은 구간의 초만 합친다(이전 기간 비교용).
fn clipped_seconds(records: &[WatchHistory], lo: Date, hi: Date, now: Date) -> TimeInterval {
    records
        .iter()
        .filter_map(|r| span(r, now))
        .map(|(s, e, _)| {
            let (a, b) = (s.max(lo), e.min(hi));
            if b > a {
                interval_since(&b, &a)
            } else {
                0.0
            }
        })
        .sum()
}

/// 추이 칸 시작 시각들(오름차순). 마지막 칸은 지금을 담는다.
fn bucket_starts(
    unit: BucketUnit,
    range_start: Date,
    now: Date,
    calendar: &dyn Calendar,
) -> Vec<Date> {
    let mut out = Vec::new();
    match unit {
        BucketUnit::Hour => {
            // 오늘 하루 전체(23·25시간인 날 포함) — 아직 오지 않은 시간은 0 으로 둔다.
            let end = calendar
                .add_days(1, range_start)
                .map(|d| calendar.start_of_day(d))
                .unwrap_or(now);
            let mut h = range_start;
            while h < end && out.len() < 26 {
                out.push(h);
                h = calendar.next_hour(h);
            }
        }
        BucketUnit::Day => {
            let mut d = range_start;
            while d <= now {
                out.push(d);
                match calendar.add_days(1, d) {
                    Some(n) if n > d => d = calendar.start_of_day(n),
                    _ => break,
                }
            }
        }
        BucketUnit::Week => {
            let back = -i64::from(calendar.weekday(range_start));
            let mut w = calendar
                .add_days(back, range_start)
                .map(|d| calendar.start_of_day(d))
                .unwrap_or(range_start);
            while w <= now {
                out.push(w);
                match calendar.add_days(7, w) {
                    Some(n) if n > w => w = calendar.start_of_day(n),
                    _ => break,
                }
            }
        }
        BucketUnit::Month => {
            let mut m = calendar.start_of_month(range_start);
            while m <= now {
                out.push(m);
                match calendar.add_days(32, m) {
                    Some(n) if n > m => m = calendar.start_of_month(n),
                    _ => break,
                }
            }
        }
    }
    if out.is_empty() {
        out.push(range_start);
    }
    out
}

fn sort_by_seconds<T>(rows: &mut [T], key: impl Fn(&T) -> (TimeInterval, &str)) {
    rows.sort_by(|a, b| {
        let (sa, na) = key(a);
        let (sb, nb) = key(b);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| na.cmp(nb))
    });
}

/// 연속 시청(오늘 결석 1회 허용)과 가장 긴 연속 일수.
fn streaks(days: &BTreeSet<Date>, today: Date, calendar: &dyn Calendar) -> (i64, i64) {
    let mut current = 0i64;
    let mut cursor = today;
    if !days.contains(&cursor) {
        cursor = calendar.add_days(-1, cursor).unwrap_or(cursor);
    }
    while days.contains(&cursor) {
        current += 1;
        match calendar.add_days(-1, cursor) {
            Some(prev) => cursor = calendar.start_of_day(prev),
            None => break,
        }
    }
    let mut best = 0i64;
    let mut run = 0i64;
    let mut prev: Option<Date> = None;
    for &d in days {
        let follows = prev
            .and_then(|p| calendar.add_days(1, p))
            .map(|n| calendar.start_of_day(n) == d)
            .unwrap_or(false);
        run = if follows { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(d);
    }
    (current, best.max(current))
}

/// 기간 리포트 본체 — 원시 행 전량에서 한 번에 접는다.
pub fn build_report(
    records: &[WatchHistory],
    period: ReportPeriod,
    now: Date,
    calendar: &dyn Calendar,
) -> WatchReport {
    let today = calendar.start_of_day(now);
    let spans: Vec<Option<(Date, Date, TimeInterval)>> =
        records.iter().map(|r| span(r, now)).collect();
    let first_watched_at = spans.iter().flatten().map(|(s, _, _)| *s).min();

    let range_start = match period.days() {
        Some(n) => calendar
            .add_days(-(n - 1), today)
            .map(|d| calendar.start_of_day(d))
            .unwrap_or(today),
        None => first_watched_at
            .map(|d| calendar.start_of_day(d).min(today))
            .unwrap_or(today),
    };
    let days = period.days().unwrap_or_else(|| {
        // 서머타임으로 23·25시간인 날이 있어 반올림한다.
        ((interval_since(&today, &range_start) / 86_400.0).round() as i64 + 1).max(1)
    });
    let unit = match period {
        ReportPeriod::Today => BucketUnit::Hour,
        ReportPeriod::Days7 | ReportPeriod::Days30 => BucketUnit::Day,
        ReportPeriod::Days90 => BucketUnit::Week,
        ReportPeriod::All if days <= 31 => BucketUnit::Day,
        ReportPeriod::All if days <= 26 * 7 => BucketUnit::Week,
        ReportPeriod::All => BucketUnit::Month,
    };
    let starts = bucket_starts(unit, range_start, now, calendar);

    // 1차: 기간 안 조각을 채널·카테고리·시간대·날로 접는다.
    struct Channel<'a> {
        name: &'a str,
        named_at: Date,
        seconds: TimeInterval,
        sessions: usize,
        last: Date,
    }
    let mut channels: HashMap<&str, Channel> = HashMap::new();
    let mut categories: HashMap<String, (TimeInterval, usize)> = HashMap::new();
    let mut heatmap = vec![vec![0.0; 24]; 7];
    let mut active_days: HashSet<Date> = HashSet::new();
    let mut lengths: Vec<LengthBucket> = LENGTH_EDGES
        .iter()
        .map(|e| Some(*e))
        .chain(std::iter::once(None))
        .map(|max_seconds| LengthBucket {
            max_seconds,
            count: 0,
            seconds: 0.0,
        })
        .collect();
    // (기록 번호, 카테고리, 추이 칸, 초) — 추이는 상위 카테고리가 정해진 뒤 쌓는다.
    let mut pieces: Vec<(usize, TimeInterval)> = Vec::new();
    let mut piece_category: Vec<String> = Vec::new();
    let mut total = 0.0;
    let mut session_count = 0usize;
    let mut longest: Option<LongestSession> = None;

    for (r, sp) in records.iter().zip(&spans) {
        let Some((s, e, full)) = *sp else { continue };
        let category = LiveChannelItem::display_category_name(r.category_name.as_deref());
        let mut inside = 0.0;
        for_each_hour_piece(s, e, range_start, now, calendar, |at, secs| {
            inside += secs;
            heatmap[calendar.weekday(at).min(6) as usize][calendar.hour(at).min(23) as usize] +=
                secs;
            active_days.insert(calendar.start_of_day(at));
            let slot = starts.partition_point(|b| *b <= at).saturating_sub(1);
            pieces.push((slot, secs));
            piece_category.push(category.clone());
        });
        if inside <= 0.0 {
            continue;
        }
        total += inside;
        session_count += 1;
        let ch = channels.entry(r.channel_id.as_str()).or_insert(Channel {
            name: &r.channel_name,
            named_at: s,
            seconds: 0.0,
            sessions: 0,
            last: s,
        });
        ch.seconds += inside;
        ch.sessions += 1;
        ch.last = ch.last.max(e.min(now));
        if s >= ch.named_at {
            ch.name = &r.channel_name;
            ch.named_at = s;
        }
        let cat = categories.entry(category.clone()).or_insert((0.0, 0));
        cat.0 += inside;
        cat.1 += 1;
        let slot = LENGTH_EDGES
            .iter()
            .position(|edge| full < *edge)
            .unwrap_or(LENGTH_EDGES.len());
        lengths[slot].count += 1;
        lengths[slot].seconds += full;
        if longest.as_ref().is_none_or(|l| full > l.seconds) {
            longest = Some(LongestSession {
                channel_id: r.channel_id.clone(),
                channel_name: r.channel_name.clone(),
                category,
                started_at: s,
                seconds: full,
            });
        }
    }

    let mut channel_rows: Vec<ChannelShare> = channels
        .into_iter()
        .map(|(id, c)| ChannelShare {
            channel_id: id.to_string(),
            name: c.name.to_string(),
            seconds: c.seconds,
            sessions: c.sessions,
            last_watched_at: c.last,
        })
        .collect();
    sort_by_seconds(&mut channel_rows, |c| (c.seconds, c.name.as_str()));
    let channel_count = channel_rows.len();
    let rest = channel_rows.split_off(channel_rows.len().min(TOP_CHANNELS));
    let other_channels = OtherShare {
        count: rest.len(),
        seconds: rest.iter().map(|c| c.seconds).sum(),
    };

    let mut category_rows: Vec<CategoryShare> = categories
        .into_iter()
        .map(|(name, (seconds, sessions))| CategoryShare {
            name,
            seconds,
            sessions,
        })
        .collect();
    sort_by_seconds(&mut category_rows, |c| (c.seconds, c.name.as_str()));
    let category_count = category_rows.len();
    let series: Vec<String> = category_rows
        .iter()
        .take(TREND_SERIES)
        .map(|c| c.name.clone())
        .collect();
    let rest = category_rows.split_off(category_rows.len().min(TOP_CATEGORIES));
    let other_categories = OtherShare {
        count: rest.len(),
        seconds: rest.iter().map(|c| c.seconds).sum(),
    };

    // 2차: 추이 칸에 카테고리별로 쌓는다.
    let mut trend: Vec<TrendBucket> = starts
        .iter()
        .map(|start| TrendBucket {
            start: *start,
            seconds: 0.0,
            parts: vec![0.0; series.len() + 1],
        })
        .collect();
    for ((slot, secs), category) in pieces.iter().zip(&piece_category) {
        let Some(bucket) = trend.get_mut(*slot) else {
            continue;
        };
        bucket.seconds += secs;
        let part = series
            .iter()
            .position(|s| s == category)
            .unwrap_or(series.len());
        bucket.parts[part] += secs;
    }

    let previous_total = period.days().map(|n| {
        let lo = calendar.add_days(-n, range_start).unwrap_or(range_start);
        let hi = calendar.add_days(-n, now).unwrap_or(range_start);
        clipped_seconds(records, lo, hi, now)
    });

    let watched_days: BTreeSet<Date> = spans
        .iter()
        .flatten()
        .map(|(s, _, _)| calendar.start_of_day(*s))
        .collect();
    let (current_streak, best_streak) = streaks(&watched_days, today, calendar);

    WatchReport {
        period: period.key(),
        range_start,
        range_end: now,
        days,
        unit,
        total,
        previous_total,
        session_count,
        active_days: active_days.len(),
        average_session: if session_count > 0 {
            total / session_count as f64
        } else {
            0.0
        },
        daily_average: total / days as f64,
        longest,
        channel_count,
        category_count,
        current_streak,
        best_streak,
        first_watched_at,
        channels: channel_rows,
        other_channels,
        categories: category_rows,
        other_categories,
        heatmap,
        series,
        trend,
        lengths,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    fn seoul() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn at(d: u32, h: u32, m: u32) -> Date {
        seoul()
            .with_ymd_and_hms(2026, 11, d, h, m, 0)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn rec(ch: &str, cat: Option<&str>, start: Date, secs: f64) -> WatchHistory {
        let mut r = WatchHistory::new(ch, format!("{ch}이름"), None, cat.map(Into::into), start);
        r.duration = secs;
        r.ended_at = Some(adding(&start, secs));
        r.updated_at = r.ended_at;
        r
    }

    #[test]
    fn splits_sessions_across_hours_and_days() {
        // 11/2(월) 23:30 부터 2시간 — 월 23시 30분, 화 0시 60분, 화 1시 30분.
        let now = at(3, 12, 0);
        let rows = [rec("a", Some("게임"), at(2, 23, 30), 7200.0)];
        let r = build_report(&rows, ReportPeriod::Days7, now, &seoul());
        assert_eq!(r.total, 7200.0);
        assert_eq!(r.heatmap[0][23], 1800.0);
        assert_eq!(r.heatmap[1][0], 3600.0);
        assert_eq!(r.heatmap[1][1], 1800.0);
        assert_eq!(r.active_days, 2);
        assert_eq!(r.trend.len(), 7);
        assert_eq!(r.trend[5].seconds, 1800.0);
        assert_eq!(r.trend[6].seconds, 5400.0);
        assert_eq!(r.session_count, 1);
        assert_eq!(r.lengths[4].count, 1, "2~4시간 칸");
    }

    #[test]
    fn today_clips_to_midnight_and_compares_with_yesterday_so_far() {
        let now = at(3, 12, 0);
        let rows = [
            rec("a", None, at(2, 23, 0), 7200.0), // 오늘 몫 1시간
            rec("b", None, at(2, 9, 0), 1800.0),  // 어제 정오 전
            rec("b", None, at(2, 15, 0), 1800.0), // 어제 정오 뒤 — 비교에서 빠진다
        ];
        let r = build_report(&rows, ReportPeriod::Today, now, &seoul());
        assert_eq!(r.unit, BucketUnit::Hour);
        assert_eq!(r.trend.len(), 24);
        assert_eq!(r.total, 3600.0);
        assert_eq!(r.trend[0].seconds, 3600.0);
        assert_eq!(r.previous_total, Some(1800.0));
        assert_eq!(r.channels.len(), 1);
        assert_eq!(r.categories[0].name, "분류 없음");
    }

    #[test]
    fn ranks_channels_categories_and_stacks_trend_series() {
        let now = at(10, 20, 0);
        let mut rows = Vec::new();
        for (i, cat) in ["A", "B", "C", "D", "E", "F"].iter().enumerate() {
            rows.push(rec(
                &format!("c{i}"),
                Some(cat),
                at(9, 10, 0),
                600.0 * (6 - i) as f64,
            ));
        }
        let r = build_report(&rows, ReportPeriod::Days30, now, &seoul());
        assert_eq!(r.series, ["A", "B", "C", "D"]);
        let day = &r.trend[r.trend.len() - 2];
        assert_eq!(
            day.parts,
            vec![3600.0, 3000.0, 2400.0, 1800.0, 1200.0 + 600.0]
        );
        assert_eq!(r.channels[0].channel_id, "c0");
        assert_eq!(r.channel_count, 6);
        assert_eq!(r.other_channels.count, 0);
        assert_eq!(r.category_count, 6);
        assert_eq!(r.longest.as_ref().unwrap().channel_id, "c0");
    }

    #[test]
    fn channel_name_follows_latest_record() {
        let now = at(10, 20, 0);
        let mut old = rec("x", None, at(8, 10, 0), 60.0);
        old.channel_name = "옛 이름".into();
        let mut new = rec("x", None, at(9, 10, 0), 60.0);
        new.channel_name = "새 이름".into();
        let r = build_report(&[new, old], ReportPeriod::Days7, now, &seoul());
        assert_eq!(r.channels[0].name, "새 이름");
        assert_eq!(r.channels[0].sessions, 2);
    }

    #[test]
    fn all_period_picks_unit_from_span_and_counts_streaks() {
        let now = at(30, 12, 0);
        let rows: Vec<_> = [1, 2, 3, 10, 29, 30]
            .iter()
            .map(|d| rec("a", None, at(*d, 10, 0), 600.0))
            .collect();
        let r = build_report(&rows, ReportPeriod::All, now, &seoul());
        assert_eq!(r.days, 30);
        assert_eq!(r.unit, BucketUnit::Day);
        assert_eq!(r.trend.len(), 30);
        assert_eq!(r.current_streak, 2);
        assert_eq!(r.best_streak, 3);
        assert_eq!(r.previous_total, None);
        assert_eq!(r.daily_average, 3600.0 / 30.0);

        let mut long = rows.clone();
        long.push(rec(
            "a",
            None,
            seoul()
                .with_ymd_and_hms(2025, 1, 15, 10, 0, 0)
                .unwrap()
                .with_timezone(&chrono::Utc),
            600.0,
        ));
        let r = build_report(&long, ReportPeriod::All, now, &seoul());
        assert_eq!(r.unit, BucketUnit::Month);
        assert_eq!(r.trend.len(), 23, "2025-01 ~ 2026-11");
        assert_eq!(r.trend[0].seconds, 600.0);
    }

    #[test]
    fn empty_history_is_a_zero_report() {
        let now = at(3, 12, 0);
        let r = build_report(&[], ReportPeriod::Days90, now, &seoul());
        assert_eq!(r.total, 0.0);
        assert_eq!(r.unit, BucketUnit::Week);
        assert!(!r.trend.is_empty());
        assert!(r
            .trend
            .iter()
            .all(|b| b.seconds == 0.0 && b.parts.len() == 1));
        assert!(r.longest.is_none());
        assert_eq!(r.previous_total, Some(0.0));
    }

    #[test]
    fn week_buckets_start_on_monday() {
        let now = at(3, 12, 0); // 화요일
        let r = build_report(&[], ReportPeriod::Days90, now, &seoul());
        for b in &r.trend {
            assert_eq!(seoul().weekday(b.start), 0);
            assert_eq!(seoul().hour(b.start), 0);
        }
        assert!(r.trend.last().unwrap().start <= now);
    }

    #[test]
    fn calendar_hour_and_month_helpers() {
        let cal = seoul();
        assert_eq!(cal.next_hour(at(3, 12, 30)), at(3, 13, 0));
        assert_eq!(cal.next_hour(at(3, 12, 0)), at(3, 13, 0));
        assert_eq!(cal.start_of_month(at(17, 5, 0)), at(1, 0, 0));
        assert_eq!(cal.weekday(at(2, 0, 0)), 0, "2026-11-02 은 월요일");
    }
}
