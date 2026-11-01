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
    /// 인증 마크 종류(`"partner"` · `"challenger"` · `"official"`).
    pub verified_badge: Option<String>,
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
    /// 인증 마크 종류(`"partner"` · `"challenger"` · `"official"`).
    pub verified_badge: Option<String>,
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
            verified_badge: row.verified_badge.clone(),
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
        verified_badge: row.verified_badge.clone(),
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
mod tests;
