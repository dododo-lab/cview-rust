//! FollowingViewState·MultiLiveAddSessionResult 테스트.
//!
//! 원본 행위: `FollowingViewState.swift` 의 허브 프리셋·시청/멀티 진입·되돌리기,
//! `Tests/CViewAppTests/PrecisionReview0926HomeMultiAddTests.swift`·`FollowingSheet*Tests.swift` 의
//! 계약 중 매니저 없이 검증할 수 있는 것(FakeHost 로).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use cview_app_core::viewmodels::chat::deps::ToastPresenter;
use cview_app_core::viewmodels::following::{
    defaults_key, ChatDockFocus, FollowingBrowserFilter, FollowingChatSessions, FollowingHubMode,
    FollowingSortOrder, FollowingViewState, FollowingWorkspaceHost, KeyValueStore,
    MemoryKeyValueStore, NoopChatSessions, WatchNavigationOutcome,
};
use cview_app_core::viewmodels::multi_live::MultiLiveAddSessionResult;
use cview_core::models::LiveChannelItem;

#[derive(Default)]
struct HostState {
    sessions: Vec<(Uuid, String)>,
    selected: Option<Uuid>,
    grid: bool,
    focus_layout: bool,
    focus_main: Option<Uuid>,
    frame_ticks: std::collections::HashMap<Uuid, u64>,
    adding: HashSet<String>,
    audible: HashSet<Uuid>,
    log: Vec<String>,
}

#[derive(Default)]
struct FakeHost {
    s: Mutex<HostState>,
}

impl FakeHost {
    fn add_session(&self, channel: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.s.lock().unwrap().sessions.push((id, channel.into()));
        id
    }
    fn log(&self) -> Vec<String> {
        self.s.lock().unwrap().log.clone()
    }
    fn push(&self, e: String) {
        self.s.lock().unwrap().log.push(e);
    }
}

impl FollowingWorkspaceHost for FakeHost {
    fn session_ids(&self) -> Vec<Uuid> {
        self.s
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|s| s.0)
            .collect()
    }
    fn session_channel_ids(&self) -> Vec<String> {
        self.s
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|s| s.1.clone())
            .collect()
    }
    fn session_id_for_channel(&self, channel_id: &str) -> Option<Uuid> {
        self.s
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.1 == channel_id)
            .map(|s| s.0)
    }
    fn selected_session_id(&self) -> Option<Uuid> {
        self.s.lock().unwrap().selected
    }
    fn presented_frame_tick(&self, session_id: Uuid) -> u64 {
        *self
            .s
            .lock()
            .unwrap()
            .frame_ticks
            .get(&session_id)
            .unwrap_or(&0)
    }
    fn is_adding_channel(&self, channel_id: &str) -> bool {
        self.s.lock().unwrap().adding.contains(channel_id)
    }
    fn is_grid_layout(&self) -> bool {
        self.s.lock().unwrap().grid
    }
    fn set_grid_layout(&self, grid: bool) {
        self.s.lock().unwrap().grid = grid;
    }
    fn is_focus_layout(&self) -> bool {
        self.s.lock().unwrap().focus_layout
    }
    fn focus_main_session_id(&self) -> Option<Uuid> {
        self.s.lock().unwrap().focus_main
    }
    fn select_session(&self, id: Uuid, promote: bool) {
        let mut s = self.s.lock().unwrap();
        s.selected = Some(id);
        if promote {
            s.focus_main = Some(id);
        }
    }
    fn set_stage_session(&self, id: Option<Uuid>) {
        self.push(format!("stage:{}", id.is_some()));
    }
    fn is_audible(&self, session_id: Uuid) -> bool {
        self.s.lock().unwrap().audible.contains(&session_id)
    }
    fn toggle_session_audio(&self, session_id: Uuid) {
        let mut s = self.s.lock().unwrap();
        if !s.audible.remove(&session_id) {
            s.audible.insert(session_id);
        }
    }
    fn note_zap_requested(&self, c: &str) {
        self.push(format!("zap:{c}"));
    }
    fn forget_zap_request(&self, c: &str) {
        self.push(format!("forget:{c}"));
    }
    fn complete_watch_zap(&self, c: &str, reason: &str) {
        self.push(format!("complete:{c}:{reason}"));
    }
    fn activate_new_watch_audio(&self, c: &str) {
        self.push(format!("audio:{c}"));
    }
    fn restart_session_if_stalled(&self, c: &str) {
        self.push(format!("restart:{c}"));
    }
    fn restore_video_surface_for_watch(&self, c: &str) {
        self.push(format!("surfaceWatch:{c}"));
    }
    fn park_video_surfaces_for_resident_menu_leave(&self) {
        self.push("park".into());
    }
    fn defer_peer_surface_restore(&self) {
        self.push("defer".into());
    }
    fn restore_video_surfaces_for_menu_enter(&self) {
        self.push("restore".into());
    }
    fn complete_deferred_peer_restore(&self) {
        self.push("completeDeferred".into());
    }
}

