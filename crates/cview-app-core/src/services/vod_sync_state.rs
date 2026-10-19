//! `Sources/CViewApp/Services/VODSyncState.swift` 포팅.
//!
//! 팔로우 VOD 증분 동기화의 순수 부분 — 네트워크·저장·시계 없이 값만 다룬다.
//!
//! - [`VodSyncState`] — `vod-sync.‹scope›.v1.json` 스냅숏 본문.
//! - [`VodSyncPolicy`] — 채널별 주기(방송 종료 3시간 안 30분 / 최근 7일 VOD 6시간 / 그 밖 24시간 /
//!   VOD 없는 채널 백오프 최대 7일), 방송 종료 10분 유예, 실패 5분 제외, 진입 예산.
//! - [`VodSyncPlanner`] — 한 번의 패스에 받을 채널을 우선순위(T0 → T3)로 고른다.
//! - [`VodSubscriptionTag`] — 계정 토큰을 만드는 한곳.

use std::collections::{HashMap, HashSet};

use cview_core::utils::time::{adding, interval_since, Date, TimeInterval};
use cview_persistence::models::cached_vod::CachedVod;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use super::support::KeyValueStore;

/// 채널 한 곳의 동기화 기록.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VodSyncChannel {
    pub channel_id: String,
    /// 마지막으로 page 0 을 **성공적으로** 받은 시각.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_video_no: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_publish_date: Option<Date>,
    /// 마지막으로 본 방송 종료 시각.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_live_ended_at: Option<Date>,
    /// page 0 이 연달아 빈 횟수.
    #[serde(default)]
    pub empty_streak: i64,
}

impl VodSyncChannel {
    pub fn new(channel_id: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            last_checked_at: None,
            latest_video_no: None,
            latest_publish_date: None,
            last_live_ended_at: None,
            empty_streak: 0,
        }
    }
}

/// 채널별 VOD 동기화 기록. channelId 오름차순, 중복 없음 — 같은 상태가 같은 바이트로 인코딩된다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodSyncState {
    channels: Vec<VodSyncChannel>,
    /// 마지막 동기화 패스가 끝난 시각 — 피드 fresh 15분의 기준.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pass_at: Option<Date>,
    /// due 가 하나도 남지 않은 패스가 끝난 시각.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_full_sync_at: Option<Date>,
}

impl<'de> Deserialize<'de> for VodSyncState {
    /// 손상되거나 손으로 고친 파일도 정렬·중복 불변식을 지키게 읽는다.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            channels: Vec<VodSyncChannel>,
            #[serde(default)]
            last_pass_at: Option<Date>,
            #[serde(default)]
            last_full_sync_at: Option<Date>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self::new(
            raw.channels,
            raw.last_pass_at,
            raw.last_full_sync_at,
        ))
    }
}

impl VodSyncState {
    pub fn new(
        channels: Vec<VodSyncChannel>,
        last_pass_at: Option<Date>,
        last_full_sync_at: Option<Date>,
    ) -> Self {
        Self {
            channels: Self::normalized(channels),
            last_pass_at,
            last_full_sync_at,
        }
    }

    pub fn channels(&self) -> &[VodSyncChannel] {
        &self.channels
    }

    pub fn get(&self, channel_id: &str) -> Option<&VodSyncChannel> {
        let index = self.insertion_index(channel_id);
        self.channels
            .get(index)
            .filter(|c| c.channel_id == channel_id)
    }

    /// 같은 채널이 있으면 바꾸고, 없으면 정렬 위치에 넣는다.
    pub fn upsert(&mut self, channel: VodSyncChannel) {
        let index = self.insertion_index(&channel.channel_id);
        if index < self.channels.len() && self.channels[index].channel_id == channel.channel_id {
            self.channels[index] = channel;
        } else {
            self.channels.insert(index, channel);
        }
    }

