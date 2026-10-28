//! 진입 전 레이아웃 기록·되돌리기 — 멀티라이브로 들어가기 전 배치를 기억했다가 돌아올 때 되살린다.

use super::{
    FollowingHubMode, FollowingState, FollowingWorkspaceHost, InMenuAddEntry,
    MultiNavigationOrigin, MultiNavigationOriginRecord, NavigationOriginStart,
    WorkspaceNavigationKind, WorkspaceRequestToken,
};
use crate::viewmodels::multi_live::MultiLiveAddSessionResult;

impl FollowingState {
    pub fn begin_multi_navigation_request(
        &mut self,
        host: &dyn FollowingWorkspaceHost,
        apply_preset: impl FnOnce(&mut Self),
    ) -> MultiNavigationOriginRecord {
        let start = self.navigation_origin_start(host, true);
        apply_preset(self);
        let request = self.begin_workspace_request();
        self.record_navigation_origin(start, request, host)
    }

    pub fn navigation_origin_start(
        &self,
        host: &dyn FollowingWorkspaceHost,
        joins_in_menu_chain: bool,
    ) -> NavigationOriginStart {
        let inherited = self.inherited_multi_navigation_origin(host).filter(|r| {
            joins_in_menu_chain
                || !self.has_in_menu_add(r.chain_root, WorkspaceNavigationKind::Multi)
        });
        NavigationOriginStart {
            origin: inherited
                .as_ref()
                .map(|r| r.origin)
                .unwrap_or_else(|| self.capture_multi_navigation_origin(host)),
            chain_root: inherited.map(|r| r.chain_root),
        }
    }

    pub(super) fn record_navigation_origin(
        &mut self,
        start: NavigationOriginStart,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOriginRecord {
        let record = MultiNavigationOriginRecord {
            request,
            origin: start.origin,
            applied: self.capture_multi_navigation_origin(host),
            applied_session_ids: host.session_ids(),
            chain_root: start.chain_root.unwrap_or(request.generation),
        };
        self.multi_navigation_origin = Some(record.clone());
        self.pending_multi_rollback = None;
        record
    }

    pub fn begin_in_menu_watch_add(
        &mut self,
        start: NavigationOriginStart,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOriginRecord {
        let record = self.record_navigation_origin(start, request, host);
        self.in_menu_adds.push(InMenuAddEntry {
            request,
            chain_root: record.chain_root,
            kind: WorkspaceNavigationKind::Watch,
        });
        record
    }

    pub fn roll_back_watch_navigation(
        &mut self,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) {
        self.settle_multi_navigation_origin(&started.request);
        if Self::has_session_added(started, host) {
            return;
        }
        if !self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == started.chain_root)
        {
            self.restore_multi_navigation_origin(started.origin, host, false);
            return;
        }
        self.pending_multi_rollback = Some(MultiNavigationOriginRecord {
            request: WorkspaceRequestToken {
                generation: self.workspace_request_generation,
                menu_epoch: (self.menu_epoch)(),
            },
            origin: started.origin,
            applied: self.capture_multi_navigation_origin(host),
            applied_session_ids: host.session_ids(),
            chain_root: started.chain_root,
        });
    }

    fn has_in_menu_add(&self, chain_root: u64, kind: WorkspaceNavigationKind) -> bool {
        self.in_menu_adds
            .iter()
            .any(|e| e.chain_root == chain_root && e.kind == kind)
    }

    fn inherited_multi_navigation_origin(
        &self,
        host: &dyn FollowingWorkspaceHost,
    ) -> Option<MultiNavigationOriginRecord> {
        let session_ids = host.session_ids();
        let layout = self.capture_multi_navigation_origin(host);
        [&self.multi_navigation_origin, &self.pending_multi_rollback]
            .into_iter()
            .flatten()
            .find(|r| {
                r.request.generation == self.workspace_request_generation
                    && r.applied_session_ids == session_ids
                    && layout.matches_user_layout(&r.applied)
            })
            .cloned()
    }

    pub fn settle_multi_navigation_origin(&mut self, request: &WorkspaceRequestToken) {
        if self.multi_navigation_origin.as_ref().map(|r| &r.request) == Some(request) {
            self.multi_navigation_origin = None;
        }
    }

    pub fn abandon_multi_navigation_origin(&mut self, request: &WorkspaceRequestToken) {
        if self.multi_navigation_origin.as_ref().map(|r| &r.request) != Some(request)
            || request.generation == self.workspace_request_generation
        {
            return;
        }
        self.multi_navigation_origin = None;
    }

    pub fn begin_in_menu_multi_add(
        &mut self,
        host: &dyn FollowingWorkspaceHost,
        apply_preset: impl FnOnce(&mut Self),
    ) -> MultiNavigationOriginRecord {
        let record = self.begin_multi_navigation_request(host, apply_preset);
        self.in_menu_adds.push(InMenuAddEntry {
            request: record.request,
            chain_root: record.chain_root,
            kind: WorkspaceNavigationKind::Multi,
        });
        record
    }

    pub fn finish_in_menu_multi_add(
        &mut self,
        result: &MultiLiveAddSessionResult,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        if let Some(i) = self
            .in_menu_adds
            .iter()
            .position(|e| e.request == started.request)
        {
            self.in_menu_adds.remove(i);
        }
        self.resolve_pending_multi_rollback(host);
        if !self.is_current_workspace_request(&started.request) {
            self.abandon_multi_navigation_origin(&started.request);
            return false;
        }
        self.settle_multi_navigation_origin(&started.request);
        if result.is_success() {
            return true;
        }
        if *result == MultiLiveAddSessionResult::Cancelled {
            return false;
        }
        if host.session_ids().is_empty() {
            self.roll_back_multi_navigation(started, host);
        }
        false
    }

    /// Smart Queue 일괄 추가 마무리. 화면이 활성화할 채널을 돌려준다.
    pub fn finish_smart_queue_batch(
        &mut self,
        outcomes: &[(String, MultiLiveAddSessionResult)],
        started: Option<&MultiNavigationOriginRecord>,
        host: &dyn FollowingWorkspaceHost,
        live_menu_visible: bool,
    ) -> Option<String> {
        let started = started?;
        let results: Vec<MultiLiveAddSessionResult> =
            outcomes.iter().map(|(_, r)| r.clone()).collect();
        let result = MultiLiveAddSessionResult::batch_outcome(&results);
        let was_current = self.is_current_workspace_request(&started.request);
        if self.finish_in_menu_multi_add(&result, started, host) {
            return outcomes
                .iter()
                .rev()
                .find(|(_, r)| r.is_success())
                .map(|(c, _)| c.clone());
        }
        if was_current
            || started.request.generation != self.workspace_request_generation
            || result.is_success()
            || result == MultiLiveAddSessionResult::Cancelled
            || !host.session_ids().is_empty()
            || self
                .in_menu_adds
                .iter()
                .any(|e| e.chain_root == started.chain_root)
            || !self
                .capture_multi_navigation_origin(host)
                .matches_user_layout(&started.applied)
        {
            return None;
        }
        self.settle_multi_navigation_origin(&started.request);
        self.restore_multi_navigation_origin(started.origin, host, !live_menu_visible);
        None
    }

    pub(super) fn roll_back_multi_navigation(
        &mut self,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if Self::has_session_added(started, host) {
            return;
        }
        if self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == started.chain_root)
        {
            self.pending_multi_rollback = Some(started.clone());
        } else {
            self.restore_multi_navigation_origin(started.origin, host, false);
        }
    }

