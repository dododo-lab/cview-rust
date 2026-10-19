//! `Sources/CViewCore/Models/SettingsModels+UISettings.swift` 포팅 — 키보드 단축키, 채널별 알림,
//! 멀티라이브·멀티채팅·동기화 설정.
//!
//! 저장 형식(JSON 키·인코딩 모양)은 Swift 와 같다. 누락 키는 기본값, 형식이 어긋난 키는 오류
//! (Swift `decodeIfPresent ?? default` 와 같은 정책).

use std::collections::HashMap;

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

// MARK: - 키보드 단축키

/// 단축키 액션 정의. raw value 는 camelCase 이름.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutAction {
    TogglePlay,
    ToggleMute,
    ToggleFullscreen,
    ToggleChat,
    #[serde(rename = "togglePiP")]
    TogglePiP,
    Screenshot,
    VolumeUp,
    VolumeDown,
    /// 스트림 북마크 — 라이브 시청 중 현재 순간을 캡처.
    BookmarkStream,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 9] = [
        Self::TogglePlay,
        Self::ToggleMute,
        Self::ToggleFullscreen,
        Self::ToggleChat,
        Self::TogglePiP,
        Self::Screenshot,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::BookmarkStream,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::TogglePlay => "togglePlay",
            Self::ToggleMute => "toggleMute",
            Self::ToggleFullscreen => "toggleFullscreen",
            Self::ToggleChat => "toggleChat",
            Self::TogglePiP => "togglePiP",
            Self::Screenshot => "screenshot",
            Self::VolumeUp => "volumeUp",
            Self::VolumeDown => "volumeDown",
            Self::BookmarkStream => "bookmarkStream",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::TogglePlay => "재생/일시정지",
            Self::ToggleMute => "음소거 전환",
            Self::ToggleFullscreen => "전체화면 전환",
            Self::ToggleChat => "채팅 표시/숨기기",
            Self::TogglePiP => "PiP 전환",
            Self::Screenshot => "스크린샷",
            Self::VolumeUp => "볼륨 올리기",
            Self::VolumeDown => "볼륨 내리기",
            Self::BookmarkStream => "스트림 북마크 캡처",
        }
    }

    /// 이 동작의 단축키를 메인 메뉴가 소유하는가(사용자 재바인딩 무효).
    pub fn is_menu_owned(self) -> bool {
        matches!(
            self,
            Self::TogglePlay | Self::ToggleMute | Self::VolumeUp | Self::VolumeDown
        )
    }

    /// 메뉴가 소유한 경우 현재 고정 단축키 표기.
    pub fn menu_owned_shortcut_label(self) -> Option<&'static str> {
        match self {
            Self::TogglePlay => Some("Space"),
            Self::ToggleMute => Some("M"),
            Self::VolumeUp => Some("↑"),
            Self::VolumeDown => Some("↓"),
            _ => None,
        }
    }

    /// SF Symbol 이름.
    pub fn icon(self) -> &'static str {
        match self {
            Self::TogglePlay => "play.fill",
            Self::ToggleMute => "speaker.slash.fill",
            Self::ToggleFullscreen => "arrow.up.left.and.arrow.down.right",
            Self::ToggleChat => "bubble.left.and.bubble.right",
            Self::TogglePiP => "pip",
            Self::Screenshot => "camera.fill",
            Self::VolumeUp => "speaker.wave.3.fill",
            Self::VolumeDown => "speaker.wave.1.fill",
            Self::BookmarkStream => "bookmark.fill",
        }
    }
}

/// 단축키 수식 키 조합 (OptionSet — rawValue 정수로 저장).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShortcutModifiers(pub i64);

