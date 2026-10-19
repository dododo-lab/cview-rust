//! `Sources/CViewPlayer/LatencyPresetPairing.swift` + `LatencyPresetPairing+MatchWebV2.swift` 포팅.
//!
//! 레이턴시 프리셋 → (PID Configuration, hls.js 프로파일) 쌍을 **한 곳에서만** 결정한다.
//! 실행 시점의 목표 지연은 Rust 컨트롤러가 단독 소유하고(주입), JS 프로파일은 버퍼/네트워크 그릇을 고른다.

use cview_core::models::settings::{LatencyPreset, PlayerSettings};

use crate::hlsjs::profile::HlsJsStreamingProfile;
use crate::low_latency::LowLatencyConfiguration;
use crate::policy::constants::latency_defaults;

/// custom 슬라이더가 만들 수 있는 최소 목표 지연.
pub const MINIMUM_CUSTOM_TARGET_LATENCY: f64 = 0.5;
/// 목표→seek 문턱 사이에 반드시 남겨야 하는 폭.
pub const MINIMUM_LATENCY_BAND: f64 = 1.0;

/// 프리셋 하나가 확정하는 (컨트롤러 목표, JS 프로파일) 쌍.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyPresetPairing {
    pub configuration: LowLatencyConfiguration,
    pub profile: HlsJsStreamingProfile,
}

impl LatencyPresetPairing {
    /// 설정 → 쌍. `None` 은 "설정 로드 전 시작" 경로(matchWeb 기본 쌍).
    pub fn pairing(settings: Option<&PlayerSettings>) -> Self {
        let preset = settings.and_then(|s| LatencyPreset::from_raw(&s.latency_preset));
        let low_latency_off = settings.is_some_and(|s| !s.low_latency_mode);
        let pick = |p| {
            if low_latency_off {
                HlsJsStreamingProfile::Mirror
            } else {
                p
            }
        };
        match preset {
            Some(LatencyPreset::MatchWeb) => Self {
                configuration: LowLatencyConfiguration::WEB_PARITY_HLSJS,
                profile: pick(HlsJsStreamingProfile::WebParity),
            },
            Some(LatencyPreset::WebSync) => Self {
                configuration: LowLatencyConfiguration::WEB_SYNC,
                profile: HlsJsStreamingProfile::Mirror,
            },
            Some(LatencyPreset::Standard) => Self {
                configuration: LowLatencyConfiguration::STANDARD,
                profile: pick(HlsJsStreamingProfile::LowLatency),
            },
            Some(LatencyPreset::UltraLow) => Self {
                configuration: LowLatencyConfiguration::ULTRA_LOW,
                profile: pick(HlsJsStreamingProfile::UltraLow),
            },
            Some(LatencyPreset::Custom) | None => Self {
                configuration: settings
                    .map(Self::custom_configuration)
                    .unwrap_or(LowLatencyConfiguration::WEB_PARITY_HLSJS),
                profile: pick(if preset.is_none() {
                    HlsJsStreamingProfile::WebParity
                } else {
                    HlsJsStreamingProfile::LowLatency
                }),
            },
        }
    }

    /// 슬라이더 값 → Configuration. 밴드 불변식(target < max)을 여기서 강제한다.
    pub fn custom_configuration(ps: &PlayerSettings) -> LowLatencyConfiguration {
        let raw_target = if ps.latency_target.is_finite() {
            ps.latency_target
        } else {
            3.0
        };
        let target = MINIMUM_CUSTOM_TARGET_LATENCY.max(raw_target);
        let raw_max = if ps.latency_max.is_finite() {
            ps.latency_max
        } else {
            target + MINIMUM_LATENCY_BAND
        };
        let raw_min = if ps.latency_min.is_finite() {
            ps.latency_min
        } else {
            target
        };
        LowLatencyConfiguration {
            target_latency: target,
            max_latency: raw_max.max(target + MINIMUM_LATENCY_BAND),
            min_latency: raw_min.min(target),
            max_playback_rate: ps.latency_max_rate,
            min_playback_rate: ps.latency_min_rate,
            catch_up_threshold: ps.latency_catch_up_threshold,
            slow_down_threshold: ps.latency_slow_down_threshold,
            pid_kp: ps.latency_pid_kp,
            pid_ki: ps.latency_pid_ki,
            pid_kd: ps.latency_pid_kd,
            rate_significance_threshold: latency_defaults::RATE_SIGNIFICANCE_THRESHOLD,
        }
    }

