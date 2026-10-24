//! 설정 도메인 모델 포팅:
//! - `Sources/CViewCore/Models/SettingsModels.swift` (스크린샷 포맷, 플레이어 설정, 실험 스위치)
//! - `Sources/CViewCore/Models/SettingsModels+AppConfig.swift` (일반/외관/네트워크 설정, 프리셋, 테마)
//! - `Sources/CViewCore/Models/SettingsModels+Chat.swift` 의 열거형(`ChatDisplayMode`·`ChatDesignStyle`·
//!   `ChatMessageAlignment`). `ChatSettings` 본체는 채팅 모델(`ChatBackgroundSettings`)에 의존하므로
//!   채팅 모델 포트(`models::chat`)가 들어온 뒤 옮긴다.
//!
//! [SET-3 2026-08-11] 저장본 호환 규칙(원본과 동일):
//! - **없는 키는 기본값**으로 채운다 — 구조체 단위 `#[serde(default)]`.
//! - **있는데 형식이 어긋난 값은 오류**다(Swift `decodeIfPresent` 와 같은 엄격도).
//! - 범위 밖 값은 디코딩 시 클램프한다(필드별 `deserialize_with`).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Deserializer, Serialize};

use super::live::StreamQuality;
use super::stream_proxy::StreamProxyMode;

// MARK: - 스크린샷 포맷

/// 스크린샷 저장 포맷.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    #[default]
    Png,
    Jpeg,
}

impl ScreenshotFormat {
    pub const ALL: [ScreenshotFormat; 2] = [ScreenshotFormat::Png, ScreenshotFormat::Jpeg];

    pub fn display_name(self) -> &'static str {
        match self {
            ScreenshotFormat::Png => "PNG",
            ScreenshotFormat::Jpeg => "JPEG",
        }
    }

    pub fn file_extension(self) -> &'static str {
        match self {
            ScreenshotFormat::Png => "png",
            ScreenshotFormat::Jpeg => "jpeg",
        }
    }
}

// MARK: - 플레이어 설정

