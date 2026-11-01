//! 팔로잉 폴의 값 타입 — 사유·결과·표면·홈 반영/저장 스로틀·새 라이브 계측 줄.
//!
//! 원본: `Sources/CViewCore/Utilities/FollowingPollTypes.swift`
//!
//! 폴 자체(네트워크·상세 팬아웃·병합·싱크 호출)는 앱 계층이 한다. 여기에는 시계·네트워크 없이
//! 숫자만으로 판정하는 부분을 둔다. 상세 선택 조회 규칙은 [`super::live_detail_refresh`].
//!
//! 모델 의존 분리: 원본은 `LiveChannelItem` 목록을 직접 다룬다. 여기서는 항목 타입을
//! [`FollowingPublishItem`] trait 으로 받는 제네릭으로 두었다(병합 결과
//! [`MergedFollowingItem`] 에는 구현이 들어 있다). `FollowingPollFailure.isUnauthorized` 는
//! `APIError` 모델에 의존하므로 옮기지 않았다 — 오류 타입 `E` 쪽에서 판정한다.
//!
//! ⚠ 계측 줄(`live_detected`, E8)에는 채널 ID·이름을 싣지 않는다. 사유와 시간만.

use std::collections::{BTreeMap, HashSet};

use super::live_detail_refresh::{LiveDetailRequest, MergedFollowingItem};
use super::time::{interval_since, Date, TimeInterval};

// MARK: - 사유

/// 팔로잉 폴을 부른 이유. 저장·홈 반영·알림 기준선·상세 동시 수가 이것으로 갈린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FollowingPollReason {
    /// 부팅 인증 뒤 첫 조회. 알림기는 기준선으로만 받는다.
    Boot,
    /// 명시적 로그인 직후 조회. 알림기는 기준선으로만 받는다.
    Login,
    /// 알림 주기(설정 간격).
    Scheduled,
    /// 화면 진입·경량 새로고침. 마지막 성공이 60초 안이면 네트워크 없이 그 결과를 쓴다.
    ScreenEntry,
    /// 사용자 새로고침. 진행 중인 폴이 manual 이 아니면 끝난 뒤 한 번 더 돈다.
    Manual { invalidate_thumbnails: bool },
}

impl FollowingPollReason {
    pub fn is_manual(self) -> bool {
        matches!(self, Self::Manual { .. })
    }

    /// 알림기가 이벤트 없이 기준선으로만 받는 사유 — 첫 폴은 원래 알림을 내지 않는다.
    pub fn seeds_baseline(self) -> bool {
        matches!(self, Self::Boot | Self::Login)
    }

    /// 홈이 180초 저장 스로틀 없이 바로 저장하는 사유(사용자가 결과를 기다리는 경로).
    pub fn persists_immediately(self) -> bool {
        self.is_manual() || self.seeds_baseline()
    }

    /// 계측 줄의 `path` 값.
    pub fn capture_path(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Login => "login",
            Self::Scheduled => "scheduled",
            Self::ScreenEntry => "screen_entry",
            Self::Manual { .. } => "manual",
        }
    }
}

// MARK: - 항목 추상화

/// 홈 반영 스로틀이 비교하는 팔로잉 목록 항목(원본 `LiveChannelItem`).
pub trait FollowingPublishItem {
    /// 방송 중인가(상세가 종료를 알린 행은 false).
    fn is_live(&self) -> bool;
    /// 시청자 수 밖의 모든 표시 값이 같은가(id·channelId·라이브 여부·제목·카테고리·유형·
    /// 채널명·채널 이미지·썸네일·openDate).
    fn same_except_viewers(&self, other: &Self) -> bool;
}

impl FollowingPublishItem for MergedFollowingItem {
    fn is_live(&self) -> bool {
        self.is_live
    }

    fn same_except_viewers(&self, o: &Self) -> bool {
        self.id == o.id
            && self.channel_id == o.channel_id
            && self.is_live == o.is_live
            && self.live_title == o.live_title
            && self.category_name == o.category_name
            && self.category_type == o.category_type
            && self.channel_name == o.channel_name
            && self.channel_image_url == o.channel_image_url
            && self.thumbnail_url == o.thumbnail_url
            && self.open_date == o.open_date
            && self.verified_badge == o.verified_badge
    }
}

