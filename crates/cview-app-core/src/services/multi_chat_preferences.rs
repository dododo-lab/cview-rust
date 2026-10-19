//! `Sources/CViewApp/Services/MultiChatPreferences.swift` 포팅.
//!
//! 멀티채팅 단일 진실 저장소(키-값 저장소). 미읽음·멘션 사전과 멘션 규칙은 메모리에 캐시하고
//! 변경 시 즉시 영속화한다. 채널별 사전은 200개 상한 — 넘으면 방금 쓴 키를 뺀 나머지에서
//! 가장 오래 안 쓴 키부터 덜어 낸다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use cview_core::logic::mention_detector::MentionDetectionRule;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use super::support::{lock, KeyValueStore, MemoryKeyValueStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfflineSessionPolicy {
    AutoRemove,
    KeepWithIndicator,
    Reconnect,
}

impl OfflineSessionPolicy {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::AutoRemove => "autoRemove",
            Self::KeepWithIndicator => "keepWithIndicator",
            Self::Reconnect => "reconnect",
        }
    }
    pub fn from_raw(raw: &str) -> Option<Self> {
        [Self::AutoRemove, Self::KeepWithIndicator, Self::Reconnect]
            .into_iter()
            .find(|p| p.raw_value() == raw)
    }
}

/// 멀티채팅 워크스페이스 스코프 — 라이브 메뉴 인라인(`Live`)과 사이드바 "멀티채팅"(`Standalone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiChatScope {
    #[default]
    Live,
    Standalone,
}

impl MultiChatScope {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Standalone => "standalone",
        }
    }

    /// 화면이 이 워크스페이스를 부르는 이름. 두 이름은 서로 달라야 한다.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Live => "라이브 채팅 도크",
            Self::Standalone => "멀티채팅 작업영역",
        }
    }

    pub fn role_description(self) -> &'static str {
        match self {
            Self::Live => "라이브 메뉴 옆에 붙는 채팅 도크입니다. 시청 중인 방송과 함께 봅니다.",
            Self::Standalone => {
                "라이브 재생과 분리된 채팅 전용 작업영역입니다. 방송을 켜지 않아도 유지됩니다."
            }
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Live => "sidebar.right",
            Self::Standalone => "bubble.left.and.bubble.right",
        }
    }

    /// 통합 타임라인 토글의 키. `Live` 는 기존 키를 그대로 쓴다.
    pub fn merged_chat_defaults_key(self) -> &'static str {
        match self {
            Self::Live => "MultiChatView.showMergedChat",
            Self::Standalone => "MultiChatView.showMergedChat.standalone",
        }
    }
}

const KEY_PREFIX: &str = "multichat.v1.";
const STANDALONE_PREFIX: &str = "standalone|";

fn key(name: &str) -> String {
    format!("{KEY_PREFIX}{name}")
}

#[derive(Default)]
struct Cache {
    unread_counts: Option<HashMap<String, i64>>,
    unread_mentions: Option<HashMap<String, i64>>,
    touched_at: Option<HashMap<String, f64>>,
    muted_channels: Option<HashSet<String>>,
    user_keywords: Option<Vec<String>>,
    mention_rule: Option<(Option<String>, MentionDetectionRule)>,
}

pub struct MultiChatPreferences {
    defaults: Arc<dyn KeyValueStore>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    cache: Mutex<Cache>,
}

impl MultiChatPreferences {
    /// 채널별 읽음 위치·미읽음 사전의 상한.
    pub const PER_CHANNEL_MAP_LIMIT: usize = 200;

    pub fn new(defaults: Arc<dyn KeyValueStore>, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> Self {
        Self {
            defaults,
            now,
            cache: Mutex::new(Cache::default()),
        }
    }

    /// 앱 전역 인스턴스. 처음 부르기 전에 [`MultiChatPreferences::install_shared`] 로 실제 저장소를 넣는다.
    pub fn shared() -> Arc<MultiChatPreferences> {
        Self::slot()
            .get_or_init(|| {
                Arc::new(Self::new(
                    MemoryKeyValueStore::shared(),
                    Arc::new(|| chrono::Utc::now().timestamp_millis() as f64 / 1000.0),
                ))
            })
            .clone()
    }

    /// 전역 인스턴스를 한 번 설치한다. 이미 있으면 `false`.
    pub fn install_shared(prefs: Arc<MultiChatPreferences>) -> bool {
        Self::slot().set(prefs).is_ok()
    }

    fn slot() -> &'static OnceLock<Arc<MultiChatPreferences>> {
        static SHARED: OnceLock<Arc<MultiChatPreferences>> = OnceLock::new();
        &SHARED
    }