impl ShortcutModifiers {
    pub const COMMAND: Self = Self(1 << 0);
    pub const SHIFT: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const CONTROL: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn display_symbols(self) -> String {
        let mut s = String::new();
        if self.contains(Self::CONTROL) {
            s.push('⌃');
        }
        if self.contains(Self::OPTION) {
            s.push('⌥');
        }
        if self.contains(Self::SHIFT) {
            s.push('⇧');
        }
        if self.contains(Self::COMMAND) {
            s.push('⌘');
        }
        s
    }
}

impl std::ops::BitOr for ShortcutModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// 키 바인딩 (키 + 수식키).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    pub modifiers: ShortcutModifiers,
}

impl KeyBinding {
    pub fn new(key: impl Into<String>, modifiers: ShortcutModifiers) -> Self {
        Self {
            key: key.into(),
            modifiers,
        }
    }

    pub fn key_display_name(&self) -> String {
        match self.key.as_str() {
            "space" => "Space".into(),
            "upArrow" => "↑".into(),
            "downArrow" => "↓".into(),
            "leftArrow" => "←".into(),
            "rightArrow" => "→".into(),
            "return" => "⏎".into(),
            "escape" => "Esc".into(),
            "tab" => "⇥".into(),
            "delete" => "⌫".into(),
            other => other.to_uppercase(),
        }
    }

    pub fn display_name(&self) -> String {
        self.modifiers.display_symbols() + &self.key_display_name()
    }
}

/// 키보드 단축키 설정.
///
/// `bindings` 는 Swift 표준 라이브러리 형식 그대로 비키 배열 `[키, 값, 키, 값, …]` 로 저장하고,
/// 같은 설정이면 같은 바이트가 나오도록 raw value 순으로 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardShortcutSettings {
    pub bindings: HashMap<ShortcutAction, KeyBinding>,
}

impl KeyboardShortcutSettings {
    pub fn default_bindings() -> HashMap<ShortcutAction, KeyBinding> {
        use ShortcutAction::*;
        let none = ShortcutModifiers::empty();
        HashMap::from([
            (TogglePlay, KeyBinding::new("space", none)),
            (ToggleMute, KeyBinding::new("m", none)),
            (ToggleFullscreen, KeyBinding::new("f", none)),
            (ToggleChat, KeyBinding::new("c", none)),
            (TogglePiP, KeyBinding::new("p", none)),
            (Screenshot, KeyBinding::new("s", ShortcutModifiers::COMMAND)),
            (VolumeUp, KeyBinding::new("upArrow", none)),
            (VolumeDown, KeyBinding::new("downArrow", none)),
            (
                BookmarkStream,
                KeyBinding::new("b", ShortcutModifiers::COMMAND),
            ),
        ])
    }

    pub fn binding(&self, action: ShortcutAction) -> KeyBinding {
        self.bindings
            .get(&action)
            .cloned()
            .or_else(|| Self::default_bindings().remove(&action))
            .unwrap_or_else(|| KeyBinding::new("", ShortcutModifiers::empty()))
    }
}

impl Default for KeyboardShortcutSettings {
    fn default() -> Self {
        Self {
            bindings: Self::default_bindings(),
        }
    }
}

struct BindingPairs<'a>(&'a HashMap<ShortcutAction, KeyBinding>);

impl Serialize for BindingPairs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<_> = self.0.iter().collect();
        entries.sort_by(|a, b| a.0.raw_value().cmp(b.0.raw_value()));
        let mut seq = serializer.serialize_seq(Some(entries.len() * 2))?;
        for (action, binding) in entries {
            seq.serialize_element(action)?;
            seq.serialize_element(binding)?;
        }
        seq.end()
    }
}

impl Serialize for KeyboardShortcutSettings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("bindings", &BindingPairs(&self.bindings))?;
        map.end()
    }
}

struct PairsVisitor;

impl<'de> Visitor<'de> for PairsVisitor {
    type Value = HashMap<ShortcutAction, KeyBinding>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[action, binding, …] 배열")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut map = HashMap::new();
        while let Some(action) = seq.next_element::<ShortcutAction>()? {
            let binding = seq
                .next_element::<KeyBinding>()?
                .ok_or_else(|| de::Error::custom("키에 대응하는 값이 없습니다"))?;
            map.insert(action, binding);
        }
        Ok(map)
    }
}