/// 플레이어 설정.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayerSettings {
    pub quality: StreamQuality,
    pub low_latency_mode: bool,
    /// 레거시 호환 필드. 실제 캐치업 상한은 latency preset 의 `latency_max_rate` 가 단독 소유한다.
    pub catchup_rate: f64,
    pub buffer_duration: f64,
    pub volume_level: f32,
    /// [Persistence 2026-04-18] 마지막 음소거 상태 — 재실행 시 복원
    pub start_muted: bool,
    pub auto_play: bool,
    /// 백그라운드 및 비활성 상태에서도 라이브 방송 재생을 유지할지 여부
    pub continue_playback_in_background: bool,
    /// 항상 최고 화질(1080p60) 유지
    pub force_highest_quality: bool,
    /// 스트림 Content-Type 교정 / 인터셉트 모드
    pub stream_proxy_mode: StreamProxyMode,
    pub screenshot_path: String,
    pub screenshot_format: ScreenshotFormat,
    /// 이퀄라이저 프리셋 이름 (`None` = 비활성화)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equalizer_preset: Option<String>,
    /// 이퀄라이저 프리앰프 (dB, -20~+20)
    pub equalizer_pre_amp: f32,
    /// 이퀄라이저 밴드 값 (dB, -20~+20, 최대 10밴드)
    pub equalizer_bands: Vec<f32>,
    pub video_adjust_enabled: bool,
    pub video_brightness: f32,
    pub video_contrast: f32,
    pub video_saturation: f32,
    pub video_hue: f32,
    pub video_gamma: f32,
    /// 선명한 화면 — nearest-neighbor 스케일링
    pub sharp_pixel_scaling: bool,
    /// 화질 자동 보정
    pub auto_picture_enhancement: bool,
    /// 업스케일링
    pub video_upscaling: bool,
    /// 하드웨어 디코딩 — **현재 어떤 엔진도 읽지 않는다.** 저장 호환을 위해 필드만 유지.
    pub hardware_decoding: bool,
    /// 화면 비율 (`None` = 기본, "16:9" 등)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// 레이턴시 프리셋 ("matchWeb", "webSync", "default", "ultraLow", "custom")
    pub latency_preset: String,
    pub latency_target: f64,
    pub latency_max: f64,
    pub latency_min: f64,
    pub latency_max_rate: f64,
    pub latency_min_rate: f64,
    pub latency_catch_up_threshold: f64,
    pub latency_slow_down_threshold: f64,
    pub latency_pid_kp: f64,
    pub latency_pid_ki: f64,
    /// [UIUX 2026-09-20 C65] 디코드 시 0.5 로 상한 클램프.
    #[serde(deserialize_with = "de_kd")]
    pub latency_pid_kd: f64,
    /// 후원/구독 알림 오버레이의 정규화 앵커 X (0...1)
    pub stream_alert_anchor_x: f64,
    /// 후원/구독 알림 오버레이의 정규화 앵커 Y (0...1)
    pub stream_alert_anchor_y: f64,
    /// 후원/구독 알림 오버레이 크기 배율(0.7~1.7) — 디코드 시 클램프.
    #[serde(deserialize_with = "de_alert_scale")]
    pub stream_alert_scale: f64,
    /// 시청 중인 라이브를 디스크에 기록해 되감을 수 있게 한다(로컬 타임시프트).
    pub live_rewind_enabled: bool,
    /// 되감을 수 있는 최대 길이(분). 10/30/60 — 선택지 밖 값은 가장 가까운 선택지로.
    #[serde(deserialize_with = "de_rewind_window")]
    pub live_rewind_window_minutes: i64,
    /// 되감기 기록에 쓸 디스크 한도(MiB).
    #[serde(
        rename = "liveRewindDiskBudgetMB",
        deserialize_with = "de_rewind_budget"
    )]
    pub live_rewind_disk_budget_mb: i64,
    /// [실험] 치지직 공식 타임머신(치트키 전용) 재생.
    pub experimental_server_time_machine: bool,
    /// [실험] 웹 동기화 v2.
    #[serde(rename = "experimentalMatchWebV2")]
    pub experimental_match_web_v2: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        let v = LatencyPreset::MatchWeb.values();
        Self {
            quality: StreamQuality::Source,
            low_latency_mode: true,
            catchup_rate: 1.05,
            buffer_duration: 2.0,
            volume_level: 1.0,
            start_muted: false,
            auto_play: true,
            continue_playback_in_background: true,
            force_highest_quality: false,
            stream_proxy_mode: StreamProxyMode::LocalProxy,
            screenshot_path: "~/Pictures/CView Screenshots".into(),
            screenshot_format: ScreenshotFormat::Png,
            equalizer_preset: None,
            equalizer_pre_amp: 0.0,
            equalizer_bands: Vec::new(),
            video_adjust_enabled: false,
            video_brightness: 1.0,
            video_contrast: 1.0,
            video_saturation: 1.0,
            video_hue: 0.0,
            video_gamma: 1.0,
            sharp_pixel_scaling: false,
            auto_picture_enhancement: false,
            video_upscaling: false,
            hardware_decoding: true,
            aspect_ratio: None,
            latency_preset: LatencyPreset::MatchWeb.raw_value().into(),
            latency_target: v.target,
            latency_max: v.max,
            latency_min: v.min,
            latency_max_rate: v.max_rate,
            latency_min_rate: v.min_rate,
            latency_catch_up_threshold: v.catch_up,
            latency_slow_down_threshold: v.slow_down,
            latency_pid_kp: v.kp,
            latency_pid_ki: v.ki,
            latency_pid_kd: v.kd,
            stream_alert_anchor_x: 0.5,
            stream_alert_anchor_y: 0.08,
            stream_alert_scale: 1.0,
            live_rewind_enabled: true,
            live_rewind_window_minutes: 60,
            live_rewind_disk_budget_mb: 5_120,
            experimental_server_time_machine: false,
            experimental_match_web_v2: false,
        }
    }
}

fn de_kd<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(f64::deserialize(d)?.min(0.5))
}

fn de_alert_scale<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(f64::deserialize(d)?.clamp(0.7, 1.7))
}

fn de_rewind_window<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(PlayerSettings::sanitized_rewind_window(i64::deserialize(
        d,
    )?))
}

fn de_rewind_budget<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(PlayerSettings::sanitized_rewind_budget(i64::deserialize(
        d,
    )?))
}

fn nearest(choices: &[i64], value: i64, fallback: i64) -> i64 {
    // Swift `min(by:)` 은 동률이면 앞의 것을 고른다 — `min_by_key` 와 같다.
    choices
        .iter()
        .copied()
        .min_by_key(|c| (c - value).abs())
        .unwrap_or(fallback)
}

impl PlayerSettings {
    /// 되감기 창 선택지(분).
    pub const LIVE_REWIND_WINDOW_CHOICES: [i64; 3] = [10, 30, 60];
    /// 되감기 디스크 한도 선택지(MiB).
    pub const LIVE_REWIND_DISK_BUDGET_CHOICES: [i64; 4] = [2_048, 5_120, 10_240, 20_480];

