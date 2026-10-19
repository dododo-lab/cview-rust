//! 부팅 직후 스냅샷 선로드 — 인기는 곧바로, 팔로잉·히어로·추천은 계정 확인 뒤 채택.
//!
//! 원본: `Sources/CViewApp/ViewModels/HomeViewModel+BootSnapshots.swift`. signpost·PerfCaptureLog
//! 기록(`recordBootHydrate`)은 계측이라 옮기지 않고 보고서 값만 남긴다.

use std::time::Instant;

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use cview_core::models::LiveChannelItem;
use cview_core::utils::time::Date;

use super::deps::{HomeExtrasSnapshot, SnapshotLoadResult};
use super::promotion::DataOrigin;
use super::recommendation::ScoredChannel;
use super::{HomeState, HomeViewModel};

#[derive(Debug, Clone, PartialEq)]
pub struct BootFollowingSnapshot {
    pub scope: Uuid,
    pub channels: Vec<LiveChannelItem>,
    pub fetched_at: Date,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootExtrasSnapshot {
    pub scope: Uuid,
    pub snapshot: HomeExtrasSnapshot,
    pub fetched_at: Date,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootHero {
    pub channel: LiveChannelItem,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BootHydrateReport {
    pub total_bytes: u64,
    pub total_ms: f64,
    pub popular_rows: usize,
    pub following_rows: usize,
    pub summary_loaded: bool,
    pub extras_rows: usize,
}

impl HomeState {
    fn adopt_boot_following(&mut self, provisional: bool) -> bool {
        let Some(boot) = self.boot_following.clone() else {
            return false;
        };
        if boot.scope != self.following_cache_scope
            || boot.generation != self.following_network_generation
            || !self.following_channels().is_empty()
        {
            return false;
        }
        self.set_following_channels(boot.channels.clone());
        self.set_following_origin(DataOrigin::Snapshot);
        self.set_following_cached_at(Some(boot.fetched_at));
        self.following_is_provisional = provisional;
        let stats = HomeViewModel::compute_following_stats(&boot.channels);
        self.apply_following_stats(stats);
        true
    }

    fn hold_boot_extras(
        &mut self,
        result: SnapshotLoadResult<HomeExtrasSnapshot>,
        scope: Uuid,
        generation: u64,
    ) -> usize {
        let SnapshotLoadResult::Fresh(snapshot, fetched_at) = result else {
            return 0;
        };
        let n = snapshot.recommended.len();
        self.boot_extras = Some(BootExtrasSnapshot {
            scope,
            snapshot,
            fetched_at,
            generation,
        });
        n
    }

    /// 히어로·추천을 채택하고, 되살릴 활동을 돌려준다.
    fn take_boot_extras_visuals(&mut self) -> Option<serde_json::Value> {
        let boot = self.boot_extras.clone()?;
        if boot.scope != self.following_cache_scope
            || boot.generation != self.following_network_generation
        {
            return None;
        }
        let extras = boot.snapshot;
        if !self.boot_extras_visuals_discarded && self.boot_hero.is_none() {
            if let Some(hero) = extras.hero.clone() {
                self.boot_hero = Some(BootHero {
                    channel: hero,
                    reasons: extras.hero_reasons.clone(),
                });
            }
        }
        if !self.boot_extras_visuals_discarded
            && self.boot_recommendations.is_empty()
            && !extras.recommended.is_empty()
        {
            self.boot_recommendations = extras
                .recommended
                .iter()
                .map(|r| ScoredChannel {
                    channel: r.channel.clone(),
                    score: r.score,
                    reasons: r.reasons.clone(),
                })
                .collect();
        }
        Some(extras.activity)
    }

    pub fn discard_boot_extras(&mut self) {
        self.boot_extras_visuals_discarded = true;
        self.boot_hero = None;
        if !self.boot_recommendations.is_empty() {
            self.boot_recommendations = Vec::new();
        }
    }

    pub fn reset_boot_extras(&mut self) {
        self.boot_extras = None;
        self.boot_extras_visuals_discarded = false;
        self.boot_hero = None;
        if !self.boot_recommendations.is_empty() {
            self.boot_recommendations = Vec::new();
        }
    }
}

impl HomeViewModel {
    /// 부팅 스냅샷을 한 번만 읽는다. 동시에 부르면 첫 호출의 완료를 기다린다.
    pub async fn preload_boot_snapshots(&self) {
        let existing = self.read().boot_preload.clone();
        if let Some(mut rx) = existing {
            let _ = rx.wait_for(|done| *done).await;
            return;
        }
        let (tx, rx) = tokio::sync::watch::channel(false);
        self.lock().boot_preload = Some(rx);
        self.run_boot_preload().await;
        let _ = tx.send(true);
    }

    async fn await_boot_preload(&self) {
        let existing = self.read().boot_preload.clone();
        if let Some(mut rx) = existing {
            let _ = rx.wait_for(|done| *done).await;
        }
    }

    async fn run_boot_preload(&self) {
        let started = Instant::now();
        let (snapshots, scope, live_generation, following_generation) = {
            let s = self.read();
            (
                s.deps.snapshots.clone(),
                s.following_cache_scope,
                s.live_channels_network_generation,
                s.following_network_generation,
            )
        };
        let bytes_before = snapshots.bytes_read().await;
        let scope_str = scope.to_string();
        let (popular, following, (), extras) = futures_util::join!(
            snapshots.load_channel_list("popular", "public", Self::PUBLIC_SNAPSHOT_RESTORE_MAX_AGE),
            snapshots.load_channel_list(
                "following",
                &scope_str,
                Self::FOLLOWING_SNAPSHOT_RESTORE_MAX_AGE
            ),
            self.hydrate_catalog_first_paint(),
            snapshots.load_home_extras(&scope_str, Self::HOME_EXTRAS_RESTORE_MAX_AGE),
        );
        let bytes_after = snapshots.bytes_read().await;
        let mut s = self.lock();
        let mut report = BootHydrateReport {
            summary_loaded: s.deps.catalog.usable_summary().is_some(),
            ..Default::default()
        };
        match popular {
            SnapshotLoadResult::Fresh(snapshot, _) => {
                if Self::should_apply_restored_source(
                    s.live_channels().len(),
                    true,
                    live_generation,
                    s.live_channels_network_generation,
                ) {
                    report.popular_rows = snapshot.channels.len();
                    s.set_live_channels(snapshot.channels);
                    s.set_popular_origin(DataOrigin::Snapshot);
                    s.set_live_channels_cached_at(Some(snapshot.fetched_at));
                }
            }
            SnapshotLoadResult::Expired(fetched_at) => {
                if Self::should_apply_restored_source(
                    s.live_channels().len(),
                    true,
                    live_generation,
                    s.live_channels_network_generation,
                ) {
                    s.set_popular_origin(DataOrigin::Snapshot);
                    s.set_live_channels_cached_at(Some(fetched_at));
                }
            }
            SnapshotLoadResult::Missing | SnapshotLoadResult::Rejected => {}
        }
        if let SnapshotLoadResult::Fresh(snapshot, _) = following {
            report.following_rows = snapshot.channels.len();
            s.boot_following = Some(BootFollowingSnapshot {
                scope,
                channels: snapshot.channels,
                fetched_at: snapshot.fetched_at,
                generation: following_generation,
            });
        }
        report.extras_rows = s.hold_boot_extras(extras, scope, following_generation);
        if report.popular_rows > 0 {
            s.refresh_popular_derived();
        }
        report.total_ms = started.elapsed().as_secs_f64() * 1_000.0;
        report.total_bytes = bytes_after.saturating_sub(bytes_before);
        s.boot_hydrate_report = Some(report);
    }

    /// 계정 확인 전 — 선로드한 팔로잉을 '확인 중' 으로 보여 준다.
    pub async fn show_provisional_boot_following(&self) {
        self.await_boot_preload().await;
        self.lock().adopt_boot_following(true);
        self.adopt_boot_extras(Utc::now()).await;
    }

    /// 계정 확인 뒤 — 같은 계정이면 확정한다.
    pub async fn apply_boot_following_if_allowed(&self) {
        self.await_boot_preload().await;
        {
            let mut s = self.lock();
            if !s.following_cache_allowed {
                return;
            }
            if s.following_is_provisional {
                s.following_is_provisional = false;
            } else {
                s.adopt_boot_following(false);
            }
            s.boot_following = None;
        }
        self.adopt_boot_extras(Utc::now()).await;
        self.lock().boot_extras = None;
    }

    pub async fn adopt_boot_extras(&self, now: Date) {
        let (activity, restorer) = {
            let mut s = self.lock();
            (s.take_boot_extras_visuals(), s.deps.activity.clone())
        };
        if let Some(activity) = activity {
            if !activity.is_null() {
                restorer.restore(&activity, now).await;
            }
        }
    }

    pub fn discard_boot_extras(&self) {
        self.lock().discard_boot_extras();
    }
}
