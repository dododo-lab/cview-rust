//! 공개 API 로 묶은 AppState 흐름 — 인증 경계 → 로그인 → 프로필 → 워크스페이스 저장/복원.

use cview_app_core::app_state::{
    open_sidebar_destination, workspace_restore, AppState, LiveSectionEffect, WorkspaceInputs,
};
use cview_app_core::caching::workspace_snapshot::WorkspaceStateStore;
use cview_app_core::navigation::hub_mode::FollowingHubMode;
use cview_app_core::navigation::router::{AppRouter, SidebarItem};
use cview_app_core::navigation::sidebar_destination::SidebarDestination;
use cview_app_core::services::support::MemoryKeyValueStore;
use cview_core::utils::time::from_unix;

#[test]
fn login_flow_then_workspace_round_trip() {
    let kv = MemoryKeyValueStore::shared();
    let mut state = AppState::new(kv.clone());

    let boundary = state.begin_authentication_ui_transition(None);
    assert!(state.apply_login_result(boundary.epoch, true));
    let identity = state
        .apply_user_status(boundary.epoch, Some("닉"), None, Some("hash"))
        .expect("현재 에포크·로그인 상태");
    assert_eq!(identity.nickname.as_deref(), Some("닉"));

    // 늦게 도착한 옛 로그인 흐름은 새 경계 뒤 아무것도 바꾸지 않는다.
    let next = state.begin_authentication_ui_transition(None);
    assert_eq!(next.retired_scope, Some(boundary.scope));
    assert!(state
        .apply_user_status(boundary.epoch, Some("옛"), None, None)
        .is_none());

    let mut router = AppRouter::new(kv.clone());
    let store = WorkspaceStateStore::new(kv.clone());
    let snap = state.capture_workspace_snapshot(
        Some(&router),
        &WorkspaceInputs {
            hub_mode: FollowingHubMode::Watch,
            multi_live_channel_ids: vec!["a".into()],
            multi_chat_channel_ids: vec![],
            smart_queue_channel_ids: vec![],
        },
        from_unix(1_700_000_000.0),
    );
    store.save(&snap);
    let loaded = store.load().expect("저장한 스냅샷");
    assert_eq!(
        workspace_restore(&loaded, &router).hub_mode,
        Some(FollowingHubMode::Watch)
    );

    let effect = open_sidebar_destination(
        SidebarDestination::Live(FollowingHubMode::Multi),
        &mut router,
    );
    assert_eq!(effect, LiveSectionEffect::Routed);
    assert_eq!(router.selected_sidebar_item, SidebarItem::Following);
    assert_eq!(
        workspace_restore(&loaded, &router).hub_mode,
        None,
        "명시적 이동 뒤에는 복원하지 않는다"
    );
}