#[derive(Default)]
struct RecordingChats {
    capacity_full: Mutex<bool>,
    adopted: Mutex<Vec<String>>,
    selected: Mutex<Vec<String>>,
}

impl FollowingChatSessions for RecordingChats {
    fn remove_borrowed_sessions(&self, _not_in: &HashSet<String>) {}
    fn adopt_live_session(&self, _session_id: Uuid, channel_id: &str) -> bool {
        if *self.capacity_full.lock().unwrap() {
            return false;
        }
        self.adopted.lock().unwrap().push(channel_id.into());
        true
    }
    fn select_channel(&self, channel_id: &str) {
        self.selected.lock().unwrap().push(channel_id.into());
    }
}

type Undo = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct RecordingToast {
    shown: Mutex<Vec<String>>,
    undo: Mutex<Option<Undo>>,
}

impl ToastPresenter for RecordingToast {
    fn show(&self, message: String, _undo_title: Option<String>, undo: Option<Undo>) {
        self.shown.lock().unwrap().push(message);
        *self.undo.lock().unwrap() = undo;
    }
}

fn make() -> (
    FollowingViewState,
    Arc<FakeHost>,
    Arc<RecordingChats>,
    Arc<AtomicU64>,
) {
    let epoch = Arc::new(AtomicU64::new(0));
    let e2 = epoch.clone();
    let chats = Arc::new(RecordingChats::default());
    let vs = FollowingViewState::new(
        Arc::new(MemoryKeyValueStore::default()),
        Arc::new(move || e2.load(Ordering::SeqCst)),
        chats.clone(),
    );
    (vs, Arc::new(FakeHost::default()), chats, epoch)
}

fn live(id: &str) -> LiveChannelItem {
    LiveChannelItem {
        id: id.into(),
        channel_name: id.into(),
        channel_image_url: None,
        live_title: String::new(),
        viewer_count: 0,
        category_name: None,
        category_type: None,
        thumbnail_url: None,
        channel_id: id.into(),
        is_live: true,
        open_date: None,
    }
}

// MARK: - 열거형·설정

#[test]
fn hub_mode_tokens() {
    assert_eq!(
        FollowingHubMode::from_deep_link_token("WATCH"),
        Some(FollowingHubMode::Watch)
    );
    assert_eq!(
        FollowingHubMode::from_deep_link_token("카테고리"),
        Some(FollowingHubMode::Category)
    );
    assert_eq!(FollowingHubMode::from_deep_link_token("nope"), None);
    assert!(FollowingHubMode::Popular.occupies_full_stage());
    assert!(!FollowingHubMode::Multi.occupies_full_stage());
    assert_eq!(FollowingHubMode::Explore.display_name(), "개요");
    let sorted = FollowingSortOrder::LiveFirst.sort(&[
        LiveChannelItem {
            is_live: false,
            viewer_count: 99,
            ..live("off")
        },
        LiveChannelItem {
            viewer_count: 1,
            ..live("a")
        },
        LiveChannelItem {
            viewer_count: 5,
            ..live("b")
        },
    ]);
    let ids: Vec<_> = sorted.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["b", "a", "off"]);
}

#[test]
fn defaults_migration_and_restore() {
    let defaults = Arc::new(MemoryKeyValueStore::default());
    defaults.set_string(defaults_key::CHAT_DOCK_FOCUS, "multi");
    defaults.set_bool(defaults_key::CHAT_DOCK_COLLAPSED, true);
    let vs = FollowingViewState::new(defaults.clone(), Arc::new(|| 0), Arc::new(NoopChatSessions));
    assert_eq!(
        vs.read().chat_dock_focus(),
        ChatDockFocus::Balanced,
        "이관 전 값은 지운다"
    );
    assert!(
        vs.read().chat_dock_collapsed(),
        "이관 대상이 아닌 값은 남는다"
    );
    assert_eq!(
        defaults.string(defaults_key::REDESIGN_MIGRATION).as_deref(),
        Some(defaults_key::REDESIGN_MIGRATION_CURRENT)
    );
    vs.lock()
        .set_browser_filter(FollowingBrowserFilter::Favorites);
    vs.lock().set_chat_dock_focus(ChatDockFocus::Single);
    let again = FollowingViewState::new(defaults, Arc::new(|| 0), Arc::new(NoopChatSessions));
    assert_eq!(
        again.read().browser_filter(),
        FollowingBrowserFilter::Favorites
    );
    assert_eq!(again.read().chat_dock_focus(), ChatDockFocus::Single);
}