    /// 기록을 고친다. 없으면 빈 기록에서 시작한다.
    pub fn update(&mut self, channel_id: &str, body: impl FnOnce(&mut VodSyncChannel)) {
        let mut channel = self
            .get(channel_id)
            .cloned()
            .unwrap_or_else(|| VodSyncChannel::new(channel_id));
        body(&mut channel);
        self.upsert(channel);
    }

    /// 목록 밖(언팔로우한) 채널의 기록을 뺀다. 뺀 수를 돌려준다.
    pub fn retain(&mut self, followed: &HashSet<String>) -> usize {
        let before = self.channels.len();
        self.channels.retain(|c| followed.contains(&c.channel_id));
        before - self.channels.len()
    }

    fn insertion_index(&self, channel_id: &str) -> usize {
        self.channels
            .partition_point(|c| c.channel_id.as_str() < channel_id)
    }

    /// 정렬하고, 같은 채널이 여러 번이면 뒤의 것을 쓴다.
    fn normalized(channels: Vec<VodSyncChannel>) -> Vec<VodSyncChannel> {
        let mut by_id: HashMap<String, VodSyncChannel> = HashMap::new();
        for c in channels {
            by_id.insert(c.channel_id.clone(), c);
        }
        let mut out: Vec<VodSyncChannel> = by_id.into_values().collect();
        out.sort_by(|a, b| a.channel_id.cmp(&b.channel_id));
        out
    }
}

/// 주기 등급 — 위에서부터 처음 맞는 것.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CadenceTier {
    /// 방송 종료 3시간 안 → 30분.
    RecentLive,
    /// 최근 7일 안에 VOD → 6시간.
    RecentVod,
    /// page 0 이 2번 이상 연달아 빔 → 24시간 × 2^(연속 − 1), 최대 7일.
    EmptyBackoff,
    /// 나머지 → 24시간.
    Standard,
}

impl CadenceTier {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::RecentLive => "recentLive",
            Self::RecentVod => "recentVOD",
            Self::EmptyBackoff => "emptyBackoff",
            Self::Standard => "standard",
        }
    }
}

/// 채널별 동기화 주기와 진입 예산.
pub struct VodSyncPolicy;

impl VodSyncPolicy {
    pub const FEED_FRESH: TimeInterval = 15.0 * 60.0;
    pub const RECENT_LIVE_WINDOW: TimeInterval = 3.0 * 3600.0;
    pub const RECENT_LIVE_CADENCE: TimeInterval = 30.0 * 60.0;
    pub const RECENT_VOD_WINDOW: TimeInterval = 7.0 * 86_400.0;
    pub const RECENT_VOD_CADENCE: TimeInterval = 6.0 * 3600.0;
    pub const DEFAULT_CADENCE: TimeInterval = 24.0 * 3600.0;
    pub const MAX_CADENCE: TimeInterval = 7.0 * 86_400.0;
    pub const EMPTY_BACKOFF_STREAK: i64 = 2;
    pub const VOD_PUBLISH_GRACE: TimeInterval = 10.0 * 60.0;
    pub const FAILURE_RETRY_AFTER: TimeInterval = 5.0 * 60.0;
    pub const ENTRY_BUDGET_FLOOR: usize = 24;

    /// 시계가 뒤로 가 기록이 미래면 '방금'으로 본다.
    pub fn cadence_tier(channel: &VodSyncChannel, now: Date) -> CadenceTier {
        if let Some(ended) = channel.last_live_ended_at {
            if interval_since(&now, &ended) < Self::RECENT_LIVE_WINDOW {
                return CadenceTier::RecentLive;
            }
        }
        if let Some(published) = channel.latest_publish_date {
            if interval_since(&now, &published) < Self::RECENT_VOD_WINDOW {
                return CadenceTier::RecentVod;
            }
        }
        if channel.empty_streak >= Self::EMPTY_BACKOFF_STREAK {
            CadenceTier::EmptyBackoff
        } else {
            CadenceTier::Standard
        }
    }