    /// v2 를 이 재생 세션에 적용할 수 있는가.
    pub fn is_match_web_v2_eligible(
        settings: Option<&PlayerSettings>,
        flag_enabled: bool,
        server_time_machine_stream: bool,
        source_eligible: bool,
    ) -> bool {
        let Some(settings) = settings else {
            return false;
        };
        if !flag_enabled || !source_eligible || server_time_machine_stream {
            return false;
        }
        LatencyPreset::from_raw(&settings.latency_preset) == Some(LatencyPreset::MatchWeb)
            && settings.low_latency_mode
    }

    /// 호출자가 이미 판정한 v2 여부를 믿는다.
    pub fn pairing_v2(settings: Option<&PlayerSettings>, match_web_v2: bool) -> Self {
        if match_web_v2 {
            Self {
                configuration: LowLatencyConfiguration::MATCH_WEB_V2_HLSJS,
                profile: HlsJsStreamingProfile::WebParity,
            }
        } else {
            Self::pairing(settings)
        }
    }

    pub fn configuration_for(
        settings: &PlayerSettings,
        match_web_v2: bool,
    ) -> LowLatencyConfiguration {
        Self::pairing_v2(Some(settings), match_web_v2).configuration
    }

    /// v2 가 폴백한 세션이 되돌아갈 v1 설정(= 기본 쌍).
    pub fn match_web_v2_fallback_configuration() -> LowLatencyConfiguration {
        Self::pairing(None).configuration
    }
}

#[cfg(test)]
mod tests {
    //! `LatencyPairingParityTests.swift` · `MatchWebV2PairingTests.swift` 포팅.
    //! HTML 은 `Sources/CViewPlayer/Resources/hlsjs-player.html` 원본을 직접 파싱한다.
    use super::*;
    use cview_core::models::match_web_v2::MatchWebV2Flag;

    const HTML: &str =
        include_str!("../../../../../Sources/CViewPlayer/Resources/hlsjs-player.html");

    fn js_profile_target(key: &str) -> f64 {
        let start = HTML
            .find(&format!("    {key}: {{"))
            .unwrap_or_else(|| panic!("PROFILES.{key} 선언 없음"));
        let rest = &HTML[start..];
        let field = rest.find("liveSyncDuration:").unwrap() + "liveSyncDuration:".len();
        let tail = &rest[field..];
        let end = tail.find(',').unwrap();
        tail[..end].trim().parse().unwrap()
    }

    fn js_rate_significance() -> f64 {
        let marker = "if (Math.abs(video.playbackRate - rate) < ";
        let start = HTML.find(marker).unwrap() + marker.len();
        let tail = &HTML[start..];
        let end = tail.find(')').unwrap();
        tail[..end].parse().unwrap()
    }

    struct Row {
        preset: LatencyPreset,
        swift_target: f64,
        profile_key: &'static str,
        js_fallback: f64,
    }

    fn declared() -> Vec<Row> {
        vec![
            Row {
                preset: LatencyPreset::MatchWeb,
                swift_target: 3.0,
                profile_key: "webParity",
                js_fallback: 3.0,
            },
            Row {
                preset: LatencyPreset::WebSync,
                swift_target: 6.0,
                profile_key: "mirror",
                js_fallback: 6.0,
            },
            Row {
                preset: LatencyPreset::Standard,
                swift_target: 3.0,
                profile_key: "lowLatency",
                js_fallback: 6.0,
            },
            Row {
                preset: LatencyPreset::UltraLow,
                swift_target: 1.5,
                profile_key: "ultraLow",
                js_fallback: 4.0,
            },
            Row {
                preset: LatencyPreset::Custom,
                swift_target: 3.0,
                profile_key: "lowLatency",
                js_fallback: 6.0,
            },
        ]
    }