// MARK: - 허브 프리셋

#[test]
fn hub_mode_presets_drive_layout_and_surfaces() {
    let (vs, host, ..) = make();
    let h = host.as_ref();
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Multi, h, false, false);
    {
        let s = vs.read();
        assert_eq!(s.hub_mode(), FollowingHubMode::Multi);
        assert_eq!(s.chat_dock_focus(), ChatDockFocus::Multi);
        assert!(s.filter_live_only && s.show_multi_live);
    }
    assert!(host.is_grid_layout());
    assert_eq!(host.log(), ["completeDeferred"]);
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Popular, h, false, false);
    assert_eq!(host.log().last().unwrap(), "park");
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Explore, h, false, false);
    assert_eq!(host.log().last().unwrap(), "restore");
    assert!(!host.is_grid_layout());
    assert!(!vs.read().show_multi_live && !vs.read().filter_live_only);
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Popular, h, false, false);
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Watch, h, true, false);
    assert_eq!(
        host.log().last().unwrap(),
        "defer",
        "보류 진입은 복원을 미룬다"
    );
}

#[test]
fn hub_mode_change_invalidates_requests() {
    let (vs, ..) = make();
    let token = vs.lock().begin_workspace_request();
    assert!(vs.read().is_current_workspace_request(&token));
    vs.lock().set_hub_mode(FollowingHubMode::Watch);
    assert!(!vs.read().is_current_workspace_request(&token));
}

#[test]
fn menu_epoch_change_invalidates_requests() {
    let (vs, _host, _chats, epoch) = make();
    let token = vs.lock().begin_workspace_request();
    epoch.store(1, Ordering::SeqCst);
    assert!(!vs.read().is_current_workspace_request(&token));
}

// MARK: - 시청 진입

#[tokio::test]
async fn navigate_to_watch_existing_session_commits_immediately() {
    let (vs, host, chats, _) = make();
    let id = host.add_session("ch1");
    let outcome = vs
        .navigate_to_watch("ch1", Some("채널1"), host.clone(), async {}, |_| async {
            panic!("기존 세션은 추가하지 않는다")
        })
        .await;
    assert_eq!(outcome, WatchNavigationOutcome::Committed);
    assert_eq!(host.selected_session_id(), Some(id));
    assert!(host.log().contains(&"restart:ch1".to_string()));
    assert_eq!(chats.adopted.lock().unwrap().as_slice(), ["ch1"]);
    let s = vs.read();
    assert_eq!(s.hub_mode(), FollowingHubMode::Watch);
    assert!(!s.is_preparing_watch());
    assert_eq!(
        s.presenting_watch_channel_id(),
        Some("ch1"),
        "첫 프레임 전에는 표시를 유지한다"
    );
    assert!(s.show_multi_chat);
}

#[tokio::test]
async fn navigate_to_watch_adds_session_and_activates_audio() {
    let (vs, host, ..) = make();
    let h2 = host.clone();
    let outcome = vs
        .navigate_to_watch("ch2", None, host.clone(), async {}, move |_| async move {
            let id = h2.add_session("ch2");
            h2.s.lock().unwrap().frame_ticks.insert(id, 1);
            MultiLiveAddSessionResult::Added { session_id: id }
        })
        .await;
    assert_eq!(outcome, WatchNavigationOutcome::Committed);
    let log = host.log();
    assert!(log.contains(&"zap:ch2".to_string()));
    assert!(log.contains(&"audio:ch2".to_string()));
    assert!(log.contains(&"complete:ch2:ready".to_string()));
    assert!(vs.read().presenting_watch_channel_id().is_none());
}

#[tokio::test]
async fn failed_watch_navigation_rolls_back_to_origin() {
    let (vs, host, ..) = make();
    assert_eq!(vs.read().hub_mode(), FollowingHubMode::Explore);
    let outcome = vs
        .navigate_to_watch("ch3", None, host.clone(), async {}, |_| async {
            MultiLiveAddSessionResult::NotLive
        })
        .await;
    assert_eq!(
        outcome,
        WatchNavigationOutcome::Failed(MultiLiveAddSessionResult::NotLive)
    );
    let s = vs.read();
    assert_eq!(
        s.hub_mode(),
        FollowingHubMode::Explore,
        "진입 전 모드로 되돌린다"
    );
    assert!(!s.filter_live_only);
    assert!(s.presenting_watch_channel_id().is_none());
    assert!(host.log().contains(&"forget:ch3".to_string()));
}

