//! '로컬 데이터 삭제' 가 사용자에게 말하는 내용의 단일 출처.
//!
//! 원본: `Sources/CViewCore/Utilities/LocalDataDeletionPolicy.swift`
//!
//! 확인 문구가 실제로 일어나는 일(시청 기록 동기화 자동 해제, 로그인 표시는 남음)을 말하도록
//! 한 곳에 모은다. ⚠ 문구를 호출부에서 다시 적지 말 것.

pub const CONFIRM_TITLE: &str = "로컬 데이터를 삭제할까요?";

pub const CONFIRM_MESSAGE: &str = "시청 기록·즐겨찾기·웹 세션(쿠키/localStorage)이 삭제됩니다. 저장소 백업·채널 상세 사본도 삭제됩니다.\n\
시청 기록 동기화는 자동으로 꺼집니다. 앱은 로그인 상태로 보이지만 웹 세션이 사라져 다시 로그인해야 할 수 있습니다.\n\
서버에 이미 전송된 동기화 데이터는 보존 기간(약 90일)이 지나면 만료됩니다.";

pub const COMPLETION_TITLE: &str = "데이터 삭제 완료";

pub const COMPLETION_MESSAGE: &str = "로컬 시청 기록·즐겨찾기·웹 세션과 저장소 백업·채널 상세 사본이 삭제되고 시청 기록 동기화가 꺼졌습니다.\n\
팔로잉·채팅이 동작하지 않으면 다시 로그인하세요.";

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/UIUXAudit0920GuardTests.swift` (D32 문구 검사)
    use super::*;

    #[test]
    fn messages_mention_sync_and_login() {
        assert!(CONFIRM_MESSAGE.contains("동기화"));
        assert!(CONFIRM_MESSAGE.contains("로그인"));
        assert!(COMPLETION_MESSAGE.contains("동기화"));
        assert!(COMPLETION_MESSAGE.contains("로그인"));
        assert_eq!(CONFIRM_MESSAGE.lines().count(), 3);
        assert_eq!(COMPLETION_MESSAGE.lines().count(), 2);
        assert!(
            CONFIRM_MESSAGE.contains("사라져 다시 로그인"),
            "Swift 의 줄 이음(\\\\)은 공백 없이 잇는다"
        );
    }
}
