//! 원본: `Tests/CViewAppTests` 의 AppState 인증 경계·프로필 재동기화·로컬 삭제·프리셋 게이트·
//! 저장소 복구 테스트 핵심.
use super::*;
use crate::services::channel_preset_store::PresetKind;
use crate::services::support::MemoryKeyValueStore;
use cview_core::utils::time::from_unix;

fn state() -> AppState {
    AppState::new(MemoryKeyValueStore::shared())
}

#[test]
fn sidebar_collapsed_persists() {
    let kv = MemoryKeyValueStore::shared();
    let mut s = AppState::new(kv.clone());
    assert!(!s.sidebar_collapsed());
    s.toggle_sidebar();
    assert!(kv.bool(SIDEBAR_COLLAPSED_KEY));
    assert!(!s.set_sidebar_collapsed(true), "같은 값은 쓰지 않는다");
    assert!(AppState::new(kv).sidebar_collapsed());
}

#[test]
fn workspace_restore_only_applies_live_modes_without_explicit_navigation() {
    let s = state();
    let mut router = AppRouter::new(MemoryKeyValueStore::shared());
    let inputs = WorkspaceInputs {
        hub_mode: FollowingHubMode::Multi,
        multi_live_channel_ids: vec!["a".into()],
        multi_chat_channel_ids: vec![],
        smart_queue_channel_ids: vec!["q".into()],
    };
    let snap = s.capture_workspace_snapshot(Some(&router), &inputs, from_unix(1.0));
    assert_eq!(snap.sidebar.as_deref(), Some("홈"));
    let restore = workspace_restore(&snap, &router);
    assert_eq!(restore.hub_mode, Some(FollowingHubMode::Multi));
    assert_eq!(restore.smart_queue_channel_ids, Some(vec!["q".to_string()]));

    let mut old = snap.clone();
    old.hub_mode = Some("검색".into());
    assert_eq!(
        workspace_restore(&old, &router).hub_mode,
        None,
        "탐색 모드는 복원하지 않는다"
    );

    router.select_sidebar(SidebarItem::Search);
    assert_eq!(workspace_restore(&snap, &router).hub_mode, None);
}

#[test]
fn live_section_routes_or_applies_mode() {
    let mut router = AppRouter::new(MemoryKeyValueStore::shared());
    assert_eq!(
        open_live_section(FollowingHubMode::Popular, &mut router),
        LiveSectionEffect::Routed
    );
    assert_eq!(router.selected_sidebar_item, SidebarItem::AllBroadcasts);

    assert_eq!(
        open_live_section(FollowingHubMode::Multi, &mut router),
        LiveSectionEffect::Routed
    );
    assert_eq!(router.selected_sidebar_item, SidebarItem::Following);
    assert_eq!(router.pending_hub_mode, Some(FollowingHubMode::Multi));
    assert_eq!(
        pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Explore),
        Some(FollowingHubMode::Multi)
    );
    assert_eq!(router.pending_hub_mode, None, "예약은 소비된다");

    assert_eq!(
        open_live_section(FollowingHubMode::Watch, &mut router),
        LiveSectionEffect::ApplyHubModePreset(FollowingHubMode::Watch)
    );
    assert_eq!(
        pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Search),
        Some(FollowingHubMode::Explore),
        "탐색 모드 잔재는 개요로 정규화"
    );
    assert_eq!(
        pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Watch),
        None
    );
}

