//! `Sources/CViewApp/Actions/ClipboardActions.swift`, `SupportActions.swift`,
//! `SystemSettingsActions.swift` 포팅.
//!
//! AppKit(`NSPasteboard`·`NSWorkspace`·`ToastCenter`) 호출은 [`Clipboard`]·[`UrlOpener`]·
//! [`Toaster`] trait 뒤로 옮겼다. 복사와 고지를 분리할 수 없게 묶는 규칙과 지원 이슈 본문
//! 구성(로그 내용은 넣지 않는다)은 그대로다.

use cview_persistence::StoreOpenHealth;

/// 시스템 클립보드.
pub trait Clipboard {
    /// 텍스트를 쓴다. 성공하면 true.
    fn set_string(&mut self, text: &str) -> bool;
}

/// 사용자 고지(토스트).
pub trait Toaster {
    fn success(&mut self, message: &str);
    fn error(&mut self, message: &str);
}

/// 외부 URL 열기(`NSWorkspace.shared.open`). 열었으면 true.
pub trait UrlOpener {
    fn open(&mut self, url: &str) -> bool;
}

/// 복사 동작의 단일 진입점.
pub struct ClipboardAction;

impl ClipboardAction {
    /// 텍스트를 복사하고 결과를 알린다. `subject` 는 "링크"·"닉네임" 같은 대상 이름이다.
    pub fn copy(
        text: &str,
        subject: &str,
        clipboard: &mut dyn Clipboard,
        toaster: &mut dyn Toaster,
    ) {
        if text.is_empty() {
            toaster.error(&format!("복사할 {subject}이(가) 없습니다"));
            return;
        }
        if clipboard.set_string(text) {
            toaster.success(&format!("{subject} 복사됨"));
        } else {
            toaster.error(&format!("{subject} 복사에 실패했습니다"));
        }
    }
}

/// 지원·문제 보고 진입점.
pub struct SupportActions;

impl SupportActions {
    /// 릴리스 저장소. `SupportFiles/ReleaseContract.json` 의 `repositorySlug` 와 같아야 한다.
    pub const REPOSITORY_SLUG: &'static str = "dododo-lab/cview-v2";

    /// 열기 실패 기록이 있으면 '저장소 열기 실패 N회 (domain code)', 없으면 None.
    pub fn store_open_line(health: &StoreOpenHealth) -> Option<String> {
        if health.failures <= 0 {
            return None;
        }
        match &health.last_error {
            None => Some(format!("저장소 열기 실패 {}회", health.failures)),
            Some(e) => Some(format!(
                "저장소 열기 실패 {}회 ({} {})",
                health.failures, e.domain, e.code
            )),
        }
    }

    /// 이슈 본문 — 환경 정보만 싣고 로그 내용은 넣지 않는다.
    pub fn issue_body(version: &str, build: &str, os: &str, health: &StoreOpenHealth) -> String {
        let store_line = Self::store_open_line(health)
            .map(|l| format!("\n- {l}"))
            .unwrap_or_default();
        format!(
            "## 무슨 일이 있었나요\n\n\n## 재현 방법\n1.\n2.\n\n## 환경\n- CView {version} (build {build})\n- {os}{store_line}\n\n---\n진단 로그가 필요하면 도움말 › '로컬 로그 폴더 열기' 에서 파일을 첨부해 주세요.\n(로그에는 채널명·시청 기록이 포함될 수 있으니 확인 후 올려 주세요.)"
        )
    }

    /// 환경 정보가 채워진 GitHub 이슈 작성 URL.
    pub fn report_problem_url(
        version: &str,
        build: &str,
        os: &str,
        health: &StoreOpenHealth,
    ) -> String {
        let body = Self::issue_body(version, build, os, health);
        let mut url = url::Url::parse(&format!(
            "https://github.com/{}/issues/new",
            Self::REPOSITORY_SLUG
        ))
        .expect("정적 URL");
        url.query_pairs_mut().append_pair("body", &body);
        url.to_string()
    }