    /// 선택지 밖의 값(손상·구버전)은 가장 가까운 선택지로 맞춘다.
    pub fn sanitized_rewind_window(minutes: i64) -> i64 {
        nearest(&Self::LIVE_REWIND_WINDOW_CHOICES, minutes, 60)
    }

    pub fn sanitized_rewind_budget(megabytes: i64) -> i64 {
        nearest(&Self::LIVE_REWIND_DISK_BUDGET_CHOICES, megabytes, 5_120)
    }

    /// 현재 선택된 프리셋 (알 수 없는 값은 `MatchWeb`).
    pub fn current_preset(&self) -> LatencyPreset {
        LatencyPreset::from_raw(&self.latency_preset).unwrap_or(LatencyPreset::MatchWeb)
    }

    /// 프리셋 값을 현재 설정에 적용. `Custom` 은 이름만 바꾸고 값은 건드리지 않는다.
    pub fn apply_latency_preset(&mut self, preset: LatencyPreset) {
        self.latency_preset = preset.raw_value().into();
        if preset == LatencyPreset::Custom {
            return;
        }
        let v = preset.values();
        self.latency_target = v.target;
        self.latency_max = v.max;
        self.latency_min = v.min;
        self.latency_max_rate = v.max_rate;
        self.latency_min_rate = v.min_rate;
        self.latency_catch_up_threshold = v.catch_up;
        self.latency_slow_down_threshold = v.slow_down;
        self.latency_pid_kp = v.kp;
        self.latency_pid_ki = v.ki;
        self.latency_pid_kd = v.kd;
    }
}

/// 레이턴시 프리셋 값 묶음 (Swift 튜플).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyPresetValues {
    pub target: f64,
    pub max: f64,
    pub min: f64,
    pub max_rate: f64,
    pub min_rate: f64,
    pub catch_up: f64,
    pub slow_down: f64,
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
}

/// 레이턴시 프리셋 정의 (Swift `PlayerSettings.LatencyPreset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LatencyPreset {
    MatchWeb,
    WebSync,
    Standard,
    UltraLow,
    Custom,
}

impl LatencyPreset {
    pub const ALL: [LatencyPreset; 5] = [
        Self::MatchWeb,
        Self::WebSync,
        Self::Standard,
        Self::UltraLow,
        Self::Custom,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::MatchWeb => "matchWeb",
            Self::WebSync => "webSync",
            Self::Standard => "default",
            Self::UltraLow => "ultraLow",
            Self::Custom => "custom",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.raw_value() == raw)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::MatchWeb => "웹 동기화",
            Self::WebSync => "안정 동기화 (6초)",
            Self::Standard => "기본",
            Self::UltraLow => "초저지연",
            Self::Custom => "사용자 지정",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::MatchWeb => "antenna.radiowaves.left.and.right",
            Self::WebSync => "globe",
            Self::Standard => "gauge.with.dots.needle.50percent",
            Self::UltraLow => "bolt.fill",
            Self::Custom => "slider.horizontal.3",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MatchWeb => "치지직 웹과 동일한 재생 위치 (권장)",
            Self::WebSync => "보수적 동기화 (6초 지연 — 안정 우선)",
            Self::Standard => "안정적인 재생 (3초 지연)",
            Self::UltraLow => "최소 지연, 끊김 가능 (1.5초 지연)",
            Self::Custom => "모든 파라미터를 직접 조정",
        }
    }

    /// 프리셋 기본 값. [지연 정합 2026-08-11] hls.js 기저(`hls.latency`) 값 —
    /// ⚠ `LowLatencyController.Configuration` 과 **쌍으로** 검증된다. 한쪽만 바꾸지 말 것.
    pub fn values(self) -> LatencyPresetValues {
        let (target, max, min, max_rate, min_rate, catch_up, slow_down, kp, ki, kd) = match self {
            Self::MatchWeb | Self::Custom => {
                (3.0, 8.0, 1.5, 1.03, 0.97, 0.55, 0.55, 0.35, 0.03, 0.02)
            }
            Self::WebSync => (6.0, 12.0, 3.0, 1.08, 0.93, 1.0, 0.8, 0.5, 0.05, 0.03),
            Self::Standard => (3.0, 8.0, 1.0, 1.15, 0.90, 1.2, 0.5, 0.8, 0.12, 0.06),
            Self::UltraLow => (1.5, 8.0, 0.5, 1.20, 0.85, 1.0, 0.3, 1.0, 0.15, 0.08),
        };
        LatencyPresetValues {
            target,
            max,
            min,
            max_rate,
            min_rate,
            catch_up,
            slow_down,
            kp,
            ki,
            kd,
        }
    }
}

