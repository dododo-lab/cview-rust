//! 구독 VOD 피드 — 첫 로드·로컬 사본 읽기·새로고침·페이지 이동.

use std::collections::HashSet;
use std::time::Duration;

use super::{spawn_joinable, InitialLoad, VodBrowseState, VodBrowseViewModel};
use crate::services::vod_sync_state::VodSyncMode;
use crate::viewmodels::vod_library::VodLoadState;

impl VodBrowseViewModel {
    /// 최초 1회 로드(이미 로드됐으면 무시, 도는 중이면 그 로드에 합류).
    pub async fn load_initial_if_needed(&self) {
        let running = {
            let mut s = self.state.lock();
            if let Some(r) = s.initial_load_task.clone() {
                Some(r)
            } else if matches!(s.initial_load, InitialLoad::Syncing | InitialLoad::Synced) {
                return;
            } else {
                let epoch = s.invalidation_epoch;
                let me = self.clone();
                let task = spawn_joinable(async move { me.perform_initial_load(epoch).await });
                s.initial_load_task = Some(task.clone());
                drop(s);
                task.await;
                let mut s = self.state.lock();
                if s.invalidation_epoch == epoch {
                    s.initial_load_task = None;
                }
                return;
            }
        };
        if let Some(r) = running {
            r.await;
        }
    }

    async fn perform_initial_load(&self, epoch: u64) {
        self.load_local_if_needed().await;
        {
            let mut s = self.state.lock();
            if s.invalidation_epoch != epoch || s.initial_load != InitialLoad::Local {
                return;
            }
            s.initial_load = InitialLoad::Syncing;
        }
        let ids_load = self.load_watch_later_ids();
        let continue_load = self.load_continue_watching();
        let sync = self.sync_and_rebuild(VodSyncMode::Entry);
        futures_util::join!(ids_load, continue_load, sync);
        if self.epoch() != epoch {
            return;
        }
        self.wait_for_follow_list(epoch).await;
        {
            let mut s = self.state.lock();
            if s.invalidation_epoch != epoch {
                return;
            }
            s.initial_load = InitialLoad::Synced;
        }
        self.schedule_continuation_if_needed();
    }

    /// 계정 사본만으로(네트워크 없이) 첫 화면을 세운다.
    pub async fn load_local_if_needed(&self) {
        let task = {
            let mut s = self.state.lock();
            if let Some(r) = s.local_load_task.clone() {
                r
            } else {
                if s.initial_load != InitialLoad::Idle {
                    return;
                }
                s.initial_load = InitialLoad::ReadingLocal;
                s.state = VodLoadState::Loading;
                let epoch = s.invalidation_epoch;
                let me = self.clone();
                let task = spawn_joinable(async move { me.read_local_feed(epoch).await });
                s.local_load_task = Some(task.clone());
                task
            }
        };
        task.await;
        let mut s = self.state.lock();
        if s.initial_load != InitialLoad::ReadingLocal {
            s.local_load_task = None;
        }
    }

    async fn read_local_feed(&self, epoch: u64) {
        let count = self.channel_ids().len() as i64;
        let now = self.now();
        self.state.lock().entry_window.begin(count, now);
        self.show_local_feed().await;
        let mut s = self.state.lock();
        if s.invalidation_epoch == epoch {
            s.initial_load = InitialLoad::Local;
        }
    }

    /// 콜드 레이스 — 팔로우 목록이 아직 비었으면 750ms 간격으로 최대 8번 기다린다.
    async fn wait_for_follow_list(&self, epoch: u64) {
        if !self.channel_ids().is_empty() {
            return;
        }
        for _ in 0..8 {
            (self.deps.sleep)(Duration::from_millis(750)).await;
            if self.epoch() != epoch {
                return;
            }
            let ids = self.channel_ids();
            if !ids.is_empty() {
                let now = self.now();
                self.state.lock().entry_window.begin(ids.len() as i64, now);
                self.prepare_subscriptions().await;
                if self.epoch() != epoch {
                    return;
                }
                if self.state.read().subscriptions_page == 0 {
                    self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
                        .await;
                }
                self.sync_and_rebuild(VodSyncMode::Entry).await;
                return;
            }
        }
    }

