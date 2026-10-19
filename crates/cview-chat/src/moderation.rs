//! 채팅 모더레이션·슬래시 명령. 원본: `Sources/CViewChat/ChatModerationService.swift`.
//!
//! Swift `actor ChatModerationService` → 내부 `Mutex` 를 가진 구조체(메서드는 `&self`).
//! 서버 API 호출은 없다 — 서버로 보낼 명령은 [`ChatCommandResult::ServerCommand`] 로 돌려줄 뿐이다.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};

use crate::constants::chat_defaults;
use cview_core::models::chat::{ChatMessage, MessageType};

/// 지원하는 슬래시 명령.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatCommand {
    Mute,
    Unmute,
    Ban,
    Unban,
    Slow,
    Clear,
    Notice,
    Host,
    Filter,
    Export,
    Block,
    Unblock,
    Help,
}

impl ChatCommand {
    /// Swift `CaseIterable.allCases` (선언 순서).
    pub const ALL: [ChatCommand; 13] = [
        Self::Mute,
        Self::Unmute,
        Self::Ban,
        Self::Unban,
        Self::Slow,
        Self::Clear,
        Self::Notice,
        Self::Host,
        Self::Filter,
        Self::Export,
        Self::Block,
        Self::Unblock,
        Self::Help,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Mute => "/mute",
            Self::Unmute => "/unmute",
            Self::Ban => "/ban",
            Self::Unban => "/unban",
            Self::Slow => "/slow",
            Self::Clear => "/clear",
            Self::Notice => "/notice",
            Self::Host => "/host",
            Self::Filter => "/filter",
            Self::Export => "/export",
            Self::Block => "/block",
            Self::Unblock => "/unblock",
            Self::Help => "/help",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.raw_value() == raw)
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Mute => "사용자 채팅 금지",
            Self::Unmute => "사용자 채팅 금지 해제",
            Self::Ban => "사용자 차단",
            Self::Unban => "사용자 차단 해제",
            Self::Slow => "슬로우 모드 설정",
            Self::Clear => "채팅 내역 삭제",
            Self::Notice => "공지 등록",
            Self::Host => "호스트 설정",
            Self::Filter => "키워드 필터 추가",
            Self::Export => "채팅 로그 내보내기",
            Self::Block => "사용자 차단 (로컬)",
            Self::Unblock => "사용자 차단 해제 (로컬)",
            Self::Help => "명령어 도움말",
        }
    }

    /// 사용자 입력에서 명령과 인자를 파싱한다. 첫 공백 뒤는 공백으로 나눈 인자 목록.
    pub fn parse(input: &str) -> Option<(Self, Vec<String>)> {
        let trimmed = input.trim_matches(|c: char| c == ' ' || c == '\t');
        if !trimmed.starts_with('/') {
            return None;
        }
        let mut parts = trimmed.splitn(2, ' ');
        let cmd = Self::from_raw(parts.next()?)?;
        let args = parts
            .next()
            .map(|rest| rest.split(' ').filter(|s| !s.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        Some((cmd, args))
    }
}

/// 필터 종류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterType {
    Keyword(Vec<String>),
    Regex(String),
    User(Vec<String>),
    DonationOnly,
}

/// 메시지 콘텐츠 필터.
#[derive(Debug, Clone)]
pub struct ChatFilter {
    pub r#type: FilterType,
    pub is_enabled: bool,
    compiled_regex: Option<Regex>,
    lowered_keywords: Vec<String>,
    user_id_set: HashSet<String>,
}