#[tokio::test]
async fn watch_navigation_superseded_by_newer_request() {
    let (vs, host, ..) = make();
    let vs2 = vs.clone();
    let outcome = vs
        .navigate_to_watch(
            "ch4",
            None,
            host.clone(),
            async move {
                vs2.lock().begin_workspace_request();
            },
            |_| async { panic!("밀린 요청은 추가하지 않는다") },
        )
        .await;
    assert_eq!(outcome, WatchNavigationOutcome::Superseded);
    assert!(host.log().contains(&"forget:ch4".to_string()));
    assert!(vs.read().presenting_watch_channel_id().is_none());
}

#[tokio::test]
async fn duplicate_watch_navigation_is_ignored() {
    let (vs, host, ..) = make();
    vs.lock().begin_watch_preparation("ch5", None, None, false);
    let outcome = vs
        .navigate_to_watch("ch5", None, host.clone(), async {}, |_| async {
            panic!("중복 요청")
        })
        .await;
    assert_eq!(outcome, WatchNavigationOutcome::Superseded);
}

// MARK: - 멀티 진입

#[tokio::test]
async fn navigate_to_multi_success_and_capacity_error() {
    let (vs, host, chats, _) = make();
    let main = host.add_session("main");
    host.select_session(main, false);
    *chats.capacity_full.lock().unwrap() = true;
    let h2 = host.clone();
    let outcome = vs
        .navigate_to_multi(
            "m1",
            Some("멀티1"),
            host.clone(),
            async {},
            move |_| async move {
                let id = h2.add_session("m1");
                MultiLiveAddSessionResult::Added { session_id: id }
            },
        )
        .await;
    assert_eq!(outcome, WatchNavigationOutcome::Committed);
    assert_eq!(
        host.selected_session_id(),
        Some(main),
        "새 세션은 메인 포커스를 유지한다"
    );
    let s = vs.read();
    assert_eq!(s.hub_mode(), FollowingHubMode::Multi);
    assert!(!s.is_awaiting_multi_entry_motion());
    assert_eq!(
        s.ml_add_error.as_deref(),
        Some("영상은 추가됐지만 채팅 최대 개수에 도달해 채팅을 함께 열지 못했습니다.")
    );
    assert_eq!(s.presenting_multi_channel_id(), Some("m1"));
    assert!(host.log().contains(&"audio:m1".to_string()));
}

#[tokio::test]
async fn failed_multi_navigation_with_no_new_session_restores_origin() {
    let (vs, host, ..) = make();
    let outcome = vs
        .navigate_to_multi("m2", None, host.clone(), async {}, |_| async {
            MultiLiveAddSessionResult::MaxSessionsReached
        })
        .await;
    assert_eq!(
        outcome,
        WatchNavigationOutcome::Failed(MultiLiveAddSessionResult::MaxSessionsReached)
    );
    let s = vs.read();
    assert_eq!(s.hub_mode(), FollowingHubMode::Explore);
    assert!(s.presenting_multi_channel_id().is_none());
    assert!(!host.is_grid_layout());
}

#[test]
fn in_menu_multi_add_chain_defers_rollback_until_last_finishes() {
    let (vs, host, ..) = make();
    let h = host.as_ref();
    let first = vs.lock().begin_in_menu_multi_add(h, |s| {
        s.apply_hub_mode_preset(FollowingHubMode::Multi, h, false, false)
    });
    let second = vs.lock().begin_in_menu_multi_add(h, |s| {
        s.apply_hub_mode_preset(FollowingHubMode::Multi, h, false, false)
    });
    assert_eq!(second.chain_root, first.chain_root, "같은 체인을 잇는다");
    assert_eq!(second.origin.mode, FollowingHubMode::Explore);
    // 옛 요청 실패 — 현재 요청이 아니므로 되돌리지 않는다.
    assert!(!vs
        .lock()
        .finish_in_menu_multi_add(&MultiLiveAddSessionResult::NotLive, &first, h));
    assert_eq!(vs.read().hub_mode(), FollowingHubMode::Multi);
    // 마지막 요청 실패 — 세션이 없으면 체인 시작 전으로.
    assert!(!vs
        .lock()
        .finish_in_menu_multi_add(&MultiLiveAddSessionResult::NotLive, &second, h));
    assert_eq!(vs.read().hub_mode(), FollowingHubMode::Explore);
}

