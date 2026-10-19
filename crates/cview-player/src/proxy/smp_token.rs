//! `Sources/CViewPlayer/LocalStreamProxy+SMPToken.swift` 포팅 — [실험] 치지직 공식 타임머신
//! 세그먼트 토큰(`__bgda__`, Akamai Segmented Media Protection).
//!
//! 공식 웹 플레이어는 DVR 세그먼트 요청마다 재생목록 URL 의 `hdnts` 를 원문 그대로 `__bgda__` 로
//! 옮겨 붙인다. 프록시가 그 일을 대신한다. 코디네이터가 **타임머신 스트림일 때만** 켠다.

use url::Url;

/// 프록시 하나(스트림 하나)의 SMP 토큰 상태.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmpSegmentTokenState {
    pub enabled: bool,
    /// 마지막으로 본 재생목록 요청의 `hdnts` 원문(퍼센트 인코딩 그대로).
    pub token: Option<String>,
    /// 세그먼트에 처음 붙인 순간 한 번만 로그를 남긴다.
    pub logged_attach: bool,
}

/// 공식 플레이어 정규식과 같은 대상 확장자.
pub const SMP_SEGMENT_EXTENSIONS: [&str; 6] = [".ts", ".m4v", ".m4a", ".m4i", ".m4s", ".mp4"];
/// 토큰을 읽어 둘 재생목록 확장자.
pub const SMP_MANIFEST_MARKERS: [&str; 3] = [".m3u8", ".multiview", ".mpd"];

impl SmpSegmentTokenState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }

    /// 상류 URL 에 SMP 재작성을 적용한다 — 재생목록이면 토큰을 저장, 세그먼트면 `__bgda__` 를 붙인다.
    pub fn apply(&mut self, url: &Url) -> Url {
        let lower = url.path().to_lowercase();
        let is_manifest = SMP_MANIFEST_MARKERS.iter().any(|m| lower.contains(m));
        let is_segment = !is_manifest && SMP_SEGMENT_EXTENSIONS.iter().any(|e| lower.ends_with(e));
        if !is_manifest && !is_segment {
            return url.clone();
        }
        if is_manifest {
            if let Some(token) = url.query().and_then(hdnts_token) {
                if self.enabled {
                    self.token = Some(token);
                }
            }
            return url.clone();
        }
        let Some(token) = self.token.clone().filter(|_| self.enabled) else {
            return url.clone();
        };
        let first = !self.logged_attach;
        self.logged_attach = true;
        let Some(rewritten) = appending_bgda_token(&token, url) else {
            return url.clone();
        };
        if first {
            let raw = token.contains('*') || token.contains('/');
            tracing::info!(
                "[TM-SMP] 세그먼트에 __bgda__ 부착 시작 (토큰 {}자, 원문 형태={raw})",
                token.chars().count()
            );
        }
        rewritten
    }
}

/// 쿼리 문자열에서 `hdnts` 원문 — 공식 정규식 `hdnts=([^&?#]{1,})`.
pub fn hdnts_token(query: &str) -> Option<String> {
    let pair = query.split('&').find(|p| p.starts_with("hdnts="))?;
    let value = &pair[6..];
    let end = value.find(['?', '#']).unwrap_or(value.len());
    let token = &value[..end];
    (!token.is_empty()).then(|| token.to_string())
}

/// `__bgda__=<원문>` 을 붙인 URL. 이미 있으면 `None`.
pub fn appending_bgda_token(token: &str, url: &Url) -> Option<Url> {
    let query = url.query();
    if query.is_some_and(|q| q.split('&').any(|p| p.starts_with("__bgda__="))) {
        return None;
    }
    let mut text = url.as_str().to_string();
    if let Some(hash) = text.find('#') {
        text.truncate(hash);
    }
    let sep = if query.is_some_and(|q| !q.is_empty()) {
        "&"
    } else if text.ends_with('?') {
        ""
    } else {
        "?"
    };
    Url::parse(&format!("{text}{sep}__bgda__={token}")).ok()
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/SMPSegmentTokenTests.swift` 의 순수 부분 포팅.
    use super::*;

    const TOKEN: &str = "st=1790000000~exp=1790086400~acl=*/tm/*~hmac=0a1b2c";

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn extracts_raw_hdnts() {
        assert_eq!(
            hdnts_token(&format!("a=1&hdnts={TOKEN}&b=2")).as_deref(),
            Some(TOKEN)
        );
        assert_eq!(hdnts_token("hdnts=abc%2Adef").as_deref(), Some("abc%2Adef"));
        assert!(hdnts_token("hdnts=").is_none());
        assert!(hdnts_token("xhdnts=abc").is_none());
        assert!(hdnts_token("a=1").is_none());
    }

    #[test]
    fn appends_once() {
        assert_eq!(
            appending_bgda_token("T", &u("https://h.navercdn.com/a/seg_1.m4v"))
                .unwrap()
                .as_str(),
            "https://h.navercdn.com/a/seg_1.m4v?__bgda__=T"
        );
        assert_eq!(
            appending_bgda_token("T", &u("https://h.navercdn.com/a/seg_1.m4v?x=1"))
                .unwrap()
                .as_str(),
            "https://h.navercdn.com/a/seg_1.m4v?x=1&__bgda__=T"
        );
        assert!(
            appending_bgda_token("T", &u("https://h.navercdn.com/a/seg_1.m4v?__bgda__=old"))
                .is_none()
        );
    }

    #[test]
    fn rewrites_only_when_enabled() {
        let mut s = SmpSegmentTokenState::default();
        let master = u(&format!(
            "https://h.navercdn.com/tm/master.m3u8?hdnts={TOKEN}"
        ));
        let segment = u("https://h.navercdn.com/tm/1080p/seg_7.m4v");
        let init = u("https://h.navercdn.com/tm/1080p/init.m4s");
        assert_eq!(s.apply(&master), master);
        assert_eq!(s.apply(&segment), segment);

        s = SmpSegmentTokenState::new(true);
        assert_eq!(s.apply(&segment), segment);
        assert_eq!(s.apply(&master), master);
        assert_eq!(
            s.apply(&segment).as_str(),
            format!("{}?__bgda__={TOKEN}", segment.as_str())
        );
        assert_eq!(
            s.apply(&init).as_str(),
            format!("{}?__bgda__={TOKEN}", init.as_str())
        );
        s.apply(&u("https://h.navercdn.com/tm/1080p/chunklist.m3u8"));
        assert!(s
            .apply(&segment)
            .as_str()
            .ends_with(&format!("__bgda__={TOKEN}")));
        let other = u("https://h.navercdn.com/tm/thumb.jpg");
        assert_eq!(s.apply(&other), other);

        s = SmpSegmentTokenState::new(false);
        assert_eq!(s.apply(&segment), segment);
        s = SmpSegmentTokenState::new(true);
        assert_eq!(s.apply(&segment), segment);
    }
}