#[test]
fn auth_boundary_rotates_scope_and_clears_account_state() {
    let kv = MemoryKeyValueStore::shared();
    let mut s = AppState::new(kv.clone());
    let old = Uuid::new_v4();
    kv.set_string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY, &scope_string(old));
    s.is_logged_in = true;
    s.user_nickname = Some("n".into());
    s.favorites_store.hydrate(vec!["c".to_string()]);
    s.profile_resync.arm(s.authentication_ui_epoch, 3, None);
    let before = s.authentication_ui_epoch;

    let b = s.begin_authentication_ui_transition(None);
    assert_ne!(b.epoch, before);
    assert!(!s.is_current_epoch(before));
    assert_eq!(b.retired_scope, Some(old));
    assert_eq!(
        kv.string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY),
        Some(scope_string(b.scope))
    );
    assert_eq!(b.vod_subscription_tag, s.persisted_vod_subscription_tag());
    assert!(!s.is_logged_in && s.user_nickname.is_none());
    assert!(s.favorites_store.channel_ids().is_empty());
    assert!(!s.profile_resync.is_armed());
    assert!(!s.apply_login_result(before, true), "옛 에포크는 버린다");
    assert!(s.apply_login_result(b.epoch, true));
}

#[test]
fn profile_application_rules() {
    let mut s = state();
    let epoch = s.authentication_ui_epoch;
    assert!(
        s.apply_user_status(epoch, Some("a"), None, None).is_none(),
        "비로그인"
    );
    s.is_logged_in = true;
    let id = s
        .apply_user_status(epoch, Some("a"), Some("http://img"), Some("hash"))
        .unwrap();
    assert_eq!(id.chat_uid.as_deref(), Some("hash"));
    assert_eq!(id.multi_live_uid, None);
    s.apply_user_status(epoch, Some("b"), Some("http://other"), None);
    assert_eq!(
        s.user_nickname.as_deref(),
        Some("a"),
        "쿠키 경로는 덮지 않는다"
    );
    assert_eq!(s.user_profile_url.as_deref(), Some("http://img"));
    assert!(!s.is_profile_incomplete(true) || s.user_channel_id.is_none());
    assert!(s.is_profile_incomplete(true), "OAuth 세션인데 채널 ID 없음");
    assert!(!s.is_profile_incomplete(false));
    let id = s
        .apply_oauth_profile(epoch, None, Some("ch"), None)
        .unwrap();
    assert_eq!(id.chat_uid.as_deref(), Some("ch"));
    assert_eq!(s.user_nickname.as_deref(), Some("a"));
    assert!(!s.is_profile_incomplete(true));
}

#[test]
fn profile_resync_signals_once_and_rearms_with_spacing() {
    let epoch = Uuid::new_v4();
    let mut r = ProfileResync::default();
    assert_eq!(r.signal(0.0, epoch), None);
    r.arm(epoch, ProfileResync::DEFAULT_ATTEMPTS, None);
    assert_eq!(r.signal(0.0, Uuid::new_v4()), None, "에포크가 다르면 무시");
    let run = r.signal(0.0, epoch).unwrap();
    assert_eq!(r.signal(0.0, epoch), None, "한 번에 하나");
    assert!(r.rearm_after(run, true, epoch, 10.0));
    assert_eq!(r.signal(69.0, epoch), None, "간격 전");
    let run = r.signal(70.0, epoch).unwrap();
    assert_eq!(run.attempts, 2);
    assert!(!r.rearm_after(run, false, epoch, 70.0), "완성되면 멈춘다");
    assert!(!r.rearm_after(ResyncRun { epoch, attempts: 1 }, true, epoch, 0.0));
}

#[test]
fn auth_follow_up_only_takes_oauth_refresh_and_final_expiry() {
    let oauth = AuthState::LoggedIn {
        user_id: "oauth".into(),
    };
    assert_eq!(auth_follow_up(false, &oauth), AuthFollowUp::Ignore);
    assert_eq!(
        auth_follow_up(true, &oauth),
        AuthFollowUp::FireProfileResyncSignal
    );
    assert_eq!(
        auth_follow_up(
            true,
            &AuthState::LoggedIn {
                user_id: "cookie".into()
            }
        ),
        AuthFollowUp::Ignore
    );
    assert_eq!(
        auth_follow_up(true, &AuthState::Expired),
        AuthFollowUp::VerifyExpiry
    );
    assert!(should_end_expired_session(
        &AuthState::Expired,
        false,
        true,
        true
    ));
    assert!(!should_end_expired_session(
        &AuthState::Expired,
        true,
        true,
        true
    ));
    assert!(!should_end_expired_session(
        &AuthState::LoggedOut,
        false,
        true,
        true
    ));
}