    fn settings_for(preset: LatencyPreset) -> PlayerSettings {
        let mut ps = PlayerSettings::default();
        ps.apply_latency_preset(preset);
        ps.latency_preset = preset.raw_value().into();
        ps
    }

    #[test]
    fn pairing_matches_declaration() {
        for row in declared() {
            let p = LatencyPresetPairing::pairing(Some(&settings_for(row.preset)));
            assert_eq!(
                p.configuration.target_latency, row.swift_target,
                "{:?}",
                row.preset
            );
            assert_eq!(p.profile.profile_key(), row.profile_key);
            let js = js_profile_target(row.profile_key);
            assert_eq!(js, row.js_fallback);
            assert_eq!(p.profile.target_live_latency_seconds(), js);
        }
    }

    #[test]
    fn low_latency_off_overrides_profile_only() {
        for row in declared() {
            let mut ps = settings_for(row.preset);
            ps.low_latency_mode = false;
            let p = LatencyPresetPairing::pairing(Some(&ps));
            assert_eq!(p.profile.profile_key(), "mirror");
            assert_eq!(p.configuration.target_latency, row.swift_target);
        }
    }

    #[test]
    fn nil_settings_falls_back_to_web_parity() {
        let p = LatencyPresetPairing::pairing(None);
        assert_eq!(p.configuration.target_latency, 3.0);
        assert_eq!(p.profile.profile_key(), "webParity");
    }

    #[test]
    fn preset_value_table_matches_configurations() {
        let expected = [
            (
                LatencyPreset::MatchWeb,
                LowLatencyConfiguration::WEB_PARITY_HLSJS,
            ),
            (LatencyPreset::WebSync, LowLatencyConfiguration::WEB_SYNC),
            (LatencyPreset::Standard, LowLatencyConfiguration::STANDARD),
            (LatencyPreset::UltraLow, LowLatencyConfiguration::ULTRA_LOW),
            (
                LatencyPreset::Custom,
                LowLatencyConfiguration::WEB_PARITY_HLSJS,
            ),
        ];
        for (preset, c) in expected {
            let v = preset.values();
            assert_eq!(v.target, c.target_latency);
            assert_eq!(v.max, c.max_latency);
            assert_eq!(v.min, c.min_latency);
            assert_eq!(v.max_rate, c.max_playback_rate);
            assert_eq!(v.min_rate, c.min_playback_rate);
            assert_eq!(v.catch_up, c.catch_up_threshold);
            assert_eq!(v.slow_down, c.slow_down_threshold);
            assert_eq!(v.kp, c.pid_kp);
            assert_eq!(v.ki, c.pid_ki);
            assert_eq!(v.kd, c.pid_kd);
        }
    }

    #[test]
    fn custom_builder_passes_rate_significance() {
        let js = js_rate_significance();
        let mut ps = PlayerSettings {
            latency_preset: LatencyPreset::Custom.raw_value().into(),
            ..Default::default()
        };
        ps.latency_target = 4.25;
        ps.latency_max_rate = 1.07;
        let c = LatencyPresetPairing::pairing(Some(&ps)).configuration;
        assert_eq!(c.target_latency, 4.25);
        assert_eq!(c.max_playback_rate, 1.07);
        assert_eq!(c.rate_significance_threshold, js);
        assert_eq!(latency_defaults::RATE_SIGNIFICANCE_THRESHOLD, js);
        for row in declared() {
            let c = LatencyPresetPairing::pairing(Some(&settings_for(row.preset))).configuration;
            assert!(c.rate_significance_threshold >= js);
        }
    }