// MARK: - 실험 재생 스위치

static SERVER_TIME_MACHINE: AtomicBool = AtomicBool::new(false);
static MATCH_WEB_V2_SETTING: AtomicBool = AtomicBool::new(false);

/// 설정 저장소를 들고 있지 않은 곳이 읽는 실험 스위치 (Swift `ExperimentalPlaybackFlags`).
/// 설정이 바뀔 때마다 저장소가 갱신한다. 기본은 꺼짐.
pub struct ExperimentalPlaybackFlags;

impl ExperimentalPlaybackFlags {
    /// [실험] 치지직 공식 타임머신 재생.
    pub fn server_time_machine() -> bool {
        SERVER_TIME_MACHINE.load(Ordering::Relaxed)
    }

    pub fn set_server_time_machine(value: bool) {
        SERVER_TIME_MACHINE.store(value, Ordering::Relaxed);
    }

    /// 설정 저장소가 기록하는 웹 동기화 v2 **설정 값**(덮어쓰기 적용 전).
    pub fn match_web_v2_setting_value() -> bool {
        MATCH_WEB_V2_SETTING.load(Ordering::Relaxed)
    }

    pub fn set_match_web_v2_setting_value(value: bool) {
        MATCH_WEB_V2_SETTING.store(value, Ordering::Relaxed);
    }

    /// [실험] 웹 동기화 v2 — 읽을 때마다 덮어쓰기(`override_value`)를 적용한다.
    pub fn match_web_v2(override_value: Option<bool>) -> bool {
        super::match_web_v2::MatchWebV2Flag::resolve_enabled(
            Self::match_web_v2_setting_value(),
            override_value,
        )
    }
}

// MARK: - 일반 설정

/// 일반 설정.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub show_in_menu_bar: bool,
    pub notifications_enabled: bool,
    /// [NOTIF-1] 알림 종류별 전역 스위치
    pub notify_on_live_start: bool,
    pub notify_on_category_change: bool,
    pub notify_on_title_change: bool,
    pub auto_refresh_interval: f64,
    pub auto_refresh_enabled: bool,
    pub always_on_top: bool,
    pub restore_window_on_launch: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            show_in_menu_bar: true,
            notifications_enabled: true,
            notify_on_live_start: true,
            notify_on_category_change: false,
            notify_on_title_change: false,
            auto_refresh_interval: 60.0,
            auto_refresh_enabled: true,
            always_on_top: false,
            restore_window_on_launch: true,
        }
    }
}

// MARK: - 외관 설정

/// 사용자 체감 모션 품질.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MotionQualityTier {
    /// implicit spring · stagger · hover · 장식 pulse 모두 off
    Reduced,
    /// 기본 — 게이트 존중
    #[default]
    Balanced,
    /// 게이트가 허용할 때 풍부한 hover·장식 모션
    Rich,
}

impl MotionQualityTier {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Reduced => "절약",
            Self::Balanced => "균형",
            Self::Rich => "풍부",
        }
    }
}

/// 연속 효과의 프레임 상한 (`Sources/CViewCore/Performance/DisplayRefreshRate.swift` 의 `FrameRateMode`).
/// 외관 설정이 저장하는 값이라 여기 둔다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameRateMode {
    /// 60fps 고정.
    #[serde(rename = "fixed60")]
    Fixed60,
    /// 적응형(기본) — 디스플레이 주사율을 따라가되 120fps 를 넘지 않는다.
    #[default]
    Adaptive,
}

impl FrameRateMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Fixed60 => "60Hz 고정",
            Self::Adaptive => "적응형 (최대 120Hz)",
        }
    }
}

/// 사이드바 레이아웃 상수 (`DesignTokens.Layout` 의 사이드바 부분).
/// `max`→`min` 순서는 의도적이다 — NaN 입력이 하한으로 흡수된다(`clamp` 는 NaN 을 그대로 둔다).
#[allow(clippy::manual_clamp)]
pub mod sidebar {
    pub const MIN_WIDTH: f64 = 180.0;
    pub const DEFAULT_WIDTH: f64 = 204.0;
    pub const MAX_WIDTH: f64 = 300.0;
    pub const SCALE_MIN: f64 = 0.8;
    pub const SCALE_DEFAULT: f64 = 1.0;
    pub const SCALE_MAX: f64 = 1.4;

