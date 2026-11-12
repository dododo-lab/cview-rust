//! 채팅 아바타 — 시청자(userIdHash = 채널 ID)의 채널 사진 주소를 찾아 준다.
//!
//! 치지직 채팅 메시지의 `profile.profileImageUrl` 은 늘 빈 문자열로 온다(2026-11-12 인기 라이브 4곳 242건 실측).
//! 그래서 화면은 먼저 사용자 ID 로 정해지는 랜덤 아이콘을 그리고, 프런트(`chat-avatar.js`)가 천천히(초당 몇 건)
//! 이 명령으로 사진을 물어 찾으면 덮는다. 같은 사람은 앱이 켜져 있는 동안 다시 묻지 않게 여기서 기억한다
//! (사진 없음도 기억 — 사진이 없는 시청자가 더 많다). 네트워크 실패는 기억하지 않아 다음에 다시 묻는다.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use cview_core::error::ApiError;
use tauri::State;

use super::CmdResult;
use crate::state::AppServices;

/// 한 번에 물을 수 있는 사용자 수 — 프런트는 더 작게 나눠 보낸다.
pub const MAX_BATCH: usize = 8;
/// 기억할 사용자 수(오래된 것부터 잊는다). 한 항목은 ID(≈32자) + 주소(≈100자).
pub const CACHE_CAP: usize = 6000;

/// 사용자 ID 로 쓸 수 있는가 — 영숫자 해시만(경로를 바꿀 글자가 섞이면 보내지 않는다).
pub fn is_plain_user_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 사용자 → 사진 주소(`None` = 사진 없음) 기억. 넣은 순서로 오래된 것부터 잊는다.
#[derive(Debug, Default)]
pub struct AvatarCache {
    map: HashMap<String, Option<String>>,
    order: VecDeque<String>,
    cap: usize,
}

impl AvatarCache {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    /// 아는 사용자면 `Some(결과)`, 모르면 `None`.
    pub fn get(&self, user_id: &str) -> Option<Option<String>> {
        self.map.get(user_id).cloned()
    }

    pub fn put(&mut self, user_id: &str, url: Option<String>) {
        if self.map.insert(user_id.to_string(), url).is_none() {
            self.order.push_back(user_id.to_string());
        }
        while self.map.len() > self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn cache() -> &'static Mutex<AvatarCache> {
    static CACHE: OnceLock<Mutex<AvatarCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(AvatarCache::new(CACHE_CAP)))
}

/// 응답을 기억할 결과로 — 받았으면 사진 주소(없으면 `None`), 채널이 없다는 응답(404·빈 내용)도 "사진 없음".
/// 그 밖의 실패(네트워크·429·5xx)는 `None` — 기억하지 않는다.
pub fn outcome(result: Result<Option<String>, ApiError>) -> Option<Option<String>> {
    match result {
        Ok(url) => Some(url),
        Err(ApiError::EmptyContent) => Some(None),
        Err(ApiError::HttpError { status_code: 404 }) => Some(None),
        Err(_) => None,
    }
}

/// 사용자 ID 들의 채널 사진 주소 — 입력 순서대로, 모르거나 없으면 `null`.
/// 잘못된 ID·`MAX_BATCH` 를 넘는 몫은 묻지 않고 `null`.
#[tauri::command]
pub async fn chat_avatars(
    services: State<'_, AppServices>,
    user_ids: Vec<String>,
) -> CmdResult<Vec<Option<String>>> {
    let ids: Vec<String> = user_ids.into_iter().map(|u| u.trim().to_string()).collect();
    let mut out: Vec<Option<String>> = vec![None; ids.len()];
    let mut ask: Vec<usize> = Vec::new();
    {
        let c = cache().lock().unwrap_or_else(|p| p.into_inner());
        for (i, id) in ids.iter().enumerate() {
            match c.get(id) {
                Some(hit) => out[i] = hit,
                None if is_plain_user_id(id) && ask.len() < MAX_BATCH => ask.push(i),
                None => {}
            }
        }
    }
    let api = &services.api;
    let fetched = futures_util::future::join_all(ask.iter().map(|&i| {
        let id = ids[i].clone();
        async move {
            let r = api
                .channel_avatar(&id)
                .await
                .map(|a| a.channel_image_url.map(|u| u.to_string()));
            (i, outcome(r))
        }
    }))
    .await;
    let mut c = cache().lock().unwrap_or_else(|p| p.into_inner());
    for (i, r) in fetched {
        if let Some(url) = r {
            c.put(&ids[i], url.clone());
            out[i] = url;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ids_only() {
        assert!(is_plain_user_id("0123abcdEF"));
        assert!(is_plain_user_id("a_b-c"));
        assert!(!is_plain_user_id(""));
        assert!(!is_plain_user_id("../x"));
        assert!(!is_plain_user_id("a/b"));
        assert!(!is_plain_user_id("a?b"));
        assert!(!is_plain_user_id(&"a".repeat(65)));
    }

    #[test]
    fn cache_remembers_misses_and_forgets_oldest() {
        let mut c = AvatarCache::new(2);
        assert_eq!(c.get("a"), None);
        c.put("a", Some("https://x/a.png".into()));
        c.put("b", None);
        assert_eq!(c.get("a"), Some(Some("https://x/a.png".into())));
        assert_eq!(c.get("b"), Some(None), "사진 없음도 기억한다");
        // 같은 키를 다시 넣어도 순서 목록이 늘지 않는다
        c.put("b", None);
        c.put("c", None);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get("a"), None, "가장 오래된 것부터 잊는다");
        assert!(c.get("c").is_some());
    }

    #[test]
    fn outcome_caches_only_definite_answers() {
        assert_eq!(outcome(Ok(Some("u".into()))), Some(Some("u".into())));
        assert_eq!(outcome(Ok(None)), Some(None));
        assert_eq!(outcome(Err(ApiError::EmptyContent)), Some(None));
        assert_eq!(outcome(Err(ApiError::http(404))), Some(None));
        assert_eq!(outcome(Err(ApiError::http(429))), None);
        assert_eq!(outcome(Err(ApiError::http(500))), None);
    }
}
