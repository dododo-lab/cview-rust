//! `Sources/CViewPlayer/LowLatencyController.swift` 의 `Configuration` (+ `+MatchWebV2.swift` 의
//! `matchWebV2HLSJS`) 포팅.

use crate::policy::constants::latency_defaults;

/// 저지연 PID 설정. 시간 값은 초.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LowLatencyConfiguration {
    pub target_latency: f64,
    pub max_latency: f64,
    pub min_latency: f64,
    pub max_playback_rate: f64,
    pub min_playback_rate: f64,
    pub catch_up_threshold: f64,
    pub slow_down_threshold: f64,
    pub pid_kp: f64,
    pub pid_ki: f64,
    pub pid_kd: f64,
    /// `playbackRate` 재적용 최소 Δ.
    pub rate_significance_threshold: f64,
    /// 치지직 웹처럼 **전방 버퍼 기준**으로 따라잡는 세션인가. 켜면 이 컨트롤러는 배속을 바꾸지 않고
    /// (따라잡기는 플레이어 페이지의 웹 규칙이 맡는다) 시작 스냅·초기 seek 도 하지 않는다 — 웹은 시작한
    /// 자리에서 그대로 재생한다. 목표 지연은 시작·복귀·리싱크 위치로만 쓰고, `max_latency` 를 넘는 큰
    /// 어긋남만 seek 로 되돌린다.
    pub buffer_paced: bool,
}

impl Default for LowLatencyConfiguration {
    /// Swift `Configuration.init()` 의 기본 인자 (`.default` 프리셋과는 다르다).
    fn default() -> Self {
        Self {
            target_latency: 3.0,
            max_latency: 10.0,
            min_latency: 1.0,
            max_playback_rate: 1.15,
            min_playback_rate: 0.9,
            catch_up_threshold: 1.5,
            slow_down_threshold: 0.5,
            pid_kp: 0.8,
            pid_ki: 0.1,
            pid_kd: 0.05,
            rate_significance_threshold: latency_defaults::RATE_SIGNIFICANCE_THRESHOLD,
            buffer_paced: false,
        }
    }
}

impl LowLatencyConfiguration {
    #[allow(clippy::too_many_arguments)]
    const fn preset(
        target: f64,
        max: f64,
        min: f64,
        max_rate: f64,
        min_rate: f64,
        catch_up: f64,
        slow_down: f64,
        kp: f64,
        ki: f64,
        kd: f64,
    ) -> Self {
        Self {
            target_latency: target,
            max_latency: max,
            min_latency: min,
            max_playback_rate: max_rate,
            min_playback_rate: min_rate,
            catch_up_threshold: catch_up,
            slow_down_threshold: slow_down,
            pid_kp: kp,
            pid_ki: ki,
            pid_kd: kd,
            rate_significance_threshold: latency_defaults::RATE_SIGNIFICANCE_THRESHOLD,
            buffer_paced: false,
        }
    }

    /// Swift `.default` (standard 프리셋).
    pub const STANDARD: Self = Self::preset(3.0, 8.0, 1.0, 1.15, 0.90, 1.2, 0.5, 0.8, 0.12, 0.06);

    pub const ULTRA_LOW: Self = Self::preset(1.5, 8.0, 0.5, 1.20, 0.85, 1.0, 0.3, 1.0, 0.15, 0.08);

    /// 웹(hls.js) 동기화 프리셋(완화형, 6.0s).
    pub const WEB_SYNC: Self = Self::preset(6.0, 12.0, 3.0, 1.08, 0.93, 1.0, 0.8, 0.5, 0.05, 0.03);

    /// VLC/AVPlayer 전용 웹 mirror (3.5s).
    pub const MATCH_WEB: Self = Self::preset(3.5, 8.0, 1.5, 1.10, 0.92, 0.8, 0.5, 0.7, 0.10, 0.05);

    /// hls.js 전용 웹 패리티 (3.0s, `PROFILES.webParity.liveSyncDuration` 과 일치).
    pub const WEB_PARITY_HLSJS: Self =
        Self::preset(3.0, 8.0, 1.5, 1.03, 0.97, 0.55, 0.55, 0.35, 0.03, 0.02);