struct Pairs(HashMap<ShortcutAction, KeyBinding>);

impl<'de> Deserialize<'de> for Pairs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(PairsVisitor).map(Pairs)
    }
}

impl<'de> Deserialize<'de> for KeyboardShortcutSettings {
    /// 누락 키 허용 — `bindings` 가 없으면 기본 단축키.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            bindings: Option<Pairs>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            bindings: raw
                .bindings
                .map(|p| p.0)
                .unwrap_or_else(Self::default_bindings),
        })
    }
}

// MARK: - 채널별 알림

/// 채널별 알림 세분 설정.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelNotificationSetting {
    pub channel_id: String,
    pub channel_name: String,
    pub notify_on_live: bool,
    pub notify_on_category_change: bool,
    pub notify_on_title_change: bool,
}

impl ChannelNotificationSetting {
    pub fn default_setting(channel_id: impl Into<String>, channel_name: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            notify_on_live: true,
            notify_on_category_change: true,
            notify_on_title_change: true,
        }
    }

    pub fn id(&self) -> &str {
        &self.channel_id
    }

    pub fn is_all_disabled(&self) -> bool {
        !self.notify_on_live && !self.notify_on_category_change && !self.notify_on_title_change
    }
}

/// 채널별 알림 설정 저장소. `settings` 키가 없으면 빈 사전.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelNotificationSettings {
    pub settings: HashMap<String, ChannelNotificationSetting>,
}

impl ChannelNotificationSettings {
    pub fn setting(&self, channel_id: &str, channel_name: &str) -> ChannelNotificationSetting {
        self.settings.get(channel_id).cloned().unwrap_or_else(|| {
            ChannelNotificationSetting::default_setting(channel_id, channel_name)
        })
    }

    pub fn update(&mut self, setting: ChannelNotificationSetting) {
        self.settings.insert(setting.channel_id.clone(), setting);
    }

    pub fn is_live_notification_enabled(&self, channel_id: &str) -> bool {
        self.settings
            .get(channel_id)
            .map(|s| s.notify_on_live)
            .unwrap_or(true)
    }

    pub fn is_category_change_notification_enabled(&self, channel_id: &str) -> bool {
        self.settings
            .get(channel_id)
            .map(|s| s.notify_on_category_change)
            .unwrap_or(true)
    }

    pub fn is_title_change_notification_enabled(&self, channel_id: &str) -> bool {
        self.settings
            .get(channel_id)
            .map(|s| s.notify_on_title_change)
            .unwrap_or(true)
    }
}

// MARK: - 멀티라이브

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiLiveLayoutMode {
    #[default]
    Preset,
    Focus,
}

impl MultiLiveLayoutMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Preset => "균등 분할",
            Self::Focus => "포커스 모드",
        }
    }
}

/// 포커스 레이아웃 기본 방향 설정.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiLiveFocusDirectionSetting {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl MultiLiveFocusDirectionSetting {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Left => "왼쪽",
            Self::Right => "오른쪽",
            Self::Top => "위",
            Self::Bottom => "아래",
        }
    }
}

fn de_max_sessions<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    // 과거 빌드가 4 미만으로 저장한 값은 기본값(4)으로 끌어올린다.
    Ok(i64::deserialize(d)?.max(MultiLiveSettings::default().max_concurrent_sessions))
}

