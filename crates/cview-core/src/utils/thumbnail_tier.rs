//! 치지직 라이브 썸네일의 **서버측 리사이즈 티어** 선택.
//!
//! 원본: `Sources/CViewCore/Networking/ChzzkThumbnailTier.swift`
//!
//! 라이브 썸네일 URL 은 `.../thumbnail/image_{type}.jpg` 형태이고 `{type}` 이 서버가 만들어 주는
//! 이미지의 가로 픽셀이다. 다운로드 크기를 줄이는 레버는 이것 하나뿐이다.
//!
//! ⚠ 티어는 **화이트리스트**다 — 계산식으로 고르면 안 된다(240·288·600·900·1440 은 HTTP 400).
//! 정책: 카드/아바타는 720 상한, 히어로 1장만 1080 예외.

use url::Url;

/// 서버가 허용하는 티어(가로 픽셀). 실측 화이트리스트 — 여기 없는 값은 HTTP 400 이다.
pub const SUPPORTED: [i64; 13] = [
    90, 144, 180, 270, 320, 360, 405, 480, 540, 640, 720, 1080, 1280,
];

/// 디코더가 `{type}` 에 넣는 기본값이자 **카드/아바타 경로의 상한**.
pub const DEFAULT_TIER: i64 = 720;

/// 히어로(화면당 1장) 전용 다운로드 상한.
pub const HERO_CEILING: i64 = 1080;

/// 너무 작게 내려 화질이 눈에 띄게 무너지는 것을 막는 하한.
pub const MINIMUM_TIER: i64 = 320;

/// 요구 픽셀을 만족하는 최소 티어. `ceiling`(기본 [`DEFAULT_TIER`])을 넘지 않는다.
pub fn tier(max_pixel_width: i64, ceiling: i64) -> i64 {
    if max_pixel_width <= 0 {
        return DEFAULT_TIER.min(MINIMUM_TIER.max(ceiling));
    }
    let capped_ceiling = ceiling.min(SUPPORTED[SUPPORTED.len() - 1]);
    let candidate = SUPPORTED
        .iter()
        .copied()
        .find(|&t| t >= max_pixel_width && t <= capped_ceiling)
        .unwrap_or(capped_ceiling);
    // ceiling 이 supported 밖이면 가장 가까운 하한 티어로 내린다.
    let resolved = SUPPORTED
        .iter()
        .rev()
        .copied()
        .find(|&t| t <= candidate)
        .unwrap_or(MINIMUM_TIER);
    MINIMUM_TIER.max(resolved.min(capped_ceiling))
}

/// 기본 상한(720)으로 [`tier`].
pub fn tier_default(max_pixel_width: i64) -> i64 {
    tier(max_pixel_width, DEFAULT_TIER)
}