    #[test]
    fn custom_band_invariant() {
        let mut ps = PlayerSettings {
            latency_preset: "custom".into(),
            ..Default::default()
        };
        ps.latency_target = 10.0;
        ps.latency_max = 8.0;
        ps.latency_min = 12.0;
        let c = LatencyPresetPairing::custom_configuration(&ps);
        assert_eq!(c.target_latency, 10.0);
        assert_eq!(c.max_latency, 11.0);
        assert_eq!(c.min_latency, 10.0);
        ps.latency_target = -1.0;
        ps.latency_max = f64::NAN;
        let c = LatencyPresetPairing::custom_configuration(&ps);
        assert_eq!(c.target_latency, 0.5);
        assert_eq!(c.max_latency, 1.5);
    }

    #[test]
    fn js_fallback_structure() {
        for needle in [
            "if (_controllerTarget > 0) return _controllerTarget;",
            "const configured = cfg.liveSyncDuration;",
            "function setControllerTargetLatency(seconds)",
            "function setLatencyOwnedByController(owned)",
            "_latencyOwnedByController",
            "const preLat = hls.latency, preTgt = correctionTargetLatency();",
            "const injected = _controllerTarget;",
            "const targetBuffer = Math.max(3, configuredLiveTargetLatency() || 4);",
        ] {
            assert!(HTML.contains(needle), "{needle}");
        }
    }

    fn v2_settings(preset: LatencyPreset, low_latency: bool) -> PlayerSettings {
        PlayerSettings {
            latency_preset: preset.raw_value().into(),
            low_latency_mode: low_latency,
            ..Default::default()
        }
    }

    #[test]
    fn eligibility_table() {
        for preset in LatencyPreset::ALL {
            for low in [true, false] {
                for flag in [true, false] {
                    let expected = flag && low && preset == LatencyPreset::MatchWeb;
                    let actual = LatencyPresetPairing::is_match_web_v2_eligible(
                        Some(&v2_settings(preset, low)),
                        flag,
                        false,
                        true,
                    );
                    assert_eq!(actual, expected);
                }
            }
        }
        let mw = v2_settings(LatencyPreset::MatchWeb, true);
        assert!(LatencyPresetPairing::is_match_web_v2_eligible(
            Some(&mw),
            true,
            false,
            true
        ));
        assert!(!LatencyPresetPairing::is_match_web_v2_eligible(
            Some(&mw),
            true,
            true,
            true
        ));
        assert!(!LatencyPresetPairing::is_match_web_v2_eligible(
            None, true, false, true
        ));
        assert!(!LatencyPresetPairing::is_match_web_v2_eligible(
            Some(&mw),
            true,
            false,
            MatchWebV2Flag::source_eligible(true, false)
        ));
        assert!(!LatencyPresetPairing::is_match_web_v2_eligible(
            Some(&mw),
            true,
            false,
            MatchWebV2Flag::source_eligible(false, true)
        ));
    }

    #[test]
    fn pairing_for_v2_and_fallback() {
        let mw = v2_settings(LatencyPreset::MatchWeb, true);
        let v2 = LatencyPresetPairing::pairing_v2(Some(&mw), true);
        assert_eq!(v2.configuration.target_latency, 2.0);
        assert_eq!(v2.configuration.pid_kp, 0.0);
        assert_eq!(v2.profile, HlsJsStreamingProfile::WebParity);
        let v1 = LatencyPresetPairing::pairing_v2(Some(&mw), false);
        assert_eq!(v1, LatencyPresetPairing::pairing(Some(&mw)));
        assert_eq!(
            LatencyPresetPairing::configuration_for(&mw, true).target_latency,
            2.0
        );
        let fallback = LatencyPresetPairing::match_web_v2_fallback_configuration();
        assert_eq!(fallback, v1.configuration);
        assert_eq!(fallback, LowLatencyConfiguration::WEB_PARITY_HLSJS);
    }
}
