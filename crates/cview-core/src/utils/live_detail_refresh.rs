//! 팔로잉 폴의 liveDetail **선택 조회** 정책(순수 값 타입).
//!
//! 원본: `Sources/CViewCore/Utilities/LiveDetailRefreshPolicy.swift`
//!
//! "이번 폴에 어느 채널의 상세를 받을지"(`plan`)와 "받은 상세·지난 상세·목록 값을 어떻게
//! 한 행으로 합칠지"(`merge`)만 정한다. 네트워크·저장·시계는 호출자가 넘긴다.
//!
//! 규칙 요약(사유 우선순위 순):
//! - `NewLive` 직전 폴에 라이브가 아니던 채널. 맨 앞, 상한 없음.
//! - `MissingMeta` 상세를 한 번도 못 받은 채널. 보이는 채널 먼저, 폴당 24개까지.
//! - `NotifyWatch` 제목·카테고리 알림을 받는 채널. 목록에 liveInfo 가 없을 때만 매 폴.
//! - `VisibleDue` 보이는 채널 중 상세 나이 ≥ 120초.
//! - `BackgroundDue` 나머지 채널 중 상세 나이 ≥ 600초(목록 liveInfo 있음·가림이면 1800초).
//!   VisibleDue + BackgroundDue 는 합쳐서 오래된 순 12개까지.
//! - `Manual` 사용자 새로고침. 라이브 전부.
//!
//! ⚠ 시각은 **uptime**(단조 시계) 초다. `now` 보다 미래인 시각은 "기한 도래" 로 본다.
//!
//! 모델 의존 분리: 원본 `merge` 는 `LiveInfo` 를 받아 `LiveChannelItem` 을 만든다. 여기서는
//! 모델 크레이트와 독립적이도록 입력을 [`LiveDetailSnapshot`], 출력을 [`MergedFollowingItem`]
//! (같은 필드)으로 둔다. `LiveDetailNotifyWatch` 의 설정 타입도 [`NotifyWatchGeneral`] 과
//! [`ChannelNotificationLookup`] trait 으로 추상화했다.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::time::{Date, TimeInterval};

// MARK: - 입력

/// 팔로잉 목록 한 행(목록 응답에서 바로 얻는 값만).
///
/// `list_title`·`list_viewers`·`list_category` 는 목록 응답이 liveInfo 를 실어 올 때만 채워진다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowingRowSnapshot {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub is_live: bool,
    pub list_title: Option<String>,
    pub list_viewers: Option<i64>,
    pub list_category: Option<String>,
}

/// 채널별 마지막 liveDetail 결과. 한 방송(liveId) 동안만 유효하다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveDetailMeta {
    pub live_id: i64,
    /// 받은 시각(uptime 초). 기한 판정에만 쓴다.
    pub fetched_at_uptime: TimeInterval,
    pub title: String,
    pub viewers: i64,
    pub category: Option<String>,
    pub category_type: Option<String>,
    pub thumbnail_url: Option<String>,
    pub open_date: Option<Date>,
    /// 상세가 `.close` 를 말했으면 false.
    pub is_open: bool,
}

/// 이번 폴에 받은 liveDetail 의 병합에 필요한 값(원본 `LiveInfo` 의 부분집합).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveDetailSnapshot {
    pub live_id: i64,
    pub live_title: String,
    pub concurrent_user_count: i64,
    pub live_category_value: Option<String>,
    pub category_type: Option<String>,
    /// `LiveInfo.resolvedLiveImageURL?.absoluteString`.
    pub thumbnail_url: Option<String>,
    pub open_date: Option<Date>,
    /// `status == .close`.
    pub is_closed: bool,
}

/// 이번 폴에 상세를 받는 이유. 선언 순서가 우선순위다(`NewLive` 가 가장 앞).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LiveDetailReason {
    NewLive,
    MissingMeta,
    NotifyWatch,
    VisibleDue,
    BackgroundDue,
    Manual,
}

impl LiveDetailReason {
    pub const ALL: [Self; 6] = [
        Self::NewLive,
        Self::MissingMeta,
        Self::NotifyWatch,
        Self::VisibleDue,
        Self::BackgroundDue,
        Self::Manual,
    ];
}

/// 계획 한 줄 — 이 채널의 상세를 이 사유로 받는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDetailRequest {
    pub channel_id: String,
    pub reason: LiveDetailReason,
}

impl LiveDetailRequest {
    pub fn new(channel_id: impl Into<String>, reason: LiveDetailReason) -> Self {
        Self {
            channel_id: channel_id.into(),
            reason,
        }
    }
}