    /// 피드 새로고침 — 새 진입 창 안에서 refresh 모드 패스를 돈다.
    pub async fn reload(&self) {
        let epoch = {
            let mut s = self.state.lock();
            s.subscriptions_page = 0;
            s.invalidation_epoch
        };
        if self.state.read().sync_running_mode.is_none() {
            let count = self.channel_ids().len() as i64;
            let now = self.now();
            self.state.lock().entry_window.begin(count, now);
        }
        self.prepare_subscriptions().await;
        if self.epoch() != epoch {
            return;
        }
        if self.state.read().feed.is_empty() {
            self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
                .await;
            if self.epoch() != epoch {
                return;
            }
        }
        self.sync_and_rebuild(VodSyncMode::Refresh).await;
    }

    /// 더 보기 — 받아 둔 항목과 떨어진 채널의 다음 페이지로 신규 항목을 붙인다.
    pub async fn load_more(&self) {
        let generation = {
            let mut s = self.state.lock();
            if !s.can_load_more
                || s.is_loading_more
                || s.state != VodLoadState::Loaded
                || !s.cursor.has_more()
            {
                return;
            }
            s.is_loading_more = true;
            s.subscription_generation
        };
        let rounds = if self.is_network_allowed() { 3 } else { 0 };
        let next = self
            .drain_subscriptions(Self::PAGE_SIZE, rounds, generation, true)
            .await;
        let mut s = self.state.lock();
        s.is_loading_more = false;
        let Some(next) = next else {
            return;
        };
        let mut existing: HashSet<i64> = s.feed.iter().map(|v| v.video_no).collect();
        s.feed
            .extend(next.into_iter().filter(|v| existing.insert(v.video_no)));
        s.can_load_more = s.cursor.has_more();
    }

    /// 지정 페이지로 이동. 데이터가 부족하면 먼저 더 받는다.
    pub async fn go_to_page(&self, page: usize) {
        let (pending, loading) = {
            let s = self.state.read();
            (s.has_pending_rebuild, s.is_loading_more)
        };
        if page == 0 && pending && !loading {
            let allow = self.is_network_allowed();
            self.rebuild_feed(Self::PAGE_SIZE, allow, true, VodLoadState::Empty)
                .await;
            return;
        }
        let target_start = page * VodBrowseState::VOD_PER_PAGE;
        let need_more = {
            let s = self.state.read();
            target_start >= s.feed.len() && s.can_load_more && !s.is_loading_more
        };
        if need_more {
            self.load_more().await;
        }
        let mut s = self.state.lock();
        if target_start < s.feed.len() {
            s.subscriptions_page = page;
        }
    }

    pub async fn next_subscriptions_page(&self) {
        let page = self.state.read().subscriptions_page + 1;
        self.go_to_page(page).await;
    }

    pub fn previous_subscriptions_page(&self) {
        let rebuild = {
            let mut s = self.state.lock();
            if !s.has_previous_page() {
                return;
            }
            s.subscriptions_page -= 1;
            s.subscriptions_page == 0 && s.has_pending_rebuild
        };
        if rebuild && tokio::runtime::Handle::try_current().is_ok() {
            let me = self.clone();
            tokio::spawn(async move { me.go_to_page(0).await });
        }
    }

    /// 받은 쓰기가 모두 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_pending_writes(&self) {
        loop {
            let chain = self.state.read().persist_chain.clone();
            let Some(chain) = chain else { return };
            chain.clone().await;
            let s = self.state.read();
            let same = s.persist_chain.as_ref().map(|c| c.ptr_eq(&chain));
            if same != Some(false) {
                return;
            }
        }
    }
}