struct FailingRows {
    fail_at: usize,
}

#[async_trait]
impl LocalUserRows for FailingRows {
    async fn delete_all_watch_history(&self) -> Result<(), String> {
        (self.fail_at != 0).then_some(()).ok_or("h".into())
    }
    async fn clear_all_last_watched(&self) -> Result<(), String> {
        (self.fail_at != 1).then_some(()).ok_or("l".into())
    }
    async fn delete_all_vod_playback(&self) -> Result<(), String> {
        (self.fail_at != 2).then_some(()).ok_or("v".into())
    }
    async fn delete_all_clip_playback(&self) -> Result<(), String> {
        (self.fail_at != 3).then_some(()).ok_or("c".into())
    }
    async fn clear_all_favorites(&self) -> Result<(), String> {
        (self.fail_at != 4).then_some(()).ok_or("f".into())
    }
}

#[tokio::test]
async fn local_clear_only_empties_mirrors_of_succeeded_steps() {
    let cleared = delete_local_user_rows(&FailingRows { fail_at: 2 }).await;
    assert_eq!(
        cleared,
        LocalUserDataClearance {
            history: true,
            last_watched: true,
            failure: Some("v".into()),
            ..Default::default()
        }
    );
    let mut s = state();
    s.favorites_store.hydrate(vec!["fav".to_string()]);
    let mut vod = VodPlaybackProgressStore::new();
    s.apply_local_user_data_clearance(&cleared, &mut vod);
    assert!(
        s.favorites_store.contains("fav"),
        "실패한 단계는 미러를 남긴다"
    );
    assert!(s.recent_channels_store.channel_ids().is_empty());
    assert_eq!(s.local_user_data_revision, 0);
    s.finish_local_user_data_clear();
    assert_eq!(s.local_user_data_revision, 1);

    let all = delete_local_user_rows(&FailingRows { fail_at: 9 }).await;
    assert!(all.favorites && all.failure.is_none());
}

#[test]
fn reload_failures_keep_mirrors() {
    let mut s = state();
    s.apply_favorites_reload::<()>(Ok(vec!["a".into()]));
    s.apply_favorites_reload(Err("cancelled"));
    assert!(s.favorites_store.contains("a"));
    let generation = s.recent_channels_store.generation();
    assert_eq!(s.apply_recent_channels_reload(generation, Err(())), None);
    assert!(!s.recent_channels_store.did_hydrate());
    s.apply_saved_favorite("a", false);
    assert!(!s.favorites_store.contains("a"));
}

#[derive(Default)]
struct RecordingSnapshots {
    log: Mutex<Vec<String>>,
}

#[async_trait]
impl AccountSnapshots for RecordingSnapshots {
    async fn remove_scope(&self, scope: &str) {
        tokio::task::yield_now().await;
        lock(&self.log).push(format!("remove:{scope}"));
    }
    async fn prune_accounts(&self, keeping: &[String]) {
        lock(&self.log).push(format!("prune:{}", keeping.join(",")));
    }
    async fn sweep(&self, keep: &[String]) {
        lock(&self.log).push(format!("sweep:{}", keep.join(",")));
    }
}

#[tokio::test]
async fn account_cleanup_chain_keeps_order_and_skips_stale_prune() {
    let snaps = Arc::new(RecordingSnapshots::default());
    let chain = AccountSnapshotCleanup::new();
    let old = Uuid::new_v4();
    let new = Uuid::new_v4();
    let current: ScopeReader = Arc::new(move || Some(new));
    chain.retire(snaps.clone(), Some(old), Some(async {}.boxed()));
    chain.prune(snaps.clone(), old, current.clone());
    chain.prune(snaps.clone(), new, current.clone());
    chain.sweep(snaps.as_ref(), current).await;
    assert_eq!(
        *lock(&snaps.log),
        vec![
            format!("remove:{}", scope_string(old)),
            format!("prune:{}", scope_string(new)),
            format!("sweep:{}", scope_string(new)),
        ]
    );
    chain.sweep(snaps.as_ref(), Arc::new(|| None)).await;
    assert_eq!(lock(&snaps.log).len(), 3, "범위를 모르면 건너뛴다");
}