// MARK: - 결과

/// 성공한 폴 한 번의 결과. 싱크(홈·알림기)가 같은 값을 받는다.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowingPollResult<T> {
    pub reason: FollowingPollReason,
    /// 목록이 라이브라고 한 행 먼저(API 순서), 그다음 오프라인 행(API 순서).
    pub channels: Vec<T>,
    /// 가장 오래된 목록 페이지를 받은 시각.
    pub fetched_at: Date,
    /// 스토어의 계정 세대. 경계 뒤에 도착한 결과를 싱크가 버리는 데 쓴다.
    pub generation: u64,
    /// 직전 폴에 라이브가 아니던 채널 중 이번에 라이브인 것. 기준선 폴이면 비어 있다.
    pub newly_live: HashSet<String>,
    /// 직전 폴이 없었다(부팅·경계 뒤 첫 성공) — newly_live 를 판정할 수 없다.
    pub is_baseline: bool,
    /// 목록이 제목·시청자를 실어 왔는가.
    pub list_carries_live_info: bool,
    /// 이번 폴에 받은(또는 받으려 한) 상세 — 우선순위 순.
    pub detail_requests: Vec<LiveDetailRequest>,
}

impl<T: FollowingPublishItem> FollowingPollResult<T> {
    /// 나머지 필드는 원본 기본값(비어 있음/거짓).
    pub fn new(
        reason: FollowingPollReason,
        channels: Vec<T>,
        fetched_at: Date,
        generation: u64,
    ) -> Self {
        Self {
            reason,
            channels,
            fetched_at,
            generation,
            newly_live: HashSet::new(),
            is_baseline: false,
            list_carries_live_info: false,
            detail_requests: Vec::new(),
        }
    }

    /// 방송 중인 행 수(상세가 종료를 알린 행은 빼고).
    pub fn live_count(&self) -> usize {
        self.channels.iter().filter(|c| c.is_live()).count()
    }
}

/// 실패한 폴. 취소·계정 경계 뒤 실패는 만들지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowingPollFailure<E> {
    pub reason: FollowingPollReason,
    pub generation: u64,
    pub error: E,
}

/// 싱크가 받는 폴 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum FollowingPollOutcome<T, E> {
    Success(FollowingPollResult<T>),
    Failure(FollowingPollFailure<E>),
}

impl<T, E> FollowingPollOutcome<T, E> {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Success(r) => r.generation,
            Self::Failure(f) => f.generation,
        }
    }
}

/// liveDetail 을 120초 기한으로 받는 "보이는 카드" 의 출처. 표면마다 따로 등록하고 합집합을 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FollowingVisibleSurface {
    HomeRail,
    OverviewPage,
    ChannelDock,
    MenuBarPopover,
}

// MARK: - 홈 반영 스로틀

/// 홈 반영을 묶는다 — 시청자 수만 바뀌었거나 완전히 같으면 `VIEWER_ONLY_INTERVAL` 에 한 번만.
/// 사용자 새로고침·부팅·로그인은 늘 반영한다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FollowingPublishThrottle {
    pub last_publish_at: Option<Date>,
}

impl FollowingPublishThrottle {
    pub const VIEWER_ONLY_INTERVAL: TimeInterval = 120.0;

    pub fn new(last_publish_at: Option<Date>) -> Self {
        Self { last_publish_at }
    }

    /// 이번 결과를 홈에 반영할지. 반영하면 시각을 기록한다. 시계가 거꾸로 가면 기한이 지난 것으로 본다.
    pub fn should_publish<T: FollowingPublishItem>(
        &mut self,
        current: &[T],
        next: &[T],
        reason: FollowingPollReason,
        now: Date,
    ) -> bool {
        let publish = if reason.persists_immediately() || !Self::same_except_viewers(current, next)
        {
            true
        } else if let Some(last) = &self.last_publish_at {
            let age = interval_since(&now, last);
            !(0.0..Self::VIEWER_ONLY_INTERVAL).contains(&age)
        } else {
            true
        };
        if publish {
            self.last_publish_at = Some(now);
        }
        publish
    }