/// 기한·상한 설정. 기본값은 계획서 수치다.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDetailRefreshConfig {
    /// 보이는 카드의 상세 수명.
    pub visible_ttl: TimeInterval,
    /// 보이지 않는 채널의 상세 수명.
    pub background_ttl: TimeInterval,
    /// 목록이 제목·시청자·카테고리를 실어 올 때 보이지 않는 채널의 상세 수명.
    pub background_ttl_when_list_has_live_info: TimeInterval,
    /// 가림(창이 안 보임) 동안의 상세 수명.
    pub suspended_ttl: TimeInterval,
    /// VisibleDue + BackgroundDue 합의 폴당 상한.
    pub due_cap_per_poll: usize,
    /// MissingMeta 의 폴당 상한.
    pub missing_meta_cap_per_poll: usize,
}

impl Default for LiveDetailRefreshConfig {
    fn default() -> Self {
        Self {
            visible_ttl: 120.0,
            background_ttl: 600.0,
            background_ttl_when_list_has_live_info: 1800.0,
            suspended_ttl: 1800.0,
            due_cap_per_poll: 12,
            missing_meta_cap_per_poll: 24,
        }
    }
}

impl LiveDetailRefreshConfig {
    /// 보이지 않는 채널의 수명. 가림이 가장 먼저다.
    pub fn background_ttl_for(
        &self,
        list_carries_live_info: bool,
        suspended: bool,
    ) -> TimeInterval {
        if suspended {
            self.suspended_ttl
        } else if list_carries_live_info {
            self.background_ttl_when_list_has_live_info
        } else {
            self.background_ttl
        }
    }
}

/// 병합된 화면 행 — 원본 `LiveChannelItem` 의 해당 필드.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MergedFollowingItem {
    pub id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub live_title: String,
    pub viewer_count: i64,
    pub category_name: Option<String>,
    pub category_type: Option<String>,
    pub thumbnail_url: Option<String>,
    pub channel_id: String,
    pub is_live: bool,
    pub open_date: Option<Date>,
}

/// 병합 결과 — 화면 행과, 다음 폴에 넘길 meta(`None` 이면 지운다).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDetailMergeResult {
    pub item: MergedFollowingItem,
    pub meta: Option<LiveDetailMeta>,
}

/// `plan` 의 상황 입력.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanOptions {
    /// 목록 응답이 제목·시청자·카테고리를 실어 오는지.
    pub list_carries_live_info: bool,
    /// 창이 가려져 있는지(가림에는 보이는 표면이 없다).
    pub suspended: bool,
    /// 사용자 새로고침인지.
    pub manual: bool,
}

/// 이번 폴에 받을 상세 목록. **라이브 행만** 대상이다.
///
/// 결과 순서는 NewLive → MissingMeta → NotifyWatch → 기한 도래(오래된 순)이고,
/// manual 이면 NewLive → Manual.
///
/// - `previous_live`: 직전 폴의 라이브 채널 집합. `None` 이면 기준이 없다(부팅) — NewLive 를 만들지 않는다.
///   빈 집합은 "직전 폴에 라이브가 하나도 없었다" 는 뜻이라 지금 라이브 전부가 NewLive 다.
/// - `now`: 지금 uptime(초).
#[allow(clippy::too_many_arguments)]
pub fn plan(
    rows: &[FollowingRowSnapshot],
    previous_live: Option<&HashSet<String>>,
    meta: &HashMap<String, LiveDetailMeta>,
    visible: &HashSet<String>,
    notify_watch: impl Fn(&str) -> bool,
    options: PlanOptions,
    now: TimeInterval,
    config: &LiveDetailRefreshConfig,
) -> Vec<LiveDetailRequest> {
    let empty = HashSet::new();
    let mut planner = Planner::new(rows, if options.suspended { &empty } else { visible });
    if planner.live.is_empty() {
        return Vec::new();
    }

    // 1) 새 라이브 — 맨 앞, 상한 없음.
    if let Some(prev) = previous_live {
        let fresh: Vec<_> = planner
            .live
            .iter()
            .copied()
            .filter(|r| !prev.contains(&r.channel_id))
            .collect();
        planner.take(&fresh, LiveDetailReason::NewLive, usize::MAX);
    }

    // manual — 라이브 전부(보이는 채널 먼저).
    if options.manual {
        let all = planner.visible_first();
        planner.take(&all, LiveDetailReason::Manual, usize::MAX);
        return planner.requests;
    }

    // 2) 상세가 없는 채널 — 보이는 채널 먼저, 상한 안에서.
    let missing: Vec<_> = planner
        .visible_first()
        .into_iter()
        .filter(|r| !meta.contains_key(&r.channel_id))
        .collect();
    planner.take(
        &missing,
        LiveDetailReason::MissingMeta,
        config.missing_meta_cap_per_poll,
    );

    // 3) 알림 감시 채널 — 목록이 제목·카테고리를 싣지 않을 때만, 상한 없음.
    if !options.list_carries_live_info {
        let watched: Vec<_> = planner
            .live
            .iter()
            .copied()
            .filter(|r| notify_watch(&r.channel_id))
            .collect();
        planner.take(&watched, LiveDetailReason::NotifyWatch, usize::MAX);
    }

    // 4) 기한 도래 — 합쳐 오래된 순으로 상한까지. meta 가 없는 채널은 기한 대상이 아니다.
    planner.take_due(
        meta,
        now,
        config.visible_ttl,
        config.background_ttl_for(options.list_carries_live_info, options.suspended),
        config.due_cap_per_poll,
    );
    planner.requests
}

