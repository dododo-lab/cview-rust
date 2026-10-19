//! `Sources/CViewApp/Actions/CViewActionRegistry.swift` 포팅.
//!
//! `CViewAction` → 실제 사이드 이펙트(라우터·앱 상태 변경)를 수행하는 단일 디스패처.
//! Swift 레지스트리가 `weak` 로 쥐던 `AppState` 는 [`ActionHost`] trait 로 좁혔다 — 멀티라이브
//! 세션 추가·멀티채팅 연결처럼 ViewModel 이 필요한 일은 호스트가 맡는다.

use crate::navigation::hub_mode::FollowingHubMode;
use crate::navigation::router::{AppRoute, AppRouter, HomeHubMode};

use super::cview_action::{CViewAction, LiveOpenMode};

/// 레지스트리가 앱 상태에 요구하는 진입점.
pub trait ActionHost {
    /// `AppState.openLiveSection(_:router:)` — 사이드바 행과 같은 진입점.
    fn open_live_section(&mut self, mode: FollowingHubMode, router: &mut AppRouter);
    /// `AppState.addEmbeddedMultiLiveSession(channelId:openFollowingPanel:)` 을 예약한다.
    fn add_embedded_multi_live_session(&mut self, channel_id: &str, open_following_panel: bool);
    /// 멀티채팅 세션 추가(`ChatSessionBootstrap.resolveLiveSession` → 세션 매니저)를 예약한다.
    fn add_to_multi_chat(&mut self, channel_id: &str);
}

/// 단일 액션 디스패처.
#[derive(Debug, Default, Clone, Copy)]
pub struct CViewActionRegistry;

impl CViewActionRegistry {
    pub fn new() -> Self {
        Self
    }

    /// 액션 하나를 실제 라우팅/상태 변경으로 바꾼다. 검증에 실패하면 false.
    pub fn dispatch(
        &self,
        raw: &CViewAction,
        router: &mut AppRouter,
        host: &mut dyn ActionHost,
    ) -> bool {
        let Some(action) = raw.validated() else {
            tracing::warn!("Action 무시 (validation 실패): {}", raw.id());
            return false;
        };
        tracing::info!("Action dispatch: {}", action.id());
        match action {
            CViewAction::OpenLive { channel_id, mode } => match mode {
                LiveOpenMode::Watch => router.navigate_to_watch(&channel_id, None),
                LiveOpenMode::Multi => {
                    // 이동은 사용자 액션 시점에 확정한다 — 멀티 모드를 먼저 예약한다.
                    host.open_live_section(FollowingHubMode::Multi, router);
                    host.add_embedded_multi_live_session(&channel_id, true);
                }
                LiveOpenMode::ChatOnly => router.navigate(AppRoute::ChatOnly { channel_id }),
            },
            CViewAction::OpenChannel { channel_id } => {
                router.navigate(AppRoute::ChannelDetail { channel_id })
            }
            CViewAction::AddToMultiLive { channel_id } => {
                host.add_embedded_multi_live_session(&channel_id, false)
            }
            CViewAction::AddToMultiChat { channel_id } => host.add_to_multi_chat(&channel_id),
            CViewAction::SwitchLiveMode(mode) => host.open_live_section(mode, router),
            CViewAction::OpenSearch { query } => router.navigate(AppRoute::Search { query }),
            CViewAction::OpenClip { clip_uid } => router.navigate(AppRoute::Clip { clip_uid }),
            CViewAction::OpenHome => {
                router.set_home_hub_mode(HomeHubMode::Feed);
                router.navigate(AppRoute::Home);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::router::SidebarItem;
    use crate::services::menu_transition_gate::MenuTransitionGate;
    use crate::services::support::MemoryKeyValueStore;
    use std::sync::Arc;

    #[derive(Default)]
    struct Host {
        sections: Vec<FollowingHubMode>,
        multi: Vec<(String, bool)>,
        chats: Vec<String>,
    }
    impl ActionHost for Host {
        fn open_live_section(&mut self, mode: FollowingHubMode, router: &mut AppRouter) {
            self.sections.push(mode);
            router.select_sidebar(SidebarItem::Following);
        }
        fn add_embedded_multi_live_session(&mut self, id: &str, open: bool) {
            self.multi.push((id.into(), open));
        }
        fn add_to_multi_chat(&mut self, id: &str) {
            self.chats.push(id.into());
        }
    }

    fn router() -> AppRouter {
        AppRouter::with_gate(
            MemoryKeyValueStore::shared(),
            Arc::new(MenuTransitionGate::default()),
        )
    }

    #[test]
    fn invalid_action_is_ignored() {
        let mut r = router();
        let mut h = Host::default();
        assert!(!CViewActionRegistry.dispatch(
            &CViewAction::OpenChannel {
                channel_id: " ".into()
            },
            &mut r,
            &mut h
        ));
        assert!(r.path().is_empty());
    }

    #[test]
    fn open_live_multi_reserves_mode_then_adds() {
        let mut r = router();
        let mut h = Host::default();
        CViewActionRegistry.dispatch(
            &CViewAction::OpenLive {
                channel_id: "c".into(),
                mode: LiveOpenMode::Multi,
            },
            &mut r,
            &mut h,
        );
        assert_eq!(h.sections, vec![FollowingHubMode::Multi]);
        assert_eq!(h.multi, vec![("c".into(), true)]);
    }

    #[test]
    fn open_home_resets_hub_mode() {
        let mut r = router();
        let mut h = Host::default();
        r.set_home_hub_mode(HomeHubMode::Insights);
        r.select_sidebar(SidebarItem::Library);
        CViewActionRegistry.dispatch(&CViewAction::OpenHome, &mut r, &mut h);
        assert_eq!(r.home_hub_mode, HomeHubMode::Feed);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
    }

    #[test]
    fn add_to_multi_chat_goes_to_host() {
        let mut r = router();
        let mut h = Host::default();
        CViewActionRegistry.dispatch(
            &CViewAction::AddToMultiChat {
                channel_id: " z ".into(),
            },
            &mut r,
            &mut h,
        );
        assert_eq!(h.chats, vec!["z".to_string()]);
    }
}