    pub fn cadence(channel: &VodSyncChannel, now: Date) -> TimeInterval {
        match Self::cadence_tier(channel, now) {
            CadenceTier::RecentLive => Self::RECENT_LIVE_CADENCE,
            CadenceTier::RecentVod => Self::RECENT_VOD_CADENCE,
            CadenceTier::Standard => Self::DEFAULT_CADENCE,
            CadenceTier::EmptyBackoff => {
                let doublings = (channel.empty_streak - 1).min(16) as u32;
                (Self::DEFAULT_CADENCE * f64::from(1u32 << doublings)).min(Self::MAX_CADENCE)
            }
        }
    }

    /// 이번 패스에 받아야 하는가.
    pub fn is_due(channel: Option<&VodSyncChannel>, now: Date, failed_at: Option<Date>) -> bool {
        if Self::is_recently_failed(failed_at, now) {
            return false;
        }
        let Some(channel) = channel else {
            return true;
        };
        let Some(checked) = channel.last_checked_at else {
            return true;
        };
        if let Some(ended) = channel.last_live_ended_at {
            if checked < ended {
                return now >= adding(&ended, Self::VOD_PUBLISH_GRACE);
            }
        }
        interval_since(&now, &checked) >= Self::cadence(channel, now)
    }

    /// 얼마나 밀렸나(경과 ÷ 주기). 미확인은 무한대.
    pub fn overdue_ratio(channel: Option<&VodSyncChannel>, now: Date) -> f64 {
        let Some(channel) = channel else {
            return f64::INFINITY;
        };
        let Some(checked) = channel.last_checked_at else {
            return f64::INFINITY;
        };
        let unseen_end = channel.last_live_ended_at.is_some_and(|e| checked < e);
        let base = if unseen_end {
            Self::RECENT_LIVE_CADENCE
        } else {
            Self::cadence(channel, now)
        };
        interval_since(&now, &checked).max(0.0) / base
    }

    /// 실패한 지 5분이 지나지 않았는가. 미래 시각은 막지 않는다.
    pub fn is_recently_failed(failed_at: Option<Date>, now: Date) -> bool {
        let Some(failed_at) = failed_at else {
            return false;
        };
        let elapsed = interval_since(&now, &failed_at);
        (0.0..Self::FAILURE_RETRY_AFTER).contains(&elapsed)
    }

    /// 마지막 패스가 15분 안인가. 미래 시각은 믿지 않는다.
    pub fn is_feed_fresh(last_pass_at: Option<Date>, now: Date) -> bool {
        let Some(last) = last_pass_at else {
            return false;
        };
        let elapsed = interval_since(&now, &last);
        (0.0..Self::FEED_FRESH).contains(&elapsed)
    }

    /// 진입 예산 — max(24, ⌈팔로우 × 25%⌉).
    pub fn entry_budget(follow_count: i64) -> usize {
        Self::ENTRY_BUDGET_FLOOR.max((follow_count.max(0) as usize).div_ceil(4))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowSignal {
    pub channel_id: String,
    pub is_live: bool,
}

/// 패스 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VodSyncMode {
    Entry,
    Stale,
    Refresh,
    Continuation,
}

impl VodSyncMode {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Stale => "stale",
            Self::Refresh => "refresh",
            Self::Continuation => "continuation",
        }
    }
}

/// 우선순위 등급 — 작을수록 먼저.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VodSyncTier {
    RecentLiveEnd = 0,
    NeverChecked = 1,
    Due = 2,
    Rotation = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VodSyncPlan {
    pub channel_ids: Vec<String>,
    pub due_count: usize,
    pub never_checked: usize,
    pub tier_counts: HashMap<VodSyncTier, usize>,
}

struct Candidate {
    channel_id: String,
    tier: VodSyncTier,
    is_live: bool,
    ratio: f64,
    latest_publish_date: Option<Date>,
    order: usize,
}