/// 멀티라이브 설정. 제거된 프로세스 격리 키는 디코드 시 무시된다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MultiLiveSettings {
    #[serde(deserialize_with = "de_max_sessions")]
    pub max_concurrent_sessions: i64,
    pub default_layout_mode: MultiLiveLayoutMode,
    pub multi_audio_enabled: bool,
    pub secondary_volume: f32,
    pub background_quality_reduction: bool,
    pub auto_reconnect: bool,
    pub auto_reconnect_max_retries: i64,
    pub chat_overlay_in_grid: bool,
    pub chat_overlay_opacity: f64,
    pub chat_overlay_font_size: f64,
    /// 대역폭 코디네이터가 세션 간 대역폭을 자동 분배.
    pub bandwidth_coordination_enabled: bool,
    /// 화면 크기 기반 자동 화질 캡핑.
    pub level_capping_enabled: bool,
    /// 선택 세션에 할당하는 대역폭 가중치 (1.0=균등).
    #[serde(rename = "selectedSessionBWWeight")]
    pub selected_session_bw_weight: f64,
    /// 마지막 사용 레이아웃을 기억해 다음 실행에서 이어간다.
    pub remember_last_layout: bool,
    /// 포커스 모드의 기본 메인 패인 방향.
    pub default_focus_direction: MultiLiveFocusDirectionSetting,
}

impl Default for MultiLiveSettings {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 4,
            default_layout_mode: MultiLiveLayoutMode::Preset,
            multi_audio_enabled: false,
            secondary_volume: 0.3,
            background_quality_reduction: true,
            auto_reconnect: true,
            auto_reconnect_max_retries: 10,
            chat_overlay_in_grid: false,
            chat_overlay_opacity: 0.5,
            chat_overlay_font_size: 12.0,
            bandwidth_coordination_enabled: true,
            level_capping_enabled: true,
            selected_session_bw_weight: 1.5,
            remember_last_layout: true,
            default_focus_direction: MultiLiveFocusDirectionSetting::Left,
        }
    }
}

// MARK: - 멀티채팅

/// 멀티채팅 패널 폭 비율 정책 (`Utilities/MultiChatPanelWidthPolicy.swift` 의 비율 클램프 부분).
pub mod multi_chat_panel_width {
    pub const MINIMUM_RATIO: f64 = 0.05;
    pub const MAXIMUM_RATIO: f64 = 0.95;
    pub const DEFAULT_RATIO: f64 = 0.20;

    /// 유한하지 않으면 기본값, 아니면 0.05–0.95 로 제한.
    pub fn clamped_ratio(ratio: f64) -> f64 {
        if !ratio.is_finite() {
            return DEFAULT_RATIO;
        }
        ratio.clamp(MINIMUM_RATIO, MAXIMUM_RATIO)
    }
}

/// 저장용 멀티채팅 세션 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedChatSession {
    pub channel_id: String,
    pub channel_name: String,
}

/// 멀티채팅 세션 설정 (영속 저장용).
///
/// 세션·선택·그리드 필드는 작업영역 파일로 이관 중이며, 이관이 끝나면(`workspace_migrated`) 인코딩하지 않는다.
/// `panel_width_ratio` 는 setter 로만 바꾼다 — 새 폭을 고르면 이전 프리셋 복원 이력이 끝난다(Swift `didSet`).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiChatSettings {
    pub saved_sessions: Vec<SavedChatSession>,
    pub selected_channel_id: Option<String>,
    /// false = 탭(단일 채널), true = 그리드.
    pub is_grid_layout: bool,
    /// "preset" | "custom" | "focusLeft"
    pub grid_layout_mode: String,
    pub grid_horizontal_ratio: f64,
    pub grid_vertical_ratio: f64,
    /// 포커스(1+N) 메인 분할 비율.
    pub grid_focus_ratio: f64,
    panel_width_ratio: f64,
    /// 읽기 폭 프리셋을 적용하기 직전의 비율. `None` 이면 되돌릴 대상이 없다.
    pub previous_panel_width_ratio: Option<f64>,
    /// 옮긴 필드를 작업영역 파일로 이관했다는 표시.
    pub workspace_migrated: bool,
}