    pub fn reset(&mut self) {
        self.last_publish_at = None;
    }

    /// 순서·구성과 시청자 수 밖의 모든 값이 같은가(완전히 같아도 참).
    pub fn same_except_viewers<T: FollowingPublishItem>(lhs: &[T], rhs: &[T]) -> bool {
        lhs.len() == rhs.len() && lhs.iter().zip(rhs).all(|(a, b)| a.same_except_viewers(b))
    }
}

// MARK: - 저장 스로틀

/// 홈의 팔로잉 스냅샷 저장 간격.
pub struct FollowingPersistThrottle;

impl FollowingPersistThrottle {
    pub const MIN_INTERVAL: TimeInterval = 180.0;

    pub fn should_persist(
        reason: FollowingPollReason,
        last_persist_at: Option<&Date>,
        now: &Date,
    ) -> bool {
        if reason.persists_immediately() {
            return true;
        }
        let Some(last) = last_persist_at else {
            return true;
        };
        let age = interval_since(now, last);
        !(0.0..Self::MIN_INTERVAL).contains(&age)
    }
}

// MARK: - 새 라이브 계측(E8)

/// `PerfCaptureLog.fixed` — 소수 셋째 자리 고정.
fn fixed(value: f64) -> String {
    format!("{value:.3}")
}

/// `PerfCaptureLog.ms` — 초를 ms 로 바꿔 [`fixed`].
fn ms(seconds: f64) -> String {
    fixed(seconds * 1_000.0)
}

/// `live_detected` 한 줄 — 새 라이브 한 채널마다. 시각은 모두 폴 시작 기준(초)이다.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDetectedTiming {
    /// 방송 시작(openDate)부터 싱크 전달까지. openDate 를 모르면 싣지 않는다.
    pub lag_seconds: Option<TimeInterval>,
    /// 목록을 받은 시점.
    pub detect_seconds: TimeInterval,
    /// 그 채널 상세를 받은 시점(받지 않았으면 없음).
    pub detail_seconds: Option<TimeInterval>,
    /// 싱크 전달이 끝난 시점.
    pub notify_seconds: TimeInterval,
    /// 직전 폴 시작과의 간격(부팅 첫 폴이면 없음).
    pub poll_seconds: Option<TimeInterval>,
    pub path: String,
    pub carries_live_info: bool,
}