    fn read_map<V: DeserializeOwned>(&self, key: &str) -> HashMap<String, V> {
        self.defaults
            .value(key)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn write<V: Serialize>(&self, key: &str, value: &V) {
        if let Ok(v) = serde_json::to_value(value) {
            self.defaults.set_value(key, v);
        }
    }

    // MARK: 채널 음소거

    pub fn is_channel_muted(&self, chat_channel_id: &str) -> bool {
        self.muted_channels().contains(chat_channel_id)
    }

    pub fn set_channel_muted(&self, chat_channel_id: &str, muted: bool) {
        let k = key("channelMute");
        let mut map: HashMap<String, bool> = self.read_map(&k);
        if muted {
            map.insert(chat_channel_id.to_string(), true);
        } else {
            map.remove(chat_channel_id);
        }
        self.write(&k, &map);
        let mut cache = lock(&self.cache);
        cache.muted_channels = Some(
            map.into_iter()
                .filter(|(_, v)| *v)
                .map(|(k, _)| k)
                .collect(),
        );
        cache.mention_rule = None;
    }

    pub fn muted_channels(&self) -> HashSet<String> {
        if let Some(c) = &lock(&self.cache).muted_channels {
            return c.clone();
        }
        let map: HashMap<String, bool> = self.read_map(&key("channelMute"));
        let value: HashSet<String> = map
            .into_iter()
            .filter(|(_, v)| *v)
            .map(|(k, _)| k)
            .collect();
        lock(&self.cache).muted_channels = Some(value.clone());
        value
    }

    // MARK: 사용자 키워드

    pub fn user_keywords(&self) -> Vec<String> {
        if let Some(c) = &lock(&self.cache).user_keywords {
            return c.clone();
        }
        let value = self
            .defaults
            .strings(&key("userKeywords"))
            .unwrap_or_default();
        lock(&self.cache).user_keywords = Some(value.clone());
        value
    }

    pub fn add_keyword(&self, keyword: &str) {
        let trimmed = keyword.trim_matches(' ');
        if trimmed.is_empty() {
            return;
        }
        let mut arr = self.user_keywords();
        if arr.iter().any(|k| k == trimmed) {
            return;
        }
        arr.push(trimmed.to_string());
        self.store_keywords(arr);
    }

    pub fn remove_keyword(&self, keyword: &str) {
        let mut arr = self.user_keywords();
        arr.retain(|k| k != keyword);
        self.store_keywords(arr);
    }

    fn store_keywords(&self, arr: Vec<String>) {
        self.defaults.set_strings(&key("userKeywords"), &arr);
        let mut cache = lock(&self.cache);
        cache.user_keywords = Some(arr);
        cache.mention_rule = None;
    }

    // MARK: 스코프 키

    fn scoped_key(chat_channel_id: &str, scope: MultiChatScope) -> String {
        match scope {
            MultiChatScope::Live => chat_channel_id.to_string(),
            MultiChatScope::Standalone => format!("{STANDALONE_PREFIX}{chat_channel_id}"),
        }
    }

    /// 상한을 넘으면 `keeping` 을 뺀 나머지에서 (시각, 키) 오름차순으로 덜어 낸다.
    pub fn capped<V: Clone>(
        map: &HashMap<String, V>,
        keeping: &str,
        touched_at: &HashMap<String, f64>,
    ) -> HashMap<String, V> {
        if map.len() <= Self::PER_CHANNEL_MAP_LIMIT {
            return map.clone();
        }
        let mut out = map.clone();
        let mut doomed: Vec<&String> = map.keys().filter(|k| k.as_str() != keeping).collect();
        doomed.sort_by(|a, b| {
            let (l, r) = (
                touched_at.get(*a).copied().unwrap_or(0.0),
                touched_at.get(*b).copied().unwrap_or(0.0),
            );
            l.total_cmp(&r).then_with(|| a.cmp(b))
        });
        for k in doomed
            .into_iter()
            .take(map.len() - Self::PER_CHANNEL_MAP_LIMIT)
        {
            out.remove(k);
        }
        out
    }

    fn touched_at_map(&self) -> HashMap<String, f64> {
        if let Some(c) = &lock(&self.cache).touched_at {
            return c.clone();
        }
        let map: HashMap<String, f64> = self.read_map(&key("channelTouchedAt"));
        lock(&self.cache).touched_at = Some(map.clone());
        map
    }

    fn touched_at_including(&self, k: &str) -> HashMap<String, f64> {
        let mut touched = self.touched_at_map();
        touched.insert(k.to_string(), (self.now)());
        touched
    }

    fn commit_touched(
        &self,
        touched: HashMap<String, f64>,
        last_read: Option<&HashMap<String, String>>,
    ) {
        let mut out = touched;
        if out.len() > Self::PER_CHANNEL_MAP_LIMIT {
            let read = last_read
                .cloned()
                .unwrap_or_else(|| self.read_map(&key("lastReadMessageId")));
            let mut alive: HashSet<String> = read.into_keys().collect();
            alive.extend(self.unread_counts_map().into_keys());
            alive.extend(self.unread_mentions_map().into_keys());
            out.retain(|k, _| alive.contains(k));
        }
        self.write(&key("channelTouchedAt"), &out);
        lock(&self.cache).touched_at = Some(out);
    }

    fn unread_counts_map(&self) -> HashMap<String, i64> {
        if let Some(c) = &lock(&self.cache).unread_counts {
            return c.clone();
        }
        let map: HashMap<String, i64> = self.read_map(&key("unreadCounts"));
        lock(&self.cache).unread_counts = Some(map.clone());
        map
    }

    fn unread_mentions_map(&self) -> HashMap<String, i64> {
        if let Some(c) = &lock(&self.cache).unread_mentions {
            return c.clone();
        }
        let map: HashMap<String, i64> = self.read_map(&key("unreadMentions"));
        lock(&self.cache).unread_mentions = Some(map.clone());
        map
    }

    fn persist_unread_counts(&self, map: HashMap<String, i64>) {
        self.write(&key("unreadCounts"), &map);
        lock(&self.cache).unread_counts = Some(map);
    }

    fn persist_unread_mentions(&self, map: HashMap<String, i64>) {
        self.write(&key("unreadMentions"), &map);
        lock(&self.cache).unread_mentions = Some(map);
    }

    // MARK: 미읽음

    pub fn unread_count(&self, chat_channel_id: &str, scope: MultiChatScope) -> i64 {
        self.unread_counts_map()
            .get(&Self::scoped_key(chat_channel_id, scope))
            .copied()
            .unwrap_or(0)
    }

    /// 스코프의 채널별 미읽음(키 = bare channelId).
    pub fn all_unread_counts(&self, scope: MultiChatScope) -> HashMap<String, i64> {
        Self::unscoped_map(&self.unread_counts_map(), scope)
    }

    pub fn all_unread_mention_counts(&self, scope: MultiChatScope) -> HashMap<String, i64> {
        Self::unscoped_map(&self.unread_mentions_map(), scope)
    }

    fn unscoped_map(map: &HashMap<String, i64>, scope: MultiChatScope) -> HashMap<String, i64> {
        match scope {
            MultiChatScope::Standalone => map
                .iter()
                .filter_map(|(k, v)| {
                    k.strip_prefix(STANDALONE_PREFIX)
                        .map(|b| (b.to_string(), *v))
                })
                .collect(),
            MultiChatScope::Live => map
                .iter()
                .filter(|(k, _)| !k.starts_with(STANDALONE_PREFIX))
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    }

    pub fn increment_unread(&self, chat_channel_id: &str, count: i64, scope: MultiChatScope) {
        let mut map = self.unread_counts_map();
        let k = Self::scoped_key(chat_channel_id, scope);
        *map.entry(k.clone()).or_insert(0) += count;
        let touched = self.touched_at_including(&k);
        self.persist_unread_counts(Self::capped(&map, &k, &touched));
        self.commit_touched(touched, None);
    }

    pub fn unread_mention_count(&self, chat_channel_id: &str, scope: MultiChatScope) -> i64 {
        self.unread_mentions_map()
            .get(&Self::scoped_key(chat_channel_id, scope))
            .copied()
            .unwrap_or(0)
    }

    pub fn increment_mention_unread(&self, chat_channel_id: &str, scope: MultiChatScope) {
        let mut map = self.unread_mentions_map();
        let k = Self::scoped_key(chat_channel_id, scope);
        *map.entry(k.clone()).or_insert(0) += 1;
        let touched = self.touched_at_including(&k);
        self.persist_unread_mentions(Self::capped(&map, &k, &touched));
        self.commit_touched(touched, None);
    }

    /// 지울 값이 있을 때만 쓴다.
    pub fn reset_unread(&self, chat_channel_id: &str, scope: MultiChatScope) {
        let k = Self::scoped_key(chat_channel_id, scope);
        let mut u = self.unread_counts_map();
        if u.remove(&k).is_some() {
            self.persist_unread_counts(u);
        }
        let mut m = self.unread_mentions_map();
        if m.remove(&k).is_some() {
            self.persist_unread_mentions(m);
        }
    }

    // MARK: 읽음 위치

    pub fn last_read_message_id(
        &self,
        chat_channel_id: &str,
        scope: MultiChatScope,
    ) -> Option<String> {
        let map: HashMap<String, String> = self.read_map(&key("lastReadMessageId"));
        map.get(&Self::scoped_key(chat_channel_id, scope)).cloned()
    }

    pub fn set_last_read_message_id(
        &self,
        chat_channel_id: &str,
        message_id: &str,
        scope: MultiChatScope,
    ) {
        let lk = key("lastReadMessageId");
        let mut map: HashMap<String, String> = self.read_map(&lk);
        let k = Self::scoped_key(chat_channel_id, scope);
        if map.get(&k).map(String::as_str) == Some(message_id) {
            return;
        }
        map.insert(k.clone(), message_id.to_string());
        let touched = self.touched_at_including(&k);
        let kept = Self::capped(&map, &k, &touched);
        self.write(&lk, &kept);
        self.commit_touched(touched, Some(&kept));
    }

    // MARK: 오프라인 정책

    pub fn global_offline_policy(&self) -> OfflineSessionPolicy {
        self.defaults
            .string(&key("offlineSessionPolicy"))
            .and_then(|r| OfflineSessionPolicy::from_raw(&r))
            .unwrap_or(OfflineSessionPolicy::KeepWithIndicator)
    }

    pub fn set_global_offline_policy(&self, policy: OfflineSessionPolicy) {
        self.defaults
            .set_string(&key("offlineSessionPolicy"), policy.raw_value());
    }

    pub fn channel_offline_policy(&self, chat_channel_id: &str) -> Option<OfflineSessionPolicy> {
        let map: HashMap<String, String> = self.read_map(&key("channelOfflinePolicy"));
        map.get(chat_channel_id)
            .and_then(|r| OfflineSessionPolicy::from_raw(r))
    }

    // MARK: 알림 토글

    pub fn mention_notification_center_enabled(&self) -> bool {
        match self.defaults.value(&key("mentionNotificationCenter")) {
            Some(Value::Bool(b)) => b,
            _ => true,
        }
    }

    pub fn set_mention_notification_center_enabled(&self, on: bool) {
        self.defaults
            .set_bool(&key("mentionNotificationCenter"), on);
    }

    pub fn mention_sound_enabled(&self) -> bool {
        match self.defaults.value(&key("mentionSoundEnabled")) {
            Some(Value::Bool(b)) => b,
            _ => false,
        }
    }

    pub fn set_mention_sound_enabled(&self, on: bool) {
        self.defaults.set_bool(&key("mentionSoundEnabled"), on);
    }

    // MARK: 규칙

    pub fn current_mention_rule(&self, nickname: Option<&str>) -> MentionDetectionRule {
        if let Some((nick, rule)) = &lock(&self.cache).mention_rule {
            if nick.as_deref() == nickname {
                return rule.clone();
            }
        }
        let rule = MentionDetectionRule::new(
            nickname.map(str::to_string),
            self.user_keywords(),
            self.muted_channels(),
        );
        lock(&self.cache).mention_rule = Some((nickname.map(str::to_string), rule.clone()));
        rule
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MultiChatPreferencesTests.swift` 계열 핵심 계약.
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn prefs() -> (MultiChatPreferences, Arc<dyn KeyValueStore>, Arc<AtomicU64>) {
        let kv = MemoryKeyValueStore::shared();
        let t = Arc::new(AtomicU64::new(1));
        let tick = t.clone();
        let p = MultiChatPreferences::new(
            kv.clone(),
            Arc::new(move || tick.fetch_add(1, Ordering::SeqCst) as f64),
        );
        (p, kv, t)
    }

    #[test]
    fn scopes_are_independent() {
        let (p, _, _) = prefs();
        p.increment_unread("c", 2, MultiChatScope::Live);
        p.increment_unread("c", 5, MultiChatScope::Standalone);
        p.increment_mention_unread("c", MultiChatScope::Standalone);
        assert_eq!(p.unread_count("c", MultiChatScope::Live), 2);
        assert_eq!(p.unread_count("c", MultiChatScope::Standalone), 5);
        assert_eq!(p.all_unread_counts(MultiChatScope::Standalone)["c"], 5);
        assert_eq!(p.all_unread_counts(MultiChatScope::Live).len(), 1);
        p.reset_unread("c", MultiChatScope::Standalone);
        assert_eq!(p.unread_mention_count("c", MultiChatScope::Standalone), 0);
        assert_eq!(p.unread_count("c", MultiChatScope::Live), 2);
        assert_ne!(
            MultiChatScope::Live.display_name(),
            MultiChatScope::Standalone.display_name()
        );
    }

    #[test]
    fn cap_evicts_least_recently_touched() {
        let mut map = HashMap::new();
        let mut touched = HashMap::new();
        for i in 0..=MultiChatPreferences::PER_CHANNEL_MAP_LIMIT {
            map.insert(format!("k{i:03}"), i as i64);
            touched.insert(format!("k{i:03}"), (1000 - i) as f64);
        }
        let out = MultiChatPreferences::capped(&map, "k000", &touched);
        assert_eq!(out.len(), MultiChatPreferences::PER_CHANNEL_MAP_LIMIT);
        assert!(out.contains_key("k000"));
        assert!(!out.contains_key(&format!(
            "k{:03}",
            MultiChatPreferences::PER_CHANNEL_MAP_LIMIT
        )));
    }

    #[test]
    fn keywords_mute_and_rule_cache() {
        let (p, kv, _) = prefs();
        p.add_keyword("  hi ");
        p.add_keyword("hi");
        p.add_keyword(" ");
        assert_eq!(p.user_keywords(), vec!["hi".to_string()]);
        p.set_channel_muted("x", true);
        assert!(p.is_channel_muted("x"));
        let _ = p.current_mention_rule(Some("nick"));
        p.remove_keyword("hi");
        assert!(p.user_keywords().is_empty());
        assert!(kv.strings("multichat.v1.userKeywords").unwrap().is_empty());
        assert!(p.mention_notification_center_enabled());
        assert!(!p.mention_sound_enabled());
        assert_eq!(
            p.global_offline_policy(),
            OfflineSessionPolicy::KeepWithIndicator
        );
        p.set_global_offline_policy(OfflineSessionPolicy::Reconnect);
        assert_eq!(p.global_offline_policy(), OfflineSessionPolicy::Reconnect);
    }

    #[test]
    fn last_read_skips_same_value() {
        let (p, kv, t) = prefs();
        p.set_last_read_message_id("c", "m1", MultiChatScope::Live);
        let before = t.load(Ordering::SeqCst);
        p.set_last_read_message_id("c", "m1", MultiChatScope::Live);
        assert_eq!(
            t.load(Ordering::SeqCst),
            before,
            "같은 값은 다시 쓰지 않는다"
        );
        assert_eq!(
            p.last_read_message_id("c", MultiChatScope::Live).as_deref(),
            Some("m1")
        );
        assert!(kv.contains("multichat.v1.channelTouchedAt"));
    }
}
