//! `Sources/CViewApp/Navigation/DeepLinkRouter.swift` 포팅.
//!
//! 지원 URL Scheme:
//! - `cview://live?channelId=XYZ[&mode=watch|multi|chatonly]`
//! - `cview://channel?channelId=XYZ`
//! - `cview://following[?mode=explore|watch|multi|search|popular|category]`
//! - `cview://multilive`, `cview://home`, `cview://search[?q=keyword]`, `cview://clip?id=UID`,
//!   `cview://login`
//!
//! `DeepLinkRouter.apply` 가 쓰던 `AppState` 는 [`DeepLinkHost`] trait 로 좁혔다.

use std::collections::HashSet;

use url::Url;

use super::hub_mode::FollowingHubMode;
use super::router::{AppRoute, AppRouter, SheetRoute, SidebarItem};
use crate::actions::cview_action::{CViewAction, LiveOpenMode};

/// URL Scheme 으로 들어온 명령.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    Live {
        channel_id: String,
        mode: LiveOpenMode,
    },
    Channel {
        channel_id: String,
    },
    Following {
        mode: Option<FollowingHubMode>,
    },
    MultiLive,
    Home,
    Search {
        query: Option<String>,
    },
    Clip {
        uid: String,
    },
    Login,
}

/// `%XX` 디코딩(`+` 는 그대로 — `URLComponents.queryItems` 와 같다). 잘못된 UTF-8 은 None.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// `URLComponents.queryItems` — 이름과 (있으면) 디코딩된 값.
fn query_items(url: &Url) -> Vec<(String, Option<String>)> {
    let Some(query) = url.query() else {
        return Vec::new();
    };
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (
                percent_decode(name).unwrap_or_else(|| name.to_string()),
                percent_decode(value),
            ),
            None => (
                percent_decode(pair).unwrap_or_else(|| pair.to_string()),
                None,
            ),
        })
        .collect()
}

impl DeepLink {
    /// `cview://...` 문자열을 파싱한다.
    pub fn parse(raw: &str) -> Option<Self> {
        Url::parse(raw).ok().and_then(|u| Self::from_url(&u))
    }

    /// `init?(url:)`.
    pub fn from_url(url: &Url) -> Option<Self> {
        if url.scheme().to_lowercase() != "cview"
            || url.as_str().len() > 4096
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let path = url.path();
        if !(path.is_empty() || path == "/") {
            return None;
        }

        let host = url.host_str().unwrap_or("").to_lowercase();
        let items = query_items(url);
        let names: Vec<String> = items.iter().map(|(n, _)| n.to_lowercase()).collect();
        let unique: HashSet<&String> = names.iter().collect();
        if unique.len() != names.len()
            || (names.iter().any(|n| n == "q") && names.iter().any(|n| n == "query"))
            || (names.iter().any(|n| n == "id") && names.iter().any(|n| n == "uid"))
        {
            return None;
        }
        let q = |name: &str| -> Option<String> {
            let lowered = name.to_lowercase();
            items
                .iter()
                .find(|(n, _)| n.to_lowercase() == lowered)
                .and_then(|(_, v)| v.clone())
        };
        let channel_id = q("channelId");

        match host.as_str() {
            "live" => {
                let id = channel_id.filter(|id| Self::is_valid_identifier(id))?;
                let mode = match q("mode").map(|m| m.to_lowercase()).as_deref() {
                    Some("multi") => LiveOpenMode::Multi,
                    Some("chatonly") | Some("chat-only") | Some("chat") => LiveOpenMode::ChatOnly,
                    _ => LiveOpenMode::Watch,
                };
                Some(Self::Live {
                    channel_id: id,
                    mode,
                })
            }
            "channel" => {
                let id = channel_id.filter(|id| Self::is_valid_identifier(id))?;
                Some(Self::Channel { channel_id: id })
            }
            "following" => Some(Self::Following {
                mode: q("mode").and_then(|m| FollowingHubMode::from_deep_link_token(&m)),
            }),
            "multilive" => Some(Self::MultiLive),
            "home" | "" => Some(Self::Home),
            "search" => {
                let raw = q("q").or_else(|| q("query"));
                let trimmed = raw.map(|r| r.trim().to_string());
                if trimmed.as_ref().map_or(0, |t| t.chars().count()) > 512 {
                    return None;
                }
                Some(Self::Search {
                    query: trimmed.filter(|t| !t.is_empty()),
                })
            }
            "clip" => {
                let uid = q("id")
                    .or_else(|| q("uid"))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !Self::is_valid_identifier(&uid) {
                    return None;
                }
                Some(Self::Clip { uid })
            }
            "login" => Some(Self::Login),
            _ => None,
        }
    }