    pub fn clamped_width(width: f64) -> f64 {
        width.max(MIN_WIDTH).min(MAX_WIDTH)
    }

    pub fn clamped_scale(scale: f64) -> f64 {
        scale.max(SCALE_MIN).min(SCALE_MAX)
    }
}

/// 외관 설정.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    pub theme: AppTheme,
    pub transparency_mode: TransparencyMode,
    pub motion_quality: MotionQualityTier,
    pub frame_rate_mode: FrameRateMode,
    #[serde(deserialize_with = "de_sidebar_width")]
    pub sidebar_width: f64,
    #[serde(deserialize_with = "de_sidebar_scale")]
    pub sidebar_scale: f64,
    pub compact_mode: bool,
    #[serde(rename = "maxMemoryMB")]
    pub max_memory_mb: i64,
    pub debug_mode: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: AppTheme::Dark,
            transparency_mode: TransparencyMode::System,
            motion_quality: MotionQualityTier::Balanced,
            frame_rate_mode: FrameRateMode::Adaptive,
            sidebar_width: sidebar::DEFAULT_WIDTH,
            sidebar_scale: sidebar::SCALE_DEFAULT,
            compact_mode: false,
            max_memory_mb: 512,
            debug_mode: false,
        }
    }
}

fn de_sidebar_width<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(sidebar::clamped_width(f64::deserialize(d)?))
}

fn de_sidebar_scale<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(sidebar::clamped_scale(f64::deserialize(d)?))
}

// MARK: - 네트워크 설정

/// 네트워크 설정. 누락된 신규 키는 기본값만 채워 기존 사용자의 네트워크 설정을 보존한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub request_rate_limit: i64,
    #[serde(rename = "maxConcurrentAPIRequests")]
    pub max_concurrent_api_requests: i64,
    pub api_resource_timeout: i64,
    pub pause_refresh_when_offline: bool,
    pub reduce_background_activity: bool,
    pub cache_expiry: i64,
    pub retry_count: i64,
    pub max_reconnect_attempts: i64,
    pub auto_reconnect: bool,
    pub connection_timeout: i64,
    pub stream_connection_timeout: i64,
    pub reconnect_base_delay: f64,
    pub max_connections_per_host: i64,
    pub force_stream_proxy: bool,
    /// 자동 프리셋 — 켜져 있으면 앱이 시청 상황에 맞춰 위 값들을 프리셋으로 바꿔 넣는다.
    pub auto_preset: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            request_rate_limit: 10,
            max_concurrent_api_requests: 4,
            api_resource_timeout: 12,
            pause_refresh_when_offline: true,
            reduce_background_activity: true,
            cache_expiry: 300,
            retry_count: 3,
            max_reconnect_attempts: 10,
            auto_reconnect: true,
            connection_timeout: 15,
            stream_connection_timeout: 10,
            reconnect_base_delay: 1.0,
            max_connections_per_host: 6,
            force_stream_proxy: true,
            auto_preset: false,
        }
    }
}

impl NetworkSettings {
    /// 구 저장본·외부 복원값도 실행 계층의 안전 범위로 제한한다.
    pub fn effective_api_rate_limit(&self) -> i64 {
        self.request_rate_limit.clamp(1, 30)
    }

    pub fn effective_api_concurrency(&self) -> i64 {
        self.max_concurrent_api_requests.clamp(1, 12)
    }

    /// API 타임아웃(초, 5~60).
    pub fn effective_api_timeout(&self) -> i64 {
        self.api_resource_timeout.clamp(5, 60)
    }

    pub fn effective_api_attempts(&self) -> i64 {
        self.retry_count.clamp(1, 10)
    }

    /// 프리셋에 정의된 값과 현재 값이 같은지 비교하여 프리셋 자동 감지.
    /// 자동 프리셋이 켜져 있으면 값과 상관없이 `Auto`.
    pub fn matching_preset(&self) -> NetworkPreset {
        if self.auto_preset {
            return NetworkPreset::Auto;
        }
        NetworkPreset::ALL
            .into_iter()
            .filter(|p| !matches!(p, NetworkPreset::Custom | NetworkPreset::Auto))
            .find(|p| *self == p.settings())
            .unwrap_or(NetworkPreset::Custom)
    }
}

/// 네트워크 프리셋.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkPreset {
    Auto,
    Balanced,
    Stability,
    LowLatency,
    Performance,
    Custom,
}

