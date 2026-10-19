//! `Sources/CViewApp/Playback/PlaybackGuardrail.swift` 포팅 — 칩 값 계산만.
//!
//! 원본은 SwiftUI 칩 행(지연·버퍼·화질·프록시)이다. 칩이 보여 주는 문구·경고 판정·툴팁을
//! 순수 함수로 옮겼다. 그리기는 셸(WebView)이 맡는다.

/// 칩 하나의 표시 값.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailChipValue {
    pub icon: &'static str,
    pub label: String,
    /// true 면 경고색.
    pub warning: bool,
    pub tooltip: String,
}

/// 지연 칩 — `current <= target * 1.5` 면 정상.
pub fn latency_chip(info: Option<(f64, f64, f64)>) -> GuardrailChipValue {
    match info {
        Some((current, target, ewma)) => GuardrailChipValue {
            icon: "clock.fill",
            label: format!("{current:.1}s"),
            warning: current > target * 1.5,
            tooltip: format!("현재: {current:.2}s  목표: {target:.2}s  EWMA: {ewma:.2}s"),
        },
        None => GuardrailChipValue {
            icon: "clock.fill",
            label: "--".into(),
            warning: false,
            tooltip: "지연 정보 없음".into(),
        },
    }
}

/// 버퍼 칩 — (현재, 목표, 건강, 신뢰도 0~1).
pub fn buffer_chip(health: Option<(f64, f64, bool, f64)>) -> GuardrailChipValue {
    match health {
        Some((current, target, healthy, confidence)) => GuardrailChipValue {
            icon: "waveform.path",
            label: format!("{current:.1}s"),
            warning: !healthy,
            tooltip: format!(
                "버퍼: {current:.1}s / 목표 {target:.1}s  신뢰도: {}%",
                (confidence * 100.0) as i64
            ),
        },
        None => GuardrailChipValue {
            icon: "waveform.path",
            label: "--".into(),
            warning: false,
            tooltip: "버퍼 정보 없음".into(),
        },
    }
}

/// 화질 칩 — 메뉴 칩이라 접근성 이름에 역할을 함께 싣는다.
pub fn quality_chip(current: Option<&str>) -> (GuardrailChipValue, String) {
    let quality = current.unwrap_or("auto").to_string();
    let tooltip = format!("현재 화질: {quality}");
    let accessibility = format!("화질 선택, {tooltip}");
    (
        GuardrailChipValue {
            icon: "sparkles",
            label: quality,
            warning: false,
            tooltip,
        },
        accessibility,
    )
}

/// 프록시 칩 — `mode` 는 `PlayerViewModel.currentProxyMode`.
pub fn proxy_chip(mode: &str, route_summary: Option<&str>) -> GuardrailChipValue {
    let is_none = mode == "none";
    let label = match mode {
        "pending" => "확인 중",
        "unavailable" => "연결 불가",
        _ if is_none => "직접",
        _ => "프록시",
    };
    GuardrailChipValue {
        icon: "network",
        label: label.into(),
        warning: is_none,
        tooltip: route_summary.unwrap_or("연결 확인 중").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips() {
        let l = latency_chip(Some((3.1, 2.0, 2.5)));
        assert_eq!(l.label, "3.1s");
        assert!(l.warning);
        assert!(!latency_chip(Some((2.9, 2.0, 2.5))).warning);
        assert_eq!(latency_chip(None).label, "--");
        assert_eq!(
            buffer_chip(Some((1.0, 2.0, true, 0.876))).tooltip,
            "버퍼: 1.0s / 목표 2.0s  신뢰도: 87%"
        );
        assert_eq!(proxy_chip("none", None).label, "직접");
        assert!(proxy_chip("none", None).warning);
        assert_eq!(proxy_chip("local", Some("ok")).label, "프록시");
        assert_eq!(proxy_chip("pending", None).label, "확인 중");
        assert_eq!(quality_chip(None).1, "화질 선택, 현재 화질: auto");
    }
}