impl ChatFilter {
    pub fn new(r#type: FilterType, is_enabled: bool) -> Self {
        let compiled_regex = match &r#type {
            FilterType::Regex(pattern) => match RegexBuilder::new(pattern).case_insensitive(true).build() {
                Ok(r) => Some(r),
                Err(error) => {
                    tracing::warn!(pattern, %error, "Chat filter regex compile failed");
                    None
                }
            },
            _ => None,
        };
        let lowered_keywords = match &r#type {
            FilterType::Keyword(words) => words.iter().map(|w| w.to_lowercase()).filter(|w| !w.is_empty()).collect(),
            _ => Vec::new(),
        };
        let user_id_set = match &r#type {
            FilterType::User(ids) => ids.iter().cloned().collect(),
            _ => HashSet::new(),
        };
        Self { r#type, is_enabled, compiled_regex, lowered_keywords, user_id_set }
    }

    /// 메시지를 숨겨야 하는가.
    pub fn should_filter(&self, message: &ChatMessage) -> bool {
        if !self.is_enabled {
            return false;
        }
        match &self.r#type {
            FilterType::Keyword(_) => {
                if self.lowered_keywords.is_empty() {
                    return false;
                }
                let lc = message.content.to_lowercase();
                self.lowered_keywords.iter().any(|k| lc.contains(k.as_str()))
            }
            FilterType::Regex(_) => self.compiled_regex.as_ref().is_some_and(|r| r.is_match(&message.content)),
            FilterType::User(_) => message.user_id.as_ref().is_some_and(|id| self.user_id_set.contains(id)),
            FilterType::DonationOnly => message.r#type != MessageType::Donation,
        }
    }
}

/// 명령 처리 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommandResult {
    LocalAction(String),
    ServerCommand { command: ChatCommand, args: Vec<String> },
    ClearChat,
    ExportChat,
    Error(String),
}

#[derive(Debug, Default)]
struct ModerationState {
    filters: Vec<ChatFilter>,
    keyword_filter: Option<ChatFilter>,
    blocked_users: HashSet<String>,
    /// userId → 만료 시각 (이미 지난 시각일 수 있다).
    muted_users: HashMap<String, MuteExpiry>,
}

#[derive(Debug, Clone, Copy)]
enum MuteExpiry {
    At(Instant),
    /// 음수 duration — 이미 만료.
    Past,
}

impl MuteExpiry {
    fn from_duration(duration: f64) -> Self {
        if duration.is_finite() && duration >= 0.0 {
            match Instant::now().checked_add(Duration::from_secs_f64(duration.min(1e12))) {
                Some(t) => Self::At(t),
                None => Self::At(Instant::now() + Duration::from_secs(u32::MAX as u64)),
            }
        } else if duration.is_infinite() && duration > 0.0 {
            Self::At(Instant::now() + Duration::from_secs(u32::MAX as u64))
        } else {
            Self::Past
        }
    }

    /// `now <= expiry`.
    fn active(self, now: Instant) -> bool {
        match self {
            Self::At(t) => now <= t,
            Self::Past => false,
        }
    }
}

/// 필터·차단·뮤트·명령 처리.
#[derive(Debug, Default)]
pub struct ChatModerationService {
    state: Mutex<ModerationState>,
}