/// 이미 해석된 썸네일 URL 의 티어를 요구 픽셀에 맞춰 **낮추거나**(기본), 히어로 ceiling
/// 예외에서는 더 높은 지원 티어로 **올린다**.
///
/// 보수적으로 동작한다 — 마지막 경로 성분이 `_<숫자>.<확장자>` 가 아니거나, 그 숫자가
/// 화이트리스트에 없거나, 계산된 티어가 같으면 원본을 그대로 돌려준다.
pub fn retiered(url: &Url, max_pixel_width: i64, ceiling: i64) -> Url {
    if max_pixel_width <= 0 {
        return url.clone();
    }
    let Some(filename) = url.path_segments().and_then(|mut s| s.next_back()) else {
        return url.clone();
    };
    let Some(dot) = filename.rfind('.').filter(|&d| d > 0) else {
        return url.clone();
    };
    let (stem, ext) = filename.split_at(dot);
    let Some(underscore) = stem.rfind('_') else {
        return url.clone();
    };
    let digits = &stem[underscore + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return url.clone();
    }
    let Some(current) = digits.parse::<i64>().ok().filter(|c| SUPPORTED.contains(c)) else {
        return url.clone();
    };

    let desired = tier(max_pixel_width, ceiling);
    if desired == current || (ceiling <= DEFAULT_TIER && desired > current) {
        return url.clone();
    }
    let new_name = format!("{}{desired}{ext}", &stem[..=underscore]);
    let mut out = url.clone();
    if let Ok(mut segments) = out.path_segments_mut() {
        segments.pop().push(&new_name);
    } else {
        return url.clone();
    }
    out
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/ChzzkThumbnailTierTests.swift`
    use super::*;

    fn sample() -> Url {
        Url::parse("https://livecloud-thumb.akamaized.net/chzzk/livecloud/KR/stream/26464698/live/20308496/record/57282396/thumbnail/image_720.jpg").unwrap()
    }

    fn last(url: &Url) -> String {
        url.path_segments()
            .unwrap()
            .next_back()
            .unwrap()
            .to_string()
    }

    #[test]
    fn tier_is_always_supported() {
        for px in (1..=2400).step_by(7) {
            assert!(SUPPORTED.contains(&tier_default(px)), "{px}");
        }
    }

    #[test]
    fn never_exceeds_default_tier() {
        for px in [800, 828, 1000, 1472, 2000, 4000, i64::MAX / 2] {
            assert!(tier_default(px) <= DEFAULT_TIER, "{px}");
        }
    }

    #[test]
    fn hero_ceiling_allows_1080() {
        assert_eq!(tier(1472, HERO_CEILING), 1080);
        assert_eq!(tier(414, HERO_CEILING), 480);
        assert_eq!(tier_default(1472), DEFAULT_TIER);
    }

    #[test]
    fn hero_ceiling_upsizes_url() {
        assert_eq!(
            last(&retiered(&sample(), 1472, HERO_CEILING)),
            "image_1080.jpg"
        );
    }

    #[test]
    fn respects_minimum_tier() {
        for px in [1, 10, 48, 100, 200, 319] {
            assert!(tier_default(px) >= MINIMUM_TIER);
        }
    }

    #[test]
    fn picks_smallest_sufficient_tier() {
        assert_eq!(tier_default(414), 480);
        assert_eq!(tier_default(480), 480);
        assert_eq!(tier_default(481), 540);
        assert_eq!(tier_default(368), 405);
        assert_eq!(tier_default(552), 640);
    }

    #[test]
    fn tier_is_monotonic() {
        let mut previous = 0;
        for px in (1..=1600).step_by(11) {
            let t = tier_default(px);
            assert!(t >= previous);
            previous = t;
        }
    }

    #[test]
    fn retiers_url() {
        let out = retiered(&sample(), 414, DEFAULT_TIER);
        assert_eq!(last(&out), "image_480.jpg");
        assert_eq!(out.host_str(), sample().host_str());
        let dir = |u: &Url| u.as_str().rsplit_once('/').unwrap().0.to_string();
        assert_eq!(dir(&out), dir(&sample()));
    }

    #[test]
    fn does_not_upsize_url() {
        for px in [720, 828, 1472, 4000] {
            assert_eq!(retiered(&sample(), px, DEFAULT_TIER), sample(), "{px}");
        }
    }

    #[test]
    fn leaves_foreign_urls_alone() {
        for s in [
            "https://cdn.example.com/thumb.jpg",
            "https://cdn.example.com/image_999.jpg",
            "https://cdn.example.com/image_abc.jpg",
            "https://cdn.example.com/image_.jpg",
            "https://cdn.example.com/720",
        ] {
            let url = Url::parse(s).unwrap();
            assert_eq!(retiered(&url, 200, DEFAULT_TIER), url, "{s}");
        }
    }

    #[test]
    fn zero_request_is_noop() {
        assert_eq!(retiered(&sample(), 0, DEFAULT_TIER), sample());
        assert_eq!(retiered(&sample(), -5, DEFAULT_TIER), sample());
        assert_eq!(tier_default(0), DEFAULT_TIER);
    }

    #[test]
    fn retiered_url_always_uses_supported_tier() {
        for px in (1..=1200).step_by(13) {
            let out = retiered(&sample(), px, DEFAULT_TIER);
            if out == sample() {
                continue;
            }
            let name = last(&out);
            let stem = name.rsplit_once('.').unwrap().0;
            let t: i64 = stem.rsplit_once('_').unwrap().1.parse().unwrap();
            assert!(SUPPORTED.contains(&t), "{t}");
        }
    }
}