    /// 이슈 작성 페이지를 연다. 실패하면 이슈 목록이라도 연다.
    pub fn report_problem(
        opener: &mut dyn UrlOpener,
        version: &str,
        build: &str,
        os: &str,
        health: &StoreOpenHealth,
    ) {
        let url = Self::report_problem_url(version, build, os, health);
        if !opener.open(&url) {
            opener.open(&format!(
                "https://github.com/{}/issues",
                Self::REPOSITORY_SLUG
            ));
        }
    }
}

/// 시스템 설정의 특정 창을 여는 액션.
pub struct SystemSettingsActions;

impl SystemSettingsActions {
    /// macOS 알림 설정 딥링크 후보(순서대로 시도) — 전부 실패하면 시스템 설정 최상단.
    pub const NOTIFICATION_CANDIDATES: [&'static str; 2] = [
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
        "x-apple.systempreferences:com.apple.preference.notifications",
    ];

    pub fn open_notifications(opener: &mut dyn UrlOpener) {
        for raw in Self::NOTIFICATION_CANDIDATES {
            if opener.open(raw) {
                return;
            }
        }
        opener.open("x-apple.systempreferences:");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cview_persistence::store_open_health::ErrorSummary;

    #[derive(Default)]
    struct Fake {
        ok: bool,
        toasts: Vec<String>,
        opened: Vec<String>,
    }
    impl Clipboard for Fake {
        fn set_string(&mut self, _: &str) -> bool {
            self.ok
        }
    }
    impl UrlOpener for Fake {
        fn open(&mut self, url: &str) -> bool {
            self.opened.push(url.into());
            self.ok
        }
    }
    struct Toasts(Vec<String>);
    impl Toaster for Toasts {
        fn success(&mut self, m: &str) {
            self.0.push(format!("ok:{m}"));
        }
        fn error(&mut self, m: &str) {
            self.0.push(format!("err:{m}"));
        }
    }

    #[test]
    fn copy_always_announces() {
        let mut cb = Fake {
            ok: true,
            ..Default::default()
        };
        let mut t = Toasts(vec![]);
        ClipboardAction::copy("x", "링크", &mut cb, &mut t);
        ClipboardAction::copy("", "링크", &mut cb, &mut t);
        cb.ok = false;
        ClipboardAction::copy("x", "링크", &mut cb, &mut t);
        assert_eq!(
            t.0,
            vec![
                "ok:링크 복사됨",
                "err:복사할 링크이(가) 없습니다",
                "err:링크 복사에 실패했습니다"
            ]
        );
        assert!(cb.toasts.is_empty());
    }

    #[test]
    fn store_open_line_has_only_counts_and_codes() {
        let mut h = StoreOpenHealth::default();
        assert_eq!(SupportActions::store_open_line(&h), None);
        h.failures = 2;
        assert_eq!(
            SupportActions::store_open_line(&h).as_deref(),
            Some("저장소 열기 실패 2회")
        );
        h.last_error = Some(ErrorSummary {
            domain: "SQLite".into(),
            code: 11,
            underlying_domain: None,
            underlying_code: None,
            sqlite_code: None,
        });
        assert_eq!(
            SupportActions::store_open_line(&h).as_deref(),
            Some("저장소 열기 실패 2회 (SQLite 11)")
        );
        let body = SupportActions::issue_body("1.0", "7", "macOS", &h);
        assert!(body.contains("- CView 1.0 (build 7)\n- macOS\n- 저장소 열기 실패 2회 (SQLite 11)"));
    }

    #[test]
    fn notifications_fall_back_to_settings_root() {
        let mut o = Fake::default();
        SystemSettingsActions::open_notifications(&mut o);
        assert_eq!(o.opened.len(), 3);
        assert_eq!(o.opened[2], "x-apple.systempreferences:");
        let mut ok = Fake {
            ok: true,
            ..Default::default()
        };
        SupportActions::report_problem(&mut ok, "1", "2", "os", &StoreOpenHealth::default());
        assert_eq!(ok.opened.len(), 1);
        assert!(ok.opened[0].starts_with("https://github.com/dododo-lab/cview-v2/issues/new?body="));
    }
}