    fn is_valid_identifier(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }

    /// `CViewAction` 으로 변환(없으면 None — 직접 라우팅 필요).
    pub fn as_action(&self) -> Option<CViewAction> {
        match self {
            Self::Live { channel_id, mode } => Some(CViewAction::OpenLive {
                channel_id: channel_id.clone(),
                mode: *mode,
            }),
            Self::Channel { channel_id } => Some(CViewAction::OpenChannel {
                channel_id: channel_id.clone(),
            }),
            Self::Following { mode } => mode.map(CViewAction::SwitchLiveMode),
            Self::MultiLive => Some(CViewAction::SwitchLiveMode(FollowingHubMode::Multi)),
            Self::Home => Some(CViewAction::OpenHome),
            Self::Search { query } => Some(CViewAction::OpenSearch {
                query: query.clone(),
            }),
            Self::Clip { uid } => Some(CViewAction::OpenClip {
                clip_uid: uid.clone(),
            }),
            Self::Login => None,
        }
    }
}

/// 딥링크 적용에 필요한 앱 상태 쪽 진입점.
pub trait DeepLinkHost {
    /// 액션 레지스트리로 위임. 레지스트리가 없거나 실패하면 false.
    fn dispatch_action(&mut self, router: &mut AppRouter, action: &CViewAction) -> bool;
    /// `AppState.openLiveSection(_:router:)`.
    fn open_live_section(&mut self, mode: FollowingHubMode, router: &mut AppRouter);
}

/// 외부에서 들어온 `DeepLink` 를 라우터 + 앱 상태에 적용한다.
pub struct DeepLinkRouter;

impl DeepLinkRouter {
    /// URL 을 받아 라우팅 적용. 알 수 없는 스킴/형식은 조용히 무시. 적용했으면 true.
    pub fn handle(url: &str, router: &mut AppRouter, host: &mut dyn DeepLinkHost) -> bool {
        match DeepLink::parse(url) {
            Some(link) => {
                Self::apply(&link, router, host);
                true
            }
            None => false,
        }
    }

