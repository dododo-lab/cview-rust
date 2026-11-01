//! `Sources/CViewApp/Services/MultiLiveProposer.swift` 포팅.
//!
//! 팔로우 채널의 새 라이브 시작을 폴 결과(`FollowingStore` 싱크)로 받아 멀티채팅 추가 제안을 쌓는다.
//! 기준선 폴(`is_baseline`)은 제안을 만들지 않는다(앱 시작 노이즈 방지). 제안은 최대 3개, 수명 30분.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use cview_core::utils::following_poll::FollowingPollOutcome;
use cview_core::utils::time::{Date, TimeInterval};
use futures_util::FutureExt;

use super::support::lock;
use crate::caching::following_store::{
    FollowingPollSinkSlot, FollowingStore, PollOutcome, PollResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub channel_name: String,
    /// 제안이 만들어진 시각 — 만료 판정의 기준.
    pub created_at: Date,
}

type SessionIds = Arc<dyn Fn() -> HashSet<String> + Send + Sync>;

#[derive(Default)]
struct State {
    proposals: Vec<Proposal>,
    store: Option<FollowingStore>,
    get_current_session_ids: Option<SessionIds>,
}

/// 새 라이브 제안기. `Clone` 은 같은 제안기를 가리킨다.
#[derive(Clone, Default)]
pub struct MultiLiveProposer {
    state: Arc<Mutex<State>>,
}

impl MultiLiveProposer {
    /// 제안의 수명(30분) — 폴링 주기(300초)의 여러 배.
    pub const MAXIMUM_AGE: TimeInterval = 1_800.0;
    /// 쌓아 두는 제안 수 상한.
    pub const MAX_PROPOSALS: usize = 3;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn proposals(&self) -> Vec<Proposal> {
        lock(&self.state).proposals.clone()
    }

    /// 폴 소유자의 결과를 받기 시작한다.
    pub fn start(
        &self,
        store: &FollowingStore,
        get_current_session_ids: impl Fn() -> HashSet<String> + Send + Sync + 'static,
    ) {
        self.stop();
        {
            let mut st = lock(&self.state);
            st.store = Some(store.clone());
            st.get_current_session_ids = Some(Arc::new(get_current_session_ids));
        }
        let weak = Arc::downgrade(&self.state);
        store.set_sink(
            FollowingPollSinkSlot::Proposer,
            Some(Arc::new(move |outcome: PollOutcome| {
                if let (FollowingPollOutcome::Success(result), Some(state)) =
                    (outcome, weak.upgrade())
                {
                    let me = MultiLiveProposer { state };
                    let ids = lock(&me.state)
                        .get_current_session_ids
                        .clone()
                        .map(|f| f())
                        .unwrap_or_default();
                    me.ingest(&result, &ids, Utc::now());
                }
                async {}.boxed()
            })),
        );
    }

    /// 폴 결과 하나를 반영한다 — 끝난 방송·오래된 제안을 걷어내고, 새 라이브로 제안을 쌓는다.
    pub fn ingest(&self, result: &PollResult, current_session_ids: &HashSet<String>, now: Date) {
        let live: Vec<_> = result.channels.iter().filter(|c| c.is_live).collect();
        let live_ids: HashSet<String> = live.iter().map(|c| c.channel_id.clone()).collect();
        self.prune_stale_proposals(&live_ids, now);
        if result.is_baseline {
            return;
        }
        let mut st = lock(&self.state);
        for entry in live {
            if !result.newly_live.contains(&entry.channel_id)
                || current_session_ids.contains(&entry.channel_id)
            {
                continue;
            }
            if st.proposals.iter().any(|p| p.id == entry.channel_id) {
                continue;
            }
            st.proposals.push(Proposal {
                id: entry.channel_id.clone(),
                channel_name: entry.channel_name.clone(),
                created_at: now,
            });
            if st.proposals.len() > Self::MAX_PROPOSALS {
                let excess = st.proposals.len() - Self::MAX_PROPOSALS;
                st.proposals.drain(..excess);
            }
        }
    }

    /// 이번 조회에서 **새로** 시작한 방송과 다음 기준. 빈 조회는 "모름"(기준 유지),
    /// 기준이 없을 때의 첫 유효 조회는 기준만 잡는다.
    pub fn detect_newly_live(
        baseline: Option<&HashSet<String>>,
        live_ids: &HashSet<String>,
    ) -> (HashSet<String>, Option<HashSet<String>>) {
        if live_ids.is_empty() {
            return (HashSet::new(), baseline.cloned());
        }
        match baseline {
            None => (HashSet::new(), Some(live_ids.clone())),
            Some(b) => (
                live_ids.difference(b).cloned().collect(),
                Some(live_ids.clone()),
            ),
        }
    }