impl Candidate {
    fn cmp(l: &Self, r: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if l.tier != r.tier {
            return l.tier.cmp(&r.tier);
        }
        if l.tier == VodSyncTier::NeverChecked {
            if l.is_live != r.is_live {
                return if l.is_live {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            return l.order.cmp(&r.order);
        }
        if l.ratio != r.ratio {
            return r.ratio.total_cmp(&l.ratio);
        }
        match (l.latest_publish_date, r.latest_publish_date) {
            (Some(a), Some(b)) if a != b => b.cmp(&a),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            _ => l.order.cmp(&r.order),
        }
    }
}

/// 한 번의 동기화 패스에 받을 채널을 고른다.
pub struct VodSyncPlanner;

impl VodSyncPlanner {
    pub fn plan(
        follows: &[FollowSignal],
        state: &VodSyncState,
        failed_at: &HashMap<String, Date>,
        now: Date,
        limit: usize,
        mode: VodSyncMode,
    ) -> VodSyncPlan {
        let mut candidates = Vec::new();
        let mut due_count = 0;
        let mut never_checked = 0;
        let mut seen = HashSet::new();
        for (order, follow) in follows.iter().enumerate() {
            if !seen.insert(follow.channel_id.clone()) {
                continue;
            }
            let record = state.get(&follow.channel_id);
            if record.and_then(|r| r.last_checked_at).is_none() {
                never_checked += 1;
            }
            let failed = failed_at.get(&follow.channel_id).copied();
            let tier = if VodSyncPolicy::is_due(record, now, failed) {
                due_count += 1;
                Self::due_tier(record, now)
            } else if mode == VodSyncMode::Refresh
                && !VodSyncPolicy::is_recently_failed(failed, now)
            {
                VodSyncTier::Rotation
            } else {
                continue;
            };
            candidates.push(Candidate {
                channel_id: follow.channel_id.clone(),
                tier,
                is_live: follow.is_live,
                ratio: VodSyncPolicy::overdue_ratio(record, now),
                latest_publish_date: record.and_then(|r| r.latest_publish_date),
                order,
            });
        }
        candidates.sort_by(Candidate::cmp);
        candidates.truncate(limit);
        let mut tier_counts = HashMap::new();
        for c in &candidates {
            *tier_counts.entry(c.tier).or_insert(0) += 1;
        }
        VodSyncPlan {
            channel_ids: candidates.into_iter().map(|c| c.channel_id).collect(),
            due_count,
            never_checked,
            tier_counts,
        }
    }

    /// due 채널의 등급 — 방송 종료 3시간 안이면 확인한 적이 없어도 T0.
    pub fn due_tier(record: Option<&VodSyncChannel>, now: Date) -> VodSyncTier {
        if let Some(r) = record {
            if VodSyncPolicy::cadence_tier(r, now) == CadenceTier::RecentLive {
                return VodSyncTier::RecentLiveEnd;
            }
        }
        if record.and_then(|r| r.last_checked_at).is_none() {
            VodSyncTier::NeverChecked
        } else {
            VodSyncTier::Due
        }
    }
}

/// 계정 범위 구독 토큰(`CachedVOD.sources` 의 `subscriptions.‹scope›`).
pub struct VodSubscriptionTag;

impl VodSubscriptionTag {
    /// scope UUID 를 담는 키 — 인증 전환마다 새로 쓴다.
    pub const SCOPE_DEFAULTS_KEY: &'static str = "CView.followingCacheScope.v2";

    /// 지금 계정의 토큰. 팔로잉 캐시가 허용되지 않으면 `None`.
    pub fn current(scope: Option<Uuid>, allowed: bool) -> Option<String> {
        if !allowed {
            return None;
        }
        CachedVod::subscription_tag(&scope?.to_string().to_uppercase())
    }

    /// 영속 scope 로 만든 토큰 — 부팅 유지보수용.
    pub fn persisted(defaults: &dyn KeyValueStore) -> Option<String> {
        let raw = defaults.string(Self::SCOPE_DEFAULTS_KEY)?;
        let scope = Uuid::parse_str(&raw).ok()?;
        CachedVod::subscription_tag(&scope.to_string().to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/VODSyncPolicyTests.swift`, `VODSyncPlannerTests.swift` 핵심.
    use super::*;
    use cview_core::utils::time::from_unix;

    const MIN: f64 = 60.0;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86_400.0;

    fn now() -> Date {
        from_unix(1_800_000_000.0)
    }
    fn ago(s: f64) -> Date {
        adding(&now(), -s)
    }
    fn ch(
        checked: Option<f64>,
        published: Option<f64>,
        ended: Option<f64>,
        streak: i64,
    ) -> VodSyncChannel {
        VodSyncChannel {
            channel_id: "c".into(),
            last_checked_at: checked.map(ago),
            latest_video_no: None,
            latest_publish_date: published.map(ago),
            last_live_ended_at: ended.map(ago),
            empty_streak: streak,
        }
    }

    #[test]
    fn cadence_tiers_and_boundaries() {
        let inside = ch(None, None, Some(3.0 * HOUR - MIN), 0);
        assert_eq!(
            VodSyncPolicy::cadence_tier(&inside, now()),
            CadenceTier::RecentLive
        );
        assert_eq!(VodSyncPolicy::cadence(&inside, now()), 30.0 * MIN);
        let outside = ch(None, Some(DAY), Some(3.0 * HOUR + MIN), 0);
        assert_eq!(VodSyncPolicy::cadence(&outside, now()), 6.0 * HOUR);
        let old = ch(None, Some(7.0 * DAY + HOUR), None, 0);
        assert_eq!(
            VodSyncPolicy::cadence_tier(&old, now()),
            CadenceTier::Standard
        );
        for (streak, hours) in [(1, 24.0), (2, 48.0), (3, 96.0), (4, 168.0), (40, 168.0)] {
            let r = ch(None, None, None, streak);
            assert_eq!(VodSyncPolicy::cadence(&r, now()), hours * HOUR);
        }
    }

    #[test]
    fn due_rules() {
        assert!(VodSyncPolicy::is_due(None, now(), None));
        assert!(!VodSyncPolicy::is_due(
            None,
            now(),
            Some(ago(4.0 * MIN + 59.0))
        ));
        assert!(!VodSyncPolicy::is_due(None, now(), Some(ago(0.0))));
        assert!(VodSyncPolicy::is_due(None, now(), Some(ago(5.0 * MIN))));
        assert!(VodSyncPolicy::is_due(
            None,
            now(),
            Some(adding(&now(), 60.0))
        ));
        // 종료 전에 확인한 채널은 10분 유예 뒤 due.
        let before_end = ch(Some(HOUR), None, Some(10.0 * MIN), 0);
        assert!(VodSyncPolicy::is_due(Some(&before_end), now(), None));
        let in_grace = ch(Some(HOUR), None, Some(9.0 * MIN), 0);
        assert!(!VodSyncPolicy::is_due(Some(&in_grace), now(), None));
        assert!(!VodSyncPolicy::is_due(
            Some(&ch(Some(6.0 * HOUR - 1.0), Some(DAY), None, 0)),
            now(),
            None
        ));
        assert!(VodSyncPolicy::is_due(
            Some(&ch(Some(6.0 * HOUR), Some(DAY), None, 0)),
            now(),
            None
        ));
        assert_eq!(
            VodSyncPolicy::overdue_ratio(Some(&ch(Some(12.0 * HOUR), Some(DAY), None, 0)), now()),
            2.0
        );
        let unseen_end = ch(Some(12.0 * HOUR), None, Some(HOUR), 0);
        assert_eq!(VodSyncPolicy::overdue_ratio(Some(&unseen_end), now()), 24.0);
        assert!(!VodSyncPolicy::is_feed_fresh(Some(ago(15.0 * MIN)), now()));
        assert!(VodSyncPolicy::is_feed_fresh(
            Some(ago(15.0 * MIN - 1.0)),
            now()
        ));
        for (follows, budget) in [(0, 24), (96, 24), (97, 25), (320, 80)] {
            assert_eq!(VodSyncPolicy::entry_budget(follows), budget);
        }
    }

    #[test]
    fn state_normalizes_and_encodes_stably() {
        let mut a = VodSyncChannel::new("a");
        a.empty_streak = 1;
        let mut a2 = VodSyncChannel::new("a");
        a2.empty_streak = 2;
        let state = VodSyncState::new(
            vec![VodSyncChannel::new("c"), a, VodSyncChannel::new("b"), a2],
            None,
            None,
        );
        let ids: Vec<&str> = state
            .channels()
            .iter()
            .map(|c| c.channel_id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert_eq!(state.get("a").unwrap().empty_streak, 2);
        let mut s = state.clone();
        s.update("x", |c| c.empty_streak = 1);
        assert_eq!(s.get("x").unwrap().empty_streak, 1);
        assert_eq!(s.retain(&["a".to_string(), "c".to_string()].into()), 2);
        let json = serde_json::to_string(&state).unwrap();
        let back: VodSyncState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
        let shuffled: VodSyncState =
            serde_json::from_str(r#"{"channels":[{"channelId":"z"},{"channelId":"m"}]}"#).unwrap();
        assert_eq!(shuffled.channels()[0].channel_id, "m");
    }

    #[test]
    fn planner_orders_tiers() {
        let mut state = VodSyncState::default();
        state.upsert(VodSyncChannel {
            last_checked_at: Some(ago(2.0 * DAY)),
            ..VodSyncChannel::new("old")
        });
        state.upsert(VodSyncChannel {
            last_checked_at: Some(ago(2.0 * HOUR)),
            last_live_ended_at: Some(ago(HOUR)),
            ..VodSyncChannel::new("ended")
        });
        state.upsert(VodSyncChannel {
            last_checked_at: Some(ago(HOUR)),
            ..VodSyncChannel::new("fresh")
        });
        let follows: Vec<FollowSignal> = ["old", "new1", "ended", "new2", "fresh", "failed"]
            .iter()
            .map(|id| FollowSignal {
                channel_id: id.to_string(),
                is_live: *id == "new2",
            })
            .collect();
        let failed = HashMap::from([("failed".to_string(), ago(MIN))]);
        let plan = VodSyncPlanner::plan(&follows, &state, &failed, now(), 10, VodSyncMode::Entry);
        assert_eq!(plan.channel_ids, vec!["ended", "new2", "new1", "old"]);
        assert_eq!(plan.due_count, 4);
        assert_eq!(plan.never_checked, 3);
        let refresh =
            VodSyncPlanner::plan(&follows, &state, &failed, now(), 10, VodSyncMode::Refresh);
        assert_eq!(
            refresh.channel_ids.last().map(String::as_str),
            Some("fresh")
        );
        assert_eq!(refresh.tier_counts[&VodSyncTier::Rotation], 1);
        let capped = VodSyncPlanner::plan(&follows, &state, &failed, now(), 2, VodSyncMode::Entry);
        assert_eq!(capped.channel_ids.len(), 2);
    }

    #[test]
    fn subscription_tag_requires_allowed_scope() {
        let id = Uuid::nil();
        assert!(VodSubscriptionTag::current(Some(id), false).is_none());
        assert!(VodSubscriptionTag::current(None, true).is_none());
        assert!(VodSubscriptionTag::current(Some(id), true)
            .unwrap()
            .ends_with("00000000-0000-0000-0000-000000000000"));
    }
}