/// 목록 행 + 이번 상세 + 지난 meta → 화면 행과 새 meta.
///
/// - `is_live = row.is_live && 상세가 close 가 아님`. 이번 상세가 없으면 지난 meta 의 `is_open` 을 쓴다.
/// - 제목·시청자·카테고리: 목록 liveInfo > 이번 상세 > meta.
/// - 썸네일·openDate·categoryType: 이번 상세 > (같은 방송의) meta.
/// - 이번 상세의 liveId 가 meta 와 다르면 meta 를 통째로 바꾼다.
/// - `detail == None` 이면 meta 를 그대로 둔다.
/// - 목록이 오프라인이면 meta 를 지운다.
pub fn merge(
    row: &FollowingRowSnapshot,
    detail: Option<&LiveDetailSnapshot>,
    meta: Option<&LiveDetailMeta>,
    now: TimeInterval,
) -> LiveDetailMergeResult {
    if !row.is_live {
        let offline = MergedFollowingItem {
            id: row.channel_id.clone(),
            channel_name: row.channel_name.clone(),
            channel_image_url: row.channel_image_url.clone(),
            channel_id: row.channel_id.clone(),
            ..MergedFollowingItem::default()
        };
        return LiveDetailMergeResult {
            item: offline,
            meta: None,
        };
    }

    let source: Option<LiveDetailMeta> = match detail {
        Some(d) => {
            let carry = meta.filter(|m| m.live_id == d.live_id);
            Some(LiveDetailMeta {
                live_id: d.live_id,
                fetched_at_uptime: now,
                title: d.live_title.clone(),
                viewers: d.concurrent_user_count,
                category: d.live_category_value.clone(),
                category_type: d
                    .category_type
                    .clone()
                    .or_else(|| carry.and_then(|c| c.category_type.clone())),
                thumbnail_url: d
                    .thumbnail_url
                    .clone()
                    .or_else(|| carry.and_then(|c| c.thumbnail_url.clone())),
                open_date: d.open_date.or_else(|| carry.and_then(|c| c.open_date)),
                is_open: !d.is_closed,
            })
        }
        None => meta.cloned(),
    };

    let is_live = source.as_ref().is_none_or(|s| s.is_open);
    let item = MergedFollowingItem {
        id: row.channel_id.clone(),
        channel_name: row.channel_name.clone(),
        channel_image_url: row.channel_image_url.clone(),
        live_title: row
            .list_title
            .clone()
            .or_else(|| source.as_ref().map(|s| s.title.clone()))
            .unwrap_or_default(),
        viewer_count: row
            .list_viewers
            .or(source.as_ref().map(|s| s.viewers))
            .unwrap_or(0),
        category_name: row
            .list_category
            .clone()
            .or_else(|| source.as_ref().and_then(|s| s.category.clone())),
        category_type: source.as_ref().and_then(|s| s.category_type.clone()),
        thumbnail_url: source.as_ref().and_then(|s| s.thumbnail_url.clone()),
        channel_id: row.channel_id.clone(),
        is_live,
        open_date: if is_live {
            source.as_ref().and_then(|s| s.open_date)
        } else {
            None
        },
    };
    LiveDetailMergeResult { item, meta: source }
}

// MARK: - 계획 누산기

/// 한 번의 `plan` 동안 쌓는 상태 — 채널은 한 사유로 한 번만 뽑힌다.
struct Planner<'a> {
    /// 라이브 행(목록 순, channelId 중복·빈 id 제외).
    live: Vec<&'a FollowingRowSnapshot>,
    visible: &'a HashSet<String>,
    requests: Vec<LiveDetailRequest>,
    taken: HashSet<&'a str>,
}

impl<'a> Planner<'a> {
    fn new(rows: &'a [FollowingRowSnapshot], visible: &'a HashSet<String>) -> Self {
        let mut seen = HashSet::new();
        let live = rows
            .iter()
            .filter(|r| r.is_live && !r.channel_id.is_empty() && seen.insert(r.channel_id.as_str()))
            .collect();
        Self {
            live,
            visible,
            requests: Vec::new(),
            taken: HashSet::new(),
        }
    }