    /// 치지직 웹과 같은 지점 — 기본 프리셋 "웹과 같게". 웹 플레이어(hls.js LL-HLS)의 `liveSyncDuration` 2초를
    /// LL 재생목록 끝 기준 목표로 쓰고, 배속은 바꾸지 않는다(`buffer_paced`). 웹은 전방 버퍼가 4초를 넘으면
    /// 1.03배, 2초 이하로 내려오면 1배로 돌아가며 그 규칙은 `player.html` 의 `webCatchupTick` 이 똑같이 한다.
    /// `max_latency` 12초는 웹의 라이브 구간(20초) 안쪽 안전망 — 그보다 크게 밀리면 목표 자리로 seek 한다.
    pub const WEB_MATCH_HLSJS: Self = Self {
        buffer_paced: true,
        // PID 값은 이 세션에서 쓰지 않는다 — "직접 조정"으로 옮겨 갈 때 슬라이더 시작값이 될 뿐이다.
        ..Self::preset(2.0, 12.0, 1.0, 1.03, 0.97, 0.55, 0.55, 0.35, 0.03, 0.02)
    };

    /// matchWeb v2 — hls.js 가장자리 목표(H_base). PID 는 돌지 않는다.
    pub const MATCH_WEB_V2_HLSJS: Self =
        Self::preset(2.0, 18.0, 1.0, 1.0, 1.0, 0.55, 0.55, 0.0, 0.0, 0.0);
}

#[cfg(test)]
mod tests {
    //! `LowLatencyControllerTests` Configuration 스위트 + `MatchWebV2PairingTests.v2ConfigurationConstants`.
    use super::*;

    #[test]
    fn standard_and_ultra_low() {
        let c = LowLatencyConfiguration::STANDARD;
        assert_eq!(
            (c.target_latency, c.max_latency, c.min_latency),
            (3.0, 8.0, 1.0)
        );
        assert_eq!((c.max_playback_rate, c.min_playback_rate), (1.15, 0.9));
        assert_eq!((c.catch_up_threshold, c.slow_down_threshold), (1.2, 0.5));
        assert_eq!((c.pid_kp, c.pid_ki, c.pid_kd), (0.8, 0.12, 0.06));
        let u = LowLatencyConfiguration::ULTRA_LOW;
        assert_eq!(
            (u.target_latency, u.max_latency, u.min_latency),
            (1.5, 8.0, 0.5)
        );
        assert_eq!((u.max_playback_rate, u.min_playback_rate), (1.2, 0.85));
        assert_eq!((u.pid_kp, u.pid_ki, u.pid_kd), (1.0, 0.15, 0.08));
    }

    #[test]
    fn web_parity_micro_stutter_guards() {
        let c = LowLatencyConfiguration::WEB_PARITY_HLSJS;
        assert_eq!(c.target_latency, 3.0);
        assert_eq!(c.max_playback_rate, 1.03);
        assert_eq!(c.min_playback_rate, 0.97);
        assert_eq!(c.catch_up_threshold, 0.55);
        assert_eq!(c.slow_down_threshold, 0.55);
        assert_eq!(c.rate_significance_threshold, 0.015);
    }

    #[test]
    fn web_match_is_buffer_paced_with_web_target() {
        let presets = [LowLatencyConfiguration::WEB_MATCH_HLSJS];
        let c = presets[0];
        assert!(presets.iter().all(|p| p.buffer_paced));
        assert_eq!(c.target_latency, 2.0, "웹 liveSyncDuration");
        assert_eq!(c.max_latency, 12.0);
        let others = [
            LowLatencyConfiguration::WEB_PARITY_HLSJS,
            LowLatencyConfiguration::STANDARD,
            LowLatencyConfiguration::default(),
        ];
        assert!(others.iter().all(|o| !o.buffer_paced));
    }

    #[test]
    fn match_web_v2_constants() {
        let c = LowLatencyConfiguration::MATCH_WEB_V2_HLSJS;
        assert_eq!(
            (c.target_latency, c.max_latency, c.min_latency),
            (2.0, 18.0, 1.0)
        );
        assert_eq!((c.max_playback_rate, c.min_playback_rate), (1.0, 1.0));
        assert_eq!((c.pid_kp, c.pid_ki, c.pid_kd), (0.0, 0.0, 0.0));
    }

    #[test]
    fn custom_defaults() {
        let c = LowLatencyConfiguration {
            target_latency: 2.0,
            max_latency: 8.0,
            max_playback_rate: 1.3,
            ..Default::default()
        };
        assert_eq!(c.target_latency, 2.0);
        assert_eq!(c.max_playback_rate, 1.3);
        assert_eq!(LowLatencyConfiguration::default().max_latency, 10.0);
    }
}
