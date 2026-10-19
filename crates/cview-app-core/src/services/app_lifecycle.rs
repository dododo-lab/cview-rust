//! `Sources/CViewApp/AppLifecycle.swift` 의 플랫폼 비의존 부분 포팅.
//!
//! 옮긴 것:
//! - 라이브 창 가시성 판정(`liveWindowVisibility`)과 활성 복귀 시 백그라운드 복구 판정
//!   (`needsBackgroundRecovery`·`consumeBackgroundPeriod`) — [`BackgroundPeriod`].
//! - 백그라운드 업데이트 이벤트의 알림 필터(`startBackgroundUpdates` 의 싱크 본문) —
//!   [`filter_background_event`].
//! - 종료 확인에 쓰는 진행 중 작업 설명(`inFlightWorkWarning`) — [`in_flight_work_warning`].
//!
//! 건너뛴 것: NotificationCenter·NSWorkspace 옵저버, App Nap(`ProcessInfo.beginActivity`),
//! 메모리 압박 DispatchSource, 열 상태 GPU 강등, `NSWindow` 순회, `NSApplicationDelegate`
//! 종료 대화상자. 셸(Tauri)이 같은 이벤트를 받아 여기 함수를 부른다.

use cview_core::models::settings::GeneralSettings;
use cview_core::models::settings_ui::ChannelNotificationSettings;
use cview_core::utils::live_detail_refresh::{
    notify_watches, ChannelNotificationLookup, NotifyWatchGeneral,
};
use cview_core::utils::time::{Date, TimeInterval};

use super::background_update::BackgroundUpdateEvent;

// MARK: - 가시성

/// 라이브 콘텐츠 창의 가시성.
///
/// - `Visible`: 보이는 라이브 콘텐츠 창이 실제로 있다 → 복원해도 되는 유일한 상태.
/// - `Occluded`: 창은 화면에 올라와 있으나 가려졌다/최소화됐다 → 합성 정지 대상.
/// - `Indeterminate`: 활성 Space 안에서 창이 order out 되어 판단 근거가 없다(아무 것도 하지 않는다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveContentVisibility {
    Visible,
    Occluded,
    Indeterminate,
}

/// `HLSJSVideoView.isWindowPresentationVisible` 과 같은 식.
pub fn is_window_presentation_visible(
    ordered_on_screen: bool,
    miniaturized: bool,
    on_active_space: bool,
    occlusion_visible: bool,
) -> bool {
    ordered_on_screen && !miniaturized && on_active_space && occlusion_visible
}

pub fn live_window_visibility(
    ordered_on_screen: bool,
    miniaturized: bool,
    on_active_space: bool,
    occlusion_visible: bool,
) -> LiveContentVisibility {
    if is_window_presentation_visible(
        ordered_on_screen,
        miniaturized,
        on_active_space,
        occlusion_visible,
    ) {
        return LiveContentVisibility::Visible;
    }
    // 다른 Space 의 창은 확실히 없다. 현재 Space 에서 order out 된 창만 근거가 모호하다.
    if !ordered_on_screen && !miniaturized && on_active_space {
        return LiveContentVisibility::Indeterminate;
    }
    LiveContentVisibility::Occluded
}

/// 여러 창의 가시성 합성 — 하나라도 보이면 `Visible`, 보이는 창은 없고 모호한 창이 있으면
/// `Indeterminate`, 나머지는 `Occluded`(창이 없을 때 포함).
pub fn combine_visibility(
    windows: impl IntoIterator<Item = LiveContentVisibility>,
) -> LiveContentVisibility {
    let mut saw_indeterminate = false;
    for v in windows {
        match v {
            LiveContentVisibility::Visible => return LiveContentVisibility::Visible,
            LiveContentVisibility::Indeterminate => saw_indeterminate = true,
            LiveContentVisibility::Occluded => {}
        }
    }
    if saw_indeterminate {
        LiveContentVisibility::Indeterminate
    } else {
        LiveContentVisibility::Occluded
    }
}

/// 백그라운드 복구 임계(초). 종전 값 그대로다.
pub const BACKGROUND_RECOVERY_THRESHOLD: TimeInterval = 5.0;

/// 활성 복귀 시 백그라운드 복구가 필요한가.
///
/// **5초 넘게 비활성이었고, 그동안 한 번이라도 안 보였을 때만** 건다. 표면이 숨겨진 상태로
/// 끝났거나 복귀 순간에 아직 보이지 않으면 복구 쪽으로 기운다.
pub fn needs_background_recovery(
    background_duration: TimeInterval,
    hidden_while_inactive: bool,
    surface_suspended: bool,
    visible_now: bool,
) -> bool {
    if background_duration <= BACKGROUND_RECOVERY_THRESHOLD {
        return false;
    }
    hidden_while_inactive || surface_suspended || !visible_now
}