#[test]
fn settle_rules() {
    let ok = MultiLiveAddOutcome {
        is_success: true,
        is_new_session: true,
        user_message: None,
    };
    let fx = settle_embedded_multi_live_add(&ok, Some(true), true, false);
    assert!(fx.activate_channel && fx.preserves_main_focus && fx.show_multi_live_panel);
    assert!(fx.release_video_surfaces);
    let fx = settle_embedded_multi_live_add(&ok, Some(false), true, true);
    assert!(!fx.activate_channel && !fx.show_multi_live_panel && !fx.is_current);
    let fx = settle_embedded_multi_live_add(&ok, None, false, true);
    assert!(!fx.activate_channel && fx.is_current);
    let err = MultiLiveAddOutcome {
        is_success: false,
        is_new_session: false,
        user_message: Some("실패".into()),
    };
    assert_eq!(
        settle_embedded_multi_live_add(&err, Some(true), false, true).error_message,
        Some("실패".into())
    );
    assert_eq!(
        settle_embedded_multi_live_add(&err, Some(false), false, true).error_message,
        None
    );
    assert_eq!(
        settle_deferred_multi_live_navigation(true, false),
        (true, false)
    );
    assert_eq!(
        settle_deferred_multi_live_navigation(false, false),
        (false, true)
    );
}

#[test]
fn notices() {
    let p = Preset::new("묶음", vec!["a".into(), "b".into()], PresetKind::Live);
    assert_eq!(preset_apply_notice(&p, &p.channels), None);
    let n = preset_apply_notice(&p, &[]).unwrap();
    assert_eq!(n.message, "'묶음' 묶음을 열었습니다 (채널 2개)");
    assert!(n.undo_channel_ids.is_none());
    let n = preset_apply_notice(&p, &["x".into()]).unwrap();
    assert_eq!(n.message, "'묶음' 묶음으로 바꿨습니다 (이전 1개 해제)");
    assert_eq!(n.undo_channel_ids, Some(vec!["x".to_string()]));
    assert_eq!(stop_all_notice(&[]), None);
    assert_eq!(
        stop_all_notice(&["a".into(), "b".into()]).unwrap().message,
        "2개 채널을 모두 해제했습니다"
    );
    assert_eq!(
        close_session_notice("c", "").message,
        "채널 세션을 종료했습니다"
    );
}

/// 멀티라이브 매니저 가짜 — `slow` 채널의 추가는 `release` 가 열릴 때까지 멈춘다.
struct FakeManager {
    sessions: Mutex<Vec<String>>,
    slow: String,
    release: tokio::sync::Notify,
    started_slow: tokio::sync::Notify,
}

impl FakeManager {
    fn new(initial: &[&str], slow: &str) -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(initial.iter().map(|s| s.to_string()).collect()),
            slow: slow.into(),
            release: tokio::sync::Notify::new(),
            started_slow: tokio::sync::Notify::new(),
        })
    }
}