    /// 보이는 채널 먼저, 그 안에서는 목록 순.
    fn visible_first(&self) -> Vec<&'a FollowingRowSnapshot> {
        let (vis, rest): (Vec<_>, Vec<_>) = self
            .live
            .iter()
            .partition(|r| self.visible.contains(&r.channel_id));
        vis.into_iter().chain(rest).collect()
    }

    /// 아직 뽑히지 않은 행을 순서대로 `limit` 개까지 뽑는다.
    fn take(&mut self, rows: &[&'a FollowingRowSnapshot], reason: LiveDetailReason, limit: usize) {
        let mut budget = limit;
        for row in rows {
            if budget == 0 {
                break;
            }
            if self.taken.insert(row.channel_id.as_str()) {
                self.requests
                    .push(LiveDetailRequest::new(row.channel_id.clone(), reason));
                budget -= 1;
            }
        }
    }

    /// 기한이 지난 채널을 오래된 순(같으면 목록 순)으로 `limit` 개까지 뽑는다.
    fn take_due(
        &mut self,
        meta: &HashMap<String, LiveDetailMeta>,
        now: TimeInterval,
        visible_ttl: TimeInterval,
        background_ttl: TimeInterval,
        limit: usize,
    ) {
        struct Candidate {
            index: usize,
            age: f64,
            is_visible: bool,
        }
        let mut due: Vec<Candidate> = Vec::new();
        for (index, row) in self.live.iter().enumerate() {
            if self.taken.contains(row.channel_id.as_str()) {
                continue;
            }
            let Some(entry) = meta.get(&row.channel_id) else {
                continue;
            };
            let is_visible = self.visible.contains(&row.channel_id);
            let raw_age = now - entry.fetched_at_uptime;
            // 미래 시각(재부팅 뒤 남은 uptime·시계 역행)은 가장 오래된 것으로 친다.
            let age = if raw_age < 0.0 {
                f64::INFINITY
            } else {
                raw_age
            };
            if age
                < if is_visible {
                    visible_ttl
                } else {
                    background_ttl
                }
            {
                continue;
            }
            due.push(Candidate {
                index,
                age,
                is_visible,
            });
        }
        due.sort_by(|a, b| b.age.total_cmp(&a.age).then(a.index.cmp(&b.index)));
        for c in due.into_iter().take(limit) {
            let row = self.live[c.index];
            let reason = if c.is_visible {
                LiveDetailReason::VisibleDue
            } else {
                LiveDetailReason::BackgroundDue
            };
            self.requests
                .push(LiveDetailRequest::new(row.channel_id.clone(), reason));
            self.taken.insert(row.channel_id.as_str());
        }
    }
}

// MARK: - 알림 감시 판정

/// 알림 싱크가 보는 전역 설정(원본 `GeneralSettings` 의 해당 필드).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NotifyWatchGeneral {
    pub notifications_enabled: bool,
    pub auto_refresh_enabled: bool,
    pub notify_on_title_change: bool,
    pub notify_on_category_change: bool,
}

/// 채널별 알림 설정 조회(원본 `ChannelNotificationSettings`).
pub trait ChannelNotificationLookup {
    fn is_title_change_notification_enabled(&self, channel_id: &str) -> bool;
    fn is_category_change_notification_enabled(&self, channel_id: &str) -> bool;
}