impl Default for MultiChatSettings {
    fn default() -> Self {
        Self {
            saved_sessions: Vec::new(),
            selected_channel_id: None,
            is_grid_layout: false,
            grid_layout_mode: "preset".into(),
            grid_horizontal_ratio: 0.5,
            grid_vertical_ratio: 0.5,
            grid_focus_ratio: Self::DEFAULT_GRID_FOCUS_RATIO,
            panel_width_ratio: multi_chat_panel_width::DEFAULT_RATIO,
            previous_panel_width_ratio: None,
            workspace_migrated: false,
        }
    }
}

impl MultiChatSettings {
    /// 포커스 메인 기본 비율 — 멀티라이브 `focusRatio` 기본값과 같아야 한다.
    pub const DEFAULT_GRID_FOCUS_RATIO: f64 = 0.7;

    pub fn panel_width_ratio(&self) -> f64 {
        self.panel_width_ratio
    }

    /// 새 폭을 고른다(클램프). 값이 바뀌면 이전 프리셋 복원 이력을 끝낸다.
    pub fn set_panel_width_ratio(&mut self, ratio: f64) {
        let ratio = multi_chat_panel_width::clamped_ratio(ratio);
        if ratio != self.panel_width_ratio {
            self.previous_panel_width_ratio = None;
        }
        self.panel_width_ratio = ratio;
    }

    /// 읽기 폭 프리셋 적용 — 처음 적용 전 비율을 보존한다.
    pub fn apply_reading_width_ratio(&mut self, ratio: f64) {
        if !ratio.is_finite() {
            return;
        }
        let ratio = multi_chat_panel_width::clamped_ratio(ratio);
        if ratio == self.panel_width_ratio {
            return;
        }
        let previous = self
            .previous_panel_width_ratio
            .unwrap_or(self.panel_width_ratio);
        self.panel_width_ratio = ratio;
        self.previous_panel_width_ratio = Some(previous);
    }

    pub fn restore_previous_panel_width(&mut self) {
        if let Some(previous) = self.previous_panel_width_ratio.take() {
            self.panel_width_ratio = previous;
        }
    }

    /// 정상 종료 뒤 복원하면 안 되는 세션 identity 만 비운다.
    pub fn clearing_restorable_sessions(&self) -> Self {
        Self {
            saved_sessions: Vec::new(),
            selected_channel_id: None,
            ..self.clone()
        }
    }

    /// 작업영역 파일로 옮긴 뒤의 도메인.
    pub fn marking_workspace_migrated(&self) -> Self {
        Self {
            workspace_migrated: true,
            ..self.clearing_restorable_sessions()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiChatSettingsRaw {
    saved_sessions: Option<Vec<SavedChatSession>>,
    selected_channel_id: Option<String>,
    is_grid_layout: Option<bool>,
    grid_layout_mode: Option<String>,
    grid_horizontal_ratio: Option<f64>,
    grid_vertical_ratio: Option<f64>,
    grid_focus_ratio: Option<f64>,
    panel_width_ratio: Option<f64>,
    previous_panel_width_ratio: Option<f64>,
    workspace_migrated: Option<bool>,
}

impl<'de> Deserialize<'de> for MultiChatSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let r = MultiChatSettingsRaw::deserialize(deserializer)?;
        let d = Self::default();
        Ok(Self {
            saved_sessions: r.saved_sessions.unwrap_or_default(),
            selected_channel_id: r.selected_channel_id,
            is_grid_layout: r.is_grid_layout.unwrap_or(false),
            grid_layout_mode: r.grid_layout_mode.unwrap_or(d.grid_layout_mode),
            grid_horizontal_ratio: r.grid_horizontal_ratio.unwrap_or(0.5),
            grid_vertical_ratio: r.grid_vertical_ratio.unwrap_or(0.5),
            grid_focus_ratio: r.grid_focus_ratio.unwrap_or(Self::DEFAULT_GRID_FOCUS_RATIO),
            panel_width_ratio: multi_chat_panel_width::clamped_ratio(
                r.panel_width_ratio
                    .unwrap_or(multi_chat_panel_width::DEFAULT_RATIO),
            ),
            previous_panel_width_ratio: r
                .previous_panel_width_ratio
                .map(multi_chat_panel_width::clamped_ratio),
            workspace_migrated: r.workspace_migrated.unwrap_or(false),
        })
    }
}

impl Serialize for MultiChatSettings {
    /// 이관이 끝난 도메인은 옮긴 필드를 쓰지 않는다. 이관 전에는 디코드한 값을 그대로 되쓴다.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("panelWidthRatio", &self.panel_width_ratio)?;
        if let Some(previous) = self.previous_panel_width_ratio {
            map.serialize_entry("previousPanelWidthRatio", &previous)?;
        }
        if self.workspace_migrated {
            map.serialize_entry("workspaceMigrated", &true)?;
            return map.end();
        }
        map.serialize_entry("savedSessions", &self.saved_sessions)?;
        if let Some(id) = &self.selected_channel_id {
            map.serialize_entry("selectedChannelId", id)?;
        }
        map.serialize_entry("isGridLayout", &self.is_grid_layout)?;
        map.serialize_entry("gridLayoutMode", &self.grid_layout_mode)?;
        map.serialize_entry("gridHorizontalRatio", &self.grid_horizontal_ratio)?;
        map.serialize_entry("gridVerticalRatio", &self.grid_vertical_ratio)?;
        map.serialize_entry("gridFocusRatio", &self.grid_focus_ratio)?;
        map.end()
    }
}

