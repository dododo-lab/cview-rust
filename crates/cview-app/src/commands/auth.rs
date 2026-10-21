//! 인증 명령 — 상태 조회 / 로그인 창 열기 / 로그아웃.

use tauri::{AppHandle, Emitter, Runtime, State};

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
    match services.api.user_status().await {
        Ok(user) if user.logged_in == Some(false) => {
            if services.auth.expire_cookie_session(generation) {
                return AuthStatusDto::from_state(&services.auth.auth_state());
            }
            status
        }
        Ok(user) => status.with_profile(user.nickname, user.profile_image_url),
        Err(e) => {
            tracing::debug!("사용자 상태 조회 실패: {e}");
            status
        }
    }
}

/// 상태를 프런트에 알린다.
pub async fn broadcast_status<R: Runtime>(app: &AppHandle<R>, services: &AppServices) {
    let status = current_status(services).await;
    let _ = app.emit(events::AUTH_CHANGED, status);
}

#[tauri::command]
pub async fn auth_status(services: State<'_, AppServices>) -> CmdResult<AuthStatusDto> {
    Ok(current_status(&services).await)
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