/// 비활성 구간 기록(`_backgroundEntryTime`·`liveContentHiddenWhileInactive`).
#[derive(Debug, Clone, Default)]
pub struct BackgroundPeriod {
    entry_time: Option<Date>,
    hidden_while_inactive: bool,
}

impl BackgroundPeriod {
    pub fn new() -> Self {
        Self::default()
    }

    /// 비활성 진입. 이미 구간 안이면 첫 진입 시각을 지킨다.
    pub fn enter(&mut self, now: Date) {
        if self.entry_time.is_none() {
            self.entry_time = Some(now);
        }
    }

    /// 비활성 중 안 보임을 관측했다.
    pub fn note_hidden(&mut self) {
        if self.entry_time.is_some() {
            self.hidden_while_inactive = true;
        }
    }

    pub fn entry_time(&self) -> Option<Date> {
        self.entry_time
    }

    pub fn hidden_while_inactive(&self) -> bool {
        self.hidden_while_inactive
    }

    /// 비활성 구간 길이(초). 구간이 없으면 0.
    pub fn duration(&self, now: Date) -> TimeInterval {
        self.entry_time
            .map(|t| (now - t).num_milliseconds() as f64 / 1000.0)
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// 활성 복귀 — 복구 필요 여부를 확정하고 구간 기록을 비운다(동기로 판정한다).
    pub fn consume(&mut self, now: Date, surface_suspended: bool, visible_now: bool) -> bool {
        let duration = self.duration(now);
        let needed = needs_background_recovery(
            duration,
            self.hidden_while_inactive,
            surface_suspended,
            visible_now,
        );
        self.entry_time = None;
        self.hidden_while_inactive = false;
        if duration > BACKGROUND_RECOVERY_THRESHOLD && !needed {
            tracing::info!(
                "App became active — stream recovery skipped (stayed visible {duration:.0}s)"
            );
        }
        needed
    }
}

// MARK: - 백그라운드 업데이트 알림 필터

struct ChannelLookup<'a>(&'a ChannelNotificationSettings);

impl ChannelNotificationLookup for ChannelLookup<'_> {
    fn is_title_change_notification_enabled(&self, channel_id: &str) -> bool {
        self.0.is_title_change_notification_enabled(channel_id)
    }
    fn is_category_change_notification_enabled(&self, channel_id: &str) -> bool {
        self.0.is_category_change_notification_enabled(channel_id)
    }
}

/// `LiveDetailNotifyWatch.predicate` — 팔로잉 폴이 매번 상세를 받을 감시 채널 판정.
pub fn notify_watch_predicate<'a>(
    general: &GeneralSettings,
    channels: &'a ChannelNotificationSettings,
) -> impl Fn(&str) -> bool + 'a {
    let watch = NotifyWatchGeneral {
        notifications_enabled: general.notifications_enabled,
        auto_refresh_enabled: general.auto_refresh_enabled,
        notify_on_title_change: general.notify_on_title_change,
        notify_on_category_change: general.notify_on_category_change,
    };
    move |id: &str| notify_watches(id, &watch, &ChannelLookup(channels))
}

/// 백그라운드 이벤트 중 실제로 알릴 것만 남긴다. 알림이 꺼져 있으면 빈 이벤트.
///
/// - 방송 시작: 전역 종류 스위치 + 채널별 설정.
/// - 카테고리·제목: 감시 판정(폴이 상세를 받는 채널과 같은 판정) + 종류 스위치 + 채널별 설정.
pub fn filter_background_event(
    event: &BackgroundUpdateEvent,
    general: &GeneralSettings,
    channels: &ChannelNotificationSettings,
) -> BackgroundUpdateEvent {
    let mut out = BackgroundUpdateEvent::default();
    if !general.notifications_enabled {
        return out;
    }
    let watches = notify_watch_predicate(general, channels);
    if general.notify_on_live_start {
        out.newly_online = event
            .newly_online
            .iter()
            .filter(|c| channels.is_live_notification_enabled(&c.channel_id))
            .cloned()
            .collect();
    }
    if general.notify_on_category_change {
        out.category_changed = event
            .category_changed
            .iter()
            .filter(|c| {
                watches(&c.channel_id)
                    && channels.is_category_change_notification_enabled(&c.channel_id)
            })
            .cloned()
            .collect();
    }
    if general.notify_on_title_change {
        out.title_changed = event
            .title_changed
            .iter()
            .filter(|c| {
                watches(&c.channel_id)
                    && channels.is_title_change_notification_enabled(&c.channel_id)
            })
            .cloned()
            .collect();
    }
    out
}

// MARK: - 종료 확인