impl NetworkPreset {
    pub const ALL: [NetworkPreset; 6] = [
        Self::Auto,
        Self::Balanced,
        Self::Stability,
        Self::LowLatency,
        Self::Performance,
        Self::Custom,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Auto => "자동",
            Self::Balanced => "밸런스",
            Self::Stability => "안정 우선",
            Self::LowLatency => "저지연 우선",
            Self::Performance => "고성능",
            Self::Custom => "커스텀",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Auto => "시청 상황에 맞춰 밸런스·고성능·안정 우선을 고름",
            Self::Balanced => "일반 시청에 적합한 기본 설정",
            Self::Stability => "느린 네트워크에서도 끊김 없는 안정적인 연결",
            Self::LowLatency => "빠른 응답 우선. 불안정한 네트워크에서 끊김 가능",
            Self::Performance => "멀티라이브 등 다수 동시 스트림에 최적화",
            Self::Custom => "수동으로 각 항목을 직접 조정",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Auto => "wand.and.stars",
            Self::Balanced => "scale.3d",
            Self::Stability => "shield.checkered",
            Self::LowLatency => "bolt.fill",
            Self::Performance => "gauge.with.dots.needle.67percent",
            Self::Custom => "slider.horizontal.3",
        }
    }

    /// 프리셋 값. `Auto` 는 지금 고른 프리셋을 알 수 없으므로 밸런스 값에 자동 표시만 켠다 —
    /// 실제 값은 [`Self::auto_choice`] 로 고른 프리셋에 `auto_preset` 을 켜서 넣는다.
    pub fn settings(self) -> NetworkSettings {
        let base = NetworkSettings::default();
        match self {
            Self::Auto => NetworkSettings {
                auto_preset: true,
                ..base
            },
            Self::Balanced | Self::Custom => base,
            Self::Stability => NetworkSettings {
                request_rate_limit: 5,
                cache_expiry: 600,
                retry_count: 5,
                max_reconnect_attempts: 20,
                auto_reconnect: true,
                connection_timeout: 30,
                stream_connection_timeout: 20,
                reconnect_base_delay: 2.0,
                max_connections_per_host: 4,
                force_stream_proxy: true,
                ..base
            },
            Self::LowLatency => NetworkSettings {
                request_rate_limit: 20,
                cache_expiry: 60,
                retry_count: 2,
                max_reconnect_attempts: 5,
                auto_reconnect: true,
                connection_timeout: 8,
                stream_connection_timeout: 5,
                reconnect_base_delay: 0.5,
                max_connections_per_host: 10,
                force_stream_proxy: true,
                ..base
            },
            Self::Performance => NetworkSettings {
                request_rate_limit: 15,
                cache_expiry: 600,
                retry_count: 3,
                max_reconnect_attempts: 15,
                auto_reconnect: true,
                connection_timeout: 20,
                stream_connection_timeout: 15,
                reconnect_base_delay: 1.5,
                max_connections_per_host: 16,
                force_stream_proxy: true,
                ..base
            },
        }
    }

    /// 자동 프리셋이 고를 프리셋. 최근 재생 오류가 있으면 안정 우선, 동시 스트림이 둘 이상이면
    /// 고성능, 그 밖에는 밸런스.
    pub fn auto_choice(active_streams: usize, unstable: bool) -> NetworkPreset {
        if unstable {
            Self::Stability
        } else if active_streams >= 2 {
            Self::Performance
        } else {
            Self::Balanced
        }
    }
}

/// 앱 테마.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    System,
    Light,
    #[default]
    Dark,
}

impl AppTheme {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::System => "시스템",
            Self::Light => "라이트",
            Self::Dark => "다크",
        }
    }
}

/// 투명도 모드 — 시스템 "투명도 줄이기" 접근성 설정 존중이 기본.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransparencyMode {
    #[default]
    System,
    Full,
    Reduced,
}

impl TransparencyMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::System => "시스템",
            Self::Full => "선명하게",
            Self::Reduced => "불투명",
        }
    }
}

// MARK: - 채팅 설정 열거형

/// 채팅 표시 모드.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatDisplayMode {
    /// 사이드 패널 (기본)
    #[default]
    Side,
    /// 비디오 오버레이
    Overlay,
    /// 숨김
    Hidden,
}

impl ChatDisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Side => "사이드",
            Self::Overlay => "화면 위",
            Self::Hidden => "숨김",
        }
    }
}

/// 시청·멀티라이브 채팅에 공통으로 적용하는 시각 디자인.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatDesignStyle {
    #[default]
    Unified,
    Chzzk,
    Hybrid,
    Kakao,
    Imessage,
    /// 과거 저장값 및 디자인 비교 렌더에만 남긴다.
    Calm,
    Compact,
    Dashboard,
}