    pub fn apply(link: &DeepLink, router: &mut AppRouter, host: &mut dyn DeepLinkHost) {
        // 1) 액션으로 바뀌는 케이스는 레지스트리 위임(단일 진입점).
        if let Some(action) = link.as_action() {
            if host.dispatch_action(router, &action) {
                return;
            }
        }
        // 2) 레지스트리 미바인딩/실패 시 fallback.
        match link {
            DeepLink::Live { channel_id, mode } => match mode {
                LiveOpenMode::Watch => router.navigate_to_watch(channel_id, None),
                LiveOpenMode::Multi => router.navigate_to_multi_live_channel(channel_id, None),
                LiveOpenMode::ChatOnly => router.navigate(AppRoute::ChatOnly {
                    channel_id: channel_id.clone(),
                }),
            },
            DeepLink::Channel { channel_id } => router.navigate(AppRoute::ChannelDetail {
                channel_id: channel_id.clone(),
            }),
            DeepLink::Following { mode } => {
                host.open_live_section(mode.unwrap_or(FollowingHubMode::Explore), router)
            }
            DeepLink::MultiLive => router.navigate_to_multi_live(),
            DeepLink::Home => {
                router.select_sidebar(SidebarItem::Home);
                router.set_path(Vec::new());
            }
            DeepLink::Search { query } => router.navigate(AppRoute::Search {
                query: query.clone(),
            }),
            DeepLink::Clip { uid } => router.navigate(AppRoute::Clip {
                clip_uid: uid.clone(),
            }),
            DeepLink::Login => router.present_sheet(SheetRoute::Login),
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/DeepLinkSecurityTests.swift` (+ 파싱 계약).
    use super::*;

    #[test]
    fn rejects_ambiguous_or_unsafe_ids() {
        let long = format!("cview://live?channelId={}", "a".repeat(129));
        for value in [
            "cview://live?channelId=../secret",
            "cview://channel?channelId=a%2Fb",
            "cview://clip?id=a%3Fb",
            "cview://clip?id=abc&uid=def",
            "cview://live?channelId=a&CHANNELID=b",
            "cview://user@home",
            "cview://home:1234",
            "cview://home/ignored",
            long.as_str(),
        ] {
            assert_eq!(DeepLink::parse(value), None, "{value}");
        }
    }

    #[test]
    fn parses_supported_forms() {
        assert_eq!(
            DeepLink::parse("cview://live?channelId=abc&mode=chat-only"),
            Some(DeepLink::Live {
                channel_id: "abc".into(),
                mode: LiveOpenMode::ChatOnly
            })
        );
        assert_eq!(
            DeepLink::parse("CVIEW://following?mode=popular"),
            Some(DeepLink::Following {
                mode: Some(FollowingHubMode::Popular)
            })
        );
        assert_eq!(
            DeepLink::parse("cview://following?mode=%EC%B9%B4%ED%85%8C%EA%B3%A0%EB%A6%AC"),
            Some(DeepLink::Following {
                mode: Some(FollowingHubMode::Category)
            })
        );
        assert_eq!(
            DeepLink::parse("cview://search?q=%20%20"),
            Some(DeepLink::Search { query: None })
        );
        assert_eq!(
            DeepLink::parse("cview://search?query=hi"),
            Some(DeepLink::Search {
                query: Some("hi".into())
            })
        );
        assert_eq!(DeepLink::parse("cview://"), Some(DeepLink::Home));
        assert_eq!(DeepLink::parse("cview://home/"), Some(DeepLink::Home));
        assert_eq!(DeepLink::parse("https://home"), None);
        assert_eq!(DeepLink::parse("cview://unknown"), None);
        assert_eq!(DeepLink::parse("cview://login"), Some(DeepLink::Login));
        assert_eq!(
            DeepLink::parse("cview://clip?uid=X_1-2"),
            Some(DeepLink::Clip {
                uid: "X_1-2".into()
            })
        );
    }

    #[test]
    fn as_action_mapping() {
        assert_eq!(
            DeepLink::MultiLive.as_action(),
            Some(CViewAction::SwitchLiveMode(FollowingHubMode::Multi))
        );
        assert_eq!(DeepLink::Following { mode: None }.as_action(), None);
        assert_eq!(DeepLink::Login.as_action(), None);
    }

    struct NoRegistry {
        opened: Vec<FollowingHubMode>,
    }
    impl DeepLinkHost for NoRegistry {
        fn dispatch_action(&mut self, _: &mut AppRouter, _: &CViewAction) -> bool {
            false
        }
        fn open_live_section(&mut self, mode: FollowingHubMode, _: &mut AppRouter) {
            self.opened.push(mode);
        }
    }

    fn router() -> AppRouter {
        AppRouter::with_gate(
            crate::services::support::MemoryKeyValueStore::shared(),
            std::sync::Arc::new(
                crate::services::menu_transition_gate::MenuTransitionGate::default(),
            ),
        )
    }

    #[test]
    fn fallback_preserves_live_mode_without_registry() {
        let mut r = router();
        let mut host = NoRegistry { opened: vec![] };
        DeepLinkRouter::handle("cview://live?channelId=c1&mode=multi", &mut r, &mut host);
        assert_eq!(r.pending_multi_channel_id.as_deref(), Some("c1"));
        DeepLinkRouter::handle("cview://live?channelId=c2&mode=chat", &mut r, &mut host);
        assert_eq!(
            r.path().last(),
            Some(&AppRoute::ChatOnly {
                channel_id: "c2".into()
            })
        );
        DeepLinkRouter::handle("cview://following", &mut r, &mut host);
        assert_eq!(host.opened, vec![FollowingHubMode::Explore]);
        DeepLinkRouter::handle("cview://login", &mut r, &mut host);
        assert_eq!(r.presented_sheet, Some(SheetRoute::Login));
    }

    #[test]
    fn home_fallback_leaves_settings_mode() {
        let mut r = router();
        r.enter_settings();
        let mut host = NoRegistry { opened: vec![] };
        DeepLinkRouter::handle("cview://home", &mut r, &mut host);
        assert!(!r.is_in_settings_mode);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
    }
}