/// 종료하면 잃게 되는 진행 중 작업의 설명. 없으면 `None`.
pub fn in_flight_work_warning(
    download_titles: &[String],
    recording_count: usize,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if !download_titles.is_empty() {
        let names = download_titles
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = if download_titles.len() > 2 {
            format!(" 외 {}개", download_titles.len() - 2)
        } else {
            String::new()
        };
        parts.push(format!(
            "다운로드 {}개({names}{more})",
            download_titles.len()
        ));
    }
    if recording_count > 0 {
        parts.push(format!("녹화 {recording_count}건"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests` 의 가시성·백그라운드 복구 판정 테스트.
    use super::*;
    use crate::services::background_update::{ChannelChangeInfo, OnlineChannel};
    use chrono::Duration;
    use cview_core::utils::time::from_unix;

    #[test]
    fn window_visibility_truth_table() {
        use LiveContentVisibility as V;
        assert_eq!(live_window_visibility(true, false, true, true), V::Visible);
        assert_eq!(
            live_window_visibility(true, false, true, false),
            V::Occluded
        );
        assert_eq!(live_window_visibility(true, true, true, true), V::Occluded);
        assert_eq!(
            live_window_visibility(false, false, true, false),
            V::Indeterminate
        );
        assert_eq!(
            live_window_visibility(false, false, false, false),
            V::Occluded
        );
        assert_eq!(
            live_window_visibility(false, true, true, false),
            V::Occluded
        );
        assert_eq!(
            combine_visibility([V::Occluded, V::Indeterminate]),
            V::Indeterminate
        );
        assert_eq!(
            combine_visibility([V::Indeterminate, V::Visible]),
            V::Visible
        );
        assert_eq!(combine_visibility([]), V::Occluded);
    }

    #[test]
    fn background_recovery_rules() {
        assert!(!needs_background_recovery(5.0, true, true, false));
        assert!(!needs_background_recovery(60.0, false, false, true));
        assert!(needs_background_recovery(5.1, true, false, true));
        assert!(needs_background_recovery(60.0, false, true, true));
        assert!(needs_background_recovery(60.0, false, false, false));
    }

    #[test]
    fn background_period_consume_resets() {
        let t0 = from_unix(1_000.0);
        let mut p = BackgroundPeriod::new();
        p.note_hidden();
        assert!(!p.hidden_while_inactive(), "구간 밖 관측은 무시");
        p.enter(t0);
        p.enter(t0 + Duration::seconds(3));
        assert_eq!(p.entry_time(), Some(t0));
        p.note_hidden();
        assert!(p.consume(t0 + Duration::seconds(10), false, true));
        assert_eq!(p.entry_time(), None);
        p.enter(t0);
        assert!(!p.consume(t0 + Duration::seconds(10), false, true));
    }

    fn online(id: &str) -> OnlineChannel {
        OnlineChannel {
            channel_id: id.into(),
            channel_name: id.into(),
            live_title: String::new(),
            viewer_count: 0,
            thumbnail_url: None,
            category_name: None,
            channel_image_url: None,
            open_date: None,
        }
    }

    fn change(id: &str) -> ChannelChangeInfo {
        ChannelChangeInfo {
            channel_id: id.into(),
            channel_name: id.into(),
            old_value: "a".into(),
            new_value: "b".into(),
            channel_image_url: None,
        }
    }

    #[test]
    fn background_event_filter_follows_settings() {
        let event = BackgroundUpdateEvent {
            newly_online: vec![online("a")],
            category_changed: vec![change("a")],
            title_changed: vec![change("a")],
        };
        let channels = ChannelNotificationSettings::default();
        let mut general = GeneralSettings::default();
        // 기본: 방송 시작만 켜짐, 제목·카테고리는 끔.
        let out = filter_background_event(&event, &general, &channels);
        assert_eq!(out.newly_online.len(), 1);
        assert!(out.category_changed.is_empty() && out.title_changed.is_empty());

        general.notify_on_title_change = true;
        let out = filter_background_event(&event, &general, &channels);
        assert_eq!(out.title_changed.len(), 1);
        assert!(out.category_changed.is_empty());

        general.auto_refresh_enabled = false;
        let out = filter_background_event(&event, &general, &channels);
        assert!(
            out.title_changed.is_empty(),
            "감시 판정은 자동 새로고침이 켜져야 참"
        );

        general.notifications_enabled = false;
        assert!(!filter_background_event(&event, &general, &channels).has_any_event());
    }

    #[test]
    fn in_flight_warning_text() {
        assert_eq!(in_flight_work_warning(&[], 0), None);
        let titles = vec!["가".to_string(), "나".to_string(), "다".to_string()];
        assert_eq!(
            in_flight_work_warning(&titles, 2).as_deref(),
            Some("다운로드 3개(가, 나 외 1개) · 녹화 2건")
        );
        assert_eq!(
            in_flight_work_warning(&titles[..1], 0).as_deref(),
            Some("다운로드 1개(가)")
        );
    }
}
