//! 보임 게이트와 이어받기(백그라운드 계속 동기화).

use futures_util::future::FutureExt;

use super::{
    due_count, spawn_joinable, InitialLoad, SyncOutcome, SyncProgress, VodBrowseViewModel, VodTab,
};
use crate::services::vod_sync_state::{
    VodSubscriptionTag, VodSyncMode, VodSyncPolicy, VodSyncState,
};
use crate::services::vod_sync_store::VodSyncStore;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};
use crate::viewmodels::vod_library::VodLoadState;

impl VodBrowseViewModel {
    /// 허브 페이지가 앞에 있는지 알린다.
    pub fn set_visible(&self, page_active: bool) {
        let was = {
            let mut s = self.state.lock();
            let was = s.is_visible();
            s.is_page_active = page_active;
            was
        };
        self.apply_visibility(was);
    }

    pub(super) fn change_tab(&self, tab: VodTab) {
        let was = {
            let mut s = self.state.lock();
            let was = s.is_visible();
            s.selected_tab = tab;
            was
        };
        self.apply_visibility(was);
    }

    fn apply_visibility(&self, was_visible: bool) {
        let visible = self.state.read().is_visible();
        if visible == was_visible {
            return;
        }
        if !visible {
            self.cancel_continuation();
            return;
        }
        let mut s = self.state.lock();
        if s.initial_load != InitialLoad::Synced || s.reactivation_task.is_some() {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let epoch = s.invalidation_epoch;
        let me = self.clone();
        s.reactivation_task = Some(spawn_joinable(async move {
            me.reactivate(epoch).await;
            let mut s = me.state.lock();
            if s.invalidation_epoch == epoch {
                s.reactivation_task = None;
            }
        }));
    }

    async fn reactivate(&self, epoch: u64) {
        if !self.state.read().is_visible() || !self.is_network_allowed() {
            return;
        }
        let ids = self.channel_ids();
        if ids.is_empty() {
            return;
        }
        self.prepare_subscriptions().await;
        if self.epoch() != epoch || !self.state.read().is_visible() {
            return;
        }
        let Some(store) = self.active_sync_store() else {
            return;
        };
        if VodSyncPolicy::is_feed_fresh(store.state().last_pass_at, self.now()) {
            self.schedule_continuation_if_needed();
            return;
        }
        let now = self.now();
        self.state.lock().entry_window.begin(ids.len() as i64, now);
        self.sync_and_rebuild(VodSyncMode::Stale).await;
    }

    fn active_sync_store(&self) -> Option<VodSyncStore> {
        match self
            .current_tag()
            .as_deref()
            .and_then(VodSubscriptionTag::scope_of_tag)
        {
            None => Some(self.untagged_sync_store.clone()),
            Some(scope) => (self.sync_store.scope().as_deref() == Some(scope.as_str()))
                .then(|| self.sync_store.clone()),
        }
    }

    fn has_backlog(state: &VodSyncState) -> bool {
        let Some(last_pass) = state.last_pass_at else {
            return false;
        };
        match state.last_full_sync_at {
            None => true,
            Some(full) => full < last_pass,
        }
    }

    pub(super) fn schedule_continuation_if_needed(&self) {
        {
            let s = self.state.read();
            if s.continuation_task.is_some()
                || !s.is_visible()
                || s.initial_load != InitialLoad::Synced
            {
                return;
            }
        }
        if !self.is_network_allowed() || tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let Some(store) = self.active_sync_store() else {
            return;
        };
        if !Self::has_backlog(&store.state()) {
            return;
        }
        let ids = self.channel_ids();
        let due = due_count(&ids, &store, self.now());
        if due == 0 {
            return;
        }
        let mut s = self.state.lock();
        s.sync_progress = Some(SyncProgress {
            checked: ids.len() - due,
            total: ids.len(),
        });
        let epoch = s.invalidation_epoch;
        let me = self.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let slot = TaskSlot::spawn(move |id| async move {
            me.run_continuation(epoch, id).await;
            let _ = tx.send(());
        });
        s.continuation_task = Some(slot);
        s.continuation_running = Some(
            async move {
                let _ = rx.await;
            }
            .boxed()
            .shared(),
        );
    }

    async fn run_continuation(&self, epoch: u64, id: u64) {
        let mut failed_batches = 0;
        (self.deps.sleep)(Self::CONTINUATION_DELAY).await;
        loop {
            {
                let s = self.state.read();
                if !slot_is(&s.continuation_task, id)
                    || !s.is_visible()
                    || s.invalidation_epoch != epoch
                {
                    break;
                }
            }
            if !self.is_network_allowed() {
                break;
            }
            let outcome = self.run_sync_pass_once(VodSyncMode::Continuation).await;
            if self.epoch() != epoch {
                break;
            }
            self.apply_background_batch(&outcome).await;
            if self.epoch() != epoch
                || outcome.planned == 0
                || outcome.due_left == 0
                || !self.is_network_allowed()
            {
                break;
            }
            self.state.lock().sync_progress = Some(SyncProgress {
                checked: outcome.follows.saturating_sub(outcome.due_left),
                total: outcome.follows,
            });
            if outcome.is_failed_batch() {
                failed_batches += 1;
            }
            if failed_batches >= Self::CONTINUATION_MAX_FAILED_BATCHES {
                break;
            }
            (self.deps.sleep)(Self::CONTINUATION_INTERVAL).await;
        }
        let mut s = self.state.lock();
        if s.invalidation_epoch == epoch && slot_is(&s.continuation_task, id) {
            s.continuation_task = None;
            s.sync_progress = None;
        }
    }

    async fn apply_background_batch(&self, outcome: &SyncOutcome) {
        let (pending, defer, empty_state) = {
            let s = self.state.read();
            (
                s.has_pending_rebuild,
                s.subscriptions_page > 0 || s.is_loading_more,
                if s.feed.is_empty() {
                    s.state.clone()
                } else {
                    VodLoadState::Empty
                },
            )
        };
        if outcome.successes == 0 && !pending {
            return;
        }
        if defer {
            self.state.lock().has_pending_rebuild = true;
            return;
        }
        self.rebuild_feed(Self::PAGE_SIZE, false, false, empty_state)
            .await;
    }

    fn cancel_continuation(&self) {
        let mut s = self.state.lock();
        cancel_slot(&mut s.continuation_task);
        s.continuation_running = None;
        s.sync_progress = None;
    }

    /// 이어받기가 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_continuation(&self) {
        let running = self.state.read().continuation_running.clone();
        if let Some(r) = running {
            r.await;
        }
    }

    /// 다시 보임에 따른 패스가 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_reactivation(&self) {
        let running = self.state.read().reactivation_task.clone();
        if let Some(r) = running {
            r.await;
        }
    }
}