impl ChatModerationService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 저장된 차단 목록으로 초기화.
    pub fn with_blocked_users<I: IntoIterator<Item = String>>(blocked: I) -> Self {
        let svc = Self::default();
        svc.lock().blocked_users = blocked.into_iter().collect();
        svc
    }

    fn lock(&self) -> MutexGuard<'_, ModerationState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // MARK: - Filter Management

    pub fn add_filter(&self, filter: ChatFilter) {
        self.lock().filters.push(filter);
    }

    pub fn remove_all_filters(&self) {
        self.lock().filters.clear();
    }

    pub fn current_filters(&self) -> Vec<ChatFilter> {
        self.lock().filters.clone()
    }

    /// UI 키워드 필터 — replace semantics. 빈 배열이면 슬롯을 비운다.
    pub fn set_keyword_filter(&self, keywords: Vec<String>, enabled: bool) {
        self.lock().keyword_filter =
            (!keywords.is_empty()).then(|| ChatFilter::new(FilterType::Keyword(keywords), enabled));
    }

    /// 키워드 필터 활성 토글 — 슬롯 값은 유지.
    pub fn set_keyword_filter_enabled(&self, enabled: bool) {
        let mut st = self.lock();
        if let Some(ChatFilter { r#type: FilterType::Keyword(words), .. }) = &st.keyword_filter {
            let words = words.clone();
            st.keyword_filter = Some(ChatFilter::new(FilterType::Keyword(words), enabled));
        }
    }

    /// 실제로 적용될 규칙이 하나라도 있는가. 만료된 뮤트는 판정하면서 걷어 낸다.
    pub fn has_active_rules(&self) -> bool {
        let mut st = self.lock();
        if !st.blocked_users.is_empty() {
            return true;
        }
        if !st.muted_users.is_empty() {
            Self::clean_expired_mutes_locked(&mut st);
            if !st.muted_users.is_empty() {
                return true;
            }
        }
        if let Some(ChatFilter { r#type: FilterType::Keyword(words), is_enabled: true, .. }) = &st.keyword_filter {
            if !words.is_empty() {
                return true;
            }
        }
        st.filters.iter().any(|f| f.is_enabled)
    }

    // MARK: - User Management

    pub fn block_user(&self, user_id: &str) {
        self.lock().blocked_users.insert(user_id.to_owned());
        tracing::info!(user_id, "Blocked user");
    }

    pub fn unblock_user(&self, user_id: &str) {
        self.lock().blocked_users.remove(user_id);
        tracing::info!(user_id, "Unblocked user");
    }

    /// `duration` 초 동안 뮤트. 음수는 이미 만료된 항목을 남긴다(Swift 와 같다).
    pub fn mute_user(&self, user_id: &str, duration: f64) {
        self.lock().muted_users.insert(user_id.to_owned(), MuteExpiry::from_duration(duration));
        tracing::info!(user_id, duration, "Muted user");
    }

    pub fn unmute_user(&self, user_id: &str) {
        self.lock().muted_users.remove(user_id);
    }

    pub fn is_blocked(&self, user_id: &str) -> bool {
        self.lock().blocked_users.contains(user_id)
    }

    pub fn blocked_users(&self) -> HashSet<String> {
        self.lock().blocked_users.clone()
    }

    pub fn is_muted(&self, user_id: &str) -> bool {
        self.lock().muted_users.get(user_id).is_some_and(|e| e.active(Instant::now()))
    }

    /// 만료된 뮤트 항목 정리.
    pub fn clean_expired_mutes(&self) {
        Self::clean_expired_mutes_locked(&mut self.lock());
    }

    /// 남아 있는 뮤트 항목 수 (만료 포함).
    pub fn muted_entry_count(&self) -> usize {
        self.lock().muted_users.len()
    }

    fn clean_expired_mutes_locked(st: &mut ModerationState) {
        let now = Instant::now();
        // Swift: `value > now` 만 남긴다.
        st.muted_users.retain(|_, e| match e {
            MuteExpiry::At(t) => *t > now,
            MuteExpiry::Past => false,
        });
    }

    // MARK: - Message Processing

    /// 필터·차단을 통과한 메시지만 남긴다.
    /// 후원·구독·공지·시스템 메시지는 콘텐츠 필터를 우회한다(차단·뮤트는 그대로 적용).
    pub fn filter_messages(&self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let st = self.lock();
        messages.iter().filter(|m| Self::passes(&st, m)).cloned().collect()
    }

    pub fn should_show(&self, message: &ChatMessage) -> bool {
        Self::passes(&self.lock(), message)
    }

    /// 표시 여부 술어 — 단일 출처.
    fn passes(st: &ModerationState, message: &ChatMessage) -> bool {
        let user_id = message.user_id.as_deref().unwrap_or("");
        if st.blocked_users.contains(user_id) {
            return false;
        }
        if st.muted_users.get(user_id).is_some_and(|e| e.active(Instant::now())) {
            return false;
        }
        if matches!(
            message.r#type,
            MessageType::Donation | MessageType::Subscription | MessageType::Notice | MessageType::SystemMessage
        ) {
            return true;
        }
        if st.keyword_filter.as_ref().is_some_and(|f| f.should_filter(message)) {
            return false;
        }
        !st.filters.iter().any(|f| f.should_filter(message))
    }

    // MARK: - Command Processing

    /// 명령을 처리한다. 명령이 아니면 `None`.
    pub fn process_command(&self, input: &str) -> Option<ChatCommandResult> {
        let (command, args) = ChatCommand::parse(input)?;
        let first = args.first().cloned();
        let usage = |text: &str| Some(ChatCommandResult::Error(text.to_owned()));

        match command {
            ChatCommand::Mute => {
                let Some(user_id) = first else { return usage("사용법: /mute <사용자ID> [시간(초)]") };
                // 사용자 입력이라 "inf"/"nan"/"1e30" 이 모두 실수로 파싱된다 — 유한값만 받아 1초~상한으로.
                let duration = match args.get(1).and_then(|s| s.parse::<f64>().ok()) {
                    Some(d) if d.is_finite() => d.clamp(1.0, chat_defaults::MAX_MUTE_DURATION_SECS),
                    _ => chat_defaults::DEFAULT_MUTE_DURATION_SECS,
                };
                self.mute_user(&user_id, duration);
                Some(ChatCommandResult::LocalAction(format!(
                    "'{user_id}' 님이 {}초간 채팅 금지 되었습니다.",
                    duration as i64
                )))
            }
            ChatCommand::Unmute => {
                let Some(user_id) = first else { return usage("사용법: /unmute <사용자ID>") };
                self.unmute_user(&user_id);
                Some(ChatCommandResult::LocalAction(format!("'{user_id}' 님의 채팅 금지가 해제되었습니다.")))
            }
            ChatCommand::Ban => {
                let Some(user_id) = first else { return usage("사용법: /ban <사용자ID>") };
                self.block_user(&user_id);
                Some(ChatCommandResult::LocalAction(format!("'{user_id}' 님이 차단되었습니다.")))
            }
            ChatCommand::Unban => {
                let Some(user_id) = first else { return usage("사용법: /unban <사용자ID>") };
                self.unblock_user(&user_id);
                Some(ChatCommandResult::LocalAction(format!("'{user_id}' 님의 차단이 해제되었습니다.")))
            }
            ChatCommand::Clear => Some(ChatCommandResult::ClearChat),
            ChatCommand::Notice => {
                if args.join(" ").is_empty() {
                    return usage("사용법: /notice <메시지>");
                }
                Some(ChatCommandResult::ServerCommand { command, args })
            }
            ChatCommand::Slow | ChatCommand::Host => Some(ChatCommandResult::ServerCommand { command, args }),
            ChatCommand::Filter => {
                if args.is_empty() {
                    return usage("사용법: /filter <키워드> [키워드2 …]");
                }
                let message = format!("키워드 필터 추가: {}", args.join(", "));
                self.add_filter(ChatFilter::new(FilterType::Keyword(args), true));
                Some(ChatCommandResult::LocalAction(message))
            }
            ChatCommand::Export => Some(ChatCommandResult::ExportChat),
            ChatCommand::Block => {
                let Some(user_id) = first else { return usage("사용법: /block <사용자ID>") };
                self.block_user(&user_id);
                Some(ChatCommandResult::LocalAction(format!("'{user_id}' 님이 로컬 차단되었습니다.")))
            }
            ChatCommand::Unblock => {
                let Some(user_id) = first else { return usage("사용법: /unblock <사용자ID>") };
                self.unblock_user(&user_id);
                Some(ChatCommandResult::LocalAction(format!("'{user_id}' 님의 로컬 차단이 해제되었습니다.")))
            }
            ChatCommand::Help => {
                let help = ChatCommand::ALL
                    .iter()
                    .map(|c| format!("{} — {}", c.raw_value(), c.description()))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(ChatCommandResult::LocalAction(format!("사용 가능한 명령어:\n{help}")))
            }
        }
    }
}