impl LiveDetectedTiming {
    /// 캡처 줄 필드(E8 이름 그대로).
    pub fn fields(&self) -> BTreeMap<&'static str, String> {
        let mut f = BTreeMap::from([
            ("detect_ms", ms(self.detect_seconds)),
            ("notify_ms", ms(self.notify_seconds)),
            ("path", self.path.clone()),
            (
                "carries_live_info",
                if self.carries_live_info { "1" } else { "0" }.to_string(),
            ),
        ]);
        if let Some(lag) = self.lag_seconds {
            f.insert("lag_ms", ms(lag.max(0.0)));
        }
        if let Some(detail) = self.detail_seconds {
            f.insert("detail_ms", ms(detail));
        }
        if let Some(poll) = self.poll_seconds {
            f.insert("poll_s", fixed(poll));
        }
        f
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/FollowingPollTypesTests.swift`
    use super::*;
    use crate::utils::time::{adding, from_unix};
    use FollowingPollReason as Reason;

    fn base() -> Date {
        from_unix(1_800_000_000.0)
    }

    fn at(s: f64) -> Date {
        adding(&base(), s)
    }

    fn item(id: &str) -> MergedFollowingItem {
        MergedFollowingItem {
            id: id.into(),
            channel_name: format!("name-{id}"),
            channel_image_url: None,
            live_title: "t".into(),
            viewer_count: 100,
            category_name: Some("cat".into()),
            category_type: None,
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: true,
            open_date: None,
            verified_badge: None,
        }
    }

    fn viewers(id: &str, v: i64) -> MergedFollowingItem {
        MergedFollowingItem {
            viewer_count: v,
            ..item(id)
        }
    }

    const MANUAL: Reason = Reason::Manual {
        invalidate_thumbnails: false,
    };

    #[test]
    fn reason_properties() {
        let all = [
            Reason::Boot,
            Reason::Login,
            Reason::Scheduled,
            Reason::ScreenEntry,
            MANUAL,
        ];
        let seeds: Vec<_> = all.iter().copied().filter(|r| r.seeds_baseline()).collect();
        let immediate: Vec<_> = all
            .iter()
            .copied()
            .filter(|r| r.persists_immediately())
            .collect();
        let manuals: Vec<_> = all.iter().copied().filter(|r| r.is_manual()).collect();
        let paths: Vec<_> = all.iter().map(|r| r.capture_path()).collect();
        assert_eq!(seeds, vec![Reason::Boot, Reason::Login]);
        assert_eq!(immediate, vec![Reason::Boot, Reason::Login, MANUAL]);
        assert_eq!(manuals, vec![MANUAL]);
        assert!(Reason::Manual {
            invalidate_thumbnails: true
        }
        .is_manual());
        assert_eq!(
            paths,
            ["boot", "login", "scheduled", "screen_entry", "manual"]
        );
        assert_ne!(
            Reason::Manual {
                invalidate_thumbnails: true
            },
            MANUAL
        );
    }

    #[test]
    fn result_live_count_skips_closed_rows() {
        let r = FollowingPollResult::new(
            Reason::Scheduled,
            vec![
                item("a"),
                MergedFollowingItem {
                    is_live: false,
                    ..item("b")
                },
                item("c"),
            ],
            base(),
            3,
        );
        assert_eq!(r.live_count(), 2);
        assert!(!r.is_baseline);
        assert!(r.newly_live.is_empty() && r.detail_requests.is_empty());
    }

    #[test]
    fn outcome_generation() {
        let failure: FollowingPollOutcome<MergedFollowingItem, &str> =
            FollowingPollOutcome::Failure(FollowingPollFailure {
                reason: Reason::Scheduled,
                generation: 0,
                error: "x",
            });
        assert_eq!(failure.generation(), 0);
        let success: FollowingPollOutcome<MergedFollowingItem, &str> =
            FollowingPollOutcome::Success(FollowingPollResult::new(
                Reason::Boot,
                vec![],
                base(),
                7,
            ));
        assert_eq!(success.generation(), 7);
    }

    #[test]
    fn viewer_only_changes_publish_every_120_seconds() {
        let mut throttle = FollowingPublishThrottle::default();
        let mut current = vec![viewers("a", 100)];
        let mut publish = |next: Vec<MergedFollowingItem>, s: f64| {
            let r = throttle.should_publish(&current, &next, Reason::Scheduled, at(s));
            if r {
                current = next;
            }
            r
        };
        assert!(publish(vec![viewers("a", 100)], 0.0));
        assert!(!publish(vec![viewers("a", 300)], 60.0));
        assert!(!publish(vec![viewers("a", 400)], 119.9));
        assert!(publish(vec![viewers("a", 500)], 120.0));
        assert!(!publish(vec![viewers("a", 500)], 180.0));
        assert!(publish(vec![viewers("a", 500)], 240.0));
    }

    #[test]
    fn structural_changes_publish_immediately() {
        let start = vec![item("a"), item("b")];
        let b = item("b");
        let changes: Vec<(&str, Vec<MergedFollowingItem>)> = vec![
            ("구성", vec![item("a"), item("b"), item("c")]),
            ("순서", vec![item("b"), item("a")]),
            (
                "라이브 여부",
                vec![
                    item("a"),
                    MergedFollowingItem {
                        is_live: false,
                        ..b.clone()
                    },
                ],
            ),
            (
                "제목",
                vec![
                    item("a"),
                    MergedFollowingItem {
                        live_title: "new".into(),
                        ..b.clone()
                    },
                ],
            ),
            (
                "카테고리",
                vec![
                    item("a"),
                    MergedFollowingItem {
                        category_name: Some("other".into()),
                        ..b.clone()
                    },
                ],
            ),
            (
                "썸네일",
                vec![
                    item("a"),
                    MergedFollowingItem {
                        thumbnail_url: Some("https://t/1".into()),
                        ..b.clone()
                    },
                ],
            ),
            (
                "방송 시작",
                vec![
                    item("a"),
                    MergedFollowingItem {
                        open_date: Some(base()),
                        ..b.clone()
                    },
                ],
            ),
        ];
        for (label, next) in changes {
            let mut t = FollowingPublishThrottle::new(Some(base()));
            assert!(
                t.should_publish(&start, &next, Reason::Scheduled, at(10.0)),
                "{label} 변경이 스로틀에 걸렸다"
            );
        }
        assert!(FollowingPublishThrottle::same_except_viewers(
            &start,
            &[viewers("a", 1), viewers("b", 2)]
        ));
    }

    #[test]
    fn immediate_reasons_and_clock_rollback() {
        let same = vec![item("a")];
        for reason in [MANUAL, Reason::Boot, Reason::Login] {
            let mut t = FollowingPublishThrottle::new(Some(base()));
            assert!(
                t.should_publish(&same, &same, reason, at(1.0)),
                "{reason:?}"
            );
        }
        let mut t = FollowingPublishThrottle::new(Some(base()));
        assert!(t.should_publish(&same, &same, Reason::Scheduled, at(-30.0)));
        assert_eq!(t.last_publish_at, Some(at(-30.0)));
        t.reset();
        assert_eq!(t.last_publish_at, None);
    }

    #[test]
    fn persist_throttle() {
        let should = |reason: Reason, last: Option<f64>, now: f64| {
            FollowingPersistThrottle::should_persist(reason, last.map(at).as_ref(), &at(now))
        };
        assert!(should(Reason::Scheduled, None, 0.0));
        assert!(!should(Reason::Scheduled, Some(0.0), 60.0));
        assert!(!should(Reason::Scheduled, Some(0.0), 179.9));
        assert!(should(Reason::Scheduled, Some(0.0), 180.0));
        assert!(!should(Reason::ScreenEntry, Some(0.0), 120.0));
        assert!(should(MANUAL, Some(0.0), 1.0));
        assert!(should(Reason::Boot, Some(0.0), 1.0));
        assert!(should(Reason::Login, Some(0.0), 1.0));
        assert!(should(Reason::Scheduled, Some(100.0), 50.0), "되감긴 시계");
        assert_eq!(FollowingPersistThrottle::MIN_INTERVAL, 180.0);
        assert_eq!(FollowingPublishThrottle::VIEWER_ONLY_INTERVAL, 120.0);
    }

    #[test]
    fn live_detected_fields() {
        let full = LiveDetectedTiming {
            lag_seconds: Some(12.5),
            detect_seconds: 0.4,
            detail_seconds: Some(0.6),
            notify_seconds: 0.7,
            poll_seconds: Some(60.0),
            path: Reason::Scheduled.capture_path().into(),
            carries_live_info: false,
        };
        let expected: BTreeMap<&str, String> = [
            ("lag_ms", "12500.000"),
            ("detect_ms", "400.000"),
            ("detail_ms", "600.000"),
            ("notify_ms", "700.000"),
            ("poll_s", "60.000"),
            ("path", "scheduled"),
            ("carries_live_info", "0"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
        assert_eq!(full.fields(), expected);

        let partial = LiveDetectedTiming {
            lag_seconds: None,
            detect_seconds: 1.0,
            detail_seconds: None,
            notify_seconds: 2.0,
            poll_seconds: None,
            path: Reason::Manual {
                invalidate_thumbnails: true,
            }
            .capture_path()
            .into(),
            carries_live_info: true,
        };
        let pf = partial.fields();
        assert_eq!(
            pf.keys().copied().collect::<Vec<_>>(),
            ["carries_live_info", "detect_ms", "notify_ms", "path"]
        );
        assert_eq!(pf["path"], "manual");
        assert_eq!(pf["carries_live_info"], "1");

        let future = LiveDetectedTiming {
            lag_seconds: Some(-3.0),
            detect_seconds: 0.0,
            detail_seconds: None,
            notify_seconds: 0.0,
            poll_seconds: None,
            path: "boot".into(),
            carries_live_info: false,
        };
        assert_eq!(future.fields()["lag_ms"], "0.000");
        assert!(!full.fields().keys().any(|k| k.contains("channel")));
    }
}