    /// 끝난 방송과 오래된 제안을 걷어낸다. `live_ids` 가 비면 오프라인 판정은 하지 않는다.
    pub fn pruning(proposals: &[Proposal], live_ids: &HashSet<String>, now: Date) -> Vec<Proposal> {
        let fresh = proposals.iter().filter(|p| {
            (now - p.created_at).num_milliseconds() as f64 / 1000.0 <= Self::MAXIMUM_AGE
        });
        if live_ids.is_empty() {
            return fresh.cloned().collect();
        }
        fresh
            .filter(|p| live_ids.contains(&p.id))
            .cloned()
            .collect()
    }

    pub fn prune_stale_proposals(&self, live_ids: &HashSet<String>, now: Date) {
        let mut st = lock(&self.state);
        st.proposals = Self::pruning(&st.proposals, live_ids, now);
    }

    pub fn dismiss(&self, id: &str) {
        lock(&self.state).proposals.retain(|p| p.id != id);
    }

    pub fn stop(&self) {
        let store = {
            let mut st = lock(&self.state);
            st.get_current_session_ids = None;
            st.store.take()
        };
        if let Some(store) = store {
            store.set_sink(FollowingPollSinkSlot::Proposer, None);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MultiLiveProposerExpiryTests.swift`
    use super::*;
    use chrono::Duration;
    use cview_core::utils::following_poll::FollowingPollReason;
    use cview_core::utils::live_detail_refresh::MergedFollowingItem;
    use cview_core::utils::time::from_unix;

    fn proposal(id: &str, at: Date) -> Proposal {
        Proposal {
            id: id.into(),
            channel_name: id.into(),
            created_at: at,
        }
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn item(id: &str) -> MergedFollowingItem {
        MergedFollowingItem {
            id: id.into(),
            channel_name: format!("name-{id}"),
            channel_image_url: None,
            live_title: String::new(),
            viewer_count: 0,
            category_name: None,
            category_type: None,
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: true,
            open_date: None,
            verified_badge: None,
        }
    }

    fn result(ids: &[&str], newly: &[&str], baseline: bool) -> PollResult {
        PollResult {
            reason: FollowingPollReason::Scheduled,
            channels: ids.iter().map(|i| item(i)).collect(),
            fetched_at: from_unix(0.0),
            generation: 0,
            newly_live: set(newly),
            is_baseline: baseline,
            list_carries_live_info: true,
            detail_requests: Vec::new(),
        }
    }

    #[test]
    fn pruning_expires_and_drops_offline_but_not_on_empty_lookup() {
        let now = from_unix(10_000.0);
        let list = vec![
            proposal("a", now - Duration::seconds(1_801)),
            proposal("b", now - Duration::seconds(60)),
            proposal("c", now - Duration::seconds(60)),
        ];
        let kept = MultiLiveProposer::pruning(&list, &set(&["b"]), now);
        assert_eq!(
            kept.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        let kept = MultiLiveProposer::pruning(&list, &HashSet::new(), now);
        assert_eq!(kept.len(), 2, "빈 조회는 오프라인 판정을 하지 않는다");
    }

    #[test]
    fn detect_newly_live_rules() {
        let (newly, base) = MultiLiveProposer::detect_newly_live(None, &set(&["a"]));
        assert!(newly.is_empty());
        assert_eq!(base, Some(set(&["a"])));
        let (newly, base) =
            MultiLiveProposer::detect_newly_live(Some(&set(&["a"])), &HashSet::new());
        assert!(newly.is_empty());
        assert_eq!(base, Some(set(&["a"])));
        let (newly, _) =
            MultiLiveProposer::detect_newly_live(Some(&set(&["a"])), &set(&["a", "b"]));
        assert_eq!(newly, set(&["b"]));
    }

    #[test]
    fn ingest_skips_baseline_and_caps_at_three() {
        let p = MultiLiveProposer::new();
        let now = from_unix(1_000.0);
        p.ingest(&result(&["a", "b"], &[], true), &HashSet::new(), now);
        assert!(p.proposals().is_empty());
        p.ingest(
            &result(&["a", "b", "c", "d", "e"], &["b", "c", "d", "e"], false),
            &set(&["b"]),
            now,
        );
        let ids: Vec<_> = p.proposals().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["c", "d", "e"]);
        p.dismiss("d");
        assert_eq!(p.proposals().len(), 2);
    }
}
