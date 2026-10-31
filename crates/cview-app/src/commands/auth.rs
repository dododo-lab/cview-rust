//! 인증 명령 — 상태 조회 / 로그인 창 열기 / 로그아웃.

use tauri::{AppHandle, Emitter, Runtime, State};

use cview_core::error::ApiError;
use cview_networking::response::UserStatusInfo;

use super::{events, CmdResult};
use crate::dto::AuthStatusDto;
use crate::state::AppServices;

/// 현재 인증 상태. 로그인 상태면 게임 API 사용자 상태에서 닉네임·프로필을 붙인다.
///
/// 쿠키 세션은 이 조회가 곧 서버 확인이다 — 서버가 분명히 로그아웃(`loggedIn=false`)이라고 답하면
/// (재부팅 사이 세션 만료·다른 기기에서 로그아웃 등) 로그인 만료로 바꾼다. 네트워크·서버 오류는
/// 알 수 없음이라 상태를 그대로 둔다(유효한 로그인을 잃지 않게).
pub async fn current_status(services: &AppServices) -> AuthStatusDto {
    let generation = services.auth.authentication_generation();
    let status = AuthStatusDto::from_state(&services.auth.auth_state());
    if !status.logged_in {
        return status;
    }
    let result = services.api.user_status().await;
    match session_check(&result) {
        SessionCheck::ServerLoggedOut => {
            if services.auth.expire_cookie_session(generation) {
                return AuthStatusDto::from_state(&services.auth.auth_state());
            }
            status
        }
        SessionCheck::Valid => match result {
            Ok(user) => status.with_profile(user.nickname, user.profile_image_url),
            Err(_) => status,
        },
        SessionCheck::Unknown => {
            if let Err(e) = &result {
                tracing::debug!("사용자 상태 조회 실패: {e}");
            }
            status
        }
    }
}

/// 사용자 상태 응답으로 본 세션 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCheck {
    /// 서버가 로그인으로 본다(또는 판정 값이 없다 — 로그인을 잃지 않는 쪽).
    Valid,
    /// 서버가 분명히 로그아웃이라고 답했다.
    ServerLoggedOut,
    /// 네트워크·서버 오류 — 알 수 없음, 상태를 바꾸지 않는다.
    Unknown,
}

pub(crate) fn session_check(result: &Result<UserStatusInfo, ApiError>) -> SessionCheck {
    match result {
        Ok(user) if user.logged_in == Some(false) => SessionCheck::ServerLoggedOut,
        Ok(_) => SessionCheck::Valid,
        Err(_) => SessionCheck::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(logged_in: Option<bool>) -> Result<UserStatusInfo, ApiError> {
        Ok(UserStatusInfo {
            has_profile: None,
            user_id_hash: None,
            nickname: None,
            profile_image_url: None,
            penalties: None,
            logged_in,
        })
    }

    #[test]
    fn only_a_definite_server_logout_expires_the_session() {
        assert_eq!(
            session_check(&user(Some(false))),
            SessionCheck::ServerLoggedOut
        );
        assert_eq!(session_check(&user(Some(true))), SessionCheck::Valid);
        // 판정 값이 없으면 로그인을 잃지 않는 쪽.
        assert_eq!(session_check(&user(None)), SessionCheck::Valid);
    }

    #[test]
    fn network_and_server_errors_are_unknown() {
        for e in [
            ApiError::NetworkError("timed out".into()),
            ApiError::RateLimited { retry_after: 1.0 },
            ApiError::http(500),
            ApiError::http(503),
            ApiError::Unauthorized,
            ApiError::Cancelled,
        ] {
            assert_eq!(session_check(&Err(e)), SessionCheck::Unknown);
        }
    }
}

/// 상태를 프런트에 알린다.
pub async fn broadcast_status<R: Runtime>(app: &AppHandle<R>, services: &AppServices) {
    let status = current_status(services).await;
    let _ = app.emit(events::AUTH_CHANGED, status);
}

/// 계정 상태. `quick` 이면 서버 확인(`/users/me`) 없이 저장된 로그인 복원이 끝난 상태만 바로 돌려준다.
///
/// 시작 화면은 `quick` 으로 로그인 여부만 알고 곧바로 홈 데이터를 부른다 — 예전엔 서버 확인 한 번을
/// 기다린 뒤에야 홈 요청을 시작했다. 닉네임·만료 판정은 뒤이은 일반 호출과 `auth://changed` 가 채운다.
#[tauri::command]
pub async fn auth_status(
    services: State<'_, AppServices>,
    quick: Option<bool>,
) -> CmdResult<AuthStatusDto> {
    if quick.unwrap_or(false) {
        wait_auth_initialized(&services).await;
        return Ok(AuthStatusDto::from_state(&services.auth.auth_state()));
    }
    Ok(current_status(&services).await)
}

/// 시작 때 키체인·웹뷰 쿠키 복원(`spawn_auth_tasks`)이 끝날 때까지 잠깐 기다린다 — 복원 전에 물으면
/// 로그인한 사용자도 "로그아웃"으로 보여 홈이 손님 화면을 먼저 그렸다. 키체인이 늦으면 3초에서 멈춘다.
async fn wait_auth_initialized(services: &AppServices) {
    const STEP: std::time::Duration = std::time::Duration::from_millis(15);
    const LIMIT: u32 = 200;
    for _ in 0..LIMIT {
        if services.app_state().is_auth_initialized {
            return;
        }
        tokio::time::sleep(STEP).await;
    }
}

/// 네이버 로그인 창을 연다. 결과는 `auth://changed` / `auth://login-failed` 로 온다.
#[tauri::command]
pub async fn auth_login(app: AppHandle, services: State<'_, AppServices>) -> CmdResult<()> {
    crate::login::open_login_window(&app, &services).map_err(Into::into)
}

#[tauri::command]
pub async fn auth_logout(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> CmdResult<AuthStatusDto> {
    // 채팅은 로그인 쿠키로 연결돼 있을 수 있다 — 끊는다.
    services.chat.disconnect().await;
    services.auth.logout().await;
    services.api.invalidate_authentication_cache().await;
    services.sync_authentication().await;
    let status = current_status(&services).await;
    let _ = app.emit(events::AUTH_CHANGED, status.clone());
    Ok(status)
}