#[async_trait]
impl MultiLiveComposition for FakeManager {
    fn current_channel_ids(&self) -> Vec<String> {
        lock(&self.sessions).clone()
    }
    async fn stop_all(&self) {
        lock(&self.sessions).clear();
    }
    async fn add_channel(&self, id: &str) {
        if id == self.slow {
            self.started_slow.notify_one();
            self.release.notified().await;
        }
        lock(&self.sessions).push(id.to_string());
    }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn gate_noop_when_already_that_composition() {
    let gate = MultiLivePresetApplyGate::new();
    let m = FakeManager::new(&["a"], "");
    assert_eq!(
        replace_multi_live_channels(&gate, m.clone(), ids(&["a"])).await,
        None
    );
    let t = replace_multi_live_channels(&gate, m.clone(), ids(&["b", "c"]))
        .await
        .unwrap();
    assert_eq!(t.origin, ids(&["a"]));
    assert_eq!(m.current_channel_ids(), ids(&["b", "c"]));
}

#[tokio::test]
async fn overlapping_applies_do_not_mix_and_keep_chain_origin() {
    let gate = MultiLivePresetApplyGate::new();
    let m = FakeManager::new(&["orig"], "slow");
    let first = {
        let gate = gate.clone();
        let m = m.clone();
        tokio::spawn(
            async move { replace_multi_live_channels(&gate, m, ids(&["slow", "x"])).await },
        )
    };
    m.started_slow.notified().await;
    let second = {
        let gate = gate.clone();
        let m = m.clone();
        tokio::spawn(async move { replace_multi_live_channels(&gate, m, ids(&["y"])).await })
    };
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    m.release.notify_one();
    assert_eq!(first.await.unwrap(), None, "대체된 적용은 티켓이 없다");
    let t = second.await.unwrap().unwrap();
    assert_eq!(
        t.origin,
        ids(&["orig"]),
        "실행취소 기준은 사슬의 첫 적용 직전"
    );
    assert_eq!(
        m.current_channel_ids(),
        ids(&["y"]),
        "앞 루프의 x 가 섞이지 않는다"
    );
}

#[tokio::test]
async fn supersede_stops_running_apply() {
    let gate = MultiLivePresetApplyGate::new();
    let m = FakeManager::new(&[], "slow");
    let run = {
        let gate = gate.clone();
        let m = m.clone();
        tokio::spawn(
            async move { replace_multi_live_channels(&gate, m, ids(&["slow", "x"])).await },
        )
    };
    m.started_slow.notified().await;
    gate.supersede();
    m.release.notify_one();
    assert_eq!(run.await.unwrap(), None);
    assert_eq!(m.current_channel_ids(), ids(&["slow"]));
}

#[test]
fn store_recovery_summary_and_restore_gate() {
    let mut health = StoreOpenHealth::default();
    let err = ErrorSummary {
        domain: "SQLite".into(),
        code: 1,
        underlying_domain: Some("IO".into()),
        underlying_code: Some(13),
        sqlite_code: None,
    };
    let info = store_recovery_info(&health, err.clone(), vec![]);
    assert_eq!(info.failure.consecutive_failures, 1);
    assert_eq!(
        info.failure.display_line(),
        "오류 코드 SQLite 1 (IO 13) · 연속 1회"
    );
    assert_eq!(
        info.failure.support_line(),
        "저장소 열기 실패 1회 (SQLite 1)"
    );
    health.consecutive_failures = 2;
    health.last_error = Some(ErrorSummary {
        domain: "Schema".into(),
        code: 9,
        underlying_domain: None,
        underlying_code: None,
        sqlite_code: None,
    });
    let info = store_recovery_info(&health, err, vec![]);
    assert_eq!(info.failure.display_line(), "오류 코드 Schema 9 · 연속 2회");

    let mut s = state();
    s.note_store_open_failure(info.clone());
    assert!(
        s.last_store_open_failure.is_none(),
        "실패 상태가 아니면 덮지 않는다"
    );
    assert!(!s.begin_store_restore());
    s.data_store_initialization_failed = true;
    s.note_store_open_failure(info);
    assert!(s.last_store_open_failure.is_some());
    assert!(s.begin_store_restore());
    assert!(!s.begin_store_restore(), "복원 중");
    assert!(!s.finish_store_restore_copy(false));
    assert!(s.store_restore_failed && !s.is_restoring_store_backup);
    assert!(s.begin_store_restore());
    assert!(s.finish_store_restore_copy(true));
    s.data_store_initialization_failed = false;
    s.is_settings_loaded = true;
    assert!(s.finish_store_restore());
    s.clear_store_recovery_state();
    assert!(s.last_store_open_failure.is_none());
    assert_eq!(
        WindowVisibilitySample {
            visible: true,
            active: false
        }
        .fields()["active"],
        "0"
    );
}