#[test]
fn smart_queue_batch_returns_last_success() {
    let (vs, host, ..) = make();
    let h = host.as_ref();
    let started = vs.lock().begin_in_menu_multi_add(h, |s| {
        s.apply_hub_mode_preset(FollowingHubMode::Multi, h, false, false)
    });
    host.add_session("a");
    let outcomes = vec![
        (
            "a".to_string(),
            MultiLiveAddSessionResult::Added {
                session_id: Uuid::nil(),
            },
        ),
        ("b".to_string(), MultiLiveAddSessionResult::NotLive),
        (
            "c".to_string(),
            MultiLiveAddSessionResult::ActivatedExisting,
        ),
    ];
    assert_eq!(
        vs.lock()
            .finish_smart_queue_batch(&outcomes, Some(&started), h, true)
            .as_deref(),
        Some("c")
    );
    assert_eq!(
        vs.lock().finish_smart_queue_batch(&outcomes, None, h, true),
        None
    );
}

// MARK: - Smart Queue·도크

#[test]
fn smart_queue_enqueue_and_undo() {
    let (vs, ..) = make();
    let toast = Arc::new(RecordingToast::default());
    vs.lock().toast = Some(toast.clone());
    let active: HashSet<String> = ["a".to_string()].into();
    let offline = LiveChannelItem {
        is_live: false,
        ..live("off")
    };
    let n = vs.lock().enqueue_overview_channels(
        &[live("a"), live("b"), live("b"), offline, live("c")],
        &active,
    );
    assert_eq!(n, 2);
    assert_eq!(
        vs.lock().enqueue_overview_channels(&[live("b")], &active),
        0
    );
    vs.lock().clear_smart_queue_with_undo();
    assert!(vs.read().smart_queue_channel_ids.is_empty());
    assert_eq!(
        toast.shown.lock().unwrap().as_slice(),
        ["Smart Queue 를 비웠습니다 (2개)"]
    );
    let undo = toast.undo.lock().unwrap().take().unwrap();
    undo();
    assert_eq!(vs.read().smart_queue_channel_ids, ["b", "c"]);
}

#[test]
fn docks_toggle() {
    let (vs, host, chats, _) = make();
    vs.lock().toggle_channel_dock();
    assert!(
        vs.read().is_channel_dock_hidden,
        "개요에서는 채널 도크를 열지 않는다"
    );
    vs.lock()
        .apply_hub_mode_preset(FollowingHubMode::Watch, host.as_ref(), false, false);
    vs.lock().toggle_channel_dock();
    assert!(!vs.read().is_channel_dock_hidden);
    vs.lock().toggle_chat_dock();
    assert!(vs.read().show_multi_chat && vs.read().chat_dock_revealed_in_full_stage);
    vs.lock().toggle_chat_dock();
    assert!(!vs.read().show_multi_chat);
    let id = host.add_session("g");
    vs.lock()
        .activate_grid_header_channel("g", host.as_ref(), true);
    assert!(host.is_audible(id));
    assert_eq!(chats.selected.lock().unwrap().as_slice(), ["g"]);
}

// MARK: - 추가 결과

#[test]
fn add_session_result_contract() {
    use MultiLiveAddSessionResult as R;
    let added = R::Added {
        session_id: Uuid::nil(),
    };
    assert!(added.is_success() && added.is_new_session());
    assert!(R::ActivatedExisting.is_success() && !R::ActivatedExisting.is_new_session());
    assert_eq!(
        R::batch_outcome(&[R::Cancelled, R::NotLive, added.clone()]),
        added
    );
    assert_eq!(R::batch_outcome(&[R::Cancelled, R::NotLive]), R::NotLive);
    assert_eq!(R::batch_outcome(&[R::Cancelled]), R::Cancelled);
    assert_eq!(R::batch_outcome(&[]), R::Cancelled);
    assert_eq!(R::AlreadyInProgress.user_message(), None);
    assert_eq!(
        R::ApiError {
            message: String::new()
        }
        .user_message()
        .as_deref(),
        Some("채널 추가 실패 — 잠시 후 다시 시도하세요")
    );
    assert_eq!(
        R::ApiError {
            message: "429".into()
        }
        .user_message()
        .as_deref(),
        Some("채널 추가 실패 — 429")
    );
}