/// 제목·카테고리 알림을 받는 채널인지 — 알림 싱크의 필터와 같은 판정.
///
/// 싱크는 ① 자동 새로고침이 켜져 있어야 돌고 ② 알림이 켜져 있을 때만 알리며
/// ③ 종류별 전역 스위치와 채널별 설정을 AND 한다.
pub fn notify_watches(
    channel_id: &str,
    general: &NotifyWatchGeneral,
    channels: &impl ChannelNotificationLookup,
) -> bool {
    if !general.notifications_enabled || !general.auto_refresh_enabled {
        return false;
    }
    (general.notify_on_title_change && channels.is_title_change_notification_enabled(channel_id))
        || (general.notify_on_category_change
            && channels.is_category_change_notification_enabled(channel_id))
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/LiveDetailRefreshPolicyTests.swift`
    use super::*;
    use crate::utils::time::from_unix;
    use LiveDetailReason as R;

    const NOW: f64 = 10_000.0;

    fn row(id: &str) -> FollowingRowSnapshot {
        FollowingRowSnapshot {
            channel_id: id.into(),
            channel_name: format!("name-{id}"),
            channel_image_url: Some(format!("https://img/{id}")),
            is_live: true,
            ..Default::default()
        }
    }

    fn offline(id: &str) -> FollowingRowSnapshot {
        FollowingRowSnapshot {
            is_live: false,
            ..row(id)
        }
    }

    fn meta_at(fetched_at: f64) -> LiveDetailMeta {
        meta_with(1, fetched_at)
    }

    fn meta_with(live_id: i64, fetched_at: f64) -> LiveDetailMeta {
        LiveDetailMeta {
            live_id,
            fetched_at_uptime: fetched_at,
            title: "meta-title".into(),
            viewers: 10,
            category: Some("meta-cat".into()),
            category_type: Some("GAME".into()),
            thumbnail_url: Some("https://thumb/meta".into()),
            open_date: Some(from_unix(1_000.0)),
            is_open: true,
        }
    }

    fn detail() -> LiveDetailSnapshot {
        LiveDetailSnapshot {
            live_id: 1,
            live_title: "detail-title".into(),
            concurrent_user_count: 20,
            live_category_value: Some("detail-cat".into()),
            category_type: Some("SPORTS".into()),
            thumbnail_url: Some("https://thumb/detail".into()),
            open_date: Some(from_unix(2_000.0)),
            is_closed: false,
        }
    }

    fn bare_detail(live_id: i64) -> LiveDetailSnapshot {
        LiveDetailSnapshot {
            live_id,
            category_type: None,
            thumbnail_url: None,
            open_date: None,
            ..detail()
        }
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn ids(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    #[derive(Default)]
    struct P {
        previous_live: Option<HashSet<String>>,
        meta: HashMap<String, LiveDetailMeta>,
        visible: HashSet<String>,
        watching: HashSet<String>,
        options: PlanOptions,
        now: Option<f64>,
    }

    fn run(rows: &[FollowingRowSnapshot], p: P) -> Vec<LiveDetailRequest> {
        plan(
            rows,
            p.previous_live.as_ref(),
            &p.meta,
            &p.visible,
            |id| p.watching.contains(id),
            p.options,
            p.now.unwrap_or(NOW),
            &LiveDetailRefreshConfig::default(),
        )
    }

    fn req(id: &str, r: R) -> LiveDetailRequest {
        LiveDetailRequest::new(id, r)
    }

    #[test]
    fn no_live_rows_plans_nothing() {
        let rows = [offline("a"), offline("b")];
        let p = P {
            previous_live: Some(set(&[])),
            options: PlanOptions {
                manual: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(run(&rows, p).is_empty());
    }

    #[test]
    fn fresh_meta_plans_nothing() {
        let all = ids("c", 71);
        let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
        let meta = all
            .iter()
            .map(|i| (i.clone(), meta_at(NOW - 30.0)))
            .collect();
        let p = P {
            previous_live: Some(all.iter().cloned().collect()),
            meta,
            visible: all.iter().take(6).cloned().collect(),
            ..Default::default()
        };
        assert!(run(&rows, p).is_empty());
    }

    #[test]
    fn new_live_first_and_uncapped() {
        let old = ids("old", 5);
        let new = ids("new", 30);
        let mut rows: Vec<_> = old.iter().chain(&new).map(|i| row(i)).collect();
        rows.push(offline("off"));
        let meta = old
            .iter()
            .map(|i| (i.clone(), meta_at(NOW - 5_000.0)))
            .collect();
        let result = run(
            &rows,
            P {
                previous_live: Some(old.iter().cloned().collect()),
                meta,
                visible: set(&[&old[0]]),
                ..Default::default()
            },
        );
        let new_live: Vec<_> = result
            .iter()
            .take_while(|r| r.reason == R::NewLive)
            .map(|r| r.channel_id.clone())
            .collect();
        assert_eq!(new_live, new);
        assert_eq!(result.len(), new.len() + old.len());
        assert!(result[new.len()..]
            .iter()
            .all(|r| matches!(r.reason, R::VisibleDue | R::BackgroundDue)));
        assert!(!result.iter().any(|r| r.channel_id == "off"));
    }

    #[test]
    fn previous_live_none_versus_empty() {
        let rows = [row("a"), row("b")];
        assert!(run(&rows, P::default())
            .iter()
            .all(|r| r.reason == R::MissingMeta));
        let after = run(
            &rows,
            P {
                previous_live: Some(set(&[])),
                ..Default::default()
            },
        );
        assert_eq!(
            after.iter().map(|r| r.reason).collect::<Vec<_>>(),
            vec![R::NewLive, R::NewLive]
        );
    }

    #[test]
    fn missing_meta_cap_visible_first() {
        let all = ids("c", 30);
        let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
        let first = run(
            &rows,
            P {
                visible: set(&["c25", "c28"]),
                ..Default::default()
            },
        );
        assert_eq!(first.len(), 24);
        assert!(first.iter().all(|r| r.reason == R::MissingMeta));
        assert_eq!(
            first[..2]
                .iter()
                .map(|r| r.channel_id.as_str())
                .collect::<Vec<_>>(),
            ["c25", "c28"]
        );
        assert_eq!(
            first[2..]
                .iter()
                .map(|r| r.channel_id.clone())
                .collect::<Vec<_>>(),
            all[..22].to_vec()
        );

        let meta = first
            .iter()
            .map(|r| (r.channel_id.clone(), meta_at(NOW)))
            .collect();
        let second = run(
            &rows,
            P {
                previous_live: Some(all.iter().cloned().collect()),
                meta,
                visible: set(&["c25", "c28"]),
                now: Some(NOW + 60.0),
                ..Default::default()
            },
        );
        assert_eq!(
            second
                .iter()
                .map(|r| r.channel_id.as_str())
                .collect::<Vec<_>>(),
            ["c22", "c23", "c24", "c26", "c27", "c29"]
        );
        assert!(second.iter().all(|r| r.reason == R::MissingMeta));
    }

    #[test]
    fn notify_watch_only_without_list_live_info() {
        let rows = [row("a"), row("b"), row("c")];
        let meta: HashMap<_, _> = ["a", "b", "c"]
            .iter()
            .map(|i| (i.to_string(), meta_at(NOW - 1.0)))
            .collect();
        let without = run(
            &rows,
            P {
                previous_live: Some(set(&["a", "b", "c"])),
                meta: meta.clone(),
                watching: set(&["b", "c"]),
                ..Default::default()
            },
        );
        assert_eq!(
            without,
            vec![req("b", R::NotifyWatch), req("c", R::NotifyWatch)]
        );
        let with = run(
            &rows,
            P {
                previous_live: Some(set(&["a", "b", "c"])),
                meta,
                watching: set(&["b", "c"]),
                options: PlanOptions {
                    list_carries_live_info: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(with.is_empty());
    }

    #[test]
    fn notify_watch_beyond_missing_meta_cap() {
        let rows: Vec<_> = ids("c", 30).iter().map(|i| row(i)).collect();
        let result = run(
            &rows,
            P {
                watching: set(&["c29"]),
                ..Default::default()
            },
        );
        assert_eq!(result.len(), 25);
        assert_eq!(result.last(), Some(&req("c29", R::NotifyWatch)));
    }

    fn single(meta: LiveDetailMeta, visible: bool, options: PlanOptions) -> Vec<LiveDetailRequest> {
        run(
            &[row("a")],
            P {
                previous_live: Some(set(&["a"])),
                meta: HashMap::from([("a".to_string(), meta)]),
                visible: if visible { set(&["a"]) } else { set(&[]) },
                options,
                ..Default::default()
            },
        )
    }

    #[test]
    fn visible_boundary() {
        assert!(single(meta_at(NOW - 119.9), true, PlanOptions::default()).is_empty());
        assert_eq!(
            single(meta_at(NOW - 120.0), true, PlanOptions::default()),
            vec![req("a", R::VisibleDue)]
        );
    }

    #[test]
    fn background_boundary() {
        let d = PlanOptions::default();
        let info = PlanOptions {
            list_carries_live_info: true,
            ..Default::default()
        };
        assert!(single(meta_at(NOW - 599.9), false, d).is_empty());
        assert_eq!(
            single(meta_at(NOW - 600.0), false, d),
            vec![req("a", R::BackgroundDue)]
        );
        assert!(single(meta_at(NOW - 600.0), false, info).is_empty());
        assert!(single(meta_at(NOW - 1799.9), false, info).is_empty());
        assert_eq!(
            single(meta_at(NOW - 1800.0), false, info),
            vec![req("a", R::BackgroundDue)]
        );
    }

    #[test]
    fn suspended_boundary() {
        let sus = PlanOptions {
            suspended: true,
            ..Default::default()
        };
        assert!(single(meta_at(NOW - 600.0), true, sus).is_empty());
        assert!(single(meta_at(NOW - 1799.9), false, sus).is_empty());
        assert_eq!(
            single(meta_at(NOW - 1800.0), false, sus),
            vec![req("a", R::BackgroundDue)]
        );
    }

    #[test]
    fn due_cap_oldest_first() {
        let all = ids("c", 20);
        let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
        let mut meta: HashMap<String, LiveDetailMeta> = all
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), meta_at(NOW - 700.0 - i as f64 * 50.0)))
            .collect();
        let c3 = meta["c3"].clone();
        meta.insert("c4".into(), c3);
        let prev: HashSet<String> = all.iter().cloned().collect();
        let result = run(
            &rows,
            P {
                previous_live: Some(prev.clone()),
                meta: meta.clone(),
                visible: set(&["c1"]),
                ..Default::default()
            },
        );
        assert_eq!(result.len(), 12);
        let expected: Vec<String> = (8..20).rev().map(|i| format!("c{i}")).collect();
        assert_eq!(
            result
                .iter()
                .map(|r| r.channel_id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(result.iter().all(|r| r.reason == R::BackgroundDue));

        meta.insert("c1".into(), meta_at(NOW - 1_000_000.0));
        let with_visible = run(
            &rows,
            P {
                previous_live: Some(prev),
                meta,
                visible: set(&["c1"]),
                ..Default::default()
            },
        );
        assert_eq!(with_visible.first(), Some(&req("c1", R::VisibleDue)));
        assert_eq!(with_visible.len(), 12);
    }

    #[test]
    fn due_tie_keeps_list_order() {
        let all = ids("c", 15);
        let meta = all
            .iter()
            .map(|i| (i.clone(), meta_at(NOW - 900.0)))
            .collect();
        let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
        let result = run(
            &rows,
            P {
                previous_live: Some(all.iter().cloned().collect()),
                meta,
                ..Default::default()
            },
        );
        assert_eq!(
            result
                .iter()
                .map(|r| r.channel_id.clone())
                .collect::<Vec<_>>(),
            all[..12].to_vec()
        );
    }

    #[test]
    fn future_meta_is_due() {
        assert_eq!(
            single(meta_at(NOW + 50_000.0), false, PlanOptions::default()),
            vec![req("a", R::BackgroundDue)]
        );
    }

    #[test]
    fn manual_takes_all_live() {
        let rows = [row("a"), row("b"), offline("off"), row("c"), row("n")];
        let meta = ["a", "b", "c"]
            .iter()
            .map(|i| (i.to_string(), meta_at(NOW)))
            .collect();
        let result = run(
            &rows,
            P {
                previous_live: Some(set(&["a", "b", "c"])),
                meta,
                visible: set(&["c"]),
                options: PlanOptions {
                    manual: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!(
            result,
            vec![
                req("n", R::NewLive),
                req("c", R::Manual),
                req("a", R::Manual),
                req("b", R::Manual)
            ]
        );
    }

    #[test]
    fn duplicate_rows_planned_once() {
        let result = run(
            &[row("a"), row("a"), row("")],
            P {
                previous_live: Some(set(&[])),
                ..Default::default()
            },
        );
        assert_eq!(result, vec![req("a", R::NewLive)]);
    }

    #[test]
    fn reason_ordering() {
        let mut all = LiveDetailReason::ALL;
        all.sort();
        assert_eq!(
            all,
            [
                R::NewLive,
                R::MissingMeta,
                R::NotifyWatch,
                R::VisibleDue,
                R::BackgroundDue,
                R::Manual
            ]
        );
    }

    // MARK: 병합

    #[test]
    fn merge_list_wins() {
        let r = FollowingRowSnapshot {
            list_title: Some("list-title".into()),
            list_viewers: Some(99),
            list_category: Some("list-cat".into()),
            ..row("a")
        };
        let listed = merge(&r, Some(&detail()), Some(&meta_at(0.0)), NOW);
        assert_eq!(listed.item.live_title, "list-title");
        assert_eq!(listed.item.viewer_count, 99);
        assert_eq!(listed.item.category_name.as_deref(), Some("list-cat"));
        assert_eq!(
            listed.item.thumbnail_url.as_deref(),
            Some("https://thumb/detail")
        );
        assert_eq!(listed.item.category_type.as_deref(), Some("SPORTS"));
        assert_eq!(listed.item.open_date, Some(from_unix(2_000.0)));
        let m = listed.meta.unwrap();
        assert_eq!(m.title, "detail-title");
        assert_eq!(m.fetched_at_uptime, NOW);
    }

    #[test]
    fn merge_detail_beats_meta() {
        let d = merge(&row("a"), Some(&detail()), Some(&meta_at(0.0)), NOW);
        assert_eq!(d.item.live_title, "detail-title");
        assert_eq!(d.item.viewer_count, 20);
        assert_eq!(d.item.category_name.as_deref(), Some("detail-cat"));
        assert!(d.item.is_live);
    }

    #[test]
    fn merge_falls_back_to_meta() {
        let old = meta_at(0.0);
        let m = merge(&row("a"), None, Some(&old), NOW);
        assert_eq!(m.item.live_title, "meta-title");
        assert_eq!(m.item.viewer_count, 10);
        assert_eq!(m.item.category_name.as_deref(), Some("meta-cat"));
        assert_eq!(m.item.category_type.as_deref(), Some("GAME"));
        assert_eq!(m.item.thumbnail_url.as_deref(), Some("https://thumb/meta"));
        assert!(m.item.is_live);
        assert_eq!(m.meta, Some(old));
    }

    #[test]
    fn merge_partial_list_values() {
        let r = FollowingRowSnapshot {
            list_viewers: Some(7),
            ..row("a")
        };
        let m = merge(&r, None, Some(&meta_at(0.0)), NOW);
        assert_eq!(m.item.viewer_count, 7);
        assert_eq!(m.item.live_title, "meta-title");
        assert_eq!(m.item.channel_name, "name-a");
        assert_eq!(m.item.channel_image_url.as_deref(), Some("https://img/a"));
        assert_eq!(m.item.id, "a");
    }

    #[test]
    fn merge_same_live_carries_missing_fields() {
        let old = meta_with(5, 0.0);
        let r = merge(&row("a"), Some(&bare_detail(5)), Some(&old), NOW);
        assert_eq!(r.item.thumbnail_url.as_deref(), Some("https://thumb/meta"));
        assert_eq!(r.item.open_date, Some(from_unix(1_000.0)));
        assert_eq!(r.item.category_type.as_deref(), Some("GAME"));
        assert_eq!(
            r.meta.unwrap().thumbnail_url.as_deref(),
            Some("https://thumb/meta")
        );
    }

    #[test]
    fn merge_live_id_change_replaces_meta() {
        let old = meta_with(5, 0.0);
        let r = merge(&row("a"), Some(&bare_detail(6)), Some(&old), NOW);
        let m = r.meta.clone().unwrap();
        assert_eq!(m.live_id, 6);
        assert_eq!(m.thumbnail_url, None);
        assert_eq!(m.open_date, None);
        assert_eq!(m.category_type, None);
        assert_eq!(r.item.thumbnail_url, None);
        assert_eq!(r.item.live_title, "detail-title");
    }

    #[test]
    fn offline_drops_meta_then_restart_is_new_live() {
        let old = meta_with(5, NOW - 10.0);
        let off = merge(&offline("a"), None, Some(&old), NOW);
        assert_eq!(off.meta, None);
        assert!(!off.item.is_live);
        assert!(off.item.live_title.is_empty());
        assert_eq!(off.item.thumbnail_url, None);
        assert_eq!(off.item.open_date, None);

        let p = run(
            &[row("a")],
            P {
                previous_live: Some(set(&[])),
                now: Some(NOW + 60.0),
                ..Default::default()
            },
        );
        assert_eq!(p, vec![req("a", R::NewLive)]);

        let restarted = merge(
            &row("a"),
            Some(&LiveDetailSnapshot {
                live_id: 6,
                ..detail()
            }),
            off.meta.as_ref(),
            NOW + 60.0,
        );
        assert_eq!(restarted.meta.unwrap().live_id, 6);
        assert!(restarted.item.is_live);
        assert_eq!(restarted.item.open_date, Some(from_unix(2_000.0)));
    }

    #[test]
    fn merge_close_detail() {
        let closed = merge(
            &row("a"),
            Some(&LiveDetailSnapshot {
                is_closed: true,
                ..detail()
            }),
            None,
            NOW,
        );
        assert!(!closed.item.is_live);
        assert_eq!(closed.item.open_date, None);
        let closed_meta = closed.meta.unwrap();
        assert!(!closed_meta.is_open);

        let next = merge(&row("a"), None, Some(&closed_meta), NOW + 60.0);
        assert!(
            !next.item.is_live,
            "폴마다 라이브↔오프라인이 뒤집히면 배지·알림이 흔들린다"
        );

        let p = run(
            &[row("a")],
            P {
                previous_live: Some(set(&["a"])),
                meta: HashMap::from([("a".to_string(), closed_meta)]),
                now: Some(NOW + 600.0),
                ..Default::default()
            },
        );
        assert_eq!(p, vec![req("a", R::BackgroundDue)]);
    }

    #[test]
    fn merge_without_any_detail() {
        let r = merge(&row("a"), None, None, NOW);
        assert_eq!(r.meta, None);
        assert!(r.item.is_live);
        assert!(r.item.live_title.is_empty());
        assert_eq!(r.item.viewer_count, 0);
        assert_eq!(r.item.category_name, None);
    }

    // MARK: 알림 감시 판정

    /// 채널별 설정 하나("set")만 있는 조회 — 없는 채널은 둘 다 꺼짐.
    struct OneChannel {
        category: bool,
        title: bool,
    }

    impl ChannelNotificationLookup for OneChannel {
        fn is_title_change_notification_enabled(&self, id: &str) -> bool {
            id == "set" && self.title
        }
        fn is_category_change_notification_enabled(&self, id: &str) -> bool {
            id == "set" && self.category
        }
    }

    fn sink_would_notify(id: &str, g: &NotifyWatchGeneral, c: &OneChannel) -> bool {
        if !g.auto_refresh_enabled || !g.notifications_enabled {
            return false;
        }
        let category = g.notify_on_category_change && c.is_category_change_notification_enabled(id);
        let title = g.notify_on_title_change && c.is_title_change_notification_enabled(id);
        category || title
    }

    #[test]
    fn notify_watch_matches_sink_filter() {
        for mask in 0..64u32 {
            let g = NotifyWatchGeneral {
                notifications_enabled: mask & 1 != 0,
                notify_on_category_change: mask & 2 != 0,
                notify_on_title_change: mask & 4 != 0,
                auto_refresh_enabled: mask & 8 != 0,
            };
            let c = OneChannel {
                category: mask & 16 != 0,
                title: mask & 32 != 0,
            };
            for id in ["set", "absent"] {
                assert_eq!(
                    notify_watches(id, &g, &c),
                    sink_would_notify(id, &g, &c),
                    "mask={mask} id={id}"
                );
            }
        }
    }

    #[test]
    fn default_settings_watch_nothing() {
        assert!(!notify_watches(
            "a",
            &NotifyWatchGeneral::default(),
            &OneChannel {
                category: true,
                title: true
            }
        ));
    }
}