impl ChatDesignStyle {
    pub const SELECTABLE: [ChatDesignStyle; 5] = [
        Self::Unified,
        Self::Chzzk,
        Self::Hybrid,
        Self::Kakao,
        Self::Imessage,
    ];

    pub fn uses_conversation_cards(self) -> bool {
        matches!(self, Self::Unified | Self::Hybrid)
    }

    pub fn uses_event_rail(self) -> bool {
        self.uses_conversation_cards() || self == Self::Dashboard
    }

    pub fn uses_messenger_bubbles(self) -> bool {
        matches!(self, Self::Kakao | Self::Imessage)
    }

    pub fn uses_line_chrome(self) -> bool {
        self == Self::Chzzk || self.uses_messenger_bubbles()
    }

    /// 저장 문자열 → 선택 가능한 스타일(아니면 `Unified`). `ChatSettings` 디코더 규칙.
    pub fn from_saved(raw: Option<&str>) -> Self {
        Self::SELECTABLE
            .into_iter()
            .find(|s| Some(s.raw_value()) == raw)
            .unwrap_or(Self::Unified)
    }

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Unified => "unified",
            Self::Chzzk => "chzzk",
            Self::Hybrid => "hybrid",
            Self::Kakao => "kakao",
            Self::Imessage => "imessage",
            Self::Calm => "calm",
            Self::Compact => "compact",
            Self::Dashboard => "dashboard",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unified => "Chat Lounge",
            Self::Chzzk => "치지직 스타일",
            Self::Hybrid => "Chat Lounge × CHZZK",
            Self::Kakao => "카카오톡형 말풍선",
            Self::Imessage => "iMessage형 말풍선",
            Self::Calm => "A · 차분한 레일",
            Self::Compact => "B · 고밀도 대화",
            Self::Dashboard => "C · 채널·이벤트 관제",
        }
    }
}

/// 채팅 메시지 row 정렬 모드.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatMessageAlignment {
    #[default]
    Auto,
    Leading,
    Trailing,
}

impl ChatMessageAlignment {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Auto => "자동 (자연스러운 흐름)",
            Self::Leading => "왼쪽",
            Self::Trailing => "오른쪽",
        }
    }
}

#[cfg(test)]
mod tests {
    //! `SettingsForwardCompatibilityTests.swift` · `NetworkResourceSettingsTests.swift` 의 계약을 옮겼다.
    use super::*;

    #[test]
    fn empty_json_decodes_to_defaults() {
        assert_eq!(
            serde_json::from_str::<PlayerSettings>("{}").unwrap(),
            PlayerSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<GeneralSettings>("{}").unwrap(),
            GeneralSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<AppearanceSettings>("{}").unwrap(),
            AppearanceSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<NetworkSettings>("{}").unwrap(),
            NetworkSettings::default()
        );
    }