    fn has_session_added(
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        host.session_ids()
            .iter()
            .any(|id| !started.applied_session_ids.contains(id))
    }

    fn resolve_pending_multi_rollback(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(pending) = self.pending_multi_rollback.clone() else {
            return;
        };
        if !self.is_current_workspace_request(&pending.request)
            || Self::has_session_added(&pending, host)
            || !self
                .capture_multi_navigation_origin(host)
                .matches_user_layout(&pending.applied)
        {
            self.pending_multi_rollback = None;
            return;
        }
        if self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == pending.chain_root)
        {
            return;
        }
        self.pending_multi_rollback = None;
        self.restore_multi_navigation_origin(pending.origin, host, false);
    }

    fn capture_multi_navigation_origin(
        &self,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOrigin {
        MultiNavigationOrigin {
            mode: self.hub_mode,
            grid_layout: host.is_grid_layout(),
            show_multi_live: self.show_multi_live,
            filter_live_only: self.filter_live_only,
            sort_order: self.sort_order,
            chat_dock_focus: self.chat_dock_focus,
            chat_dock_reveal: self.chat_dock_revealed_in_full_stage,
        }
    }

    fn restore_multi_navigation_origin(
        &mut self,
        origin: MultiNavigationOrigin,
        host: &dyn FollowingWorkspaceHost,
        off_menu: bool,
    ) {
        if off_menu {
            if origin.mode != FollowingHubMode::Watch && self.presenting_watch_channel_id.is_some()
            {
                self.clear_watch_presentation();
            }
            if origin.mode != FollowingHubMode::Multi && self.presenting_multi_channel_id.is_some()
            {
                self.time_out_multi_presentation();
            }
            if self.hub_mode != origin.mode {
                self.set_hub_mode(origin.mode);
            }
        } else {
            self.apply_hub_mode_preset(origin.mode, host, false, false);
        }
        host.set_grid_layout(origin.grid_layout);
        self.show_multi_live = origin.show_multi_live;
        self.filter_live_only = origin.filter_live_only;
        self.sort_order = origin.sort_order;
        self.set_chat_dock_focus(origin.chat_dock_focus);
        self.chat_dock_revealed_in_full_stage = origin.chat_dock_reveal;
    }
}
