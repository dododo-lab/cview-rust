//! `Sources/CViewCore/Models/StreamProxyMode.swift` · `StreamProxyRouteStatus.swift` 포팅.
//!
//! 스트림 프록시 / 인터셉트 모드 — chzzk CDN 이 fMP4 세그먼트를 잘못된 Content-Type 으로
//! 응답하는 이슈를 해결하기 위한 전략. [엔진 단일화 2026-08-11] 이후 두 가지만 남았다.

use serde::{Deserialize, Deserializer, Serialize};

/// 스트림 프록시 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum StreamProxyMode {
    /// 기본값. 로컬 HTTP 프록시 경유.
    #[default]
    #[serde(rename = "localProxy")]
    LocalProxy,
    /// 직접 재생. 어떤 보정도 하지 않음. 디버깅용.
    #[serde(rename = "none")]
    None,
}

impl<'de> Deserialize<'de> for StreamProxyMode {
    /// 제거된 케이스(`directVLCAdaptive` 등)가 저장돼 있어도 설정 전체를 날리지 않는다 —
    /// 알 수 없는 값은 안전값 `LocalProxy` 로 흡수한다. 문자열이 아니면 에러.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_raw(&raw).unwrap_or(StreamProxyMode::LocalProxy))
    }
}

impl StreamProxyMode {
    pub const ALL: [StreamProxyMode; 2] = [StreamProxyMode::LocalProxy, StreamProxyMode::None];

    pub fn raw_value(self) -> &'static str {
        match self {
            StreamProxyMode::LocalProxy => "localProxy",
            StreamProxyMode::None => "none",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "localProxy" => Some(StreamProxyMode::LocalProxy),
            "none" => Some(StreamProxyMode::None),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            StreamProxyMode::LocalProxy => "로컬 프록시 (기본)",
            StreamProxyMode::None => "직접 재생 (보정 없음)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            StreamProxyMode::LocalProxy => {
                "호환성 보정과 인증 처리를 위해 권장합니다. 변경 시 필요한 영상 연결만 다시 시작합니다."
            }
            StreamProxyMode::None => {
                "원본 URL로 재생합니다. 암호화 방송은 로그인 인증을 위해 로컬 프록시를 사용합니다."
            }
        }
    }

    pub fn is_experimental(self) -> bool {
        false
    }

    /// UI picker 에 노출할 케이스 목록.
    pub fn visible_cases(_show_advanced: bool) -> Vec<StreamProxyMode> {
        Self::ALL.to_vec()
    }

    /// 저장된 값 정규화(현재는 항등).
    pub fn sanitized(raw: StreamProxyMode) -> StreamProxyMode {
        raw
    }

    /// `chzzk_target_url` 이 false 면 어떤 모드든 의미 없음 → `None`.
    pub fn resolved(self, chzzk_target_url: bool) -> StreamProxyMode {
        if chzzk_target_url {
            self
        } else {
            StreamProxyMode::None
        }
    }
}

/// 프록시 경로 결정 사유.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamProxyRouteReason {
    UserSelected,
    AuthenticationRequired,
    UnsupportedHost,
    ProxyStartFailed,
    AuthenticationUnavailable,
}

/// 실제 적용된 프록시 경로 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamProxyRouteStatus {
    pub requested: StreamProxyMode,
    pub effective: Option<StreamProxyMode>,
    pub reason: StreamProxyRouteReason,
    pub pending: bool,
}

impl StreamProxyRouteStatus {
    pub fn new(requested: StreamProxyMode, effective: Option<StreamProxyMode>, reason: StreamProxyRouteReason) -> Self {
        Self { requested, effective, reason, pending: false }
    }

    pub fn resolve(
        requested: StreamProxyMode,
        requires_authentication: bool,
        supported_host: bool,
        proxy_started: bool,
    ) -> Self {
        use StreamProxyRouteReason as R;
        if requires_authentication {
            let ok = supported_host && proxy_started;
            return Self::new(
                requested,
                ok.then_some(StreamProxyMode::LocalProxy),
                if ok { R::AuthenticationRequired } else { R::AuthenticationUnavailable },
            );
        }
        if !supported_host {
            return Self::new(requested, Some(StreamProxyMode::None), R::UnsupportedHost);
        }
        if requested == StreamProxyMode::LocalProxy && !proxy_started {
            return Self::new(requested, Some(StreamProxyMode::None), R::ProxyStartFailed);
        }
        Self::new(requested, Some(requested), R::UserSelected)
    }

    pub fn summary(&self) -> String {
        let route = match self.effective {
            Some(StreamProxyMode::LocalProxy) => "로컬 프록시",
            Some(StreamProxyMode::None) => "직접 재생",
            None => "연결 불가",
        };
        let detail = match self.reason {
            StreamProxyRouteReason::UserSelected => "선택한 설정 적용",
            StreamProxyRouteReason::AuthenticationRequired => "암호화 방송 인증에 필요",
            StreamProxyRouteReason::UnsupportedHost => "보정 대상 외 스트림",
            StreamProxyRouteReason::ProxyStartFailed => "프록시 시작 실패",
            StreamProxyRouteReason::AuthenticationUnavailable => "인증용 프록시 연결 실패",
        };
        let pending = if self.pending { " · 라이브 재생 시 변경 적용" } else { "" };
        format!("실제 적용: {route} · {detail}{pending}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_raw_value_falls_back_to_local_proxy() {
        let m: StreamProxyMode = serde_json::from_str("\"directVLCAdaptive\"").unwrap();
        assert_eq!(m, StreamProxyMode::LocalProxy);
        let n: StreamProxyMode = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(n, StreamProxyMode::None);
        assert!(serde_json::from_str::<StreamProxyMode>("3").is_err());
        assert_eq!(serde_json::to_string(&StreamProxyMode::LocalProxy).unwrap(), "\"localProxy\"");
    }

    #[test]
    fn resolved_and_route() {
        assert_eq!(StreamProxyMode::LocalProxy.resolved(false), StreamProxyMode::None);
        let auth = StreamProxyRouteStatus::resolve(StreamProxyMode::None, true, true, true);
        assert_eq!(auth.effective, Some(StreamProxyMode::LocalProxy));
        assert_eq!(auth.summary(), "실제 적용: 로컬 프록시 · 암호화 방송 인증에 필요");
        let fail = StreamProxyRouteStatus::resolve(StreamProxyMode::None, true, false, true);
        assert_eq!(fail.effective, None);
        assert_eq!(fail.reason, StreamProxyRouteReason::AuthenticationUnavailable);
        let start = StreamProxyRouteStatus::resolve(StreamProxyMode::LocalProxy, false, true, false);
        assert_eq!(start.reason, StreamProxyRouteReason::ProxyStartFailed);
        let mut user = StreamProxyRouteStatus::resolve(StreamProxyMode::None, false, true, false);
        user.pending = true;
        assert_eq!(user.summary(), "실제 적용: 직접 재생 · 선택한 설정 적용 · 라이브 재생 시 변경 적용");
    }
}