    #[test]
    fn present_but_wrong_type_is_an_error() {
        assert!(serde_json::from_str::<PlayerSettings>(r#"{"autoPlay":"yes"}"#).is_err());
        assert!(serde_json::from_str::<NetworkSettings>(r#"{"retryCount":"3"}"#).is_err());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let s: PlayerSettings =
            serde_json::from_str(r#"{"preferredEngine":"VLC","audioDelay":1.0,"volumeLevel":0.5}"#)
                .unwrap();
        assert_eq!(s.volume_level, 0.5);
    }

    #[test]
    fn player_settings_round_trip() {
        let mut s = PlayerSettings::default();
        s.apply_latency_preset(LatencyPreset::UltraLow);
        s.equalizer_preset = Some("록".into());
        let back: PlayerSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn player_settings_clamps_on_decode() {
        let s: PlayerSettings = serde_json::from_str(
            r#"{"latencyPidKd":0.9,"streamAlertScale":3.0,"liveRewindWindowMinutes":45,"liveRewindDiskBudgetMB":1}"#,
        )
        .unwrap();
        assert_eq!(s.latency_pid_kd, 0.5);
        assert_eq!(s.stream_alert_scale, 1.7);
        assert_eq!(s.live_rewind_window_minutes, 30);
        assert_eq!(s.live_rewind_disk_budget_mb, 2_048);
    }

    #[test]
    fn removed_proxy_mode_is_absorbed() {
        let s: PlayerSettings =
            serde_json::from_str(r#"{"streamProxyMode":"directVLCAdaptive"}"#).unwrap();
        assert_eq!(s.stream_proxy_mode, StreamProxyMode::LocalProxy);
    }

    #[test]
    fn latency_presets() {
        let mut s = PlayerSettings::default();
        assert_eq!(s.current_preset(), LatencyPreset::MatchWeb);
        s.apply_latency_preset(LatencyPreset::WebSync);
        assert_eq!(s.latency_preset, "webSync");
        assert_eq!(s.latency_target, 6.0);
        let before = s.clone();
        s.apply_latency_preset(LatencyPreset::Custom);
        assert_eq!(s.latency_preset, "custom");
        assert_eq!(s.latency_target, before.latency_target);
        s.latency_preset = "bogus".into();
        assert_eq!(s.current_preset(), LatencyPreset::MatchWeb);
        assert_eq!(LatencyPreset::Standard.raw_value(), "default");
    }

    #[test]
    fn network_effective_values() {
        let s = NetworkSettings {
            request_rate_limit: 100,
            max_concurrent_api_requests: 0,
            api_resource_timeout: 1,
            retry_count: 50,
            ..NetworkSettings::default()
        };
        assert_eq!(s.effective_api_rate_limit(), 30);
        assert_eq!(s.effective_api_concurrency(), 1);
        assert_eq!(s.effective_api_timeout(), 5);
        assert_eq!(s.effective_api_attempts(), 10);
    }

    #[test]
    fn network_presets_detect() {
        assert_eq!(
            NetworkSettings::default().matching_preset(),
            NetworkPreset::Balanced
        );
        assert_eq!(
            NetworkPreset::Stability.settings().matching_preset(),
            NetworkPreset::Stability
        );
        assert_eq!(
            NetworkPreset::Performance.settings().matching_preset(),
            NetworkPreset::Performance
        );
        let custom = NetworkSettings {
            retry_count: 7,
            ..NetworkSettings::default()
        };
        assert_eq!(custom.matching_preset(), NetworkPreset::Custom);
        let auto = NetworkSettings {
            auto_preset: true,
            ..NetworkPreset::Performance.settings()
        };
        assert_eq!(auto.matching_preset(), NetworkPreset::Auto);
    }

    #[test]
    fn network_auto_choice() {
        assert_eq!(
            NetworkPreset::auto_choice(0, false),
            NetworkPreset::Balanced
        );
        assert_eq!(
            NetworkPreset::auto_choice(1, false),
            NetworkPreset::Balanced
        );
        assert_eq!(
            NetworkPreset::auto_choice(3, false),
            NetworkPreset::Performance
        );
        assert_eq!(
            NetworkPreset::auto_choice(3, true),
            NetworkPreset::Stability
        );
        // 구 저장본(키 없음)은 자동이 꺼진 채로 읽힌다.
        let old: NetworkSettings = serde_json::from_str(r#"{"retryCount":3}"#).unwrap();
        assert!(!old.auto_preset);
    }

    #[test]
    fn appearance_clamps_sidebar() {
        let a: AppearanceSettings =
            serde_json::from_str(r#"{"sidebarWidth":999,"sidebarScale":0.1}"#).unwrap();
        assert_eq!(a.sidebar_width, sidebar::MAX_WIDTH);
        assert_eq!(a.sidebar_scale, sidebar::SCALE_MIN);
        assert_eq!(
            serde_json::to_value(FrameRateMode::Fixed60).unwrap(),
            "fixed60"
        );
    }

    #[test]
    fn chat_design_style_saved_values() {
        assert_eq!(
            ChatDesignStyle::from_saved(Some("kakao")),
            ChatDesignStyle::Kakao
        );
        assert_eq!(
            ChatDesignStyle::from_saved(Some("calm")),
            ChatDesignStyle::Unified
        );
        assert_eq!(ChatDesignStyle::from_saved(None), ChatDesignStyle::Unified);
        assert!(ChatDesignStyle::Kakao.uses_line_chrome());
        assert!(ChatDesignStyle::Dashboard.uses_event_rail());
    }

    #[test]
    fn experimental_flags() {
        ExperimentalPlaybackFlags::set_server_time_machine(true);
        assert!(ExperimentalPlaybackFlags::server_time_machine());
        ExperimentalPlaybackFlags::set_server_time_machine(false);
        assert!(!ExperimentalPlaybackFlags::match_web_v2(Some(false)));
    }

    #[test]
    fn screenshot_format() {
        assert_eq!(ScreenshotFormat::Jpeg.file_extension(), "jpeg");
        assert_eq!(
            serde_json::to_string(&ScreenshotFormat::Png).unwrap(),
            "\"png\""
        );
    }
}