// MARK: - 동기화

/// 시청 기록 sync 설정. 기본 OFF — 명시적 opt-in 후에만 push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    pub watch_history_sync_enabled: bool,
    pub endpoint: String,
    /// `true` 가 의도된 사용자 opt-in 인지 표시.
    pub explicitly_opted_in: bool,
}

impl SyncSettings {
    pub const DEFAULT_ENDPOINT: &'static str = "https://cv.dododo.app";
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            watch_history_sync_enabled: false,
            endpoint: Self::DEFAULT_ENDPOINT.into(),
            explicitly_opted_in: false,
        }
    }
}

impl<'de> Deserialize<'de> for SyncSettings {
    /// 마이그레이션: 명시적으로 opt-in 하지 않은 사용자는 저장값과 무관하게 OFF.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            watch_history_sync_enabled: Option<bool>,
            endpoint: Option<String>,
            explicitly_opted_in: Option<bool>,
        }
        let r = Raw::deserialize(deserializer)?;
        let d = Self::default();
        let opted_in = r.explicitly_opted_in.unwrap_or(false);
        let stored = r
            .watch_history_sync_enabled
            .unwrap_or(d.watch_history_sync_enabled);
        Ok(Self {
            watch_history_sync_enabled: opted_in && stored,
            endpoint: r.endpoint.unwrap_or(d.endpoint),
            explicitly_opted_in: opted_in,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shortcut_encoding_is_sorted_pairs() {
        let settings = KeyboardShortcutSettings::default();
        let value = serde_json::to_value(&settings).unwrap();
        let arr = value["bindings"].as_array().unwrap();
        assert_eq!(arr.len(), 18);
        assert_eq!(arr[0], "bookmarkStream");
        assert_eq!(arr[1], json!({"key": "b", "modifiers": 1}));
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            serde_json::to_string(&settings.clone()).unwrap()
        );
        let back: KeyboardShortcutSettings = serde_json::from_value(value).unwrap();
        assert_eq!(back, settings);
        let missing: KeyboardShortcutSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(missing, settings);
        let custom: KeyboardShortcutSettings = serde_json::from_value(
            json!({"bindings": ["togglePiP", {"key": "x", "modifiers": 3}]}),
        )
        .unwrap();
        assert_eq!(
            custom.binding(ShortcutAction::TogglePiP).display_name(),
            "⇧⌘X"
        );
        assert_eq!(
            custom.binding(ShortcutAction::TogglePlay).key,
            "space",
            "없는 액션은 기본값"
        );
    }

    #[test]
    fn shortcut_metadata() {
        assert!(ShortcutAction::TogglePlay.is_menu_owned());
        assert!(!ShortcutAction::Screenshot.is_menu_owned());
        assert_eq!(
            ShortcutAction::VolumeUp.menu_owned_shortcut_label(),
            Some("↑")
        );
        assert_eq!(
            KeyBinding::new("upArrow", ShortcutModifiers::empty()).display_name(),
            "↑"
        );
        let all = ShortcutModifiers::COMMAND
            | ShortcutModifiers::SHIFT
            | ShortcutModifiers::OPTION
            | ShortcutModifiers::CONTROL;
        assert_eq!(all.display_symbols(), "⌃⌥⇧⌘");
    }

    #[test]
    fn channel_notifications() {
        let mut s: ChannelNotificationSettings = serde_json::from_str("{}").unwrap();
        assert!(s.is_live_notification_enabled("x"));
        let mut one = s.setting("x", "X");
        one.notify_on_live = false;
        s.update(one);
        assert!(!s.is_live_notification_enabled("x"));
        assert!(s.is_title_change_notification_enabled("x"));
        let back: ChannelNotificationSettings =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn multi_live_backfills_and_lifts_minimum() {
        let s: MultiLiveSettings = serde_json::from_value(
            json!({"maxConcurrentSessions": 2, "useSeparateProcesses": true}),
        )
        .unwrap();
        assert_eq!(s.max_concurrent_sessions, 4);
        assert_eq!(s.selected_session_bw_weight, 1.5);
        let s: MultiLiveSettings =
            serde_json::from_value(json!({"maxConcurrentSessions": 8})).unwrap();
        assert_eq!(s.max_concurrent_sessions, 8);
        assert!(serde_json::to_value(MultiLiveSettings::default())
            .unwrap()
            .get("selectedSessionBWWeight")
            .is_some());
    }

    #[test]
    fn multi_chat_panel_ratio_and_encoding() {
        let s: MultiChatSettings = serde_json::from_value(json!({"panelWidthRatio": 2.0})).unwrap();
        assert_eq!(s.panel_width_ratio(), 0.95);
        assert_eq!(s.grid_focus_ratio, 0.7);

        let mut s = MultiChatSettings::default();
        s.apply_reading_width_ratio(0.3);
        s.apply_reading_width_ratio(0.4);
        assert_eq!(s.previous_panel_width_ratio, Some(0.2));
        s.restore_previous_panel_width();
        assert_eq!(s.panel_width_ratio(), 0.2);
        s.apply_reading_width_ratio(0.3);
        s.set_panel_width_ratio(0.5);
        assert_eq!(s.previous_panel_width_ratio, None);

        let mut s = MultiChatSettings::default();
        s.saved_sessions.push(SavedChatSession {
            channel_id: "a".into(),
            channel_name: "A".into(),
        });
        let full = serde_json::to_value(&s).unwrap();
        assert!(full.get("savedSessions").is_some());
        let migrated = serde_json::to_value(s.marking_workspace_migrated()).unwrap();
        assert_eq!(
            migrated,
            json!({"panelWidthRatio": 0.2, "workspaceMigrated": true})
        );
    }

    #[test]
    fn sync_settings_migration() {
        let s: SyncSettings =
            serde_json::from_value(json!({"watchHistorySyncEnabled": true})).unwrap();
        assert!(!s.watch_history_sync_enabled);
        let s: SyncSettings = serde_json::from_value(
            json!({"watchHistorySyncEnabled": true, "explicitlyOptedIn": true}),
        )
        .unwrap();
        assert!(s.watch_history_sync_enabled);
        assert_eq!(s.endpoint, SyncSettings::DEFAULT_ENDPOINT);
    }
}
